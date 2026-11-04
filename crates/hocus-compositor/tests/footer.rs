//! The bar's footer (5.5, issue #395): the real shell, its user named
//! "Alex" (`HOCUS_USER_NAME`), its home, configuration, cache and state
//! directories the test's own, no session bus.
//!
//! One scenario per point of the approved spec: the full form's footer is
//! four buttons in a row along the bar's bottom, each opening its thing
//! (the clipboard's history, the notifications' history, the launcher, the
//! user's name and logging out, confirmed by a second click within 4 s);
//! the menu opens the launcher on a click, not on a hover; the mini form's
//! bell sits over the user's avatar, last of all. The badge still counting is the notifications' tests' (they
//! click the footer's bell), and its colour is checked there.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{BTN_LEFT, Clip, Command, Event, KEY_ESC, WAIT, wait};
use hocus_compositor::{Config, Ending, Headless, HeadlessConfig, RendererChoice, model::Rect};

struct Bed {
    hp: Headless,
    _dir: tempfile::TempDir,
    log: PathBuf,
    screen: (i32, i32),
}

impl Bed {
    /// The compositor on a `width` × `height` screen, its shell's user
    /// named Alex.
    fn start((width, height): (i32, i32)) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        let dir = tempfile::tempdir()?;
        let root = dir.path();
        for sub in ["home", "config", "cache", "state", "data"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let log = root.join("shell.log");
        let script = root.join("shell");
        let at = |sub: &str| root.join(sub).display().to_string();
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{}' XDG_CONFIG_HOME='{}' \
                 XDG_CACHE_HOME='{}' XDG_STATE_HOME='{}' XDG_DATA_HOME='{}' \
                 XDG_DATA_DIRS='{}' HOCUS_USER_NAME=Alex RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{}' \"$@\" 2>>'{}'\n",
                at("home"),
                at("config"),
                at("cache"),
                at("state"),
                at("data"),
                at("data"),
                shell.display(),
                log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![format!("{width}x{height}").parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self {
            hp,
            _dir: dir,
            log,
            screen: (width, height),
        };
        let up = bed.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::BELL_NAMESPACE).len() == 1
        })?;
        anyhow::ensure!(up, "no footer");
        bed.logged("the footer shows it")?;
        bed.hp.pointer_move(400.0, 300.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn plain(&self) -> String {
        plain(&self.log)
    }

    fn mark(&self) -> usize {
        self.plain().len()
    }

    /// Waits for the shell to log `text` after `from`: the line.
    fn logged_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let done = self.hp.dispatch_until(WAIT, |_| {
            plain(&log).get(from..).is_some_and(|l| l.contains(text))
        })?;
        let after = plain(&log).get(from..).unwrap_or_default().to_owned();
        anyhow::ensure!(done, "the shell never logged {text:?}; it logged:\n{after}");
        Ok(after
            .lines()
            .rfind(|l| l.contains(text))
            .unwrap_or_default()
            .to_owned())
    }

    fn logged(&mut self, text: &str) -> anyhow::Result<String> {
        self.logged_after(0, text)
    }

    /// The footer's surface, on the screen.
    fn footer(&self) -> Rect {
        // As drawn: its place before its first buffer is not its own.
        self.hp.drawn_layers(hocus_link::BELL_NAMESPACE)[0].1
    }

    /// The full form's button `i` (from the left), its centre: 40 × 34,
    /// 4 px apart, 11 px and 6 px into the surface.
    fn button(&self, i: i32) -> (f64, f64) {
        let f = self.footer();
        (f64::from(f.x + 11 + 44 * i + 20), f64::from(f.y + 6 + 17))
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        // Once the shell's surfaces there are drawn: else the press goes
        // to what is under them.
        common::point(&mut self.hp, (x, y))?;
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// The popover open, once it is there and drawn.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| common::drawn_popovers(hp).len() == 1)?;
        anyhow::ensure!(open, "no popover: {:?}", self.hp.popover_surfaces());
        // Its fade in over in the frame the screen shows.
        common::at_rest(&mut self.hp)?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    fn closed(&mut self) -> anyhow::Result<()> {
        let closed = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(closed, "the popover stayed");
        Ok(())
    }

    /// The pixel at (`x`, `y`): red, green, blue.
    fn pixel(&mut self, (x, y): (i32, i32)) -> anyhow::Result<[u8; 3]> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let at = usize::try_from(y * width + x)? * 4;
        let p = bytes
            .get(at..at + 4)
            .ok_or_else(|| anyhow::anyhow!("({x}, {y}) off the screen"))?;
        Ok([p[2], p[1], p[0]])
    }

    /// The light pixels in `rect`: a glyph drawn there.
    fn ink(&mut self, rect: Rect) -> anyhow::Result<usize> {
        let (width, _, bytes) = self.hp.screenshot(0)?;
        let mut lit = 0;
        for y in rect.y..rect.y + rect.height {
            for x in rect.x..rect.x + rect.width {
                let at = usize::try_from(y * width + x)? * 4;
                if bytes
                    .get(at..at + 3)
                    .is_some_and(|p| p.iter().all(|c| *c > 120))
                {
                    lit += 1;
                }
            }
        }
        Ok(lit)
    }
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            // `ESC [ … m`
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The names a "the footer shows it" line gives, in order.
fn names(line: &str) -> Vec<String> {
    let Some(start) = line.find("names=[") else {
        return Vec::new();
    };
    let rest = &line[start + 7..];
    let end = rest.find(']').unwrap_or(rest.len());
    rest[..end]
        .split("\", \"")
        .map(|n| n.trim_matches('"').to_owned())
        .collect()
}

