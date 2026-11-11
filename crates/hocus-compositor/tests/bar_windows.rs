//! The windows in the bar (issue #604, as the maintainer changed variant
//! B): the windows of the screen's current desktop over its desktops'
//! tiles, at most 3, the focused one first and lit, each with its ✕; "+ N"
//! past 3; in the mini form an item and its popover. The real shell on the
//! CPU, headless, in French, test clients for windows, in a home of the
//! test's own, no session bus.
//!
//! One test per point of the maintainer's changes: up to three lines, the
//! focused one first and lit; a click focuses (a minimised window comes
//! back); the ✕ asks the window to close; "+ N" past three, which opens
//! the desktop's list; no window, no group; `[panel] windows = false`
//! hides it (and `true`, written live, brings it back); only free room is
//! used, nothing else moving as the focus changes; the mini form's
//! popover; each screen's bar its own screen's windows; nothing drawn at
//! rest. Each waits for what the shell said it drew or what the compositor
//! has, never for a time. With `HOCUS_SHOTS` naming a folder, the tests
//! save the bar's right side there.

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::mpsc::TryRecvError,
};

use common::{
    ClientHandle, Command, Event, KEY_1, KEY_2, KEY_ESC, KEY_H, KEY_LEFTMETA, WAIT, chord,
};
use hocus_compositor::{
    Config, FormChoice, Headless, HeadlessConfig, RendererChoice,
    model::{Rect, WindowId},
    radial::{RING, Sector},
};
use hocus_link::{
    DESKTOPS_NAMESPACE, MEETING_NAMESPACE, TASKS_NAMESPACE, TODAY_NAMESPACE, WINDOWS_NAMESPACE,
};

/// The screen; the full bar's surfaces' left (the bar inside its rim).
const SCREEN: (u32, u32) = (1920, 1080);
const FULL_X: i32 = 1920 - 9 - 194;
/// Around each group on its surface: the keyboard's ring; the bar's top
/// on the screen; between groups.
const ROOM: i32 = 5;
const EDGE: i32 = 8;
const GAP: i32 = 14;
const LINE: i32 = 30;
const MORE: i32 = 20;

/// The next meeting at 14:25 and two events after it: the card and Today.
const DAY: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[events]]
title = "Point produit"
start = "today 14:25"
end = "today 14:55"

[[events]]
title = "Entretien — poste back-end"
start = "today 15:00"
end = "today 15:45"

[[events]]
title = "Atelier planification T1"
start = "today 16:30"
end = "today 17:30"

[tasks]
board = "Sprint"

[[tasks.cards]]
title = "Relire la spec"

[[tasks.cards]]
title = "Écrire le ticket"

[[tasks.cards]]
title = "Préparer la démo"

[[tasks.cards]]
title = "Réserver la salle"
"##;

const DAY_SOURCES: &str = "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\
                           [[day.tasks]]\nkind = \"fixture\"\nfile = \"day.toml\"\n";

/// What the test's machine has.
struct Machine {
    screens: Vec<(u32, u32)>,
    form: FormChoice,
    /// Its day: the card, Today and To do (else none of them).
    day: bool,
    /// `[panel] windows`.
    windows: bool,
}

impl Default for Machine {
    fn default() -> Self {
        Self {
            screens: vec![SCREEN],
            form: FormChoice::Full,
            day: false,
            windows: true,
        }
    }
}

