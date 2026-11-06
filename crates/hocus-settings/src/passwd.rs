//! The password changed through the system's own `passwd` (6.11), as the
//! system's tools change it: on a pseudo-terminal of its own, its prompts
//! answered in turn (the current password, the new one, the new one
//! again), so that PAM's rules, its quality checks and its other modules
//! apply as they do at the terminal. Run on a thread of its own, never the
//! window's: it blocks while `passwd` and PAM take their time.
//!
//! Its prompts are said in the session's language, so they are not read:
//! a prompt is what `passwd` writes without ending its line (`…: `), and
//! the answers go in their order. What it says between the new password
//! and the next prompt is PAM's refusal (`BAD PASSWORD: …`); ending with
//! an error before asking for the new one is the current one refused.
//!
//! The three values leave the process only on the terminal; nothing of
//! them is logged. A read that waits more than [`PATIENCE`] ends it.

use std::{
    ffi::OsStr,
    io::{Read as _, Write as _},
    os::unix::{
        ffi::OsStrExt as _,
        fs::OpenOptionsExt as _,
        process::{CommandExt as _, ExitStatusExt as _},
    },
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use crate::secret::Secret;

/// How long `passwd` is waited for, at each step.
pub const PATIENCE: Duration = Duration::from_secs(10);

/// How it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Changed,
    /// The current password is not the one given.
    WrongCurrent,
    /// The new one refused by the system's rules: their reason, in the
    /// session's language.
    Refused(String),
    /// Anything else: what `passwd` said last.
    Failed(String),
    /// `passwd` stopped answering.
    NoAnswer,
}

/// `passwd`, if the system has it on the `PATH`.
pub fn program() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("passwd"))
        .find(|p| p.is_file())
}

/// `current` changed for `new`, now. Blocks until `passwd` is done.
pub fn change(current: &Secret, new: &Secret) -> Outcome {
    match run(current, new) {
        Ok(outcome) => outcome,
        Err(err) => {
            tracing::warn!(%err, "passwd could not run");
            Outcome::Failed(err.to_string())
        }
    }
}

