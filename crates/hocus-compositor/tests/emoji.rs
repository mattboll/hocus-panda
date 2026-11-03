//! The emoji picker (5.7, issue #397): the real shell, its home,
//! configuration, cache, state and data directories the test's own, no
//! session bus, in French.
//!
//! One scenario per point of the approved spec: it opens under the caret
//! of a window whose text input says where it is, else centred on the
//! window; the search reads French and English; `Enter` inserts through
//! the text input and closes, `Shift+Enter` inserts and keeps it open; a
//! window without a text input gets it pasted and the clipboard back; the
//! skin tone applies; the recent emoji come first, kept on disk; the
//! launcher's `:chat` row inserts into the window that had the keyboard.
//!
//! Pixman here, so the shell draws on the CPU, as in CI.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use common::{
    BTN_LEFT, ClientHandle, Clip, Command, Event, KEY_ESC, KEY_LEFTMETA, KEY_LEFTSHIFT, Shows,
    WAIT, chord, wait,
};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::Rect};

const KEY_DOT: u32 = 52;
const KEY_SPACE: u32 = 57;
const KEY_ENTER: u32 = 28;
const KEY_BACKSPACE: u32 = 14;
const KEY_SEMICOLON: u32 = 39;

/// The picker's sizes (the mockup's × 1.5): 504 wide, 606 tall; its tone's
/// button and its tones' row, from its top left corner.
const PICKER: (i32, i32) = (504, 606);
const TONE_BUTTON: (f64, f64) = (461.0, 46.0);
/// The fourth tone (index 3, medium), in the row under the top line.
const TONE_3: (f64, f64) = (384.5, 101.5);

struct Bed {
    hp: Headless,
    log: PathBuf,
    state: PathBuf,
}

struct Window {
    client: ClientHandle,
    rect: Rect,
}

