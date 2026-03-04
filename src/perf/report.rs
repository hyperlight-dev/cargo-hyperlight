//! `cargo hyperlight perf report` — Display a profile report.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Context, Result, bail};
use clap::Parser;
use regex::Regex;

use super::parse_hex_or_dec;

/// Display a profile report from previously recorded perf data.
///
/// Generates a kallsyms file from the guest ELF binary and runs
/// `perf kvm report` to show demangled symbols with overhead.
#[derive(Parser, Debug)]
pub struct ReportArgs {
    /// Path to the guest ELF binary.
    pub guest_binary: PathBuf,

    /// Input perf.data path (default: ./perf.data.guest, or ./perf.data.kvm with --host).
    #[arg(short, long)]
    pub input: Option<PathBuf>,

    /// Include host kernel and userspace samples alongside guest samples.
    ///
    /// This must match the mode used during recording. If `--host` was
    /// used with `perf record`, use it here too — otherwise host samples
    /// will be missing from the report.
    #[arg(long)]
    pub host: bool,

    /// Group report by guest, kernel, and userspace (requires --host).
    #[arg(long, requires = "host")]
    pub group: bool,

    /// Guest load base address (hex with 0x prefix or decimal).
    #[arg(long, default_value = "0x1000", value_parser = parse_hex_or_dec)]
    pub base_address: u64,
}

/// Run `cargo hyperlight perf report`.
pub fn run(args: ReportArgs) -> Result<()> {
    let kallsyms_file = super::prepare_kallsyms(&args.guest_binary, args.base_address)?;

    report_perf(&args, kallsyms_file.path())?;

    Ok(())
}

/// Run `perf kvm report` and format the output.
fn report_perf(args: &ReportArgs, kallsyms: &std::path::Path) -> Result<()> {
    if args.host {
        eprintln!("Host + guest profile:");
        if !args.group {
            eprintln!("  [g] = guest VM  [k] = host kernel  [.] = host userspace");
        }
    } else {
        eprintln!("Guest profile:");
    }

    let mut perf_args = super::perf_kvm_args(args.host, kallsyms);
    perf_args.extend(["report".into(), "--stdio".into(), "--no-children".into()]);
    if let Some(input) = &args.input {
        perf_args.push("-i".into());
        perf_args.push(input.as_os_str().to_owned());
    }
    perf_args.extend(["-F".into(), "overhead,sym".into()]);

    let mut child = Command::new("perf")
        .args(&perf_args)
        .stderr(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to execute perf report")?;

    let stdout = child.stdout.take().expect("stdout piped");

    // Drain stderr in a background thread to prevent pipe deadlock:
    // if perf fills the stderr pipe buffer while we're reading stdout,
    // it would block and we'd hang waiting for stdout EOF.
    let mut stderr = child.stderr.take().expect("stderr piped");
    let stderr_handle = thread::spawn(move || {
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut stderr, &mut buf).ok();
        buf
    });

    let events = collect_report_events(stdout)?;

    // Compile the demangling regex once, outside the per-line loop.
    let demangle_re = Regex::new(r"_(?:R[A-Za-z0-9_]+|ZN[A-Za-z0-9_]+)").unwrap();

    for event in &events {
        if events.len() > 1 {
            eprintln!("\n  {} ({} samples):", event.name, event.sample_count);
        }
        // Demangle any Rust symbols perf didn't handle (host-side v0 mangling).
        let lines: Vec<String> = event
            .lines
            .iter()
            .map(|l| demangle_line(l, &demangle_re))
            .collect();
        if args.group {
            print_grouped(&lines);
        } else if args.host {
            for line in &lines {
                println!("{}", line.trim_end());
            }
        } else {
            for line in &lines {
                println!("{}", line.replace("[g] ", "").trim_end());
            }
        }
    }

    let stderr_output = stderr_handle.join().unwrap_or_default();
    let status = child.wait().context("Failed to wait for perf report")?;
    if !status.success() {
        let stderr_msg = stderr_output.trim();

        if !stderr_msg.is_empty() {
            eprintln!("perf report stderr: {stderr_msg}");
        }

        // Hint about --host mismatch (common mistake).
        if !args.host {
            eprintln!(
                "Hint: if the data was recorded with --host, you must also pass --host to report."
            );
        }

        bail!("perf report exited with status {status}");
    }

    Ok(())
}

