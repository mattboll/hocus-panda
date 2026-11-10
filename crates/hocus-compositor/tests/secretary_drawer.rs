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
//! - Her text copied (#757): a drag selects within a bubble and across
//!   bubbles, a double click a word, a triple click a paragraph; `Ctrl+C`
//!   and the right click's « Copier » copy it; her answers and draft cards
//!   have « Copier » on hover; a masked secret copies as its mask; the
//!   selection goes as the drawer closes, and draws nothing at rest.
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

use common::{BTN_LEFT, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTMETA, WAIT, chord, wait};
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
/// that proposes a message, one that takes its time, one that says a
/// secret masked (first: the others' words are in its request's thread).
const MODEL: &str = r#"
name = "Claude Sonnet 5.5"

[[reply]]
role = "answer"
contains = ["mot de passe du wifi"]
text = "Le code du wifi : [secret masqué — voir le mail de Léa]."

[[reply]]
role = "answer"
contains = ["son article"]
text = "Je te propose ce message pour Léa."
[[reply.tool_calls]]
name = "send_message"
input = { to = "Léa Martin", channel = "slack", draft = "Je relis ton article cet après-midi, je te fais mes retours vers 16 h." }

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
    /// With `google`: her answers by the local model, which needs no key
    /// (#758: her first run goes on to the accounts).
    keyless: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            output: "1920x1080",
            mode: SecretaryMode::Assistant,
            reduced_motion: false,
            google: false,
            refusing_api: false,
            keyless: false,
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
            let name = if setup.refusing_api || setup.keyless {
                "[secretary]\nname = \"Margot\"\n"
            } else {
                ""
            };
            let models = if setup.keyless {
                "[secretary.models]\nanswer = \"local/qwen3:8b\"\nbriefing = \"local/qwen3:8b\"\nextraction = \"local/qwen3:8b\"\ncompaction = \"local/qwen3:8b\"\n"
            } else {
                ""
            };
            std::fs::write(
                &config,
                format!(
                    "{name}{models}[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{api}\"\n\
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

    /// Where `item` is once the drawer's layout holds still: her thread
    /// may move as what she sends changes.
    fn place_settled(&mut self, item: &str) -> anyhow::Result<(f64, f64)> {
        let mut at = self.place(item)?;
        for _ in 0..20 {
            common::at_rest(&mut self.hp)?;
            wait(&mut self.hp, Duration::from_millis(250))?;
            let again = self.place(item)?;
            if again == at {
                return Ok(at);
            }
            at = again;
        }
        Ok(at)
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

/// AC5: `Super+S` toggles it; a click in a window closes it, as the bar's
/// menus do (#755), and the window takes the keyboard; `Super+S` opens it
/// again where it was, an answer written meanwhile in its thread; a click
/// in its field gives it the keyboard back; `Esc` closes it.
#[test]
fn super_s_toggles_it_a_click_elsewhere_closes_it_and_esc_closes_it() -> anyhow::Result<()> {
    const KEY_O: u32 = 24;
    const KEY_U: u32 = 22;
    const KEY_I: u32 = 23;
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    let window = common::open(&mut bed.hp, "drawer.test", (600, 400))?;
    bed.super_s();
    bed.opened()?;
    // A long answer asked, and something typed in the field.
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("réfléchis longuement".to_owned()));
    bed.shows(|s| s["items"].to_string().contains("réfléchis longuement"))?;
    bed.type_keys(&[KEY_O, KEY_U, KEY_I]);
    bed.shows(|s| s["field"] == 3)?;
    // A click in the window: the drawer closes, the window has the keyboard.
    let inside = (f64::from(window.x) + 50.0, f64::from(window.y) + 50.0);
    common::click(&mut bed.hp, inside)?;
    bed.closed()?;
    let moved = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() != Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(moved, "the window took the keyboard");
    // Her answer comes while it is closed.
    bed.advance(61)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    bed.super_s();
    bed.opened()?;
    let shown = bed.shows(|s| s["items"].to_string().contains("Voilà."))?;
    assert_eq!(shown["field"], 3, "what was typed, kept: {shown}");
    assert_eq!(shown["tab"], "talk");
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(focused);
    chord(&mut bed.hp, &[KEY_ESC]);
    bed.closed()?;
    bed.super_s();
    bed.opened()?;
    Ok(())
}

/// #755: with the keyboard gone to a window while the drawer is open (a
/// window that opens takes it), a click in the field gives it back: its
/// ring and its caret.
#[test]
fn a_click_in_the_field_gives_it_the_keyboard() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    bed.shows(|s| s["focus"] == "field")?;
    // The pointer over the drawer first (the toolkit then says nothing of
    // the keyboard coming back), then a window opens and takes it.
    let field = bed.place(" Field")?;
    common::point(&mut bed.hp, field)?;
    common::open(&mut bed.hp, "drawer.test", (600, 400))?;
    let moved = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() != Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(moved, "the window took the keyboard");
    bed.shows(|s| s["focus"] == "none")?;
    bed.click(field)?;
    bed.shows(|s| s["focus"] == "field")?;
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::SECRETARY_NAMESPACE)
    })?;
    assert!(focused, "the keyboard is the drawer's");
    bed.shot("field-clicked")?;
    Ok(())
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
        // A state in words, no model's name (#758).
        s["status"] == "Tout est à jour" && !s.to_string().contains("Claude Sonnet")
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
/// it ("Noté"), and the word is not asked about again.
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
        items.contains("Chapitô") && items.contains("Oui, c’est Chapitô | Non | Je t’explique"),
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
        items.contains("learned:Noté\u{a0}: «\u{a0}égaluantes\u{a0}», c’est Chapitô.")
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
            .contains(
                "card:Draft:Message Slack · à Léa Martin, en privé:Envoyer sur Slack/Modifier/Ne pas envoyer",
            )
    })?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.recorded().1.is_empty(), "nothing sent before the click");
    bed.shot("drawer-card")?;
    let accept = bed.place_starting("Button(")?;
    assert!(accept.ends_with("Accept)"), "{accept}");
    let at = bed.place(&accept)?;
    bed.click(at)?;
    // What left, where, when (#758).
    bed.shows(|s| s["items"].to_string().contains("→ Envoyé sur Slack à "))?;
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