struct Bed {
    dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
    /// The test's windows' clients, by their application's id.
    clients: Vec<(&'static str, ClientHandle)>,
}

/// The configuration file for `machine`: the keyboard's layout, the
/// day's sources, `[panel] windows`.
fn config_text(machine: &Machine) -> String {
    let mut text = format!(
        "[keyboard]\nlayouts = [\"us\"]\n\n[panel]\nwindows = {}\nform = \"{}\"\nscreens = \"{}\"\n\n",
        machine.windows,
        machine.form.name(),
        if machine.screens.len() > 1 {
            "all"
        } else {
            "primary"
        }
    );
    if machine.day {
        text.push_str(DAY_SOURCES);
    }
    text
}

/// The compositor with the real shell on the CPU, for `machine`; none
/// when the shell is not built.
fn start(machine: &Machine) -> anyhow::Result<Option<Bed>> {
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    let home = root.join("home");
    for sub in ["home", "config", "state/hocuspanda", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    // Day.1's first-run card set aside ("Plus tard").
    std::fs::write(root.join("state/hocuspanda/day-sources"), "later\n")?;
    let settings = root.join("config/config.toml");
    std::fs::write(&settings, config_text(machine))?;
    std::fs::write(
        root.join("config/day.toml"),
        if machine.day { DAY } else { "" },
    )?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{config}' XDG_CACHE_HOME='{cache}' \
             XDG_STATE_HOME='{state}' XDG_DATA_HOME='{home}/.local/share' \
             TZ=UTC HOCUS_CLOCK_AT='2026-11-04T14:00:00' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             PATH='/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            config = root.join("config").display(),
            cache = root.join("cache").display(),
            state = root.join("state").display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut config = Config {
        file: Some(settings),
        plain_background: true,
        french: true,
        ..Config::default()
    };
    config.keyboard.layouts = vec!["us".to_owned()];
    config.panel.form = machine.form;
    config.panel.windows = machine.windows;
    if machine.screens.len() > 1 {
        config.panel.screens = hocus_compositor::PanelScreens::All;
    }
    let outputs = machine
        .screens
        .iter()
        .map(|(w, h)| format!("{w}x{h}").parse())
        .collect::<Result<Vec<_>, _>>()?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs,
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(WAIT, |hp| {
        hp.bar_commits() > 0 && !hp.drawn_layers(DESKTOPS_NAMESPACE).is_empty()
    })?;
    anyhow::ensure!(up, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Some(Bed {
        dir,
        hp,
        log,
        clients: Vec::new(),
    }))
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
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

/// Whether the windows show no group: their surface, kept with the bar,
/// shorter than a line, drawing nothing (or not there yet).
fn no_group(hp: &Headless) -> bool {
    hp.layer_rects(WINDOWS_NAMESPACE)
        .iter()
        .all(|(_, r)| r.height < LINE)
}

/// What the windows said they draw at rest, the last time.
const AT_REST: &str = "the windows are drawn at rest";
/// What a window's menu said it shows (#723).
const MENU: &str = "the window's menu shows it";
const BTN_RIGHT: u32 = 0x111;
const BTN_LEFT: u32 = 0x110;
const KEY_W: u32 = 17;

impl Bed {
    /// Waits until the shell's last line of `what` holds all of `holds`:
    /// that line.
    fn last_is(&mut self, what: &str, holds: &[&str]) -> anyhow::Result<String> {
        self.last_of(&[what], holds)
    }

    /// Waits until the shell's last line holding all of `what` holds all
    /// of `holds`: that line.
    fn last_of(&mut self, what: &[&str], holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let last = |text: &str| {
            text.lines()
                .rev()
                .find(|l| what.iter().all(|w| l.contains(w)))
                .filter(|l| holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned)
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| last(&plain(&log)).is_some())?;
        let text = plain(&self.log);
        anyhow::ensure!(
            done,
            "the shell's last {what:?} never held {holds:?}: {:?}\n{}",
            text.lines()
                .rev()
                .find(|l| what.iter().all(|w| l.contains(w))),
            text.lines()
                .filter(|l| l.contains("windows"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(last(&text).unwrap_or_default())
    }

    /// Waits until the windows, at rest, show `shows` (as `Words::said`
    /// says them): that line.
    fn shows(&mut self, shows: &str) -> anyhow::Result<String> {
        self.last_is(AT_REST, &[&format!("shows={shows} ")])
    }

    /// What the shell logged of its windows and the popovers, to say why a
    /// test failed.
    /// Waits until the windows' surface commits nothing for half a second.
    fn settle(&mut self) -> anyhow::Result<()> {
        let start = std::time::Instant::now();
        loop {
            let before = self.hp.windows_commits();
            common::wait(&mut self.hp, std::time::Duration::from_millis(500))?;
            if self.hp.windows_commits() == before {
                return Ok(());
            }
            anyhow::ensure!(start.elapsed() < WAIT, "the windows never came to rest");
        }
    }

    fn no_group(&self) -> bool {
        no_group(&self.hp)
    }

    fn windows_log(&self) -> String {
        plain(&self.log)
            .lines()
            .filter(|l| l.contains("windows") || l.contains("popover"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Opens a window of `app` titled `title`, and waits until the
    /// compositor has mapped it.
    fn open(&mut self, app: &'static str, title: &'static str) -> anyhow::Result<()> {
        let (_, handle) = common::open_with(&mut self.hp, app, (500, 400), false)?;
        handle.commands.send(Command::Title(title))?;
        self.clients.push((app, handle));
        Ok(())
    }

    fn client(&self, app: &str) -> anyhow::Result<&ClientHandle> {
        self.clients
            .iter()
            .find(|(a, _)| *a == app)
            .map(|(_, c)| c)
            .ok_or_else(|| anyhow::anyhow!("no client {app}"))
    }

    /// The app id of the focused window.
    fn focused(&self) -> Option<String> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    }

    /// The window id of `app`'s window.
    fn id_of(&self, app: &str) -> anyhow::Result<u64> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some(app))
            .map(|w| w.id.0)
            .ok_or_else(|| anyhow::anyhow!("no window {app}"))
    }

    /// Where `item` (as the windows log it: `Window(3)`) is on the screen
    /// of a `width` wide screen, from the last line drawn at rest.
    fn item_at(&mut self, item: &str, width: u32) -> anyhow::Result<(f64, f64)> {
        let line = self.last_is(AT_REST, &[&format!("{item}@")])?;
        let at = line
            .split(&format!("{item}@"))
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|r| r.split_once(','))
            .ok_or_else(|| anyhow::anyhow!("no {item} in {line:?}"))?;
        let (x, y): (f64, f64) = (at.0.parse()?, at.1.parse()?);
        Ok((f64::from(width) - f64::from(EDGE) + x, f64::from(EDGE) + y))
    }

    /// A click on `item` of the bar's windows, once the compositor has
    /// their surface drawn there: the pointer moves onto it after it maps
    /// (a surface mapping under a pointer at rest gets no enter).
    fn click(&mut self, item: &str) -> anyhow::Result<()> {
        let (x, y) = self.item_at(item, SCREEN.0)?;
        let there = |r: &Rect| {
            f64::from(r.x) <= x
                && x < f64::from(r.x + r.width)
                && f64::from(r.y) <= y
                && y < f64::from(r.y + r.height)
        };
        let drawn = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(WINDOWS_NAMESPACE)
                .iter()
                .any(|(_, r)| there(r))
                && hp.undrawn_at(x, y).is_empty()
        })?;
        anyhow::ensure!(drawn, "the windows never drawn at {:?}", (x, y));
        common::click_on(&mut self.hp, (x, y), WINDOWS_NAMESPACE)
    }

    /// The windows' surfaces as drawn, if any shows.
    fn group(&self) -> Option<Rect> {
        self.hp
            .drawn_layers(WINDOWS_NAMESPACE)
            .first()
            .map(|(_, r)| *r)
    }

    /// Waits until the windows' surface is drawn `height` tall (its group's
    /// height and its ring's room): where it is.
    fn group_of(&mut self, height: i32) -> anyhow::Result<Rect> {
        let done = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(WINDOWS_NAMESPACE)
                .first()
                .is_some_and(|(_, r)| r.height == height + 2 * ROOM)
        })?;
        anyhow::ensure!(
            done,
            "the windows never {height} px tall: {:?}",
            self.hp.layer_rects(WINDOWS_NAMESPACE)
        );
        self.group()
            .ok_or_else(|| anyhow::anyhow!("no windows drawn"))
    }

    /// The desktops' tiles' (or dots') top on the screen.
    fn tiles_top(&mut self) -> anyhow::Result<i32> {
        Ok(common::drawn(&mut self.hp, DESKTOPS_NAMESPACE)?[0].1.y + ROOM)
    }

    /// Waits until client `app` was asked to close its window.
    fn asked_to_close(&mut self, app: &str) -> anyhow::Result<()> {
        let index = self
            .clients
            .iter()
            .position(|(a, _)| *a == app)
            .ok_or_else(|| anyhow::anyhow!("no client {app}"))?;
        let mut asked = false;
        let done = self.hp.dispatch_until(WAIT, |_| {
            while let Ok(event) = self.clients[index].1.events.try_recv() {
                asked |= event == Event::CloseRequested;
            }
            asked
        })?;
        anyhow::ensure!(
            done,
            "{app} was never asked to close\n{}",
            self.windows_log()
        );
        Ok(())
    }

    /// Whether client `app` was asked to close its window, as of now.
    fn was_asked_to_close(&self, app: &str) -> anyhow::Result<bool> {
        let client = self.client(app)?;
        loop {
            match client.events.try_recv() {
                Ok(Event::CloseRequested) => return Ok(true),
                Ok(_) => {}
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return Ok(false),
            }
        }
    }

    /// With `HOCUS_SHOTS` naming a folder: the screen's right side (the
    /// bar and the room for its popovers), as `bar-windows-WHAT.png`.
    fn shot(&mut self, what: &str) -> anyhow::Result<()> {
        let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
            return Ok(());
        };
        common::at_rest(&mut self.hp)?;
        let (width, height, bytes) = self.hp.screenshot(0)?;
        let (w, h) = (620.min(width), height);
        let x0 = width - w;
        let mut image = image::RgbaImage::new(w as u32, h as u32);
        for y in 0..h {
            for x in 0..w {
                let at = usize::try_from(y * width + x0 + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
            }
        }
        image.save(Path::new(&folder).join(format!("bar-windows-{what}.png")))?;
        Ok(())
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
        image.save(Path::new(&folder).join(format!("{what}.png")))?;
        Ok(())
    }

    /// A right click on `item` of the bar's windows (#723).
    fn right_click(&mut self, item: &str) -> anyhow::Result<()> {
        let (x, y) = self.item_at(item, SCREEN.0)?;
        let drawn = self
            .hp
            .dispatch_until(WAIT, |hp| hp.undrawn_at(x, y).is_empty())?;
        anyhow::ensure!(drawn, "the windows never drawn at {:?}", (x, y));
        common::over(&mut self.hp, (x, y), WINDOWS_NAMESPACE)?;
        self.hp.button(BTN_RIGHT, true);
        self.hp.button(BTN_RIGHT, false);
        Ok(())
    }

    /// Where the window's menu's `target` (as it logs them: `Above`,
    /// `Tile{screen:0,desktop:1}`) is on the screen, once its popover is
    /// drawn.
    fn menu_at(&mut self, target: &str) -> anyhow::Result<(f64, f64)> {
        let id = format!("window_menu:{target}");
        let log = self.log.clone();
        let place = |log: &Path| -> Option<(f64, f64)> {
            let at = common::last_json(log, " shows {")?
                .get(&id)?
                .get("at")?
                .clone();
            let n = |i: usize| at.get(i).and_then(serde_json::Value::as_f64);
            Some((n(0)? + n(2)? / 2.0, n(1)? + n(3)? / 2.0))
        };
        let found = self.hp.dispatch_until(WAIT, |_| place(&log).is_some())?;
        anyhow::ensure!(found, "no {id} in the menu's dump\n{}", self.windows_log());
        let (x, y) = place(&log).unwrap_or_default();
        let popover = self.menu()?;
        Ok((f64::from(popover.x) + x, f64::from(popover.y) + y))
    }

    /// The window's menu's popover, once drawn.
    fn menu(&mut self) -> anyhow::Result<Rect> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(done, "no menu drawn\n{}", self.windows_log());
        Ok(common::drawn_popovers(&self.hp)[0])
    }

