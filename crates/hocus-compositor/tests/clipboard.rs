//! The clipboard's history (5.6, issue #396): the real shell, its home,
//! configuration, cache, state and data directories the test's own, no
//! session bus (so no keyring: its key is a file beside it), its
//! applications' desktop entries the test's (an editor, a terminal).
//!
//! One scenario per point of the approved spec: a copy shows with its
//! application; the search filters; `Enter` pastes into the window that
//! had the keyboard, `Ctrl+V` there and `Ctrl+Shift+V` in a terminal;
//! `Shift+Enter` only copies, and what is used moves to the top; `Delete`
//! removes; a pin stays through "Clear all"; a secret copy is never kept;
//! the history comes back after a restart and is not plain text on disk;
//! an image shows its thumbnail. And a screen capture (5.8b, issue #528):
//! the history's first row, pasted from there.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{
    BTN_LEFT, ClientHandle, Clip, Command, Event, KEY_DOWN, KEY_ESC, KEY_LEFTCTRL, KEY_LEFTMETA,
    KEY_LEFTSHIFT, Shows, WAIT, chord, wait,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_V: u32 = 47;
const KEY_ENTER: u32 = 28;
const KEY_DELETE: u32 = 111;
const KEY_BACKSPACE: u32 = 14;
/// Print (`KEY_SYSRQ`).
const KEY_PRINT: u32 = 99;

/// A 40 × 20 PNG, all `#E53935`.
const RED_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x28, 0x00, 0x00, 0x00, 0x14, 0x08, 0x06, 0x00, 0x00, 0x00, 0xff, 0x46, 0x7f,
    0xbb, 0x00, 0x00, 0x00, 0x2c, 0x49, 0x44, 0x41, 0x54, 0x78, 0xda, 0xed, 0xce, 0x21, 0x0d, 0x00,
    0x00, 0x08, 0x00, 0x30, 0x4a, 0x21, 0xe8, 0x9f, 0x85, 0x1c, 0x50, 0x83, 0xb1, 0x8b, 0xfb, 0x47,
    0x57, 0xce, 0x65, 0x21, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0x28, 0xf8, 0x39,
    0xb8, 0x57, 0x98, 0x40, 0xaa, 0xd6, 0xd9, 0xa7, 0x3a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

struct Bed {
    hp: Headless,
    log: PathBuf,
    state: PathBuf,
}

/// A window of the test's, and where it is.
struct Window {
    client: ClientHandle,
    rect: Rect,
}

