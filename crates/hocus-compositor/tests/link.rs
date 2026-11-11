//! The line to the shell opens with a hello each way (issue #774,
//! `hocus_link::wire`): a shell of the compositor's version is fed, one of
//! another version (a package upgraded under the session) is not, and is
//! started again from the installed program, once; a second one leaves the
//! session without a shell, its bars' space kept.
//!
//! The shell here is the test's own: the program the compositor starts
//! only waits, and a thread speaks on the shell's socket in its place, in
//! whatever version the test says.

use std::{
    io::{BufRead as _, BufReader, Write as _},
    os::unix::{fs::PermissionsExt as _, net::UnixStream},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use anyhow::Context as _;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use hocus_link::{ToCompositor, ToShell, VERSION, wire};

const WAIT: Duration = Duration::from_secs(30);

/// A compositor whose shell's program only waits to be ended.
fn compositor(dir: &Path) -> anyhow::Result<Headless> {
    let program = dir.join("shell");
    std::fs::write(&program, "#!/bin/sh\nexec sleep 600\n")?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755))?;
    Headless::start_with_shell(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(program),
    )
}

/// The shell's socket, beside the Wayland one.
fn shell_socket(hp: &Headless) -> PathBuf {
    let mut path = hp.socket().as_os_str().to_owned();
    path.push(".shell");
    PathBuf::from(path)
}

/// A shell's end of the line: what it says, and the lines it hears, read
/// on a thread.
struct Speaks {
    says: UnixStream,
    hears: Receiver<String>,
}

impl Speaks {
    /// Connects as the shell, and says `first` if there is one.
    fn connect(hp: &Headless, first: Option<&str>) -> anyhow::Result<Self> {
        let says = UnixStream::connect(shell_socket(hp)).context("the shell's socket")?;
        let reads = says.try_clone()?;
        let (said, hears) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(reads).lines() {
                let Ok(line) = line else { break };
                if said.send(line).is_err() {
                    break;
                }
            }
        });
        let mut speaks = Self { says, hears };
        if let Some(first) = first {
            speaks.say(first)?;
        }
        Ok(speaks)
    }

    fn say(&mut self, line: &str) -> anyhow::Result<()> {
        self.says.write_all(format!("{line}\n").as_bytes())?;
        Ok(())
    }

    /// The next line heard, the loop running meanwhile.
    fn hear(&self, hp: &mut Headless) -> anyhow::Result<String> {
        let mut heard = None;
        hp.dispatch_until(WAIT, |_| {
            if heard.is_none() {
                heard = self.hears.try_recv().ok();
            }
            heard.is_some()
        })?;
        heard.context("the compositor said nothing")
    }

    /// Whether anything more is heard within `time`.
    fn hears_more(&self, hp: &mut Headless, time: Duration) -> anyhow::Result<Option<String>> {
        let mut heard = None;
        hp.dispatch_until(time, |_| {
            if heard.is_none() {
                heard = self.hears.try_recv().ok();
            }
            heard.is_some()
        })?;
        Ok(heard)
    }
}

impl Drop for Speaks {
    fn drop(&mut self) {
        // Its reading thread's copy too.
        let _ = self.says.shutdown(std::net::Shutdown::Both);
    }
}

/// The shell's status, as `hocus msg shell status` asks it.
fn status(hp: &mut Headless) -> anyhow::Result<String> {
    let path = hp
        .control_socket()
        .context("no control socket")?
        .to_path_buf();
    let (said, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let asked = (|| -> std::io::Result<String> {
            use std::io::Read as _;
            let mut stream = UnixStream::connect(&path)?;
            stream.set_read_timeout(Some(WAIT))?;
            stream.write_all(b"msg shell status\n")?;
            let mut answer = String::new();
            stream.read_to_string(&mut answer)?;
            Ok(answer)
        })();
        let _ = said.send(asked);
    });
    let mut answer = None;
    hp.dispatch_until(WAIT, |_| {
        if answer.is_none() {
            answer = heard.try_recv().ok();
        }
        answer.is_some()
    })?;
    Ok(answer.context("no answer")??)
}

/// The shell's process started, other than `before`.
fn started(hp: &mut Headless, before: Option<i32>) -> anyhow::Result<i32> {
    let there = hp.dispatch_until(WAIT, |hp| {
        hp.shell_pid().is_some() && hp.shell_pid() != before
    })?;
    anyhow::ensure!(there, "no shell started:\n{}", status(hp)?);
    hp.shell_pid().context("no shell")
}

