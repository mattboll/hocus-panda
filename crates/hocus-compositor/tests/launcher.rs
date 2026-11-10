//! The launcher (5.1, issue #391): the real shell, its applications and
//! their icons from a data directory of the test's own (`XDG_DATA_HOME`,
//! `XDG_DATA_DIRS`), its launch counts in a state directory of its own,
//! never the machine's.
//!
//! One scenario per point of the approved spec: `Super+Space` opens it
//! full screen holding the keyboard, `Escape`, `Super+Space` again and a
//! click outside the field and the tiles close it; the empty field shows
//! the frequent row, then all the applications from A to Z (those hidden
//! left out); typing filters the grid, the best match first and selected,
//! accents ignored, and says when nothing matches; the arrows move the
//! selection and `Enter` starts it, its field codes taken out, and counts
//! it; the wheel scrolls by rows; a long name ends with "…"; an entry
//! installed while it runs shows up; an application without an icon shows
//! its letter on a neutral square.
//!
//! Then one per point of the 5.2 spec (issue #392): a calculation shows in
//! a card, `Enter` copies it and a toast says so; a conversion; an
//! address opens; the web search comes last, and `Ctrl+Enter` runs it
//! whatever is selected; an answer lists no application. The browser is a program of the test's, the shell's only
//! `xdg-open` (its `PATH` is the test's own directory).
//!
//! Then the launcher larger (issue #518): the sizes × 1.5 throughout, and
//! fewer columns on a screen too narrow for eight.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{
    BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_RIGHT, KEY_UP,
    WAIT, chord, wait,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_SPACE: u32 = 57;
const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;

/// A US keyboard's letters, by their evdev codes.
fn letter(c: char) -> Option<u32> {
    Some(match c {
        'a' => 30,
        'b' => 48,
        'c' => 46,
        'd' => 32,
        'e' => 18,
        'f' => 33,
        'g' => 34,
        'h' => 35,
        'i' => 23,
        'k' => 37,
        'l' => 38,
        'm' => 50,
        'n' => 49,
        'o' => 24,
        'r' => 19,
        's' => 31,
        't' => 20,
        'u' => 22,
        'v' => 47,
        'x' => 45,
        'z' => 44,
        ' ' => KEY_SPACE,
        _ => return None,
    })
}

/// The key typing `c` on a US keyboard, and whether with Shift.
fn key_of(c: char) -> Option<(u32, bool)> {
    if let Some(code) = letter(c) {
        return Some((code, false));
    }
    Some(match c {
        '1'..='9' => (2 + u32::from(c) - u32::from('1'), false),
        '0' => (11, false),
        '-' => (12, false),
        '+' => (13, true),
        '*' => (9, true),
        '(' => (10, true),
        ')' => (11, true),
        '.' => (52, false),
        ',' => (51, false),
        ':' => (39, true),
        _ => return None,
    })
}

/// The test's applications: id, name, generic name, icon (none: no
/// `Icon` key), and what else their entry says.
const APPS: &[(&str, &str, &str, Option<&str>, &str)] = &[
    (
        "org.mozilla.firefox",
        "Firefox",
        "Navigateur web",
        Some("red"),
        "",
    ),
    ("chromium", "Chromium", "Navigateur web", Some("blue"), ""),
    ("kitty", "kitty", "Terminal", Some("green"), ""),
    (
        "org.gnome.Nautilus",
        "Fichiers",
        "Gestionnaire de fichiers",
        Some("blue"),
        "",
    ),
    (
        "org.gnome.TextEditor",
        "Éditeur de texte",
        "Éditeur",
        Some("green"),
        "",
    ),
    (
        "org.gnome.Settings",
        "Paramètres",
        "Réglages",
        Some("blue"),
        "",
    ),
    ("thunderbird", "Thunderbird", "Courriel", Some("red"), ""),
    ("obsidian", "Obsidian", "Notes", Some("absent-icon"), ""),
    ("calc", "Calculatrice", "", None, ""),
    (
        "update-manager",
        "Gestionnaire de mises à jour du système",
        "Mises à jour",
        Some("green"),
        "",
    ),
    ("hidden-one", "Caché", "", Some("red"), "NoDisplay=true\n"),
    (
        "gnome-only",
        "Seulement GNOME",
        "",
        Some("red"),
        "OnlyShowIn=GNOME;\n",
    ),
];

