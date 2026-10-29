//! The keyboard layout's row (docs/design/bar.md §4.3, §5; issue #365):
//! the real shell, its levels against the fakes (`levels/fakes.rs`), the
//! layouts the compositor's keymap holds.
//!
//! Two layouts: the row (full form) under the other connections, or the
//! code in its button under the battery (mini form), the first's name and
//! code; a click goes to the next layout, which the window holding the
//! keyboard sees in its modifiers, and the row follows, round and round.
//! Three, two of them French: the variant's code ("BÉPO"). One: no row,
//! no click taken. A group switch of the user's XKB options moves the row
//! too, drawing it alone; nothing draws at rest. The keyboard reaches the
//! row from the Wi-Fi's (Esc in its popover, then Tab), where Enter and
//! Space go to the next layout. The names in French from XKB's catalog (a
//! catalog of the test's), in English from the keymap.
//!
//! Pixman here, so the shell draws on the CPU, without motion: as in CI.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;
#[path = "levels/nm.rs"]
mod nm;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, ClientHandle, Event, KEY_ESC, KEY_LEFTALT, KEY_LEFTSHIFT, KEY_TAB, Shows, WAIT,
    chord, wait,
};
use fakes::Fakes;
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;

/// XKB's names of the test's layouts, and their French names.
const FRENCH: [(&str, &str); 4] = [
    ("French", "Français"),
    ("French (BEPO)", "Français (BÉPO)"),
    ("English (US)", "Anglais (US)"),
    ("Russian", "Russe"),
];

/// A little-endian `.mo` catalog of `pairs`, as msgfmt writes one.
fn mo(pairs: &[(&str, &str)]) -> Vec<u8> {
    let n = pairs.len();
    let originals = 28;
    let translations = originals + 8 * n;
    let mut strings = translations + 8 * n;
    let (mut tables, mut data) = (Vec::new(), Vec::new());
    for side in [0, 1] {
        for pair in pairs {
            let text = if side == 0 { pair.0 } else { pair.1 };
            tables.extend((text.len() as u32).to_le_bytes());
            tables.extend((strings as u32).to_le_bytes());
            data.extend(text.as_bytes());
            data.push(0);
            strings += text.len() + 1;
        }
    }
    let mut file = Vec::new();
    for word in [
        0x9504_12de_u32,
        0,
        n as u32,
        originals as u32,
        translations as u32,
        0,
        0,
    ] {
        file.extend(word.to_le_bytes());
    }
    file.extend(tables);
    file.extend(data);
    file
}

/// What a test starts.
struct Setup {
    layouts: &'static [&'static str],
    options: &'static str,
    lang: &'static str,
    mini: bool,
    /// NetworkManager on a network: the Wi-Fi's row over the layout's.
    wifi: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            layouts: &["fr", "us"],
            options: "",
            lang: "fr_FR.UTF-8",
            mini: false,
            wifi: false,
        }
    }
}

struct Bed {
    _fakes: Fakes,
    _nm: Option<nm::Nm>,
    hp: Headless,
    log: PathBuf,
    /// A window holding the keyboard, and the groups it was given.
    client: ClientHandle,
    groups: Vec<u32>,
}

fn ready() -> Option<PathBuf> {
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
        return None;
    };
    if !fakes::available() {
        eprintln!("dbus-daemon, pipewire, pw-dump, wpctl or pw-metadata missing: skipped");
        return None;
    }
    Some(shell)
}

