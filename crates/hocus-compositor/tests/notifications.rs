//! The notifications' server and their bubbles (5.3, issue #393): the real
//! shell serving `org.freedesktop.Notifications` on a D-Bus daemon of the
//! test's own (never the session's), told by a client of the test's.
//!
//! One test per point of the approved spec: a bubble shows against the bar
//! and goes after its time; hovering pauses it; a critical one stays; a low
//! one goes to the history, the bar's dot counting it; a replacement
//! updates its bubble in place; more than three wait under a "+N" chip;
//! the actions, the card and the ✕ say what they should; an agents' tool
//! saying its agent waits makes no bubble, its other notifications do; an
//! agent's notification opens its conversation in a terminal (#823).
//!
//! And 5.4's (issue #394), one per point of its spec, as #923 changed it:
//! the history lists by application, newest first; a click on a line
//! invokes the default, raises, and leaves it seen; an expired bubble's
//! action still works from it (a bubble's time up closes nothing: the
//! notification stays held); its ✕ removes one, "clear all" empties it;
//! the bell's badge counts what is new and opening the history resets it;
//! do not disturb holds a normal bubble but not a critical one; the
//! history survives a restart of the shell without what is older than 7
//! days.
//!
//! And #923's, one per point of its spec: opening an application folds its
//! notifications, the badge dropping, its bubble on screen living its
//! time; an application set not to folds nothing; a `transient` one makes
//! its bubble, no line, no count, and a journal's line; the journal is the
//! user's alone and says each one's fields. The
//! shell's wall clock reads 2 November 2026, 14:32:07 UTC
//! (`HOCUS_CLOCK_AT`); do not disturb ending at its time is the stack's
//! unit test, on the clock it is given.
//!
//! Pixman here, so the shell draws on the CPU: each bubble's surface is its
//! card, and nothing moves (as in CI).

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{Command as Asked, KEY_ESC, Shows, WAIT};
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
    /// The test's bus's address.
    address: String,
    heard: Heard,
    /// The configuration file the shell writes into (a level chosen from a
    /// bubble, #899).
    config: PathBuf,
}

impl Drop for Bed {
    fn drop(&mut self) {
        let _ = self.bus.kill();
        let _ = self.bus.wait();
    }
}

