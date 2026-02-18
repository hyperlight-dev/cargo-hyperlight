use std::ops::Not as _;
use std::path::PathBuf;

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

use crate::cargo_cmd::{CargoCmd, cargo_cmd};
use crate::cli::Args;

const CARGO_TOML: &str = include_str!("dummy/_Cargo.toml");
const LIB_RS: &str = include_str!("dummy/_lib.rs");

#[derive(serde::Deserialize, Default)]
struct CargoBuildMessageTarget {
    name: String,
}

#[derive(serde::Deserialize)]
struct CargoBuildMessage {
    reason: String,
    #[serde(default)]
    target: CargoBuildMessageTarget,
    #[serde(default)]
    filenames: Vec<PathBuf>,
}

pub fn build(args: &Args) -> Result<()> {
    let target_spec = match args.target.as_str() {
        "x86_64-hyperlight-none" => {
            let mut spec = get_spec(args, "x86_64-unknown-none")?;
            // entry_name seems to be ignored, use RUSTFLAGS with -Clink-args=-eentrypoint instead
            //spec.entry_name = Some("entrypoint".into());
            let Value::Object(custom) = json!({
                "code-model": "small",
                "linker": "rust-lld",
                "linker-flavor": "gnu-lld",
                "pre-link-args": {
                    "gnu-lld": ["-znostart-stop-gc"],
                },
                "features": "-mmx,+sse,+sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2,-soft-float"
            }) else {
                unreachable!()
            };
            spec.extend(custom);
            spec.remove("rustc-abi");
            spec
        }
        triplet => bail!(
            "Unsupported target triple: {triplet:?}
Supported values are:
 * x86_64-hyperlight-none"
        ),
    };

    let sysroot_dir = args.sysroot_dir();
    let target_dir = args.build_dir();
    let triplet_dir = args.triplet_dir();
    let crate_dir = args.crate_dir();
    let lib_dir = args.libs_dir();

    std::fs::create_dir_all(&triplet_dir).context("Failed to create sysroot directories")?;
    std::fs::write(
        triplet_dir.join("target.json"),
        serde_json::to_string_pretty(&target_spec).unwrap(),
    )
    .context("Failed to write target spec file")?;

    let version = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("version")
        .arg("--verbose")
        .checked_output()
        .context("Failed to get cargo version")?;

    let version = String::from_utf8_lossy(&version.stdout);
    let version = version
        .lines()
        .find_map(|l| l.trim().strip_prefix("release: "))
        .context("Failed to parse cargo version")?;

    let cargo_toml = CARGO_TOML.replace("0.0.0", version);

    std::fs::create_dir_all(&crate_dir).context("Failed to create target directory")?;
    std::fs::write(crate_dir.join("Cargo.toml"), cargo_toml)
        .context("Failed to write Cargo.toml")?;
    std::fs::write(crate_dir.join("lib.rs"), LIB_RS).context("Failed to write lib.rs")?;

    // if we are using rustup, ensure that the rust-src component is installed
    if let Some(rustup_toolchain) = std::env::var_os("RUSTUP_TOOLCHAIN") {
        std::process::Command::new("rustup")
            .arg("--quiet")
            .arg("component")
            .arg("add")
            .arg("rust-src")
            .arg("--toolchain")
            .arg(rustup_toolchain)
            .checked_output()
            .context("Failed to get Rust's std lib sources")?;
    }

    // Build the sysroot
    let output = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("build")
        .target(&args.target)
        .manifest_path(&Some(crate_dir.join("Cargo.toml")))
        .target_dir(&target_dir)
        .arg("-Zbuild-std=core,alloc")
        .arg("-Zbuild-std-features=compiler_builtins/mem")
        .arg("--release")
        .arg("--message-format=json")
        // The core, alloc and compiler_builtins crates use unstable features
        .allow_unstable()
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .sysroot(&sysroot_dir)
        .output()
        .context("Failed to build sysroot cargo project")?;

    ensure!(
        output.status.success(),
        "Failed to build sysroot\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = String::from_utf8_lossy(&output.stdout);
    let mut artifacts = Vec::new();

    for message in messages.lines() {
        let message = serde_json::from_str::<CargoBuildMessage>(message)
            .context("Failed to parse sysroot build message")?;
        if message.reason == "compiler-artifact" {
            let name = message.target.name;
            if name == "core" || name == "alloc" || name == "compiler_builtins" {
                artifacts.extend(message.filenames);
            }
        }
    }

    std::fs::create_dir_all(&lib_dir).context("Failed to create sysroot lib directory")?;

    // Find any old artifacts in the sysroot lib directory
    let to_remove = lib_dir
        .read_dir()
        .context("Failed to read sysroot lib directory")?
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            let filename = path.file_name()?;
            artifacts
                .iter()
                .any(|file| file.file_name() == Some(filename))
                .not()
                .then_some(path)
        });

    // Remove old artifacts
    for artifact in to_remove {
        std::fs::remove_file(artifact).context("Failed to remove old sysroot artifact")?;
    }

    // Copy new artifacts
    for artifact in artifacts {
        let filename = artifact.file_name().unwrap();
        let dst = lib_dir.join(filename);
        if !dst.exists() {
            std::fs::copy(&artifact, dst).context("Failed to copy sysroot artifact")?;
        }
    }

    Ok(())
}

fn get_spec(args: &Args, triplet: impl AsRef<str>) -> Result<Map<String, Value>> {
    let output = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("rustc")
        .target(triplet)
        .manifest_path(&args.manifest_path)
        .arg("-Zunstable-options")
        .arg("--print=target-spec-json")
        .arg("--")
        .arg("-Zunstable-options")
        // printing target-spec-json is an unstable feature
        .allow_unstable()
        .checked_output()
        .context("Failed to get base target spec")?;

    serde_json::from_slice(&output.stdout).context("Failed to parse target spec JSON")
}