fn start(shell: &Path, setup: Setup) -> anyhow::Result<Bed> {
    let fakes = Fakes::start()?;
    let nm = if setup.wifi {
        let livebox = "Livebox-A3F2";
        let wifi = nm::Setup {
            aps: vec![nm::FakeAp::new(livebox, 80, nm::Security::Psk("t"))],
            saved: vec![(livebox, Some("t"))],
            current: Some(livebox),
            ..nm::Setup::default()
        };
        Some(nm::Nm::start(fakes.runtime(), &fakes.address, wifi)?)
    } else {
        None
    };
    let locale = fakes.dir.path().join("locale");
    let messages = locale.join("fr/LC_MESSAGES");
    std::fs::create_dir_all(&messages)?;
    std::fs::write(messages.join("xkeyboard-config-2.mo"), mo(&FRENCH))?;
    let log = fakes.dir.path().join("shell.log");
    let env = [
        ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ("HOCUS_LOCALE_DIR", locale.display().to_string()),
    ];
    let script = fakes.shell_with(shell, setup.lang, &env, Some(&log))?;
    let mut config = Config::default();
    config.keyboard.layouts = setup.layouts.iter().map(|l| (*l).to_owned()).collect();
    config.keyboard.options = setup.options.to_owned();
    if setup.mini {
        config.panel.form = FormChoice::Mini;
    }
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.connections_surfaces().len() == 1
            && hp.connections_commits() > 0
            && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no connections: {:?}", hp.connections_surfaces());
    let before = hp.windows().len();
    let client = common::client(
        hp.socket(),
        Some(Shows::Typing {
            app_id: "typing",
            size: (400, 300),
        }),
    )?;
    let mapped = hp.dispatch_until(WAIT, |hp| {
        hp.windows().len() > before && hp.windows().iter().any(|w| w.keyboard)
    })?;
    anyhow::ensure!(mapped, "the window never took the keyboard");
    let mut bed = Bed {
        _fakes: fakes,
        _nm: nm,
        hp,
        log,
        client,
        groups: Vec::new(),
    };
    bed.settle()?;
    Ok(bed)
}

impl Bed {
    /// Waits until nothing of the shell's commits for a while.
    fn settle(&mut self) -> anyhow::Result<()> {
        let start = Instant::now();
        loop {
            let counts = self.counts();
            wait(&mut self.hp, Duration::from_millis(600))?;
            if self.counts() == counts || start.elapsed() > WAIT {
                return Ok(());
            }
        }
    }

    /// The commits of the bar's, the levels', the connections' and the
    /// popovers' surfaces.
    fn counts(&self) -> (u64, u64, u64, u64) {
        (
            self.hp.bar_commits(),
            self.hp.levels_commits(),
            self.hp.connections_commits(),
            self.hp.popover_commits(),
        )
    }

