use anyhow::Context as _;
use hyperlight_host::GuestBinary;
use hyperlight_host::sandbox::SandboxConfiguration;

fn main() -> anyhow::Result<()> {
    let guest = std::env::args()
        .nth(1)
        .context("Guest binary path not provided")?;
    let guest = GuestBinary::FilePath(guest);

    let mut config = SandboxConfiguration::default();
    config.set_heap_size(1024 * 1024); // 1 MiB
    config.set_scratch_size(1024 * 1024); // 1 MiB

    // create the sandbox
    let mut sbox = hyperlight_host::UninitializedSandbox::new(guest, Some(config))?.evolve()?;

    // call a guest function
    let n: i32 = sbox.call("SayHello", "World".to_string())?;

    println!("The guest printed {n} bytes");

    Ok(())
}