impl Bed {
    /// The compositor and its shell on `root`'s directories (made if they
    /// are not there).
    fn start(root: &Path) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        for sub in ["home", "config", "cache", "state", "data/applications"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        let applications = root.join("data/applications");
        std::fs::write(
            applications.join("test.editor.desktop"),
            "[Desktop Entry]\nType=Application\nName=Éditeur\nExec=true\nCategories=Utility;\n",
        )?;
        std::fs::write(
            applications.join("test.terminal.desktop"),
            "[Desktop Entry]\nType=Application\nName=Terminal\nExec=true\n\
             Categories=System;TerminalEmulator;\n",
        )?;
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
        config.applications = vec![applications];
        // Captures land in the test's own folder (5.8b).
        config.pictures = Some(root.join("Images"));
        let hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        let mut bed = Self {
            hp,
            log,
            state: root.join("state/hocuspanda"),
        };
        let up = bed
            .hp
            .dispatch_until(WAIT, |hp| hp.bell_surfaces().len() == 1)?;
        anyhow::ensure!(up, "no footer");
        // Drawn: until then it takes no click.
        let button = bed.button();
        common::over(&mut bed.hp, button, hocus_link::BELL_NAMESPACE)?;
        bed.hp.pointer_move(400.0, 300.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
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

    /// The history as the shell last showed it, after `from`, once it says
    /// `text`.
    fn history_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let shown = |log: &Path| {
            plain(log)
                .get(from..)
                .and_then(|l| {
                    l.lines()
                        .rfind(|l| l.contains("the clipboard's history shows it"))
                        .map(str::to_owned)
                })
                .unwrap_or_default()
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| history(&shown(&log)).contains(text))?;
        let line = shown(&log);
        anyhow::ensure!(done, "the history never said {text:?}; it said {line:?}");
        Ok(line)
    }

    /// A window that takes the keyboard and says its keys.
    fn window(&mut self, app_id: &'static str, size: (u32, u32)) -> anyhow::Result<Window> {
        let (rect, client) =
            common::open_showing(&mut self.hp, app_id, Shows::Typing { app_id, size })?;
        Ok(Window { client, rect })
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        self.hp.pointer_move(x - 1.0, y);
        self.hp.pointer_move(x, y);
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        Ok(())
    }

    /// A click into `window`, near its top left corner: windows open at
    /// the screen's centre, the later ones smaller here, so that the
    /// earlier ones show there still.
    fn focus(&mut self, window: &Window) -> anyhow::Result<()> {
        let r = window.rect;
        self.click((f64::from(r.x + 20), f64::from(r.y + 20)))?;
        wait(&mut self.hp, Duration::from_millis(100))
    }

    /// `window` takes the keyboard and copies `text`: once the history has
    /// it.
    fn copy(&mut self, window: &Window, text: &'static str) -> anyhow::Result<()> {
        self.copy_with(
            window,
            Command::Copy(Clip {
                types: &["text/plain;charset=utf-8", "text/plain"],
                text,
                hang: false,
            }),
        )?;
        Ok(())
    }

    fn copy_with(&mut self, window: &Window, command: Command) -> anyhow::Result<()> {
        self.focus(window)?;
        let mark = self.mark();
        window.client.commands.send(command)?;
        let copied = self.hp.dispatch_until(WAIT, |_| {
            window.client.events.try_iter().any(|e| e == Event::Copied)
        })?;
        anyhow::ensure!(copied, "the clipboard was never taken");
        self.logged_after(mark, "a copy for the history")?;
        Ok(())
    }

    /// `Super+V`: the history, once it shows.
    fn super_v(&mut self) -> anyhow::Result<(Rect, String)> {
        let mark = self.mark();
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_V]);
        let rect = self.popover()?;
        let line = self.logged_after(mark, "the clipboard's history shows it")?;
        Ok((rect, line))
    }

    /// The popover open, once it is there and drawn.
    fn popover(&mut self) -> anyhow::Result<Rect> {
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().len() == 1)?;
        anyhow::ensure!(open, "no popover");
        wait(&mut self.hp, Duration::from_millis(200))?;
        Ok(self.hp.popover_surfaces()[0].1)
    }

    fn closed(&mut self) -> anyhow::Result<()> {
        let closed = self
            .hp
            .dispatch_until(WAIT, |hp| hp.popover_surfaces().is_empty())?;
        anyhow::ensure!(closed, "the popover stayed");
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// Types `text`'s letters (a US keyboard).
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            if let Some(code) = letter(c) {
                self.press(code);
            }
        }
    }

    /// The footer's clipboard, its centre: its first button, 40 × 34, 11
    /// px and 6 px into its surface.
    fn button(&self) -> (f64, f64) {
        let f = self.hp.bell_surfaces()[0].1;
        (f64::from(f.x + 11 + 20), f64::from(f.y + 6 + 17))
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

    /// The history's files on disk.
    fn files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(self.state.join("clipboard"))
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default()
    }
}

/// How many items the history keeps on disk, in `state`.
fn clips(state: &Path) -> usize {
    std::fs::read_dir(state.join("clipboard"))
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "clip"))
                .count()
        })
        .unwrap_or(0)
}

/// The keys a window got, its codes pressed (`true`) and released, since
/// the last time.
fn keys(window: &Window) -> Vec<(u32, bool)> {
    window
        .client
        .events
        .try_iter()
        .filter_map(|e| match e {
            Event::Key(key, pressed) => Some((key, pressed)),
            _ => None,
        })
        .collect()
}