/// Her first run (§15, #750, #758): her steps, one card per account
/// with a switch per kind and « Travail | Privé », « Autoriser l'agenda et
/// les mails » writing a grant for each kind switched on; then « Dis-moi
/// sur quoi tu travailles, et avec qui. ».
#[test]
fn her_first_run_asks_one_account_at_a_time_in_the_drawer() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup {
        google: true,
        keyless: true,
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
            .contains("card:Consent:Ton compte Google · 1 sur 2")
    })?;
    let items = shown["items"].to_string();
    assert!(items.contains("Je ne fais rien sans ton clic."), "{items}");
    assert!(
        items.contains("steps:Ce que je peux lire de tes comptes Google=Now"),
        "{items}"
    );
    assert_eq!(
        items.matches("card:Consent").count(),
        1,
        "one at a time: {items}"
    );
    assert!(
        items.contains("Autoriser l’agenda et les mails/Pas maintenant"),
        "{items}"
    );
    assert_eq!(shown["status"], "Je ne lis encore rien");
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.shot("first-run-consent")?;
    // « Privé », the mail switched off, then « Autoriser l'agenda »: the
    // calendar granted, private; not the mail.
    let item = bed
        .place_starting("Book(")?
        .trim_start_matches("Book(")
        .split(',')
        .next()
        .unwrap_or_default()
        .to_owned();
    let at = bed.place_settled(&format!("Book({item},true)"))?;
    bed.click(at)?;
    let at = bed.place_settled(&format!("Switch({item},mail)"))?;
    bed.click(at)?;
    bed.shows(|_| true)?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    let allow = button_of(&mut bed, "Accept")?;
    bed.click(allow)?;
    let shown = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("card:Consent:Ton compte Google · 2 sur 2")
    })?;
    assert!(
        shown["items"]
            .to_string()
            .contains("Je lis ton agenda · privé"),
        "{shown}"
    );
    let book = std::fs::read_to_string(
        bed.dir
            .path()
            .join("data/hocuspanda/secretary/consents.json"),
    )?;
    let book: hocus_secretary_link::consent::ConsentBook = serde_json::from_str(&book)?;
    let key = |kind| hocus_secretary_link::consent::SourceKey::new("alex@atelier-lune.fr", kind);
    let grant = book
        .grant(&key(hocus_secretary_link::consent::SourceKind::Calendar))
        .expect("the work calendar granted");
    assert_eq!(grant.notebook(), hocus_secretary_link::Notebook::Perso);
    assert!(
        book.grant(&key(hocus_secretary_link::consent::SourceKind::Mail))
            .is_none(),
        "switched off: not granted"
    );
    bed.shot("first-run-granted")?;
    // "Pas maintenant" on the gmail.com account.
    let not_now = button_of(&mut bed, "Reject")?;
    bed.click(not_now)?;
    let shown = bed.shows(|s| {
        s["items"]
            .to_string()
            .contains("her:Dis-moi sur quoi tu travailles, et avec qui.")
    })?;
    assert_eq!(
        shown["items"].to_string().matches("card:Consent").count(),
        2
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
    // What a key is and where to make one; no terminal (#758).
    assert!(
        shown["key_lines"].as_str().is_some_and(|l| l
            .contains("Je la garde dans le trousseau de ta session")
            && !l.contains("hocus-secretary key")),
        "{shown}"
    );
    bed.place("Console")?;
    // Her first run's steps: her key now, nothing else asked yet.
    let items = shown["items"].to_string();
    assert!(
        items.contains("steps:Ma clé Anthropic=Now") && !items.contains("card:Consent"),
        "{items}"
    );
    assert!(
        !items.contains("briefing:"),
        "no briefing in her first run: {items}"
    );
    bed.shot("key-band")?;
    bed.type_keys(&[
        KEY_S, KEY_K, KEY_MINUS, KEY_A, KEY_N, KEY_T, KEY_MINUS, KEY_X, KEY_X,
    ]);
    bed.shows(|s| s["key_field"] == serde_json::json!(true))?;
    bed.shot("key-band-typed")?;
    bed.type_keys(&[KEY_ENTER]);
    let shown = bed.shows(|s| {
        s["key_lines"]
            .as_str()
            .is_some_and(|l| l.contains("Anthropic refuse cette clé. Vérifie qu’elle est complète"))
    })?;
    assert_eq!(shown["key_field"], serde_json::json!(false), "it empties");
    bed.shot("key-band-refused")?;
    // The composer clicked: the keyboard types there.
    // " Field", not "KeyField".
    let field = bed.place(" Field")?;
    bed.click(field)?;
    bed.shows(|s| s["focus"] == "field")?;
    bed.shot("field-focused")?;
    // Its caret is still, as the shell's other fields': nothing draws at
    // rest with the keyboard in it.
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.drawer_commits();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.drawer_commits(), commits, "no blink at rest");
    let logs = format!(
        "{}{}",
        common::logged(&bed.log),
        common::logged(&bed.dir.path().join("secretary.log"))
    );
    assert!(!logs.contains("sk-ant-xx"), "the key in no log");
    Ok(())
}