/// Launch counts the test starts with.
const COUNTS: &str = "120\tkitty\n90\torg.mozilla.firefox\n20\tthunderbird\n";

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    log: PathBuf,
}

/// A square SVG icon filled with `colour`.
fn svg(colour: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"48\" height=\"48\">\
         <rect width=\"48\" height=\"48\" fill=\"{colour}\"/></svg>"
    )
}

/// An entry starting the test's program, which writes down its arguments.
fn entry(
    dir: &Path,
    (id, name, generic, icon, extra): (&str, &str, &str, Option<&str>, &str),
) -> String {
    let mut text = format!(
        "[Desktop Entry]\nType=Application\nName={name}\nExec={} {id} %U --name=%c\n",
        dir.join("bin/app").display()
    );
    if !generic.is_empty() {
        text.push_str(&format!("GenericName={generic}\n"));
    }
    if let Some(icon) = icon {
        text.push_str(&format!("Icon={icon}\n"));
    }
    text.push_str(extra);
    text
}

impl Bed {
    fn start(extra_apps: usize) -> anyhow::Result<Option<Self>> {
        Self::start_on(extra_apps, "1920x1080")
    }

    /// On a screen `output` (`WxH[@SCALE]`).
    fn start_on(extra_apps: usize, output: &str) -> anyhow::Result<Option<Self>> {
        let shell = common::shell_program();
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in [
            "home",
            "config",
            "cache",
            "state/hocuspanda",
            "bin",
            "launched",
        ] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        // The applications' program: its arguments, one a line, in a file
        // named after its first.
        let app = root.join("bin/app");
        std::fs::write(
            &app,
            format!(
                "#!/bin/sh\nid=\"$1\"\nshift\nprintf '%s\\n' \"$@\" > '{}'/\"$id\"\n",
                root.join("launched").display()
            ),
        )?;
        std::fs::set_permissions(&app, std::fs::Permissions::from_mode(0o755))?;
        let data = root.join("data");
        let applications = data.join("applications");
        let icons = data.join("icons/hicolor/scalable/apps");
        std::fs::create_dir_all(&applications)?;
        std::fs::create_dir_all(&icons)?;
        for (name, colour) in [
            ("red", "#ff0000"),
            ("green", "#00ff00"),
            ("blue", "#0000ff"),
        ] {
            std::fs::write(icons.join(format!("{name}.svg")), svg(colour))?;
        }
        for app in APPS {
            std::fs::write(
                applications.join(format!("{}.desktop", app.0)),
                entry(root, *app),
            )?;
        }
        for i in 0..extra_apps {
            let id = format!("zz-extra-{i:02}");
            let name = format!("Zz extra {i:02}");
            std::fs::write(
                applications.join(format!("{id}.desktop")),
                entry(root, (&id, &name, "", Some("green"), "")),
            )?;
        }
        std::fs::write(root.join("state/hocuspanda/launches"), COUNTS)?;
        // The browser: it writes down what it opens.
        let opener = root.join("bin/xdg-open");
        std::fs::write(
            &opener,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" >> '{}'\n",
                root.join("launched/xdg-open").display()
            ),
        )?;
        std::fs::set_permissions(&opener, std::fs::Permissions::from_mode(0o755))?;
        let log = root.join("shell.log");
        let script = root.join("shell");
        let home = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' PATH='{}' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                home("home"),
                home("config"),
                home("cache"),
                home("state"),
                data.display(),
                root.join("system").display(),
                root.join("bin").display(),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![output.parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self { hp, dir, log };
        let up = bed
            .hp
            .dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        bed.logged("the launcher's applications")?;
        bed.hp.pointer_move(600.0, 500.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn root(&self) -> &Path {
        self.dir.path()
    }

    fn plain(&self) -> String {
        String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default())
            .chars()
            .filter(|c| *c != '\u{1b}')
            .collect()
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for the shell to log `text` after `from`.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let read = |log: &Path| {
            String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default())
                .chars()
                .filter(|c| *c != '\u{1b}')
                .collect::<String>()
        };
        let done = self.hp.dispatch_until(WAIT, |_| {
            read(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        let after = read(&log).get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(
            done,
            "the shell never logged {text:?}; it logged:\n{}",
            after
                .lines()
                .filter(|l| l.contains("launcher"))
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok(after)
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<String> {
        self.logged_after(0, text)
    }

    /// The last thing the launcher said it shows after `from`.
    fn shows_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let after = self.logged_after(from, text)?;
        Ok(after
            .lines()
            .rfind(|l| l.contains("the launcher shows it"))
            .unwrap_or_default()
            .to_owned())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    fn type_text(&mut self, text: &str) -> anyhow::Result<()> {
        for c in text.chars() {
            let (key, shift) = key_of(c).ok_or_else(|| anyhow::anyhow!("no key for {c:?}"))?;
            if shift {
                chord(&mut self.hp, &[KEY_LEFTSHIFT, key]);
            } else {
                self.press(key);
            }
        }
        Ok(())
    }

    /// What the browser was asked to open, waiting for it.
    fn opened(&mut self) -> anyhow::Result<String> {
        let file = self.root().join("launched/xdg-open");
        let done = self.hp.dispatch_until(WAIT, |_| file.exists())?;
        anyhow::ensure!(done, "the browser was never started");
        Ok(std::fs::read_to_string(&file)?)
    }

    /// `Super+Space`, and the launcher drawn.
    fn open(&mut self) -> anyhow::Result<String> {
        let from = self.mark();
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_SPACE]);
        let opened = self.hp.dispatch_until(WAIT, |hp| {
            hp.launcher_commits() > 0 && hp.launcher_surfaces().len() == 1
        })?;
        anyhow::ensure!(opened, "no launcher");
        let shows = self.icons_found_after(from)?;
        // Its motion over, in the frame the screen shows: it starts at the
        // shell's first buffer, which on a loaded machine comes well after
        // the shell said what it shows.
        let still = self.hp.dispatch_until(WAIT, Headless::launcher_at_rest)?;
        anyhow::ensure!(still, "the launcher still moves");
        Ok(shows)
    }

    /// What the launcher shows after `from` once its tiles' icons are
    /// found: the shell looks them up off its loop, its tiles saying "…"
    /// meanwhile, which on a loaded machine outlasts its first frames.
    fn icons_found_after(&mut self, from: usize) -> anyhow::Result<String> {
        let log = self.log.clone();
        let last = |log: &Path| {
            let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default())
                .chars()
                .filter(|c| *c != '\u{1b}')
                .collect::<String>();
            text.get(from..)
                .and_then(|l| l.lines().rfind(|l| l.contains("the launcher shows it")))
                .unwrap_or_default()
                .to_owned()
        };
        let found = self.hp.dispatch_until(WAIT, |_| {
            let shows = last(&log);
            !shows.is_empty() && !shows.contains("[…]")
        })?;
        let shows = last(&log);
        anyhow::ensure!(found, "the launcher's icons never came: {shows}");
        Ok(shows)
    }

    fn closed(&mut self) -> anyhow::Result<bool> {
        self.hp
            .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        // Once the shell's surfaces there are drawn: else the press goes
        // to what is under them.
        common::point(&mut self.hp, (x, y))?;
        wait(&mut self.hp, Duration::from_millis(100))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    /// The pixel at (`x`, `y`): red, green, blue.
    fn pixel(&mut self, x: i32, y: i32) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }

    /// The pixels at `points` in the first frame drawn where they pass
    /// `done`, read again at each new frame; the last ones read if none
    /// does within [`WAIT`].
    fn pixels_until<const N: usize>(
        &mut self,
        points: [(i32, i32); N],
        done: impl Fn(&[[u8; 3]; N]) -> bool,
    ) -> anyhow::Result<[[u8; 3]; N]> {
        let start = Instant::now();
        loop {
            let frames = self.hp.frames();
            let (width, _, bytes) = self.hp.screenshot(0)?;
            let mut read = [[0; 3]; N];
            for (pixel, (x, y)) in read.iter_mut().zip(points) {
                let at = usize::try_from(y * width + x)? * 4;
                let p = bytes
                    .get(at..at + 4)
                    .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
                *pixel = [p[2], p[1], p[0]];
            }
            if done(&read) || start.elapsed() >= WAIT {
                return Ok(read);
            }
            self.hp.dispatch_until(WAIT, |hp| hp.frames() != frames)?;
        }
    }
}

/// On the 1920 × 1080 screen, the 5.1 mockup's sizes × 1.5 (#518): the
/// panel 1140 px wide from x 390, its grid's area from y 195, its headings
/// 36 px, its rows of tiles 168, a tile 138 × 159 and 18 px in.
const AREA_TOP: f64 = 195.0;
const HEAD: f64 = 36.0;
const ROW: f64 = 168.0;

/// Tile `column` of the row whose top is `top`, its middle.
fn tile_middle(column: usize, top: f64) -> (f64, f64) {
    (390.0 + 18.0 + 138.0 * column as f64 + 69.0, top + 79.5)
}

/// The cards of what the launcher says it shows, top first.
fn cards(shows: &str) -> Vec<String> {
    let Some(start) = shows.find("cards=[") else {
        return Vec::new();
    };
    let rest = &shows[start + 7..];
    let end = rest.find("] offset=").unwrap_or(rest.len());
    rest[..end]
        .split("\", \"")
        .map(|card| card.trim_matches('"').to_owned())
        .filter(|card| !card.is_empty())
        .collect()
}

/// The tiles of what the launcher says it shows, row by row.
fn tiles(shows: &str) -> Vec<String> {
    let Some(start) = shows.find("tiles=[") else {
        return Vec::new();
    };
    let rest = &shows[start + 7..];
    let end = rest.find("] cards=").unwrap_or(rest.len());
    rest[..end]
        .split("\", \"")
        .map(|row| row.trim_matches('"').to_owned())
        .filter(|row| !row.is_empty())
        .collect()
}

#[test]
fn super_space_opens_it_full_screen_with_the_keyboard_and_escape_closes_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    // Over the whole screen, the bar too, holding the keyboard.
    let surfaces = bed.hp.launcher_surfaces();
    assert_eq!(
        surfaces,
        [("HEADLESS-1".to_owned(), Rect::new(0, 0, 1920, 1080))],
        "{surfaces:?}"
    );
    assert_eq!(bed.hp.keyboard_layer().as_deref(), Some("hocus-launcher"));
    // The bar's dark blue behind: on the CPU, as here, the shell's surface
    // is opaque and draws it itself, #161B2A at the top to #0E111C at the
    // bottom.
    let [r, g, b] = bed.pixel(100, 10)?;
    assert!(
        r.abs_diff(22) <= 2 && g.abs_diff(27) <= 2 && b.abs_diff(42) <= 2,
        "{:?}",
        [r, g, b]
    );
    let [r, g, b] = bed.pixel(100, 1070)?;
    assert!(
        r.abs_diff(14) <= 2 && g.abs_diff(17) <= 2 && b.abs_diff(28) <= 2,
        "{:?}",
        [r, g, b]
    );
    bed.press(KEY_ESC);
    assert!(bed.closed()?, "Escape did not close it");
    assert_eq!(bed.hp.keyboard_layer(), None);
    // Super+Space again closes it too.
    bed.open()?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_SPACE]);
    assert!(bed.closed()?, "Super+Space did not close it");
    // So does a click away from the field and the tiles, the bar's edge
    // too, which the launcher covers: the bar keeps its form.
    let bars = bed.hp.bars();
    bed.open()?;
    bed.click((1915.0, 540.0))?;
    assert!(bed.closed()?, "a click outside did not close it");
    assert_eq!(bed.hp.bars(), bars);
    // A click in the field does not.
    bed.open()?;
    bed.click((960.0, 120.0))?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.launcher_surfaces().len(), 1);
    Ok(())
}

