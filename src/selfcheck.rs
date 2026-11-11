//! Headless selfcheck plumbing: one check's outcome, and the report that
//! prints them and turns them into a process exit code.
//!
//! Shared by the workbench framework and the nanobot harness; it replaced
//! their two identical copies. The check *suites* stay in each framework
//! (they probe very different runtimes), as does the crash-recovery probe
//! driver.

/// One selfcheck outcome.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub skipped: bool,
    pub detail: String,
}

impl Check {
    pub fn new(name: impl Into<String>, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok,
            skipped: false,
            detail: detail.into(),
        }
    }

    /// Precondition not met (probe-style: unrelated checks SKIP, not FAIL).
    pub fn skip(name: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ok: false,
            skipped: true,
            detail: reason.into(),
        }
    }
}

/// The language of the summary line. The check names themselves are the
/// product's, so only the frame around them is chosen here.
#[derive(Clone, Copy)]
pub enum Summary {
    English,
    Chinese,
}

/// Print every check and the summary; returns the process exit code
/// (0 = nothing failed, 1 = at least one check failed).
pub fn report(checks: &[Check], summary: Summary) -> i32 {
    let failed = checks.iter().filter(|c| !c.ok && !c.skipped).count();
    let skipped = checks.iter().filter(|c| c.skipped).count();
    for check in checks {
        let tag = if check.skipped {
            "SKIP"
        } else if check.ok {
            "PASS"
        } else {
            "FAIL"
        };
        println!("[{}] {} — {}", tag, check.name, check.detail);
    }
    let run_total = checks.len() - skipped;
    match summary {
        Summary::English => println!(
            "selfcheck: {}/{} passed ({} skipped)",
            run_total - failed,
            run_total,
            skipped
        ),
        Summary::Chinese => println!(
            "selfcheck: {}/{} 项通过（{} 项跳过）",
            run_total - failed,
            run_total,
            skipped
        ),
    }
    if failed == 0 {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skipped_check_is_not_a_failure() {
        let checks = vec![
            Check::new("a", true, "ok"),
            Check::skip("b", "no precondition"),
            Check::new("c", false, "broken"),
        ];
        // Two ran, one failed -> exit 1 (the skip does not count either way).
        assert_eq!(report(&checks, Summary::English), 1);
    }

    #[test]
    fn all_passing_exits_zero() {
        let checks = vec![Check::new("a", true, "ok"), Check::skip("b", "no precondition")];
        assert_eq!(report(&checks, Summary::Chinese), 0);
    }
}
