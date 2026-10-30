//! The desktops' tiles (docs/design/bar.md §4 group 8, §5; issue #366):
//! the real shell, its levels against the fakes (`levels/fakes.rs`), the
//! compositor's desktops.
//!
//! A tile per desktop, its windows sketched as they open, move and close
//! (a drag told where it lands), the current one ringed in the accent; a
//! click goes to its desktop, the wheel a desktop a notch without wrapping;
//! six desktops on two rows; the mini form's dots in their three looks; the
//! keyboard from the sound's row up to the tiles, where Enter goes there;
//! a desktop's change drawing the desktops' surface alone, nothing at rest.
//!
//! And 3.9b (issue #473): the windows' icons in the sketches with room for
//! them; resting on a tile (or a dot) opens the list of its windows front
//! to back, or says it is empty; a row brings its window; a retitle draws
//! the list alone; nothing at rest while it shows. The icons come from a
//! theme of the test's own (`XDG_DATA_DIRS`), red squares.
//!
//! And 3.9c (issue #479): past 8 windows the list scrolls, by the wheel and
//! to the row the keyboard holds; minimised windows are listed last,
//! greyed, and a click brings one back; an application without an icon
//! shows its letter on a colour.
//!
//! Pixman here, so the shell draws on the CPU, opaque and without motion:
//! as in CI.

mod common;
#[path = "levels/fakes.rs"]
mod fakes;

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, ClientHandle, Command, KEY_2, KEY_ESC, KEY_H, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_TAB,
    KEY_UP, WAIT, chord, wait,
};
use fakes::Fakes;
use hocus_compositor::{Config, FormChoice, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;
/// The sound's row's middle on the 1920 × 1080 screen (three rows and a
/// one-line status line under it).
const SOUND_ROW: (f64, f64) = (1800.0, (1080 - 8 - 41 - 175 + 57) as f64);

/// What a test starts.
struct Setup {
    count: usize,
    mini: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            count: 4,
            mini: false,
        }
    }
}