    fn connections(&self) -> Rect {
        self.hp
            .connections_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    fn levels(&self) -> Rect {
        self.hp
            .levels_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// The layout's row (the connections' lowest, full form) or its button
    /// (6 px over the levels' surface's bottom, mini form): its middle.
    fn row(&self, mini: bool) -> (f64, f64) {
        if mini {
            let l = self.levels();
            (
                f64::from(l.x + l.width / 2),
                f64::from(l.y + l.height - 6 - 15),
            )
        } else {
            let s = self.connections();
            (
                f64::from(s.x + s.width / 2),
                f64::from(s.y + s.height) - 5.5 - 15.0,
            )
        }
    }

    fn click(&mut self, (x, y): (f64, f64)) {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// Waits until the window holding the keyboard is given `group`.
    fn group(&mut self, group: u32) -> anyhow::Result<()> {
        let (events, groups) = (&self.client.events, &mut self.groups);
        let given = self.hp.dispatch_until(WAIT, |_| {
            while let Ok(event) = events.try_recv() {
                if let Event::Group(g) = event {
                    groups.push(g);
                }
            }
            groups.last() == Some(&group)
        })?;
        anyhow::ensure!(given, "the window was given {:?}", self.groups);
        Ok(())
    }

    /// Waits until the shell's log has a line with `text`, past `from`
    /// bytes of it.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<()> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        anyhow::ensure!(done, "the shell never logged {text:?}");
        Ok(())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<()> {
        self.logged_after(0, text)
    }

    /// How long the log is now.
    fn mark(&self) -> usize {
        plain(&self.log).len()
    }

    /// The pixels of `rect` (global).
    fn pixels(&mut self, rect: Rect) -> anyhow::Result<Vec<u8>> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let mut out = Vec::new();
        for y in rect.y..rect.y + rect.height {
            let at = usize::try_from(y * width + rect.x)? * 4;
            let row = bytes
                .get(at..at + usize::try_from(rect.width)? * 4)
                .ok_or_else(|| anyhow::anyhow!("{rect:?} off the screen"))?;
            out.extend_from_slice(row);
        }
        Ok(out)
    }

    /// The light pixels in `rect` (global).
    fn ink(&mut self, rect: Rect) -> anyhow::Result<usize> {
        let pixels = self.pixels(rect)?;
        Ok(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[0].min(p[1]).min(p[2]) > 150)
            .count())
    }

    /// The row's rectangle (full form), from its middle.
    fn row_rect(&self) -> Rect {
        let (_, middle) = self.row(false);
        let s = self.connections();
        Rect::new(s.x + 5, middle as i32 - 15, s.width - 10, 30)
    }

    /// The commits, and the screen's damage, a change made by `change`
    /// brings, once the surface it draws on (`connections` or not: the
    /// levels') has drawn: tried again while the clock ticks through it.
    fn drawn_by(
        &mut self,
        connections: bool,
        mut change: impl FnMut(&mut Self),
    ) -> anyhow::Result<((u64, u64, u64, u64), Rect)> {
        for _ in 0..5 {
            self.settle()?;
            let clocks = self.hp.clock_commits();
            wait(&mut self.hp, Duration::from_millis(100))?;
            self.hp.take_damage(0);
            let before = self.counts();
            change(self);
            let drawn = self.hp.dispatch_until(WAIT, |hp| {
                if connections {
                    hp.connections_commits() > before.2
                } else {
                    hp.levels_commits() > before.1
                }
            })?;
            anyhow::ensure!(drawn, "the layout did not draw");
            wait(&mut self.hp, Duration::from_millis(150))?;
            let after = self.counts();
            let ticked = self.hp.clock_commits() - clocks;
            eprintln!("the clock committed {ticked} times meanwhile");
            if ticked == 0 {
                let damage = self.hp.take_damage(0).unwrap_or(Rect::new(0, 0, 0, 0));
                let commits = (
                    after.0 - before.0,
                    after.1 - before.1,
                    after.2 - before.2,
                    after.3 - before.3,
                );
                return Ok((commits, damage));
            }
        }
        anyhow::bail!("the clock ticked through every try")
    }

    /// The commits in two seconds at rest.
    fn at_rest(&mut self) -> anyhow::Result<(u64, u64, u64, u64)> {
        self.settle()?;
        let before = self.counts();
        wait(&mut self.hp, Duration::from_secs(2))?;
        let after = self.counts();
        Ok((
            after.0 - before.0,
            after.1 - before.1,
            after.2 - before.2,
            after.3 - before.3,
        ))
    }
}

/// What the shell logged, without its colours.
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

/// What the row's log line says for a layout.
fn shows(name: &str, code: &str) -> String {
    format!("layout=Some(Layout {{ name: \"{name}\", code: \"{code}\" }})")
}

fn inside(rect: Rect, surface: Rect) -> bool {
    rect.x >= surface.x
        && rect.y >= surface.y
        && rect.x + rect.width <= surface.x + surface.width
        && rect.y + rect.height <= surface.y + surface.height
}

#[test]
fn a_click_on_the_row_goes_to_the_next_layout_and_the_row_follows_round_and_round()
-> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    bed.logged(&shows("Français", "FR"))?;
    // One row, 14 px above the levels: the surface as tall as one row.
    let s = bed.connections();
    assert_eq!((s.width, s.height), (194, 40));
    let row = bed.row_rect();
    assert!(
        bed.ink(Rect::new(row.x + 30, row.y + 6, 80, 18))? > 20,
        "no name drawn"
    );
    let first = bed.pixels(row)?;
    assert_eq!(bed.hp.layout(), 0);
    // A click: the compositor's keyboard on the next layout, which the
    // window holding the keyboard is given; then the row follows.
    let mark = bed.mark();
    bed.click(bed.row(false));
    bed.group(1)?;
    assert_eq!(bed.hp.layout(), 1);
    bed.logged_after(mark, &shows("Anglais (US)", "US"))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    assert_ne!(bed.pixels(row)?, first, "the row did not change");
    // Again: back to the first, which the row shows as it was.
    let mark = bed.mark();
    bed.click(bed.row(false));
    bed.group(0)?;
    bed.logged_after(mark, &shows("Français", "FR"))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    assert_eq!(bed.pixels(row)?, first);
    // The window kept the keyboard: no popover, nothing of the shell's.
    assert!(bed.hp.popover_surfaces().is_empty());
    assert_eq!(bed.hp.keyboard_layer(), None);
    Ok(())
}