    /// The brightness of the screen's pixel at `(x, y)`.
    fn brightness(&mut self, (x, y): (i32, i32)) -> anyhow::Result<u32> {
        common::at_rest(&mut self.hp)?;
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        Ok(bytes[at..at + 3].iter().map(|b| u32::from(*b)).sum())
    }
}

/// A terminal and a browser, as a desktop in use has them, the browser
/// focused.
const FOOT: (&str, &str) = ("foot", "~/hocus-panda — fish");
const FIREFOX: (&str, &str) = (
    "org.mozilla.firefox",
    "Les fenêtres dans la barre · Issue #604 · mattboll/hocus-panda — Mozilla Firefox",
);

fn two(bed: &mut Bed) -> anyhow::Result<()> {
    bed.open(FOOT.0, FOOT.1)?;
    bed.open(FIREFOX.0, FIREFOX.1)
}

/// Three windows, opened in order, the last focused.
fn three(bed: &mut Bed) -> anyhow::Result<()> {
    bed.open("test.un", "Un")?;
    bed.open("test.deux", "Deux")?;
    bed.open("test.trois", "Trois")?;
    Ok(())
}

#[test]
fn up_to_three_lines_the_focused_first_and_lit() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    let focused = bed.focused();
    anyhow::ensure!(focused.as_deref() == Some(FIREFOX.0), "{focused:?}");
    bed.shows(&format!("«{}»*; «{}»", FIREFOX.1, FOOT.1))?;
    // Two 30 px lines, 14 px over the desktops' tiles, at the bar's side.
    let tiles = bed.tiles_top()?;
    let group = bed.group_of(2 * LINE)?;
    assert_eq!(group.y + group.height - ROOM + GAP, tiles);
    assert_eq!(group.x, FULL_X);
    bed.shot("two")?;
    bed.open("test.trois", "Trois")?;
    bed.shows(&format!("«Trois»*; «{}»; «{}»", FIREFOX.1, FOOT.1))?;
    let group = bed.group_of(3 * LINE)?;
    assert_eq!(group.y + group.height - ROOM + GAP, tiles);
    // The focused one's line is lit, the others not: past its ✕, where
    // nothing else is drawn.
    let x = FULL_X + 187;
    let middle = group.y + ROOM + LINE / 2;
    let (lit, plain) = (
        bed.brightness((x, middle))?,
        bed.brightness((x, middle + LINE))?,
    );
    assert!(lit > plain, "the focused line {lit}, the next {plain}");
    Ok(())
}

