use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use proc_macro2::TokenStream;
use quote::{TokenStreamExt, quote};
use regex::Regex;

use crate::cargo_cmd::{CargoCmd, cargo_cmd};
use crate::cli::Args;
use crate::sysroot::CargoBuildMessage;
use crate::{toolchain_flags, util};

#[derive(serde::Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoMetadataPackage>,
}

#[derive(serde::Deserialize)]
struct CargoMetadataPackage {
    name: String,
    manifest_path: PathBuf,
    #[allow(dead_code)]
    // we can use this if we ever change the include paths to be copied
    version: semver::Version,
}

struct PackageDirectories {
    hyperlight_libc: Option<PathBuf>,
    hyperlight_guest_bin: Option<PathBuf>,
    hyperlight_guest_capi: Option<PathBuf>,
}
impl PackageDirectories {
    fn libc(&self) -> Result<PathBuf> {
        self.hyperlight_libc
            .as_ref()
            .or(self.hyperlight_guest_bin.as_ref())
            .cloned()
            .context(
                "Could not find hyperlight-libc or hyperlight-guest-bin package in cargo metadata",
            )
    }
    fn guest_capi(&self) -> Result<PathBuf> {
        self.hyperlight_guest_capi
            .clone()
            .context("Could not find hyperlight-guest-capi package in cargo metadata")
    }
}

fn find_package_dir(metadata: &CargoMetadata, name: &str) -> Result<Option<PathBuf>> {
    metadata
        .packages
        .iter()
        .find(|x| x.name == name)
        .map(|pkg| {
            pkg.manifest_path
                .parent()
                .with_context(|| format!("Failed to get directory for {name}"))
                .map(|x| x.to_path_buf())
        })
        .transpose()
}

fn find_package_dirs(args: &Args) -> Result<PackageDirectories> {
    let metadata = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("metadata")
        .manifest_path(&args.manifest_path)
        .arg("--format-version=1")
        .append_rustflags("--cfg=hyperlight")
        .append_rustflags("--check-cfg=cfg(hyperlight)")
        .checked_output()
        .context("Failed to get cargo metadata")?;

    let metadata = serde_json::from_slice::<CargoMetadata>(&metadata.stdout)
        .context("Failed to parse cargo metadata")?;

    Ok(PackageDirectories {
        hyperlight_libc: find_package_dir(&metadata, "hyperlight-libc")?,
        hyperlight_guest_bin: find_package_dir(&metadata, "hyperlight-guest-bin")?,
        hyperlight_guest_capi: if args.with_guest_capi {
            find_package_dir(&metadata, "hyperlight_guest_capi")?
        } else {
            None
        },
    })
}

fn copy_includes(src_dirs: impl Iterator<Item: AsRef<Path>>, dst_dir: &Path) -> Result<()> {
    util::union_glob(src_dirs, dst_dir, "**/*.h")
}