#[test]
fn the_empty_field_shows_the_frequent_row_then_all_from_a_to_z() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    let shows = bed.open()?;
    assert!(
        shows.contains(r#"heads=["FRÉQUENTES@195", "TOUTES LES APPLICATIONS@399"]"#),
        "{shows}"
    );
    let rows = tiles(&shows);
    // The most launched first, the first selected; then A to Z, accents
    // ignored; the hidden and GNOME's own left out.
    assert_eq!(
        rows[0], "*kitty[icon], Firefox[icon], Thunderbird[icon]",
        "{shows}"
    );
    assert!(
        rows[1].starts_with(
            "Calculatrice[letter], Chromium[icon], Éditeur de texte[icon], Fichiers[icon], \
             Firefox[icon], Gestionnaire de / mises à jour d…[icon], kitty[icon], \
             Obsidian[letter]"
        ),
        "{shows}"
    );
    assert_eq!(rows[2], "Paramètres[icon], Thunderbird[icon]", "{shows}");
    assert!(
        !shows.contains("Caché") && !shows.contains("GNOME"),
        "{shows}"
    );
    assert!(shows.contains("mark=false"), "{shows}");
    Ok(())
}

#[test]
fn a_narrow_screen_has_fewer_columns_of_tiles_their_size_kept() -> anyhow::Result<()> {
    let count = |row: &str| row.matches("[icon]").count() + row.matches("[letter]").count();
    let row_of = |shows: &str, tile: &str| {
        tiles(shows)
            .into_iter()
            .find(|row| row.contains(tile))
            .unwrap_or_default()
    };
    // 1366 × 768 at scale 1: the eight columns fit (1140 px).
    let Some(mut bed) = Bed::start_on(0, "1366x768")? else {
        return Ok(());
    };
    let shows = bed.open()?;
    assert_eq!(count(&row_of(&shows, "Calculatrice")), 8, "{shows}");
    drop(bed);
    // At 1.25, it is 1093 logical pixels wide: seven columns. From kitty,
    // down to Calculatrice, the first from A to Z, in a row of seven…
    let Some(mut bed) = Bed::start_on(0, "1366x768@1.25")? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.press(KEY_DOWN);
    let shows = bed.shows_after(from, "*Calculatrice")?;
    let row = row_of(&shows, "*Calculatrice");
    assert_eq!(count(&row), 7, "{shows}");
    assert!(row.ends_with("kitty[icon]"), "{shows}");
    // …then to Obsidian under it, the eighth, in a row of three.
    let from = bed.mark();
    bed.press(KEY_DOWN);
    let shows = bed.shows_after(from, "*Obsidian")?;
    let row = row_of(&shows, "*Obsidian");
    assert_eq!(
        row, "*Obsidian[letter], Paramètres[icon], Thunderbird[icon]",
        "{shows}"
    );
    Ok(())
}