fn run(current: &Secret, new: &Secret) -> std::io::Result<Outcome> {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    let program = program().ok_or_else(|| std::io::Error::other("no passwd"))?;
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY | OpenptFlags::CLOEXEC)?;
    grantpt(&master)?;
    unlockpt(&master)?;
    let name = ptsname(&master, Vec::new())?;
    let terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(rustix::fs::OFlags::NOCTTY.bits() as i32)
        .open(OsStr::from_bytes(name.as_bytes()))?;
    // No echo, whatever `passwd` does: what is typed never comes back to
    // be read as something it said.
    let mut modes = rustix::termios::tcgetattr(&terminal)?;
    modes.local_modes.remove(rustix::termios::LocalModes::ECHO);
    rustix::termios::tcsetattr(&terminal, rustix::termios::OptionalActions::Now, &modes)?;
    let mut command = Command::new(program);
    command
        .stdin(Stdio::from(terminal.try_clone()?))
        .stdout(Stdio::from(terminal.try_clone()?))
        .stderr(Stdio::from(terminal));
    #[allow(unsafe_code)]
    // SAFETY: the closure runs in the child between fork and exec: it makes
    // two system calls through rustix, allocates nothing and takes no
    // lock, as `pre_exec` requires.
    unsafe {
        command.pre_exec(|| {
            // Its own session, the terminal its controlling one: what
            // PAM's conversation reads from.
            rustix::process::setsid()?;
            // SAFETY: fd 0 is open in the child: the terminal, its stdin.
            let stdin = std::os::fd::BorrowedFd::borrow_raw(0);
            rustix::process::ioctl_tiocsctty(stdin)?;
            Ok(())
        });
    }
    let mut child = command.spawn()?;
    // The terminal's ends of this process closed: reading the master ends
    // once `passwd` is gone.
    drop(command);
    let mut master = std::fs::File::from(master);
    let answers = [current.as_str(), new.as_str(), new.as_str()];
    let mut answered = 0;
    // What it said since the last answer: whole lines, and the line it is
    // writing.
    let mut lines: Vec<String> = Vec::new();
    let mut partial: Vec<u8> = Vec::new();
    let mut buffer = [0u8; 1024];
    let stop = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
    };
    loop {
        if !readable(&master, PATIENCE)? {
            tracing::warn!("passwd stopped answering");
            stop(&mut child);
            return Ok(Outcome::NoAnswer);
        }
        let read = match master.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => read,
            // The terminal's other end closed: `passwd` is gone.
            Err(err) if err.raw_os_error() == Some(rustix::io::Errno::IO.raw_os_error()) => break,
            Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(err) => return Err(err),
        };
        partial.extend_from_slice(&buffer[..read]);
        while let Some(at) = partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = partial.drain(..=at).collect();
            let line = String::from_utf8_lossy(&line).trim().to_owned();
            if !line.is_empty() {
                lines.push(line);
            }
        }
        let prompt = String::from_utf8_lossy(&partial);
        if !prompt.trim_end().ends_with(':') {
            continue;
        }
        // A prompt. After the new password, what was said meanwhile is a
        // refusal, and the prompt asks for another.
        if answered >= 2 && !lines.is_empty() {
            stop(&mut child);
            return Ok(Outcome::Refused(reason(&lines)));
        }
        let Some(answer) = answers.get(answered) else {
            stop(&mut child);
            return Ok(Outcome::Failed(lines.last().cloned().unwrap_or_default()));
        };
        master.write_all(answer.as_bytes())?;
        master.write_all(b"\n")?;
        master.flush()?;
        answered += 1;
        lines.clear();
        partial.clear();
    }
    let status = wait(&mut child)?;
    let Some(status) = status else {
        stop(&mut child);
        return Ok(Outcome::NoAnswer);
    };
    if status.success() {
        return Ok(Outcome::Changed);
    }
    tracing::info!(
        code = status.code(),
        signal = status.signal(),
        answered,
        "passwd refused"
    );
    Ok(match answered {
        0 | 1 => Outcome::WrongCurrent,
        2 if !lines.is_empty() => Outcome::Refused(reason(&lines)),
        _ => Outcome::Failed(lines.last().cloned().unwrap_or_default()),
    })
}

/// Whether `master` has something to read within `patience`.
fn readable(master: &std::fs::File, patience: Duration) -> std::io::Result<bool> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let until = Instant::now() + patience;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(false);
        }
        let timeout = Timespec {
            tv_sec: i64::try_from(left.as_secs()).unwrap_or(i64::MAX),
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [PollFd::new(master, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) => return Ok(false),
            Ok(_) => return Ok(true),
            Err(rustix::io::Errno::INTR) => {}
            Err(err) => return Err(err.into()),
        }
    }
}

/// `passwd`'s end, waited for [`PATIENCE`] at most.
fn wait(child: &mut std::process::Child) -> std::io::Result<Option<std::process::ExitStatus>> {
    let until = Instant::now() + PATIENCE;
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(Some(status));
        }
        if Instant::now() >= until {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// PAM's reason, without its shouting prefix: "BAD PASSWORD: The password
/// is shorter than 8 characters" says "The password is shorter than 8
/// characters".
fn reason(lines: &[String]) -> String {
    let said = lines.join(" ");
    match said.split_once(':') {
        Some((head, rest))
            if !rest.trim().is_empty()
                && head.chars().any(char::is_alphabetic)
                && !head.chars().any(char::is_lowercase) =>
        {
            rest.trim().to_owned()
        }
        _ => said,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pams_reason_is_said_without_its_prefix() {
        assert_eq!(
            reason(&["BAD PASSWORD: The password is shorter than 8 characters".to_owned()]),
            "The password is shorter than 8 characters"
        );
        assert_eq!(
            reason(&["MOT DE PASSE INCORRECT : trop simple".to_owned()]),
            "trop simple"
        );
        assert_eq!(
            reason(&["passwd: Authentication token manipulation error".to_owned()]),
            "passwd: Authentication token manipulation error"
        );
    }
}