#[test]
fn two_french_layouts_go_by_their_variants_code() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        layouts: &["fr", "fr(bepo)", "ru"],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged(&shows("Français", "FR"))?;
    for (group, name, code) in [
        (1, "Français (BÉPO)", "BÉPO"),
        (2, "Russe", "RU"),
        (0, "Français", "FR"),
    ] {
        let mark = bed.mark();
        bed.click(bed.row(false));
        bed.group(group)?;
        bed.logged_after(mark, &shows(name, code))?;
    }
    Ok(())
}

#[test]
fn in_english_the_names_are_xkbs() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        layouts: &["fr", "fr(bepo)"],
        lang: "en_US.UTF-8",
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged(&shows("French", "FR"))?;
    let mark = bed.mark();
    bed.click(bed.row(false));
    bed.group(1)?;
    bed.logged_after(mark, &shows("French (BEPO)", "BEPO"))?;
    Ok(())
}

#[test]
fn one_layout_no_row_and_no_click_taken() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        layouts: &["fr"],
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged("the keyboard's layouts shown=None")?;
    // No connection at all: the surface lets the pointer through to the
    // bar, and a click there changes nothing.
    let (x, y) = bed.row(false);
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    let through = bed.hp.dispatch_until(WAIT, |hp| {
        hp.pointer_layer()
            .is_some_and(|l| l != hocus_link::CONNECTIONS_NAMESPACE)
    })?;
    assert!(through, "the pointer is on {:?}", bed.hp.pointer_layer());
    bed.click((x, y));
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert_eq!(bed.hp.layout(), 0);
    assert!(!plain(&bed.log).contains("the layout's row shows it layout=Some"));
    // The mini form: no button, the levels' surface as it was.
    chord(&mut bed.hp, &[common::KEY_LEFTMETA, 48]);
    let mini = bed.hp.dispatch_until(WAIT, |hp| {
        hp.levels_surfaces()
            .first()
            .is_some_and(|(_, r)| r.width == 54)
    })?;
    assert!(mini);
    bed.settle()?;
    assert_eq!(bed.levels().height, 128);
    Ok(())
}

#[test]
fn a_group_switch_of_the_users_options_moves_the_row_drawing_it_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        options: "grp:alt_shift_toggle",
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged(&shows("Français", "FR"))?;
    bed.hp.pointer_move(600.0, 500.0);
    assert_eq!(bed.at_rest()?, (0, 0, 0, 0), "at rest");
    // Alt+Shift: the next layout, the row alone drawn (tried again, and
    // the layout switched again, should the clock tick meanwhile).
    let mark = bed.mark();
    let (commits, damage) = bed.drawn_by(true, |bed| {
        chord(&mut bed.hp, &[KEY_LEFTALT, KEY_LEFTSHIFT]);
    })?;
    let group = bed.hp.layout();
    bed.group(group)?;
    let (name, code) = [("Français", "FR"), ("Anglais (US)", "US")][group as usize];
    bed.logged_after(mark, &shows(name, code))?;
    assert_eq!((commits.0, commits.1, commits.3), (0, 0, 0), "{commits:?}");
    assert!(inside(damage, bed.connections()), "{damage:?}");
    assert_eq!(bed.at_rest()?, (0, 0, 0, 0), "at rest again");
    Ok(())
}