struct Bed {
    _fakes: Fakes,
    hp: Headless,
    log: PathBuf,
    mini: bool,
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

/// The applications whose icon the test's theme has: a red square.
const RED: [&str; 2] = ["test.red", "test.small"];

/// The test's own icon theme, in place of the system's.
fn icons(dir: &Path) -> anyhow::Result<PathBuf> {
    let data = dir.join("data");
    let apps = data.join("icons/hicolor/scalable/apps");
    std::fs::create_dir_all(&apps)?;
    for app in RED {
        std::fs::write(
            apps.join(format!("{app}.svg")),
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\">\
             <rect width=\"16\" height=\"16\" fill=\"#ff0000\"/></svg>",
        )?;
    }
    Ok(data)
}

fn start(shell: &Path, setup: Setup) -> anyhow::Result<Bed> {
    let fakes = Fakes::start()?;
    let log = fakes.dir.path().join("shell.log");
    let data = icons(fakes.dir.path())?.display().to_string();
    let env = [
        ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
        ("XDG_DATA_HOME", data.clone()),
        ("XDG_DATA_DIRS", data),
    ];
    let script = fakes.shell_with(shell, "fr_FR.UTF-8", &env, Some(&log))?;
    let mut config = Config::default();
    config.desktops.count = setup.count;
    if setup.mini {
        config.panel.form = FormChoice::Mini;
    }
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let mut bed = Bed {
        _fakes: fakes,
        hp,
        log,
        mini: setup.mini,
    };
    let drawn = bed.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.desktops_surfaces().len() == 1 && hp.desktops_commits() > 0 && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no desktops: {:?}", bed.hp.desktops_surfaces());
    bed.logged("the desktops' tiles show it")?;
    bed.hp.pointer_move(600.0, 500.0);
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

    /// The commits of the bar's, the levels', the connections', the
    /// popovers' and the desktops' surfaces.
    fn counts(&self) -> [u64; 5] {
        [
            self.hp.bar_commits(),
            self.hp.levels_commits(),
            self.hp.connections_commits(),
            self.hp.popover_commits(),
            self.hp.desktops_commits(),
        ]
    }

    fn surface(&self) -> Rect {
        self.hp
            .desktops_surfaces()
            .first()
            .map_or(Rect::new(0, 0, 0, 0), |(_, r)| *r)
    }

    /// Desktop `index`'s tile (or dot), global: four tiles 38.5 px wide to
    /// a row, 6 px apart, in the full form; two 18 × 13 dots to a row, 4 px
    /// apart, in the mini; 5 px in from the surface's top and side.
    fn tile(&self, index: usize) -> Rect {
        let s = self.surface();
        if self.mini {
            let (row, column) = (index / 2, index % 2);
            return Rect::new(
                s.x + 7 + 22 * column as i32,
                s.y + 5 + 17 * row as i32,
                18,
                13,
            );
        }
        let (row, column) = (index / 4, index % 4);
        let from = f64::from(s.x) + 11.0 + 44.5 * column as f64;
        let (x0, x1) = (from.round() as i32, (from + 38.5).round() as i32);
        Rect::new(x0, s.y + 5 + 36 * row as i32, x1 - x0, 30)
    }

    fn middle(&self, index: usize) -> (f64, f64) {
        let t = self.tile(index);
        (
            f64::from(t.x) + f64::from(t.width) / 2.0,
            f64::from(t.y) + f64::from(t.height) / 2.0,
        )
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

    /// Waits until the screen shows desktop `index`, still.
    fn on(&mut self, index: usize) -> anyhow::Result<()> {
        let still = self.hp.dispatch_until(WAIT, |hp| {
            let (current, at) = hp.desktop();
            current == index && (at - index as f64).abs() < 1e-9
        })?;
        anyhow::ensure!(still, "on {:?}, not {index}", self.hp.desktop());
        Ok(())
    }

    /// Waits until the shell's log has a line with `text`, past `from`
    /// bytes of it.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<()> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        let lines: Vec<String> = plain(&log)
            .get(from..)
            .unwrap_or_default()
            .lines()
            .filter(|l| l.contains("desktop") || l.contains("popover") || l.contains("icon"))
            .map(str::to_owned)
            .collect();
        anyhow::ensure!(
            done,
            "the shell never logged {text:?}; it logged:\n{}",
            lines.join("\n")
        );
        Ok(())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<()> {
        self.logged_after(0, text)
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
    }

    /// The pixel at (`x`, `y`), global: red, green, blue.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }

    /// How many pixels of `rect` (global) `test` takes.
    fn count(&mut self, rect: Rect, test: impl Fn([u8; 3]) -> bool) -> anyhow::Result<usize> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let mut n = 0;
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let at = usize::try_from(y * width + x)? * 4;
                let p = &bytes[at..at + 4];
                n += usize::from(test([p[2], p[1], p[0]]));
            }
        }
        Ok(n)
    }

    /// The accent's green along the top of desktop `index`'s tile, where
    /// its ring is.
    fn ringed(&mut self, index: usize) -> anyhow::Result<bool> {
        let t = self.tile(index);
        let band = Rect::new(t.x + 8, t.y, t.width - 16, 3);
        Ok(self.count(band, green)? > band.width as usize)
    }

    /// A window's sketch in desktop `index`'s tile, from its place on the
    /// 1920 × 1080 screen: 4 px inside the tile.
    fn sketch(&self, index: usize, window: Rect) -> Rect {
        let t = self.tile(index);
        let (w, h) = (f64::from(t.width - 8), f64::from(t.height - 8));
        let x = |v: i32| (f64::from(t.x + 4) + f64::from(v) / 1920.0 * w).round() as i32;
        let y = |v: i32| (f64::from(t.y + 4) + f64::from(v) / 1080.0 * h).round() as i32;
        Rect::new(
            x(window.x),
            y(window.y),
            x(window.x + window.width) - x(window.x),
            y(window.y + window.height) - y(window.y),
        )
    }

    /// The commits a change made by `change` brings once the desktops'
    /// surface has drawn: tried again while the clock ticks through it.
    fn drawn_by(&mut self, mut change: impl FnMut(&mut Self)) -> anyhow::Result<[u64; 5]> {
        for _ in 0..5 {
            self.settle()?;
            let clocks = self.hp.clock_commits();
            wait(&mut self.hp, Duration::from_millis(100))?;
            let before = self.counts();
            change(self);
            let drawn = self
                .hp
                .dispatch_until(WAIT, |hp| hp.desktops_commits() > before[4])?;
            anyhow::ensure!(drawn, "the desktops did not draw");
            wait(&mut self.hp, Duration::from_millis(500))?;
            let after = self.counts();
            if self.hp.clock_commits() == clocks {
                return Ok(std::array::from_fn(|i| after[i] - before[i]));
            }
        }
        anyhow::bail!("the clock ticked through every try")
    }

    /// The commits in two seconds at rest.
    fn at_rest(&mut self) -> anyhow::Result<[u64; 5]> {
        self.settle()?;
        let before = self.counts();
        wait(&mut self.hp, Duration::from_secs(2))?;
        let after = self.counts();
        Ok(std::array::from_fn(|i| after[i] - before[i]))
    }

    /// Opens a window and keeps its client.
    fn open(
        &mut self,
        app_id: &'static str,
        size: (u32, u32),
    ) -> anyhow::Result<(Rect, ClientHandle)> {
        common::open_with(&mut self.hp, app_id, size, false)
    }

    /// Opens a window titled `title`.
    fn open_titled(
        &mut self,
        app_id: &'static str,
        title: &'static str,
        size: (u32, u32),
    ) -> anyhow::Result<ClientHandle> {
        let (_, client) = self.open(app_id, size)?;
        client.commands.send(Command::Title(title))?;
        Ok(client)
    }

    /// The pointer comes to rest on desktop `index`'s tile.
    fn rest_on(&mut self, index: usize) {
        let (x, y) = self.middle(index);
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
    }

    /// The list, once it shows.
    fn list(&mut self) -> anyhow::Result<Rect> {
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            hp.popover_surfaces()
                .first()
                .is_some_and(|(_, r)| r.height > 0)
                && hp.popover_commits() > 0
        })?;
        anyhow::ensure!(shown, "no list");
        Ok(self.hp.popover_surfaces()[0].1)
    }

    /// Row `i` of the list at `list`: its middle.
    fn row(list: Rect, i: usize) -> (f64, f64) {
        (
            f64::from(list.x + 120),
            f64::from(list.y + 41 + 34 * i as i32 + 16),
        )
    }
}

