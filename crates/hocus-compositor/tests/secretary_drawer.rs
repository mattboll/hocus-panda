//! The secretary's drawer (docs/design/secretary.md §5, lot 2): the real
//! shell and her real process, on a clock of her own, with the scripted
//! fake model, its recorder and a fixture board.
//!
//! - AC4: a click on her head opens a 420 px drawer 8 px from the bar, its
//!   top and bottom the bar's; 360 px on a 1366 px screen; no window moves.
//! - AC5: `Super+S` toggles it, `Esc` closes it, a window focused leaves
//!   it open; in classic mode `Super+S` is no shortcut.
//! - AC6: a sentence typed shows the user's bubble, her typing, then her
//!   answer as her clock lets it come.
//! - AC7: a word she does not know on one project's board: one question
//!   with its quick replies; "Oui" keeps it, and it is not asked again.
//! - AC8: the agents' line says 4.6's words, unfolds her briefing, and a
//!   line goes to its agent.
//! - AC24: a card proposes; nothing happens before its click.
//! - AC35: her moods' marks show; with reduced motion nothing moves.
//! - Her first run (§15, #750): with two Google accounts in a fake Online
//!   Accounts on a private bus, one consent card at a time, "Autoriser"
//!   writing the grant in the notebook chosen, then « Dis-moi sur quoi tu
//!   travailles et avec qui. »; without a key in the keyring, her status
//!   says so and how to give one.
//!
//! Pixman, so the shell draws on the CPU, opaque and still, as in CI.
//! With `HOCUS_DRAWER_SHOTS=DIR`, the tests leave PNGs of the drawer
//! there (screenshots for review, never compared).

mod common;

#[path = "../../hocus-day/tests/fakes/mod.rs"]
mod fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

use common::{KEY_ESC, KEY_LEFTMETA, WAIT, chord, wait};
use hocus_agents::tools::Places;
use hocus_compositor::{
    Config, Headless, HeadlessConfig, RendererChoice, SecretaryMode, model::Rect,
};
use hocus_link::ToShell;
use hocus_secretary_link::{Client, Connection, Status, ToSecretary};

const KEY_S: u32 = 31;
const KEY_ENTER: u32 = 28;

/// Chapitô's board, for AC7.
const BOARD: &str = r#"
name = "fixture-board"
notebook = "pro"
every = 300
[[items]]
id = "CHAP-412"
at = "2026-11-09T07:00:00Z"
kind = "ticket"
digest = "CHAP-412 · Égaluantes : arrondis des tarifs"
project = "Chapitô"
"#;

/// Her model: a reply that streams a word a second of her clock, one
/// that proposes a message, one that takes its time.
const MODEL: &str = r#"
name = "Claude Sonnet 5.5"

[[reply]]
role = "answer"
contains = ["son article"]
text = "Je te propose ce message pour Léa."
[[reply.tool_calls]]
name = "send_message"
input = { to = "Léa", draft = "Je relis ton article cet après-midi, je te dis vers 16 h." }

[[reply]]
role = "answer"
contains = ["réfléchis longuement"]
text = "Voilà."
delay = 60
doing = "elle lit le fil de Léa"

[default]
text = "Bien noté, je range ta journée."
delay = 1
pace = 1
doing = "elle regarde ton agenda"
"#;

/// The programs: the shell, hers, `hocus`.
fn ready() -> Option<(PathBuf, PathBuf, PathBuf)> {
    let shell = common::shell_program()?;
    let secretary = common::secretary_program()?;
    let exe = std::env::current_exe().ok()?;
    let hocus = exe.parent()?.parent()?.join("hocus");
    hocus.exists().then_some((shell, secretary, hocus))
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    hocus: PathBuf,
    /// A line to her, in assistant mode.
    her: Option<Connection>,
    /// Online Accounts faked on a private bus, with Google's services
    /// nowhere (her real model's routes, no key): her first run.
    _google: Option<(fakes::FakeGoa, fakes::PrivateBus)>,
}

#[derive(Clone, Copy)]
struct Setup {
    output: &'static str,
    mode: SecretaryMode,
    reduced_motion: bool,
    /// Her Google accounts (two, in a fake Online Accounts) instead of a
    /// fixture board, her real providers (on nothing, with no key)
    /// instead of the fake model.
    google: bool,
    /// With `google`: her Anthropic API a fake on this machine that
    /// refuses every key (#752), instead of nothing.
    refusing_api: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            output: "1920x1080",
            mode: SecretaryMode::Assistant,
            reduced_motion: false,
            google: false,
            refusing_api: false,
        }
    }
}

