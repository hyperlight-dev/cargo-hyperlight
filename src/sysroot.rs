use std::ops::Not as _;
use std::path::PathBuf;
use std::process::{Output, Stdio};

use anyhow::{Context, Result, bail, ensure};
use serde_json::{Map, Value, json};

use crate::cargo_cmd::{CargoCmd, cargo_cmd, get_env_value, rustup_auto_install_enabled};
use crate::cli::Args;

const CARGO_TOML: &str = include_str!("dummy/_Cargo.toml");
const LIB_RS: &str = include_str!("dummy/_lib.rs");

/// Flag that gates loading a custom target specification. Custom targets
/// became unstable in Rust 1.95, so newer toolchains only accept them on the
/// nightly channel and only when this flag is passed.
pub(crate) const UNSTABLE_TARGET_SPEC_FLAG: &str = "-Zunstable-options";

fn rust_src_install_command(args: &Args) -> Option<std::process::Command> {
    let toolchain = get_env_value(&args.env, "RUSTUP_TOOLCHAIN")?;
    if !rustup_auto_install_enabled(get_env_value(&args.env, "RUSTUP_AUTO_INSTALL")) {
        return None;
    }
    let mut command = std::process::Command::new("rustup");
    command
        .env_clear()
        .envs(&args.env)
        .args(["--quiet", "component", "add", "rust-src", "--toolchain"])
        .arg(toolchain);
    Some(command)
}

/// Queries the version of the cargo release in use.
fn cargo_version(args: &Args) -> Result<semver::Version> {
    let output = cargo_cmd(&args.env)?
        .current_dir(&args.current_dir)
        .arg("version")
        .arg("--verbose")
        .checked_output()
        .context("Failed to get cargo version")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let release = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("release: "))
        .map(str::trim)
        .context("Failed to parse cargo version")?;

    semver::Version::parse(release)
        .with_context(|| format!("Failed to parse cargo version {release:?}"))
}

/// Asks rustc to print the configuration of the hyperlight target, which forces
/// it to load the custom target specification from the sysroot.
fn probe_target_spec(args: &Args, extra_flags: &[&str]) -> Result<Output> {
    let rustc = match get_env_value(&args.env, "RUSTC") {
        Some(rustc) => PathBuf::from(rustc),
        None => which::which("rustc").context("Failed to find rustc")?,
    };

    std::process::Command::new(rustc)
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("--sysroot")
        .arg(args.sysroot_dir())
        .arg("--target")
        .arg(&args.target)
        .args(extra_flags)
        .arg("--print=cfg")
        .stdin(Stdio::null())
        .output()
        .context("Failed to run rustc")
}

/// Checks that the toolchain in use can load the custom target specification
/// describing the hyperlight guest target, and records whether doing so
/// requires [`UNSTABLE_TARGET_SPEC_FLAG`].
///
/// The target specification must already have been written to the sysroot.
fn check_target_spec_support(args: &mut Args) -> Result<()> {
    let output = probe_target_spec(args, &[])?;
    if output.status.success() {
        args.unstable_target_spec = false;
        return Ok(());
    }

    if probe_target_spec(args, &[UNSTABLE_TARGET_SPEC_FLAG])?
        .status
        .success()
    {
        args.unstable_target_spec = true;
        return Ok(());
    }

    let target = &args.target;
    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "This toolchain cannot build for {target}.

The hyperlight guest targets are described by a custom target specification, \
which rustc rejected:

{}

Custom target specifications were made unstable in Rust 1.95, so they are only \
available on earlier releases or on nightly. For example, pin the toolchain for \
your project by adding a `rust-toolchain.toml` file next to your `Cargo.toml`:

    [toolchain]
    channel = \"1.94.0\"

or select the toolchain for a single invocation with rustup:

    cargo +1.94.0 hyperlight build
    cargo +nightly hyperlight build",
        stderr.trim_end()
    );
}

#[derive(serde::Deserialize, Default, Debug)]
pub(crate) struct CargoBuildMessageTarget {
    pub(crate) name: String,
}

#[derive(serde::Deserialize, Debug)]
pub(crate) struct CargoBuildMessage {
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) target: CargoBuildMessageTarget,
    #[serde(default)]
    pub(crate) filenames: Vec<PathBuf>,
}