fn ready() -> Option<PathBuf> {
    let shell = common::shell_program();
    let daemon = Command::new("sh")
        .args(["-c", "command -v dbus-daemon"])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    if !daemon {
        common::missing("dbus-daemon");
    }
    Some(shell)
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
    let (bus, address) = common::private_bus(dir.path())?;
    let log = dir.path().join("shell.log");
    let pid = dir.path().join("shell.pid");
    let script = dir.path().join("shell");
    let config_file = dir.path().join("config.toml");
    std::fs::write(&config_file, "# Mine.\n[panel]\nform = \"full\"\n")?;
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
             XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' TZ=UTC HOCUS_CLOCK_AT='2026-11-02T14:32:07' \
             HOCUS_CONFIG='{config}' \
             RUST_LOG=warn,hocuspanda_shell=debug,hocuspanda_shell::notifications::bell=trace\necho $$ > '{pid}'\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            state = state.display(),
            pid = pid.display(),
            shell = shell.display(),
            log = log.display(),
            config = config_file.display(),
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
        address,
        heard,
        config: config_file,
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
        self.notify_hinted(notice, Vec::new())
    }

    /// Sends `notice` with these hints too (`transient`, `desktop-entry`);
    /// its id.
    fn notify_hinted(
        &mut self,
        notice: Notice<'_>,
        more: Vec<(&'static str, OwnedVariant)>,
    ) -> anyhow::Result<u32> {
        let mut hints: HashMap<&str, OwnedVariant> = more.into_iter().collect();
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
        // Each drawn at its place: the shell sizes and places a bubble as
        // it draws it, which on a loaded machine comes well after its
        // surface.
        let done = self.hp.dispatch_until(Duration::from_secs(10), |hp| {
            let bubbles = hp.bubble_surfaces();
            bubbles.len() == n
                && bubbles
                    .iter()
                    .all(|(namespace, _, r)| hp.drawn_layers(namespace).iter().any(|(_, d)| d == r))
        })?;
        anyhow::ensure!(done, "not {n} bubbles drawn: {:?}", self.bubbles());
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

    /// A click there, once the shell's surfaces there are drawn
    /// ([`common::click`]).
    fn click(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        common::click(&mut self.hp, at)
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

    /// Waits until the shell logged `what`; the line.
    fn logged_line(&mut self, what: &str) -> anyhow::Result<String> {
        self.logged_since(0, what)
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
        // Drawn: the shell started again shows its bell anew.
        let shown = self.hp.dispatch_until(Duration::from_secs(10), |hp| {
            hp.drawn_layers(hocus_link::BELL_NAMESPACE).len() == 1
        })?;
        anyhow::ensure!(shown, "no bell");
        let (_, at) = self.hp.bell_surfaces()[0];
        Ok((f64::from(at.x + 11 + 44 + 20), f64::from(at.y + 6 + 17)))
    }

    /// Opens the history with a click on the bell: what it shows, as
    /// logged.
    fn open_history(&mut self) -> anyhow::Result<String> {
        let bell = self.bell()?;
        let mark = self.mark();
        common::point(&mut self.hp, bell)?;
        self.idle(Duration::from_millis(100))?;
        self.click(bell)?;
        let shows = self.logged_since(mark, "the history shows")?;
        self.logged_since(mark, "\"history:Switch\"")?;
        // Drawn at its place: its place and size before, 0 × 0.
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(open, "the history did not open");
        Ok(shows)
    }

    /// The history's popover, on the screen.
    fn history(&self) -> Rect {
        self.hp.popover_surfaces()[0].1
    }

    /// Where the history's controls are on it, as the kit's dump last
    /// placed them: name (`Clear`, `Switch`, `Item(3)`, `Close(3)`,
    /// `Action(3,0)`) → middle, top first.
    fn history_places(&self) -> Vec<(String, (f64, f64))> {
        let mut found = Vec::new();
        if let Some(serde_json::Value::Object(dump)) = common::last_json(&self.log, " shows {") {
            for (id, place) in dump {
                let Some(name) = id.strip_prefix("history:") else {
                    continue;
                };
                let at = place.get("at").and_then(serde_json::Value::as_array);
                let n = |i: usize| {
                    at.and_then(|a| a.get(i))
                        .and_then(serde_json::Value::as_f64)
                };
                if let (Some(x), Some(y), Some(w), Some(h)) = (n(0), n(1), n(2), n(3)) {
                    found.push((name.to_owned(), (x + w / 2.0, y + h / 2.0)));
                }
            }
        }
        found.sort_by(|a, b| a.1.1.total_cmp(&b.1.1));
        found
    }

    /// Where `name` (`Clear`, `Switch`, `Item(3)`, `Close(3)`…) is on the
    /// screen, as the kit's dump last placed it.
    fn history_item(&self, name: &str) -> anyhow::Result<(f64, f64)> {
        let at = self
            .history_places()
            .into_iter()
            .find(|(who, _)| who == name)
            .map(|(_, at)| at)
            .ok_or_else(|| anyhow::anyhow!("no {name} in the kit's dump"))?;
        let popover = self.history();
        Ok((f64::from(popover.x) + at.0, f64::from(popover.y) + at.1))
    }

    /// The items the history lists, by their names in the kit's dump
    /// (`Item(3)`…), top first.
    fn history_items(&self) -> Vec<String> {
        self.history_places()
            .into_iter()
            .map(|(who, _)| who)
            .filter(|who| who.starts_with("Item("))
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
    common::point(
        &mut bed.hp,
        (f64::from(card.x + 60), f64::from(card.y + 30)),
    )?;
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
    bed.click(close_of(card))?;
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
        summary: "hocuspanda_1.0.0-1_amd64.deb",
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
        summary: "hocuspanda_1.0.0-1_amd64.deb",
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
    // Drawn at its place: 0 × 0 before its first buffer; placed with the
    // stack as the bubbles' places settle.
    let mut seen = Vec::new();
    bed.hp.dispatch_until(WAIT, |hp| {
        let chip = hp.drawn_layers(hocus_link::BUBBLES_WAITING_NAMESPACE);
        if seen.last() != Some(&chip) {
            seen.push(chip.clone());
        }
        chip.first().is_some_and(|(_, r)| {
            r.y == cards[2].y + cards[2].height + 8 && r.x + r.width == CARDS_RIGHT
        })
    })?;
    let chip = bed.hp.drawn_layers(hocus_link::BUBBLES_WAITING_NAMESPACE);
    assert_eq!(chip.len(), 1, "{seen:?}");
    let (_, chip) = chip[0];
    // Under the stack, on its right.
    assert_eq!(chip.y, cards[2].y + cards[2].height + 8);
    assert_eq!(chip.x + chip.width, CARDS_RIGHT);
    // One closed: the next takes its turn.
    bed.click(close_of(cards[0]))?;
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
    bed.click(action)?;
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
    bed.click((f64::from(card.x + 150), f64::from(card.y + 30)))?;
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
    bed.click(close_of(card))?;
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

/// An agent working on its own (#823): a fake `claude`, in a folder of the
/// test's, its transcript in the shell's home, sends a notification on the
/// test's bus through a program of its own (`busctl`, as `notify-send`
/// would). A click on its bubble opens the terminal (a stand-in named
/// `foot`) in that folder resuming its conversation; so does a click on it
/// in the history.
#[test]
fn an_agents_notification_opens_its_conversation_in_a_terminal() -> anyhow::Result<()> {
    const SESSION: &str = "3f6c2a1e-0b7d-4c55-9a51-2f0e8d1c7b42";
    let Some(shell) = ready() else {
        return Ok(());
    };
    let busctl = ["/usr/bin/busctl", "/bin/busctl"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .unwrap_or_else(|| common::missing("busctl (systemd)"));
    let work = tempfile::tempdir()?;
    let bin = work.path().join("bin");
    let folder = work.path().join("hocus-site");
    std::fs::create_dir_all(&bin)?;
    std::fs::create_dir_all(&folder)?;
    let folder = folder.canonicalize()?;
    let marks = work.path().join("marks");
    let script = |name: &str, text: String| -> anyhow::Result<()> {
        let file = bin.join(name);
        std::fs::write(&file, text)?;
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755))?;
        Ok(())
    };
    // The terminal's stand-in: what it was asked to run, and where.
    script(
        "foot",
        format!(
            "#!/bin/sh\nprintf '%s\\t%s\\n' \"$*\" \"$PWD\" >> '{}'\n",
            marks.display()
        ),
    )?;
    // The agent, its notification sent by a program it runs (not the last
    // of its script: it stays its parent).
    script(
        "claude",
        format!(
            "#!/bin/sh\n'{}' --address=\"$HOCUS_TEST_BUS\" -- call org.freedesktop.Notifications \
             /org/freedesktop/Notifications org.freedesktop.Notifications Notify \
             'susssasa{{sv}}i' Ada 0 '' 'J’ai trouvé ce que tu voulais' 'pas cher' 0 0 -1 \
             >/dev/null\nstatus=$?\nexit $status\n",
            busctl.display()
        ),
    )?;
    let terminal = bin.join("foot");
    let path = bin.clone();
    let mut home_of = PathBuf::new();
    let mut bed = start(&shell, |home, config| {
        home_of = home.to_path_buf();
        config.programs.terminal = terminal.display().to_string();
        config.agent_places = Some(hocus_agents::tools::Places {
            home: home.to_path_buf(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![path],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: PathBuf::from("hocus"),
            state_dir: Some(home.join("state")),
        });
    })?;
    // Its transcript, as Claude Code names its folder.
    let mangled: String = folder
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let transcripts = home_of.join(".claude/projects").join(mangled);
    std::fs::create_dir_all(&transcripts)?;
    std::fs::write(transcripts.join(format!("{SESSION}.jsonl")), "{}\n")?;

    let sent = Command::new(bin.join("claude"))
        .current_dir(&folder)
        .env_remove("CLAUDE_CODE_SESSION_ID")
        .env("HOCUS_TEST_BUS", &bed.address)
        .status()?;
    anyhow::ensure!(sent.success(), "the fake agent's notification: {sent}");
    bed.logged("an agent's conversation's notification")?;
    let card = bed.shows(1)?[0];
    bed.click((f64::from(card.x + 150), f64::from(card.y + 30)))?;
    let shell_program = std::env::var("SHELL")
        .ok()
        .filter(|s| s.starts_with('/'))
        .unwrap_or_else(|| "/bin/sh".to_owned());
    let want = format!(
        "--working-directory={} -- {shell_program} -c claude --resume {SESSION}; exec $SHELL\t{}",
        folder.display(),
        folder.display()
    );
    let lines = |n: usize| {
        let marks = marks.clone();
        move |_: &Headless| std::fs::read_to_string(&marks).is_ok_and(|m| m.lines().count() >= n)
    };
    let opened = bed.hp.dispatch_until(WAIT, lines(1))?;
    anyhow::ensure!(opened, "no terminal:\n{}", bed.log());
    assert_eq!(
        std::fs::read_to_string(&marks)?.lines().collect::<Vec<_>>(),
        [want.as_str()]
    );

    // From the history, the same.
    bed.open_history()?;
    let items = bed.history_items();
    let item = bed.history_item(items.first().map_or("", String::as_str))?;
    common::point(&mut bed.hp, item)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(item)?;
    let opened = bed.hp.dispatch_until(WAIT, lines(2))?;
    anyhow::ensure!(opened, "no terminal from the history:\n{}", bed.log());
    assert_eq!(
        std::fs::read_to_string(&marks)?.lines().collect::<Vec<_>>(),
        [want.as_str(), want.as_str()]
    );
    Ok(())
}

// --- 5.4: the history, clear all, do not disturb -------------------------------------

#[test]
fn the_history_lists_by_application_newest_first() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Logiciels", "12 mises à jour disponibles", 26 * 60, false)
        + &kept("Firefox", "Téléchargement terminé", 70, false)
        + &kept("Calendrier", "Point produit", 40, false);
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read kept=3 dropped=0")?;
    // Read, its icons found, then the history has it: a while after.
    bed.logged("the history read back kept=3")?;
    bed.notify(Notice {
        summary: "Camille dans #design",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged("a notification to the history")?;
    let shows = bed.open_history()?;
    // Newest first, each application its card, a lone line with its
    // name; the one that just came new.
    assert!(
        shows.contains(
            "history=Slack «Camille dans #design» maintenant (new) Calendrier «Point produit» \
             il y a 40\u{a0}min Firefox «Téléchargement terminé» il y a 1\u{a0}h Logiciels «12 \
             mises à jour disponibles» hier"
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
fn a_click_on_a_line_invokes_its_default_raises_and_leaves_it_seen() -> anyhow::Result<()> {
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
        shows.contains("Slack «Hugo dans #dev» maintenant (new) [1 actions]"),
        "{shows}"
    );
    let items = bed.history_items();
    assert_eq!(items.len(), 2);
    // The older: Slack's.
    let item = bed.history_item(&items[1])?;
    let mark = bed.mark();
    common::point(&mut bed.hp, item)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(item)?;
    // Its default invoked, its window asked for, then it is seen, not
    // taken out: its sender hears it closed by the user, and the history
    // closes.
    bed.hears(&format!("action {id} default"))?;
    bed.hears(&format!("closed {id} 2"))?;
    bed.logged_since(mark, "a notification of the history opened")?;
    let closed = bed.hp.dispatch_until(Duration::from_secs(5), |hp| {
        hp.popover_surfaces().is_empty()
    })?;
    assert!(closed, "the history stays open");
    let shows = bed.open_history()?;
    assert!(shows.ends_with("| Slack · 1 vue ▸"), "{shows}");
    assert_eq!(bed.history_items().len(), 1, "Slack's folded");
    // Unfolded, it is there, seen.
    let fold = bed.history_item("Fold(slack)")?;
    let mark = bed.mark();
    common::point(&mut bed.hp, fold)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(fold)?;
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(
        shows.contains("Slack · 1 vue ▾ «Hugo dans #dev» maintenant (seen)"),
        "{shows}"
    );
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
    common::point(&mut bed.hp, first)?;
    bed.idle(Duration::from_millis(100))?;
    let close = bed.history_item(&items[0].replace("Item", "Close"))?;
    let mark = bed.mark();
    common::point(&mut bed.hp, close)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(close)?;
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(!shows.contains("Firefox"), "{shows}");
    assert!(
        shows.contains("Discord") && shows.contains("Spotify"),
        "{shows}"
    );
    // "Clear all": empty, the resting panda's words, nothing to clear.
    let clear = bed.history_item("Clear")?;
    let mark = bed.mark();
    common::point(&mut bed.hp, clear)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(clear)?;
    let shows = bed.logged_since(mark, "the history shows")?;
    assert!(shows.contains("history=Rien de neuf"), "{shows}");
    // The places of a frame drawn after that line: one logged since the
    // mark but before it, of the history as it was, names the switch too
    // (#844).
    let emptied = bed
        .log()
        .get(mark..)
        .and_then(|new| new.find(&shows))
        .map_or(mark, |at| mark + at + shows.len());
    bed.logged_since(emptied, "\"history:Switch\"")?;
    let places = bed.history_places();
    let names: Vec<&str> = places.iter().map(|(who, _)| who.as_str()).collect();
    assert_eq!(names, ["Switch"], "{places:?}");
    assert_eq!(places[0].1, (313.0, 82.0), "{places:?}");
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
    common::point(
        &mut bed.hp,
        (f64::from(card.x + 60), f64::from(card.y + 30)),
    )?;
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
    // Spotify's new; Slack's two, the newest noticed (the pointer rested
    // on it), the other new behind the strips.
    assert!(
        shows.contains(
            "history=Spotify «Morceau suivant» maintenant (new) Slack 2: «Hugo dans #dev» \
             maintenant +1"
        ),
        "{shows}"
    );
    bed.close_history()?;
    let shows = bed.open_history()?;
    assert!(!shows.contains("(new)"), "{shows}");
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
    common::point(&mut bed.hp, switch)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(switch)?;
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
    let held = bed.logged_since(mark, "a notification's bubble held: do not disturb")?;
    assert!(held.contains("app=\"Slack\""), "{held}");
    assert!(!held.contains("Camille"), "its text in the journal: {held}");
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
    // Read, its icons found, then the history has it: a while after.
    bed.logged("the history read back kept=1")?;
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
    common::point(&mut bed.hp, switch)?;
    bed.idle(Duration::from_millis(100))?;
    let mark = bed.mark();
    bed.click(switch)?;
    bed.logged_since(mark, "do not disturb on=true")?;
    bed.close_history()?;
    // The shell is told to end (SIGTERM, by its process id, its script's)
    // while busy, as the history it showed is written, seen: it ends once
    // that is written (#557). The compositor starts it again.
    let pid = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    let mark = bed.mark();
    bed.hp.hold_shell(true)?;
    let killed = Command::new("kill").arg(&pid).status();
    bed.hp.hold_shell(false)?;
    assert!(killed?.success());
    bed.logged_since(mark, "the history read back kept=2 dnd=true")?;
    let again = std::fs::read_to_string(&bed.pid)?.trim().to_owned();
    assert_ne!(again, pid);
    // Noticed as the history closed before.
    let shows = bed.open_history()?;
    assert!(
        shows.contains("history=Spotify «Morceau suivant»")
            && shows.contains("Slack «Inès dans #général» hier"),
        "{shows}"
    );
    assert!(!shows.contains("(new)"), "{shows}");
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
    let action = bed
        .history_places()
        .into_iter()
        .map(|(who, _)| who)
        .find(|who| who.starts_with("Action("))
        .ok_or_else(|| anyhow::anyhow!("no action listed:\n{}", bed.log()))?;
    let at = bed.history_item(&action)?;
    common::point(&mut bed.hp, at)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(at)?;
    // Invoked, then closed by the user.
    bed.hears(&format!("action {id} reply"))?;
    bed.hears(&format!("closed {id} 2"))?;
    Ok(())
}

// --- #923: the bell keeps what still needs you ---------------------------------------

impl Bed {
    /// The notifications' journal (#923).
    fn journal_file(&self) -> PathBuf {
        self._dir.path().join("state/hocuspanda/notifications.log")
    }

    /// Waits until the journal holds `n` lines; its lines, read.
    fn journal_lines(&mut self, n: usize) -> anyhow::Result<Vec<serde_json::Value>> {
        let file = self.journal_file();
        let read = |file: &Path| -> Vec<serde_json::Value> {
            std::fs::read_to_string(file)
                .unwrap_or_default()
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect()
        };
        let done = self.hp.dispatch_until(WAIT, |_| read(&file).len() >= n)?;
        anyhow::ensure!(done, "the journal holds {:?}\n{}", read(&file), self.log());
        Ok(read(&file))
    }
}

#[test]
fn opening_an_application_folds_its_notifications_and_its_bubble_lives_its_time()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    // Courriel's window first: its notification is its, not the window's
    // of the process that sends them all (the test's own, which draws
    // Chat's window too). The shell is told of its focus on the link and
    // of the notifications on the bus: Chat's sent before it knew would
    // fold as the process's, so the notifications wait for its fold.
    common::open(&mut bed.hp, "org.example.Courriel", (500, 300))?;
    bed.logged("its notifications seen app=\"org.example.Courriel\"")?;
    for summary in ["Hugo dans #dev", "Inès dans #général"] {
        bed.notify(Notice {
            app: "Chat",
            summary,
            urgency: Some(0),
            ..Notice::default()
        })?;
    }
    bed.notify(Notice {
        app: "Courriel",
        summary: "Facture",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged("the bell unread=3")?;
    // One of Chat's on screen as its window opens.
    bed.notify(Notice {
        app: "Chat",
        summary: "Camille dans #design",
        timeout: 0,
        ..Notice::default()
    })?;
    bed.shows(1)?;
    let mark = bed.mark();
    common::open(&mut bed.hp, "org.example.Chat", (600, 400))?;
    let folded = bed.logged_since(mark, "a window takes the focus: its notifications seen")?;
    assert!(
        folded.contains("app=\"org.example.Chat\" folded=3"),
        "{folded}"
    );
    // The badge drops to Courriel's; Chat's bubble stays on screen.
    bed.logged_since(mark, "the bell unread=1")?;
    assert_eq!(bed.bubbles().len(), 1, "{:?}", bed.bubbles());
    let shows = bed.open_history()?;
    assert!(
        shows.contains("history=Courriel «Facture» maintenant (new) | Chat · 3 vues ▸"),
        "{shows}"
    );
    Ok(())
}

#[test]
fn an_application_set_not_to_fold_keeps_its_notifications_new() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, config| {
        // By the notifications' application's id, as the levels.
        config
            .notifications
            .seen_on_focus
            .insert("Chat".to_owned(), false);
    })?;
    for summary in ["Hugo dans #dev", "Inès dans #général"] {
        bed.notify(Notice {
            app: "Chat",
            summary,
            urgency: Some(0),
            ..Notice::default()
        })?;
    }
    bed.logged("the bell unread=2")?;
    let mark = bed.mark();
    common::open(&mut bed.hp, "org.example.Chat", (600, 400))?;
    let folded = bed.logged_since(mark, "a window takes the focus: its notifications seen")?;
    assert!(folded.contains("folded=0"), "{folded}");
    let shows = bed.open_history()?;
    assert!(
        shows.contains("history=Chat 2: «Inès dans #général» maintenant (new) +1"),
        "{shows}"
    );
    Ok(())
}

#[test]
fn a_transient_one_makes_its_bubble_then_goes_nowhere_but_the_journal() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    let transient = |v: Variant<'static>| OwnedVariant::try_from(v);
    let id = bed.notify_hinted(
        Notice {
            app: "Musique",
            summary: "Morceau suivant",
            timeout: 800,
            ..Notice::default()
        },
        vec![("transient", transient(Variant::Bool(true))?)],
    )?;
    bed.shows(1)?;
    // Its time up: closed as expired, nothing kept.
    bed.hears(&format!("closed {id} 1"))?;
    // One that makes no bubble (low, an integer for the hint): closed at
    // once.
    let low = bed.notify_hinted(
        Notice {
            app: "Musique",
            summary: "Volume",
            urgency: Some(0),
            ..Notice::default()
        },
        vec![("transient", transient(Variant::U32(1))?)],
    )?;
    bed.hears(&format!("closed {low} 1"))?;
    let lines = bed.journal_lines(2)?;
    let said: Vec<(&str, bool, &str)> = lines
        .iter()
        .map(|l| {
            (
                l["title"].as_str().unwrap_or_default(),
                l["transient"].as_bool().unwrap_or(false),
                l["to"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("Morceau suivant", true, "bubble"),
            ("Volume", true, "journal")
        ]
    );
    let shows = bed.open_history()?;
    assert!(shows.contains("history=Rien de neuf"), "{shows}");
    assert!(!bed.log().contains("unread=1"), "{}", bed.log());
    Ok(())
}

#[test]
fn the_journal_keeps_every_one_readable_by_the_user_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, config| {
        config
            .notifications
            .apps
            .insert("Firefox".to_owned(), hocus_link::NotifyLevel::None);
    })?;
    bed.notify(Notice {
        actions: &["default", "", "reply", "Répondre"],
        timeout: 0,
        ..Notice::default()
    })?;
    // Dropped by its level: written all the same.
    bed.notify(Notice {
        app: "Firefox",
        summary: "Téléchargement terminé",
        ..Notice::default()
    })?;
    let lines = bed.journal_lines(2)?;
    let mode = std::fs::metadata(bed.journal_file())?.permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let first = &lines[0];
    // The shell's wall clock: 2 November 2026, 14:32 UTC.
    let at = first["at"].as_str().unwrap_or_default();
    assert!(
        at.starts_with("2026-11-02T14:3") && at.ends_with("+00:00") && at.contains('.'),
        "{at}"
    );
    assert!(first["id"].as_u64().is_some_and(|id| id > 0), "{first}");
    assert_eq!(first["app"], "Slack");
    assert_eq!(first["app_id"], "Slack");
    assert_eq!(first["title"], "Camille dans #design");
    assert_eq!(first["body"], "J'ai mis à jour la maquette du lanceur.");
    assert_eq!(first["urgency"], "normal");
    assert_eq!(first["actions"], serde_json::json!(["Répondre"]));
    assert_eq!(first["level"], "all");
    assert_eq!(first["to"], "bubble");
    assert!(first.get("transient").is_none() && first.get("icon").is_none());
    assert_eq!(lines[1]["to"], "dropped");
    assert_eq!(lines[1]["level"], "none");
    assert_eq!(
        lines[1]["id"].as_u64(),
        first["id"].as_u64().map(|id| id + 1)
    );
    Ok(())
}

// --- What the journal says (#710) ----------------------------------------------------

/// The compositor's lines, as the session's journal has them (`INFO` and
/// over), while its guard is held on the test's thread, the compositor's.
#[derive(Clone, Default)]
struct Journal(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Journal {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Journal {
    fn kept() -> (Self, tracing::subscriber::DefaultGuard) {
        let journal = Self::default();
        let writer = journal.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_env_filter("info")
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish();
        (journal, tracing::subscriber::set_default(subscriber))
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner)).into_owned()
    }
}

/// The journal says what became of each notification's bubble, by its
/// application and id, never by what it says: shown (and drawn, by the
/// compositor), waiting its turn behind the three shown, waiting for the
/// unlock, or none for a low one.
#[test]
fn the_journal_says_what_became_of_each_bubble_without_its_text() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let (journal, _kept) = Journal::kept();
    let mut bed = start(&shell, |_, _| {})?;
    let long = Notice {
        timeout: 30_000,
        ..Notice::default()
    };
    for _ in 0..3 {
        bed.notify(Notice { ..long })?;
    }
    bed.shows(3)?;
    let fourth = bed.notify(Notice { ..long })?;
    let waits = bed.logged_line("a notification's bubble waits its turn")?;
    assert!(
        waits.contains(&format!("id={fourth} app=\"Slack\" behind=3 place=1")),
        "{waits}"
    );
    bed.notify(Notice {
        urgency: Some(0),
        ..long
    })?;
    let low = bed.logged_line("a notification without a bubble: to the history only")?;
    assert!(low.contains("low=true agent=false"), "{low}");
    bed.hp.lock();
    bed.logged("the session is locked")?;
    let locked = bed.notify(Notice { ..long })?;
    let unlock = bed.logged_line("a notification's bubble waits for the unlock")?;
    assert!(
        unlock.contains(&format!("id={locked} app=\"Slack\"")),
        "{unlock}"
    );
    let log = bed.log();
    let said: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("a notification's bubble") || l.contains("without a bubble"))
        .collect();
    let shows = said.iter().filter(|l| l.contains("bubble shows")).count();
    assert_eq!(shows, 3, "{said:#?}");
    assert!(
        said.iter()
            .all(|l| l.contains(" INFO ") && !l.contains("Camille") && !l.contains("maquette")),
        "{said:#?}"
    );
    // The compositor drew each of the three.
    let drawn = journal.text();
    for serial in 1..=3 {
        assert!(
            drawn.contains(&format!(
                "a bubble is drawn namespace=\"hocus-bubble-{serial}\""
            )),
            "{drawn}"
        );
    }
    assert!(!drawn.contains("without ever being drawn"), "{drawn}");
    Ok(())
}

/// A bubble's surface that goes without ever having been drawn is said
/// (`WARN`): the shell took it for shown, the user never saw it. One
/// drawn is said as it first is, and nothing as it goes.
#[test]
fn a_bubble_never_drawn_is_said_as_it_goes() -> anyhow::Result<()> {
    let (journal, _kept) = Journal::kept();
    let mut hp = common::compositor()?;
    let undrawn = common::client(
        hp.socket(),
        Some(Shows::Undrawn {
            namespace: "hocus-bubble-7",
        }),
    )?;
    let drawn = common::client(
        hp.socket(),
        Some(Shows::Layer {
            namespace: "hocus-bubble-8",
            size: (340, 80),
            margin: (8, 8),
            colour: 0xFF20_2030,
        }),
    )?;
    let placed = hp.dispatch_until(WAIT, |hp| {
        !hp.layer_rects("hocus-bubble-7").is_empty()
            && journal_has(&journal, "a bubble is drawn namespace=\"hocus-bubble-8\"")
    })?;
    anyhow::ensure!(placed, "{}", journal.text());
    undrawn.commands.send(Asked::CloseLayer)?;
    drawn.commands.send(Asked::CloseLayer)?;
    let said = hp.dispatch_until(WAIT, |hp| {
        hp.layer_rects("hocus-bubble-7").is_empty() && hp.layer_rects("hocus-bubble-8").is_empty()
    })?;
    anyhow::ensure!(said, "{}", journal.text());
    let text = journal.text();
    let warned: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("a bubble's surface went without ever being drawn"))
        .collect();
    assert_eq!(warned.len(), 1, "{text}");
    assert!(
        warned[0].contains(" WARN ") && warned[0].contains("namespace=\"hocus-bubble-7\""),
        "{text}"
    );
    assert!(
        !text.contains("a bubble is drawn namespace=\"hocus-bubble-7\""),
        "{text}"
    );
    Ok(())
}

fn journal_has(journal: &Journal, what: &str) -> bool {
    journal.text().contains(what)
}

/// The history in its main state (four, by sections), shot under
/// `HOCUS_KIT_SHOTS` (#772's side-by-sides; see `wifi::kit_shot`).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Logiciels", "12 mises à jour disponibles", 26 * 60, false)
        + &kept("Firefox", "Téléchargement terminé", 70, false)
        + &kept("Calendrier", "Point produit", 40, false);
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read back kept=3")?;
    bed.notify(Notice {
        summary: "Camille dans #design",
        urgency: Some(0),
        ..Notice::default()
    })?;
    bed.logged("a notification to the history")?;
    bed.open_history()?;
    bed.hp.pointer_move(400.0, 300.0);
    common::popover_shot(&mut bed.hp, "history")
}

/// A bubble in its main state (its actions, no countdown: what it draws
/// at rest), shot under `HOCUS_KIT_SHOTS` (#772's side-by-sides).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn bubble_kit_shot() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, |_, _| {})?;
    bed.notify(Notice {
        actions: &["reply", "Répondre", "later", "Plus tard"],
        timeout: 0,
        ..Notice::default()
    })?;
    bed.shows(1)?;
    bed.hp.pointer_move(400.0, 600.0);
    let namespace = bed
        .hp
        .bubble_surfaces()
        .first()
        .map(|(namespace, _, _)| namespace.clone())
        .ok_or_else(|| anyhow::anyhow!("no bubble"))?;
    common::layer_shot(&mut bed.hp, &namespace, "bubble")
}