fn start(setup: Setup) -> anyhow::Result<Option<Bed>> {
    let Some((shell, secretary, hocus)) = ready() else {
        eprintln!("the shell, hocus-secretary or hocus is not built: skipped");
        return Ok(None);
    };
    let dir = tempfile::tempdir()?;
    let home = dir.path().join("home");
    std::fs::create_dir_all(home.join(".local/state/hocuspanda"))?;
    // Day.1's first-run card aside: the agents' group under the clock.
    std::fs::write(home.join(".local/state/hocuspanda/day-sources"), "later\n")?;
    std::fs::create_dir_all(home.join("bin"))?;
    let model = dir.path().join("model.toml");
    std::fs::write(&model, MODEL)?;
    let board = dir.path().join("board.toml");
    std::fs::write(&board, BOARD)?;
    let her = dir.path().join("secretary");
    let google = if setup.google {
        let bus = fakes::PrivateBus::start(dir.path())?;
        let goa = fakes::FakeGoa::start(&bus.address)?;
        goa.add("account_1700000000_0", "google", "alex@atelier-lune.fr")?;
        goa.add("account_1700000000_1", "google", "alex.martin@gmail.com")?;
        Some((goa, bus))
    } else {
        None
    };
    let routes = match &google {
        // Her providers on a port where nothing listens, no key in a
        // keyring: nothing reaches a model or the network.
        Some((_, bus)) => {
            let config = dir.path().join("secretary.toml");
            let api = if setup.refusing_api {
                refusing_api()?
            } else {
                "http://127.0.0.1:9".to_owned()
            };
            // Her name, with the refusing API (#752).
            let name = if setup.refusing_api {
                "[secretary]\nname = \"Margot\"\n"
            } else {
                ""
            };
            std::fs::write(
                &config,
                format!(
                    "{name}[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{api}\"\n\
                     [secretary.providers.local]\nkind = \"ollama\"\nurl = \"http://127.0.0.1:9/v1\"\n"
                ),
            )?;
            format!(
                "--bus '{}' --google-at http://127.0.0.1:9 --config '{}'",
                bus.address,
                config.display()
            )
        }
        None => format!(
            "--fake-model '{}' --recorder '{}' --fixture '{}'",
            model.display(),
            dir.path().join("calls.jsonl").display(),
            board.display(),
        ),
    };
    std::fs::write(
        &her,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\nexport XDG_DATA_HOME='{}'\n\
             exec '{}' --virtual-clock 2026-11-09T08:00:00Z {routes} \"$@\" 2>>'{}'\n",
            dir.path().join("data").display(),
            secretary.display(),
            dir.path().join("secretary.log").display(),
        ),
    )?;
    std::fs::set_permissions(&her, std::fs::Permissions::from_mode(0o755))?;
    let log = dir.path().join("shell.log");
    let script = dir.path().join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' \
             RUST_LOG=warn,hocuspanda_shell=debug\nexec '{}' \"$@\" 2>>'{}'\n",
            home.display(),
            shell.display(),
            log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        secretary_program: Some(her),
        agent_places: Some(Places {
            home: home.clone(),
            config_home: home.join(".config"),
            codex_home: home.join(".codex"),
            path: vec![home.join("bin")],
            herdr_socket: home.join(".config/herdr/herdr.sock"),
            processes: home.join("proc"),
            hocus: hocus.clone(),
            state_dir: Some(home.join("state")),
        }),
        ..Config::default()
    };
    config.secretary.mode = setup.mode;
    config.appearance.reduced_motion = setup.reduced_motion;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![setup.output.parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let her = if setup.mode == SecretaryMode::Assistant {
        let mut her = connect(&mut hp)?;
        // The shell talks to her, and her board is read.
        status_until(&mut hp, &mut her, |s| {
            s.connections
                .iter()
                .any(|c| c.client == Some(Client::Shell))
                && s.sources.iter().all(|s| !s.busy && s.synced_at.is_some())
        })?;
        Some(her)
    } else {
        None
    };
    Ok(Some(Bed {
        dir,
        hp,
        log,
        hocus,
        her,
        _google: google,
    }))
}

