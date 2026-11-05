//! The Settings' Son page (6.5, issue #405): the real `hocus-settings`
//! window, a client of a headless compositor, against a PipeWire of the
//! test's own (`levels/fakes.rs`): its sinks and its source null nodes,
//! WirePlumber's part (the default chosen made the default) a
//! `pw-metadata` of the test's, and the links a session manager would
//! make made by the test itself; never WirePlumber, never the session's
//! PipeWire. Its configuration, state, home and runtime directories are
//! the test's own; no session bus; it draws with tiny-skia.
//!
//! One test per measurable point of the approved spec: the devices
//! listed, the one in use ticked; choosing the output sets PipeWire's
//! default and "Annuler" takes it back; the output's volume and mute
//! reach PipeWire; an application playing joins the list, its volume and
//! its output set from its row; with none, the popover's words; the meter
//! records only while the input group shows, and the bar's in-use sign
//! then lists the Settings; the sound popover's "Réglages du son…" opens
//! the Settings on Son, or brings them forward there; at rest, the window
//! commits nothing.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, WAIT, wait};
use fakes::{FakeSink, Fakes};
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};
use serde_json::Value;

const APP_ID: &str = "hocus-settings";
const SPEAKERS: &str = "hocus-test-sink";
const HEADSET: &str = "hocus-test-other";
const MIC: &str = "hocus-test-mic";
const METER: &str = "hocus-settings-meter";

const SINKS: [FakeSink; 2] = [
    FakeSink::new(SPEAKERS, "Haut-parleurs"),
    FakeSink::new(HEADSET, "Casque"),
];
const SOURCES: [FakeSink; 1] = [FakeSink::new(MIC, "Micro interne")];

struct Bed {
    hp: Headless,
    fakes: Fakes,
    log: PathBuf,
    shell_log: PathBuf,
    script: PathBuf,
    _dir: tempfile::TempDir,
}

/// Whether what the tests need is there: the Settings built, PipeWire
/// and its tools (and the shell, if `shell`).
fn ready(shell: bool) -> bool {
    if common::settings_program().is_none() || (shell && common::shell_program().is_none()) {
        eprintln!("the Settings or the shell are not built: build the workspace to run this test");
        return false;
    }
    let tools = ["pw-cli", "pw-link", "pw-record", "pw-dump", "wpctl"]
        .iter()
        .all(|p| {
            Command::new("sh")
                .args(["-c", &format!("command -v {p}")])
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        });
    if !fakes::available() || !tools {
        eprintln!("pipewire or its tools missing: skipped");
        return false;
    }
    true
}

impl Bed {
    /// A compositor (and its shell, if `shell`), the PipeWire of the test,
    /// and the Settings' script ready to launch.
    fn start(shell: bool) -> anyhow::Result<Self> {
        Self::start_on(shell, "1920x1080")
    }

    /// The same on a screen `output` (`WxH[@SCALE]`).
    fn start_on(shell: bool, output: &str) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config/hocuspanda", "cache", "state", "data", "bin"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let fakes = Fakes::start_with_sources(&SINKS, &SOURCES)?;
        let config_file = root.join("config/hocuspanda/config.toml");
        std::fs::write(&config_file, "# Mine.\n")?;
        let program = common::settings_program().ok_or_else(|| anyhow::anyhow!("no settings"))?;
        let log = root.join("settings.log");
        let script = root.join("bin/hocus-settings");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 HOCUS_POWER_SUPPLY_DIR=/nonexistent ICED_BACKEND=tiny-skia \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' XDG_DATA_DIRS='{}' \
                 PIPEWIRE_REMOTE='{}' PATH='{}:/usr/bin:/bin' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocus_settings=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                fakes.pw_socket().display(),
                at("bin"),
                program.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let shell_log = root.join("shell.log");
        let shell_script = match common::shell_program().filter(|_| shell) {
            Some(shell) => Some(fakes.shell_with(
                &shell,
                "fr_FR.UTF-8",
                &[
                    ("HOME", at("home")),
                    ("XDG_CONFIG_HOME", at("config")),
                    ("XDG_CACHE_HOME", at("cache")),
                    ("XDG_STATE_HOME", at("state")),
                    ("XDG_DATA_HOME", at("data")),
                    ("XDG_DATA_DIRS", at("data")),
                    ("ICED_BACKEND", "tiny-skia".to_owned()),
                    ("TZ", "UTC".to_owned()),
                    // The sound popover's link starts the Settings by
                    // this script, in their directories.
                    ("PATH", format!("{}:/usr/bin:/bin", at("bin"))),
                    ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
                ],
                Some(&shell_log),
            )?),
            None => None,
        };
        let mut config = Config {
            plain_background: true,
            french: true,
            pipewire_remote: Some(fakes.pw_socket()),
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        config.panel.form = FormChoice::Full;
        config.file = Some(config_file);
        let address = fakes.address.clone();
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![output.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            shell_script.clone(),
            shell_script.is_some().then_some(address),
        )?;
        if shell_script.is_some() {
            let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
            anyhow::ensure!(up, "no bar");
        }
        Ok(Self {
            hp,
            fakes,
            log,
            shell_log,
            script,
            _dir: dir,
        })
    }