#[test]
fn typing_filters_the_grid_best_first_and_says_when_nothing_matches() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("fire")?;
    let shows = bed.shows_after(from, "query=\"fire\"")?;
    assert_eq!(tiles(&shows), ["*Firefox[icon]"], "{shows}");
    assert!(shows.contains("heads=[]"), "{shows}");
    // By the generic name too, the most launched first on a tie; accents
    // and case do not count.
    for _ in 0..4 {
        bed.press(KEY_BACKSPACE);
    }
    let from = bed.mark();
    bed.type_text("navig")?;
    let shows = bed.shows_after(from, "query=\"navig\"")?;
    assert_eq!(tiles(&shows), ["*Firefox[icon], Chromium[icon]"], "{shows}");
    for _ in 0..5 {
        bed.press(KEY_BACKSPACE);
    }
    let from = bed.mark();
    bed.type_text("editeur")?;
    let shows = bed.shows_after(from, "query=\"editeur\"")?;
    assert_eq!(tiles(&shows)[0], "*Éditeur de texte[icon]", "{shows}");
    for _ in 0..7 {
        bed.press(KEY_BACKSPACE);
    }
    let from = bed.mark();
    bed.type_text("zzz")?;
    let shows = bed.shows_after(from, "query=\"zzz\"")?;
    // No application: only the web search (5.2).
    assert!(tiles(&shows).is_empty(), "{shows}");
    let cards = cards(&shows);
    assert_eq!(cards.len(), 1, "{shows}");
    assert!(cards[0].starts_with("*web: Chercher"), "{shows}");
    Ok(())
}