/// An Anthropic API on this machine that refuses every key with a `401`,
/// until the test ends: its address.
fn refusing_api() -> anyhow::Result<String> {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let url = format!("http://{}", listener.local_addr()?);
    std::thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut buffer = [0u8; 4096];
            let _ = stream.read(&mut buffer);
            let body = r#"{"type":"error","error":{"type":"authentication_error"}}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 401 X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Ok(url)
}

fn connect(hp: &mut Headless) -> anyhow::Result<Connection> {
    let socket = hp
        .secretary_socket()
        .map(Path::to_path_buf)
        .ok_or_else(|| anyhow::anyhow!("no socket"))?;
    let start = Instant::now();
    loop {
        if let Ok(c) = Connection::open(&socket, Client::Cli, Duration::from_secs(5)) {
            return Ok(c);
        }
        anyhow::ensure!(start.elapsed() < WAIT, "she never listened");
        hp.dispatch(Duration::from_millis(20))?;
    }
}

fn status_until(
    hp: &mut Headless,
    her: &mut Connection,
    mut done: impl FnMut(&Status) -> bool,
) -> anyhow::Result<Status> {
    let start = Instant::now();
    loop {
        let status = her.status().map_err(anyhow::Error::msg)?;
        if done(&status) {
            return Ok(status);
        }
        anyhow::ensure!(start.elapsed() < WAIT, "never: {status:?}");
        hp.dispatch(Duration::from_millis(20))?;
    }
}

impl Bed {
    fn drawer(&self) -> Option<Rect> {
        self.hp.drawer_surfaces().first().map(|(_, r)| *r)
    }

    /// The drawer, once it shows and has drawn.
    fn opened(&mut self) -> anyhow::Result<Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            !hp.drawn_layers(hocus_link::SECRETARY_NAMESPACE).is_empty()
        })?;
        if !done {
            let said: Vec<String> = common::logged(&self.log)
                .lines()
                .filter(|l| !l.contains("icons"))
                .map(str::to_owned)
                .collect();
            anyhow::bail!(
                "no drawer: {:?}; the shell said:\n{}",
                self.hp.drawer_surfaces(),
                said.join("\n")
            );
        }
        self.shows(|s| s["height"].as_f64().is_some_and(|h| h > 0.0))?;
        self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))
    }

    fn closed(&mut self) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.drawer_surfaces().is_empty())?;
        anyhow::ensure!(done, "the drawer stays: {:?}", self.hp.drawer_surfaces());
        Ok(())
    }

    /// What the drawer last said it shows, once `done` holds for it.
    fn shows(
        &mut self,
        mut done: impl FnMut(&serde_json::Value) -> bool,
    ) -> anyhow::Result<serde_json::Value> {
        let log = self.log.clone();
        let found = self.hp.dispatch_until(WAIT, |_| {
            common::last_json(&log, "the drawer shows {").is_some_and(|s| done(&s))
        })?;
        let last = common::last_json(&self.log, "the drawer shows {");
        anyhow::ensure!(found, "the drawer never showed it; last: {last:?}");
        Ok(last.unwrap_or_default())
    }

    /// Where the drawer's `item` is, global, as it last logged its places.
    fn place(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let wanted = format!("{item}@");
        let log = self.log.clone();
        let found = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the drawer's items"))
                .and_then(|l| {
                    let at = l.split(&wanted).nth(1)?;
                    let at = at.split_whitespace().next()?;
                    let (x, y) = at.split_once(',')?;
                    Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| found(&common::logged(&log)).is_some())?;
        anyhow::ensure!(done, "no {item} in the drawer");
        let (x, y) = found(&common::logged(&self.log)).unwrap_or_default();
        let r = self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))?;
        Ok((f64::from(r.x) + x, f64::from(r.y) + y))
    }

    /// The drawer's last places' item whose name starts with `prefix`.
    fn place_starting(&mut self, prefix: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the drawer's items"))
                .and_then(|l| {
                    l.split_whitespace()
                        .find(|w| w.starts_with(prefix))
                        .and_then(|w| w.split('@').next())
                        .map(str::to_owned)
                })
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&common::logged(&log)).is_some())?;
        anyhow::ensure!(done, "no {prefix}… in the drawer");
        Ok(find(&common::logged(&self.log)).unwrap_or_default())
    }

    fn click(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        common::click_on(&mut self.hp, at, hocus_link::SECRETARY_NAMESPACE)
    }

    fn super_s(&mut self) {
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_S]);
    }

    fn type_keys(&mut self, keys: &[u32]) {
        for key in keys {
            chord(&mut self.hp, &[*key]);
        }
    }

    fn agent(&self, name: &str, state: &str) -> anyhow::Result<()> {
        let socket = self
            .hp
            .control_socket()
            .ok_or_else(|| anyhow::anyhow!("no control socket"))?
            .to_path_buf();
        let status = Command::new(&self.hocus)
            .arg("--socket")
            .arg(socket)
            .args(["agent", name, state])
            .status()?;
        anyhow::ensure!(status.success(), "hocus agent {name} {state}");
        Ok(())
    }

    /// Her clock moved on by `seconds`.
    fn advance(&mut self, seconds: u64) -> anyhow::Result<()> {
        self.her
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no secretary"))?
            .send(&ToSecretary::Clock { advance: seconds })
            .map_err(anyhow::Error::msg)
    }

    /// What the recorder holds: (model calls, effects).
    fn recorded(&self) -> (usize, Vec<String>) {
        recorded(self.dir.path())
    }

    /// A PNG of the drawer and the bar beside it, when the run asks for
    /// screenshots (`HOCUS_DRAWER_SHOTS`).
    fn shot(&mut self, name: &str) -> anyhow::Result<()> {
        let Some(dir) = std::env::var_os("HOCUS_DRAWER_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        wait(&mut self.hp, Duration::from_millis(300))?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let Some(r) = self.drawer() else {
            return Ok(());
        };
        let x0 = (r.x - 8).max(0);
        let x1 = width;
        let (y0, y1) = (0, height);
        let mut out = image::RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
        for y in y0..y1 {
            for x in x0..x1 {
                let at = ((y * width + x) * 4) as usize;
                if let Some(p) = bytes.get(at..at + 4) {
                    out.put_pixel(
                        (x - x0) as u32,
                        (y - y0) as u32,
                        image::Rgba([p[2], p[1], p[0], 255]),
                    );
                }
            }
        }
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir)?;
        out.save(dir.join(format!("{name}.png")))?;
        Ok(())
    }
}