impl Bed {
    fn start(root: &Path) -> anyhow::Result<Option<Self>> {
        let Some(shell) = common::shell_program() else {
            eprintln!("hocuspanda-shell is not built: build the workspace to run this test");
            return Ok(None);
        };
        for sub in ["home", "config", "cache", "state", "data/applications"] {
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
                 XDG_DATA_DIRS='{}:/usr/share' HOCUS_USER_NAME=Alex \
                 RUST_LOG=warn,hocuspanda_shell=debug\n\
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
        config.applications = vec![root.join("data/applications")];
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
        let up = bed.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::BELL_NAMESPACE).len() == 1
        })?;
        anyhow::ensure!(up, "no footer");
        bed.hp.pointer_move(200.0, 200.0);
        wait(&mut bed.hp, Duration::from_millis(300))?;
        Ok(Some(bed))
    }

    fn mark(&self) -> usize {
        plain(&self.log).len()
    }

    /// The last line of the shell's that holds `what` after `from`, once
    /// one says `text`.
    fn said_after(&mut self, from: usize, what: &str, text: &str) -> anyhow::Result<String> {
        let log = self.log.clone();
        let last = |log: &Path| {
            plain(log)
                .get(from..)
                .and_then(|l| l.lines().rfind(|l| l.contains(what)).map(str::to_owned))
                .unwrap_or_default()
        };
        let done = self
            .hp
            .dispatch_until(WAIT, |_| last(&log).contains(text))?;
        let line = last(&log);
        anyhow::ensure!(done, "the shell never said {text:?}; it said {line:?}");
        Ok(line)
    }

    /// What the picker shows, once it says `text`.
    fn picker_after(&mut self, from: usize, text: &str) -> anyhow::Result<String> {
        self.said_after(from, "the emoji picker shows it", text)
    }

    /// A window with a text field, its text input's caret there.
    fn window(
        &mut self,
        app_id: &'static str,
        caret: Option<(i32, i32, i32, i32)>,
    ) -> anyhow::Result<Window> {
        let (rect, client) = common::open_showing(
            &mut self.hp,
            app_id,
            Shows::Writing {
                app_id,
                size: (800, 500),
                caret,
            },
        )?;
        // Holding the keyboard, and with a caret its text input enabled
        // there: the picker opens on what the compositor knows then.
        let ready = self.hp.dispatch_until(WAIT, |hp| {
            let keyboard = hp
                .windows()
                .iter()
                .any(|w| w.keyboard && w.app_id.as_deref() == Some(app_id));
            keyboard
                && match caret {
                    Some(caret) => hp.text_input(app_id) == Some(Some(caret)),
                    None => true,
                }
        })?;
        anyhow::ensure!(
            ready,
            "{app_id} never took the keyboard with its caret: {:?}",
            self.hp.text_input(app_id)
        );
        Ok(Window { client, rect })
    }

    fn click(&mut self, (x, y): (f64, f64)) -> anyhow::Result<()> {
        // Once the shell's surfaces there are drawn: else the press goes
        // to what is under them.
        common::point(&mut self.hp, (x, y))?;
        wait(&mut self.hp, Duration::from_millis(60))?;
        self.hp.button(BTN_LEFT, true);
        self.hp.button(BTN_LEFT, false);
        wait(&mut self.hp, Duration::from_millis(100))
    }

    /// `Super+.`: the picker, once it is there and drawn, and what it
    /// shows.
    fn open(&mut self) -> anyhow::Result<(Rect, String)> {
        let mark = self.mark();
        chord(&mut self.hp, &[KEY_LEFTMETA, KEY_DOT]);
        // Drawn at its place: it has one from its first commit, 0 × 0
        // until the shell's first buffer, which takes no click either.
        let open = self.hp.dispatch_until(WAIT, |hp| {
            hp.drawn_layers(hocus_link::EMOJI_NAMESPACE).len() == 1
        })?;
        anyhow::ensure!(
            open,
            "no emoji picker: {:?}",
            self.hp.layer_rects(hocus_link::EMOJI_NAMESPACE)
        );
        let line = self.picker_after(mark, "holding=true")?;
        let picker = self.picker().ok_or_else(|| anyhow::anyhow!("no picker"))?;
        Ok((picker, line))
    }

    fn picker(&self) -> Option<Rect> {
        self.hp
            .drawn_layers(hocus_link::EMOJI_NAMESPACE)
            .first()
            .map(|(_, r)| *r)
    }

    fn closed(&mut self) -> anyhow::Result<()> {
        let closed = self.hp.dispatch_until(WAIT, |hp| {
            hp.layer_rects(hocus_link::EMOJI_NAMESPACE).is_empty()
        })?;
        anyhow::ensure!(closed, "the picker stayed");
        Ok(())
    }

    fn press(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// Types `text` on a US keyboard: letters, spaces and `:`.
    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            if c == ':' {
                chord(&mut self.hp, &[KEY_LEFTSHIFT, KEY_SEMICOLON]);
            } else if c == ' ' {
                self.press(KEY_SPACE);
            } else if let Some(code) = letter(c) {
                self.press(code);
            }
        }
    }

    /// Waits for `window` to say `want`.
    fn heard(&mut self, window: &Window, want: impl Fn(&Event) -> bool) -> anyhow::Result<Event> {
        let mut found = None;
        let done = self.hp.dispatch_until(WAIT, |_| {
            if found.is_none() {
                found = window.client.events.try_iter().find(|e| want(e));
            }
            found.is_some()
        })?;
        match found {
            Some(event) if done => Ok(event),
            _ => anyhow::bail!("the window never said it"),
        }
    }
}

fn committed(e: &Event) -> bool {
    matches!(e, Event::Committed(_))
}

fn pasted(e: &Event) -> bool {
    matches!(e, Event::Pasted(_))
}