/// The accent, `#6CC287`, more or less blended.
fn green([r, g, b]: [u8; 3]) -> bool {
    g > 140 && r < 130 && b < 160 && g > r + 40
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

/// The light of a pixel: its darkest channel.
fn light([r, g, b]: [u8; 3]) -> u8 {
    r.min(g).min(b)
}

fn centre(r: Rect) -> (i32, i32) {
    (r.x + r.width / 2, r.y + r.height / 2)
}

#[test]
fn the_tiles_sketch_their_windows_as_they_open_move_and_close() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    // Four tiles in a row, 14 px above the levels (no connection here).
    let s = bed.surface();
    assert_eq!((s.width, s.height), (194, 40));
    bed.logged("said=Bureau 1, aucune fenêtre · Bureau 2, aucune fenêtre")?;
    // An empty desktop shows its number.
    let t = bed.tile(0);
    let number = Rect::new(t.x + t.width / 2 - 4, t.y + 9, 8, 12);
    assert!(bed.count(number, |p| light(p) > 70)? > 3, "no number");
    // A window opens: its sketch where it is on the screen, the focused
    // one's light.
    let mark = bed.mark();
    let (window, client) = bed.open("test.sketched", (800, 500))?;
    bed.logged_after(mark, "said=Bureau 1, 1 fenêtre · Bureau 2, aucune fenêtre")?;
    bed.settle()?;
    let drawn = bed.sketch(0, window);
    let (x, y) = centre(drawn);
    assert!(light(bed.pixel(x, y)?) > 95, "{:?}", bed.pixel(x, y)?);
    // Dragged with Super: nothing told on the way, where it lands at the
    // end.
    let commits = bed.hp.desktops_commits();
    let grab = (f64::from(window.x + 100), f64::from(window.y + 100));
    bed.hp.pointer_move(grab.0, grab.1);
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.button(BTN_LEFT, true);
    for step in 1..=40 {
        bed.hp.pointer_move(
            grab.0 + 10.0 * f64::from(step),
            grab.1 + 5.0 * f64::from(step),
        );
        bed.hp.dispatch(Duration::from_millis(16))?;
    }
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.desktops_commits(), commits, "drawn while dragged");
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    let moved = common::rect_of(&bed.hp, "test.sketched").expect("the window");
    assert!(
        moved.x > window.x + 300 && moved.y > window.y + 150,
        "{moved:?}"
    );
    let told = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.desktops_commits() > commits)?;
    assert!(told, "not told where it landed");
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let now = bed.sketch(0, moved);
    let (nx, ny) = centre(now);
    assert!(light(bed.pixel(nx, ny)?) > 95, "not where it went");
    let left = (drawn.x + 1, drawn.y + drawn.height / 2);
    assert!(left.0 + 2 < now.x, "{drawn:?} {now:?}");
    assert!(light(bed.pixel(left.0, left.1)?) < 60, "still where it was");
    // Closed: the tile is empty again, its number back.
    let mark = bed.mark();
    client.commands.send(Command::Close)?;
    bed.logged_after(mark, "said=Bureau 1, aucune fenêtre")?;
    bed.settle()?;
    assert!(bed.count(number, |p| light(p) > 70)? > 3, "no number");
    Ok(())
}