/// Waits for `window` to get `want` among its keys (Ctrl, Shift, V).
fn pasted(bed: &mut Bed, window: &Window, want: &[(u32, bool)]) -> anyhow::Result<()> {
    let mut got = Vec::new();
    let done = bed.hp.dispatch_until(WAIT, |_| {
        got.extend(
            keys(window)
                .into_iter()
                .filter(|(k, _)| [KEY_LEFTCTRL, KEY_LEFTSHIFT, KEY_V].contains(k)),
        );
        got.len() >= want.len()
    })?;
    anyhow::ensure!(done && got == want, "the window got {got:?}, not {want:?}");
    Ok(())
}

fn letter(c: char) -> Option<u32> {
    let rows: [(&str, u32); 3] = [("qwertyuiop", 16), ("asdfghjkl", 30), ("zxcvbnm", 44)];
    rows.iter().find_map(|(row, first)| {
        row.find(c)
            .map(|at| first + u32::try_from(at).unwrap_or_default())
    })
}

/// The shell's log, without its colours' escape sequences.
fn plain(log: &Path) -> String {
    let text = String::from_utf8_lossy(&std::fs::read(log).unwrap_or_default()).into_owned();
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
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

/// What a "the clipboard's history shows it" line says the history shows.
fn history(line: &str) -> String {
    let Some(start) = line.find("history=") else {
        return String::new();
    };
    let rest = &line[start + 8..];
    rest[..rest.find(" items=").unwrap_or(rest.len())].to_owned()
}

/// Where a line's targets are, on the popover: name → centre.
fn targets(line: &str) -> HashMap<String, (f64, f64)> {
    let mut found = HashMap::new();
    if let Some(at) = line.find("items=") {
        for word in line[at + 6..].split_whitespace() {
            if let Some((name, xy)) = word.rsplit_once('@')
                && let Some((x, y)) = xy.split_once(',')
                && let (Ok(x), Ok(y)) = (x.parse(), y.parse())
            {
                found.insert(name.to_owned(), (x, y));
            }
        }
    }
    found
}

/// The rows' keys, from the top, as a line places them.
fn rows(line: &str) -> Vec<String> {
    let mut rows: Vec<(f64, String)> = targets(line)
        .into_iter()
        .filter(|(name, _)| name.starts_with("Row("))
        .map(|(name, (_, y))| (y, name[4..name.len() - 1].to_owned()))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, key)| key).collect()
}

/// `target`'s centre on the screen, the popover at `popover`.
fn on_screen(popover: Rect, (x, y): (f64, f64)) -> (f64, f64) {
    (f64::from(popover.x) + x, f64::from(popover.y) + y)
}

#[test]
fn a_copy_shows_with_its_application() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    // Nothing copied yet: the footer's button opens the history, 360 wide,
    // its bottom on the bar's, saying so.
    let mark = bed.mark();
    bed.click(bed.button())?;
    let popover = bed.popover()?;
    assert_eq!(popover.width, 360);
    assert_eq!(popover.y + popover.height, 1080 - 8);
    bed.history_after(mark, "Rien de copié pour l’instant.")?;
    bed.press(KEY_ESC);
    bed.closed()?;
    // The editor copies a text: it shows, from "Éditeur", its desktop
    // entry's name, just now.
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "Bonjour Camille")?;
    let (_, line) = bed.super_v()?;
    assert_eq!(
        history(&line),
        "RÉCENTS: [text] «Bonjour Camille» Éditeur · maintenant (selected)"
    );
    Ok(())
}

#[test]
fn the_search_filters_on_the_text_and_the_application() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let terminal = bed.window("test.terminal", (640, 420))?;
    bed.copy(&terminal, "cargo build --release")?;
    let editor = bed.window("test.editor", (320, 200))?;
    bed.copy(&editor, "Réunion à la Pépinière")?;
    let (_, line) = bed.super_v()?;
    let all = history(&line);
    assert!(
        all.contains("Pépinière") && all.contains("cargo build"),
        "{all}"
    );
    // Without its accents, in any case: the editor's text alone.
    let mark = bed.mark();
    bed.type_text("pepin");
    let line = bed.history_after(mark, "Pépinière")?;
    assert!(!history(&line).contains("cargo"), "{line}");
    // The application's name finds the terminal's.
    let mark = bed.mark();
    for _ in 0..5 {
        bed.press(KEY_BACKSPACE);
    }
    bed.type_text("term");
    let line = bed.history_after(mark, "cargo build")?;
    assert!(!history(&line).contains("Pépinière"), "{line}");
    // Nothing found: said so.
    let mark = bed.mark();
    bed.type_text("zzz");
    bed.history_after(mark, "Rien pour «\u{a0}termzzz\u{a0}»")?;
    Ok(())
}