// --- Per application (#899) ---------------------------------------------------------

const BTN_RIGHT: u32 = 0x111;

/// A card's "⋯": under its ✕, 2 px apart, 24 px.
fn more_of(card: Rect) -> (f64, f64) {
    (
        f64::from(card.x + card.width - 1 - 8 - 12),
        f64::from(card.y + 1 + 8 + 24 + 2 + 12),
    )
}

impl Bed {
    /// Where the menu of levels' `level` (`All`, `Silent`, `History`,
    /// `None`) is on the screen, once its popover is drawn.
    fn level_at(&mut self, level: &str) -> anyhow::Result<(f64, f64)> {
        let id = format!("notify_levels:{level}");
        let log = self.log.clone();
        let place = |log: &Path| -> Option<(f64, f64)> {
            let at = common::last_json(log, " shows {")?
                .get(&id)?
                .get("at")?
                .clone();
            let n = |i: usize| at.get(i).and_then(serde_json::Value::as_f64);
            Some((n(0)? + n(2)? / 2.0, n(1)? + n(3)? / 2.0))
        };
        let found = self.hp.dispatch_until(WAIT, |hp| {
            place(&log).is_some() && common::drawn_popovers(hp).len() == 1
        })?;
        anyhow::ensure!(found, "no {id} in the menu's dump\n{}", self.log());
        let (x, y) = place(&log).unwrap_or_default();
        let popover = common::drawn_popovers(&self.hp)[0];
        Ok((f64::from(popover.x) + x, f64::from(popover.y) + y))
    }