#[test]
fn the_current_desktop_is_ringed_in_the_accent() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    assert!(bed.ringed(0)?, "the first not ringed");
    assert!(!bed.ringed(1)?);
    // Super+2: the ring goes to the second.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    bed.on(1)?;
    bed.settle()?;
    assert!(bed.ringed(1)?, "the second not ringed");
    assert!(!bed.ringed(0)?, "the first still ringed");
    Ok(())
}

#[test]
fn a_click_on_a_tile_slides_to_its_desktop() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let third = bed.middle(2);
    bed.click(third);
    // The strip slides past the second, as with the shortcuts.
    let passing = bed.hp.dispatch_until(WAIT, |hp| {
        let (_, at) = hp.desktop();
        at > 0.5 && at < 1.9
    })?;
    assert!(passing, "the strip went straight to {:?}", bed.hp.desktop());
    bed.on(2)?;
    bed.settle()?;
    assert!(bed.ringed(2)?);
    Ok(())
}

#[test]
fn the_wheel_goes_a_desktop_a_notch_and_stops_at_the_ends() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let (x, y) = bed.middle(1);
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    wait(&mut bed.hp, Duration::from_millis(200))?;
    // Down, a notch at a time: the next, up to the last, no further.
    for expected in [1, 2, 3, 3, 3] {
        bed.hp.wheel(1);
        wait(&mut bed.hp, Duration::from_millis(150))?;
        bed.on(expected)?;
    }
    // Up: back to the first, no further.
    for expected in [2, 1, 0, 0] {
        bed.hp.wheel(-1);
        wait(&mut bed.hp, Duration::from_millis(150))?;
        bed.on(expected)?;
    }
    Ok(())
}

#[test]
fn six_desktops_take_two_rows() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        Setup {
            count: 6,
            ..Setup::default()
        },
    )?;
    // Two rows of 30 px, 6 px apart.
    let s = bed.surface();
    assert_eq!((s.width, s.height), (194, 76));
    bed.logged("Bureau 5, aucune fenêtre · Bureau 6, aucune fenêtre")?;
    // The fifth under the first, its number drawn; nothing where a
    // seventh would be.
    let fifth = bed.tile(4);
    assert_eq!(fifth.x, bed.tile(0).x);
    let number = Rect::new(fifth.x + fifth.width / 2 - 4, fifth.y + 9, 8, 12);
    assert!(bed.count(number, |p| light(p) > 70)? > 3, "no fifth");
    let seventh = bed.tile(6);
    assert_eq!(bed.count(seventh, |p| light(p) > 60)?, 0, "a seventh");
    // The sixth, clicked.
    let sixth = bed.middle(5);
    bed.click(sixth);
    bed.on(5)?;
    Ok(())
}

