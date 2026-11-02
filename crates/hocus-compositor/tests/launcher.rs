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
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use common::{BTN_LEFT, KEY_DOWN, KEY_ESC, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, wait};
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
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
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
                 XDG_DATA_DIRS='{}' RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                home("home"),
                home("config"),
                home("cache"),
                home("state"),
                data.display(),
                root.join("system").display(),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
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
            let key = letter(c).ok_or_else(|| anyhow::anyhow!("no key for {c:?}"))?;
            self.press(key);
        }
        Ok(())
    }

    /// `Super+Space`, and the launcher drawn.
    fn open(&mut self) -> anyhow::Result<String> {
        let from = self.mark();
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_SPACE]);
        let opened = self.hp.dispatch_until(WAIT, |hp| {
            hp.launcher_commits() > 0 && hp.launcher_surfaces().len() == 1
        })?;
        anyhow::ensure!(opened, "no launcher");
        let shows = self.shows_after(from, "the launcher shows it")?;
        // Its motion over.
        wait(&mut self.hp, Duration::from_millis(250))?;
        Ok(shows)
    }

    fn closed(&mut self) -> anyhow::Result<bool> {
        self.hp
            .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
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
}

/// On the 1920 × 1080 screen: the panel 760 px wide from x 580, its grid's
/// area from y 130; tile `column` of the row whose top is `top`, its
/// middle.
fn tile_middle(column: usize, top: f64) -> (f64, f64) {
    (580.0 + 12.0 + 92.0 * column as f64 + 46.0, top + 53.0)
}

/// The tiles of what the launcher says it shows, row by row.
fn tiles(shows: &str) -> Vec<String> {
    let Some(start) = shows.find("tiles=[") else {
        return Vec::new();
    };
    let rest = &shows[start + 7..];
    let end = rest.find("] offset=").unwrap_or(rest.len());
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
    // Dark behind: on the CPU, as here, the shell's surface is opaque and
    // draws the dimmed desktop's colour itself.
    let [r, g, b] = bed.pixel(100, 1000)?;
    assert!(r < 20 && g < 22 && b < 30, "{:?}", [r, g, b]);
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
    bed.click((960.0, 81.0))?;
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
        shows.contains(r#"heads=["FRÉQUENTES@130", "TOUTES LES APPLICATIONS@266"]"#),
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
            "Calculatrice[letter], Chromium[icon], Éditeur de / texte[icon], Fichiers[icon], \
             Firefox[icon], Gestionnaire / de mises à jou…[icon], kitty[icon], \
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
    assert_eq!(tiles(&shows)[0], "*Éditeur de / texte[icon]", "{shows}");
    for _ in 0..7 {
        bed.press(KEY_BACKSPACE);
    }
    let from = bed.mark();
    bed.type_text("zzz")?;
    let shows = bed.shows_after(from, "query=\"zzz\"")?;
    assert!(
        shows.contains("empty=\"Aucune application pour «\u{a0}zzz\u{a0}».\""),
        "{shows}"
    );
    assert!(tiles(&shows).is_empty(), "{shows}");
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
    bed.click(tile_middle(1, 130.0 + 24.0))?;
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
    bed.hp.pointer_move(960.0, 500.0);
    wait(&mut bed.hp, Duration::from_millis(100))?;
    let from = bed.mark();
    bed.hp.wheel(1);
    let shows = bed.shows_after(from, "offset=1 ")?;
    // A row up: the frequent row's heading out of sight, its tiles first.
    assert!(
        shows.contains(r#"heads=["TOUTES LES APPLICATIONS@242"]"#),
        "{shows}"
    );
    let from = bed.mark();
    bed.hp.wheel(-3);
    bed.shows_after(from, "offset=0 ")?;
    // Down to the end, and no further.
    for _ in 0..10 {
        bed.hp.wheel(1);
    }
    let from = bed.mark();
    bed.hp.wheel(1);
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let last = bed.plain();
    let offsets: Vec<&str> = last
        .lines()
        .filter(|l| l.contains("the launcher shows it"))
        .filter_map(|l| l.split("offset=").nth(1)?.split(' ').next())
        .collect();
    assert!(
        offsets.last().is_some_and(|o| *o != "0"),
        "{offsets:?} from {from}"
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
    // "Gestionnaire de" is wider than a tile's 84 px: the name goes over
    // two lines, the second cut.
    assert_eq!(
        tiles(&shows),
        ["*Gestionnaire / de mises à jou…[icon], Fichiers[icon]"],
        "{shows}"
    );
    // Rested on, its full name shows.
    let from = bed.mark();
    let (x, y) = tile_middle(0, 130.0);
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
        tiles(&shows)[1].contains("Gestionnaire / de mises à jou…[icon], Inkscape[icon]"),
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
    // (blue). Their icons' middles, 9 px into the tiles, 48 px wide.
    let top = 130.0 + 24.0 + 112.0 + 24.0;
    let icon_middle = |column: usize| {
        let (x, _) = tile_middle(column, top);
        (x as i32, (top + 9.0 + 24.0) as i32)
    };
    let (x, y) = icon_middle(1);
    let [r, g, b] = bed.pixel(x, y)?;
    assert!(
        b > 150 && r < 60 && g < 60,
        "Chromium's blue icon: {:?}",
        [r, g, b]
    );
    // The neutral square, grey: near its corner, away from the letter.
    let (x, y) = icon_middle(0);
    let [r, g, b] = bed.pixel(x - 18, y - 18)?;
    let grey = r.abs_diff(g) < 25 && g.abs_diff(b) < 30 && r > 50 && r < 170;
    assert!(grey, "Calculatrice's neutral square: {:?}", [r, g, b]);
    Ok(())
}