#[test]
fn a_click_on_a_line_focuses_its_window_even_minimised() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    three(&mut bed)?;
    bed.shows("«Trois»*; «Deux»; «Un»")?;
    bed.group_of(3 * LINE)?;
    let un = bed.id_of("test.un")?;
    bed.click(&format!("Window({un})"))?;
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows().iter().any(|w| w.focused && w.id.0 == un)
    })?;
    anyhow::ensure!(
        done,
        "Un never focused: {:?}\n{}",
        bed.focused(),
        bed.windows_log()
    );
    // It comes first, the others by most recent focus.
    bed.shows("«Un»*; «Trois»; «Deux»")?;
    // Minimised (Super+H): greyed, after the window focused now.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_H]);
    let done = bed.hp.dispatch_until(WAIT, |hp| hp.minimized() == 1)?;
    anyhow::ensure!(done, "Un never minimised");
    bed.shows("«Trois»*; «Un»~; «Deux»")?;
    // A click brings it back, focused.
    bed.click(&format!("Window({un})"))?;
    bed.shows("«Un»*; «Trois»; «Deux»")?;
    assert_eq!(bed.hp.minimized(), 0);
    assert_eq!(bed.focused().as_deref(), Some("test.un"));
    Ok(())
}

#[test]
fn the_close_button_asks_its_window_to_close() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    three(&mut bed)?;
    bed.shows("«Trois»*; «Deux»; «Un»")?;
    bed.group_of(3 * LINE)?;
    // Deux's ✕, not the focused window's: Deux alone is asked.
    let deux = bed.id_of("test.deux")?;
    bed.click(&format!("WindowClose({deux})"))?;
    bed.asked_to_close("test.deux")?;
    assert!(!bed.was_asked_to_close("test.trois")?);
    assert!(!bed.was_asked_to_close("test.un")?);
    // The application decides: it stays, and so does its line; the focus
    // did not move.
    assert_eq!(bed.focused().as_deref(), Some("test.trois"));
    bed.shows("«Trois»*; «Deux»; «Un»")?;
    // It closes: its line goes.
    bed.client("test.deux")?.commands.send(Command::Close)?;
    bed.shows("«Trois»*; «Un»")?;
    bed.group_of(2 * LINE)?;
    Ok(())
}

#[test]
fn past_three_windows_plus_n_opens_the_overview() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    bed.open("org.gnome.Nautilus", "Téléchargements")?;
    bed.open("code", "bar.md — hocus-panda — Visual Studio Code")?;
    bed.open("org.gnome.TextEditor", "Notes de réunion")?;
    bed.shows(
        "«Notes de réunion»*; «bar.md — hocus-panda — Visual Studio Code»; \
         «Téléchargements»; + 2 autres",
    )?;
    let tiles = bed.tiles_top()?;
    let group = bed.group_of(3 * LINE + MORE)?;
    assert_eq!(group.y + group.height - ROOM + GAP, tiles);
    bed.shot("five")?;
    // "+ 2 autres": the overview, every window of the desktop laid out
    // (8.5 A); the bar stays in front, and takes the clicks.
    bed.click("WindowsMore")?;
    let opened = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.overview() == Some(true))?;
    anyhow::ensure!(opened, "no overview");
    assert_eq!(bed.hp.overview_windows().len(), 5);
    assert_eq!(bed.hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    bed.shot_of("overview-from-the-bar", Rect::new(0, 0, 1920, 1080))?;
    // Again: it closes.
    bed.click("WindowsMore")?;
    let closed = bed.hp.dispatch_until(WAIT, |hp| hp.overview().is_none())?;
    anyhow::ensure!(closed, "the overview stays");
    Ok(())
}