/// What her recorder wrote in `dir`: how many model calls, and the
/// effects' kinds (one JSON object per line).
fn recorded(dir: &Path) -> (usize, Vec<String>) {
    let text = std::fs::read_to_string(dir.join("calls.jsonl")).unwrap_or_default();
    let mut calls = 0;
    let mut effects = Vec::new();
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        // Her answers' calls: her morning briefing's (lot 5) aside.
        if value.get("call").is_some() && value["call"]["request"]["role"] != "briefing" {
            calls += 1;
        }
        if let Some(kind) = value["effect"]["kind"].as_str() {
            effects.push(kind.to_owned());
        }
    }
    (calls, effects)
}

/// The pixel at (`x`, `y`) of output 0, as R, G, B.
fn pixel(hp: &mut Headless, (x, y): (f64, f64)) -> anyhow::Result<[i32; 3]> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = ((y as i32 * width + x as i32) * 4) as usize;
    let p = bytes
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("({x}, {y}) is off the screen"))?;
    Ok([i32::from(p[2]), i32::from(p[1]), i32::from(p[0])])
}

/// Her head's place in the agents' group, as the shell logged it, global.
fn her_head(bed: &mut Bed) -> anyhow::Result<(f64, f64)> {
    let log = bed.log.clone();
    let found = |text: &str| {
        text.lines()
            .rev()
            .find(|l| l.contains("the agents group's items"))
            .and_then(|l| {
                let at = l.split("Secretary@").nth(1)?;
                let at = at.split_whitespace().next()?;
                let (x, y) = at.split_once(',')?;
                Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?))
            })
    };
    let done = bed
        .hp
        .dispatch_until(WAIT, |_| found(&common::logged(&log)).is_some())?;
    anyhow::ensure!(done, "no secretary in the agents' group");
    let (x, y) = found(&common::logged(&bed.log)).unwrap_or_default();
    let right = f64::from(bed.hp.outputs()[0].1.width) - 8.0;
    Ok((right + x, 8.0 + y))
}

