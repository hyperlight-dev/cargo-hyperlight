use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use clap::Parser;

const HYPERLIGHT_VERSION: &str = "0.13";

const GUEST_CARGO_TOML: &str = include_str!("guest/_Cargo.toml");
const GUEST_MAIN_RS: &str = include_str!("guest/_main.rs");
const HOST_CARGO_TOML: &str = include_str!("host/_Cargo.toml");
const HOST_MAIN_RS: &str = include_str!("host/_main.rs");
const GITIGNORE: &str = include_str!("_gitignore");

/// Scaffold a new Hyperlight project.
#[derive(Parser, Debug)]
#[command(name = "scaffold")]
struct ScaffoldCli {
    /// Path to create the project at. The directory name is used as the crate
    /// name (like `cargo new`).
    path: PathBuf,

    /// Generate only a guest project instead of both host and guest.
    #[arg(long, default_value_t = false)]
    guest_only: bool,
}

pub fn run(args: impl Iterator<Item = OsString>) -> Result<()> {
    let cli = ScaffoldCli::parse_from(args);

    let name = cli
        .path
        .file_name()
        .context("Invalid project path")?
        .to_str()
        .context("Project name must be valid UTF-8")?;

    validate_name(name)?;
    ensure!(
        !cli.path.exists(),
        "Directory '{}' already exists",
        cli.path.display()
    );

    if cli.guest_only {
        write_guest(&cli.path, name)?;
    } else {
        let guest_name = format!("{name}-guest");
        write_guest(&cli.path.join("guest"), &guest_name)?;
        write_host(&cli.path.join("host"), &format!("{name}-host"), &guest_name)?;
    }
    write_file(cli.path.join(".gitignore"), GITIGNORE)?;

    let dir = cli.path.display();
    println!("Created project at '{dir}'\n");
    if cli.guest_only {
        println!("Build:");
        println!("  cd {dir} && cargo hyperlight build");
    } else {
        println!("Build and run:");
        println!("  cd {dir}/guest && cargo hyperlight build");
        println!("  cd {dir}/host && cargo run");
    }

    Ok(())
}

fn write_guest(dir: &Path, name: &str) -> Result<()> {
    let cargo_toml = GUEST_CARGO_TOML
        .replace("{name}", name)
        .replace("{version}", HYPERLIGHT_VERSION);
    write_file(dir.join("Cargo.toml"), &cargo_toml)?;
    write_file(dir.join("src/main.rs"), GUEST_MAIN_RS)?;
    Ok(())
}

fn write_host(dir: &Path, name: &str, guest_name: &str) -> Result<()> {
    let cargo_toml = HOST_CARGO_TOML
        .replace("{name}", name)
        .replace("{version}", HYPERLIGHT_VERSION);
    let main_rs = HOST_MAIN_RS
        .replace("{name}", name)
        .replace("{guest_name}", guest_name);
    write_file(dir.join("Cargo.toml"), &cargo_toml)?;
    write_file(dir.join("src/main.rs"), &main_rs)?;
    Ok(())
}

fn write_file(path: impl AsRef<Path>, content: &str) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory '{}'", parent.display()))?;
    }
    fs::write(path, content).with_context(|| format!("Failed to write '{}'", path.display()))?;
    Ok(())
}

/// Validate that the name is usable as a Cargo package name.
/// Mirrors the essential checks from `cargo new`.
fn validate_name(name: &str) -> Result<()> {
    ensure!(!name.is_empty(), "project name must not be empty");
    ensure!(
        name.chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_'),
        "invalid project name `{name}`: must contain only letters, numbers, `-`, or `_`"
    );
    ensure!(
        name.chars()
            .next()
            .is_some_and(|c| c.is_alphabetic() || c == '_'),
        "invalid project name `{name}`: must start with a letter or `_`"
    );
    let reserved = [
        "test",
        "core",
        "std",
        "alloc",
        "proc_macro",
        "proc-macro",
        "self",
        "Self",
        "crate",
        "super",
    ];
    ensure!(
        !reserved.contains(&name),
        "invalid project name `{name}`: it conflicts with a Rust built-in name"
    );
    Ok(())
}