const KEY_C: u32 = 46;
const BTN_RIGHT: u32 = 0x111;

/// A block of her text as the drawer logged it: what it shows, where its
/// first line starts and where its last ends, global.
#[derive(Debug, Clone)]
struct Block {
    source: String,
    start: (f64, f64),
    end: (f64, f64),
}

impl Bed {
    /// The blocks of text the drawer last logged, once there are `least`.
    fn texts(&mut self, least: usize) -> anyhow::Result<Vec<Block>> {
        let log = self.log.clone();
        let parse = |text: &str| -> Option<Vec<Block>> {
            let line = text
                .lines()
                .rev()
                .find(|l| l.contains("the drawer's texts"))?;
            let list = line.split("the drawer's texts").nth(1)?;
            list.split_whitespace()
                .map(|b| {
                    let (source, at) = b.split_once('@')?;
                    let (start, end) = at.split_once('>')?;
                    let point = |p: &str| -> Option<(f64, f64)> {
                        let (x, y) = p.split_once(',')?;
                        Some((x.parse().ok()?, y.parse().ok()?))
                    };
                    Some(Block {
                        source: source.to_owned(),
                        start: point(start)?,
                        end: point(end)?,
                    })
                })
                .collect()
        };
        let done = self.hp.dispatch_until(WAIT, |_| {
            parse(&common::logged(&log)).is_some_and(|b| b.len() >= least)
        })?;
        anyhow::ensure!(done, "never {least} blocks of text");
        let r = self.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))?;
        let (x, y) = (f64::from(r.x), f64::from(r.y));
        Ok(parse(&common::logged(&self.log))
            .unwrap_or_default()
            .into_iter()
            .map(|b| Block {
                start: (b.start.0 + x, b.start.1 + y),
                end: (b.end.0 + x, b.end.1 + y),
                ..b
            })
            .collect())
    }

    /// How much of the log there is now, to read what comes after.
    fn mark(&self) -> usize {
        common::logged(&self.log).len()
    }

    /// The first line holding `text` logged after `mark`.
    fn logged_after(&mut self, mark: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = |all: &str| {
            all.get(mark..)
                .and_then(|after| after.lines().find(|l| l.contains(text)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| find(&common::logged(&log)).is_some())?;
        anyhow::ensure!(done, "never logged: {text}");
        Ok(find(&common::logged(&self.log)).unwrap_or_default())
    }

    /// A press at `from` dragged to `to`, then let go.
    fn drag(&mut self, from: (f64, f64), to: (f64, f64)) -> anyhow::Result<()> {
        common::over(&mut self.hp, from, hocus_link::SECRETARY_NAMESPACE)?;
        self.hp.button(BTN_LEFT, true);
        wait(&mut self.hp, Duration::from_millis(60))?;
        for step in 1..=5 {
            let t = f64::from(step) / 5.0;
            self.hp
                .pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
            wait(&mut self.hp, Duration::from_millis(40))?;
        }
        self.hp.button(BTN_LEFT, false);
        wait(&mut self.hp, Duration::from_millis(60))?;
        Ok(())
    }

    /// `clicks` presses in a row at `at`.
    fn clicks(&mut self, at: (f64, f64), clicks: usize) -> anyhow::Result<()> {
        common::over(&mut self.hp, at, hocus_link::SECRETARY_NAMESPACE)?;
        for _ in 0..clicks {
            self.hp.button(BTN_LEFT, true);
            self.hp.button(BTN_LEFT, false);
        }
        wait(&mut self.hp, Duration::from_millis(60))?;
        Ok(())
    }

    /// What is selected, once the drawer says so after `mark`: how many
    /// characters.
    fn selected_after(&mut self, mark: usize) -> anyhow::Result<usize> {
        let line = self.logged_after(mark, "the drawer's selection")?;
        line.split("chars=")
            .nth(1)
            .and_then(|c| c.split_whitespace().next())
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| anyhow::anyhow!("no count in {line}"))
    }

    fn ctrl_c(&mut self) {
        chord(&mut self.hp, &[KEY_LEFTCTRL, KEY_C]);
    }

    /// The clipboard, once it holds `want`.
    fn clipboard_is(&mut self, want: &str) -> anyhow::Result<()> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.clipboard().as_deref() == Some(want))?;
        anyhow::ensure!(
            done,
            "the clipboard holds {:?}, not {want:?}",
            self.hp.clipboard()
        );
        Ok(())
    }

    /// The clipboard, once it holds something other than `not`.
    fn clipboard_changed(&mut self, not: Option<&str>) -> anyhow::Result<String> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.clipboard().is_some_and(|c| Some(c.as_str()) != not)
        })?;
        anyhow::ensure!(done, "the clipboard stays {not:?}");
        Ok(self.hp.clipboard().unwrap_or_default())
    }

    /// The « Copier » buttons the drawer last logged, by name.
    fn copy_buttons(&mut self) -> anyhow::Result<Vec<String>> {
        let log = self.log.clone();
        let find = |text: &str| -> Vec<String> {
            text.lines()
                .rev()
                .find(|l| l.contains("the drawer's items"))
                .map(|l| {
                    l.split_whitespace()
                        .filter(|w| w.starts_with("Copy("))
                        .filter_map(|w| w.split('@').next())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| !find(&common::logged(&log)).is_empty())?;
        anyhow::ensure!(done, "no « Copier » in the drawer");
        Ok(find(&common::logged(&self.log)))
    }
}

/// #757: her answer selected by a drag and copied with `Ctrl+C`; a
/// double click takes a word, a triple click the paragraph; a drag across
/// the user's bubble and hers takes both; the right click's « Copier »
/// copies what is selected. Nothing draws at rest with a selection, and
/// it goes as the drawer closes.
#[test]
fn her_text_is_selected_and_copied() -> anyhow::Result<()> {
    const HERS: &str = "Bien noté, je range ta journée.";
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    bed.hp
        .tell_shell(ToShell::SecretaryAsk("bonjour".to_owned()));
    bed.shows(|s| s["items"].to_string().contains("typing:"))?;
    bed.advance(30)?;
    // Her morning's briefing in too: nothing moves the thread after.
    bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains(&format!("her:{HERS}")) && items.contains("briefing:")
    })?;
    let blocks = bed.texts(2)?;
    let n = blocks.len();
    let (user, her) = (blocks[n - 2].clone(), blocks[n - 1].clone());
    assert!(
        user.start.0 > her.start.0,
        "the user's on the right: {blocks:?}"
    );
    assert!(
        user.source.starts_with("Item(") && her.source.starts_with("Item("),
        "{blocks:?}"
    );
    // Dragged from before its first letter to past its last: all of it.
    let mark = bed.mark();
    bed.drag(
        (her.start.0 + 1.0, her.start.1),
        (her.end.0 + 30.0, her.end.1),
    )?;
    assert_eq!(bed.selected_after(mark)?, HERS.chars().count());
    bed.shot("drawer-selection")?;
    // Nothing draws at rest with a selection.
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    let commits = bed.hp.drawer_commits();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert_eq!(bed.hp.drawer_commits(), commits, "nothing draws at rest");
    let mark = bed.mark();
    bed.ctrl_c();
    bed.logged_after(mark, "the drawer copies")?;
    bed.clipboard_is(HERS)?;
    // A double click on her first line: a word.
    let on_word = (her.start.0 + 40.0, her.start.1);
    bed.clicks(on_word, 2)?;
    bed.ctrl_c();
    let word = bed.clipboard_changed(Some(HERS))?;
    assert!(
        !word.is_empty() && !word.contains(' ') && HERS.contains(&word),
        "a word: {word:?}"
    );
    // A triple click, once the double's time is over: the paragraph.
    wait(&mut bed.hp, Duration::from_millis(700))?;
    bed.clicks(on_word, 3)?;
    bed.ctrl_c();
    bed.clipboard_is(HERS)?;
    // Across the user's bubble and hers: both, a line between.
    bed.drag(
        (user.start.0 + 1.0, user.start.1),
        (her.end.0 + 30.0, her.end.1),
    )?;
    bed.ctrl_c();
    bed.clipboard_is(&format!("bonjour\n{HERS}"))?;
    // A word selected, then the right click's « Copier ».
    wait(&mut bed.hp, Duration::from_millis(700))?;
    bed.clicks(on_word, 2)?;
    let mark = bed.mark();
    bed.hp.button(BTN_RIGHT, true);
    bed.hp.button(BTN_RIGHT, false);
    let menu = bed.logged_after(mark, "the drawer's menu CopyMenu@")?;
    let at = menu
        .split("CopyMenu@")
        .nth(1)
        .and_then(|a| a.split_whitespace().next())
        .and_then(|a| a.split_once(','))
        .and_then(|(x, y)| Some((x.parse::<f64>().ok()?, y.parse::<f64>().ok()?)))
        .ok_or_else(|| anyhow::anyhow!("no menu's place in {menu}"))?;
    let r = bed.drawer().ok_or_else(|| anyhow::anyhow!("no drawer"))?;
    bed.shot("drawer-menu")?;
    bed.click((f64::from(r.x) + at.0, f64::from(r.y) + at.1))?;
    bed.clipboard_is(&word)?;
    // Closed and opened again: nothing is selected any more.
    bed.type_keys(&[KEY_ESC]);
    bed.closed()?;
    bed.super_s();
    bed.opened()?;
    let mark = bed.mark();
    bed.ctrl_c();
    wait(&mut bed.hp, Duration::from_secs(1))?;
    assert!(
        !common::logged(&bed.log)[mark..].contains("the drawer copies"),
        "nothing selected after it closed"
    );
    assert_eq!(bed.hp.clipboard().as_deref(), Some(word.as_str()));
    Ok(())
}

