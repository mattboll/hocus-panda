//! The notifications' server and their bubbles (5.3, issue #393): the real
//! shell serving `org.freedesktop.Notifications` on a D-Bus daemon of the
//! test's own (never the session's), told by a client of the test's.
//!
//! One test per point of the approved spec: a bubble shows against the bar
//! and goes after its time; hovering pauses it; a critical one stays; a low
//! one goes to the history, the bar's dot counting it; a replacement
//! updates its bubble in place; more than three wait under a "+N" chip;
//! the actions, the card and the ✕ say what they should; an agents' tool
//! saying its agent waits makes no bubble, its other notifications do.
//!
//! And 5.4's (issue #394), one per point of its spec: the history lists by
//! sections, newest first; a click in it invokes the default, raises and
//! removes; an expired bubble's action still works from it (a bubble's time
//! up closes nothing: the notification stays held); its ✕ removes one, "clear all" empties it; the bell's badge
//! counts what was not seen and opening the history resets it; do not
//! disturb holds a normal bubble but not a critical one; the history
//! survives a restart of the shell without what is older than 7 days. The
//! shell's wall clock reads 2 November 2026, 14:32:07 UTC
//! (`HOCUS_CLOCK_AT`); do not disturb ending at its time is the stack's
//! unit test, on the clock it is given.
//!
//! Pixman here, so the shell draws on the CPU: each bubble's surface is its
//! card, and nothing moves (as in CI).

mod common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_ESC};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};
use zbus::{MatchRule, MessageStream, OwnedVariant, Variant};

/// The full bar's right edge's distance from the screen's, and the gap.
const CARDS_RIGHT: i32 = 1920 - 8 - 196 - 8;

/// What the client heard: `closed ID REASON`, `action ID KEY`.
type Heard = Arc<Mutex<Vec<String>>>;

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    /// The shell's process id, as its script writes it.
    pid: PathBuf,
    /// The test's own D-Bus daemon, stopped by its process id.
    bus: Child,
    runtime: tokio::runtime::Runtime,
    client: zbus::Connection,
    heard: Heard,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

fn ready() -> Option<PathBuf> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    let daemon = Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        eprintln!("no dbus-daemon: skipped");
        return None;
    }
    Some(shell)
}

/// A D-Bus daemon of the test's own in `dir`, and its address.
fn private_bus(dir: &Path) -> anyhow::Result<(Child, String)> {
    let socket = dir.join("bus");
    let config = dir.join("bus.conf");
    std::fs::write(
        &config,
        format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig><type>session</type><listen>unix:path={}</listen>\
             <policy context=\"default\"><allow send_destination=\"*\"/>\
             <allow receive_sender=\"*\"/><allow own=\"*\"/></policy></busconfig>\n",
            socket.display()
        ),
    )?;
    let bus = Command::new("dbus-daemon")
        .arg(format!("--config-file={}", config.display()))
        .arg("--nofork")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let start = Instant::now();
    while !socket.exists() {
        anyhow::ensure!(start.elapsed() < Duration::from_secs(10), "no bus");
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok((bus, format!("unix:path={}", socket.display())))
}

/// The bed: the compositor on a 1920×1080 screen (the full bar), its shell
/// serving the notifications on the test's bus; `config` changes the
/// compositor's configuration (the agents' tools' places).
fn start(shell: &Path, config: impl FnOnce(&Path, &mut Config)) -> anyhow::Result<Bed> {
    start_with(shell, config, "")
}

/// The wall clock the shell reads (`HOCUS_CLOCK_AT`, in UTC) as it starts:
/// 2 November 2026, 14:32:07.
const NOW_MS: i64 = 1_793_629_927_000;
const MINUTE_MS: i64 = 60_000;

/// The same, the history kept before holding `kept` (its file's lines).
fn start_with(
    shell: &Path,
    config: impl FnOnce(&Path, &mut Config),
    kept: &str,
) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    let state = dir.path().join("state");
    for sub in [&home, &state.join("hocuspanda")] {
        std::fs::create_dir_all(sub)?;
    }
    std::fs::write(state.join("hocuspanda/notifications"), kept)?;
    let (bus, address) = private_bus(dir.path())?;
    let log = dir.path().join("shell.log");
    let pid = dir.path().join("shell.pid");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
             XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-02T14:32:07' \
             RUST_LOG=warn,hocuspanda_shell=debug,hocuspanda_shell::notifications::bell=trace\necho $$ > '{pid}'\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            state = state.display(),
            pid = pid.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut settings = Config::default();
    config(&home, &mut settings);
    let mut hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
        Some(address.clone()),
    )?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let client = runtime.block_on(zbus::connection::Builder::address(address.as_str()).build())?;
    let heard = Heard::default();
    runtime.block_on(listen(&client, heard.clone()))?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    let mut bed = Bed {
        _dir: dir,
        hp,
        log,
        pid,
        bus,
        runtime,
        client,
        heard,
    };
    bed.logged("the notifications are served")?;
    bed.hp.pointer_move(400.0, 600.0);
    bed.idle(Duration::from_millis(300))?;
    Ok(bed)
}

