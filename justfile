export CARGO_TARGET_DIR := justfile_dir() / "target"

install:
    cargo install cargo-hyperlight --path .

fmt:
    cargo +nightly fmt --all -- --check
    cargo +nightly fmt --all --manifest-path ./examples/host/Cargo.toml -- --check
    cargo +nightly fmt --all --manifest-path ./examples/guest/Cargo.toml -- --check

fmt-apply:
    cargo +nightly fmt --all
    cargo +nightly fmt --all --manifest-path ./examples/host/Cargo.toml
    cargo +nightly fmt --all --manifest-path ./examples/guest/Cargo.toml

clippy:
    cargo clippy --all -- -D warnings
    cargo clippy --all --manifest-path ./examples/host/Cargo.toml -- -D warnings
    cargo hyperlight clippy --all --manifest-path ./examples/guest/Cargo.toml -- -D warnings

build-guest:
    cargo hyperlight build --manifest-path ./examples/guest/Cargo.toml

run-guest: build-guest
    cargo run --manifest-path ./examples/host/Cargo.toml -- ./target/{{arch()}}-hyperlight-none/debug/guest