    /// The Settings launched on Son, as the desktop launches them, their
    /// window open with the keyboard and the page drawn.
    fn open(&mut self) -> anyhow::Result<Value> {
        self.hp.launch(hocus_link::Launch {
            id: APP_ID.to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![self.script.display().to_string(), "sound".to_owned()],
        });
        let open = self.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
        })?;
        anyhow::ensure!(open, "the window never opened: {}", self.plain());
        let shown = self.shows(|s| {
            s["page"] == "sound"
                && s["sound"]["pipewire"] == true
                && s["sound"]["outputs"]
                    .as_array()
                    .is_some_and(|o| o.len() == 2)
                && s["sound"]["inputs"]
                    .as_array()
                    .is_some_and(|i| i.len() == 1)
        })?;
        self.places_with(&format!("out:{SPEAKERS}"))?;
        Ok(shown)
    }

    /// What the window logged besides what it shows and where.
    fn said(&self) -> String {
        self.plain()
            .lines()
            .filter(|l| !l.contains(" shows {") && !l.contains(" places {"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn plain(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned()
    }

    /// The last JSON line of `log` said after `what`.
    fn last(log: &Path, what: &str) -> Option<Value> {
        let text = String::from_utf8_lossy(&std::fs::read(log).ok()?).into_owned();
        let line = text.lines().rev().find(|l| l.contains(what))?;
        let at = line.find(what)? + what.len() - 1;
        serde_json::from_str(&line[at..]).ok()
    }

    /// What the window shows, once `done` holds of it.
    fn shows(&mut self, done: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            Self::last(&log, " shows {").is_some_and(|s| done(&s))
        })?;
        let last = Self::last(&log, " shows {");
        anyhow::ensure!(
            seen,
            "the window never showed it; it shows {}\nit said:\n{}",
            last.map_or_else(|| "nothing".to_owned(), |s| s.to_string()),
            self.said()
        );
        last.ok_or_else(|| anyhow::anyhow!("nothing shown"))
    }

    /// The window's places, once they hold `id`.
    fn places_with(&mut self, id: &str) -> anyhow::Result<Value> {
        let log = self.log.clone();
        let seen = self.hp.dispatch_until(WAIT, |_| {
            Self::last(&log, " places {").is_some_and(|p| p.get(id).is_some())
        })?;
        anyhow::ensure!(seen, "{id} is nowhere: {:?}", Self::last(&log, " places {"));
        Self::last(&log, " places {").ok_or_else(|| anyhow::anyhow!("no places"))
    }

    fn window(&self) -> anyhow::Result<Rect> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(APP_ID))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no settings window"))
    }

    /// The point of `id` at `fraction` of its width, in its middle's
    /// height, on the screen.
    fn at(&mut self, id: &str, fraction: f64) -> anyhow::Result<(f64, f64)> {
        let places = self.places_with(id)?;
        let place = &places[id];
        let n = |i: usize| place[i].as_f64().unwrap_or_default();
        let window = self.window()?;
        Ok((
            f64::from(window.x) + n(0) + n(2) * fraction,
            f64::from(window.y) + n(1) + n(3) / 2.0,
        ))
    }

    fn click_at(&mut self, id: &str, fraction: f64) -> anyhow::Result<()> {
        let at = self.at(id, fraction)?;
        self.hp.pointer_move(at.0 - 1.0, at.1);
        self.hp.pointer_move(at.0, at.1);
        wait(&mut self.hp, Duration::from_millis(30))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn click(&mut self, id: &str) -> anyhow::Result<()> {
        self.click_at(id, 0.5)
    }

    /// The page scrolled with the wheel until `id` is in the window.
    fn scroll_to_show(&mut self, id: &str) -> anyhow::Result<()> {
        let window = self.window()?;
        for _ in 0..60 {
            let places = self.places_with(id)?;
            let n = |i: usize| places[id][i].as_f64().unwrap_or_default();
            if n(1) >= 60.0 && n(1) + n(3) <= f64::from(window.height) - 10.0 {
                return Ok(());
            }
            let middle = (
                f64::from(window.x) + f64::from(window.width) * 0.6,
                f64::from(window.y) + f64::from(window.height) / 2.0,
            );
            self.hp.pointer_move(middle.0 - 1.0, middle.1);
            self.hp.pointer_move(middle.0, middle.1);
            self.hp.wheel(if n(1) < 60.0 { -1 } else { 1 });
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("{id} never came into view")
    }

    /// Waits until PipeWire says `done`, the compositor running.
    fn pipewire(&mut self, what: &str, done: impl Fn(&Fakes) -> bool) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < WAIT {
            if done(&self.fakes) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!(
            "PipeWire never said {what}; the window said:\n{}",
            self.plain()
        )
    }

    /// A program of PipeWire's against the test's PipeWire.
    fn pw(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self._dir.path().join("home"))
            .env("PIPEWIRE_REMOTE", self.fakes.pw_socket())
            .stdin(Stdio::null())
            .stderr(Stdio::null());
        command
    }

    /// Whether a node named `name` is in PipeWire's graph.
    fn node(&self, name: &str) -> bool {
        self.fakes.sink(name).is_ok()
    }

    /// The meter's stream linked from the microphone, as a session manager
    /// would: its port made (its `PortConfig`), then linked.
    fn link_meter(&mut self) -> anyhow::Result<()> {
        let id = self.fakes.sink(METER)?;
        let status = self
            .pw("pw-cli")
            .args([
                "set-param",
                &id.to_string(),
                "PortConfig",
                "{ direction: Input, mode: dsp, format: { mediaType: audio, mediaSubtype: raw, \
                 format: F32P, rate: 16000, channels: 1, position: [ MONO ] } }",
            ])
            .stdout(Stdio::null())
            .status()?;
        anyhow::ensure!(status.success(), "pw-cli set-param: {status}");
        let port = format!("{METER}:input_MONO");
        let start = Instant::now();
        loop {
            let inputs = self.pw("pw-link").arg("-i").output()?;
            if String::from_utf8_lossy(&inputs.stdout)
                .lines()
                .any(|l| l == port)
            {
                break;
            }
            anyhow::ensure!(start.elapsed() < WAIT, "the meter's port never came");
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        let status = self
            .pw("pw-link")
            .args([&format!("{MIC}:monitor_FL"), port.as_str()])
            .stdout(Stdio::null())
            .status()?;
        anyhow::ensure!(status.success(), "pw-link: {status}");
        Ok(())
    }

    /// The bar's sound popover, opened from its row: where it is.
    fn sound_popover(&mut self) -> anyhow::Result<Rect> {
        let bed = self;
        // The sound's row of the bar opens its popover, once the levels
        // have their size and the connections' surface is above them (as
        // `popover.rs` waits).
        let placed = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
            hp.levels_surfaces()
                .first()
                .is_some_and(|(_, r)| r.height >= 175)
        })?;
        anyhow::ensure!(placed, "no levels: {:?}", bed.hp.levels_surfaces());
        let levels = bed.hp.levels_surfaces()[0].1;
        let top = levels.y + 57;
        let clear = bed.hp.dispatch_until(WAIT, |hp| {
            hp.connections_surfaces()
                .iter()
                .all(|(_, r)| r.y + r.height <= top)
        })?;
        anyhow::ensure!(clear, "the connections over the sound's row");
        let row = (
            f64::from(levels.x + levels.width * 3 / 5),
            f64::from(levels.y + 57),
        );
        common::click_on(&mut bed.hp, row, hocus_link::LEVELS_NAMESPACE)?;
        let open = bed
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(
            open,
            "no popover from {row:?} on {levels:?}: {:?}",
            bed.hp.popover_surfaces()
        );
        common::at_rest(&mut bed.hp)?;
        let popover = common::drawn_popovers(&bed.hp)[0];
        Ok(popover)
    }

    /// The shell's log.
    fn shell_said(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.shell_log).unwrap_or_default()).into_owned()
    }

    /// Waits until the shell logs `what` on a line `pass` takes.
    fn shell_logged(&mut self, what: &str, pass: impl Fn(&str) -> bool) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            let log = self.shell_said();
            if let Some(line) = log
                .lines()
                .filter(|l| l.contains(what))
                .rev()
                .find(|l| pass(l))
            {
                return Ok(line.to_owned());
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.shell_said())
    }
}