pub fn build(args: &mut Args) -> Result<()> {
    let cargo_version = cargo_version(args)?;

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
        "aarch64-hyperlight-none" => {
            let mut spec = get_spec(args, "aarch64-unknown-none")?;
            let Value::Object(custom) = json!({
                "code-model": "small",
                "linker": "rust-lld",
                "linker-flavor": "gnu-lld",
                "pre-link-args": {
                    "gnu-lld": ["-znostart-stop-gc"],
                },
                "relocation-model": "pic",
                "direct-access-external-data": true,
                "position-independent-executables": true,
                "features": "+v8.1a,+strict-align,+neon,+fp-armv8"
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
 * x86_64-hyperlight-none
 * aarch64-hyperlight-none"
        ),
    };

    let sysroot_dir = args.sysroot_dir();
    let target_dir = args.build_dir();
    let triplet_dir = args.triplet_dir();
    let crate_dir = args.crate_dir();
    let lib_dir = args.rust_libs_dir();

    std::fs::create_dir_all(&triplet_dir).context("Failed to create sysroot directories")?;
    std::fs::write(
        triplet_dir.join("target.json"),
        serde_json::to_string_pretty(&target_spec).unwrap(),
    )
    .context("Failed to write target spec file")?;

    check_target_spec_support(args)?;

    let cargo_toml = CARGO_TOML.replace("0.0.0", &cargo_version.to_string());

    std::fs::create_dir_all(&crate_dir).context("Failed to create target directory")?;
    std::fs::write(crate_dir.join("Cargo.toml"), cargo_toml)
        .context("Failed to write Cargo.toml")?;
    std::fs::write(crate_dir.join("lib.rs"), LIB_RS).context("Failed to write lib.rs")?;

    // Install rust-src only when rustup provisioning is enabled.
    if let Some(mut command) = rust_src_install_command(args) {
        command
            .checked_output()
            .context("Failed to get Rust's std lib sources")?;
    }

    // Build the sysroot
    let output = cargo_cmd(&args.env)?
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
        .append_rustflags(if args.unstable_target_spec {
            UNSTABLE_TARGET_SPEC_FLAG
        } else {
            ""
        })
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
    let output = cargo_cmd(&args.env)?
        .current_dir(&args.current_dir)
        .arg("rustc")
        .arg("--target")
        .arg(triplet.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn rust_src_install_failure_does_not_print_inherited_credentials() {
        use std::os::unix::fs::PermissionsExt;

        const KEY: &str = "CARGO_REGISTRIES_HYPERLIGHT_TEST_TOKEN";
        const VALUE: &str = "dummy-inherited-credential-for-install-failure";
        if std::env::var_os(KEY).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sysroot::tests::rust_src_install_failure_does_not_print_inherited_credentials",
                ])
                .env(KEY, VALUE)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }

        let dir = tempfile::tempdir().unwrap();
        let rustup = dir.path().join("rustup");
        std::fs::write(
            &rustup,
            "#!/bin/sh\necho 'dummy rust-src install failure' >&2\nexit 42\n",
        )
        .unwrap();
        std::fs::set_permissions(&rustup, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut command = crate::cargo().unwrap();
        command
            .env("PATH", dir.path())
            .env("RUSTUP_TOOLCHAIN", "guest-toolchain")
            .env("RUSTUP_AUTO_INSTALL", "1");
        let args = command.build_args();
        assert_eq!(
            get_env_value(&args.env, KEY),
            Some(std::ffi::OsStr::new(VALUE))
        );
        let error = rust_src_install_command(&args)
            .unwrap()
            .checked_output()
            .err()
            .unwrap();
        let diagnostic = format!("{error:#}");
        for expected in [
            "rustup",
            "rust-src",
            "guest-toolchain",
            "code 42",
            "dummy rust-src install failure",
        ] {
            assert!(diagnostic.contains(expected), "{diagnostic}");
        }
        assert!(!diagnostic.contains(KEY), "{diagnostic}");
        assert!(!diagnostic.contains(VALUE), "{diagnostic}");
    }

    #[cfg(windows)]
    #[test]
    fn rust_src_toolchain_lookup_is_case_insensitive() {
        let mut command = crate::cargo().unwrap();
        command
            .env("rustup_toolchain", "guest-toolchain")
            .env("rustup_auto_install", "1");
        let install = rust_src_install_command(&command.build_args()).unwrap();
        assert_eq!(
            install.get_args().last(),
            Some(std::ffi::OsStr::new("guest-toolchain"))
        );
    }

    #[test]
    fn rust_src_provisioning_uses_guest_environment() {
        let mut command = crate::cargo().unwrap();
        command
            .env_clear()
            .env("RUSTUP_TOOLCHAIN", "guest-toolchain")
            .env("RUSTUP_AUTO_INSTALL", "0");
        assert!(rust_src_install_command(&command.build_args()).is_none());

        for value in [Some("1"), None] {
            if let Some(value) = value {
                command.env("RUSTUP_AUTO_INSTALL", value);
            } else {
                command.env_remove("RUSTUP_AUTO_INSTALL");
            }
            let install = rust_src_install_command(&command.build_args()).unwrap();
            assert_eq!(install.get_program(), "rustup");
            assert_eq!(
                install.get_args().collect::<Vec<_>>(),
                [
                    "--quiet",
                    "component",
                    "add",
                    "rust-src",
                    "--toolchain",
                    "guest-toolchain"
                ]
            );
        }

        command.env_remove("RUSTUP_TOOLCHAIN");
        assert!(rust_src_install_command(&command.build_args()).is_none());
    }
}