#[test]
fn the_arrows_move_the_selection_and_enter_starts_it() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    // From kitty: one right, then down to the same column of A to Z.
    let from = bed.mark();
    bed.press(KEY_RIGHT);
    bed.press(KEY_DOWN);
    let shows = bed.shows_after(from, "*Chromium")?;
    assert!(shows.contains("*Chromium[icon]"), "{shows}");
    let from = bed.mark();
    bed.press(KEY_ENTER);
    assert!(bed.closed()?, "Enter did not close it");
    bed.logged_after(from, "the launcher launches")?;
    // Started by the compositor, its field codes out (%U), %c its name.
    let launched = bed.root().join("launched/chromium");
    let done = bed.hp.dispatch_until(WAIT, |_| launched.exists())?;
    anyhow::ensure!(done, "Chromium was not started");
    let args = std::fs::read_to_string(&launched)?;
    assert_eq!(args, "--name=Chromium\n");
    // Counted, on disk.
    let counts = bed.root().join("state/hocuspanda/launches");
    let start = Instant::now();
    while !std::fs::read_to_string(&counts)?.contains("1\tchromium") && start.elapsed() < WAIT {
        wait(&mut bed.hp, Duration::from_millis(100))?;
    }
    assert!(
        std::fs::read_to_string(&counts)?.contains("1\tchromium"),
        "{}",
        std::fs::read_to_string(&counts)?
    );
    // And a click on a tile starts its application too.
    bed.open()?;
    bed.click(tile_middle(1, AREA_TOP + HEAD))?;
    let launched = bed.root().join("launched/org.mozilla.firefox");
    let done = bed.hp.dispatch_until(WAIT, |_| launched.exists())?;
    anyhow::ensure!(done, "Firefox was not started by its tile");
    assert!(bed.closed()?);
    Ok(())
}