/// AC4: a click on her head; 420 wide, its right edge 8 px from the bar,
/// its top and bottom the bar's; no window moves; a second click closes.
#[test]
fn a_click_on_her_head_opens_420_px_against_the_bar() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    let window = common::open(&mut bed.hp, "drawer.test", (800, 600))?;
    bed.agent("build", "working")?;
    let head = her_head(&mut bed)?;
    common::click_on(&mut bed.hp, head, hocus_link::AGENTS_NAMESPACE)?;
    let drawer = bed.opened()?;
    let bar = bed
        .hp
        .layer_rects(hocus_link::NAMESPACE)
        .first()
        .map(|(_, r)| *r)
        .ok_or_else(|| anyhow::anyhow!("no bar"))?;
    assert_eq!(drawer.width, 420);
    assert_eq!(drawer.x + drawer.width, bar.x - 8, "8 px from the bar");
    assert_eq!(
        (drawer.y, drawer.height),
        (bar.y, bar.height),
        "the bar's top and bottom"
    );
    assert_eq!(
        common::rect_of(&bed.hp, "drawer.test"),
        Some(window),
        "no window moved"
    );
    // Its field holds the keyboard as it opens.
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(focused, "the field focused: {:?}", bed.hp.keyboard_layer());
    bed.shot("drawer-open")?;
    common::click_on(&mut bed.hp, head, hocus_link::AGENTS_NAMESPACE)?;
    bed.closed()
}

/// AC4: on a 1366 px screen, 360 wide, against the mini bar.
#[test]
fn on_a_1366_px_screen_it_is_360_px() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup {
        output: "1366x768",
        ..Setup::default()
    })?
    else {
        return Ok(());
    };
    bed.super_s();
    let drawer = bed.opened()?;
    assert_eq!(drawer.width, 360);
    assert_eq!(
        drawer.x + drawer.width,
        1366 - 8 - 56 - 8,
        "beside the mini bar"
    );
    assert_eq!((drawer.y, drawer.height), (8, 768 - 16));
    bed.shot("drawer-1366")?;
    Ok(())
}

/// AC5: `Super+S` toggles it; a window focused leaves it open; `Esc`
/// closes it.
#[test]
fn super_s_toggles_it_esc_closes_it_and_a_window_focused_leaves_it() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    let window = common::open(&mut bed.hp, "drawer.test", (600, 400))?;
    bed.super_s();
    bed.opened()?;
    // A click in the window: it takes the keyboard, the drawer stays.
    let inside = (f64::from(window.x) + 50.0, f64::from(window.y) + 50.0);
    common::click(&mut bed.hp, inside)?;
    let moved = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() != Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(moved, "the window took the keyboard");
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(
        bed.drawer().is_some(),
        "focusing a window does not close it"
    );
    bed.super_s();
    bed.closed()?;
    bed.super_s();
    bed.opened()?;
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(focused);
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.closed()
}

/// §21: in classic mode `Super+S` opens nothing.
#[test]
fn in_classic_mode_super_s_is_no_shortcut() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup {
        mode: SecretaryMode::Classic,
        ..Setup::default()
    })?
    else {
        return Ok(());
    };
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_S]);
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert!(bed.hp.drawer_surfaces().is_empty());
    Ok(())
}

/// AC6: a sentence typed and sent: its bubble, her typing (and what she
/// does), her answer a word at a time as her clock moves, then whole.
#[test]
fn a_sentence_shows_its_bubble_her_typing_then_her_answer_as_it_comes() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    bed.shows(|s| {
        s["model"] == "Claude Sonnet 5.5"
            && s["status"]
                .as_str()
                .is_some_and(|t| t.starts_with("Planka") && t.ends_with("à jour"))
    })?;
    // "bonjour", then Enter.
    bed.type_keys(&[48, 24, 49, 36, 24, 22, 19]);
    bed.shows(|s| s["field"] == 7)?;
    bed.type_keys(&[KEY_ENTER]);
    let typing = bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains("user:bonjour") && items.contains("typing:elle regarde ton agenda")
    })?;
    assert_eq!(typing["field"], 0, "the field empties");
    bed.shows(|s| s["mood"] == "type")?;
    bed.shot("drawer-typing")?;
    // Her first word after a second of her clock, the next a second
    // later: her typing gives way to her words.
    bed.advance(1)?;
    bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains("streaming:Bien\"") && !items.contains("typing:")
    })?;
    bed.advance(1)?;
    bed.shows(|s| s["items"].to_string().contains("streaming:Bien noté,\""))?;
    bed.shot("drawer-streaming")?;
    bed.advance(10)?;
    let done = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("her:Bien noté, je range ta journée.")
    })?;
    assert!(!done["items"].to_string().contains("streaming:"));
    bed.shows(|s| s["mood"] == "listen")?;
    Ok(())
}