#[test]
fn enter_pastes_into_the_window_ctrl_shift_v_into_a_terminal() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "git rebase --onto v2")?;
    keys(&editor);
    // Enter: the clipboard holds it, the history closes, the editor gets
    // the keyboard back and Ctrl+V.
    bed.super_v()?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    pasted(
        &mut bed,
        &editor,
        &[
            (KEY_LEFTCTRL, true),
            (KEY_V, true),
            (KEY_V, false),
            (KEY_LEFTCTRL, false),
        ],
    )?;
    assert_eq!(bed.hp.clipboard().as_deref(), Some("git rebase --onto v2"));
    // A terminal (its desktop entry says so): Ctrl+Shift+V.
    let terminal = bed.window("test.terminal", (320, 200))?;
    bed.focus(&terminal)?;
    keys(&terminal);
    bed.super_v()?;
    bed.press(KEY_ENTER);
    bed.closed()?;
    pasted(
        &mut bed,
        &terminal,
        &[
            (KEY_LEFTCTRL, true),
            (KEY_LEFTSHIFT, true),
            (KEY_V, true),
            (KEY_V, false),
            (KEY_LEFTSHIFT, false),
            (KEY_LEFTCTRL, false),
        ],
    )?;
    // Opened by hovering the footer's button, a click on a row pastes it
    // too, the window having kept the keyboard.
    bed.copy(&terminal, "ls -la")?;
    keys(&terminal);
    let (x, y) = bed.button();
    let mark = bed.mark();
    bed.hp.pointer_move(x - 1.0, y);
    bed.hp.pointer_move(x, y);
    let popover = bed.popover()?;
    let line = bed.logged_after(mark, "the clipboard's history shows it")?;
    let second = rows(&line)[1].clone();
    let at = on_screen(popover, targets(&line)[&format!("Row({second})")]);
    let mark = bed.mark();
    bed.hp.pointer_move(at.0, at.1);
    bed.history_after(
        mark,
        "«git rebase --onto v2» Éditeur · maintenant (selected)",
    )?;
    bed.click(at)?;
    bed.closed()?;
    pasted(
        &mut bed,
        &terminal,
        &[
            (KEY_LEFTCTRL, true),
            (KEY_LEFTSHIFT, true),
            (KEY_V, true),
            (KEY_V, false),
            (KEY_LEFTSHIFT, false),
            (KEY_LEFTCTRL, false),
        ],
    )?;
    assert_eq!(bed.hp.clipboard().as_deref(), Some("git rebase --onto v2"));
    Ok(())
}

#[test]
fn shift_enter_only_copies_and_what_is_used_moves_to_the_top() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "première copie")?;
    bed.copy(&editor, "seconde copie")?;
    keys(&editor);
    let (_, line) = bed.super_v()?;
    assert!(
        history(&line).starts_with("RÉCENTS: [text] «seconde copie»"),
        "{line}"
    );
    // The second row, then Shift+Enter: copied, nothing pasted.
    let mark = bed.mark();
    bed.press(KEY_DOWN);
    bed.history_after(mark, "«première copie» Éditeur · maintenant (selected)")?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_ENTER]);
    bed.closed()?;
    let copied = bed.hp.dispatch_until(WAIT, |hp| {
        hp.clipboard().as_deref() == Some("première copie")
    })?;
    anyhow::ensure!(copied, "not copied: {:?}", bed.hp.clipboard());
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let got: Vec<(u32, bool)> = keys(&editor)
        .into_iter()
        .filter(|(k, _)| *k == KEY_LEFTCTRL || *k == KEY_V)
        .collect();
    assert!(got.is_empty(), "pasted: {got:?}");
    // Used, it is first now.
    let (_, line) = bed.super_v()?;
    assert!(
        history(&line).starts_with("RÉCENTS: [text] «première copie»"),
        "{line}"
    );
    Ok(())
}