/// The server's signals, as `closed ID REASON` and `action ID KEY`.
async fn listen(client: &zbus::Connection, heard: Heard) -> anyhow::Result<()> {
    let rule = MatchRule::builder()
        .msg_type(zbus::message::Type::Signal)
        .interface("org.freedesktop.Notifications")
        .build()?;
    let stream = MessageStream::for_match_rule(rule, client, None).await?;
    tokio::spawn(async move {
        use zbus::export::futures_core::Stream;
        let mut stream = Box::pin(stream);
        while let Some(Ok(message)) = std::future::poll_fn(|cx| stream.as_mut().poll_next(cx)).await
        {
            let header = message.header();
            let said = match header.member().map(|m| m.as_str().to_owned()).as_deref() {
                Some("NotificationClosed") => message
                    .body()
                    .deserialize::<(u32, u32)>()
                    .map(|(id, reason)| format!("closed {id} {reason}")),
                Some("ActionInvoked") => message
                    .body()
                    .deserialize::<(u32, String)>()
                    .map(|(id, key)| format!("action {id} {key}")),
                _ => continue,
            };
            if let Ok(said) = said {
                heard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(said);
            }
        }
    });
    Ok(())
}

/// A notification to send.
struct Notice<'a> {
    app: &'a str,
    replaces: u32,
    summary: &'a str,
    body: &'a str,
    actions: &'a [&'a str],
    urgency: Option<u8>,
    progress: Option<i32>,
    timeout: i32,
}

impl Default for Notice<'_> {
    fn default() -> Self {
        Self {
            app: "Slack",
            replaces: 0,
            summary: "Camille dans #design",
            body: "J'ai mis à jour la maquette du lanceur.",
            actions: &[],
            urgency: None,
            progress: None,
            timeout: -1,
        }
    }
}