    /// Waits for the configuration file to hold `what`.
    fn config_holds(&mut self, what: &str) -> anyhow::Result<String> {
        let file = self.config.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            std::fs::read_to_string(&file).is_ok_and(|t| t.contains(what))
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        anyhow::ensure!(
            done,
            "the file never held {what:?}:\n{text}\n{}",
            self.log()
        );
        Ok(text)
    }

    /// With `HOCUS_SHOTS` naming a folder: `rect` of the screen, as
    /// `WHAT.png`.
    fn shot_of(&mut self, what: &str, rect: Rect) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let x0 = rect.x.clamp(0, width - 1);
        let y0 = rect.y.clamp(0, height - 1);
        let (w, h) = (rect.width.min(width - x0), rect.height.min(height - y0));
        let mut image = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let at = usize::try_from((y0 + y) * width + x0 + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        std::fs::create_dir_all(&folder)?;
        image.save(Path::new(&folder).join(format!("{what}.png")))?;
        Ok(())
    }
}

#[test]
fn each_applications_level_applies_and_a_bubbles_menu_writes_it() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    use hocus_link::NotifyLevel;
    let mut bed = start(&shell, |_, config| {
        let apps = &mut config.notifications.apps;
        apps.insert("Thunderbird".to_owned(), NotifyLevel::History);
        apps.insert("org.mozilla.firefox".to_owned(), NotifyLevel::None);
    })?;
    // "Historique seulement": no bubble, in the history, counted.
    bed.notify(Notice {
        app: "Thunderbird",
        summary: "3 nouveaux messages",
        ..Notice::default()
    })?;
    bed.logged("level=\"history\"")?;
    bed.logged("unread=1")?;
    // "Rien": nowhere, by its desktop entry; its sender hears it closed.
    let dropped = bed.notify_from(
        Notice {
            app: "Firefox",
            summary: "Téléchargement terminé",
            ..Notice::default()
        },
        "org.mozilla.firefox",
    )?;
    bed.hears(&format!("closed {dropped} 1"))?;
    bed.logged("a notification dropped")?;
    assert!(bed.bubbles().is_empty(), "{:?}", bed.bubbles());
    assert!(!bed.log().contains("unread=2"), "{}", bed.log());
    // A bubble's "⋯": its application's levels, "Bulle et son" ticked;
    // "Historique seulement" chosen is written, and applies at once.
    bed.notify(Notice {
        timeout: 0,
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.click(more_of(card))?;
    bed.logged("the menu of levels shows it: app=Slack for=\"Slack\" level=all")?;
    let history = bed.level_at("History")?;
    bed.click(history)?;
    let text = bed.config_holds("Slack = \"history\"")?;
    assert!(text.starts_with("# Mine.\n"), "{text}");
    assert!(text.contains("[notifications.apps]"), "{text}");
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).is_empty())?;
    assert!(gone, "the menu stayed");
    let mark = bed.mark();
    bed.notify(Notice {
        summary: "Encore Camille",
        ..Notice::default()
    })?;
    bed.logged_since(mark, "level=\"history\"")?;
    Ok(())
}

