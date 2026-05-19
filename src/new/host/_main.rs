use hyperlight_host::{GuestBinary, MultiUseSandbox, UninitializedSandbox};

fn main() -> hyperlight_host::Result<()> {
    // Create a sandbox from the guest binary. It starts uninitialized so you
    // can register host functions before the guest begins executing.
    let mut sandbox = UninitializedSandbox::new(
        GuestBinary::FilePath("../guest/target/{arch}-hyperlight-none/debug/{guest_name}".into()),
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

fn weekday() -> hyperlight_host::Result<String> {
    // It's always Monday here, sorry Garfield!
    Ok("Monday".to_string())
}
