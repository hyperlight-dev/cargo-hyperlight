#![no_std]
#![no_main]
extern crate alloc;

use alloc::string::String;
use core::sync::atomic::{AtomicI32, Ordering};

use hyperlight_guest_bin::error::Result;
use hyperlight_guest_bin::{guest_function, host_function};

static COUNTER: AtomicI32 = AtomicI32::new(0);

// Declare a host function that the guest can call. The string is the
// registration name (must match what the host passes to register()).
// If omitted, the Rust function name is used.
// The host must register this before the sandbox is initialized.
#[host_function("GetWeekday")]
fn get_weekday() -> Result<String>;

// Register a guest function that can be called by the host.
#[guest_function("SayHello")]
fn say_hello(name: String) -> Result<String> {
    let weekday = get_weekday()?;
    Ok(alloc::format!("Hello, {name}! Today is {weekday}."))
}

// Guest functions can take multiple arguments of different types.
#[guest_function("Add")]
fn add(a: i32, b: i32) -> Result<i32> {
    Ok(a + b)
}

// Increments a counter and returns the new value. State persists across
// calls until the host restores a snapshot, which resets all VM memory
// back to the state it was in when the snapshot was taken.
#[guest_function("Increment")]
fn increment() -> Result<i32> {
    let old = COUNTER.fetch_add(1, Ordering::Relaxed);
    Ok(old + 1)
}