#[test]
fn the_mini_forms_button_under_the_battery_draws_alone_and_goes_to_the_next() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        mini: true,
        options: "grp:alt_shift_toggle",
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged(&shows("Français", "FR"))?;
    // The levels' surface grew by the button and the gap over it.
    let l = bed.levels();
    assert_eq!((l.width, l.height), (54, 168));
    // The code drawn in its 44 × 30 button, its bottom 47 px over the
    // bar's.
    let (x, y) = bed.row(true);
    let button = Rect::new(x as i32 - 22, y as i32 - 15, 44, 30);
    assert_eq!(button.y + button.height, 1080 - 8 - 47);
    assert!(bed.ink(button)? > 10, "no code drawn");
    // Hovered, nothing opens; clicked, the next layout.
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    wait(&mut bed.hp, Duration::from_millis(500))?;
    assert!(bed.hp.popover_surfaces().is_empty());
    let mark = bed.mark();
    bed.click((x, y));
    bed.group(1)?;
    bed.logged_after(mark, &shows("Anglais (US)", "US"))?;
    // A switch from the keyboard draws the levels alone.
    bed.hp.pointer_move(600.0, 500.0);
    let mark = bed.mark();
    let (commits, damage) = bed.drawn_by(false, |bed| {
        chord(&mut bed.hp, &[KEY_LEFTALT, KEY_LEFTSHIFT]);
    })?;
    let group = bed.hp.layout();
    bed.group(group)?;
    let (name, code) = [("Français", "FR"), ("Anglais (US)", "US")][group as usize];
    bed.logged_after(mark, &shows(name, code))?;
    assert_eq!((commits.0, commits.2, commits.3), (0, 0, 0), "{commits:?}");
    assert!(inside(damage, bed.levels()), "{damage:?}");
    Ok(())
}

#[test]
fn the_keyboard_reaches_the_row_where_enter_and_space_go_to_the_next() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let setup = Setup {
        wifi: true,
        ..Setup::default()
    };
    let mut bed = start(&shell, setup)?;
    bed.logged(&shows("Français", "FR"))?;
    // The Wi-Fi's row over the layout's.
    let s = bed.connections();
    assert_eq!((s.width, s.height), (194, 72));
    let wifi = (f64::from(s.x + s.width / 2), f64::from(s.y) + 5.0 + 15.0);
    bed.click(wifi);
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.popover_surfaces().is_empty())?;
    assert!(open, "no Wi-Fi popover");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.hp.pointer_move(600.0, 500.0);
    // Esc: the Wi-Fi's row holds the keyboard; Tab: the layout's.
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    assert!(closed);
    bed.press(KEY_TAB);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(
        bed.hp.keyboard_layer().as_deref(),
        Some(hocus_link::CONNECTIONS_NAMESPACE)
    );
    // The ring around the layout's row: the accent's green under it.
    let row = bed.row_rect();
    let ring = bed.pixels(Rect::new(row.x + 40, row.y + row.height + 1, 40, 2))?;
    let green = ring
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[1] > 150 && p[2] < 140 && p[0] < 170)
        .count();
    assert!(green > 20, "no ring: {green}");
    let mark = bed.mark();
    bed.press(KEY_ENTER);
    let next = bed.hp.dispatch_until(WAIT, |hp| hp.layout() == 1)?;
    assert!(next);
    bed.logged_after(mark, &shows("Anglais (US)", "US"))?;
    bed.press(KEY_SPACE);
    let back = bed.hp.dispatch_until(WAIT, |hp| hp.layout() == 0)?;
    assert!(back);
    // Esc lets the keyboard go: the window has it again, with the layout.
    bed.press(KEY_ESC);
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "{:?}", bed.hp.keyboard_layer());
    bed.group(0)?;
    Ok(())
}