#[test]
fn the_mini_forms_dots_in_their_three_looks_and_a_click() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        Setup {
            mini: true,
            ..Setup::default()
        },
    )?;
    // Two to a row: 54 × 40.
    let s = bed.surface();
    assert_eq!((s.width, s.height), (54, 40));
    // A window, sent to the second desktop.
    let (_, _client) = bed.open("test.sent", (400, 300))?;
    let mark = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    bed.logged_after(mark, "said=Bureau 1, aucune fenêtre · Bureau 2, 1 fenêtre")?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    // The current one in the accent; one with a window light; an empty
    // one faint, its rim lighter than its middle.
    let look = |bed: &mut Bed, index: usize| -> anyhow::Result<([u8; 3], [u8; 3])> {
        let (x, y) = centre(bed.tile(index));
        let t = bed.tile(index);
        Ok((bed.pixel(x, y)?, bed.pixel(x, t.y)?))
    };
    let (current, _) = look(&mut bed, 0)?;
    assert!(green(current), "{current:?}");
    let (full, _) = look(&mut bed, 1)?;
    let (empty, rim) = look(&mut bed, 2)?;
    assert!(!green(full) && light(full) > 70, "{full:?}");
    assert!(
        light(empty) < 60 && light(empty) + 25 < light(full),
        "{empty:?}"
    );
    assert!(light(rim) > light(empty) + 10, "{rim:?} {empty:?}");
    assert_eq!(look(&mut bed, 3)?, (empty, rim));
    // A click on the second: there.
    let second = bed.middle(1);
    bed.click(second);
    bed.on(1)?;
    Ok(())
}

#[test]
fn the_keyboard_reaches_the_tiles_where_enter_and_space_go_there() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    // The sound's popover, then Esc: its row holds the keyboard.
    bed.click(SOUND_ROW);
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| !hp.popover_surfaces().is_empty())?;
    assert!(open, "no popover");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    assert!(closed);
    // Shift+Tab: up to the last tile, on the desktops' surface.
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_TAB]);
    let held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::DESKTOPS_NAMESPACE)
    })?;
    assert!(held, "the keyboard is on {:?}", bed.hp.keyboard_layer());
    bed.settle()?;
    // Its ring, around the tile.
    let t = bed.tile(3);
    let under = Rect::new(t.x + 8, t.y + t.height + 2, t.width - 16, 2);
    assert!(bed.count(under, green)? > 10, "no ring");
    // Enter: there; Shift+Tab, then Space: the third.
    bed.press(KEY_ENTER);
    bed.on(3)?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_TAB]);
    bed.press(KEY_SPACE);
    bed.on(2)?;
    // The keyboard stays on the tiles; Esc lets it go.
    assert_eq!(
        bed.hp.keyboard_layer().as_deref(),
        Some(hocus_link::DESKTOPS_NAMESPACE)
    );
    bed.press(KEY_ESC);
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.keyboard_layer().is_none())?;
    assert!(gone, "{:?}", bed.hp.keyboard_layer());
    Ok(())
}

#[test]
fn a_desktop_change_draws_the_desktops_surface_alone() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let mut to = 1;
    let commits = bed.drawn_by(|bed| {
        chord(&mut bed.hp, &[KEY_LEFTMETA, common::KEY_1 + to]);
        to = 1 - to;
    })?;
    assert_eq!(commits[..4], [0, 0, 0, 0], "{commits:?}");
    assert!(commits[4] >= 1, "{commits:?}");
    Ok(())
}

