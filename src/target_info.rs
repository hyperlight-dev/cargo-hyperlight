use std::collections::HashMap;
use std::env::consts::ARCH;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use crate::cargo_cmd::{CargoCmd, cargo_cmd};
use crate::cli::{Args, WarningLevel};

#[derive(Clone, Debug)]
pub struct TargetInfo {
    suffix: String,
    arch: String,
    guest_target: String,
}
impl TargetInfo {
    fn new() -> Self {
        let v = rustc_version::version().unwrap();
        let suffix = if v > rustc_version::Version::parse("1.100.0").unwrap() {
            "unknown-hyperlight"
        } else {
            "hyperlight-none"
        };
        Self {
            suffix: suffix.to_string(),
            arch: ARCH.to_string(),
            guest_target: format!("{ARCH}-{suffix}"),
        }
    }

    fn resolve<W: WarningLevel>(
        &self,
        warn: W,
        args_override: Option<String>,
        env: &HashMap<OsString, OsString>,
        cwd: &PathBuf,
    ) -> Result<Self, W::Error> {
        let manifest_override = crate::cli::resolve_target(env, cwd);
        let manifest_override = match manifest_override {
            Err(e) => warn.warning("could not query cargo about target", e, None)?,
            Ok(o) => o,
        };
        let target = args_override
            .or(manifest_override)
            .unwrap_or_else(|| self.guest_target.to_string());
        let (arch, suffix) = target.split_once('-').map_or_else(
            || {
                warn.warning(
                    "could not parse arch/suffix from target",
                    anyhow::anyhow!("could not parse arch/suffix from {target}"),
                    (ARCH, self.suffix.as_str()),
                )
            },
            core::result::Result::Ok,
        )?;
        let suffix = if suffix != self.suffix {
            warn.warning(
                "requested target is not a hyperlight target",
                anyhow::anyhow!("invalid hyperlight target suffix: {suffix}"),
                self.suffix.as_str(),
            )?
        } else {
            suffix
        };
        Ok(Self {
            suffix: suffix.to_string(),
            arch: arch.to_string(),
            // Re-create the target in case we have changed it via the
            // validation/defaulting logic above
            guest_target: format!("{arch}-{suffix}"),
        })
    }

    pub(crate) fn write_spec(&self, args: &Args, triplet_dir: &Path) -> Result<()> {
        if self.suffix == "hyperlight-none" {
            // We are using the old downstream targets, so we need to
            // write all our modifications into a spec json.
            let base_target = format!("{}-unknown-none", self.arch);
            let mut spec = get_base_spec(args, base_target)?;
            let Value::Object(custom) = (match self.arch.as_str() {
                "x86_64" => json!({
                    "code-model": "small",
                    "linker": "rust-lld",
                    "linker-flavor": "gnu-lld",
                    "pre-link-args": {
                        "gnu-lld": ["-znostart-stop-gc"],
                    },
                    "features": "-mmx,+sse,+sse2,-sse3,-ssse3,-sse4.1,-sse4.2,-avx,-avx2,-soft-float"
                }),
                "aarch64" => json!({
                    "code-model": "small",
                    "linker": "rust-lld",
                    "linker-flavor": "gnu-lld",
                    "pre-link-args": {
                        "gnu-lld": ["-znostart-stop-gc"],
                    },
                    "relocation-model": "pic",
                    "direct-access-external-data": true,
                    "position-independent-executables": true,
                    "features": "+v8.1a,+strict-align,+neon,+fp-armv8"
                }),
                _ => anyhow::bail!("Unknown architecture {}", self.arch),
            }) else {
                unreachable!()
            };
            spec.extend(custom);
            spec.remove("rustc-abi");

            std::fs::write(
                triplet_dir.join("target.json"),
                serde_json::to_string_pretty(&spec).unwrap(),
            )
            .context("Failed to write target spec file")?
        }
        Ok(())
    }

    pub(crate) fn is_x86_64(&self) -> bool {
        self.arch == "x86_64"
    }
}
impl AsRef<str> for TargetInfo {
    fn as_ref(&self) -> &str {
        &self.guest_target
    }
}
impl AsRef<Path> for TargetInfo {
    fn as_ref(&self) -> &Path {
        Path::new(&self.guest_target)
    }
}
impl std::fmt::Display for TargetInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        write!(f, "{}", self.guest_target)
    }
}

static DEFAULT_TARGET: LazyLock<TargetInfo> = LazyLock::new(TargetInfo::new);

pub(crate) fn resolve<W: WarningLevel>(
    warn: W,
    args_override: Option<String>,
    env: &HashMap<OsString, OsString>,
    cwd: &PathBuf,
) -> Result<TargetInfo, W::Error> {
    (*DEFAULT_TARGET).resolve(warn, args_override, env, cwd)
}

pub fn default() -> &'static TargetInfo {
    &DEFAULT_TARGET
}

fn get_base_spec(args: &Args, triplet: impl AsRef<str>) -> Result<Map<String, Value>> {
    let output = cargo_cmd(&args.env)?
        .current_dir(&args.current_dir)
        .arg("rustc")
        .arg("--target")
        .arg(triplet.as_ref())
        .manifest_path(&args.manifest_path)
        .arg("-Zunstable-options")
        .arg("--print=target-spec-json")
        .arg("--")
        .arg("-Zunstable-options")
        // printing target-spec-json is an unstable feature
        .allow_unstable()
        .checked_output()
        .context("Failed to get base target spec")?;

    serde_json::from_slice(&output.stdout).context("Failed to parse target spec JSON")
}