impl Bed {
    /// Sends `notice`; its id.
    fn notify(&mut self, notice: Notice<'_>) -> anyhow::Result<u32> {
        let mut hints: HashMap<&str, OwnedVariant> = HashMap::new();
        if let Some(urgency) = notice.urgency {
            hints.insert("urgency", OwnedVariant::try_from(Variant::U8(urgency))?);
        }
        if let Some(value) = notice.progress {
            hints.insert("value", OwnedVariant::try_from(Variant::I32(value))?);
        }
        let body = (
            notice.app,
            notice.replaces,
            "",
            notice.summary,
            notice.body,
            notice.actions.to_vec(),
            hints,
            notice.timeout,
        );
        let client = self.client.clone();
        let id = self.runtime.block_on(async move {
            let reply = client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "Notify",
                    &body,
                )
                .await?;
            anyhow::Ok(reply.body().deserialize::<u32>()?)
        })?;
        Ok(id)
    }

    fn close(&mut self, id: u32) -> anyhow::Result<()> {
        let client = self.client.clone();
        self.runtime.block_on(async move {
            client
                .call_method(
                    Some("org.freedesktop.Notifications"),
                    "/org/freedesktop/Notifications",
                    Some("org.freedesktop.Notifications"),
                    "CloseNotification",
                    &(id,),
                )
                .await
        })?;
        Ok(())
    }

    /// The compositor runs for `time`.
    fn idle(&mut self, time: Duration) -> anyhow::Result<()> {
        self.hp.dispatch_until(time, |_| false)?;
        Ok(())
    }

    /// The bubbles' cards, top first.
    fn bubbles(&self) -> Vec<Rect> {
        self.hp
            .bubble_surfaces()
            .into_iter()
            .map(|(_, _, r)| r)
            .collect()
    }

    /// Waits until `n` bubbles show and are drawn; their cards.
    fn shows(&mut self, n: usize) -> anyhow::Result<Vec<Rect>> {
        let done = self.hp.dispatch_until(Duration::from_secs(10), |hp| {
            hp.bubble_surfaces().len() == n && hp.bubble_commits() >= n as u64
        })?;
        anyhow::ensure!(done, "not {n} bubbles: {:?}", self.bubbles());
        // Their surfaces at their size.
        self.idle(Duration::from_millis(200))?;
        Ok(self.bubbles())
    }

    /// What the client heard so far.
    fn heard(&self) -> Vec<String> {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Waits until the client heard `said`.
    fn hears(&mut self, said: &str) -> anyhow::Result<()> {
        let heard = self.heard.clone();
        let done = self.hp.dispatch_until(Duration::from_secs(10), |_| {
            heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .any(|h| h == said)
        })?;
        anyhow::ensure!(done, "not heard {said:?}: {:?}", self.heard());
        Ok(())
    }

    /// What the shell logged, without its colours.
    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Waits until the shell logged `what`.
    fn logged(&mut self, what: &str) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if self.log().contains(what) {
                return Ok(());
            }
            self.idle(Duration::from_millis(50))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.log())
    }

    fn click(&mut self, (x, y): (f64, f64)) {
        self.hp.pointer_move(x, y);
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
    }

    /// Waits until notification `id`'s bubble went after its time: its
    /// sender hears nothing, the notification still held (`persistence`).
    fn expires(&mut self, id: u32) -> anyhow::Result<()> {
        let start = Instant::now();
        let ended = format!("id={id}");
        while start.elapsed() < Duration::from_secs(15) {
            let up = self
                .log()
                .lines()
                .any(|l| l.contains("a bubble's time is up") && l.ends_with(&ended));
            if up {
                // Its fading out over, nothing said.
                self.idle(Duration::from_millis(300))?;
                let closed = format!("closed {id} ");
                anyhow::ensure!(
                    !self.heard().iter().any(|h| h.starts_with(&closed)),
                    "{:?}",
                    self.heard()
                );
                return Ok(());
            }
            self.idle(Duration::from_millis(50))?;
        }
        anyhow::bail!("{id}'s time was not up:\n{}", self.log())
    }

    /// Where the log is now: what it says after is new.
    fn mark(&self) -> usize {
        self.log().len()
    }

    /// Waits until the shell logged `what` after `mark`; the line.
    fn logged_since(&mut self, mark: usize, what: &str) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let log = self.log();
            if let Some(line) = log
                .get(mark..)
                .and_then(|new| new.lines().find(|l| l.contains(what)))
            {
                return Ok(line.to_owned());
            }
            self.idle(Duration::from_millis(50))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.log())
    }

    /// The bell's button's centre, on the bar: the full form's footer's
    /// second button (5.5), 40 × 34, 11 px and 6 px into its surface, the
    /// buttons 4 px apart.
    fn bell(&mut self) -> anyhow::Result<(f64, f64)> {
        let shown = self
            .hp
            .dispatch_until(Duration::from_secs(10), |hp| hp.bell_surfaces().len() == 1)?;
        anyhow::ensure!(shown, "no bell");
        let (_, at) = self.hp.bell_surfaces()[0];
        Ok((f64::from(at.x + 11 + 44 + 20), f64::from(at.y + 6 + 17)))
    }

    /// Opens the history with a click on the bell: what it shows, as
    /// logged.
    fn open_history(&mut self) -> anyhow::Result<String> {
        let bell = self.bell()?;
        let mark = self.mark();
        self.hp.pointer_move(bell.0, bell.1);
        self.idle(Duration::from_millis(100))?;
        self.click(bell);
        let shows = self.logged_since(mark, "the history shows")?;
        self.logged_since(mark, "the history's items")?;
        let open = self.hp.dispatch_until(Duration::from_secs(5), |hp| {
            hp.popover_surfaces().len() == 1
        })?;
        anyhow::ensure!(open, "the history did not open");
        self.idle(Duration::from_millis(200))?;
        Ok(shows)
    }

    /// The history's popover, on the screen.
    fn history(&self) -> Rect {
        self.hp.popover_surfaces()[0].1
    }

    /// Where `name` (`Clear`, `Switch`, `Item(3)`, `Close(3)`…) is on the
    /// screen, as the history last logged its items.
    fn history_item(&self, name: &str) -> anyhow::Result<(f64, f64)> {
        let log = self.log();
        let line = log
            .lines()
            .rev()
            .find(|l| l.contains("the history's items"))
            .ok_or_else(|| anyhow::anyhow!("no items logged"))?;
        let at = line
            .split_whitespace()
            .find_map(|word| {
                let word = word.strip_prefix("items=").unwrap_or(word);
                let (who, xy) = word.rsplit_once('@')?;
                let (x, y) = xy.split_once(',')?;
                (who == name).then(|| (x.parse::<f64>().ok(), y.parse::<f64>().ok()))
            })
            .and_then(|(x, y)| Some((x?, y?)))
            .ok_or_else(|| anyhow::anyhow!("no {name} in {line}"))?;
        let popover = self.history();
        Ok((f64::from(popover.x) + at.0, f64::from(popover.y) + at.1))
    }

    /// The items the history lists, by their names in its last line of
    /// items (`Item(3)`…), top first.
    fn history_items(&self) -> Vec<String> {
        let log = self.log();
        let Some(line) = log
            .lines()
            .rev()
            .find(|l| l.contains("the history's items"))
        else {
            return Vec::new();
        };
        line.split_whitespace()
            .filter_map(|w| w.strip_prefix("items=").or(Some(w)))
            .filter(|w| w.starts_with("Item("))
            .filter_map(|w| w.split_once('@').map(|(who, _)| who.to_owned()))
            .collect()
    }

    /// Closes the history (`Esc`).
    fn close_history(&mut self) -> anyhow::Result<()> {
        self.hp.key(KEY_ESC, true);
        self.hp.key(KEY_ESC, false);
        let closed = self.hp.dispatch_until(Duration::from_secs(5), |hp| {
            hp.popover_surfaces().is_empty()
        })?;
        anyhow::ensure!(closed, "the history did not close");
        Ok(())
    }
}