/// The overview on two screens with the real bar (8.5 A): with
/// `HOCUS_SHOTS`, the screens side by side as the overview opens, a
/// terminal hosting three agents (two waiting) badged; then a window
/// dragged over a desktop's thumbnail.
#[test]
fn the_overview_on_two_screens_with_the_bar_and_the_agents() -> anyhow::Result<()> {
    let machine = Machine {
        screens: vec![SCREEN, (2560, 1440)],
        ..Machine::default()
    };
    let Some(mut bed) = start(&machine)? else {
        return Ok(());
    };
    bed.hp.pointer_move(960.0, 540.0);
    bed.open("firefox", "Docs — Mozilla Firefox")?;
    bed.open("org.gnome.Nautilus", "Téléchargements")?;
    bed.hp.pointer_move(1920.0 + 1280.0, 720.0);
    bed.open("code", "overview.rs — hocus-panda — Visual Studio Code")?;
    // The agents' terminal: this process's last window, as a terminal is
    // its agents' ancestor.
    bed.open("kitty", "herdr — 3 agents")?;
    let socket = bed
        .hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let pid = std::process::id();
    for (name, state) in [
        ("migration", "waiting"),
        ("api-paiements", "waiting"),
        ("site-vitrine", "working"),
    ] {
        use std::io::Write;
        let mut stream = std::os::unix::net::UnixStream::connect(&socket)?;
        writeln!(stream, "agent command {name} {state} {pid} {name}")?;
    }
    let until = std::time::Instant::now() + WAIT;
    while bed.hp.agents().len() < 3 && std::time::Instant::now() < until {
        bed.hp.dispatch(std::time::Duration::from_millis(10))?;
    }
    bed.hp.pointer_move(960.0, 540.0);
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_W]);
    assert_eq!(bed.hp.overview(), Some(true));
    assert_eq!(bed.hp.overview_windows().len(), 4);
    assert_eq!(
        bed.hp.overview_badges(),
        [(Some("kitty".to_owned()), "waiting".to_owned(), 2)]
    );
    common::wait(&mut bed.hp, std::time::Duration::from_millis(600))?;
    shot_screens(&mut bed.hp, "overview-two-screens-agents")?;
    // Firefox dragged over the second screen's third desktop.
    let (_, (x, y, w, h)) = bed
        .hp
        .overview_windows()
        .into_iter()
        .find(|(a, _)| a.as_deref() == Some("firefox"))
        .ok_or_else(|| anyhow::anyhow!("no firefox laid out"))?;
    let (_, _, to) = bed
        .hp
        .overview_thumbs()
        .into_iter()
        .find(|(o, d, _)| o == "HEADLESS-2" && *d == 2)
        .ok_or_else(|| anyhow::anyhow!("no thumbnail"))?;
    let from = (x + w / 2.0, y + h / 3.0);
    bed.hp.pointer_move(from.0, from.1);
    bed.hp.button(BTN_LEFT, true);
    for step in 1..=16 {
        let t = f64::from(step) / 16.0;
        bed.hp
            .pointer_move(from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t);
    }
    common::wait(&mut bed.hp, std::time::Duration::from_millis(300))?;
    assert_eq!(
        bed.hp.overview_drag(),
        Some((
            Some("firefox".to_owned()),
            Some(("HEADLESS-2".to_owned(), 2))
        ))
    );
    shot_screens(&mut bed.hp, "overview-dragging-onto-a-thumbnail")?;
    bed.hp.button(BTN_LEFT, false);
    common::wait(&mut bed.hp, std::time::Duration::from_millis(300))?;
    shot_screens(&mut bed.hp, "overview-dropped")?;
    Ok(())
}

/// With `HOCUS_SHOTS` naming a folder: every screen side by side, as
/// `WHAT.png`; a frame of its own drawn first (a waiting agent's panda
/// waves on: the overview never rests).
fn shot_screens(hp: &mut Headless, what: &str) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    hp.hold_clock();
    hp.step(std::time::Duration::from_millis(400))?;
    let screens = [hp.screenshot(0)?, hp.screenshot(1)?];
    hp.release_clock();
    let width: i32 = screens.iter().map(|s| s.0).sum();
    let height = screens.iter().map(|s| s.1).max().unwrap_or(0);
    let mut image =
        image::RgbaImage::from_pixel(width as u32, height as u32, image::Rgba([14, 15, 18, 255]));
    let mut x0 = 0;
    for (w, h, bytes) in &screens {
        for y in 0..*h {
            for x in 0..*w {
                let at = usize::try_from(y * w + x)? * 4;
                let p = &bytes[at..at + 4];
                image.put_pixel(
                    (x0 + x) as u32,
                    y as u32,
                    image::Rgba([p[2], p[1], p[0], 255]),
                );
            }
        }
        x0 += w;
    }
    image.save(Path::new(&folder).join(format!("{what}.png")))?;
    Ok(())
}

#[test]
fn no_window_no_group() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    // The bar drawn, its tiles too: no group (its surface kept with the
    // bar, drawing nothing, #617).
    bed.last_is("the desktops' tiles show it", &[])?;
    assert!(
        bed.no_group(),
        "{:?}",
        bed.hp.layer_rects(WINDOWS_NAMESPACE)
    );
    bed.open("test.un", "Un")?;
    bed.shows("«Un»*")?;
    bed.group_of(LINE)?;
    // An empty desktop: none.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    let gone = bed.hp.dispatch_until(WAIT, no_group)?;
    anyhow::ensure!(gone, "the windows show on an empty desktop");
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_1]);
    bed.group_of(LINE)?;
    // Its window closed: none.
    bed.client("test.un")?.commands.send(Command::Close)?;
    let gone = bed.hp.dispatch_until(WAIT, no_group)?;
    anyhow::ensure!(gone, "the windows show without a window");
    Ok(())
}

#[test]
fn panel_windows_false_hides_them_and_true_brings_them_back_live() -> anyhow::Result<()> {
    let machine = Machine {
        windows: false,
        ..Machine::default()
    };
    let Some(mut bed) = start(&machine)? else {
        return Ok(());
    };
    bed.open("test.un", "Un")?;
    bed.open("test.deux", "Deux")?;
    // The shell has the windows (its tiles sketch them): none listed.
    let deux = bed.id_of("test.deux")?;
    bed.last_is("the desktops' tiles show it", &[&format!("id: {deux}")])?;
    assert!(
        bed.no_group(),
        "{:?}",
        bed.hp.layer_rects(WINDOWS_NAMESPACE)
    );
    // `true` written in the file: they show, as the file is followed.
    let settings = bed.dir.path().join("config/config.toml");
    let on = Machine {
        windows: true,
        ..Machine::default()
    };
    std::fs::write(&settings, config_text(&on))?;
    bed.shows("«Deux»*; «Un»")?;
    bed.group_of(2 * LINE)?;
    Ok(())
}

