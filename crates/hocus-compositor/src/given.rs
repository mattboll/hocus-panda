//! A program given for this session in place of an installed one: the
//! shell's as a developer just built it (`hocus msg shell restart
//! --program PATH`, `scripts/dev/live.sh`; docs/workflow.md, "Development
//! loop").
//!
//! It is held here only: nothing is written to the configuration, and the
//! next session runs the installed one. One that does not start, or ends
//! [`QUICK_ENDS`] times in a row soon after it started, is let go of: the
//! installed one runs again, the log says so and the shell says it in a
//! notification once it is there ([`ToShell::FellBack`]).
//!
//! A program is given only in a developer mode (`[developer] live-reload`,
//! off by default, #783): the shell draws the lock screen and the password
//! dialog, and the control socket is open to every program of the user's.
//! A plain restart, the installed one again and the status are always
//! answered.
//!
//! The program is looked at ([`check`]) on the control socket's thread,
//! never on the loop.

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use hocus_agents::{ProgramAsk, REFUSED, Restart, Supervised};
use hocus_link::ToShell;

use crate::State;

/// The quick ends in a row after which a given program is let go of.
pub const QUICK_ENDS: u32 = 3;

/// Why a given program was let go of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    DidNotStart,
    KeptEnding,
}

impl Why {
    const fn says(self) -> &'static str {
        match self {
            Self::DidNotStart => "did not start",
            Self::KeptEnding => "kept ending",
        }
    }
}

/// The program given in place of an installed one, if one is.
#[derive(Debug, Default)]
pub struct Given {
    program: Option<PathBuf>,
    /// Its ends in a row that came quickly.
    quick_ends: u32,
    /// The last one let go of, and why: the status says it until the next
    /// restart asked for.
    fell_back: Option<(PathBuf, Why)>,
}

impl Given {
    pub fn program(&self) -> Option<&Path> {
        self.program.as_deref()
    }

    /// A restart was asked for: from now on `program`, the installed one,
    /// or the same as before.
    fn asked(&mut self, restart: Restart) {
        match restart {
            Restart::Same => {}
            Restart::Installed => self.program = None,
            Restart::Given(program) => self.program = Some(program),
        }
        self.quick_ends = 0;
        self.fell_back = None;
    }

    /// A run of `program` ended by itself after `ran` (`None`: it did not
    /// start); one that ran `steady` did not end quickly. Why the given
    /// program is let go of, if `program` is it and it is.
    pub fn ended(
        &mut self,
        program: &Path,
        ran: Option<Duration>,
        steady: Duration,
    ) -> Option<Why> {
        if self.program.as_deref() != Some(program) {
            return None;
        }
        let why = match ran {
            None => Why::DidNotStart,
            Some(ran) if ran >= steady => {
                self.quick_ends = 0;
                return None;
            }
            Some(_) => {
                self.quick_ends += 1;
                if self.quick_ends < QUICK_ENDS {
                    return None;
                }
                Why::KeptEnding
            }
        };
        self.quick_ends = 0;
        self.fell_back = self.program.take().map(|program| (program, why));
        Some(why)
    }
}

/// When a run started: for how long it has run, and the time the user
/// reads.
#[derive(Debug, Clone)]
pub struct Started {
    at: Instant,
    wall: jiff::Zoned,
}

impl Started {
    pub fn now() -> Self {
        Self {
            at: Instant::now(),
            wall: jiff::Zoned::now(),
        }
    }
}

/// What the status says of a supervised program.
pub struct Status<'a> {
    /// `off`, `starting`, `running`, `waiting` (to start again, after an
    /// end) or `ending` (a restart was asked for).
    pub state: &'static str,
    /// The program that runs, or the one the next start runs.
    pub program: Option<&'a Path>,
    pub installed: Option<&'a Path>,
    pub given: &'a Given,
    /// The process that runs and since when.
    pub process: Option<(Option<u32>, &'a Started)>,
    pub starts: u64,
    /// Whether a program can be given (`[developer] live-reload`).
    pub live_reload: bool,
}

