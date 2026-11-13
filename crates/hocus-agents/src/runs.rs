//! An agent's test runs, as the desktop hears of them (#810): what makes
//! its fireworks go off when it finishes with all its tests passing.
//!
//! Two ways in: `hocus agent NAME done --tests-passed [N]`, for any tool
//! that knows; and the hooks of Claude Code and Codex, which read what a
//! shell command printed ([`of_output`]): a test runner's summary line
//! (cargo test, cargo-nextest, pytest, Jest, Vitest, go test). The last
//! run of a turn counts: a run that fails after one that passed is a
//! failure.

use std::{fmt, str::FromStr};

use crate::BadLine;

/// How a test run went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestRun {
    /// Every test passed: how many, when the runner said it.
    Passed(Option<u32>),
    /// At least one failed (or the run did).
    Failed,
}

/// On a report's line, after a tab: `tests passed [N]`, `tests failed`.
impl fmt::Display for TestRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Passed(Some(n)) => write!(f, "tests passed {n}"),
            Self::Passed(None) => f.write_str("tests passed"),
            Self::Failed => f.write_str("tests failed"),
        }
    }
}

impl FromStr for TestRun {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut words = s.split(' ');
        if words.next() != Some("tests") {
            return Err(BadLine("not a test run".to_owned()));
        }
        match (words.next(), words.next(), words.next()) {
            (Some("passed"), None, None) => Ok(Self::Passed(None)),
            (Some("passed"), Some(n), None) => n
                .parse()
                .map(|n| Self::Passed(Some(n)))
                .map_err(|e| BadLine(format!("a test count: {e}"))),
            (Some("failed"), None, None) => Ok(Self::Failed),
            _ => Err(BadLine("a test run that does not read".to_owned())),
        }
    }
}

/// What a command's output says of its tests, when it ran some: the
/// runners' summary lines, every one of them (cargo test prints one a
/// test binary). Failed when one says a test failed; passed when all
/// that say something say so, their counts added up.
pub fn of_output(text: &str) -> Option<TestRun> {
    let mut passed: Option<Option<u32>> = None;
    for line in text.lines() {
        let Some(said) = summary(line.trim()) else {
            continue;
        };
        match said {
            TestRun::Failed => return Some(TestRun::Failed),
            TestRun::Passed(n) => {
                passed = Some(match (passed, n) {
                    (None, n) => n,
                    (Some(Some(a)), Some(b)) => Some(a.saturating_add(b)),
                    _ => None,
                });
            }
        }
    }
    match passed {
        // A run of no test is no run.
        Some(Some(0)) | None => None,
        Some(n) => Some(TestRun::Passed(n)),
    }
}

/// The number before `word` in `line` ("12 passed"), if it is there.
fn count_before(line: &str, word: &str) -> Option<u32> {
    let words: Vec<&str> = line
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '|')
        .filter(|w| !w.is_empty())
        .collect();
    words.windows(2).find_map(|pair| match pair {
        [n, w] if w.trim_end_matches([',', '.', ';']) == word => n.parse().ok(),
        _ => None,
    })
}