/// A line of the history's file: kept `ago` minutes before the shell's
/// clock started, of `app`, `seen` or not.
fn kept(app: &str, summary: &str, ago: i64, unseen: bool) -> String {
    format!(
        "{}\n",
        serde_json::json!({
            "app": app,
            "entry": null,
            "app_icon": "",
            "image_path": null,
            "summary": summary,
            "body": "",
            "pid": 0,
            "agent": false,
            "at": NOW_MS - ago * MINUTE_MS,
            "unseen": unseen,
        })
    )
}

/// A card's ✕: 8 px in from its top right, inside the rim, 24 px.
fn close_of(card: Rect) -> (f64, f64) {
    (
        f64::from(card.x + card.width - 1 - 8 - 12),
        f64::from(card.y + 1 + 8 + 12),
    )
}

#[test]
fn a_bubble_shows_against_the_bar_then_goes_after_its_time() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let id = bed.notify(Notice {
        timeout: 1500,
        ..Notice::default()
    })?;
    let shown = Instant::now();
    let cards = bed.shows(1)?;
    // 340 wide, its top 8 px from the screen's, 8 px from the full bar.
    assert_eq!((cards[0].width, cards[0].y), (340, 8));
    assert_eq!(cards[0].x + cards[0].width, CARDS_RIGHT);
    bed.logged("title=\"Camille dans #design\"")?;
    bed.expires(id)?;
    assert!(
        shown.elapsed() >= Duration::from_millis(1300),
        "{:?}",
        shown.elapsed()
    );
    let gone = bed
        .hp
        .dispatch_until(Duration::from_secs(5), |hp| hp.bubble_surfaces().is_empty())?;
    assert!(gone, "{:?}", bed.bubbles());
    Ok(())
}