fn build_guest_capi(args: &Args, capi_dir: &Path) -> Result<()> {
    use crate::CargoCommandExt;
    let output = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .arg("build")
        .manifest_path(&Some(capi_dir.join("Cargo.toml")))
        .target_dir(args.build_dir())
        .arg("--message-format=json")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .populate_from_args(args, true)
        .output()
        .context("Failed to build capi cargo project")?;
    ensure!(
        output.status.success(),
        "Failed to build capi\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = String::from_utf8_lossy(&output.stdout);

    for message in messages.lines() {
        let message = serde_json::from_str::<CargoBuildMessage>(message)
            .context("Failed to parse sysroot build message")?;
        if message.reason == "compiler-artifact" {
            let name = message.target.name;
            if name == "hyperlight_guest_capi" {
                for file in message.filenames {
                    let file_name = file.file_name().with_context(|| {
                        format!(
                            "Failed to get filename for capi build artifact {}",
                            file.display()
                        )
                    })?;
                    let dst = args.c_libs_dir().join(file_name);
                    std::fs::copy(&file, &dst)?;
                }
            }
        }
    }

    Ok(())
}

fn path_to_tokens(p: &Path) -> TokenStream {
    let mut tokens = quote! {
        let mut x: ::std::path::PathBuf = ::std::path::PathBuf::new();
    };
    for x in p.iter() {
        let s = x.to_string_lossy();
        tokens.append_all(quote! {
            x.push(#s);
        });
    }
    tokens.append_all(quote! { x });
    quote! { { #tokens } }
}

fn build_wrappers(args: &Args) -> Result<()> {
    const CARGO_TOML: &str = include_str!("wrapper/_Cargo.toml");
    const MAIN_RS: &str = include_str!("wrapper/_main.rs");
    const CLANG_PARSER_RS: &str = include_str!("wrapper/_clang_parser.rs");
    const FLAGS_RS: &str = include_str!("toolchain_flags.rs");

    let wrapper_src_dir = args.wrapper_src_dir();
    std::fs::create_dir_all(&wrapper_src_dir)
        .context("Failed to create wrapper source directory")?;
    std::fs::write(wrapper_src_dir.join("Cargo.toml"), CARGO_TOML)?;
    let wrapper_src_src_dir = wrapper_src_dir.join("src");
    std::fs::create_dir_all(&wrapper_src_src_dir)
        .context("Failed to create wrapper source src directory")?;
    std::fs::write(wrapper_src_src_dir.join("main.rs"), MAIN_RS)?;
    std::fs::write(wrapper_src_src_dir.join("clang_parser.rs"), CLANG_PARSER_RS)?;
    std::fs::write(wrapper_src_src_dir.join("toolchain_flags.rs"), FLAGS_RS)?;
    let includes_toks = path_to_tokens(args.includes_dir().strip_prefix(args.sysroot_dir())?);
    let c_libs_toks = path_to_tokens(args.c_libs_dir().strip_prefix(args.sysroot_dir())?);
    let wrapper_toks = path_to_tokens(args.wrapper_dir().strip_prefix(args.sysroot_dir())?);
    let target = &args.target;
    let with_guest_capi = args.with_guest_capi;
    std::fs::write(
        wrapper_src_src_dir.join("args.rs"),
        (quote! {
            pub(crate) fn args(root: &std::path::Path) -> crate::toolchain_flags::Args {
                crate::toolchain_flags::Args {
                    includes_dir: root.join(#includes_toks),
                    c_libs_dir: root.join(#c_libs_toks),
                    wrapper_dir: root.join(#wrapper_toks),
                    target: #target.to_string(),
                    with_guest_capi: #with_guest_capi,
                }
            }
        })
        .to_string(),
    )?;

    let output = cargo_cmd()?
        .env_clear()
        .envs(args.env.iter())
        .current_dir(&args.current_dir)
        .arg("build")
        .target(&args.host)
        .manifest_path(&Some(wrapper_src_dir.join("Cargo.toml")))
        .target_dir(args.build_dir())
        .arg("--release")
        .arg("--message-format=json")
        .env_remove("RUSTC_WORKSPACE_WRAPPER")
        .output()
        .context("Failed to build wrapper cargo project")?;
    ensure!(
        output.status.success(),
        "Failed to build wrapper\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let messages = String::from_utf8_lossy(&output.stdout);

    for message in messages.lines() {
        let message = serde_json::from_str::<CargoBuildMessage>(message)
            .context("Failed to parse wrapper build message")?;
        if message.reason == "compiler-artifact" {
            let name = message.target.name;
            if name == "hyperlight-sysroot-wrappers" {
                let files: Vec<_> = message
                    .filenames
                    .iter()
                    .filter(|x| x.extension() != Some("pdb".as_ref()))
                    .collect();
                ensure!(
                    files.len() == 1,
                    "hyperlight-sysroot-wrappers produced wrong number of binaries",
                );
                let target_uses_exe = message.filenames[0].extension() == Some("exe".as_ref());
                let dir = args.wrapper_dir();
                let bin_name = |n| {
                    let mut p = dir.join(n);
                    if target_uses_exe {
                        p.set_extension("exe");
                    }
                    p
                };
                std::fs::create_dir_all(&dir).context("Failed to create wrapper bin directory")?;
                std::fs::copy(&message.filenames[0], bin_name("hyperlight-config"))?;
                std::fs::copy(&message.filenames[0], bin_name("clang"))?;
                std::fs::copy(
                    &message.filenames[0],
                    bin_name(&format!("{}-clang", target)),
                )?;
            }
        }
    }
    Ok(())
}

pub fn prepare(args: &Args) -> Result<()> {
    let package_dirs = find_package_dirs(args)?;
    let libc_dir = package_dirs.libc()?;

    let include_dst_dir = args.includes_dir();

    std::fs::create_dir_all(&include_dst_dir)
        .context("Failed to create sysroot include directory")?;

    // Detect which libc variant is present: picolibc or legacy musl
    let mut include_dirs: Vec<&str> = vec![
        // directories for musl
        "third_party/printf/",
        "third_party/musl/include",
        "third_party/musl/arch/generic",
        "third_party/musl/src/internal",
        // directories for picolibc
        "third_party/picolibc/libc/include",
        "third_party/picolibc/libc/stdio",
        "include",
    ];
    if !args.target.starts_with("aarch64") {
        include_dirs.push("third_party/musl/arch/x86_64");
    }

    let capi_dir = args
        .with_guest_capi
        .then(|| {
            let d = package_dirs.guest_capi()?;
            build_guest_capi(args, &d)?;
            Ok::<_, anyhow::Error>(d)
        })
        .transpose()?;

    let include_dirs = include_dirs
        .into_iter()
        .map(|dir| libc_dir.join(dir))
        .chain(capi_dir.map(|x| x.join("include")));
    copy_includes(include_dirs, &include_dst_dir)?;

    build_wrappers(args)?;

    Ok(())
}

impl From<&Args> for toolchain_flags::Args {
    fn from(args: &Args) -> toolchain_flags::Args {
        toolchain_flags::Args {
            includes_dir: args.includes_dir(),
            c_libs_dir: args.c_libs_dir(),
            wrapper_dir: args.wrapper_dir(),
            target: args.target.clone(),
            with_guest_capi: args.with_guest_capi,
        }
    }
}

pub fn cflags(args: &Args, bootstrap: bool) -> toolchain_flags::Flags {
    toolchain_flags::cflags(&args.into(), bootstrap)
}
pub fn ldflags(args: &Args) -> toolchain_flags::Flags {
    toolchain_flags::ldflags(&args.into())
}
pub fn libs(args: &Args) -> toolchain_flags::Flags {
    toolchain_flags::libs(&args.into())
}

pub fn find_cc() -> Result<PathBuf> {
    if let Ok(path) = which::which("clang") {
        return Ok(path);
    }
    // try with postfixed version clang, e.g., clang-20
    let re = Regex::new(r"clang-\d+").unwrap();
    which::which_re(&re)
        .context("Could not find 'clang' in PATH")?
        .next()
        .context("Could not find 'clang' in PATH")
}

pub fn find_ar() -> Result<PathBuf> {
    #[cfg(not(target_os = "macos"))]
    let ar = which::which("ar");
    let llvm_ar = which::which("llvm-ar");
    // The system archiver on macOS can't deal with ELFs, so check
    // `llvm-ar` first there (but stillfall back to `ar` when it's not
    // available, since the correct LLVM ar is named `ar` in some
    // environments, like when building in Nix);
    #[cfg(target_os = "macos")]
    let preferred_ar = llvm_ar.or(ar);
    #[cfg(not(target_os = "macos"))]
    let preferred_ar = ar.or(llvm_ar);
    if let Ok(ar) = preferred_ar {
        return Ok(ar);
    }

    // try with postfixed version llvm-ar, e.g., llvm-ar-20
    let re = Regex::new(r"llvm-ar-\d+").unwrap();
    which::which_re(&re)
        .context("Could not find 'ar' or 'llvm-ar' in PATH")?
        .next()
        .context("Could not find 'ar' or 'llvm-ar' in PATH")
}