/// AC7: one question naming the project, its quick replies; "Oui" keeps
/// it ("Retenu"), and the word is not asked about again.
#[test]
fn a_word_on_one_projects_board_is_asked_once_with_quick_replies() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    bed.hp.tell_shell(ToShell::SecretaryAsk(
        "aujourd'hui je vais finir le ticket sur les égaluantes".to_owned(),
    ));
    let asked = bed.shows(|s| s["items"].to_string().contains("question:"))?;
    let items = asked["items"].to_string();
    assert_eq!(items.matches("question:").count(), 1, "{items}");
    assert!(
        items.contains("Chapitô") && items.contains("Oui, Chapitô | Non, autre chose"),
        "{items}"
    );
    assert_eq!(asked["mood"], "ask");
    assert_eq!(
        bed.recorded().0,
        0,
        "no model call for a question code asks"
    );
    bed.shot("drawer-quick-replies")?;
    let yes = bed.place_starting("Reply(")?;
    assert!(yes.ends_with(",0)"), "{yes}");
    let at = bed.place(&yes)?;
    bed.click(at)?;
    bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains("learned:Retenu\u{a0}: «\u{a0}égaluantes\u{a0}» → Chapitô")
            && items.contains("chosen 0")
    })?;
    bed.advance(30)?;
    bed.shows(|s| s["items"].to_string().contains("her:Bien noté"))?;
    bed.shot("drawer-learned")?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("les égaluantes avancent".to_owned()));
    bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("user:les égaluantes avancent")
    })?;
    bed.advance(30)?;
    let after = bed.shows(|s| s["items"].to_string().matches("her:Bien noté").count() == 2)?;
    assert_eq!(
        after["items"].to_string().matches("question:").count(),
        1,
        "not asked again"
    );
    Ok(())
}

/// AC24: her model tries to send a message: a card; nothing is sent
/// before "Envoyer" on it, and once after.
#[test]
fn a_card_proposes_and_nothing_happens_before_its_click() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    bed.hp.tell_shell(ToShell::SecretaryAsk(
        "prépare un message pour Léa sur son article".to_owned(),
    ));
    bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("card:Draft:Message à Léa:Envoyer/Modifier/Annuler")
    })?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.recorded().1.is_empty(), "nothing sent before the click");
    bed.shot("drawer-card")?;
    let accept = bed.place_starting("Button(")?;
    assert!(accept.ends_with("Accept)"), "{accept}");
    let at = bed.place(&accept)?;
    bed.click(at)?;
    bed.shows(|s| s["items"].to_string().contains("→ Fait."))?;
    let dir = bed.dir.path().to_path_buf();
    let sent = bed
        .hp
        .dispatch_until(WAIT, |_| !recorded(&dir).1.is_empty())?;
    assert!(sent);
    assert_eq!(bed.recorded().1, ["message"], "that card's effect, once");
    bed.shot("drawer-card-accepted")?;
    Ok(())
}

/// AC8: the agents' line says who waits and who is stuck; it unfolds her
/// briefing in the drawer; a line goes to its agent and closes it.
#[test]
fn the_agents_line_unfolds_her_briefing_and_a_line_goes_to_its_agent() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.agent("site-web", "waiting")?;
    bed.agent("infra", "failed")?;
    bed.agent("migration", "done")?;
    bed.super_s();
    bed.opened()?;
    let shown = bed.shows(|s| s["agents"].as_str().is_some_and(|a| a.contains("3 agents")))?;
    assert_eq!(
        shown["agents"],
        "site-web t’attend · infra bloqué | 3 agents ›"
    );
    let line = bed.place("Agents")?;
    bed.click(line)?;
    bed.shows(|s| s["briefing"] == true)?;
    bed.shot("drawer-briefing")?;
    let id = bed
        .hp
        .agents_peek()
        .iter()
        .find(|a| a.name == "migration")
        .map(|a| a.id)
        .ok_or_else(|| anyhow::anyhow!("no migration"))?;
    let at = bed.place(&format!("Agent({id})"))?;
    bed.click(at)?;
    bed.closed()?;
    let seen = bed.hp.dispatch_until(WAIT, |hp| {
        hp.agents_peek()
            .iter()
            .any(|a| a.name == "migration" && a.state == "seen")
    })?;
    assert!(seen, "gone to: {:?}", bed.hp.agents_peek());
    Ok(())
}