impl Status<'_> {
    /// Its lines, `WORD VALUE` each, as `hocus msg idle`'s.
    pub fn lines(&self) -> String {
        let mut said = format!("state {}\n", self.state);
        let path = |program: Option<&Path>| {
            program.map_or_else(|| "none".to_owned(), |p| p.display().to_string())
        };
        // Writing to a String does not fail.
        let _ = writeln!(said, "program {}", path(self.program));
        let _ = writeln!(said, "installed {}", path(self.installed));
        let given = self.given.program.is_some();
        let _ = writeln!(said, "given {}", if given { "yes" } else { "no" });
        if let Some((pid, started)) = self.process {
            if let Some(pid) = pid {
                let _ = writeln!(said, "pid {pid}");
            }
            let _ = writeln!(said, "since {}", started.wall.strftime("%Y-%m-%d %H:%M:%S"));
            let _ = writeln!(said, "for {:.1} s", started.at.elapsed().as_secs_f64());
        }
        let _ = writeln!(said, "starts {}", self.starts);
        let live_reload = if self.live_reload { "on" } else { "off" };
        let _ = writeln!(said, "live-reload {live_reload}");
        if let Some((program, why)) = &self.given.fell_back {
            let _ = writeln!(said, "fell-back {} ({})", program.display(), why.says());
        }
        said
    }
}

/// A refusal's answer.
pub fn refused(why: impl std::fmt::Display) -> String {
    format!("{REFUSED}{why}\n")
}

/// Whether `program` can be given: an absolute path to a file this user
/// can execute. Reads the disk: for the control socket's thread.
pub fn check(program: &Path) -> Result<(), String> {
    let name = program.display();
    if !program.is_absolute() {
        return Err(format!("{name}: not an absolute path"));
    }
    let file = std::fs::metadata(program).map_err(|err| format!("{name}: {err}"))?;
    if !file.is_file() {
        return Err(format!("{name}: not a file"));
    }
    rustix::fs::access(program, rustix::fs::Access::EXEC_OK)
        .map_err(|_| format!("{name}: not executable"))
}

/// What `ask` is refused for before the loop hears it, if it is: a
/// program given that cannot run.
pub fn refusal(ask: &ProgramAsk) -> Option<String> {
    match ask {
        ProgramAsk::Restart(_, Restart::Given(program)) => check(program).err().map(refused),
        _ => None,
    }
}

impl State {
    /// `hocus msg shell status|restart`: the answer.
    pub(crate) fn program_asked(&mut self, ask: ProgramAsk) -> String {
        match ask {
            ProgramAsk::Status(Supervised::Shell) => self.shell_status(),
            ProgramAsk::Restart(which @ Supervised::Shell, restart) => {
                if matches!(restart, Restart::Given(_)) && !self.config.developer.live_reload {
                    tracing::info!(
                        program = which.word(),
                        "a program given is refused: the developer mode is off"
                    );
                    return refused(self.live_reload_off());
                }
                if let Err(why) = self.shell_restartable() {
                    tracing::info!(
                        program = which.word(),
                        why,
                        "a restart asked for is refused"
                    );
                    return refused(why);
                }
                self.shell.given.asked(restart);
                self.restart_shell();
                let program = self.shell_program().unwrap_or_default();
                format!("restarting\nprogram {}\n", program.display())
            }
        }
    }

    /// Why a program given is refused, and how to have it accepted: the
    /// setting, in the file this session reads.
    fn live_reload_off(&self) -> String {
        let file = self.config.file.as_deref().map_or_else(
            || "the configuration file".to_owned(),
            |file| file.display().to_string(),
        );
        format!(
            "developer mode is off, so no other program is run as the shell. \
             To allow it, set `live-reload = true` under `[developer]` in {file} \
             (followed at once)"
        )
    }