#[test]
fn only_free_room_is_used_nothing_else_moves_when_the_focus_changes() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine {
        day: true,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    three(&mut bed)?;
    bed.shows("«Trois»*; «Deux»; «Un»")?;
    let group = bed.group_of(3 * LINE)?;
    // The card, Today and To do in the column, the windows under them in
    // the room left, over the desktops.
    let parts = [
        MEETING_NAMESPACE,
        TODAY_NAMESPACE,
        TASKS_NAMESPACE,
        DESKTOPS_NAMESPACE,
        WINDOWS_NAMESPACE,
    ];
    // The day read: To do's three lines and "+ 1 autre", Today's two.
    let tall = |hp: &Headless, ns: &str, height: i32| {
        hp.drawn_layers(ns)
            .first()
            .is_some_and(|(_, r)| r.height == height + 2 * ROOM)
    };
    let laid = bed.hp.dispatch_until(WAIT, |hp| {
        tall(hp, TASKS_NAMESPACE, 18 + 3 * 22 + 20)
            && tall(hp, TODAY_NAMESPACE, 18 + 2 * 20)
            && tall(hp, MEETING_NAMESPACE, 44)
    })?;
    anyhow::ensure!(
        laid,
        "the day never laid out: {:?}",
        parts.map(|ns| bed.hp.layer_rects(ns))
    );
    common::at_rest(&mut bed.hp)?;
    let tasks = common::drawn(&mut bed.hp, TASKS_NAMESPACE)?[0].1;
    assert!(
        tasks.y + tasks.height - ROOM + GAP <= group.y + ROOM,
        "To do {tasks:?} over the windows {group:?}"
    );
    let placed = |hp: &Headless| -> Vec<Vec<(String, Rect)>> {
        parts.iter().map(|ns| hp.layer_rects(ns)).collect()
    };
    let before = placed(&bed.hp);
    bed.shot("with-the-day")?;
    // Each window focused in turn, from its line: the lines reorder,
    // nothing moves.
    for (app, shows) in [
        ("test.un", "«Un»*; «Trois»; «Deux»"),
        ("test.deux", "«Deux»*; «Un»; «Trois»"),
        ("test.trois", "«Trois»*; «Deux»; «Un»"),
    ] {
        let id = bed.id_of(app)?;
        bed.click(&format!("Window({id})"))?;
        bed.shows(shows)?;
        common::at_rest(&mut bed.hp)?;
        assert_eq!(placed(&bed.hp), before, "after {app} took the focus");
    }
    Ok(())
}

#[test]
fn the_mini_form_lists_them_in_a_popover() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine {
        form: FormChoice::Mini,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two(&mut bed)?;
    bed.open("org.gnome.Nautilus", "Téléchargements")?;
    bed.open("org.gnome.TextEditor", "Notes de réunion")?;
    bed.last_is(AT_REST, &["form=mini", "item=Some(\"Notes de réunion\")"])?;
    // The item, 44 × 34, 14 px over the dots.
    let dots = bed.tiles_top()?;
    let item = bed.group_of(34)?;
    assert_eq!(item.y + item.height - ROOM + GAP, dots);
    // A click opens the popover: three lines and "+ 1 autre".
    bed.click("Windows")?;
    bed.last_is(
        "the windows' popover shows it",
        &[&format!(
            "shows=«Notes de réunion»*; «Téléchargements»; «{}»; + 1 autre",
            FIREFOX.1
        )],
    )?;
    let done = bed
        .hp
        .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(done, "no popover drawn");
    let popover = common::drawn_popovers(&bed.hp)[0];
    // 240 px, 8 px from the bar, its bottom on the item's.
    assert_eq!(popover.width, 240);
    assert_eq!(popover.x + popover.width, 1920 - 8 - 56 - 8);
    assert_eq!(popover.y + popover.height, item.y + item.height - ROOM);
    bed.shot("mini-popover")?;
    // Its lines: 30 px from 9 px in; the ✕ 4 px in from a line's right.
    let middle = |i: i32| f64::from(popover.y + 9 + LINE * i + LINE / 2);
    let close_x = f64::from(popover.x + 7 + 226 - 4 - 11);
    // The second line's ✕: its window asked to close, the popover
    // staying.
    common::click(&mut bed.hp, (close_x, middle(1)))?;
    bed.asked_to_close("org.gnome.Nautilus")?;
    assert!(!common::drawn_popovers(&bed.hp).is_empty());
    // The third line: the browser focused, the popover closing.
    common::click(&mut bed.hp, (f64::from(popover.x + 60), middle(2)))?;
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.focused && w.app_id.as_deref() == Some(FIREFOX.0))
            && hp.popover_surfaces().is_empty()
    })?;
    anyhow::ensure!(done, "the browser not focused, or the popover still open");
    bed.last_is(
        AT_REST,
        &["form=mini", &format!("item=Some({:?})", FIREFOX.1)],
    )?;
    Ok(())
}

#[test]
fn each_screens_bar_lists_its_own_screens_windows() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine {
        screens: vec![SCREEN, SCREEN],
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.bars().len() == 2 && hp.drawn_layers(DESKTOPS_NAMESPACE).len() == 2
    })?;
    anyhow::ensure!(done, "not two bars: {:?}", bed.hp.bars());
    // A window on each screen, opened under the pointer there.
    bed.hp.pointer_move(400.0, 600.0);
    bed.open("test.gauche", "Gauche")?;
    bed.hp.pointer_move(1920.0 + 400.0, 600.0);
    bed.open("test.droite", "Droite")?;
    let mut bars = bed.hp.bars();
    bars.sort_by(|a, b| a.0.cmp(&b.0));
    let (first, second) = (bars[0].0.clone(), bars[1].0.clone());
    // The focus on the second screen: the first's window is not lit.
    bed.last_of(&[AT_REST, &format!("bar={first} ")], &["shows=«Gauche» "])?;
    let line = bed.last_of(&[AT_REST, &format!("bar={second} ")], &["shows=«Droite»* "])?;
    assert!(!line.contains("Gauche"), "{line}");
    // One line on each bar, each on its screen.
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        let drawn = hp.drawn_layers(WINDOWS_NAMESPACE);
        drawn.len() == 2 && drawn.iter().all(|(_, r)| r.height == LINE + 2 * ROOM)
    })?;
    anyhow::ensure!(done, "{:?}", bed.hp.drawn_layers(WINDOWS_NAMESPACE));
    let mut xs: Vec<i32> = bed
        .hp
        .drawn_layers(WINDOWS_NAMESPACE)
        .iter()
        .map(|(_, r)| r.x)
        .collect();
    xs.sort_unstable();
    assert_eq!(xs, [FULL_X, 1920 + FULL_X]);
    // Each bar shows its own line: the first bar's never Droite.
    let text = plain(&bed.log);
    assert!(
        !text
            .lines()
            .filter(|l| l.contains(AT_REST) && l.contains(&format!("bar={first} ")))
            .any(|l| l.contains("Droite")),
        "the first screen's bar listed the second's window"
    );
    Ok(())
}