/// The device `name` among those the page lists in `list`.
fn device<'a>(shown: &'a Value, list: &str, name: &str) -> &'a Value {
    shown["sound"][list]
        .as_array()
        .and_then(|l| l.iter().find(|d| d["name"] == name))
        .unwrap_or(&Value::Null)
}

#[test]
fn the_sound_page_lists_the_sinks_and_sources_with_the_default_checked() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    let shown = bed.open()?;
    let outputs: Vec<(&str, bool)> = shown["sound"]["outputs"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| {
            (
                d["description"].as_str().unwrap_or(""),
                d["default"] == true,
            )
        })
        .collect();
    assert_eq!(outputs, [("Haut-parleurs", true), ("Casque", false)]);
    // Each its own volume on its row: the default at 62 %.
    assert_eq!(device(&shown, "outputs", SPEAKERS)["value"], "62 %");
    assert_eq!(device(&shown, "outputs", HEADSET)["value"], "100 %");
    let inputs = &shown["sound"]["inputs"];
    assert_eq!(inputs[0]["description"], "Micro interne");
    assert_eq!(inputs[0]["default"], true);
    // The chosen output's card: its volume, centred, stereo.
    assert_eq!(shown["sound"]["output"]["volume"], 62);
    assert_eq!(shown["sound"]["output"]["balance"], 0);
    assert_eq!(shown["sound"]["output"]["mono"], false);
    // The rows' places: the one in use is the output's row.
    let places = bed.places_with("row:sound.output")?;
    assert_eq!(
        places["row:sound.output"][1],
        places[format!("out:{SPEAKERS}").as_str()][1],
        "{places}"
    );
    // The home's card says it too.
    bed.click("nav:home")?;
    let home = bed.shows(|s| s["page"] == "home")?;
    let card = home["cards"]
        .as_array()
        .and_then(|c| c.iter().find(|c| c[0] == "Son"))
        .cloned()
        .unwrap_or(Value::Null);
    assert_eq!(card[1], "Haut-parleurs · 62 %");
    Ok(())
}

