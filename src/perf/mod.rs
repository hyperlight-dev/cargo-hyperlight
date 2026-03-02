//! `cargo hyperlight perf` — Profile Hyperlight guest execution with `perf kvm`.
//!
//! This subcommand automates the workflow of generating guest symbol information
//! and running `perf kvm` to profile code executing inside Hyperlight micro-VMs.
//!
//! # How it works
//!
//! Hyperlight loads guest PIE ELF binaries at a configurable base address (default
//! `0x1000` with init-paging). `perf kvm` resolves guest samples using a
//! kallsyms-format text file (`--guestkallsyms`) containing symbol addresses
//! shifted to match the runtime guest layout (ELF VA + base address).
//!
//! This command:
//! 1. Reads the guest ELF binary using the `object` crate
//! 2. Generates a kallsyms file with addresses shifted by the base address
//! 3. Runs `perf kvm record` with the appropriate flags
//! 4. Displays a `perf kvm report` with demangled symbols
//!
//! To mitigate sample misattribution on pre-Ice Lake CPUs (see below), the
//! generated kallsyms includes synthetic `__gap__` symbols between functions
//! wherever there are inter-function regions (alignment padding, unused code).
//! This prevents perf's `symbols__fixup_end()` from stretching function ranges
//! across gaps, which would cause skidded NMI samples to be misattributed to
//! the preceding function.
//!
//! **Important:** The gap marker name must NOT use bracket characters (e.g.
//! `[gap]`), because perf's kallsyms parser interprets `[name]` as a kernel
//! module annotation (like `/proc/kallsyms` lines ending in `[module_name]`).
//! Using brackets corrupts the symbol table and causes addresses inside
//! nearby functions to become unresolvable (shown as raw hex in reports).
//!
//! # Why gap markers are needed (and when they matter)
//!
//! ## The kallsyms format has no size information
//!
//! `perf kvm --guestkallsyms` accepts a kallsyms-format file: lines of
//! `address type name` — nothing else. This format was designed for Linux
//! kernel profiling, where `/proc/kallsyms` lists kernel symbols that are
//! typically laid out contiguously with no gaps. Crucially, **kallsyms does
//! not carry `st_size`** — there is no way to express a symbol's extent.
//!
//! ## `symbols__fixup_end()` assumes contiguous layout
//!
//! Since kallsyms has no size field, perf's `symbols__fixup_end()`
//! (tools/perf/util/symbol.c) sets each symbol's end address to the start
//! of the next symbol. For contiguous kernel text this is correct, but for
//! a general ELF binary it's wrong: functions may have alignment padding
//! or gaps from linker section placement between them. Without gap markers,
//! `symbols__fixup_end()` would stretch each function's range to the next
//! function.
//!
//! ## Why `perf kvm` can't just read the ELF
//!
//! Normal userspace profiling (`perf record ./binary`) doesn't have this
//! problem — perf reads the ELF directly via `/proc/<pid>/maps` + the
//! binary's `.symtab`/`.dynsym`, which include `st_size`. But KVM guest
//! profiling goes through a completely different code path: the guest RIP
//! in samples is a guest virtual address with no associated host process
//! or `/proc` mapping. `perf kvm` resolves these addresses using the
//! kallsyms mechanism (designed for kernel symbol resolution), which has
//! no concept of ELF symbol sizes. There is no `--guest-elf` option.
//!
//! ## When gap markers matter
//!
//! **Pre-Ice Lake (no guest PEBS):** NMI skid causes the sampled guest RIP
//! to be tens to hundreds of instructions away from the true overflow point.
//! Skidded samples can land in gap regions (alignment padding, unused code).
//! Without gap markers, `symbols__fixup_end()` stretches the preceding
//! function's range to cover the gap, and these skidded samples are
//! misattributed to that function. Gap markers absorb these samples instead.
//!
//! **Ice Lake+ (guest PEBS, `precise_ip=3`):** PEBS records the exact
//! instruction that retired at counter overflow. Gap regions contain no
//! executable code (only alignment padding, `nop`/`int3` bytes), so no
//! instruction ever retires there and no sample will have an IP in a gap.
//! Whether `symbols__fixup_end()` stretches ranges across gaps or not has
//! no effect on `perf report` output — the sample counts are identical
//! either way. Gap markers are harmless but have no practical impact.
//!
//! # Subcommands
//!
//! - `cargo hyperlight perf record` — Record samples (like `perf record`).
//! - `cargo hyperlight perf report` — Display a report from recorded data
//!   (like `perf report`).
//!
//! # Modes
//!
//! - **Guest-only** (default): `perf kvm record` captures only guest samples.
//! - **Combined** (`--host`): `perf kvm --host --guest` captures host and
//!   guest samples scoped to the workload process tree.
//!
//! # Requirements
//!
//! The guest ELF binary must contain a `.symtab` section with function symbols.
//! Debug info (`.debug_*` sections) is **not** needed — only the symbol table
//! matters. Rust release builds (which omit debug info by default) work fine
//! since `.symtab` is preserved. For Rust, only `strip = "symbols"` or
//! `strip = true` in the Cargo profile will remove `.symtab` and break
//! profiling. For C/C++, `strip -s` / `--strip-all` has the same effect;
//! `strip --strip-debug` is safe.
//!
//! # Limitations
//!
//! Flat profiles only (no guest call stacks). `perf kvm` cannot unwind the
//! guest stack because guest virtual addresses are not resolvable through host
//! page tables.
//!
//! # Known issue: guest IP imprecision on pre-Ice Lake CPUs
//!
//! On pre-Ice Lake CPUs, `perf kvm` guest profiles are **unreliable for
//! function-level attribution**. Samples may appear in never-called functions.
//! This is a hardware limitation, not a software bug.
//!
//! ## Root cause
//!
//! Guest PEBS is only available on Ice Lake+. On older CPUs, `perf kvm`
//! falls back to NMI-based sampling (`precise_ip=0`). The PMU counter
//! overflows at instruction X, but the NMI is recognized many instructions
//! later (skid). The NMI triggers a VMEXIT, and KVM reads `GUEST_RIP` from
//! the VMCS—which reflects the skidded position, not the overflow point.
//!
//! The KVM path: `vmx_vcpu_enter_exit()` → NMI exit → `vmx_do_nmi_irqoff()`
//! → host NMI handler → `perf_instruction_pointer()` → `kvm_rip_read(vcpu)`
//! → `vmcs_readl(GUEST_RIP)`.
//!
//! ## Consequences
//!
//! On Broadwell, empirical analysis showed the captured IPs are **byte-level
//! random** within hot code regions:
//!
//! - Most guest IPs land at non-instruction-boundary addresses, at a rate
//!   matching random chance given the average x86 instruction length.
//! - IPs do cluster in genuinely hot ~KB-scale code regions (cold code
//!   gets zero samples), but within those regions the byte position is
//!   random.
//! - Function attribution is proportional to byte size, not execution
//!   frequency. Large functions in hot regions attract disproportionate
//!   samples even if never called.
//!
//! ## Workarounds
//!
//! - **Native profiling**: Build guest code as a native binary and profile
//!   with `perf record -e cycles:pp` for PEBS-quality results.
//! - **Upgrade to Ice Lake+**: Enables guest PEBS with `precise_ip=3`.
//! - **Treat profiles as region-level heatmaps**: ~KB-scale region hotness
//!   is valid; per-function percentages are not.