#[test]
fn nothing_draws_at_rest_and_a_move_draws_nothing() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    bed.open("test.un", "Un")?;
    bed.open("test.deux", "Deux")?;
    bed.shows("«Deux»*; «Un»")?;
    common::at_rest(&mut bed.hp)?;
    // The shell done with what it drew (a loaded machine commits late).
    bed.settle()?;
    // Snapping the focused window to a side moves its sketch in the tiles,
    // not what the windows list: the desktops draw, the windows do not.
    let (windows, desktops) = (bed.hp.windows_commits(), bed.hp.desktops_commits());
    chord(&mut bed.hp, &[KEY_LEFTMETA, common::KEY_LEFT]);
    let drawn = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.desktops_commits() > desktops)?;
    anyhow::ensure!(drawn, "the tiles never drew the move");
    common::at_rest(&mut bed.hp)?;
    let frames = bed.hp.frames();
    bed.hp.pointer_move(600.0, 300.0);
    bed.hp.pointer_move(700.0, 400.0);
    bed.hp.dispatch_until(WAIT, |hp| hp.frames() > frames + 1)?;
    common::at_rest(&mut bed.hp)?;
    assert_eq!(
        bed.hp.windows_commits(),
        windows,
        "the windows drew\n{}",
        bed.windows_log()
    );
    // A retitle draws them.
    bed.client("test.un")?
        .commands
        .send(Command::Title("Un, renommé"))?;
    bed.shows("«Deux»*; «Un, renommé»")?;
    let drew = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.windows_commits() > windows)?;
    anyhow::ensure!(drew, "the retitle drew nothing");
    Ok(())
}

/// A right click on a window's line opens its menu beside the bar (#723):
/// the radial menu's actions, "Envoyer sur" with the screen's desktops,
/// the window's own ringed; its Close asks the window to close.
#[test]
fn a_right_click_on_a_line_opens_its_windows_menu_beside_the_bar() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    bed.shows(&format!("«{}»*; «{}»", FIREFOX.1, FOOT.1))?;
    let id = bed.id_of(FOOT.0)?;
    bed.right_click(&format!("Window({id})"))?;
    bed.last_is(
        MENU,
        &[
            &format!("window={id} "),
            "shows=Agrandir; Réduire; Au premier plan=off; Sur tous les bureaux=off; ENVOYER SUR ",
            "[1* 2 3 4]; Fermer",
        ],
    )?;
    let menu = bed.menu()?;
    // 240 px, beside the bar, whole on the screen.
    assert_eq!(menu.width, 240);
    assert!(menu.x + menu.width <= FULL_X - 8, "{menu:?}");
    assert!(menu.y >= 0 && menu.y + menu.height <= 1080, "{menu:?}");
    bed.shot_of("window-menu", Rect::new(1920 - 560, 1080 - 560, 560, 560))?;
    // `Esc` closes it; its Close asks the window.
    chord(&mut bed.hp, &[KEY_ESC]);
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the menu stays after Esc");
    bed.right_click(&format!("Window({id})"))?;
    let close = bed.menu_at("Close")?;
    common::click(&mut bed.hp, close)?;
    bed.asked_to_close(FOOT.0)?;
    Ok(())
}

/// The menu's top on the line right-clicked (#726), kept whole on the
/// screen: here, nothing under the tiles, the lines sit near the bar's
/// foot, and the menu is pushed up until its bottom is the bar's. The
/// lines lifted by the levels under the tiles, as on a laptop, are
/// `bar_desktops`' (`a_windows_menu_opens_with_its_top_on_its_line`).
#[test]
fn low_in_the_bar_the_windows_menu_is_pushed_up_whole() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    bed.shows(&format!("«{}»*; «{}»", FIREFOX.1, FOOT.1))?;
    for app in [FIREFOX.0, FOOT.0] {
        let id = bed.id_of(app)?;
        let item = format!("Window({id})");
        let (_, middle) = bed.item_at(&item, SCREEN.0)?;
        let line = (middle - f64::from(LINE) / 2.0).round() as i32;
        bed.right_click(&item)?;
        bed.last_is(MENU, &[&format!("window={id} ")])?;
        let menu = bed.menu()?;
        let bottom = SCREEN.1 as i32 - EDGE;
        assert!(line + menu.height > bottom, "the line is not low: {line}");
        assert_eq!(menu.y + menu.height, bottom, "{app}: {menu:?}");
        chord(&mut bed.hp, &[KEY_ESC]);
        let gone = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(gone, "the menu stays after Esc");
    }
    Ok(())
}