#[test]
fn choosing_an_output_sets_the_default_and_annuler_takes_it_back() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    bed.open()?;
    bed.click(&format!("out:{HEADSET}"))?;
    bed.pipewire("the headset the default", |f| {
        f.default_sink_name().is_ok_and(|n| n == HEADSET)
    })?;
    let shown = bed.shows(|s| {
        s["toast"] == "Sortie : Casque" && device(s, "outputs", HEADSET)["default"] == true
    })?;
    assert_eq!(shown["toast_action"], "undo");
    assert_eq!(shown["undo"], 1);
    // Its own volume kept: each output keeps its own.
    assert_eq!(shown["sound"]["output"]["volume"], 100);
    bed.click("toast-action")?;
    bed.pipewire("the speakers the default again", |f| {
        f.default_sink_name().is_ok_and(|n| n == SPEAKERS)
    })?;
    let shown = bed.shows(|s| {
        s["toast"] == "Annulé : la sortie revient à Haut-parleurs."
            && device(s, "outputs", SPEAKERS)["default"] == true
    })?;
    assert_eq!(shown["lit"], "sound.output");
    assert_eq!(shown["undo"], 0);
    // Ctrl+Z takes it again: Ctrl+Shift+Z redoes.
    common::chord(
        &mut bed.hp,
        &[common::KEY_LEFTCTRL, common::KEY_LEFTSHIFT, 44],
    );
    bed.pipewire("the headset the default again", |f| {
        f.default_sink_name().is_ok_and(|n| n == HEADSET)
    })?;
    // Nothing of it in the file.
    let file = bed._dir.path().join("config/hocuspanda/config.toml");
    assert_eq!(std::fs::read_to_string(file)?, "# Mine.\n");
    Ok(())
}