/// The emoji selected, as a line of the picker's says it.
fn selected(line: &str) -> Option<String> {
    let at = line.find('*')?;
    let rest = &line[at + 1..];
    let end = rest.find([' ', '"']).unwrap_or(rest.len());
    Some(rest[..end].to_owned())
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

#[test]
fn it_opens_under_the_caret_else_centred_on_the_window() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    // The text input says its caret is 100 px in, 40 down, 20 tall: the
    // picker's top left corner is there, 6 px under it.
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    let (picker, line) = bed.open()?;
    assert_eq!((picker.width, picker.height), PICKER);
    assert_eq!(
        (picker.x, picker.y),
        (writer.rect.x + 100, writer.rect.y + 40 + 20 + 6),
        "{line}"
    );
    // In French, the field empty, the smileys first (nothing used yet).
    assert!(line.contains("lit=\"Smileys\""), "{line}");
    assert!(line.contains("footer=\"😀 visage rieur\""), "{line}");
    bed.press(KEY_ESC);
    bed.closed()?;
    // A window without a text input: centred on it.
    let plain = bed.window("test.plain", None)?;
    let (picker, _) = bed.open()?;
    let centre = |r: Rect| (r.x + r.width / 2, r.y + r.height / 2);
    let (px, py) = centre(picker);
    let (wx, wy) = centre(plain.rect);
    assert!(
        (px - wx).abs() <= 1 && (py - wy).abs() <= 1,
        "{picker:?} {:?}",
        plain.rect
    );
    Ok(())
}

#[test]
fn the_search_reads_french_and_english() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let _writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    bed.open()?;
    let mark = bed.mark();
    bed.type_text("chat");
    let line = bed.picker_after(mark, "query=\"chat\"")?;
    assert!(line.contains("🐱") && line.contains("🐈"), "{line}");
    assert!(
        line.contains("lit=\"\"") && line.contains("heads=[]"),
        "{line}"
    );
    for _ in 0..4 {
        bed.press(KEY_BACKSPACE);
    }
    let mark = bed.mark();
    bed.type_text("cat");
    let line = bed.picker_after(mark, "query=\"cat\"")?;
    assert!(line.contains("🐱"), "{line}");
    // Accents ignored: "etoile" finds "étoile".
    for _ in 0..3 {
        bed.press(KEY_BACKSPACE);
    }
    let mark = bed.mark();
    bed.type_text("etoile");
    let line = bed.picker_after(mark, "query=\"etoile\"")?;
    assert!(line.contains("⭐"), "{line}");
    let mark = bed.mark();
    bed.type_text("zzq");
    let line = bed.picker_after(mark, "query=\"etoilezzq\"")?;
    assert!(line.contains("empty=true"), "{line}");
    Ok(())
}

#[test]
fn enter_inserts_through_the_text_input_and_closes() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    bed.open()?;
    let mark = bed.mark();
    bed.type_text("panda");
    let line = bed.picker_after(mark, "query=\"panda\"")?;
    assert_eq!(selected(&line).as_deref(), Some("🐼"), "{line}");
    bed.press(KEY_ENTER);
    let event = bed.heard(&writer, committed)?;
    assert_eq!(event, Event::Committed("🐼".to_owned()));
    bed.closed()?;
    // Nothing pasted: the clipboard was not touched.
    assert!(!writer.client.events.try_iter().any(|e| pasted(&e)));
    Ok(())
}

#[test]
fn shift_enter_inserts_and_keeps_it_open() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    bed.open()?;
    let mark = bed.mark();
    bed.type_text("panda");
    bed.picker_after(mark, "query=\"panda\"")?;
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_ENTER]);
    let event = bed.heard(&writer, committed)?;
    assert_eq!(event, Event::Committed("🐼".to_owned()));
    // Still open, and holding the keyboard again: typing goes on in its
    // search.
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert!(bed.picker().is_some());
    let mark = bed.mark();
    bed.type_text("s");
    let line = bed.picker_after(mark, "query=\"pandas\"")?;
    assert!(line.contains("holding=true"), "{line}");
    // And again.
    bed.press(KEY_BACKSPACE);
    chord(&mut bed.hp, &[KEY_LEFTSHIFT, KEY_ENTER]);
    let event = bed.heard(&writer, committed)?;
    assert_eq!(event, Event::Committed("🐼".to_owned()));
    assert!(bed.picker().is_some());
    Ok(())
}