    /// `[developer]` changed. Turned off while a program given runs, the
    /// installed one runs again at once: what the mode allowed ends with
    /// it.
    pub(crate) fn developer_changed(&mut self, new: &crate::config::Developer) -> bool {
        if *new == self.config.developer {
            return false;
        }
        self.config.developer = new.clone();
        if !new.live_reload && self.shell.given.program().is_some() {
            tracing::info!("the developer mode is off: the installed shell runs again");
            self.shell.given.asked(Restart::Installed);
            if self.shell_restartable().is_ok() {
                self.restart_shell();
            }
        }
        true
    }

    /// The program given as `which` was let go of: logged, and said by
    /// the shell once it is there (its connection, if it is still there,
    /// is the one that ended; the next one's is told).
    pub(crate) fn fell_back(&mut self, which: Supervised, program: PathBuf, why: Why) {
        tracing::warn!(
            which = which.word(),
            program = %program.display(),
            why = why.says(),
            "the program given for this session is let go of: the installed one runs again"
        );
        self.shell
            .notices
            .push(ToShell::FellBack { which, program });
    }

    /// What the shell was to be told once it is there.
    pub(crate) fn tell_notices(&mut self) {
        if !self.shell_linked() {
            return;
        }
        for notice in std::mem::take(&mut self.shell.notices) {
            self.tell_shell(notice);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STEADY: Duration = Duration::from_secs(30);

    fn given(program: &str) -> Given {
        let mut given = Given::default();
        given.asked(Restart::Given(program.into()));
        given
    }

    #[test]
    fn a_given_program_that_keeps_ending_is_let_go_of() {
        let mut given = given("/dev/shell");
        let quick = Some(Duration::from_secs(1));
        // The installed one's ends are not its own.
        assert_eq!(
            given.ended(Path::new("/usr/bin/shell"), quick, STEADY),
            None
        );
        for _ in 1..QUICK_ENDS {
            assert_eq!(given.ended(Path::new("/dev/shell"), quick, STEADY), None);
        }
        assert_eq!(
            given.ended(Path::new("/dev/shell"), quick, STEADY),
            Some(Why::KeptEnding)
        );
        assert_eq!(given.program(), None);
        assert_eq!(
            given.fell_back,
            Some((PathBuf::from("/dev/shell"), Why::KeptEnding))
        );
    }

    #[test]
    fn one_that_ran_long_starts_the_count_again_and_one_that_does_not_start_goes_at_once() {
        let mut given = given("/dev/shell");
        let program = Path::new("/dev/shell");
        let quick = Some(Duration::from_secs(1));
        for _ in 1..QUICK_ENDS {
            assert_eq!(given.ended(program, quick, STEADY), None);
        }
        assert_eq!(given.ended(program, Some(STEADY), STEADY), None);
        assert_eq!(given.ended(program, quick, STEADY), None);
        assert_eq!(given.program(), Some(program));
        assert_eq!(given.ended(program, None, STEADY), Some(Why::DidNotStart));
        assert_eq!(given.program(), None);
        // Asked for again: what was said of the last one goes.
        given.asked(Restart::Same);
        assert_eq!(given.fell_back, None);
    }

    #[test]
    fn a_program_given_is_an_absolute_path_to_something_that_runs() -> anyhow::Result<()> {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir()?;
        let program = dir.path().join("program");
        assert!(check(Path::new("program")).is_err(), "relative");
        assert!(check(&program).is_err(), "not there");
        assert!(check(dir.path()).is_err(), "a folder");
        std::fs::write(&program, "#!/bin/sh\n")?;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o644))?;
        assert!(check(&program).is_err(), "not executable");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
        assert_eq!(check(&program), Ok(()));
        let ask = ProgramAsk::Restart(Supervised::Shell, Restart::Given("shell".into()));
        assert!(refusal(&ask).is_some_and(|r| r.starts_with(REFUSED)));
        Ok(())
    }
}