#[test]
fn a_volume_and_a_mute_moved_on_the_page_reach_pipewire() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    bed.open()?;
    // A click a quarter along the slider: about 25 % (the knob's
    // half-width taken in).
    bed.click_at("slider:output", 0.25)?;
    bed.pipewire("the speakers near 25 %", |f| {
        f.volume(SPEAKERS)
            .is_ok_and(|(v, m)| (v - 0.25).abs() < 0.06 && !m)
    })?;
    let shown = bed.shows(|s| {
        s["sound"]["output"]["volume"]
            .as_i64()
            .is_some_and(|v| (19..=31).contains(&v))
    })?;
    // No toast for a volume: it is heard at once.
    assert_eq!(shown["toast"], Value::Null);
    // The mute button mutes it; its row says it.
    bed.click("mute:output")?;
    bed.pipewire("the speakers muted", |f| {
        f.volume(SPEAKERS).is_ok_and(|(_, m)| m)
    })?;
    bed.shows(|s| device(s, "outputs", SPEAKERS)["value"] == "muet")?;
    // Moving it unmutes.
    bed.click_at("slider:output", 0.75)?;
    bed.pipewire("the speakers unmuted near 75 %", |f| {
        f.volume(SPEAKERS)
            .is_ok_and(|(v, m)| (v - 0.75).abs() < 0.06 && !m)
    })?;
    // The microphone's, the same.
    bed.click("mute:input")?;
    bed.pipewire("the microphone muted", |f| {
        f.volume(MIC).is_ok_and(|(_, m)| m)
    })?;
    bed.shows(|s| device(s, "inputs", MIC)["value"] == "coupé")?;
    Ok(())
}

#[test]
fn an_application_playing_joins_the_list_and_its_device_can_change() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    bed.open()?;
    bed.fakes.play("hocus-test-music", "Musique", None)?;
    let shown = bed.shows(|s| {
        s["sound"]["apps"]
            .as_array()
            .is_some_and(|a| a.len() == 1 && a[0]["name"] == "Musique")
    })?;
    assert_eq!(shown["sound"]["apps"][0]["follows"], true);
    // Its volume from its row, the page scrolled to it.
    bed.scroll_to_show("app:Musique")?;
    bed.click_at("slider:app:Musique", 0.5)?;
    bed.pipewire("the stream near 50 %", |f| {
        f.volume("hocus-test-music")
            .is_ok_and(|(v, _)| (v - 0.5).abs() < 0.08)
    })?;
    // Its output: the headset, from its menu.
    bed.click("menu:app:Musique")?;
    bed.shows(|s| {
        s["sound"]["menu"]
            .as_str()
            .is_some_and(|m| m.contains("Musique"))
    })?;
    bed.places_with("option:2")?;
    bed.click("option:2")?;
    let shown = bed.shows(|s| s["toast"] == "Musique joue sur Casque")?;
    assert_eq!(shown["toast_action"], "undo");
    // PipeWire's metadata says it, as the page reads it back.
    bed.shows(|s| s["sound"]["apps"][0]["target"] == HEADSET)?;
    // Annuler: it follows the output again.
    bed.click("toast-action")?;
    bed.shows(|s| s["sound"]["apps"][0]["follows"] == true)?;
    // It stops: it leaves the list.
    bed.fakes.stop("hocus-test-music");
    bed.shows(|s| s["sound"]["apps"].as_array().is_some_and(Vec::is_empty))?;
    Ok(())
}