/// #757: her answers and draft cards have « Copier » on hover: it copies
/// the answer, or the draft, and « Copié » says so; a masked secret copies
/// as its mask.
#[test]
fn her_answers_and_drafts_copy_from_their_button() -> anyhow::Result<()> {
    const HERS: &str = "Je te propose ce message pour Léa.";
    const DRAFT: &str = "Je relis ton article cet après-midi, je te fais mes retours vers 16 h.";
    const MASKED: &str = "Le code du wifi : [secret masqué — voir le mail de Léa].";
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    // Her morning's briefing first, as her clock moves: nothing moves the
    // thread after.
    bed.advance(30)?;
    bed.shows(|s| s["items"].to_string().contains("briefing:"))?;
    bed.hp.tell_shell(ToShell::SecretaryAsk(
        "prépare un message pour Léa sur son article".to_owned(),
    ));
    bed.shows(|s| {
        let items = s["items"].to_string();
        items.contains("card:Draft:Message Slack · à Léa Martin")
            && items.contains(&format!("her:{HERS}"))
    })?;
    let buttons = bed.copy_buttons()?;
    let mut copied = Vec::new();
    for (n, button) in buttons.iter().enumerate() {
        let at = bed.place(button)?;
        common::point(&mut bed.hp, at)?;
        if n == 0 {
            bed.shot("drawer-copy-button")?;
        }
        let mark = bed.mark();
        let before = bed.hp.clipboard();
        bed.click(at)?;
        bed.logged_after(mark, "the drawer copies")?;
        copied.push(bed.clipboard_changed(before.as_deref())?);
        if n == 0 {
            bed.shot("drawer-copied")?;
        }
    }
    assert!(
        copied.iter().any(|c| c == HERS) && copied.iter().any(|c| c == DRAFT),
        "her answer and the draft: {copied:?}"
    );
    // A secret she masked copies as she shows it.
    bed.hp.tell_shell(ToShell::SecretaryAsk(
        "le mot de passe du wifi ?".to_owned(),
    ));
    bed.shows(|s| s["items"].to_string().contains("user:le mot de passe"))?;
    bed.advance(30)?;
    bed.shows(|s| s["items"].to_string().contains(&format!("her:{MASKED}")))?;
    let newest = bed
        .copy_buttons()?
        .into_iter()
        .max_by_key(|b| {
            b.trim_start_matches("Copy(")
                .trim_end_matches(')')
                .parse::<u64>()
                .unwrap_or(0)
        })
        .ok_or_else(|| anyhow::anyhow!("no « Copier »"))?;
    let at = bed.place(&newest)?;
    bed.click(at)?;
    bed.clipboard_is(MASKED)?;
    Ok(())
}