impl Bed {
    /// Sends `notice` with its desktop entry `entry`; its id.
    fn notify_from(&mut self, notice: Notice<'_>, entry: &str) -> anyhow::Result<u32> {
        let entry = OwnedVariant::try_from(Variant::from(entry))?;
        self.notify_hinted(notice, vec![("desktop-entry", entry)])
    }
}

/// The pull request's screenshots (#899): a bubble with its menu of levels
/// open, and a line of the history with its own; to `$HOCUS_SHOTS`.
#[test]
#[ignore = "writes the pull request's screenshots under HOCUS_SHOTS"]
fn levels_shots() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let history = kept("Thunderbird", "3 nouveaux messages", 40, false)
        + &kept("Firefox", "Téléchargement terminé", 70, false);
    let mut bed = start_with(
        &shell,
        |_, config| {
            config
                .notifications
                .apps
                .insert("Slack".to_owned(), hocus_link::NotifyLevel::Silent);
        },
        &history,
    )?;
    bed.logged("the history read back kept=2")?;
    bed.notify(Notice {
        summary: "Julie · #projet-leroy",
        body: "Le client valide le devis",
        timeout: 0,
        ..Notice::default()
    })?;
    let card = bed.shows(1)?[0];
    bed.click(more_of(card))?;
    bed.level_at("Silent")?;
    bed.hp.pointer_move(400.0, 600.0);
    let menu = common::drawn_popovers(&bed.hp)[0];
    bed.shot_of(
        "bubble-menu",
        Rect {
            x: card.x - 24,
            y: 0,
            width: 1920 - (card.x - 24),
            height: menu.y + menu.height + 24,
        },
    )?;
    bed.hp.key(KEY_ESC, true);
    bed.hp.key(KEY_ESC, false);
    bed.hp
        .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).is_empty())?;
    bed.open_history()?;
    let popover = bed.history();
    let more = bed
        .history_places()
        .into_iter()
        .filter(|(who, _)| who.starts_with("More("))
        .nth(1)
        .map(|(_, at)| at)
        .ok_or_else(|| anyhow::anyhow!("no ⋯ in the history"))?;
    let at = (f64::from(popover.x) + more.0, f64::from(popover.y) + more.1);
    // The line under the pointer shows its ✕ and its "⋯".
    common::point(&mut bed.hp, (at.0 - 120.0, at.1))?;
    bed.idle(Duration::from_millis(200))?;
    bed.shot_of(
        "history-more",
        Rect {
            x: popover.x - 16,
            y: popover.y - 16,
            width: popover.width + 32,
            height: popover.height + 32,
        },
    )?;
    common::point(&mut bed.hp, at)?;
    bed.idle(Duration::from_millis(100))?;
    bed.hp.button(BTN_RIGHT, true);
    bed.hp.button(BTN_RIGHT, false);
    bed.level_at("All")?;
    bed.hp.pointer_move(400.0, 600.0);
    let menu = common::drawn_popovers(&bed.hp)[0];
    bed.shot_of(
        "history-menu",
        Rect {
            x: menu.x - 24,
            y: menu.y - 24,
            width: 1920 - (menu.x - 24),
            height: menu.height + 48,
        },
    )
}

