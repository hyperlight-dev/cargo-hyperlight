export CARGO_TARGET_DIR := justfile_dir() / "target"

install:
    cargo install cargo-hyperlight --path .

fmt:
    cargo +nightly fmt --all -- --check
    cargo +nightly fmt --all --manifest-path ./examples/host/Cargo.toml -- --check
    cargo +nightly fmt --all --manifest-path ./examples/guest/Cargo.toml -- --check
    # These are standalone template files not part of any crate, so cargo fmt wont find them.
    rustfmt +nightly --check ./src/new/guest/_main.rs ./src/new/host/_main.rs

fmt-apply:
    cargo +nightly fmt --all
    cargo +nightly fmt --all --manifest-path ./examples/host/Cargo.toml
    cargo +nightly fmt --all --manifest-path ./examples/guest/Cargo.toml
    # These are standalone template files not part of any crate, so cargo fmt wont find them.
    rustfmt +nightly ./src/new/guest/_main.rs ./src/new/host/_main.rs

clippy:
    cargo clippy --all -- -D warnings
    cargo clippy --all --manifest-path ./examples/host/Cargo.toml -- -D warnings
    cargo hyperlight clippy --all --manifest-path ./examples/guest/Cargo.toml -- -D warnings

build-guest:
    cargo hyperlight build --manifest-path ./examples/guest/Cargo.toml

run-guest: build-guest
    cargo run --manifest-path ./examples/host/Cargo.toml -- ./target/{{arch()}}-hyperlight-none/debug/guest

test-new:
    cargo test --test new

test-clang-parser:
    cargo test --test clang_parser

test: test-new test-clang-parser

build-c-sysroot:
    cargo hyperlight build-c-sysroot --manifest-path examples/c/fetch-capi/Cargo.toml --c-sysroot-dir examples/c/sysroot

build-c-guest: build-c-sysroot
    examples/c/sysroot/bin/clang examples/c/guest/main.c -o examples/c/guest/guest -lhyperlight_guest_capi -fuse-ld=lld

run-c-guest: build-c-guest
    cargo run --manifest-path ./examples/host/Cargo.toml -- ./examples/c/guest/guest