#[test]
fn nothing_draws_at_rest() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let (_, _client) = bed.open("test.resting", (600, 400))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    bed.on(1)?;
    bed.hp.pointer_move(600.0, 500.0);
    assert_eq!(bed.at_rest()?, [0; 5], "at rest");
    Ok(())
}

/// The test theme's red, more or less blended.
fn red([r, g, b]: [u8; 3]) -> bool {
    r > 160 && g < 90 && b < 90
}

#[test]
fn a_sketch_with_room_for_it_shows_its_windows_icon() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    // A small window, sent to the second desktop: 300 × 200 px is 5 × 4 px
    // in a tile, no room for its icon.
    let (_, _small) = bed.open("test.small", (300, 200))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    // A large one on the first: 16 × 14 px, its icon in its middle.
    let (window, _large) = bed.open("test.red", (1000, 700))?;
    bed.logged("app=test.red found=true")?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let sketch = bed.sketch(0, window);
    let (x, y) = centre(sketch);
    let icon = Rect::new(x - 5, y - 5, 10, 10);
    assert!(bed.count(icon, red)? > 50, "no icon in {sketch:?}");
    let second = bed.tile(1);
    assert_eq!(bed.count(second, red)?, 0, "an icon in a 5 px sketch");
    Ok(())
}

#[test]
fn resting_on_a_tile_lists_its_windows_front_to_back_after_150_ms() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let _alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    let _beta = bed.open_titled("test.small", "Beta", (600, 400))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    // Passing over a tile shows nothing; resting on it 150 ms does.
    let mark = bed.mark();
    bed.rest_on(0);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    assert!(bed.hp.popover_surfaces().is_empty(), "shown before 150 ms");
    let list = bed.list()?;
    bed.logged_after(
        mark,
        "title=Bureau 1 count=2 fenêtres empty=None rows=[\"Beta\", \"Alpha\"]",
    )?;
    // 240 px wide, 8 px left of the bar, its top on the tiles'.
    assert_eq!(
        (list.width, list.height),
        (240, 1 + 10 + 26 + 4 + 66 + 12 + 1)
    );
    assert_eq!(list.x + list.width, 1920 - 8 - 196 - 8);
    assert_eq!(list.y, bed.tile(0).y);
    // The pointer goes away: it closes; a click on a tile goes there and
    // opens nothing.
    bed.hp.pointer_move(600.0, 500.0);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    assert!(closed, "the list stays");
    let third = bed.middle(2);
    bed.click(third);
    bed.on(2)?;
    wait(&mut bed.hp, Duration::from_millis(400))?;
    assert!(bed.hp.popover_surfaces().is_empty(), "a list after a click");
    Ok(())
}

#[test]
fn an_empty_desktops_list_says_so() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let mark = bed.mark();
    bed.rest_on(2);
    let list = bed.list()?;
    bed.logged_after(
        mark,
        "title=Bureau 3 count= empty=Some(\"Aucune fenêtre.\") rows=[]",
    )?;
    assert_eq!(list.height, 80);
    Ok(())
}

#[test]
fn a_row_goes_to_its_desktop_and_focuses_its_window() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    // Two windows sent to the second desktop, one left on the first.
    let _alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    let _beta = bed.open_titled("test.small", "Beta", (600, 400))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    let (_, _here) = bed.open("test.here", (500, 300))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let mark = bed.mark();
    bed.rest_on(1);
    let list = bed.list()?;
    bed.logged_after(mark, "rows=[\"Beta\", \"Alpha\"]")?;
    // Into the list, on its second row, Alpha's: a click.
    let (x, y) = Bed::row(list, 1);
    bed.click((x, y));
    bed.on(1)?;
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.focused && w.app_id.as_deref() == Some("test.red"))
    })?;
    assert!(focused, "{:?}", bed.hp.windows());
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    assert!(closed, "the list stays");
    Ok(())
}