/// What one summary line says, if it is one.
fn summary(line: &str) -> Option<TestRun> {
    let failed = |line: &str| {
        ["failed", "timed", "errors", "error"]
            .into_iter()
            .any(|w| count_before(line, w).is_some_and(|n| n > 0))
    };
    // cargo test: `test result: ok. 12 passed; 0 failed; 0 ignored; …`.
    if let Some(rest) = line.strip_prefix("test result: ") {
        if rest.starts_with("FAILED") || failed(rest) {
            return Some(TestRun::Failed);
        }
        return Some(TestRun::Passed(count_before(rest, "passed")));
    }
    // cargo-nextest: `Summary [ 212.4s] 2040 tests run: 2040 passed, 3 skipped`.
    if line.contains(" run: ") && (line.contains(" tests run: ") || line.contains(" test run: ")) {
        if failed(line) {
            return Some(TestRun::Failed);
        }
        return Some(TestRun::Passed(count_before(line, "passed")));
    }
    // Jest (`Tests:       11 passed, 12 total`), Vitest (`Tests  12 passed (12)`).
    if let Some(rest) = line
        .strip_prefix("Tests:")
        .or_else(|| line.strip_prefix("Tests "))
        && (rest.contains(" passed") || rest.contains(" failed"))
    {
        if failed(rest) {
            return Some(TestRun::Failed);
        }
        return Some(TestRun::Passed(count_before(rest, "passed")));
    }
    // pytest: `===== 12 passed, 2 skipped in 0.34s =====`, `1 failed, 11 passed in 0.5s`.
    let bare = line.trim_matches(|c: char| c == '=' || c.is_whitespace());
    if (bare.contains(" passed") || bare.contains(" failed"))
        && bare
            .rsplit_once(" in ")
            .is_some_and(|(_, time)| time.trim_end_matches('s').parse::<f64>().is_ok())
    {
        if failed(bare) {
            return Some(TestRun::Failed);
        }
        return Some(TestRun::Passed(count_before(bare, "passed")));
    }
    // go test: `ok  	example.com/pkg	0.012s`, `FAIL	example.com/pkg	0.01s`.
    if line.starts_with("FAIL\t") || line == "FAIL" || line.starts_with("--- FAIL") {
        return Some(TestRun::Failed);
    }
    if line.starts_with("ok  \t") {
        return Some(TestRun::Passed(None));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_test_run_reads_back_as_written() {
        for run in [
            TestRun::Passed(Some(2040)),
            TestRun::Passed(None),
            TestRun::Failed,
        ] {
            assert_eq!(run.to_string().parse::<TestRun>(), Ok(run));
        }
        for bad in ["tests", "tests passed x", "tests ok", "allow Bash ls"] {
            assert!(bad.parse::<TestRun>().is_err(), "{bad}");
        }
    }

    #[test]
    fn the_runners_summaries_say_whether_all_passed() {
        let nextest = "    Starting 2040 tests across 9 binaries\n        PASS [   0.004s] x\n\
            ────────────\n     Summary [ 212.4s] 2040 tests run: 2040 passed, 3 skipped\n";
        assert_eq!(of_output(nextest), Some(TestRun::Passed(Some(2040))));
        let nextest_failed = "     Summary [  12.0s] 40 tests run: 39 passed, 1 failed, 0 skipped";
        assert_eq!(of_output(nextest_failed), Some(TestRun::Failed));
        let nextest_timed_out = "     Summary [  12.0s] 4 tests run: 3 passed, 1 timed out";
        assert_eq!(of_output(nextest_timed_out), Some(TestRun::Failed));
        // cargo test: one line a binary, added up.
        let cargo = "running 3 tests\ntest a ... ok\n\
            test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n\
            test result: ok. 9 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";
        assert_eq!(of_output(cargo), Some(TestRun::Passed(Some(12))));
        let cargo_failed = "test result: ok. 3 passed; 0 failed\n\
            test result: FAILED. 8 passed; 1 failed; 0 ignored";
        assert_eq!(of_output(cargo_failed), Some(TestRun::Failed));
        assert_eq!(
            of_output("============= 12 passed, 2 skipped in 0.34s =============="),
            Some(TestRun::Passed(Some(12)))
        );
        assert_eq!(
            of_output("==== 1 failed, 11 passed in 0.50s ===="),
            Some(TestRun::Failed)
        );
        assert_eq!(
            of_output("Tests:       12 passed, 12 total"),
            Some(TestRun::Passed(Some(12)))
        );
        assert_eq!(
            of_output("Tests:       1 failed, 11 passed, 12 total"),
            Some(TestRun::Failed)
        );
        assert_eq!(
            of_output("      Tests  12 passed (12)"),
            Some(TestRun::Passed(Some(12)))
        );
        assert_eq!(
            of_output("ok  \texample.com/pkg\t0.012s\nok  \texample.com/b\t0.2s"),
            Some(TestRun::Passed(None))
        );
        assert_eq!(
            of_output("ok  \texample.com/pkg\t0.012s\nFAIL\texample.com/b\t0.2s"),
            Some(TestRun::Failed)
        );
        // No summary, or no test run: nothing.
        assert_eq!(of_output("Compiling hocus v0.1\nFinished dev"), None);
        assert_eq!(of_output("total 12\ndrwxr-x 2 alex"), None);
        assert_eq!(
            of_output("test result: ok. 0 passed; 0 failed; 0 ignored"),
            None
        );
    }
}
