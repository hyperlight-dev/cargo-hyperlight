use std::ffi::{OsStr, OsString};
use std::{env, iter};

use anyhow::{Result, anyhow};
use cargo_hyperlight::{Args, cargo, toolchain, toolchain_flags};

mod new;
mod perf;
mod util;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_HASH");
const GIT_DATE: &str = env!("GIT_DATE");

enum FlagKind {
    C,
    Ld,
    Libs,
}
impl FlagKind {
    fn parse(x: &OsStr) -> Option<Self> {
        use FlagKind::*;
        match x.to_str()? {
            "cflags" => Some(C),
            "ldflags" => Some(Ld),
            "libs" => Some(Libs),
            _ => None,
        }
    }
    fn get_flags(&self, args: &Args) -> toolchain_flags::Flags {
        use FlagKind::*;
        match self {
            C => toolchain::cflags(args, false),
            Ld => toolchain::ldflags(args),
            Libs => toolchain::libs(args),
        }
    }
}
enum SpecialCommand {
    Version,
    Perf,
    New,
    BuildCSysroot,
    Flags(FlagKind),
}
impl SpecialCommand {
    fn parse(x: &OsStr) -> Option<Self> {
        use SpecialCommand::*;
        match x.to_str()? {
            "--version" | "-V" => Some(Version),
            "perf" => Some(Perf),
            "new" => Some(New),
            "build-c-sysroot" => Some(BuildCSysroot),
            _ => FlagKind::parse(x).map(Flags),
        }
    }
    fn execute(self, args: impl Iterator<Item = OsString>) -> Result<()> {
        use SpecialCommand::*;
        match self {
            Version => {
                println!("cargo-hyperlight {} ({} {})", VERSION, GIT_HASH, GIT_DATE);
            }
            Perf => perf::run(args)?,
            New => new::run(args)?,
            BuildCSysroot => {
                let mut built_args = cargo()?
                    // a C sysroot needs the C API library
                    .arg("--with-guest-capi")
                    .args(args)
                    .build_args();
                let sysroot_dir = built_args
                    .c_sysroot_dir
                    .clone()
                    .ok_or(anyhow!("Usage: cargo-hyperlight build-c-sysroot [opts] --c-sysroot-dir </path/to/sysroot>"))?;
                built_args.prepare_sysroot()?;
                util::union_glob(
                    iter::once(&built_args.wrapper_dir()),
                    &sysroot_dir.join("bin"),
                    "**/*",
                )?;
                util::union_glob(
                    iter::once(&built_args.includes_dir()),
                    &sysroot_dir.join("include"),
                    "**/*.h",
                )?;
                util::union_glob_with_predicate(
                    iter::once(&built_args.c_libs_dir()),
                    &sysroot_dir.join("lib"),
                    "**/*",
                    |x| !x.starts_with("rustlib"),
                )?;
            }
            Flags(k) => {
                let mut built_args = cargo()?.args(args).build_args();
                built_args.prepare_sysroot()?;
                let flags = k.get_flags(&built_args).joined();
                println!("{}", flags.to_str().ok_or(anyhow!("flags were not UTF-8"))?);
            }
        }
        Ok(())
    }
}

fn run_cargo(args: impl Iterator<Item = OsString>) -> Result<()> {
    cargo()?.args(args).status()
}

fn main() {
    // Skip binary name; when invoked as `cargo hyperlight`, cargo passes
    // "hyperlight" as argv[1] — skip that too.
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|a| a == "hyperlight") {
        args.next();
    }

    let result = match args.peek().and_then(|x| SpecialCommand::parse(x)) {
        Some(sc) => sc.execute(args),
        None => run_cargo(args),
    };

    if let Err(e) = result {
        eprintln!("{e:?}");
        std::process::exit(1);
    }
}
