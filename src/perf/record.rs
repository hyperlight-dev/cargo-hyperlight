//! `cargo hyperlight perf record` — Record CPU cycle samples.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::Parser;

#[cfg(target_os = "linux")]
use super::which;
use super::{DEFAULT_BASE_ADDRESS, parse_hex_or_dec};

/// Record CPU cycle samples inside Hyperlight micro-VMs.
///
/// Generates a kallsyms file from the guest ELF binary and runs
/// `perf kvm record` scoped to the given workload process tree.
/// Data is saved to `perf.data.guest` by default, or `perf.data.kvm`
/// with `--host` (override with `-o`).
///
/// Use `cargo hyperlight perf report` to view the profile afterwards.
#[derive(Parser, Debug)]
pub struct RecordArgs {
    /// Path to the guest ELF binary.
    pub guest_binary: PathBuf,

    /// Command to run as the profiling workload.
    #[arg(last = true, required = true, num_args = 1..)]
    pub workload: Vec<OsString>,

    /// Sampling frequency in Hz (default: perf's own default, typically ~4 kHz).
    #[arg(long)]
    pub freq: Option<u32>,

    /// Output perf.data path.
    #[arg(
        short,
        long,
        default_value = "perf.data.guest",
        default_value_if("host", "true", "perf.data.kvm")
    )]
    pub output: PathBuf,

    /// Include host kernel and userspace samples alongside guest samples.
    #[arg(long)]
    pub host: bool,

    /// Guest load base address (hex with 0x prefix or decimal).
    #[arg(long, default_value = "0x1000", value_parser = parse_hex_or_dec)]
    pub base_address: u64,
}

/// Run `cargo hyperlight perf record`.
pub fn run(args: RecordArgs) -> Result<()> {
    check_prerequisites()?;

    let kallsyms_file = super::prepare_kallsyms(&args.guest_binary, args.base_address)?;

    let mode_label = if args.host {
        "host+guest"
    } else {
        "guest-only"
    };
    if let Some(freq) = args.freq {
        eprintln!(
            "Recording {mode_label} cycles @ {freq} Hz -> {}",
            args.output.display()
        );
    } else {
        eprintln!("Recording {mode_label} cycles -> {}", args.output.display());
    }
    eprintln!(
        "Workload: {}",
        args.workload
            .iter()
            .map(|a| a.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    );
    eprintln!();

    record_perf(&args, &args.output, kallsyms_file.path())?;

    eprintln!();
    eprintln!("Data saved to {}", args.output.display());

    // Build the exact report command for the user to copy-paste.
    let mut report_cmd = format!(
        "cargo hyperlight perf report {}",
        args.guest_binary.display()
    );
    report_cmd.push_str(&format!(" -i {}", args.output.display()));
    if args.host {
        report_cmd.push_str(" --host");
    }
    if args.base_address != DEFAULT_BASE_ADDRESS {
        report_cmd.push_str(&format!(" --base-address {:#x}", args.base_address));
    }
    eprintln!("To view the profile, run:\n  {report_cmd}");

    Ok(())
}

/// Run `perf kvm record` scoped to the workload process tree.
fn record_perf(args: &RecordArgs, output: &Path, kallsyms: &std::path::Path) -> Result<()> {
    let mut perf_args = super::perf_kvm_args(args.host, kallsyms);
    perf_args.extend([
        "record".into(),
        "-e".into(),     // event selector
        "cycles".into(), // hardware CPU cycle counter
    ]);
    if let Some(freq) = args.freq {
        perf_args.push("-F".into());
        perf_args.push(freq.to_string().into());
    }
    perf_args.push("-o".into());
    perf_args.push(output.as_os_str().to_owned());
    perf_args.push("--".into());
    perf_args.extend(args.workload.iter().cloned());

    let status = Command::new("perf")
        .args(&perf_args)
        .status()
        .context("Failed to execute perf")?;

    // perf kvm record passes through the workload's exit code, so any
    // non-zero may just mean the workload itself returned non-zero (which
    // is fine — data was still recorded). Only warn rather than bail.
    if let Some(code) = status.code()
        && code != 0
    {
        eprintln!("Warning: perf kvm record exited with status {code} (workload may have failed)");
    }

    Ok(())
}

/// Check that we're on Linux with KVM and perf available.
fn check_prerequisites() -> Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        bail!("cargo hyperlight perf requires Linux with KVM");
    }

    #[cfg(target_os = "linux")]
    {
        use std::fs;

        which("perf").context("perf not found (install linux-perf / perf-tools / linux-tools)")?;

        let kvm = std::path::Path::new("/dev/kvm");
        if !kvm.exists() {
            bail!("No KVM device found at /dev/kvm");
        }

        // Check perf_event_paranoid
        if let Ok(val) = fs::read_to_string("/proc/sys/kernel/perf_event_paranoid")
            && let Ok(n) = val.trim().parse::<i32>()
            && n > 1
        {
            eprintln!(
                "Warning: perf_event_paranoid={n} (need <=1). Run: sudo sysctl kernel.perf_event_paranoid=-1"
            );
        }

        // Detect if we're running inside a VM or WSL2.  The CPU sets the
        // "hypervisor" flag (CPUID leaf 0x1, ECX bit 31) when running under
        // a hypervisor.  In a VM the host PMU is virtualized and may not
        // support the events needed for `perf kvm` guest profiling — samples
        // may be missing or empty.  WSL2 runs in a Hyper-V VM and has the
        // same limitation.
        if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo")
            && cpuinfo
                .lines()
                .any(|l| l.starts_with("flags") && l.contains(" hypervisor"))
        {
            eprintln!(
                "Warning: running inside a VM (hypervisor CPU flag detected). \
                     The virtualized PMU may not support KVM guest profiling — \
                     you may get zero guest samples. For reliable results, run \
                     on bare-metal hardware."
            );
        }

        // Check for precise guest IP sampling support.
        //
        // Intel: Guest PEBS was introduced with Ice Lake.  Without it,
        // `perf kvm` falls back to NMI-based sampling where the guest
        // RIP has significant skid, making function-level attribution
        // unreliable (see the module-level "Known issue" documentation).
        //
        // We read the PMU name from sysfs and check against the known
        // set of pre-Ice-Lake PMU names (a closed set that won't grow).
        // Unknown names are assumed to be newer and not warned about.
        //
        // TODO: AMD guest IBS (IBSVIRT, Zen 4+) has the same issue but
        // detection is not implemented yet — needs testing on AMD hardware.
        const PRE_ICELAKE_PMUS: &[&str] = &[
            "nehalem",
            "westmere",
            "sandybridge",
            "ivybridge",
            "haswell",
            "broadwell",
            "skylake",
            "knl", // Knights Landing
        ];
        if let Ok(pmu_name) = fs::read_to_string("/sys/bus/event_source/devices/cpu/caps/pmu_name")
        {
            let pmu = pmu_name.trim();
            if PRE_ICELAKE_PMUS.contains(&pmu) {
                eprintln!(
                    "Warning: CPU PMU is '{pmu}' (pre-Ice Lake). Guest PEBS is not \
                     available — guest IP samples will have significant NMI skid, \
                     making function-level attribution unreliable."
                );
            }
        }

        Ok(())
    }
}