/// A line of the history's file as #923 writes it: its number, its state
/// (`new`, `noticed`, `seen`), an agent's pin and reason.
fn kept_923(
    id: u64,
    app: &str,
    summary: &str,
    body: &str,
    ago: i64,
    state: &str,
    pin: Option<(&str, &str)>,
) -> String {
    let mut line = serde_json::json!({
        "id": id,
        "app": app,
        "entry": null,
        "app_icon": "",
        "image_path": null,
        "summary": summary,
        "body": body,
        "pid": 0,
        "agent": false,
        "at": NOW_MS - ago * MINUTE_MS,
        "state": state,
    });
    if let Some((agent, reason)) = pin {
        line["pin"] = serde_json::json!({ "agent": agent, "reason": reason, "at": NOW_MS });
    }
    format!("{line}\n")
}

/// The pull request's screenshots (#923): the bell grouped by
/// application, an agent's pin on top; Chat's "N vues" unfolded, then its
/// card unfolded; empty. To `$HOCUS_SHOTS`.
#[test]
#[ignore = "writes the pull request's screenshots under HOCUS_SHOTS"]
fn bell_shots() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut history = kept_923(
        1,
        "Courriel",
        "Facture d’octobre",
        "Bonjour, veuillez trouver ci-joint la facture du mois d’octobre.",
        25,
        "new",
        Some(("secretaire", "À payer avant vendredi")),
    );
    let chat_new = [
        (
            "Camille dans #design",
            "J’ai mis à jour la maquette du lanceur, tu peux jeter un œil ?",
            2,
        ),
        ("Hugo dans #dev", "La CI est verte, je fusionne ?", 9),
        (
            "Inès dans #général",
            "Pot vendredi à 17 h, tout le monde est invité !",
            14,
        ),
        (
            "Noah dans #général",
            "Qui a pris le chargeur de la salle Hublot ?",
            21,
        ),
    ];
    let mut id = 1;
    for (title, body, ago) in chat_new {
        id += 1;
        history += &kept_923(id, "Chat", title, body, ago, "new", None);
    }
    id += 1;
    history += &kept_923(
        id,
        "Agenda",
        "Point produit à 14 h 45",
        "Salle Hublot",
        40,
        "noticed",
        None,
    );
    id += 1;
    history += &kept_923(
        id,
        "Navigateur",
        "Téléchargement terminé",
        "rapport-annuel.pdf · 2,4 Mo",
        70,
        "noticed",
        None,
    );
    let chat_seen = [
        (
            "Léa dans #design",
            "Les icônes sont dans le dossier partagé.",
        ),
        ("Hugo dans #dev", "Je regarde la PR après manger."),
        ("Camille", "Merci !"),
        (
            "Inès dans #général",
            "Rappel : réunion d’équipe demain 10 h.",
        ),
        ("Noah", "Tu as deux minutes ?"),
        ("Léa dans #design", "Version 3 en ligne."),
        ("Hugo dans #dev", "Le build de nuit est passé."),
        ("Camille dans #design", "On garde le vert ?"),
        ("Inès", "Photo du tableau blanc"),
        ("Noah dans #général", "Café ?"),
        ("Hugo", "Ok pour moi"),
        ("Léa", "À demain"),
    ];
    for (i, (title, body)) in chat_seen.iter().enumerate() {
        id += 1;
        history += &kept_923(id, "Chat", title, body, 95 + 25 * i as i64, "seen", None);
    }
    for (title, body, ago) in [
        (
            "Relance : contrat à signer",
            "Le contrat de location attend toujours ta signature.",
            180,
        ),
        ("Ta commande est expédiée", "Livraison prévue jeudi.", 220),
        ("Lettre d’information", "Les nouveautés de novembre.", 300),
    ] {
        id += 1;
        history += &kept_923(id, "Courriel", title, body, ago, "seen", None);
    }
    for (title, body, ago) in [
        (
            "12 mises à jour disponibles",
            "Dont une mise à jour de sécurité.",
            1500,
        ),
        ("Mises à jour installées", "Redémarre pour finir.", 3100),
    ] {
        id += 1;
        history += &kept_923(id, "Mises à jour", title, body, ago, "seen", None);
    }
    let mut bed = start_with(&shell, |_, _| {}, &history)?;
    bed.logged("the history read back")?;
    let crop = |bed: &Bed| {
        let popover = bed.history();
        Rect {
            x: popover.x - 16,
            y: popover.y - 16,
            width: popover.width + 32 + 8 + 196 + 8,
            height: popover.height + 32,
        }
    };
    bed.open_history()?;
    bed.hp.pointer_move(400.0, 600.0);
    let rect = crop(&bed);
    bed.shot_of("bell-grouped", rect)?;
    // Chat's "12 vues" unfolded.
    for (target, name) in [
        ("Fold(chat)", "bell-seen-unfolded"),
        ("Expand(chat)", "bell-card-unfolded"),
    ] {
        let at = bed.history_item(target)?;
        let mark = bed.mark();
        common::point(&mut bed.hp, at)?;
        bed.idle(Duration::from_millis(100))?;
        bed.click(at)?;
        bed.logged_since(mark, "the history shows")?;
        bed.logged_since(mark, "\"history:Switch\"")?;
        bed.hp.pointer_move(400.0, 600.0);
        let rect = crop(&bed);
        bed.shot_of(name, rect)?;
    }
    // Empty: "Tout effacer".
    let clear = bed.history_item("Clear")?;
    let mark = bed.mark();
    common::point(&mut bed.hp, clear)?;
    bed.idle(Duration::from_millis(100))?;
    bed.click(clear)?;
    let shows = bed.logged_since(mark, "history=Rien de neuf")?;
    let emptied = bed
        .log()
        .get(mark..)
        .and_then(|new| new.find(&shows))
        .map_or(mark, |at| mark + at + shows.len());
    bed.logged_since(emptied, "\"history:Switch\"")?;
    bed.hp.pointer_move(400.0, 600.0);
    let rect = crop(&bed);
    bed.shot_of("bell-empty", rect)
}