#[test]
fn the_full_forms_footer_is_four_buttons_in_a_row_along_the_bars_bottom() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start((1920, 1080))? else {
        return Ok(());
    };
    // The bar inside its rim, 194 wide, its row's bottom 13 px over the
    // bar's (its rim and 12 px), the row 34 px with 6 px around.
    let footer = bed.footer();
    assert_eq!(
        footer,
        Rect::new(1920 - 8 - 1 - 194, 1080 - 8 - 7 - 46, 194, 46)
    );
    let line = bed.logged("the footer shows it")?;
    assert!(line.contains("form=\"full\""), "{line}");
    assert_eq!(
        names(&line),
        [
            "Presse-papiers",
            "Notifications",
            "Menu : toutes les applications",
            "Alex, menu de session"
        ]
    );
    // Each drawn in its 40 × 34 button, 4 px apart: the glyphs' strokes,
    // the avatar's gradient and its letter.
    for i in 0..3 {
        let (x, y) = bed.button(i);
        let button = Rect::new(x as i32 - 20, y as i32 - 17, 40, 34);
        assert!(bed.ink(button)? > 20, "button {i}: nothing drawn");
    }
    let (x, y) = bed.button(3);
    let [r, g, b] = bed.pixel((x as i32 - 8, y as i32))?;
    assert!(g > 120 && b > 100 && r < 140, "the avatar: {:?}", [r, g, b]);
    // The bar's other items keep 14 px over it: the levels' group's
    // surface ends 41 px over the bar's bottom, as before.
    let levels = bed.hp.levels_surfaces()[0].1;
    assert_eq!(levels.y + levels.height, 1080 - 8 - 41);
    Ok(())
}

#[test]
fn each_footer_button_opens_its_thing() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start((1920, 1080))? else {
        return Ok(());
    };
    let bar_bottom = 1080 - 8;
    // The clipboard, nothing copied: its history says so, its bottom on
    // the bar's (5.6, tests/clipboard.rs).
    let mark = bed.mark();
    let clipboard = bed.button(0);
    bed.click(clipboard)?;
    let popover = bed.popover()?;
    assert_eq!(popover.width, 360);
    assert_eq!(popover.y + popover.height, bar_bottom);
    let line = bed.logged_after(mark, "the clipboard's history shows it")?;
    assert!(line.contains("Presse-papiers"), "{line}");
    assert!(line.contains("Rien de copié pour l’instant."), "{line}");
    bed.press(KEY_ESC);
    bed.closed()?;
    // A window copies a text: the history shows it.
    let (rect, client) = common::open_with(&mut bed.hp, "test.copy", (300, 200), false)?;
    bed.click((
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    ))?;
    wait(&mut bed.hp, Duration::from_millis(100))?;
    client.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8"],
        text: "git rebase --onto v2 feat/391",
        hang: false,
    }))?;
    let copied = bed.hp.dispatch_until(WAIT, |_| {
        client.events.try_iter().any(|e| e == Event::Copied)
    })?;
    anyhow::ensure!(copied, "the clipboard was never taken");
    let mark = bed.mark();
    bed.click(clipboard)?;
    bed.popover()?;
    bed.logged_after(mark, "[code] «git rebase --onto v2")?;
    bed.press(KEY_ESC);
    bed.closed()?;
    // The bell: the notifications' history, 360 wide.
    bed.click(bed.button(1))?;
    let history = bed.popover()?;
    assert_eq!(history.width, 360);
    assert_eq!(history.y + history.height, bar_bottom);
    bed.press(KEY_ESC);
    bed.closed()?;
    // The menu: the launcher, over the whole screen.
    bed.click(bed.button(2))?;
    let whole = Rect::new(0, 0, 1920, 1080);
    let opened = bed.hp.dispatch_until(WAIT, |hp| {
        hp.launcher_surfaces()
            .first()
            .is_some_and(|(_, r)| *r == whole)
    })?;
    anyhow::ensure!(opened, "no launcher: {:?}", bed.hp.launcher_surfaces());
    wait(&mut bed.hp, Duration::from_millis(250))?;
    bed.press(KEY_ESC);
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the launcher stayed");
    // The user: their name and "Se déconnecter", which ends the session.
    let mark = bed.mark();
    bed.click(bed.button(3))?;
    let session = bed.popover()?;
    assert_eq!(session.y + session.height, bar_bottom);
    let line = bed.logged_after(mark, "the session's popover shows it")?;
    assert!(line.contains("name=\"Alex\""), "{line}");
    assert!(line.contains("Se déconnecter"), "{line}");
    assert_eq!(bed.hp.ending(), None);
    // Its row: 8 px into the popover's padding, 49 px down, 44 px tall. A
    // click asks to confirm, for 4 s; once they ran out, nothing ended.
    let row = (
        f64::from(session.x + 9 + 131),
        f64::from(session.y + 49 + 22),
    );
    let mark = bed.mark();
    bed.click(row)?;
    bed.logged_after(mark, "the session's popover asks to confirm")?;
    let mark = bed.mark();
    wait(&mut bed.hp, Duration::from_millis(4300))?;
    bed.logged_after(mark, "its time ran out")?;
    assert_eq!(bed.hp.ending(), None);
    // Asked again, Escape lets it go back, the popover still open.
    let mark = bed.mark();
    bed.click(row)?;
    bed.logged_after(mark, "asks to confirm")?;
    bed.press(KEY_ESC);
    bed.logged_after(mark, "why=\"Escape\"")?;
    assert_eq!(bed.hp.popover_surfaces().len(), 1);
    assert_eq!(bed.hp.ending(), None);
    // Asked, then confirmed within 4 s: the session ends.
    let mark = bed.mark();
    bed.click(row)?;
    bed.logged_after(mark, "asks to confirm")?;
    assert_eq!(bed.hp.ending(), None);
    bed.click(row)?;
    let ended = bed.hp.dispatch_until(WAIT, |hp| hp.ending().is_some())?;
    anyhow::ensure!(ended, "the session did not end");
    assert_eq!(bed.hp.ending(), Some(Ending::LoggedOut));
    Ok(())
}