/// AC35: her moods' marks on the screen (her ear's colour: green
/// listening, amber asking); with reduced motion, nothing moves.
#[test]
fn her_moods_show_and_reduced_motion_moves_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup {
        reduced_motion: true,
        ..Setup::default()
    })?
    else {
        return Ok(());
    };
    bed.super_s();
    let drawer = bed.opened()?;
    bed.shows(|s| s["mood"] == "listen")?;
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // Her ear, at (85, 33) of her 120-unit drawing, 52 px at (12, 10).
    let ear = (
        f64::from(drawer.x) + 12.0 + 85.0 * 52.0 / 120.0,
        f64::from(drawer.y) + 10.0 + 33.0 * 52.0 / 120.0,
    );
    let green = pixel(&mut bed.hp, ear)?;
    assert!(
        green[1] > green[0] && green[1] > green[2],
        "green: {green:?}"
    );
    // Nothing draws at rest, reduced motion or not.
    let commits = bed.hp.drawer_commits();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.drawer_commits(), commits, "nothing moves");
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("je finis les égaluantes".to_owned()));
    bed.shows(|s| s["mood"] == "ask")?;
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let amber = pixel(&mut bed.hp, ear)?;
    assert!(
        amber[0] > 200 && amber[1] > 140 && amber[2] < 120,
        "amber: {amber:?}"
    );
    bed.shot("drawer-mood-ask")?;
    let commits = bed.hp.drawer_commits();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(
        bed.hp.drawer_commits(),
        commits,
        "her ask's motion does not play"
    );
    Ok(())
}

/// The button `decision` of the newest consent card still waiting (its
/// "Autoriser" shows), as the drawer last logged its places.
fn button_of(bed: &mut Bed, decision: &str) -> anyhow::Result<(f64, f64)> {
    let log = bed.log.clone();
    let newest = |text: &str| -> Option<u64> {
        let line = text
            .lines()
            .rev()
            .find(|l| l.contains("the drawer's items"))?;
        line.split_whitespace()
            .filter_map(|w| w.strip_prefix("Button("))
            .filter_map(|w| w.split('@').next())
            .filter(|w| w.ends_with(",Accept)"))
            .filter_map(|w| w.split(',').next()?.parse::<u64>().ok())
            .max()
    };
    let done = bed
        .hp
        .dispatch_until(WAIT, |_| newest(&common::logged(&log)).is_some())?;
    anyhow::ensure!(done, "no consent card waiting");
    let item = newest(&common::logged(&bed.log)).unwrap_or_default();
    bed.place(&format!("Button({item},{decision})"))
}

