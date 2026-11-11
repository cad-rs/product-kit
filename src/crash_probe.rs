//! The crash-recovery subprocess probe: which phase to run, and how to launch
//! it.
//!
//! Shared by the workbench framework and the nanobot harness; it replaced
//! their two copies. Each framework keeps its own probe *bodies* (they crash
//! very different runtimes) and its own error wording — the failure is a
//! product-neutral enum they render.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// How long a phase may run before it is killed.
const TIMEOUT: Duration = Duration::from_secs(30);
/// How often the child is polled while it runs.
const POLL: Duration = Duration::from_millis(20);

/// The three phases of the crash-recovery probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbePhase {
    /// Exit without running `Drop`, so no clean-exit marker is written.
    Crash,
    /// The launch after a crash: it must recover.
    Recover,
    /// The launch after a clean exit: recovery must be suppressed.
    Clean,
}

impl ProbePhase {
    /// Read the phase from `var` (`A`/`B`/`C`; anything else is `Crash`).
    pub fn from_env(var: &str) -> Self {
        match std::env::var(var).unwrap_or_default().as_str() {
            "B" => Self::Recover,
            "C" => Self::Clean,
            _ => Self::Crash,
        }
    }

    /// The letter the environment spells the phase with.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Crash => "A",
            Self::Recover => "B",
            Self::Clean => "C",
        }
    }
}

/// Why a phase could not be run.
#[derive(Debug)]
pub enum ProbeFailure {
    /// The subprocess could not be started.
    Spawn(String),
    /// It ran longer than [`TIMEOUT`] and was killed.
    Timeout(ProbePhase),
    /// Waiting for it failed.
    Wait(String),
    /// Its output could not be read.
    Output(String),
}

/// The probe's working directory, from `dir_var`.
pub fn probe_dir(dir_var: &str) -> PathBuf {
    PathBuf::from(std::env::var(dir_var).unwrap_or_default())
}

/// Launch one phase as a subprocess, timing out instead of hanging when a
/// product does not implement the `--selfcheck-crash` mode.
///
/// Compiles everywhere (the selfcheck is a native tool); on the web a spawn
/// fails, as it always did.
pub fn run_phase(
    exe: &Path,
    dir: &Path,
    phase: ProbePhase,
    dir_var: &str,
    phase_var: &str,
    flag: &str,
) -> Result<Output, ProbeFailure> {
    let mut child = Command::new(exe)
        .arg(flag)
        .env(dir_var, dir)
        .env(phase_var, phase.as_str())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| ProbeFailure::Spawn(e.to_string()))?;
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if start.elapsed() > TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(ProbeFailure::Timeout(phase));
                }
                std::thread::sleep(POLL);
            }
            Err(e) => return Err(ProbeFailure::Wait(e.to_string())),
        }
    };
    let mut out = child.wait_with_output().map_err(|e| ProbeFailure::Output(e.to_string()))?;
    out.status = status;
    Ok(out)
}