#[test]
fn delete_removes_the_selected_item() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "à garder")?;
    bed.copy(&editor, "à retirer")?;
    bed.super_v()?;
    let mark = bed.mark();
    bed.press(KEY_DELETE);
    let line = bed.history_after(mark, "«à garder» Éditeur · maintenant (selected)")?;
    assert!(!history(&line).contains("retirer"), "{line}");
    // Its file went too.
    let state = bed.state.clone();
    let gone = bed.hp.dispatch_until(WAIT, |_| clips(&state) == 1)?;
    anyhow::ensure!(gone, "{} items on disk", clips(&state));
    Ok(())
}

#[test]
fn a_pin_stays_through_clear_all() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "ssh alex@192.168.1.24")?;
    bed.copy(&editor, "une copie de passage")?;
    let (popover, line) = bed.super_v()?;
    // The older one's pin, under the pointer: pinned, amber, first.
    let older = rows(&line)[1].clone();
    let pin = on_screen(popover, targets(&line)[&format!("Pin({older})")]);
    let mark = bed.mark();
    bed.click(pin)?;
    let line = bed.history_after(mark, "(pinned)")?;
    assert!(
        history(&line)
            .starts_with("ÉPINGLÉS: [code] «ssh alex@192.168.1.24» Éditeur · maintenant (pinned)"),
        "{line}"
    );
    // Clear all: the pinned one alone stays.
    let clear = on_screen(popover, targets(&line)["Clear"]);
    let mark = bed.mark();
    bed.click(clear)?;
    let line = bed.history_after(mark, "ÉPINGLÉS")?;
    assert!(!history(&line).contains("passage"), "{line}");
    assert!(!history(&line).contains("RÉCENTS"), "{line}");
    Ok(())
}

#[test]
fn a_secret_copy_is_never_kept() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    // A password marked secret, as password managers mark theirs.
    let editor = bed.window("test.editor", (640, 420))?;
    bed.focus(&editor)?;
    editor.client.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8", "x-kde-passwordManagerHint"],
        text: "hunter2-mot-de-passe",
        hang: false,
    }))?;
    // A password manager's window, unmarked.
    let keepass = bed.window("org.keepassxc.KeePassXC", (320, 200))?;
    bed.focus(&keepass)?;
    keepass.client.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8"],
        text: "un-autre-mot-de-passe",
        hang: false,
    }))?;
    wait(&mut bed.hp, Duration::from_millis(500))?;
    // Then a plain copy: the history holds it alone.
    bed.copy(&editor, "une copie ordinaire")?;
    let (_, line) = bed.super_v()?;
    assert_eq!(
        history(&line),
        "RÉCENTS: [text] «une copie ordinaire» Éditeur · maintenant (selected)"
    );
    let state = bed.state.clone();
    let one = bed.hp.dispatch_until(WAIT, |_| clips(&state) == 1)?;
    anyhow::ensure!(one, "{} items on disk", clips(&state));
    Ok(())
}

#[test]
fn the_history_comes_back_after_a_restart_and_is_not_plain_text_on_disk() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let marker = "le-marqueur-du-test 4242";
    {
        let Some(mut bed) = Bed::start(dir.path())? else {
            return Ok(());
        };
        let editor = bed.window("test.editor", (640, 420))?;
        bed.copy(&editor, "le-marqueur-du-test 4242")?;
        let state = bed.state.clone();
        let written = bed.hp.dispatch_until(WAIT, |_| clips(&state) == 1)?;
        anyhow::ensure!(written, "the copy is not on disk");
        let files = bed.files();
        // Nothing on disk reads as what was copied; the key file and the
        // items are the user's alone.
        for file in files.iter().chain([&state.join("clipboard.key")]) {
            let bytes = std::fs::read(file)?;
            assert!(
                !bytes.windows(marker.len()).any(|w| w == marker.as_bytes()),
                "{} holds the copy",
                file.display()
            );
            let mode = std::fs::metadata(file)?.permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "{}", file.display());
        }
        let mode = std::fs::metadata(state.join("clipboard"))?
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }
    // The session again: the history is read back.
    let mark = plain(&dir.path().join("shell.log")).len();
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    bed.logged_after(mark, "the clipboard's history is read back items=1")?;
    let (_, line) = bed.super_v()?;
    assert!(
        history(&line).contains(&format!("«{marker}» Éditeur")),
        "{line}"
    );
    Ok(())
}