#[test]
fn no_application_says_aucune_application_ne_joue_de_son() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    let shown = bed.open()?;
    assert_eq!(shown["sound"]["apps"], serde_json::json!([]));
    bed.places_with("apps:none")?;
    Ok(())
}

#[test]
fn the_meter_listens_only_while_the_input_shows() -> anyhow::Result<()> {
    if !ready(true) {
        return Ok(());
    }
    let mut bed = Bed::start(true)?;
    // Nothing records before the page shows.
    assert!(!bed.node(METER));
    bed.open()?;
    bed.shows(|s| s["sound"]["listening"] == true)?;
    bed.pipewire("the meter's stream", |f| f.sink(METER).is_ok())?;
    // Linked from the microphone, as a session manager links it: the bar
    // says that the Settings listen.
    bed.link_meter()?;
    bed.shell_logged("the in-use sign shows", |l| l.contains("Microphone"))?;
    let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
    let (x, y) = (sign.x + sign.width / 2, sign.y + sign.height / 2);
    common::over(
        &mut bed.hp,
        (f64::from(x), f64::from(y)),
        hocus_link::IN_USE_NAMESPACE,
    )?;
    let said = bed.shell_logged("the in-use popover shows it", |l| l.contains("Réglages"))?;
    assert!(said.contains("\"Réglages\""), "{said}");
    bed.hp.pointer_move(960.0, 540.0);
    // Another page: the meter stops, its stream goes.
    bed.click("nav:power")?;
    bed.shows(|s| s["page"] == "power" && s["sound"]["listening"] == false)?;
    bed.pipewire("the meter's stream gone", |f| f.sink(METER).is_err())?;
    bed.shell_logged("the in-use sign goes", |_| true)?;
    // Back on Son: it listens again; muted, it stops.
    bed.click("nav:sound")?;
    bed.shows(|s| s["page"] == "sound" && s["sound"]["listening"] == true)?;
    bed.places_with("mute:input")?;
    bed.click("mute:input")?;
    bed.shows(|s| s["sound"]["listening"] == false && s["sound"]["input"]["muted"] == true)?;
    bed.pipewire("the meter's stream gone again", |f| f.sink(METER).is_err())?;
    Ok(())
}

#[test]
fn the_popovers_link_opens_settings_on_son() -> anyhow::Result<()> {
    if !ready(true) {
        return Ok(());
    }
    let mut bed = Bed::start(true)?;
    let link = |bed: &mut Bed| -> anyhow::Result<()> {
        let popover = bed.sound_popover()?;
        // Its last row: 36 px, 14 px over its bottom.
        let at = (
            f64::from(popover.x + popover.width / 2),
            f64::from(popover.y + popover.height - 14 - 18),
        );
        common::click_on(&mut bed.hp, at, hocus_link::POPOVER_NAMESPACE)?;
        Ok(())
    };
    link(&mut bed)?;
    bed.shell_logged("the Settings open from a popover", |l| l.contains("sound"))?;
    let open = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some(APP_ID) && w.keyboard)
    })?;
    anyhow::ensure!(open, "the Settings never opened: {}", bed.plain());
    // On Son, the output in use lit; the popover closed.
    bed.shows(|s| s["page"] == "sound" && s["lit"] == "sound.output")?;
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the popover stayed open");
    // On another page, the link again: the same window, back on Son.
    bed.places_with("nav:power")?;
    bed.click("nav:power")?;
    bed.shows(|s| s["page"] == "power")?;
    bed.hp.pointer_move(5.0, 5.0);
    link(&mut bed)?;
    bed.shows(|s| s["page"] == "sound")?;
    assert!(
        bed.plain()
            .contains("launched again: the window comes forward"),
        "{}",
        bed.said()
    );
    let settings = bed
        .hp
        .windows()
        .iter()
        .filter(|w| w.app_id.as_deref() == Some(APP_ID))
        .count();
    assert_eq!(settings, 1);
    Ok(())
}

