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
    std::fs::write(
        &file,
        "[toolchain]\nchannel = 'cargo-hyperlight-missing-toolchain'",
    )
    .unwrap();
    let error = command.toolchain_file(&file).unwrap_err();
    let error = format!("{error:#}");
    assert!(
        error.contains("Failed to install guest toolchain")
            || error.contains("Failed to locate cargo"),
        "{error}"
    );
    assert_eq!(command.get_program(), original);
}

#[cfg(unix)]
#[test]
fn toolchain_provisioning_honors_auto_install() {
    use std::os::unix::fs::PermissionsExt;

    const FILE_ENV: &str = "CARGO_HYPERLIGHT_TEST_TOOLCHAIN_FILE";
    const MISSING_ENV: &str = "CARGO_HYPERLIGHT_TEST_TOOLCHAIN_MISSING";
    const MODE_ENV: &str = "CARGO_HYPERLIGHT_TEST_ENV_MODE";
    if let Some(file) = std::env::var_os(FILE_ENV) {
        let inherited = std::env::var_os("RUSTUP_AUTO_INSTALL");
        let mut command = cargo_hyperlight::cargo().unwrap();
        match std::env::var(MODE_ENV).unwrap().as_str() {
            "inherit" => {}
            "0" => {
                command.env("RUSTUP_AUTO_INSTALL", "0");
            }
            "1" => {
                command.env("RUSTUP_AUTO_INSTALL", "1");
            }
            "remove" => {
                command.env_remove("RUSTUP_AUTO_INSTALL");
            }
            "clear" => {
                command.env_clear();
            }
            "clear_cargo" => {
                command.env_clear_cargo();
            }
            mode => panic!("Unknown test environment mode: {mode}"),
        }
        let original = command.get_program().to_owned();
        let result = command.toolchain_file(file);
        if std::env::var_os(MISSING_ENV).as_deref() == Some(std::ffi::OsStr::new("1")) {
            let error = format!("{:#}", result.unwrap_err());
            assert!(error.contains("Failed to locate cargo"), "{error}");
            assert_eq!(command.get_program(), original);
        } else {
            result.unwrap();
            assert_eq!(command.get_program(), std::env::current_exe().unwrap());
        }
        assert_eq!(std::env::var_os("RUSTUP_AUTO_INSTALL"), inherited);
        return;
    }

    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rust-toolchain.toml");
    let log = dir.path().join("rustup.log");
    let rustup = dir.path().join("rustup");
    std::fs::write(
        &file,
        "[toolchain]\nchannel = 'guest-test'\ncomponents = ['rust-src']\n",
    )
    .unwrap();
    std::fs::write(
        &rustup,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$CARGO_HYPERLIGHT_TEST_RUSTUP_LOG"
    printf 'auto-install=%s\n' "${RUSTUP_AUTO_INSTALL-unset}" >> "$CARGO_HYPERLIGHT_TEST_RUSTUP_LOG"
case "$1" in
    toolchain) exit 0 ;;
    which)
        if [ "$CARGO_HYPERLIGHT_TEST_TOOLCHAIN_MISSING" = 1 ]; then
            exit 1
        fi
        printf '%s\n' "$CARGO_HYPERLIGHT_TEST_BINARY"
        ;;
    *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&rustup, std::fs::Permissions::from_mode(0o755)).unwrap();

    for (auto_install, mode, effective) in [
        (None, "inherit", None),
        (Some("1"), "inherit", Some("1")),
        (Some("0"), "inherit", Some("0")),
        (None, "0", Some("0")),
        (Some("1"), "0", Some("0")),
        (Some("0"), "1", Some("1")),
        (Some("0"), "remove", None),
        (Some("0"), "clear", None),
        (Some("0"), "clear_cargo", Some("0")),
    ] {
        for missing in [false, true] {
            std::fs::write(&log, "").unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap());
            child
                .args([
                    "--exact",
                    "toolchain_provisioning_honors_auto_install",
                    "--nocapture",
                ])
                .env("PATH", dir.path())
                .env("CARGO", env!("CARGO"))
                .env(FILE_ENV, &file)
                .env(MISSING_ENV, if missing { "1" } else { "0" })
                .env(MODE_ENV, mode)
                .env("CARGO_HYPERLIGHT_TEST_RUSTUP_LOG", &log)
                .env(
                    "CARGO_HYPERLIGHT_TEST_BINARY",
                    std::env::current_exe().unwrap(),
                )
                .env_remove("RUSTUP_AUTO_INSTALL");
            if let Some(value) = auto_install {
                child.env("RUSTUP_AUTO_INSTALL", value);
            }
            let output = child.output().unwrap();
            assert!(output.status.success(), "{output:?}");
            let calls = std::fs::read_to_string(&log).unwrap();
            assert_eq!(
                calls.contains("toolchain install guest-test --component rust-src\n"),
                effective != Some("0"),
                "{calls}"
            );
            let expected_env = format!("auto-install={}", effective.unwrap_or("unset"));
            assert!(
                calls
                    .lines()
                    .filter(|line| line.starts_with("auto-install="))
                    .all(|line| line == expected_env),
                "{calls}"
            );
            assert!(
                calls.contains("which --toolchain guest-test cargo\n"),
                "{calls}"
            );
        }
    }
}

/// Run with a newer host: cargo +1.99.1 test --test toolchain_file -- --include-ignored
#[test]
#[ignore = "requires a host compiler newer than 1.94, rustup downloads, and guest build dependencies"]
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