mod record;
mod report;

use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;
#[cfg(target_os = "linux")]
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use object::read::elf::ElfFile64;
use object::{Endianness, Object, ObjectSection, ObjectSymbol, SymbolKind};

/// Default base address where Hyperlight loads guest binaries (init-paging).
const DEFAULT_BASE_ADDRESS: u64 = 0x1000;

/// Profile Hyperlight guest execution with perf kvm (Linux/KVM only).
#[derive(Parser, Debug)]
#[command(name = "perf")]
struct PerfCli {
    #[command(subcommand)]
    command: PerfCommand,
}

/// Subcommands for `cargo hyperlight perf`.
#[derive(Subcommand, Debug)]
enum PerfCommand {
    /// Record CPU cycle samples inside Hyperlight micro-VMs.
    Record(record::RecordArgs),
    /// Display a profile report from previously recorded perf data.
    Report(report::ReportArgs),
}

/// Main entry point for `cargo hyperlight perf`.
///
/// The iterator should start with the subcommand name ("perf"), which
/// clap consumes as the binary name (argv\[0\]).
pub fn run(args: impl Iterator<Item = OsString>) -> Result<()> {
    let cli = PerfCli::parse_from(args);

    match cli.command {
        PerfCommand::Record(args) => record::run(args),
        PerfCommand::Report(args) => report::run(args),
    }
}