#[test]
fn hovering_a_bubble_pauses_its_countdown() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let id = bed.notify(Notice {
        timeout: 1200,
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.hp
        .pointer_move(f64::from(card.x + 60), f64::from(card.y + 30));
    bed.logged("paused=true")?;
    bed.idle(Duration::from_millis(2500))?;
    assert_eq!(bed.bubbles().len(), 1, "it went while hovered");
    assert!(bed.heard().is_empty(), "{:?}", bed.heard());
    // Away from it, it goes after what was left.
    bed.hp.pointer_move(400.0, 600.0);
    bed.logged("paused=false")?;
    bed.expires(id)?;
    Ok(())
}

#[test]
fn a_critical_one_stays_until_closed() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let id = bed.notify(Notice {
        app: "Batterie",
        summary: "Batterie très faible : 5 %",
        body: "Branche le chargeur.",
        urgency: Some(2),
        timeout: 800,
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.logged("critical=true")?;
    bed.idle(Duration::from_millis(2500))?;
    assert_eq!(bed.bubbles().len(), 1);
    assert!(bed.heard().is_empty(), "{:?}", bed.heard());
    // Its ✕ closes it, dismissed by the user.
    bed.click(close_of(card));
    bed.hears(&format!("closed {id} 2"))?;
    Ok(())
}

#[test]
fn a_low_one_goes_to_the_history_and_the_bar_counts_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    bed.notify(Notice {
        app: "Spotify",
        summary: "Morceau suivant",
        body: "",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged("unread=1")?;
    // The bell, on the bar at its bottom, counts it.
    let (x, y) = bed.bell()?;
    assert!(
        x > f64::from(1920 - 8 - 196) && y > f64::from(1080 - 60),
        "{x}, {y}"
    );
    bed.idle(Duration::from_millis(500))?;
    assert!(bed.bubbles().is_empty(), "{:?}", bed.bubbles());
    Ok(())
}

#[test]
fn a_replacement_updates_its_bubble_in_place() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let id = bed.notify(Notice {
        app: "Firefox",
        summary: "hocuspanda_0.2.0-1_amd64.deb",
        body: "Téléchargement : 12 %",
        progress: Some(12),
        timeout: 2000,
        ..Notice::default()
    })?;
    bed.shows(1)?;
    let before = bed.hp.bubble_surfaces();
    bed.idle(Duration::from_millis(1200))?;
    let again = bed.notify(Notice {
        app: "Firefox",
        replaces: id,
        summary: "hocuspanda_0.2.0-1_amd64.deb",
        body: "Téléchargement : 67 %",
        progress: Some(67),
        timeout: 2000,
        ..Notice::default()
    })?;
    assert_eq!(again, id);
    bed.logged("a bubble updated in place")?;
    // The same surface, and its countdown starts again: still there past
    // the first's time.
    bed.idle(Duration::from_millis(1300))?;
    let after = bed.hp.bubble_surfaces();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].0, before[0].0);
    assert!(bed.heard().is_empty(), "{:?}", bed.heard());
    bed.expires(id)?;
    Ok(())
}

#[test]
fn more_than_three_wait_their_turn_under_a_chip() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let mut ids = Vec::new();
    for who in ["Léa", "Hugo", "Inès", "Noah", "Jade"] {
        ids.push(bed.notify(Notice {
            app: "Signal",
            summary: who,
            body: "On se retrouve où ?",
            timeout: 60_000,
            ..Notice::default()
        })?);
    }
    let cards = bed.shows(3)?;
    // 8 px apart, downward.
    assert_eq!(cards[1].y, cards[0].y + cards[0].height + 8);
    assert_eq!(cards[2].y, cards[1].y + cards[1].height + 8);
    bed.logged("the waiting chip n=2")?;
    let chip = bed.hp.bubbles_waiting_surfaces();
    assert_eq!(chip.len(), 1);
    let (_, chip) = chip[0];
    // Under the stack, on its right.
    assert_eq!(chip.y, cards[2].y + cards[2].height + 8);
    assert_eq!(chip.x + chip.width, CARDS_RIGHT);
    // One closed: the next takes its turn.
    bed.click(close_of(cards[0]));
    bed.hears(&format!("closed {} 2", ids[0]))?;
    bed.logged("title=\"Noah\"")?;
    let cards = bed.shows(3)?;
    assert_eq!(cards[0].y, 8);
    // An application closes one waiting: the chip goes with the last.
    bed.close(ids[4])?;
    bed.hears(&format!("closed {} 3", ids[4]))?;
    let gone = bed.hp.dispatch_until(Duration::from_secs(5), |hp| {
        hp.bubbles_waiting_surfaces().is_empty()
    })?;
    assert!(gone);
    Ok(())
}

