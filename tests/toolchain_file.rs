use std::path::Path;
use std::process::Command;

#[test]
fn invalid_selection_preserves_default_program() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rust-toolchain.toml");
    let mut command = cargo_hyperlight::cargo().unwrap();
    let original = command.get_program().to_owned();

    let error = command.toolchain_file(&file).unwrap_err();
    assert!(format!("{error:#}").contains("Failed to read toolchain file"));
    assert_eq!(command.get_program(), original);

    std::fs::write(&file, "[toolchain]\nchannel = 123").unwrap();
    let error = command.toolchain_file(&file).unwrap_err();
    assert!(format!("{error:#}").contains("Invalid toolchain file"));
    assert_eq!(command.get_program(), original);
}

#[test]
fn unavailable_toolchain_does_not_fall_back() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rust-toolchain.toml");
    let mut command = cargo_hyperlight::cargo().unwrap();
    let original = command.get_program().to_owned();
    for channel in ["cargo-hyperlight-missing-toolchain", "1.94.999"] {
        std::fs::write(&file, format!("[toolchain]\nchannel = '{channel}'")).unwrap();
        let error = command.toolchain_file(&file).unwrap_err();
        let error = format!("{error:#}");
        assert!(error.contains("must already be installed"), "{error}");
        assert!(!error.contains("syncing channel updates"), "{error}");
        assert_eq!(command.get_program(), original);
    }
}

/// Run with a newer host: cargo +1.99.1 test --test toolchain_file -- --include-ignored
#[test]
#[ignore = "requires a host compiler newer than 1.94, Rust 1.94.1 with rust-src, and guest build dependencies"]
fn newer_host_builds_guest_with_older_toolchain() {
    let host = Command::new(env!("CARGO"))
        .args(["--version"])
        .output()
        .unwrap();
    assert!(host.status.success());
    let version = String::from_utf8(host.stdout).unwrap();
    let version = semver::Version::parse(version.split_whitespace().nth(1).unwrap()).unwrap();
    assert!(version >= semver::Version::new(1, 95, 0), "{version}");

    let dir = tempfile::tempdir().unwrap();
    let guest = dir.path().join("guest");
    std::fs::create_dir_all(guest.join("src")).unwrap();
    let example = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("guest");
    std::fs::copy(example.join("Cargo.toml"), guest.join("Cargo.toml")).unwrap();
    for entry in std::fs::read_dir(example.join("src")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), guest.join("src").join(entry.file_name())).unwrap();
    }
    let build_script = include_str!("../examples/guest/build.rs")
        .replace("fn main() {", "fn main() {\n    verify_toolchain();");
    std::fs::write(
        guest.join("build.rs"),
        [
            build_script.as_str(),
            r#"
fn verify_toolchain() {
    for key in ["RUSTC", "CARGO"] {
        let output = std::process::Command::new(std::env::var_os(key).unwrap())
            .arg("--version").output().unwrap();
        assert!(output.status.success());
        let version = String::from_utf8(output.stdout).unwrap();
        assert!(version.contains("1.94.1"), "{key}: {version}");
    }
}
"#,
        ]
        .concat(),
    )
    .unwrap();
    std::fs::write(
        guest.join("rust-toolchain.toml"),
        "[toolchain]\nchannel = '1.94.1'\ncomponents = ['rust-src']\n",
    )
    .unwrap();

    let mut command = cargo_hyperlight::cargo().unwrap();
    command
        .current_dir(&guest)
        .toolchain_file(guest.join("rust-toolchain.toml"))
        .unwrap()
        .env_clear_cargo()
        .env("CARGO_TARGET_DIR", dir.path().join("target"))
        .env("CARGO", env!("CARGO"))
        .env("RUSTC", "nonexistent-host-rustc")
        .env("RUSTDOC", "nonexistent-host-rustdoc")
        .env("RUSTUP_TOOLCHAIN", "nonexistent-host-toolchain")
        .arg("build");
    let args = command.build_args();
    let output = Command::new(command.get_program())
        .arg("--version")
        .output()
        .unwrap();
    assert!(String::from_utf8(output.stdout).unwrap().contains("1.94.1"));
    command.status().unwrap();
    assert!(
        args.target_dir
            .join(&args.target)
            .join("debug")
            .join("guest")
            .is_file()
    );
    assert!(
        args.wrapper_dir()
            .join(format!("clang{}", std::env::consts::EXE_SUFFIX))
            .is_file()
    );

    let mut cleared = command.clone();
    cleared.env_clear();
    let args = cleared.build_args();
    assert_eq!(
        args.env
            .get(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"))
            .unwrap(),
        "1.94.1"
    );
}