/// Her first run (§15, #750): one source at a time, each on its consent
/// card, then « Dis-moi sur quoi tu travailles et avec qui. »; her
/// status says the key she misses, and how to give one.
#[test]
fn her_first_run_asks_one_source_at_a_time_in_the_drawer() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup {
        google: true,
        ..Setup::default()
    })?
    else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    let shown = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("card:Consent:Agenda · alex@atelier-lune.fr")
    })?;
    let items = shown["items"].to_string();
    assert!(items.contains("une source à la fois"), "{items}");
    assert_eq!(
        items.matches("card:Consent").count(),
        1,
        "one at a time: {items}"
    );
    assert_eq!(shown["status"], "Il me manque ma clé Anthropic");
    assert!(
        shown["key_lines"]
            .as_str()
            .is_some_and(|h| h.contains("hocus-secretary key anthropic")),
        "{shown}"
    );
    bed.shot("first-run-consent")?;
    // "Autoriser": the grant, pro; the next source.
    let allow = button_of(&mut bed, "Accept")?;
    bed.click(allow)?;
    let shown = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("card:Consent:Anniversaires · alex@atelier-lune.fr")
    })?;
    assert!(
        shown["items"]
            .to_string()
            .contains("Je lis ton agenda · carnet pro"),
        "{shown}"
    );
    let book = std::fs::read_to_string(
        bed.dir
            .path()
            .join("data/hocuspanda/secretary/consents.json"),
    )?;
    let book: hocus_secretary_link::consent::ConsentBook = serde_json::from_str(&book)?;
    let grant = book
        .grant(&hocus_secretary_link::consent::SourceKey::new(
            // The fake's id is the account's e-mail.
            "alex@atelier-lune.fr",
            hocus_secretary_link::consent::SourceKind::Calendar,
        ))
        .expect("the work calendar granted");
    assert_eq!(grant.notebook(), hocus_secretary_link::Notebook::Pro);
    bed.shot("first-run-granted")?;
    // "Pas maintenant" on the rest, the gmail.com account perso.
    for n in 0..5 {
        let not_now = button_of(&mut bed, "Reject")?;
        let before = bed.shows(|_| true)?["items"].clone();
        bed.click(not_now)?;
        bed.shows(|s| s["items"] != before)?;
        if n == 1 {
            let shown = bed.shows(|s| {
                s["items"]
                    .to_string()
                    .contains("card:Consent:Agenda · alex.martin@gmail.com")
            })?;
            assert!(shown["items"].to_string().contains("Pas maintenant"));
            bed.shot("first-run-perso")?;
        }
    }
    let shown = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("her:Dis-moi sur quoi tu travailles et avec qui.")
    })?;
    assert_eq!(
        shown["items"].to_string().matches("card:Consent").count(),
        6
    );
    bed.shot("first-run-done")?;
    Ok(())
}

/// Her key missing (#752): her band is a masked field « Colle ta clé
/// Anthropic » with « Enregistrer », the keyboard in it; what is typed
/// shows as dots, never in a log; a key the API refuses is said so. The
/// field « Dis-lui quelque chose… » clicked takes the keyboard: its ring
/// and its caret show.
#[test]
fn her_key_is_pasted_in_her_band_masked_and_a_refused_one_is_said() -> anyhow::Result<()> {
    const KEY_K: u32 = 37;
    const KEY_MINUS: u32 = 12;
    const KEY_A: u32 = 30;
    const KEY_N: u32 = 49;
    const KEY_T: u32 = 20;
    const KEY_X: u32 = 45;
    let Some(mut bed) = start(Setup {
        google: true,
        refusing_api: true,
        ..Setup::default()
    })?
    else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    let shown = bed.shows(|s| s["key_field"] == serde_json::json!(false))?;
    assert_eq!(shown["focus"], "key", "{shown}");
    assert_eq!(shown["title"], "Margot", "her name in her header");
    assert!(
        shown["items"]
            .to_string()
            .contains("Bonjour, je suis Margot, ta secrétaire."),
        "{shown}"
    );
    assert!(
        shown["key_lines"]
            .as_str()
            .is_some_and(|l| l.contains("Elle va dans ton trousseau, jamais dans un fichier.")
                && l.contains("hocus-secretary key anthropic, Entrée, puis colle ta clé.")),
        "{shown}"
    );
    bed.shot("key-band")?;
    bed.type_keys(&[KEY_S, KEY_K, KEY_MINUS, KEY_A, KEY_N, KEY_T, KEY_MINUS, KEY_X, KEY_X]);
    bed.shows(|s| s["key_field"] == serde_json::json!(true))?;
    bed.shot("key-band-typed")?;
    bed.type_keys(&[KEY_ENTER]);
    let shown = bed.shows(|s| {
        s["key_lines"]
            .as_str()
            .is_some_and(|l| l.contains("Clé refusée par Anthropic."))
    })?;
    assert_eq!(shown["key_field"], serde_json::json!(false), "it empties");
    bed.shot("key-band-refused")?;
    // The composer clicked: the keyboard types there.
    // " Field", not "KeyField".
    let field = bed.place(" Field")?;
    bed.click(field)?;
    bed.shows(|s| s["focus"] == "field")?;
    bed.shot("field-focused")?;
    let logs = format!(
        "{}{}",
        common::logged(&bed.log),
        common::logged(&bed.dir.path().join("secretary.log"))
    );
    assert!(!logs.contains("sk-ant-xx"), "the key in no log");
    Ok(())
}