#[test]
fn a_retitle_draws_the_open_list_and_not_the_tiles() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let mark = bed.mark();
    bed.rest_on(0);
    let list = bed.list()?;
    bed.logged_after(mark, "rows=[\"Alpha\"]")?;
    // The pointer rests in the list, off its row.
    bed.hp
        .pointer_move(f64::from(list.x + 120), f64::from(list.y + 20));
    bed.settle()?;
    let before = bed.counts();
    let mark = bed.mark();
    // Retitled, without drawing: the list says it, the tiles do not draw.
    alpha.commands.send(Command::Title(
        "Gamma — a title long enough to be cut short in the list",
    ))?;
    bed.logged_after(mark, "rows=[\"Gamma — a title")?;
    bed.logged_after(mark, "…\"]")?;
    bed.settle()?;
    let after = bed.counts();
    assert!(after[3] > before[3], "the list did not draw");
    assert_eq!(after[4], before[4], "the tiles drew");
    assert_eq!(after[..3], before[..3], "{before:?} {after:?}");
    Ok(())
}

#[test]
fn resting_on_a_mini_dot_lists_its_windows() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        Setup {
            mini: true,
            ..Setup::default()
        },
    )?;
    let _alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let mark = bed.mark();
    bed.rest_on(0);
    let list = bed.list()?;
    bed.logged_after(
        mark,
        "title=Bureau 1 count=1 fenêtre empty=None rows=[\"Alpha\"]",
    )?;
    // 8 px left of the 56 px bar, its top on the dots'.
    assert_eq!(list.x + list.width, 1920 - 8 - 56 - 8);
    assert_eq!(list.y, bed.tile(0).y);
    Ok(())
}

/// The list's rows' viewport at `list`, 8 rows of 32 px 2 px apart under
/// its header, and whether its scrollbar's thumb reaches its bottom (the
/// list scrolled to its end): lighter than the rim's padding beside it.
fn thumb_at_bottom(bed: &mut Bed, list: Rect) -> anyhow::Result<bool> {
    let y = list.y + 41 + 8 * 34 - 2 - 3;
    let light = |[r, g, b]: [u8; 3]| u32::from(r) + u32::from(g) + u32::from(b);
    let thumb = light(bed.pixel(list.x + 230, y)?);
    let beside = light(bed.pixel(list.x + 236, y)?);
    Ok(thumb > beside + 30)
}

#[test]
fn past_eight_windows_the_list_scrolls_by_the_wheel_and_to_the_keyboards_row() -> anyhow::Result<()>
{
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let windows = [
        ("test.w1", "One"),
        ("test.w2", "Two"),
        ("test.w3", "Three"),
        ("test.w4", "Four"),
        ("test.w5", "Five"),
        ("test.w6", "Six"),
        ("test.w7", "Seven"),
        ("test.w8", "Eight"),
        ("test.w9", "Nine"),
        ("test.w10", "Ten"),
    ];
    let mut clients = Vec::new();
    for (app, title) in windows {
        clients.push(bed.open_titled(app, title, (400, 300))?);
    }
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let mark = bed.mark();
    bed.rest_on(0);
    let list = bed.list()?;
    bed.logged_after(
        mark,
        "count=10 fenêtres empty=None rows=[\"Ten\", \"Nine\", \"Eight\", \"Seven\", \
         \"Six\", \"Five\", \"Four\", \"Three\", \"Two\", \"One\"]",
    )?;
    // 8 rows show (#479).
    assert_eq!(list.height, 1 + 10 + 26 + 4 + 8 * 32 + 7 * 2 + 12 + 1);
    let (x, y) = Bed::row(list, 0);
    bed.hp.pointer_move(x, y);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(
        !thumb_at_bottom(&mut bed, list)?,
        "scrolled before the wheel"
    );
    // A notch: two rows, the end.
    bed.hp.wheel(1);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(thumb_at_bottom(&mut bed, list)?, "the wheel did not scroll");
    // Clicked into, off the rows: the keyboard. Tab holds the first row,
    // which scrolls back into view; Up, round to the last, scrolls to it.
    bed.click((f64::from(list.x + 120), f64::from(list.y + 20)));
    let held = bed.hp.dispatch_until(WAIT, |hp| {
        hp.keyboard_layer().as_deref() == Some(hocus_link::POPOVER_NAMESPACE)
    })?;
    assert!(held, "the keyboard is on {:?}", bed.hp.keyboard_layer());
    bed.press(KEY_TAB);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(
        !thumb_at_bottom(&mut bed, list)?,
        "the first row is out of view"
    );
    bed.press(KEY_UP);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert!(
        thumb_at_bottom(&mut bed, list)?,
        "the last row is out of view"
    );
    // Enter: the last one, the window at the back.
    bed.press(KEY_ENTER);
    let focused = bed.hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.focused && w.app_id.as_deref() == Some("test.w1"))
    })?;
    assert!(focused, "{:?}", bed.hp.windows());
    Ok(())
}