/// Its switches flip and the line shows their marks; a tile sends the
/// window to that desktop, the screen staying (#723).
#[test]
fn the_window_menus_switches_and_tiles_act_on_its_window() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    let id = bed.id_of(FOOT.0)?;
    bed.right_click(&format!("Window({id})"))?;
    let above = bed.menu_at("Above")?;
    common::click(&mut bed.hp, above)?;
    let done = bed.hp.dispatch_until(WAIT, |hp| hp.above(WindowId(id)))?;
    anyhow::ensure!(done, "not kept above");
    bed.last_is(MENU, &["Au premier plan=on"])?;
    let all = bed.menu_at("AllDesktops")?;
    common::click(&mut bed.hp, all)?;
    let done = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.everywhere() == vec![WindowId(id)])?;
    anyhow::ensure!(done, "not on all desktops");
    bed.shows(&format!("«{}»*; «{}»^∀", FIREFOX.1, FOOT.1))?;
    bed.shot_of(
        "window-menu-switches",
        Rect::new(1920 - 560, 1080 - 560, 560, 560),
    )?;
    // Desktop 3's tile: there alone, out of sight, the screen staying.
    let tile = bed.menu_at("Tile{screen:0,desktop:2}")?;
    common::click(&mut bed.hp, tile)?;
    let done = bed.hp.dispatch_until(WAIT, |hp| {
        hp.all_windows()
            .iter()
            .any(|(w, _, d, shown)| *w == WindowId(id) && *d == 2 && !shown)
            && hp.popover_surfaces().is_empty()
    })?;
    anyhow::ensure!(done, "not sent: {:?}", bed.hp.all_windows());
    assert!(bed.hp.everywhere().is_empty());
    assert_eq!(bed.hp.desktop().0, 0);
    bed.shows(&format!("«{}»*", FIREFOX.1))?;
    Ok(())
}

/// The radial menu over a window, with the bar (#723): Super and a right
/// click over the bar opens nothing; over the window, the ring on the
/// pointer. With `HOCUS_SHOTS`, the ring open, a sector lit, Close
/// armed, a toggle on.
#[test]
fn the_radial_menu_opens_over_a_window_never_over_the_bar() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    let (rect, handle) = common::open_with(&mut bed.hp, "foot", (1100, 700), false)?;
    handle.commands.send(Command::Title(FOOT.1))?;
    bed.clients.push(("foot", handle));
    bed.shows(&format!("«{}»*", FOOT.1))?;
    let super_click = |hp: &mut Headless, (x, y): (f64, f64)| {
        hp.pointer_move(x, y);
        hp.key(KEY_LEFTMETA, true);
        hp.button(BTN_RIGHT, true);
        hp.button(BTN_RIGHT, false);
        hp.key(KEY_LEFTMETA, false);
    };
    // Over the bar: nothing.
    let bar = (f64::from(FULL_X + 90), 300.0);
    super_click(&mut bed.hp, bar);
    assert!(bed.hp.radial().is_none(), "opened over the bar");
    let at = (
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    );
    super_click(&mut bed.hp, at);
    let seen = bed.hp.radial().expect("the menu over the window");
    assert_eq!(seen.centre, at);
    let around = Rect::new(at.0 as i32 - 220, at.1 as i32 - 210, 440, 420);
    // Up, lit.
    bed.hp.pointer_move(at.0, at.1 - 60.0);
    bed.hp.pointer_move(at.0, at.1 - 80.0);
    assert_eq!(bed.hp.radial().map(|r| r.lit), Some(Some(Sector::Up)));
    bed.shot_of("radial-lit", around)?;
    // Close, rested on.
    let a = 135_f64.to_radians();
    let close = (at.0 + 80.0 * a.cos(), at.1 + 80.0 * a.sin());
    bed.hp.pointer_move(close.0, close.1);
    let armed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.radial().is_some_and(|r| r.armed))?;
    anyhow::ensure!(armed, "Close never armed");
    bed.shot_of("radial-close", around)?;
    // Always on top, by a gesture; open again, its toggle on.
    bed.hp.pointer_move(at.0 + 40.0, at.1 - 40.0);
    let a = (-45_f64).to_radians();
    bed.hp.pointer_move(
        at.0 + (RING + 20.0) * a.cos(),
        at.1 + (RING + 20.0) * a.sin(),
    );
    assert!(bed.hp.radial().is_none());
    bed.shows(&format!("«{}»*^", FOOT.1))?;
    super_click(&mut bed.hp, at);
    assert!(
        bed.hp
            .radial()
            .is_some_and(|r| r.on[Sector::UpRight.index()])
    );
    bed.hp.pointer_move(at.0 - 70.0, at.1);
    bed.hp.pointer_move(at.0 - 80.0, at.1 + 2.0);
    bed.shot_of("radial-toggle-on", around)?;
    chord(&mut bed.hp, &[KEY_ESC]);
    assert!(bed.hp.radial().is_none());
    Ok(())
}

/// The mini form's popover and a window's menu in their main state, shot
/// under `HOCUS_KIT_SHOTS` (#772's side-by-sides; see `wifi::kit_shot`).
#[test]
#[ignore = "writes screenshots under HOCUS_KIT_SHOTS"]
fn kit_shot() -> anyhow::Result<()> {
    let Some(mut bed) = start(&Machine {
        form: FormChoice::Mini,
        ..Machine::default()
    })?
    else {
        return Ok(());
    };
    two(&mut bed)?;
    bed.open("org.gnome.Nautilus", "Téléchargements")?;
    bed.open("org.gnome.TextEditor", "Notes de réunion")?;
    bed.last_is(AT_REST, &["form=mini", "item=Some(\"Notes de réunion\")"])?;
    bed.click("Windows")?;
    bed.last_is(
        "the windows' popover shows it",
        &["shows=«Notes de réunion»*"],
    )?;
    bed.hp.pointer_move(600.0, 500.0);
    common::popover_shot(&mut bed.hp, "windows")?;
    drop(bed);
    let Some(mut bed) = start(&Machine::default())? else {
        return Ok(());
    };
    two(&mut bed)?;
    let id = bed.id_of(FOOT.0)?;
    bed.right_click(&format!("Window({id})"))?;
    bed.last_is(MENU, &[&format!("window={id} ")])?;
    bed.menu()?;
    bed.hp.pointer_move(600.0, 500.0);
    common::popover_shot(&mut bed.hp, "window-menu")
}
