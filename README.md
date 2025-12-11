# cargo-hyperlight

A cargo subcommand to build [hyperlight](https://github.com/hyperlight-dev/hyperlight) guest binaries.

Write a hyperlight guest binary in Rust, and build it with a simple
```sh
cargo hyperlight build
```

And there's no need for any extra configuration.

Your binary, or any of its dependencies, can have a `build.rs` script using `cc` and `bindgen` to compile C code and generate bindings.
They will work out of the box!

> [!NOTE]
> Your crate **must** have `hyperlight-guest-bin` as a transitive dependency.

## Installation

```sh
cargo install cargo-hyperlight
```

## Usage

Create a new crate for your hyperlight guest binary:

In your `Cargo.toml`
```toml
[package]
name = "guest"
version = "0.1.0"
edition = "2024"

[dependencies]
hyperlight-common = { version = "0.11.0", default-features = false }
hyperlight-guest = "0.11.0"
hyperlight-guest-bin = "0.11.0"
```

The in your `src/main.rs`
```rust
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec::Vec;

use hyperlight_common::flatbuffer_wrappers::{function_call::*, function_types::*, util::*};
use hyperlight_guest::error::Result;
use hyperlight_guest_bin::guest_function::{definition::*, register::*};
use hyperlight_guest_bin::host_comm::*;

pub fn hello_world(_: &FunctionCall) -> Result<Vec<u8>> {
    call_host_function::<i32>(
        "HostPrint",
        Some([ParameterValue::String("hello world".into())].into()),
        ReturnType::Int,
    )?;
    Ok(get_flatbuffer_result(()))
}

#[unsafe(no_mangle)]
pub extern "C" fn hyperlight_main() {
    register_function(GuestFunctionDefinition::new(
        "HelloWorld".into(),
        [ParameterType::String].into(),
        ReturnType::Void,
        hello_world as usize,
    ));
}

#[unsafe(no_mangle)]
pub fn guest_dispatch_function(_: FunctionCall) -> Result<Vec<u8>> {
    panic!("Invalid guest function call");
}
```

Then to build the hyperlight guest binary, run

```sh
cargo hyperlight build --release
```

Your binary will be built for the `x86_64-hyperlight-none` target by default, and placed in `target/x86_64-hyperlight-none/release/guest`.

There's no need for any extra configuration, the command will take care of everything.

## Releasing

To publish a new version:

1. Update the version in `Cargo.toml`
2. Commit the change: `git commit -S -s -am  "Bump version to X.Y.Z"` and open a PR
3. Create and push a tag: `git tag -s vX.Y.Z && git push origin vX.Y.Z`

The CI will automatically run tests, publish to crates.io, and create a GitHub release.