#[test]
fn a_shell_of_the_same_version_is_fed_after_the_hellos_and_an_unknown_kind_is_skipped()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = compositor(dir.path())?;
    started(&mut hp, None)?;
    // Connected, and no hello yet: it is told nothing.
    let quiet = Speaks::connect(&hp, None)?;
    assert_eq!(quiet.hears_more(&mut hp, Duration::from_millis(300))?, None);
    assert!(!hp.shell_connected());
    drop(quiet);

    let mut shell = Speaks::connect(&hp, Some(&wire::hello()))?;
    // The compositor's hello first, then what a shell is told as it
    // connects: every line a message.
    assert_eq!(shell.hear(&mut hp)?, wire::hello());
    let look = shell.hear(&mut hp)?;
    assert!(
        matches!(ToShell::read(&look), Ok(ToShell::Look { .. })),
        "{look}"
    );
    assert!(hp.shell_connected());
    let mut bars = 0;
    while let Some(line) = shell.hears_more(&mut hp, Duration::from_millis(500))? {
        let message = ToShell::read(&line).map_err(|err| anyhow::anyhow!("{line}: {err}"))?;
        bars += usize::from(matches!(message, ToShell::Bar { .. }));
    }
    assert_eq!(bars, 1, "one screen, one bar");

    // A kind this compositor does not know, a line that is no message:
    // skipped, and the next one is heard.
    shell.say(r#"{"kind":"teleport","data":{"to":"Mars"}}"#)?;
    shell.say("not a message")?;
    assert!(!hp.shell_on_cpu());
    shell.say(&ToCompositor::Software.line())?;
    let heard = hp.dispatch_until(WAIT, |hp| hp.shell_on_cpu())?;
    assert!(heard, "the line after an unknown kind was not heard");
    assert!(hp.shell_connected());
    let status = status(&mut hp)?;
    assert!(status.contains("state running\n"), "{status}");
    assert!(!status.contains("mismatch"), "{status}");
    Ok(())
}

#[test]
fn a_shell_of_another_version_is_not_fed_started_again_once_then_none_runs() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let mut hp = compositor(dir.path())?;
    let area = hp.work_area();
    let first = started(&mut hp, None)?;

    // A newer shell under this compositor: answered with the
    // compositor's version, told nothing more, and what it says is not
    // heard.
    let mut newer = Speaks::connect(&hp, Some(&wire::hello_of(VERSION + 1)))?;
    assert_eq!(newer.hear(&mut hp)?, wire::hello());
    newer.say(&ToCompositor::Software.line())?;
    assert_eq!(
        newer.hears_more(&mut hp, Duration::from_millis(300))?,
        None,
        "a shell of another version was fed"
    );
    assert!(!hp.shell_connected());
    assert!(!hp.shell_on_cpu(), "a shell of another version was heard");

    // Its program is started again, once.
    let second = started(&mut hp, Some(first))?;
    let said = status(&mut hp)?;
    assert!(said.contains("starts 2\n"), "{said}");
    assert!(!said.contains("mismatch"), "{said}");
    drop(newer);

    // That one speaks the words of before version 1 (no hello at all):
    // not fed either, and none is started after it.
    let old = Speaks::connect(&hp, Some("software"))?;
    assert_eq!(old.hear(&mut hp)?, wire::hello());
    assert_eq!(old.hears_more(&mut hp, Duration::from_millis(300))?, None);
    let ended = hp.dispatch_until(WAIT, |hp| hp.shell_pid().is_none())?;
    assert!(ended, "the second shell was not ended");
    let again = hp.dispatch_until(Duration::from_secs(2), |hp| hp.shell_pid().is_some())?;
    assert!(!again, "a third shell was started");
    assert_ne!(hp.shell_pid(), Some(second));
    assert!(!hp.shell_connected());
    let said = status(&mut hp)?;
    assert!(said.contains("state off\n"), "{said}");
    assert!(said.contains("starts 2\n"), "{said}");
    assert!(
        said.contains(&format!("mismatch shell none compositor {VERSION}\n")),
        "{said}"
    );
    // The bars' space is kept all the same.
    assert_eq!(hp.work_area(), area);
    assert!(area.is_some());
    Ok(())
}
