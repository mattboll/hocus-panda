//! The desktops' tiles (docs/design/bar.md §4 group 8, §5; issue #366):
//! the real shell, its levels against the fakes (`levels/fakes.rs`), the
//! compositor's desktops.
//!
//! A tile per desktop, its windows sketched as they open, move and close
//! (a drag told where it lands), the current one ringed in the accent; a
//! click goes to its desktop, the wheel a desktop a notch without wrapping
//! (a fast spin's notches each, on a scaled screen too);
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
//! shows its letter on a colour. A touchpad scrolls it by the logical
//! pixels the fingers moved, at ×1 and ×2 alike (#488).
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
    BTN_LEFT, ClientHandle, Command, KEY_2, KEY_ESC, KEY_H, KEY_LEFTALT, KEY_LEFTMETA,
    KEY_LEFTSHIFT, KEY_TAB, KEY_UP, WAIT, chord, wait,
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
    /// The screen, 1920 × 1080 logical pixels.
    output: &'static str,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            count: 4,
            mini: false,
            output: "1920x1080",
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
    // Either form, on a screen too narrow to choose the full one too.
    config.panel.form = if setup.mini {
        FormChoice::Mini
    } else {
        FormChoice::Full
    };
    let hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![setup.output.parse()?],
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
        hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE).len() == 1 && hp.levels_commits() > 0
    })?;
    anyhow::ensure!(drawn, "no desktops: {:?}", bed.hp.desktops_surfaces());
    bed.logged("the desktops' tiles show it")?;
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    bed.placed_drawn()?;
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

    /// Waits until the desktops' surface shows what was asked of it at
    /// its place: the shell resizes it as it learns how many desktops
    /// there are, and a loaded machine draws the new size late.
    fn placed_drawn(&mut self) -> anyhow::Result<()> {
        let drawn = self.hp.dispatch_until(WAIT, |hp| {
            let placed = hp.desktops_surfaces();
            !placed.is_empty() && hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE) == placed
        })?;
        anyhow::ensure!(
            drawn,
            "the desktops placed at {:?}, drawn at {:?}",
            self.hp.desktops_surfaces(),
            self.hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
        );
        Ok(())
    }

    /// Waits until the desktops' surface is drawn `size` large: the shell
    /// resizes it as it learns how many desktops there are. Where it is.
    fn drawn_sized(&mut self, size: (i32, i32)) -> anyhow::Result<Rect> {
        let sized = |hp: &Headless| {
            hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
                .first()
                .map(|(_, r)| *r)
                .filter(|r| (r.width, r.height) == size)
        };
        let drawn = self.hp.dispatch_until(WAIT, |hp| sized(hp).is_some())?;
        anyhow::ensure!(
            drawn,
            "the desktops drawn at {:?}, not {size:?}",
            self.hp.drawn_layers(hocus_link::DESKTOPS_NAMESPACE)
        );
        sized(&self.hp).ok_or_else(|| anyhow::anyhow!("no desktops"))
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

    /// A click there, once the shell's surfaces there are drawn
    /// ([`common::click`]).
    fn click(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        common::click(&mut self.hp, at)
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

    /// The pixel at (`x`, `y`), global: red, green, blue. Read once the
    /// screen is at rest: a buffer the shell committed shows once the
    /// compositor has drawn a frame since.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        common::at_rest(&mut self.hp)?;
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }

    /// How many pixels of `rect` (global) `test` takes, once the screen is
    /// at rest ([`Bed::pixel`]).
    fn count(&mut self, rect: Rect, test: impl Fn([u8; 3]) -> bool) -> anyhow::Result<usize> {
        common::at_rest(&mut self.hp)?;
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

    /// Waits until the screen shows what `test` looks for: the shell logs
    /// a draw before its buffer comes, and on a loaded machine the
    /// compositor may draw it well after.
    fn shows(
        &mut self,
        what: &str,
        mut test: impl FnMut(&mut Self) -> anyhow::Result<bool>,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        while !test(self)? {
            anyhow::ensure!(
                start.elapsed() < WAIT,
                "the screen never showed {what}; the tiles last drew: {:?}",
                plain(&self.log)
                    .lines()
                    .rev()
                    .find(|l| l.contains("the desktops' tiles show it"))
            );
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        Ok(())
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
    /// surface has drawn. The clock draws on a surface of its own, counted
    /// apart: its ticks change none of these.
    fn drawn_by(&mut self, change: impl FnOnce(&mut Self)) -> anyhow::Result<[u64; 5]> {
        self.settle()?;
        let before = self.counts();
        let mark = self.mark();
        change(self);
        let drawn = self
            .hp
            .dispatch_until(WAIT, |hp| hp.desktops_commits() > before[4])?;
        anyhow::ensure!(drawn, "the desktops did not draw");
        wait(&mut self.hp, Duration::from_millis(500))?;
        let after = self.counts();
        let commits = std::array::from_fn(|i| after[i] - before[i]);
        if commits[..4] != [0; 4] {
            // What the shell said meanwhile, to tell what drew.
            eprintln!("{commits:?}; the shell said:\n{}", self.said_since(mark));
        }
        Ok(commits)
    }

    /// The commits in two seconds at rest.
    fn at_rest(&mut self) -> anyhow::Result<[u64; 5]> {
        self.settle()?;
        let before = self.counts();
        let mark = self.mark();
        wait(&mut self.hp, Duration::from_secs(2))?;
        let after = self.counts();
        let commits = std::array::from_fn(|i| after[i] - before[i]);
        if commits != [0; 5] {
            eprintln!(
                "{commits:?} at rest; the shell said:\n{}",
                self.said_since(mark)
            );
        }
        Ok(commits)
    }

    /// What the shell logged past `mark`.
    fn said_since(&self, mark: usize) -> String {
        plain(&self.log).get(mark..).unwrap_or_default().to_owned()
    }

    /// Opens a window and keeps its client. The shell looks its icon up
    /// off its loop and, found, draws the tiles and the list again: one of
    /// the test's theme is waited for, so that it does not draw in a count
    /// taken after (on a loaded machine, it came after the commits had
    /// settled).
    fn open(
        &mut self,
        app_id: &'static str,
        size: (u32, u32),
    ) -> anyhow::Result<(Rect, ClientHandle)> {
        let opened = common::open_with(&mut self.hp, app_id, size, false)?;
        if RED.contains(&app_id) {
            self.logged(&format!("app={app_id} found=true"))?;
        }
        Ok(opened)
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

    /// The pointer from the tile it rests on to `at`, off the tiles: the
    /// tile is drawn once without its hover, waited for, so that a count
    /// taken after does not catch it late (on a loaded machine, after a
    /// while without commits).
    fn leave_tile_for(&mut self, at: (f64, f64)) -> anyhow::Result<()> {
        let tiles = self.hp.desktops_commits();
        self.hp.pointer_move(at.0, at.1);
        let left = self
            .hp
            .dispatch_until(WAIT, |hp| hp.desktops_commits() > tiles)?;
        anyhow::ensure!(left, "the tile left kept its hover");
        Ok(())
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
    bed.drawn_sized((194, 40))?;
    bed.logged("said=Bureau 1, aucune fenêtre · Bureau 2, aucune fenêtre")?;
    // An empty desktop shows its number.
    let t = bed.tile(0);
    let number = Rect::new(t.x + t.width / 2 - 4, t.y + 9, 8, 12);
    bed.shows("the number", |bed| {
        Ok(bed.count(number, |p| light(p) > 70)? > 3)
    })?;
    // A window opens: its sketch where it is on the screen, the focused
    // one's light.
    let mark = bed.mark();
    let (window, client) = bed.open("test.sketched", (800, 500))?;
    bed.logged_after(mark, "said=Bureau 1, 1 fenêtre · Bureau 2, aucune fenêtre")?;
    bed.settle()?;
    let drawn = bed.sketch(0, window);
    let (x, y) = centre(drawn);
    bed.shows("the sketch", |bed| Ok(light(bed.pixel(x, y)?) > 95))?;
    // Dragged with Alt: nothing told on the way, where it lands at the
    // end.
    let commits = bed.hp.desktops_commits();
    let grab = (f64::from(window.x + 100), f64::from(window.y + 100));
    bed.hp.pointer_move(grab.0, grab.1);
    bed.hp.key(KEY_LEFTALT, true);
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
    bed.hp.key(KEY_LEFTALT, false);
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
    bed.shows("the sketch where it went", |bed| {
        Ok(light(bed.pixel(nx, ny)?) > 95)
    })?;
    let left = (drawn.x + 1, drawn.y + drawn.height / 2);
    assert!(left.0 + 2 < now.x, "{drawn:?} {now:?}");
    assert!(light(bed.pixel(left.0, left.1)?) < 60, "still where it was");
    // Closed: the tile is empty again, its number back.
    let mark = bed.mark();
    client.commands.send(Command::Close)?;
    bed.logged_after(mark, "said=Bureau 1, aucune fenêtre")?;
    bed.shows("the number back", |bed| {
        Ok(bed.count(number, |p| light(p) > 70)? > 3)
    })?;
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
    // Super+2: the ring goes to the second, once the tiles are drawn so.
    let mark = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    bed.on(1)?;
    bed.logged_after(
        mark,
        "the desktops' tiles show it bar=\"HEADLESS-1\" current=2",
    )?;
    bed.settle()?;
    bed.shows("the second ringed", |bed| bed.ringed(1))?;
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
    // Seen a refresh at a time from the shell's word, the motions' clock
    // held: on a loaded machine, the frames drawn as they come may all
    // miss the middle.
    bed.hp.hold_clock();
    let seen = bed.click(third).and_then(|()| {
        let asked = bed.hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 2)?;
        anyhow::ensure!(asked, "never on the third: {:?}", bed.hp.desktop());
        common::sample(&mut bed.hp, Duration::from_secs(1), Headless::desktop)
    });
    bed.hp.release_clock();
    let seen = seen?;
    // The strip slides past the second, as with the shortcuts.
    assert!(
        seen.iter().any(|(_, (_, at))| *at > 0.5 && *at < 1.9),
        "the strip went straight there: {seen:?}"
    );
    bed.on(2)?;
    bed.settle()?;
    bed.shows("the third ringed", |bed| bed.ringed(2))?;
    Ok(())
}

#[test]
fn a_window_let_go_on_a_tile_goes_to_that_desktop() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(&shell, Setup::default())?;
    let (window, _client) = bed.open("test.dropped", (600, 400))?;
    bed.settle()?;
    // Alt+drag onto the second tile (8.5 B): the shell said where the
    // tiles are; it lights the one under the window.
    let mark = bed.mark();
    let (x, y) = (f64::from(window.x + 100), f64::from(window.y + 100));
    let to = bed.middle(1);
    bed.hp.pointer_move(x, y);
    bed.hp.key(KEY_LEFTALT, true);
    bed.hp.button(BTN_LEFT, true);
    for step in 1..=20 {
        let t = f64::from(step) / 20.0;
        bed.hp.pointer_move(x + (to.0 - x) * t, y + (to.1 - y) * t);
        bed.hp.dispatch(Duration::from_millis(16))?;
    }
    let over = bed.hp.dispatch_until(WAIT, |hp| hp.drop_over().is_some())?;
    anyhow::ensure!(over, "never over a tile");
    assert_eq!(
        bed.hp.drop_over(),
        Some(("HEADLESS-1".to_owned(), "HEADLESS-1".to_owned(), 1))
    );
    assert_eq!(bed.hp.bar_in_front().as_deref(), Some("HEADLESS-1"));
    bed.logged_after(
        mark,
        "a window over the tiles bar=\"HEADLESS-1\" over=Some((\"HEADLESS-1\", 1))",
    )?;
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTALT, false);
    // On the second desktop, at its place before the drag; the screen
    // still on the first; the tile no longer lit.
    let gone = bed.hp.dispatch_until(WAIT, |hp| hp.windows().is_empty())?;
    anyhow::ensure!(gone, "still shown: {:?}", bed.hp.windows());
    assert_eq!(bed.hp.desktop().0, 0);
    bed.logged_after(mark, "a window over the tiles bar=\"HEADLESS-1\" over=None")?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    common::wait_for_rect(&mut bed.hp, "test.dropped", window)?;
    Ok(())
}

/// With `HOCUS_SHOTS` naming a folder: `rect` of the screen, as
/// `WHAT.png`.
fn shot(hp: &mut Headless, what: &str, rect: Rect) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    common::at_rest(hp)?;
    let (width, _, bytes) = hp.screenshot(0)?;
    let mut image = image::RgbaImage::new(rect.width as u32, rect.height as u32);
    for y in 0..rect.height {
        for x in 0..rect.width {
            let at = usize::try_from((rect.y + y) * width + rect.x + x)? * 4;
            let p = &bytes[at..at + 4];
            image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
        }
    }
    image.save(Path::new(&folder).join(format!("{what}.png")))?;
    Ok(())
}

/// A right click on a window's line in the bar opens its menu with its
/// top on that line's (#726), as on a laptop: the levels under the tiles
/// and nine desktops' three rows of them lift the lines high enough for
/// the menu to hang from either whole. The first build sent it to the
/// bar's foot whatever the line.
#[test]
fn a_windows_menu_opens_with_its_top_on_its_line() -> anyhow::Result<()> {
    const BTN_RIGHT: u32 = 0x111;
    let Some(shell) = ready() else {
        return Ok(());
    };
    let mut bed = start(
        &shell,
        Setup {
            count: 9,
            ..Setup::default()
        },
    )?;
    let _first = bed.open_titled("test.first", "Premier", (600, 400))?;
    let _second = bed.open_titled("test.second", "Second", (600, 400))?;
    bed.settle()?;
    let ids: Vec<u64> = ["test.second", "test.first"]
        .iter()
        .map(|app| {
            bed.hp
                .windows()
                .into_iter()
                .find(|w| w.app_id.as_deref() == Some(*app))
                .map(|w| w.id.0)
                .ok_or_else(|| anyhow::anyhow!("no {app}"))
        })
        .collect::<anyhow::Result<_>>()?;
    let mut tops = Vec::new();
    for id in ids {
        // The line's middle as the shell said it, in the bar's frame (its
        // right edge, 8 px down).
        let item = format!("Window({id})@");
        let log = bed.log.clone();
        let said = |text: &str| {
            text.lines()
                .rev()
                .find(|l| l.contains("the windows are drawn at rest") && l.contains(&item))
                .and_then(|l| {
                    l.split(&item)
                        .nth(1)?
                        .split_whitespace()
                        .next()
                        .map(str::to_owned)
                })
        };
        let found = bed
            .hp
            .dispatch_until(WAIT, |_| said(&plain(&log)).is_some())?;
        anyhow::ensure!(found, "no line for window {id}");
        let at = said(&plain(&log)).unwrap_or_default();
        let (x, y) = at
            .split_once(',')
            .ok_or_else(|| anyhow::anyhow!("no place in {at:?}"))?;
        let (x, y) = (1920.0 - 8.0 + x.parse::<f64>()?, 8.0 + y.parse::<f64>()?);
        let line = (y - 15.0).round() as i32;
        let mark = bed.mark();
        common::over(&mut bed.hp, (x, y), hocus_link::WINDOWS_NAMESPACE)?;
        bed.hp.button(BTN_RIGHT, true);
        bed.hp.button(BTN_RIGHT, false);
        bed.logged_after(mark, &format!("the window's menu shows it: window={id} "))?;
        let drawn = bed
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(drawn, "no menu drawn");
        let menu = common::drawn_popovers(&bed.hp)[0];
        // Whole on the screen, its top on the line's.
        assert!(menu.y + menu.height <= 1080 - 8, "{menu:?}");
        assert!(
            (menu.y - line).abs() <= 1,
            "the line at {line}, the menu at {menu:?}"
        );
        tops.push(menu.y);
        if tops.len() == 2 {
            shot(
                &mut bed.hp,
                "window-menu-line",
                Rect::new(1920 - 560, 1080 - 700, 560, 700),
            )?;
        }
        chord(&mut bed.hp, &[KEY_ESC]);
        let gone = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(gone, "the menu stays after Esc");
    }
    // The second line's menu lower than the first's, by a line.
    assert_eq!(tops[1] - tops[0], 30, "{tops:?}");
    Ok(())
}

#[test]
fn the_wheel_goes_a_desktop_a_notch_and_stops_at_the_ends() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // On a scaled screen, iced hands the notches multiplied by its scale.
    for output in ["1920x1080", "3840x2160@2"] {
        let mut bed = start(
            &shell,
            Setup {
                output,
                ..Setup::default()
            },
        )?;
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
        // A fast spin, two notches in one event: two desktops.
        bed.hp.wheel(2);
        wait(&mut bed.hp, Duration::from_millis(150))?;
        bed.on(2)?;
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
    // Two rows of 30 px, 6 px apart, once drawn for six: the surface is
    // resized as the shell learns their number.
    bed.logged("Bureau 5, aucune fenêtre · Bureau 6, aucune fenêtre")?;
    bed.drawn_sized((194, 76))?;
    // The fifth under the first, its number drawn; nothing where a
    // seventh would be.
    let fifth = bed.tile(4);
    assert_eq!(fifth.x, bed.tile(0).x);
    let number = Rect::new(fifth.x + fifth.width / 2 - 4, fifth.y + 9, 8, 12);
    bed.shows("the fifth", |bed| {
        Ok(bed.count(number, |p| light(p) > 70)? > 3)
    })?;
    let seventh = bed.tile(6);
    assert_eq!(bed.count(seventh, |p| light(p) > 60)?, 0, "a seventh");
    // The sixth, clicked.
    let sixth = bed.middle(5);
    bed.click(sixth)?;
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
    bed.drawn_sized((54, 40))?;
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
    bed.click(second)?;
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
    bed.click(SOUND_ROW)?;
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
    bed.shows("the ring", |bed| Ok(bed.count(under, green)? > 10))?;
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
    let commits = bed.drawn_by(|bed| chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]))?;
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
    let mark = bed.mark();
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_2]);
    bed.on(1)?;
    // Counted once the tiles show it: on a loaded machine the shell may
    // draw it after a while without commits.
    bed.logged_after(
        mark,
        "the desktops' tiles show it bar=\"HEADLESS-1\" current=2",
    )?;
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
    bed.hp.pointer_move(600.0, 500.0);
    bed.settle()?;
    let sketch = bed.sketch(0, window);
    let (x, y) = centre(sketch);
    let icon = Rect::new(x - 5, y - 5, 10, 10);
    // Drawn once the shell has the icon, which it looks up off its loop:
    // on a loaded machine, after the commits settled.
    let start = Instant::now();
    while bed.count(icon, red)? <= 50 {
        anyhow::ensure!(start.elapsed() < WAIT, "no icon in {sketch:?}");
        wait(&mut bed.hp, Duration::from_millis(50))?;
    }
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
    bed.click(third)?;
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
    bed.click((x, y))?;
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
    bed.leave_tile_for((f64::from(list.x + 120), f64::from(list.y + 20)))?;
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
    assert_eq!(
        after[4],
        before[4],
        "the tiles drew; the shell said:\n{}",
        bed.said_since(mark)
    );
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
    bed.click((f64::from(list.x + 120), f64::from(list.y + 20)))?;
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
fn a_touchpad_scrolls_the_list_by_its_logical_pixels_at_any_scale() -> anyhow::Result<()> {
    let Some(shell) = ready() else {
        return Ok(());
    };
    // On a ×2 screen, iced hands a touchpad's pixels multiplied by its
    // scale.
    for output in ["1920x1080", "2560x1600@2"] {
        let mut bed = start(
            &shell,
            Setup {
                output,
                ..Setup::default()
            },
        )?;
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
        bed.logged_after(mark, "count=10 fenêtres")?;
        let (x, y) = Bed::row(list, 0);
        bed.hp.pointer_move(x, y);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        // A touchpad scrolls down a row and its gap, 34 logical px: the second
        // row is where the first was (two rows at twice that, the end).
        bed.hp.finger_scroll(34.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        bed.click((x, y))?;
        let focused = bed.hp.dispatch_until(WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.focused && w.app_id.as_deref() == Some("test.w9"))
        })?;
        assert!(focused, "on {output}: {:?}", bed.hp.windows());
    }
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
    bed.shows("Alpha's icon", |bed| Ok(bed.count(icon(1), red)? > 100))?;
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
    bed.click((x, y))?;
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
    // On its first row, lit: drawn, then at rest.
    let lists = bed.hp.popover_commits();
    bed.leave_tile_for(Bed::row(list, 0))?;
    let lit = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.popover_commits() > lists)?;
    anyhow::ensure!(lit, "the row under the pointer was not lit");
    assert_eq!(bed.at_rest()?, [0; 5], "at rest");
    assert!(!bed.hp.popover_surfaces().is_empty(), "the list closed");
    Ok(())
}