#[test]
fn a_minimised_window_is_listed_last_greyed_and_a_click_brings_it_back() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let focus = |hp: &mut Headless, app: &str| {
        hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.focused && w.app_id.as_deref() == Some(app))
        })
    };
    let _alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    // Beta, focused as it opens, minimised.
    let _beta = bed.open_titled("test.small", "Beta", (600, 400))?;
    anyhow::ensure!(focus(&mut bed.hp, "test.small")?, "Beta not focused");
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_H]);
    let gone = bed.hp.dispatch_until(WAIT, |hp| {
        !hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("test.small"))
    })?;
    assert!(gone, "Beta still shows");
    // An application the theme has no icon for: its letter on a colour.
    let _gamma = bed.open_titled("test.lettered", "Gamma", (500, 300))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let mark = bed.mark();
    bed.rest_on(0);
    let list = bed.list()?;
    // After the others, front to back; counted.
    bed.logged_after(
        mark,
        "count=3 fenêtres empty=None rows=[\"Gamma\", \"Alpha\", \"Beta\"] minimized=1",
    )?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // Its icon at half: the theme's red no longer red; its title fainter.
    let icon = |i: usize| {
        let (_, y) = Bed::row(list, i);
        Rect::new(list.x + 13, y as i32 - 8, 16, 16)
    };
    assert!(bed.count(icon(1), red)? > 100, "Alpha's icon");
    assert_eq!(bed.count(icon(2), red)?, 0, "Beta's icon at full");
    let brightest = |bed: &mut Bed, i: usize| -> anyhow::Result<u32> {
        let (_, y) = Bed::row(list, i);
        let band = Rect::new(list.x + 38, y as i32 - 5, 60, 10);
        let mut most = 0;
        for y in band.y..band.y + band.height {
            for x in band.x..band.x + band.width {
                let [r, g, b] = bed.pixel(x, y)?;
                most = most.max(u32::from(r) + u32::from(g) + u32::from(b));
            }
        }
        Ok(most)
    };
    let (alpha, beta) = (brightest(&mut bed, 1)?, brightest(&mut bed, 2)?);
    assert!(
        beta + 150 < alpha,
        "Beta's title not greyed: {beta}, Alpha's {alpha}"
    );
    // Gamma's letter tile: light pixels in its icon's place.
    let lettered = bed.count(icon(0), |[r, g, b]| {
        u32::from(r) + u32::from(g) + u32::from(b) > 400
    })?;
    assert!(lettered > 100, "no letter tile: {lettered}");
    // A click on Beta: back, focused, the list gone.
    let (x, y) = Bed::row(list, 2);
    bed.click((x, y));
    anyhow::ensure!(focus(&mut bed.hp, "test.small")?, "{:?}", bed.hp.windows());
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
    assert!(closed, "the list stays");
    Ok(())
}

#[test]
fn nothing_draws_at_rest_while_the_list_shows() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let _alpha = bed.open_titled("test.red", "Alpha", (800, 500))?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    bed.rest_on(0);
    let list = bed.list()?;
    let (x, y) = Bed::row(list, 0);
    bed.hp.pointer_move(x, y);
    assert_eq!(bed.at_rest()?, [0; 5], "at rest");
    assert!(!bed.hp.popover_surfaces().is_empty(), "the list closed");
    Ok(())
}
