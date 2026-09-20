//! Process-presence signals (`Start.md` §16.2).
//!
//! "Is the agent running" is exactly one signal among several (recent
//! writes, file locks, scan-before/after fingerprints) — this module
//! deliberately answers only the process question and nothing else.
//! Policy layers combine signals (§16.2); providers surface the fact
//! via `ProviderInspection::agent_running`.

use sysinfo::System;

/// What marks a process as "probably this agent".
///
/// Executable-name matching is exact (stem compared, extension and case
/// ignored — `WorkBuddy.exe` matches `workbuddy`); command-line
/// matching is substring, for the common case of agents running as
/// `node …/claude/…` where the exe name alone says nothing. Matching
/// generously is acceptable: a false "running" only blocks cleanup
/// (fail closed), it never grants anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSignature {
    /// Lowercase executable stems, e.g. `["claude", "workbuddy"]`.
    pub exe_names: Vec<String>,
    /// Lowercase needles searched in the full command line, e.g.
    /// `[".claude", "claude-code"]`.
    pub cmdline_substrings: Vec<String>,
}

impl ProcessSignature {
    pub fn new(exe_names: &[&str], cmdline_substrings: &[&str]) -> Self {
        Self {
            exe_names: exe_names.iter().map(|s| s.to_lowercase()).collect(),
            cmdline_substrings: cmdline_substrings
                .iter()
                .map(|s| s.to_lowercase())
                .collect(),
        }
    }
}

/// Is any process matching one of the signatures currently running?
///
/// This is a point-in-time check; a clean return does *not* prove the
/// agent cannot start mid-operation (the executor re-checks, §12.1).
#[must_use]
pub fn any_process_running(signatures: &[ProcessSignature]) -> bool {
    if signatures.is_empty() {
        return false;
    }
    let mut system = System::new();
    system.refresh_processes();

    for process in system.processes().values() {
        // sysinfo 0.30 returns owned strings (&str name, &[String] cmd).
        let exe_stem = process
            .name()
            .to_lowercase()
            .trim_end_matches(".exe")
            .to_string();

        for signature in signatures {
            if signature.exe_names.contains(&exe_stem) {
                return true;
            }
            if !signature.cmdline_substrings.is_empty() {
                let cmdline = process
                    .cmd()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
                    .to_lowercase();
                if signature
                    .cmdline_substrings
                    .iter()
                    .any(|needle| cmdline.contains(needle))
                {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn some_process_is_running() {
        // `svchost.exe` on Windows and `launchd` on macOS are
        // guaranteed to be running on a normal machine, so matching
        // either by executable name is a stable positive case that
        // does not depend on any specific agent being installed.
        // (`cmd()` is unreliable on Windows: cmdline reads from
        // other users' processes need elevation; we use `exe_name`
        // instead, which is always available.)
        let exe_name = if cfg!(windows) { "svchost" } else { "launchd" };
        let signature = ProcessSignature::new(&[exe_name], &[]);
        assert!(any_process_running(&[signature]));
    }

    #[test]
    fn no_process_matches_garbage_signature() {
        // Nothing on a normal machine runs anything like this.
        let signature = ProcessSignature::new(
            &["definitely-not-a-real-agent-xyzzy"],
            &["definitely-not-a-real-agent-xyzzy"],
        );
        assert!(!any_process_running(&[signature]));
    }

    #[test]
    fn empty_signatures_mean_no() {
        assert!(!any_process_running(&[]));
        // Signature with no criteria must not match everything.
        assert!(!any_process_running(&[ProcessSignature::new(&[], &[])]));
    }
}