#[test]
fn at_rest_the_page_commits_nothing() -> anyhow::Result<()> {
    if !ready(false) {
        return Ok(());
    }
    let mut bed = Bed::start(false)?;
    bed.open()?;
    bed.shows(|s| s["sound"]["listening"] == true)?;
    bed.hp.pointer_move(5.0, 5.0);
    // Settled: a second without a commit.
    let mut last = bed.hp.window_commits();
    let mut still = 0;
    for _ in 0..200 {
        wait(&mut bed.hp, Duration::from_millis(100))?;
        let now = bed.hp.window_commits();
        still = if now == last { still + 1 } else { 0 };
        last = now;
        if still >= 10 {
            break;
        }
    }
    anyhow::ensure!(still >= 10, "the window never came to rest");
    wait(&mut bed.hp, Duration::from_secs(3))?;
    assert_eq!(bed.hp.window_commits(), last, "the window drew at rest");
    Ok(())
}

/// The pull request's pictures, at ×2: `HOCUS_SHOTS=DIR`.
#[test]
#[ignore = "pictures for the pull request: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context as _;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    if !ready(true) {
        return Ok(());
    }
    std::fs::create_dir_all(&out)?;
    let mut bed = Bed::start_on(true, "2880x1800@2")?;
    let shoot = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        let mut last = bed.hp.window_commits();
        let mut still = 0;
        for _ in 0..30 {
            if still >= 5 {
                break;
            }
            wait(&mut bed.hp, Duration::from_millis(100))?;
            let now = bed.hp.window_commits();
            still = if now == last { still + 1 } else { 0 };
            last = now;
        }
        let window = bed.window()?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let pad = 24;
        let x = (window.x * 2 - pad).max(0) as u32;
        let y = (window.y * 2 - pad).max(0) as u32;
        let cw = ((window.width * 2 + 2 * pad) as u32).min(w as u32 - x);
        let ch = ((window.height * 2 + 2 * pad) as u32).min(h as u32 - y);
        let image = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let path = out.join(name);
        image.save(&path)?;
        eprintln!("{}", path.display());
        Ok(())
    };
    bed.fakes
        .play("hocus-test-music", "Lecteur de musique", None)?;
    bed.fakes.play("hocus-test-web", "Navigateur web", None)?;
    bed.open()?;
    bed.shows(|s| s["sound"]["apps"].as_array().is_some_and(|a| a.len() == 2))?;
    bed.link_meter()?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "sound.png")?;
    // The applications, the page scrolled, one's menu open.
    bed.scroll_to_show("app:Navigateur web")?;
    bed.click("menu:app:Navigateur web")?;
    bed.shows(|s| {
        s["sound"]["menu"]
            .as_str()
            .is_some_and(|m| m.contains("Navigateur"))
    })?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "sound-applications.png")?;
    bed.hp.key(common::KEY_ESC, true);
    bed.hp.key(common::KEY_ESC, false);
    // Another output chosen: its toast.
    bed.scroll_to_show(&format!("out:{HEADSET}"))?;
    bed.click(&format!("out:{HEADSET}"))?;
    bed.shows(|s| s["toast"] == "Sortie : Casque")?;
    bed.hp.pointer_move(5.0, 5.0);
    shoot(&mut bed, "sound-output-chosen.png")?;
    // The bar says that the Settings listen.
    bed.shell_logged("the in-use sign shows", |l| l.contains("Microphone"))?;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    image.save(out.join("screen.png"))?;
    // The sound popover's link (×2, around the popover).
    bed.hp.pointer_move(5.0, 5.0);
    let popover = bed.sound_popover()?;
    let link = (
        f64::from(popover.x + popover.width / 2),
        f64::from(popover.y + popover.height - 14 - 18),
    );
    common::over(&mut bed.hp, link, hocus_link::POPOVER_NAMESPACE)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let (w, h, px) = bed.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let (x, y) = (
        (popover.x * 2 - 24).max(0) as u32,
        (popover.y * 2 - 24).max(0) as u32,
    );
    let image = image::imageops::crop_imm(
        &image,
        x,
        y,
        ((popover.width * 2 + 48) as u32).min(w as u32 - x),
        ((popover.height * 2 + 48) as u32).min(h as u32 - y),
    )
    .to_image();
    image.save(out.join("popover-link.png"))?;
    Ok(())
}