#[test]
fn the_wheel_scrolls_the_grid_by_rows() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(40)? else {
        return Ok(());
    };
    let shows = bed.open()?;
    assert!(shows.contains("offset=0 mark=true"), "{shows}");
    common::over(&mut bed.hp, (960.0, 500.0), hocus_link::LAUNCHER_NAMESPACE)?;
    let from = bed.mark();
    bed.hp.wheel(1);
    let shows = bed.shows_after(from, "offset=1 ")?;
    // A row up: the frequent row's heading out of sight, its tiles first.
    assert!(
        shows.contains(r#"heads=["TOUTES LES APPLICATIONS@363"]"#),
        "{shows}"
    );
    let from = bed.mark();
    bed.hp.wheel(-3);
    bed.shows_after(from, "offset=0 ")?;
    // Down to the end, a notch at a time: the grid's ten rows (two
    // headings, the frequent row, seven rows of 50 applications) 738 px
    // high, its last four rows fill it from row 6.
    const END: usize = 6;
    for offset in 1..=END {
        let from = bed.mark();
        bed.hp.wheel(1);
        bed.shows_after(from, &format!("offset={offset} "))?;
    }
    // And no further: a notch more, then one back, ends a row up from the
    // end, never past it on the way. Had the first gone past it, the
    // second would end on it.
    let from = bed.mark();
    bed.hp.wheel(1);
    bed.hp.wheel(-1);
    let shows = bed.shows_after(from, &format!("offset={} ", END - 1))?;
    let after = bed.plain();
    let offsets: Vec<&str> = after
        .get(from..)
        .unwrap_or_default()
        .lines()
        .filter(|l| l.contains("the launcher shows it"))
        .filter_map(|l| l.split("offset=").nth(1)?.split(' ').next())
        .collect();
    assert!(
        offsets
            .iter()
            .all(|o| o.parse::<usize>().is_ok_and(|o| o <= END)),
        "{offsets:?}: {shows}"
    );
    Ok(())
}

#[test]
fn a_long_name_takes_two_lines_ends_with_an_ellipsis_and_shows_whole_on_hover() -> anyhow::Result<()>
{
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("gest")?;
    let shows = bed.shows_after(from, "query=\"gest\"")?;
    // "Gestionnaire de mises" is wider than a tile's 126 px: the name goes
    // over two lines, the second cut.
    assert_eq!(
        tiles(&shows),
        ["*Gestionnaire de / mises à jour d…[icon], Fichiers[icon]"],
        "{shows}"
    );
    // Rested on, its full name shows.
    let from = bed.mark();
    let (x, y) = tile_middle(0, AREA_TOP);
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    bed.shows_after(from, "tooltip=\"Gestionnaire de mises à jour du système\"")?;
    Ok(())
}