/// Generate a kallsyms-format string from the guest ELF binary.
///
/// For each defined symbol with a nonzero address, the output line is:
///   `{address + base_address:016x} T {name}`
///
/// Symbols are sorted by address ascending (as required by kallsyms format).
/// We also inject `_text` and `_stext` symbols at the `.text` section address
/// so that `perf kvm` can set up the guest kernel map.
///
/// ## Why we can't just emit raw symbols
///
/// The kallsyms format (`address type name`) carries no size information.
/// `perf kvm` processes these symbols through `symbols__fixup_end()`
/// (tools/perf/util/symbol.c), which extends each symbol's range to the
/// start of the next symbol — a heuristic designed for contiguous kernel
/// text. For general ELF binaries with gaps between functions (alignment
/// padding, dead code, linker-placed sections), this causes misattribution:
/// samples in gaps are credited to the preceding function.
///
/// Unlike userspace profiling where perf reads the ELF's `.symtab` with
/// `st_size` via `/proc/<pid>/maps`, KVM guest samples are guest virtual
/// addresses with no host-side process or memory mapping. `perf kvm` has
/// no `--guest-elf` option and cannot read symbol sizes from the binary.
///
/// ## Gap markers
///
/// To compensate, we read `st_size` from the ELF ourselves and inject
/// synthetic `__gap__` markers at each function's true end whenever a
/// gap exists before the next function. `symbols__fixup_end()` then clips
/// each real symbol at its true boundary. On pre-Ice Lake CPUs (no guest
/// PEBS), NMI skid causes samples to land in gap regions — the `__gap__`
/// markers absorb these instead of letting them inflate a neighboring
/// function. On Ice Lake+ with PEBS, no sample lands in gaps (no code
/// executes there), so the markers have no practical effect but are
/// harmless.
fn generate_kallsyms(guest_binary: &Path, base_address: u64) -> Result<String> {
    let data = fs::read(guest_binary)
        .with_context(|| format!("Cannot read {}", guest_binary.display()))?;

    let elf = ElfFile64::<Endianness>::parse(&*data)
        .with_context(|| format!("Failed to parse ELF: {}", guest_binary.display()))?;

    // Find .text section address for _text/_stext injection (after dedup).
    let text_addr = elf
        .section_by_name(".text")
        .map(|s| s.address() + base_address);

    // Collect (shifted_addr, size, name) for all defined function symbols.
    let mut syms: Vec<(u64, u64, String)> = Vec::new();

    for sym in elf.symbols() {
        // Only include function symbols (STT_FUNC / STT_GNU_IFUNC).
        // Data, section, and NOTYPE symbols have addresses in .rodata/.data
        // that interleave with .text, causing wrong attribution and bad gaps.
        if sym.kind() != SymbolKind::Text {
            continue;
        }

        let name = match sym.name() {
            Ok(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };

        let addr = sym.address();
        if addr == 0 {
            continue;
        }

        syms.push((addr + base_address, sym.size(), name));
    }

    // Sort by address, then by size descending (so largest-size symbol
    // comes first at each address — this matters for ICF dedup below).
    syms.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));

    if syms.is_empty() {
        bail!(
            "No symbols found in {}. Is it a stripped binary?",
            guest_binary.display()
        );
    }

    // Deduplicate ICF (Identical Code Folding) symbols.
    //
    // ICF merges function bodies with identical machine code, leaving
    // multiple symbol names at the same address with the same size.
    // perf's symbols__fixup_end() would give all but the last symbol
    // at the same address a zero-length range, so we keep only the
    // first (largest-size) symbol at each address.
    let total_before_dedup = syms.len();
    syms.dedup_by_key(|s| s.0);
    let dedup_removed = total_before_dedup - syms.len();

    // After dedup every address is unique, so gap computation below
    // can assume each consecutive pair has distinct addresses.

    // Build the final symbol list, injecting gap markers between
    // consecutive symbols wherever there are inter-function regions
    // (alignment padding, unused code, linker-placed data).
    //
    // For each symbol we compute an effective end:
    //   - st_size > 0:  effective_end = min(addr + st_size, next_addr)
    //     The min() handles inflated st_size from ICF or over-estimated
    //     linker sizes that overlap the next symbol.
    //   - st_size == 0:  the function's true size is unknown.  We let
    //     symbols__fixup_end() extend it to the next symbol (correct
    //     for contiguous code).  No gap is injected.
    //
    // A gap marker is placed at effective_end whenever it falls short
    // of the next symbol's address, absorbing PMU samples that land
    // in dead/unreachable code.
    let mut final_syms: Vec<(u64, String)> = Vec::with_capacity(syms.len() * 2);
    let mut gap_count: usize = 0;

    for (i, (addr, size, name)) in syms.iter().enumerate() {
        final_syms.push((*addr, name.clone()));

        if let Some((next_addr, _, _)) = syms.get(i + 1) {
            let effective_end = if *size > 0 {
                (*addr + *size).min(*next_addr)
            } else {
                // Size unknown — let symbols__fixup_end() extend to
                // next symbol (no gap injection).
                continue;
            };

            if effective_end < *next_addr {
                final_syms.push((effective_end, "__gap__".to_string()));
                gap_count += 1;
            }
        }
    }

    // Inject _text and _stext AFTER gap computation — perf kvm requires
    // these markers at the .text section address to set up the guest
    // kernel map.  They are inserted into the final symbol list (not
    // into `syms`) to avoid interfering with gap detection: if they
    // were present during gap iteration, the same-address entries
    // would prevent gap injection after the first .text function.
    if let Some(addr) = text_addr {
        final_syms.push((addr, "_text".to_string()));
        final_syms.push((addr, "_stext".to_string()));
        final_syms.sort_by_key(|s| s.0);
    }

    let mut output = String::new();
    for (addr, name) in &final_syms {
        let demangled = rustc_demangle::demangle(name);
        writeln!(output, "{addr:016x} T {demangled:#}").unwrap();
    }

    eprintln!(
        "Prepared {} guest symbols ({} ICF duplicates removed), {} gap markers (base +{:#x})",
        syms.len(),
        dedup_removed,
        gap_count,
        base_address
    );

    Ok(output)
}

