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

Your binary will be built for the `x86_64-hyperlight-none` target (or `aarch64-hyperlight-none` on ARM) by default, and placed in `target/<arch>-hyperlight-none/release/guest`.

There's no need for any extra configuration, the command will take care of everything.

## Building C Guests

cargo-hyperlight also supports building C guest binaries. To get the compiler and linker flags needed to build a C guest, use:

```sh
cargo hyperlight cflags
cargo hyperlight ldflags
cargo hyperlight libs
```

For example, to compile and link a C guest:
```sh
clang $(cargo hyperlight cflags) $(cargo hyperlight ldflags) -o guest main.c $(cargo hyperlight libs)
```

### Building a C Sysroot

To produce a self-contained, redistributable C sysroot (including headers, libraries, a `hyperlight-config` utility, and a clang wrapper), use:

```sh
cargo hyperlight build-c-sysroot --c-sysroot-dir /path/to/sysroot
```

This copies the following into the specified directory:
- `bin/` — a `hyperlight-config` executable and a `clang` wrapper
- `include/` — header files for the guest C API
- `lib/` — static libraries needed to link a C guest

The produced `hyperlight-config` executable provides the same `--cflags`, `--ldflags`, and `--libs` flags, and the `clang` wrapper automatically injects the correct flags when invoked. This allows downstream consumers to build C guests without installing `cargo-hyperlight` themselves.

## Releasing

To publish a new version:

1. Update the version in `Cargo.toml`
2. Commit the change: `git commit -S -s -am  "Bump version to X.Y.Z"` and open a PR
3. Create and push a tag: `git tag -s vX.Y.Z && git push origin vX.Y.Z`

The CI will automatically run tests, publish to crates.io, and create a GitHub release.