#[test]
fn a_window_without_text_input_gets_it_pasted_and_the_clipboard_back() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let plain = bed.window("test.plain", None)?;
    // It copies a text first.
    plain.client.commands.send(Command::Copy(Clip {
        types: &["text/plain;charset=utf-8", "text/plain"],
        text: "avant",
        hang: false,
    }))?;
    bed.heard(&plain, |e| *e == Event::Copied)?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    bed.open()?;
    let mark = bed.mark();
    bed.type_text("panda");
    bed.picker_after(mark, "query=\"panda\"")?;
    bed.press(KEY_ENTER);
    // Ctrl+V: it reads the emoji.
    let event = bed.heard(&plain, pasted)?;
    assert_eq!(event, Event::Pasted("🐼".to_owned()));
    bed.closed()?;
    // Then the clipboard holds what it held before.
    wait(&mut bed.hp, Duration::from_millis(200))?;
    plain.client.commands.send(Command::ReadClipboard)?;
    let event = bed.heard(&plain, pasted)?;
    assert_eq!(event, Event::Pasted("avant".to_owned()));
    Ok(())
}

#[test]
fn the_skin_tone_applies_to_the_emoji_that_have_tones() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    let (picker, _) = bed.open()?;
    let on = |(x, y): (f64, f64)| (f64::from(picker.x) + x, f64::from(picker.y) + y);
    let mark = bed.mark();
    bed.click(on(TONE_BUTTON))?;
    bed.picker_after(mark, "tones=true")?;
    let mark = bed.mark();
    bed.click(on(TONE_3))?;
    bed.picker_after(mark, "tone=3 tones=false")?;
    let mark = bed.mark();
    bed.type_text("pouce haut");
    let line = bed.picker_after(mark, "query=\"pouce haut\"")?;
    assert_eq!(selected(&line).as_deref(), Some("👍🏽"), "{line}");
    bed.press(KEY_ENTER);
    let event = bed.heard(&writer, committed)?;
    assert_eq!(event, Event::Committed("👍🏽".to_owned()));
    // Kept: on disk, and for an emoji without tones, nothing changes.
    let kept = std::fs::read_to_string(bed.state.join("emoji")).unwrap_or_default();
    assert!(kept.starts_with("tone 3\n"), "{kept:?}");
    Ok(())
}

#[test]
fn the_recent_emoji_come_first_and_are_kept() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    let mut used = Vec::new();
    for name in ["chat", "panda"] {
        bed.open()?;
        let mark = bed.mark();
        bed.type_text(name);
        let line = bed.picker_after(mark, &format!("query=\"{name}\""))?;
        used.push(selected(&line).unwrap_or_default());
        bed.press(KEY_ENTER);
        bed.heard(&writer, committed)?;
        bed.closed()?;
    }
    let (_, line) = bed.open()?;
    assert!(line.contains("lit=\"Récents\""), "{line}");
    assert!(line.contains("heads=[\"Récents\", \"Smileys\"]"), "{line}");
    // The last one first, selected.
    let first = format!("rows=[\"*{} {}\"", used[1], used[0]);
    assert!(line.contains(&first), "{first} in {line}");
    let kept = std::fs::read_to_string(bed.state.join("emoji")).unwrap_or_default();
    assert_eq!(kept, format!("tone 0\n{}\n{}\n", used[1], used[0]));
    Ok(())
}

#[test]
fn the_launchers_colon_row_inserts_into_the_window_that_had_the_keyboard() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let Some(mut bed) = Bed::start(dir.path())? else {
        return Ok(());
    };
    let writer = bed.window("test.writer", Some((100, 40, 2, 20)))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_SPACE]);
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().len() == 1)?;
    anyhow::ensure!(open, "no launcher");
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let mark = bed.mark();
    bed.type_text(":chat");
    let line = bed.said_after(mark, "the launcher shows it", "query=\":chat\"")?;
    assert!(line.contains("emoji=[\"*"), "{line}");
    let first = selected(&line[line.find("emoji=").unwrap_or(0)..]).unwrap_or_default();
    assert!(["🐱", "🐈", "😺"].contains(&first.as_str()), "{line}");
    assert!(line.contains("web: Chercher"), "{line}");
    bed.press(KEY_ENTER);
    let event = bed.heard(&writer, committed)?;
    assert_eq!(event, Event::Committed(first));
    let closed = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.launcher_surfaces().is_empty())?;
    anyhow::ensure!(closed, "the launcher stayed");
    Ok(())
}