/// Demangle any Rust-mangled symbols in a perf report line.
///
/// Guest symbols are already demangled in the kallsyms we generate,
/// but host symbols come straight from perf which may not understand
/// Rust's v0 mangling scheme (`_R...`). We find mangled names and
/// replace them with their demangled form.
fn demangle_line(line: &str, re: &Regex) -> String {
    re.replace_all(line, |caps: &regex::Captures| {
        let mangled = &caps[0];
        let demangled = rustc_demangle::demangle(mangled);
        // Use alternate format `{:#}` which omits the hash suffix.
        format!("{demangled:#}")
    })
    .into_owned()
}

/// A single perf event's report data (e.g. `cpu_core/cycles/`).
struct ReportEvent {
    /// Event name from the `# Samples: N of event '<name>'` header.
    name: String,
    /// Number of samples in this event.
    sample_count: String,
    /// Report lines in perf's original order (sorted by overhead descending).
    lines: Vec<String>,
}

/// Read perf report stdout, splitting into per-event groups.
///
/// On hybrid CPUs (e.g. Raptor Lake with P-cores and E-cores), `-e cycles`
/// creates separate PMU events (`cpu_core/cycles/` and `cpu_atom/cycles/`).
/// Each event gets its own independently-sorted section in the report.
/// We split them into separate [`ReportEvent`] groups so the caller can
/// display them individually, letting the user see which core type
/// produced the samples.
///
/// On non-hybrid CPUs there is only one event (`cycles`), so the caller
/// gets a single group — no user-visible difference.
fn collect_report_events(stdout: impl std::io::Read) -> Result<Vec<ReportEvent>> {
    let mut events: Vec<ReportEvent> = Vec::new();

    let mut current_name = String::from("cycles");
    let mut current_samples = String::new();
    let mut current_lines: Vec<String> = Vec::new();

    for line in BufReader::new(stdout).lines() {
        let line = line.context("Failed to read perf output")?;

        // Detect event section headers:
        //   # Samples: 35K of event 'cpu_core/cycles/'
        if let Some(rest) = line.strip_prefix("# Samples:") {
            // Flush the previous event if it had data.
            if !current_lines.is_empty() {
                events.push(ReportEvent {
                    name: current_name.clone(),
                    sample_count: current_samples.clone(),
                    lines: std::mem::take(&mut current_lines),
                });
            }
            // Parse: "35K  of event 'cpu_core/cycles/'"
            let rest = rest.trim();
            if let Some(of_pos) = rest.find(" of event '") {
                current_samples = rest[..of_pos].trim().to_string();
                let event_start = of_pos + " of event '".len();
                current_name = rest[event_start..].trim_end_matches('\'').to_string();
            } else {
                current_samples.clear();
                current_name = "unknown".to_string();
            }
            continue;
        }

        if line.starts_with('#') || line.is_empty() {
            continue;
        }

        current_lines.push(line);
    }

    // Flush the last event.
    if !current_lines.is_empty() {
        events.push(ReportEvent {
            name: current_name,
            sample_count: current_samples,
            lines: current_lines,
        });
    }

    Ok(events)
}

/// Print report lines grouped by guest, kernel, and userspace.
fn print_grouped(lines: &[String]) {
    let (mut guest, mut kernel, mut user) = (Vec::new(), Vec::new(), Vec::new());
    for line in lines {
        if line.contains("[g]") {
            guest.push(line);
        } else if line.contains("[k]") {
            kernel.push(line);
        } else {
            user.push(line);
        }
    }

    for (header, group) in [
        ("Guest VM", &guest),
        ("Host kernel", &kernel),
        ("Host userspace", &user),
    ] {
        if group.is_empty() {
            continue;
        }
        println!("\n  {header}:");
        for line in group {
            println!("{}", line.trim_end());
        }
    }
}