/// #757: the queue's lines are selected and copied as her answers are: a
/// triple click on what she does takes that line.
#[test]
fn the_queue_s_lines_are_copied_too() -> anyhow::Result<()> {
    let Some(mut bed) = start(Setup::default())? else {
        return Ok(());
    };
    bed.super_s();
    bed.opened()?;
    let status = bed.place("Status")?;
    bed.click(status)?;
    let shown = bed.shows(|s| s["items"].to_string().contains("state:"))?;
    let state = shown["items"]
        .as_array()
        .and_then(|items| {
            items
                .iter()
                .filter_map(|i| i.as_str())
                .find_map(|i| i.strip_prefix("state:"))
        })
        .and_then(|s| s.split(" | ").next())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("no state in {shown}"))?;
    // The sentence saying what the triage is (#758), then what she does:
    // its first line, 24 px in.
    let blocks = bed.texts(4)?;
    let x = f64::from(bed.drawer().map_or(0, |r| r.x)) + 24.0;
    let line = blocks
        .iter()
        .find(|b| (b.start.0 - x).abs() < 1.5)
        .ok_or_else(|| anyhow::anyhow!("no state line in {blocks:?}"))?;
    assert!(line.source.starts_with("Queue("), "{blocks:?}");
    bed.clicks(((line.start.0 + line.end.0) / 2.0, line.start.1), 3)?;
    bed.ctrl_c();
    bed.clipboard_is(&state)?;
    Ok(())
}
