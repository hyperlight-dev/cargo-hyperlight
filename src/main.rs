use std::env;

use cargo_hyperlight::cargo;

mod perf;
mod scaffold;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const GIT_HASH: &str = env!("GIT_HASH");
const GIT_DATE: &str = env!("GIT_DATE");

fn main() {
    // Skip binary name; when invoked as `cargo hyperlight`, cargo passes
    // "hyperlight" as argv[1] — skip that too.
    let mut args = env::args_os().skip(1).peekable();
    if args.peek().is_some_and(|a| a == "hyperlight") {
        args.next();
    }

    match args.peek().map(|a| a.to_os_string()) {
        Some(a) if a == "--version" || a == "-V" => {
            println!("cargo-hyperlight {} ({} {})", VERSION, GIT_HASH, GIT_DATE);
        }
        Some(a) if a == "perf" => {
            if let Err(e) = perf::run(args) {
                eprintln!("{e:?}");
                std::process::exit(1);
            }
        }
        Some(a) if a == "scaffold" => {
            if let Err(e) = scaffold::run(args) {
                eprintln!("{e:?}");
                std::process::exit(1);
            }
        }
        _ => {
            cargo()
                .expect("Failed to create cargo command")
                .args(args)
                .status()
                .expect("Failed to execute cargo");
        }
    }
}