#[test]
fn an_image_copy_shows_its_thumbnail() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy_with(&editor, Command::CopyPng(RED_PNG))?;
    let (popover, line) = bed.super_v()?;
    assert_eq!(
        history(&line),
        "RÉCENTS: [image] «Image · 40\u{a0}×\u{a0}20» Éditeur · maintenant (selected)"
    );
    // Its thumbnail in its tile: the row's first, 9 px in, 30 px; the
    // image 3 px inside it.
    let row = targets(&line)
        .into_iter()
        .find(|(name, _)| name.starts_with("Row("))
        .map(|(_, at)| at)
        .ok_or_else(|| anyhow::anyhow!("no row: {line}"))?;
    let tile = (popover.x + 13 + 9 + 15, popover.y + row.1.round() as i32);
    let [r, g, b] = bed.pixel(tile)?;
    assert!(
        r > 180 && g < 100 && b < 100,
        "the thumbnail: {:?}",
        [r, g, b]
    );
    // Pasted back, it is the PNG copied: once the shell's line reaches the
    // compositor, which may be after its popover went (two sockets).
    bed.press(KEY_ENTER);
    bed.closed()?;
    let pasted = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.clipboard_bytes().as_deref() == Some(RED_PNG))?;
    assert!(pasted, "{:?}", bed.hp.clipboard_bytes().map(|b| b.len()));
    Ok(())
}

/// A capture (5.8b, issue #528) is the history's first row, an image from
/// "Capture d'écran", and pasting it from there gives the window its PNG,
/// though the clipboard held something else meanwhile.
#[test]
fn a_capture_is_the_first_row_and_pastes_from_there() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let editor = bed.window("test.editor", (640, 420))?;
    bed.copy(&editor, "avant la capture")?;
    // Print, then Enter: the area kept, a third of the screen.
    let mark = bed.mark();
    chord(&mut bed.hp, &[KEY_PRINT]);
    wait(&mut bed.hp, Duration::from_millis(200))?;
    chord(&mut bed.hp, &[KEY_ENTER]);
    let taken = bed.hp.dispatch_until(WAIT, |hp| hp.captures().0 == 1)?;
    anyhow::ensure!(taken, "nothing captured");
    let file = bed
        .hp
        .captures()
        .1
        .ok_or_else(|| anyhow::anyhow!("no file"))?;
    let png = std::fs::read(&file)?;
    bed.logged_after(mark, "a capture for the history")?;
    // The editor takes the clipboard with what the history does not keep.
    bed.focus(&editor)?;
    editor.client.commands.send(Command::Copy(Clip {
        types: &["text/html"],
        text: "<b>autre chose</b>",
        hang: false,
    }))?;
    let copied = bed.hp.dispatch_until(WAIT, |_| {
        editor.client.events.try_iter().any(|e| e == Event::Copied)
    })?;
    anyhow::ensure!(copied, "the clipboard was never taken");
    assert_ne!(bed.hp.clipboard_bytes().as_deref(), Some(png.as_slice()));
    keys(&editor);
    let (_, line) = bed.super_v()?;
    let shown = history(&line);
    assert!(
        shown.starts_with(
            "RÉCENTS: [image] «Image · 640\u{a0}×\u{a0}360» Capture d'écran · maintenant \
             (selected) [text] «avant la capture» Éditeur"
        ),
        "{shown}"
    );
    // Enter pastes it: the clipboard holds the capture's PNG again, and
    // the editor gets Ctrl+V.
    bed.press(KEY_ENTER);
    bed.closed()?;
    pasted(
        &mut bed,
        &editor,
        &[
            (KEY_LEFTCTRL, true),
            (KEY_V, true),
            (KEY_V, false),
            (KEY_LEFTCTRL, false),
        ],
    )?;
    assert_eq!(bed.hp.clipboard_bytes(), Some(png));
    Ok(())
}