#[test]
fn the_menu_opens_the_launcher_on_a_click_not_a_hover() -> anyhow::Result<()> {
    let Some(mut bed) = Bed::start((1920, 1080))? else {
        return Ok(());
    };
    // Resting on it opens nothing, as a full screen must not open on the
    // way past; the bell beside it opens its history by hovering.
    let menu = bed.button(2);
    bed.hp.pointer_move(menu.0 - 1.0, menu.1);
    bed.hp.pointer_move(menu.0, menu.1);
    wait(&mut bed.hp, Duration::from_millis(600))?;
    assert!(bed.hp.launcher_surfaces().is_empty());
    assert!(bed.hp.popover_surfaces().is_empty());
    let bell = bed.button(1);
    bed.hp.pointer_move(bell.0, bell.1);
    bed.popover()?;
    bed.hp.pointer_move(menu.0, menu.1);
    wait(&mut bed.hp, Duration::from_millis(600))?;
    assert!(bed.hp.launcher_surfaces().is_empty());
    // A click opens it.
    bed.click(menu)?;
    let opened = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().len() == 1)?;
    anyhow::ensure!(opened, "a click did not open the launcher");
    assert_eq!(bed.hp.keyboard_layer().as_deref(), Some("hocus-launcher"));
    Ok(())
}

#[test]
fn the_mini_forms_bell_sits_over_the_users_avatar_last_of_all() -> anyhow::Result<()> {
    // 1366 × 768: the mini form.
    let Some(mut bed) = Bed::start((1366, 768))? else {
        return Ok(());
    };
    let (width, height) = bed.screen;
    let footer = bed.footer();
    assert_eq!(
        footer,
        Rect::new(width - 8 - 1 - 54, height - 8 - 5 - 82, 54, 82)
    );
    let line = bed.logged("form=\"mini\"")?;
    assert_eq!(names(&line), ["Notifications", "Alex, menu de session"]);
    // The bell 44 × 34 at the top, its glyph drawn; the avatar under it,
    // 2 px apart, its bottom 11 px over the bar's (its rim and 10 px).
    let bell = Rect::new(footer.x + 5, footer.y + 6, 44, 34);
    let avatar = Rect::new(footer.x + 5, footer.y + 6 + 34 + 2, 44, 34);
    assert_eq!(avatar.y + avatar.height, height - 8 - 11);
    assert!(bed.ink(bell)? > 20, "no bell drawn");
    let [r, g, b] = bed.pixel((avatar.x + 22 - 8, avatar.y + 17))?;
    assert!(g > 120 && b > 100 && r < 140, "the avatar: {:?}", [r, g, b]);
    // Hovered, the avatar opens the user's popover beside the mini bar.
    bed.hp
        .pointer_move(f64::from(avatar.x + 22), f64::from(avatar.y + 17));
    let session = bed.popover()?;
    assert_eq!(session.x + session.width, width - 8 - 56 - 8);
    bed.logged("the session's popover shows it")?;
    Ok(())
}
