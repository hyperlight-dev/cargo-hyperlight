use std::process::Command;

/// Invokes cargo-hyperlight from the current workspace via `cargo run`.
fn cargo_hyperlight() -> Command {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.args(["run", "--quiet", "--"]);
    cmd
}

/// Cargo command for generated projects. Removes CARGO_TARGET_DIR so each
/// project uses its own target/, matching how users actually run the commands.
fn cargo() -> Command {
    let mut cmd = Command::new(env!("CARGO"));
    cmd.env_remove("CARGO_TARGET_DIR");
    cmd
}

fn run(cmd: &mut Command) -> String {
    let output = cmd.output().expect("failed to execute command");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        panic!(
            "command failed: {:?}\nstdout: {stdout}\nstderr: {stderr}",
            cmd.get_program()
        );
    }
    String::from_utf8(output.stdout).expect("non-utf8 output")
}

#[test]
fn new_host_and_guest() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("myproject");

    run(cargo_hyperlight().arg("new").arg(&project));

    assert!(project.join("guest/Cargo.toml").exists());
    assert!(project.join("guest/src/main.rs").exists());
    assert!(project.join("host/Cargo.toml").exists());
    assert!(project.join("host/src/main.rs").exists());
    assert!(project.join(".gitignore").exists());

    let guest_toml = std::fs::read_to_string(project.join("guest/Cargo.toml")).unwrap();
    assert!(guest_toml.contains("name = \"myproject-guest\""));
    let host_toml = std::fs::read_to_string(project.join("host/Cargo.toml")).unwrap();
    assert!(host_toml.contains("name = \"myproject-host\""));

    // Clippy
    run(cargo()
        .args(["hyperlight", "clippy", "--all", "--manifest-path"])
        .arg(project.join("guest/Cargo.toml"))
        .args(["--", "-D", "warnings"]));
    run(cargo()
        .args(["clippy", "--all", "--manifest-path"])
        .arg(project.join("host/Cargo.toml"))
        .args(["--", "-D", "warnings"]));

    // Build
    run(cargo()
        .args(["hyperlight", "build", "--manifest-path"])
        .arg(project.join("guest/Cargo.toml")));
    run(cargo()
        .args(["build", "--manifest-path"])
        .arg(project.join("host/Cargo.toml")));

    // Run and check output
    let output = run(cargo()
        .args(["run", "--quiet", "--manifest-path"])
        .arg(project.join("host/Cargo.toml"))
        .current_dir(project.join("host")));

    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(lines[0], "Hello, World! Today is Monday.");
    assert_eq!(lines[1], "2 + 3 = 5");
    assert_eq!(lines[2], "count = 1");
    assert_eq!(lines[3], "count = 2");
    assert_eq!(lines[4], "count = 3");
    assert_eq!(lines[5], "count after restore = 1");
    assert_eq!(lines.len(), 6);
}

#[test]
fn new_guest_only() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("myguest");

    run(cargo_hyperlight().arg("new").arg("--no-host").arg(&project));

    assert!(project.join("Cargo.toml").exists());
    assert!(project.join("src/main.rs").exists());
    assert!(project.join(".gitignore").exists());
    assert!(!project.join("host").exists());

    let toml = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(toml.contains("name = \"myguest\""));

    // Clippy
    run(cargo()
        .args(["hyperlight", "clippy", "--all", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--", "-D", "warnings"]));

    // Build
    run(cargo()
        .args(["hyperlight", "build", "--manifest-path"])
        .arg(project.join("Cargo.toml")));
}

#[test]
fn new_host_only() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("myhost");

    run(cargo_hyperlight()
        .arg("new")
        .arg("--no-guest")
        .arg(&project));

    assert!(project.join("Cargo.toml").exists());
    assert!(project.join("src/main.rs").exists());
    assert!(project.join(".gitignore").exists());
    assert!(!project.join("guest").exists());

    let toml = std::fs::read_to_string(project.join("Cargo.toml")).unwrap();
    assert!(toml.contains("name = \"myhost\""));

    // Clippy and build (running it would fail since there's no guest binary).
    run(cargo()
        .args(["clippy", "--all", "--manifest-path"])
        .arg(project.join("Cargo.toml"))
        .args(["--", "-D", "warnings"]));
    run(cargo()
        .args(["build", "--manifest-path"])
        .arg(project.join("Cargo.toml")));
}

#[test]
fn new_refuses_existing_directory() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("exists");
    std::fs::create_dir(&project).unwrap();

    let output = cargo_hyperlight()
        .arg("new")
        .arg(&project)
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn new_rejects_no_host_and_no_guest_together() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("both");

    let output = cargo_hyperlight()
        .arg("new")
        .arg("--no-host")
        .arg("--no-guest")
        .arg(&project)
        .output()
        .unwrap();
    assert!(!output.status.success());
}