#[test]
fn an_application_installed_while_it_runs_shows_up() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    let shows = bed.open()?;
    assert!(!shows.contains("Inkscape"), "{shows}");
    let from = bed.mark();
    let root = bed.root().to_owned();
    std::fs::write(
        root.join("data/applications/org.inkscape.Inkscape.desktop"),
        entry(
            &root,
            (
                "org.inkscape.Inkscape",
                "Inkscape",
                "Dessin vectoriel",
                Some("red"),
                "",
            ),
        ),
    )?;
    // It shows in the open launcher, in its place from A to Z.
    let shows = bed.shows_after(from, "Inkscape[icon]")?;
    assert!(
        tiles(&shows)[1].contains("Gestionnaire de / mises à jour d…[icon], Inkscape[icon]"),
        "{shows}"
    );
    Ok(())
}

#[test]
fn an_application_without_an_icon_shows_its_letter_on_a_neutral_square() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    // The first row of A to Z: Calculatrice (no icon), then Chromium
    // (blue). Their icons' middles, 13.5 px into the tiles, 72 px wide.
    let top = AREA_TOP + HEAD + ROW + HEAD;
    let icon_middle = |column: usize| {
        let (x, _) = tile_middle(column, top);
        (x as i32, (top + 13.5 + 36.0) as i32)
    };
    let blue = |[r, g, b]: [u8; 3]| b > 150 && r < 60 && g < 60;
    // The neutral square, grey: near its corner, away from the letter.
    let grey = |[r, g, b]: [u8; 3]| r.abs_diff(g) < 25 && g.abs_diff(b) < 30 && r > 50 && r < 170;
    let (chromium, letter) = (icon_middle(1), icon_middle(0));
    let corner = (letter.0 - 27, letter.1 - 27);
    // The shell says what it shows before it draws it: on a loaded
    // machine, the frame with the icons comes a while after the one at
    // rest.
    let [icon, square] = bed.pixels_until([chromium, corner], |[i, s]| blue(*i) && grey(*s))?;
    assert!(blue(icon), "Chromium's blue icon: {icon:?}");
    assert!(grey(square), "Calculatrice's neutral square: {square:?}");
    Ok(())
}

// --- 5.2: calculations, conversions, addresses, the web ---------------------------

#[test]
fn a_calculation_shows_in_a_card_and_enter_copies_it_with_a_toast() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("12*(3+4)")?;
    let shows = bed.shows_after(from, "query=\"12*(3+4)\"")?;
    let cards = cards(&shows);
    // The result first and selected, its expression under it; the web
    // search last.
    assert_eq!(cards[0], "*answer: 84 | 12 × (3 + 4) =", "{shows}");
    assert!(cards[1].starts_with("web: "), "{shows}");
    let from = bed.mark();
    bed.press(KEY_ENTER);
    assert!(bed.closed()?, "Enter did not close it");
    // On the clipboard, and said in a toast at the bottom of the screen.
    let copied = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.clipboard().as_deref() == Some("84"))?;
    assert!(copied, "{:?}", bed.hp.clipboard());
    let said = bed.logged_after(from, "the launcher's toast shows it")?;
    assert!(said.contains("Copié") && said.contains(": 84"), "{said}");
    // Timed from its first frame, as the compositor saw it: a loaded
    // machine's test sees it come late, and go late.
    let shown = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_toast_since().is_some())?;
    assert!(shown, "no toast");
    let since = bed.hp.launcher_toast_since().unwrap_or_default();
    let (output, rect) = bed.hp.launcher_toast_surfaces()[0].clone();
    assert_eq!(output, "HEADLESS-1");
    // 60 px up, 19.5 px text 15 px from its top and bottom (× 1.5).
    assert_eq!(rect.y + rect.height, 1080 - 60, "{rect:?}");
    assert!(rect.height >= 54 && rect.height <= 64, "{rect:?}");
    assert!((rect.x + rect.width / 2 - 960).abs() <= 48, "{rect:?}");
    // For about 1.8 s.
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_toast_surfaces().is_empty())?;
    assert!(gone, "the toast stayed");
    let lasted = bed.hp.now().saturating_sub(since);
    assert!(lasted > Duration::from_millis(1400), "{lasted:?}");
    Ok(())
}

