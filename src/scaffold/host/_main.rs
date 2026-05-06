use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use hyperlight_host::{GuestBinary, MultiUseSandbox, UninitializedSandbox};

fn main() -> hyperlight_host::Result<()> {
    // TODO: support aarch64-hyperlight-none when aarch64 guests are supported.
    let base = PathBuf::from("../guest/target/x86_64-hyperlight-none");
    let guest_path = ["debug", "release"]
        .iter()
        .map(|p| base.join(p).join("{guest_name}"))
        .find(|p| p.exists())
        .expect(
            "guest binary not found - build it first with: cd ../guest && cargo hyperlight build",
        );

    // Create a sandbox from the guest binary. It starts uninitialized so you
    // can register host functions before the guest begins executing.
    let mut sandbox = UninitializedSandbox::new(
        GuestBinary::FilePath(guest_path.display().to_string()),
        None,
    )?;

    // Register a host function that the guest can call.
    sandbox.register("GetWeekday", weekday)?;

    // Evolve into a MultiUseSandbox, which lets you call guest functions
    // multiple times.
    let mut sandbox: MultiUseSandbox = sandbox.evolve()?;

    // Call a guest function with a single argument.
    let result: String = sandbox.call("SayHello", "World".to_string())?;
    println!("{result}");

    // Multiple arguments are passed as a tuple.
    let sum: i32 = sandbox.call("Add", (2_i32, 3_i32))?;
    println!("2 + 3 = {sum}");

    // Guest state persists between calls. Take a snapshot so we can
    // restore back to this point later.
    let snapshot = sandbox.snapshot()?;

    let count: i32 = sandbox.call("Increment", ())?;
    println!("count = {count}"); // 1
    let count: i32 = sandbox.call("Increment", ())?;
    println!("count = {count}"); // 2
    let count: i32 = sandbox.call("Increment", ())?;
    println!("count = {count}"); // 3

    // Restore resets all guest memory back to the snapshot.
    sandbox.restore(snapshot)?;

    let count: i32 = sandbox.call("Increment", ())?;
    println!("count after restore = {count}"); // 1 again

    Ok(())
}

// Returns the current day of the week as a String.
fn weekday() -> hyperlight_host::Result<String> {
    let days = [
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
    ];
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();
    // January 1, 1970 was a Thursday (day index 3 when Monday = 0).
    Ok(days[((secs / (60 * 60 * 24) + 3) % 7) as usize].to_string())
}