/// Write kallsyms content to a temp file and return it.
fn prepare_kallsyms(guest_binary: &Path, base_address: u64) -> Result<tempfile::NamedTempFile> {
    let kallsyms_content = generate_kallsyms(guest_binary, base_address)?;
    let kallsyms_file = tempfile::Builder::new()
        .suffix(".kallsyms")
        .tempfile()
        .context("Failed to create temp file for kallsyms")?;
    fs::write(kallsyms_file.path(), &kallsyms_content).context("Failed to write kallsyms file")?;
    Ok(kallsyms_file)
}

/// Build the common `perf kvm` argument prefix used by both record and report.
fn perf_kvm_args(host: bool, kallsyms: &Path) -> Vec<OsString> {
    let mut perf_args: Vec<OsString> = vec!["kvm".into()];
    if host {
        perf_args.push("--host".into());
        perf_args.push("--guest".into());
    }
    perf_args.push(format!("--guestkallsyms={}", kallsyms.display()).into());
    perf_args
}

/// Parse a number as hex (0x prefix) or decimal.
fn parse_hex_or_dec(s: &str) -> Result<u64, String> {
    if let Some(hex) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|e| format!("invalid hex number '{s}': {e}"))
    } else {
        s.parse::<u64>()
            .map_err(|e| format!("invalid number '{s}': {e}"))
    }
}

#[cfg(target_os = "linux")]
pub(super) fn which(cmd: &str) -> Result<PathBuf> {
    which::which(cmd).with_context(|| format!("{cmd} not found on PATH"))
}