#[test]
fn an_action_the_card_and_the_close_say_what_they_should() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    // An action: invoked, then closed.
    let id = bed.notify(Notice {
        actions: &["default", "", "reply", "Répondre"],
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    // The action's button: from the text's left (56 px in), 12 px over the
    // card's bottom, 28 px high.
    let action = (
        f64::from(card.x + 56 + 10),
        f64::from(card.y + card.height - 1 - 12 - 14),
    );
    bed.click(action);
    bed.hears(&format!("action {id} reply"))?;
    bed.hears(&format!("closed {id} 2"))?;
    let gone = bed
        .hp
        .dispatch_until(Duration::from_secs(5), |hp| hp.bubble_surfaces().is_empty())?;
    assert!(gone);
    // The card: its default, then closed.
    let id = bed.notify(Notice {
        actions: &["default", "Ouvrir"],
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.click((f64::from(card.x + 150), f64::from(card.y + 30)));
    bed.hears(&format!("action {id} default"))?;
    bed.hears(&format!("closed {id} 2"))?;
    assert_eq!(
        bed.heard()
            .iter()
            .filter(|h| h.contains(&format!(" {id} ")))
            .count(),
        2
    );
    bed.hp
        .dispatch_until(Duration::from_secs(5), |hp| hp.bubble_surfaces().is_empty())?;
    // The ✕: closed, no action.
    let id = bed.notify(Notice {
        actions: &["default", "Ouvrir"],
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.click(close_of(card));
    bed.hears(&format!("closed {id} 2"))?;
    assert!(!bed.heard().contains(&format!("action {id} default")));
    // Closed by its application.
    let id = bed.notify(Notice::default())?;
    bed.hp
        .dispatch_until(Duration::from_secs(5), |hp| hp.bubble_surfaces().len() == 1)?;
    bed.close(id)?;
    bed.hears(&format!("closed {id} 3"))?;
    Ok(())
}

#[test]
fn an_agents_tool_saying_its_agent_waits_makes_no_bubble() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // Claude Code there and plugged in: the agents' group follows it.
    let mut bed = start(&shell, |home, config| {
        let places = hocus_agents::tools::Places {
            home: home.to_path_buf(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![home.join("bin")],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: PathBuf::from("hocus"),
            state_dir: Some(home.join("state")),
        };
        let _ = std::fs::create_dir_all(home.join(".claude"));
        let mut memory = hocus_agents::tools::Memory::load(&places);
        if let Err(err) =
            hocus_agents::tools::plug(hocus_agents::tools::Tool::ClaudeCode, &places, &mut memory)
        {
            panic!("could not plug Claude Code in: {err}");
        }
        config.agent_places = Some(places);
    })?;
    let plugged = bed.hp.dispatch_until(Duration::from_secs(10), |hp| {
        hp.agent_tools()
            .iter()
            .any(|t| t.tool == "claude-code" && t.state.followed())
    })?;
    assert!(plugged, "{:?}", bed.hp.agent_tools());
    bed.idle(Duration::from_millis(500))?;
    // As its terminal shows it: under the terminal's name, titled Claude
    // Code.
    bed.notify(Notice {
        app: "foot",
        summary: "Claude Code",
        body: "Claude is waiting for your input",
        ..Notice::default()
    })?;
    bed.logged("agent=true")?;
    bed.idle(Duration::from_millis(500))?;
    assert!(bed.bubbles().is_empty(), "{:?}", bed.bubbles());
    // Not counted on the bar either: the agent's row says it.
    assert!(!bed.log().contains("unread=1"), "{}", bed.log());
    // What else it says is a bubble.
    bed.notify(Notice {
        app: "foot",
        summary: "Claude Code",
        body: "Login successful",
        ..Notice::default()
    })?;
    bed.shows(1)?;
    Ok(())
}

// --- 5.4: the history, clear all, do not disturb -------------------------------------

#[test]
fn the_history_lists_by_sections_newest_first() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Logiciels", "12 mises à jour disponibles", 26 * 60, false)
        + &kept("Firefox", "Téléchargement terminé", 70, false)
        + &kept("Calendrier", "Point produit", 40, false);
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read kept=3 dropped=0")?;
    bed.notify(Notice {
        summary: "Camille dans #design",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged("a notification to the history")?;
    let shows = bed.open_history()?;
    // Newest first, under their sections; the one that just came not seen.
    assert!(
        shows.contains(
            "RÉCENTES: Slack «Camille dans #design» maintenant (unseen) Calendrier «Point produit» \
             il y a 40\u{a0}min PLUS TÔT AUJOURD’HUI: Firefox «Téléchargement terminé» il y a \
             1\u{a0}h HIER: Logiciels «12 mises à jour disponibles» hier"
        ),
        "{shows}"
    );
    // 360 px wide, 8 px from the full bar, its bottom on the bar's.
    let popover = bed.history();
    assert_eq!(popover.width, 360);
    assert_eq!(popover.x + popover.width, 1920 - 8 - 196 - 8);
    assert_eq!(popover.y + popover.height, 1080 - 8);
    assert_eq!(bed.history_items().len(), 4);
    Ok(())
}

#[test]
fn a_click_in_the_history_invokes_its_default_raises_and_removes_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    // Held by its sender: low urgency, no bubble, its default there.
    let id = bed.notify(Notice {
        summary: "Hugo dans #dev",
        actions: &["default", "", "reply", "Répondre"],
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.notify(Notice {
        app: "Firefox",
        summary: "Téléchargement terminé",
        urgency: Some(0),
        ..Notice::default()
    })?;
    let shows = bed.open_history()?;
    assert!(
        shows.contains("Slack «Hugo dans #dev» maintenant (unseen) [1 actions]"),
        "{shows}"
    );
    let items = bed.history_items();
    assert_eq!(items.len(), 2);
    // The older: Slack's.
    let item = bed.history_item(&items[1])?;
    let mark = bed.mark();
    bed.hp.pointer_move(item.0, item.1);
    bed.idle(Duration::from_millis(100))?;
    bed.click(item);
    // Its default invoked, its window asked for, then it goes: its sender
    // hears it closed by the user, and the history closes.
    bed.hears(&format!("action {id} default"))?;
    bed.hears(&format!("closed {id} 2"))?;
    bed.logged_since(mark, "a notification of the history opened")?;
    let closed = bed.hp.dispatch_until(Duration::from_secs(5), |hp| {
        hp.popover_surfaces().is_empty()
    })?;
    assert!(closed, "the history stays open");
    let shows = bed.open_history()?;
    assert!(!shows.contains("Hugo"), "{shows}");
    assert_eq!(bed.history_items().len(), 1);
    Ok(())
}

#[test]
fn its_close_removes_one_and_clear_all_empties_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Spotify", "Morceau suivant", 180, false)
        + &kept("Discord", "Léa", 125, false)
        + &kept("Firefox", "Téléchargement terminé", 70, false);
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read back kept=3")?;
    bed.open_history()?;
    let items = bed.history_items();
    assert_eq!(items.len(), 3);
    // The ✕ of the first, under the pointer.
    let first = bed.history_item(&items[0])?;
    bed.hp.pointer_move(first.0, first.1);
    bed.idle(Duration::from_millis(100))?;
    let close = bed.history_item(&items[0].replace("Item", "Close"))?;
    let mark = bed.mark();
    bed.hp.pointer_move(close.0, close.1);
    bed.idle(Duration::from_millis(100))?;
    bed.click(close);
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(!shows.contains("Firefox"), "{shows}");
    assert!(
        shows.contains("Discord") && shows.contains("Spotify"),
        "{shows}"
    );
    // "Clear all": empty, the resting panda's words, nothing to clear.
    let clear = bed.history_item("Clear")?;
    let mark = bed.mark();
    bed.hp.pointer_move(clear.0, clear.1);
    bed.idle(Duration::from_millis(100))?;
    bed.click(clear);
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(shows.contains("history=Rien de neuf"), "{shows}");
    let items = bed.logged_since(mark, "the history's items")?;
    assert!(items.ends_with("items=Switch@313,82"), "{items}");
    // Its file holds nothing then.
    bed.logged_since(mark, "the history written count=0")?;
    Ok(())
}

#[test]
fn the_badge_counts_what_was_not_seen_and_opening_the_history_resets_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    // A bubble gone after its time, the pointer never on it.
    let id = bed.notify(Notice {
        timeout: 800,
        ..Notice::default()
    })?;
    bed.expires(id)?;
    bed.logged("the bell unread=1")?;
    // One the pointer rested on: seen, not counted.
    let id = bed.notify(Notice {
        timeout: 1500,
        summary: "Hugo dans #dev",
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.hp
        .pointer_move(f64::from(card.x + 60), f64::from(card.y + 30));
    bed.logged("paused=true")?;
    bed.hp.pointer_move(400.0, 600.0);
    bed.expires(id)?;
    // A low one: counted.
    let mark = bed.mark();
    bed.notify(Notice {
        app: "Spotify",
        summary: "Morceau suivant",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged_since(mark, "the bell unread=2")?;
    // The footer's bell wears it (5.5): its blue badge at the button's top
    // right, 15 px, its right 2 px inside the button's.
    let (_, footer) = bed.hp.bell_surfaces()[0];
    let at = (footer.x + 11 + 44 + 40 - 2 - 3, footer.y + 6 + 1 + 7);
    let mut badge = [0; 3];
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(5) {
        let (width, _, bytes) = bed.hp.screenshot(0)?;
        let i = usize::try_from(at.1 * width + at.0)? * 4;
        badge = [bytes[i + 2], bytes[i + 1], bytes[i]];
        if badge[2] > 220 && (110..180).contains(&badge[0]) {
            break;
        }
        bed.idle(Duration::from_millis(50))?;
    }
    assert!(
        badge[2] > 220 && (110..180).contains(&badge[0]),
        "the badge: {badge:?}"
    );
    // Opened: 0; what it lists is seen once it closes.
    let mark = bed.mark();
    let shows = bed.open_history()?;
    bed.logged_since(mark, "the bell unread=0")?;
    assert_eq!(shows.matches("(unseen)").count(), 2, "{shows}");
    bed.close_history()?;
    let shows = bed.open_history()?;
    assert!(!shows.contains("(unseen)"), "{shows}");
    Ok(())
}

#[test]
fn do_not_disturb_holds_a_normal_bubble_but_lets_a_critical_one_show() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    bed.open_history()?;
    let switch = bed.history_item("Switch")?;
    let mark = bed.mark();
    bed.hp.pointer_move(switch.0, switch.1);
    bed.idle(Duration::from_millis(100))?;
    bed.click(switch);
    // On for an hour, the default; the bell slashed.
    bed.logged_since(mark, "do not disturb on=true choice=Some(Hour)")?;
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(shows.contains("dnd=Some(Hour)"), "{shows}");
    bed.logged_since(mark, "the bell unread=0 dnd=true")?;
    bed.close_history()?;
    // A normal one: held, to the history, counted.
    let mark = bed.mark();
    bed.notify(Notice::default())?;
    bed.logged_since(mark, "held: do not disturb")?;
    bed.logged_since(mark, "the bell unread=1 dnd=true")?;
    bed.idle(Duration::from_millis(500))?;
    assert!(bed.bubbles().is_empty(), "{:?}", bed.bubbles());
    // A critical one shows.
    bed.notify(Notice {
        app: "Batterie",
        summary: "Batterie très faible : 5 %",
        urgency: Some(2),
        ..Notice::default()
    })?;
    bed.shows(1)?;
    Ok(())
}

#[test]
fn the_history_survives_a_restart_of_the_shell_without_what_is_older_than_seven_days()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Slack", "Il y a huit jours", 8 * 24 * 60, false)
        + &kept("Slack", "Inès dans #général", 25 * 60, true);
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read kept=1 dropped=1")?;
    let mark = bed.mark();
    bed.notify(Notice {
        app: "Spotify",
        summary: "Morceau suivant",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged_since(mark, "the history written count=2")?;
    // Do not disturb, on in this session.
    bed.open_history()?;
    let switch = bed.history_item("Switch")?;
    bed.hp.pointer_move(switch.0, switch.1);
    bed.idle(Duration::from_millis(100))?;
    let mark = bed.mark();
    bed.click(switch);
    bed.logged_since(mark, "do not disturb on=true")?;
    bed.close_history()?;
    // The shell ends (by its process id, its script's); the compositor
    // starts it again.
    let pid = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    let mark = bed.mark();
    let killed = Command::new("kill").arg(&pid).status()?;
    assert!(killed.success());
    bed.logged_since(mark, "the history read back kept=2 dnd=true")?;
    let again = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    assert_ne!(again, pid);
    // Seen as the history closed before.
    let shows = bed.open_history()?;
    assert!(
        shows.contains("RÉCENTES: Spotify «Morceau suivant»")
            && shows.contains("HIER: Slack «Inès dans #général» hier"),
        "{shows}"
    );
    assert!(!shows.contains("(unseen)"), "{shows}");
    assert!(!shows.contains("huit jours"), "{shows}");
    Ok(())
}

#[test]
fn an_expired_bubbles_action_is_invoked_from_the_history() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let id = bed.notify(Notice {
        actions: &["reply", "Répondre"],
        timeout: 800,
        ..Notice::default()
    })?;
    bed.shows(1)?;
    bed.expires(id)?;
    let shows = bed.open_history()?;
    assert!(shows.contains("[1 actions]"), "{shows}");
    let log = bed.log();
    let action = log
        .lines()
        .rev()
        .find(|l| l.contains("the history's items"))
        .and_then(|l| {
            l.split_whitespace()
                .map(|w| w.trim_start_matches("items="))
                .filter_map(|w| w.split_once('@').map(|(who, _)| who.to_owned()))
                .find(|who| who.starts_with("Action("))
        })
        .ok_or_else(|| anyhow::anyhow!("no action listed:\n{log}"))?;
    let at = bed.history_item(&action)?;
    bed.hp.pointer_move(at.0, at.1);
    bed.idle(Duration::from_millis(100))?;
    bed.click(at);
    // Invoked, then closed by the user.
    bed.hears(&format!("action {id} reply"))?;
    bed.hears(&format!("closed {id} 2"))?;
    Ok(())
}