#[test]
fn a_conversion_shows_its_kind() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("10 km en mi")?;
    let shows = bed.shows_after(from, "query=\"10 km en mi\"")?;
    assert_eq!(
        cards(&shows)[0],
        "*answer: 6,214 mi | 10 km = 6,214 mi · Longueur",
        "{shows}"
    );
    assert!(tiles(&shows).is_empty(), "{shows}");
    Ok(())
}

#[test]
fn an_address_opens_in_the_browser() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("github.com")?;
    let shows = bed.shows_after(from, "query=\"github.com\"")?;
    assert_eq!(
        cards(&shows)[0],
        "*address: Ouvrir github.com | https://github.com · dans le navigateur",
        "{shows}"
    );
    bed.press(KEY_ENTER);
    assert!(bed.closed()?, "Enter did not close it");
    assert_eq!(bed.opened()?, "https://github.com\n");
    Ok(())
}

#[test]
fn the_web_search_comes_last_and_ctrl_enter_runs_it_from_anywhere() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    let from = bed.mark();
    bed.type_text("fire")?;
    let shows = bed.shows_after(from, "query=\"fire\"")?;
    // The applications first, the first selected; the web row after them.
    assert_eq!(tiles(&shows), ["*Firefox[icon]"], "{shows}");
    let cards = cards(&shows);
    assert_eq!(cards.len(), 1, "{shows}");
    assert!(
        cards[0].starts_with("web: Chercher")
            && cards[0].contains("fire")
            && cards[0].contains("sur le web | DuckDuckGo · Ctrl+Entrée"),
        "{shows}"
    );
    // The arrows go down to it, and back.
    let from = bed.mark();
    bed.press(KEY_DOWN);
    let shows = bed.shows_after(from, "*web: ")?;
    assert_eq!(tiles(&shows), ["Firefox[icon]"], "{shows}");
    let from = bed.mark();
    bed.press(KEY_UP);
    bed.shows_after(from, "*Firefox")?;
    // Ctrl+Enter on Firefox's tile: the web search, not Firefox.
    chord(&mut bed.hp, &[KEY_LEFTCTRL, KEY_ENTER]);
    assert!(bed.closed()?, "Ctrl+Enter did not close it");
    assert_eq!(bed.opened()?, "https://duckduckgo.com/?q=fire\n");
    assert!(!bed.root().join("launched/org.mozilla.firefox").exists());
    Ok(())
}

#[test]
fn an_answer_or_an_address_lists_no_application() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start(0)? else {
        return Ok(());
    };
    bed.open()?;
    // "calc" lists the calculator…
    let from = bed.mark();
    bed.type_text("calc")?;
    let shows = bed.shows_after(from, "query=\"calc\"")?;
    assert_eq!(tiles(&shows), ["*Calculatrice[letter]"], "{shows}");
    for _ in 0..4 {
        bed.press(KEY_BACKSPACE);
    }
    // …an answer, none.
    let from = bed.mark();
    bed.type_text("2+2")?;
    let shows = bed.shows_after(from, "query=\"2+2\"")?;
    assert!(tiles(&shows).is_empty(), "{shows}");
    assert_eq!(cards(&shows)[0], "*answer: 4 | 2 + 2 =", "{shows}");
    for _ in 0..3 {
        bed.press(KEY_BACKSPACE);
    }
    // Nor an address.
    let from = bed.mark();
    bed.type_text("localhost:3000")?;
    let shows = bed.shows_after(from, "query=\"localhost:3000\"")?;
    assert!(tiles(&shows).is_empty(), "{shows}");
    assert!(
        cards(&shows)[0].starts_with("*address: Ouvrir localhost:3000 | http://localhost:3000"),
        "{shows}"
    );
    Ok(())
}
