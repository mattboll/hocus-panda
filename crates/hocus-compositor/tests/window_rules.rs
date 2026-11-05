//! Rules per application, `[[windows.rules]]` (6.14, issue #448): a
//! window of an application with a rule opens on its desktop without
//! taking the focus, at its size from its first frame (maximised, a half,
//! fullscreen), and is kept above the others until a fullscreen window in
//! front covers it; a rule's `above` changed in the file applies to the
//! windows open; a dialog follows its parent, not the rule.
//!
//! The file is written by hand, as the Settings' worker would, and each
//! test waits until the compositor has read it.

mod common;

use std::path::PathBuf;

use anyhow::Context;
use common::{
    ClientHandle, Command, Event, KEY_LEFTMETA, Shows, WAIT, chord, open, open_with, rect_of,
};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, RendererChoice,
    model::{Rect, Zone},
};

const KEY_3: u32 = 4;
/// The work area of the headless 1920×1080 output.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    file: PathBuf,
}

impl Bed {
    /// The compositor, its file saying `rules`.
    fn start(rules: &str) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, rules)?;
        let mut config: Config = toml::from_str(rules)?;
        config.file = Some(file.clone());
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        Ok(Self {
            _dir: dir,
            hp,
            file,
        })
    }

    /// The file now says `rules`, once the compositor has read it.
    fn write(&mut self, rules: &str) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, rules)?;
        std::fs::rename(&new, &self.file)?;
        let wanted: Config = toml::from_str(rules)?;
        let read = self.hp.dispatch_until(WAIT, |hp| {
            hp.state().config.windows.rules == wanted.windows.rules
        })?;
        anyhow::ensure!(read, "the file is not read: {rules}");
        Ok(())
    }

    /// The windows' app ids, bottom first, those shown.
    fn stack(&self) -> Vec<String> {
        self.hp
            .windows()
            .into_iter()
            .map(|w| w.app_id.unwrap_or_default())
            .collect()
    }

    fn focused(&self) -> Option<String> {
        self.hp
            .windows()
            .into_iter()
            .find(|w| w.focused)
            .and_then(|w| w.app_id)
    }

    /// `app_id`'s desktop (from 0) and whether it shows.
    fn on(&self, app_id: &str) -> Option<(usize, bool)> {
        self.hp
            .all_windows()
            .into_iter()
            .find(|w| w.1.as_deref() == Some(app_id))
            .map(|w| (w.2, w.3))
    }
}

fn rule(app_id: &str, rest: &str) -> String {
    format!("[[windows.rules]]\napp-id = \"{app_id}\"\n{rest}\n")
}

#[test]
fn a_window_opens_on_its_rules_desktop_without_the_focus() -> anyhow::Result<()> {
    let mut bed = Bed::start(&rule("test.mail", "desktop = 3"))?;
    open(&mut bed.hp, "test.here", (400, 300))?;
    let _mail = common::client(
        bed.hp.socket(),
        Some(Shows::Window {
            app_id: "test.mail",
            size: (400, 300),
            popup: false,
        }),
    )?;
    let mapped = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.all_windows().len() == 2)?;
    anyhow::ensure!(mapped, "test.mail was never mapped");
    assert_eq!(bed.on("test.mail"), Some((2, false)));
    assert_eq!(bed.hp.desktop().0, 0, "the screen stays");
    assert_eq!(bed.focused().as_deref(), Some("test.here"));
    // There, it shows.
    bed.hp.pointer_move(400.0, 400.0);
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_3]);
    assert_eq!(bed.on("test.mail"), Some((2, true)));
    assert!(rect_of(&bed.hp, "test.mail").is_some());
    Ok(())
}

#[test]
fn a_window_opens_at_its_rules_size_from_its_first_frame() -> anyhow::Result<()> {
    let rules = [
        rule("test.max", "size = \"maximized\""),
        rule("test.left", "size = \"left\""),
        rule("test.right", "size = \"right\""),
        rule("test.full", "size = \"fullscreen\""),
    ]
    .concat();
    let mut bed = Bed::start(&rules)?;
    // As mapped: its first buffer already that size.
    let max = open(&mut bed.hp, "test.max", (400, 300))?;
    assert_eq!(max, AREA);
    let left = open(&mut bed.hp, "test.left", (400, 300))?;
    assert_eq!(left, Zone::Left.rect(AREA));
    let right = open(&mut bed.hp, "test.right", (400, 300))?;
    assert_eq!(right, Zone::Right.rect(AREA));
    let full = open(&mut bed.hp, "test.full", (400, 300))?;
    assert_eq!(full, Rect::new(0, 0, 1920, 1080));
    // Without a rule, its own size.
    let plain = open(&mut bed.hp, "test.plain", (400, 300))?;
    assert_eq!((plain.width, plain.height), (400, 300));
    Ok(())
}

#[test]
fn a_window_kept_above_stays_on_top_until_a_fullscreen_one_covers_it() -> anyhow::Result<()> {
    let mut bed = Bed::start(&rule("test.above", "above = true"))?;
    open(&mut bed.hp, "test.above", (400, 300))?;
    let (_, other) = open_with(&mut bed.hp, "test.other", (600, 400), false)?;
    // The other has the focus, the one kept above stays on top.
    assert_eq!(bed.focused().as_deref(), Some("test.other"));
    assert_eq!(bed.stack(), ["test.other", "test.above"]);
    // Fullscreen in front, the other covers it.
    other.commands.send(Command::Fullscreen(true))?;
    let full = Rect::new(0, 0, 1920, 1080);
    common::wait_for_rect(&mut bed.hp, "test.other", full)?;
    assert_eq!(bed.stack(), ["test.above", "test.other"]);
    other.commands.send(Command::Fullscreen(false))?;
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| rect_of(hp, "test.other") != Some(full))?;
    anyhow::ensure!(back, "still fullscreen");
    // The rule's `above` off in the file: it stacks as the others.
    bed.write(&rule("test.above", "above = false"))?;
    let other = bed
        .hp
        .windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some("test.other"));
    let at = other.context("no test.other")?.rect.centre();
    bed.hp.pointer_move(at.0, at.1);
    common::click(&mut bed.hp, at)?;
    assert_eq!(bed.stack(), ["test.above", "test.other"]);
    // On again: on top at once.
    bed.write(&rule("test.above", "above = true"))?;
    assert_eq!(bed.stack(), ["test.other", "test.above"]);
    Ok(())
}

/// A window of the test's that exports itself, and its handle.
fn exported(hp: &mut Headless) -> anyhow::Result<(ClientHandle, String)> {
    let (_, parent) = open_with(hp, "test.parent", (600, 400), false)?;
    parent.commands.send(Command::Export)?;
    let mut handle = None;
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = parent.events.try_recv() {
            if let Event::Exported(h) = event {
                handle = Some(h);
            }
        }
        handle.is_some()
    })?;
    anyhow::ensure!(came, "never exported");
    Ok((parent, handle.context("no handle")?))
}

#[test]
fn a_dialog_follows_its_parent_not_the_rule() -> anyhow::Result<()> {
    let mut bed = Bed::start(&rule("test.dialog", "desktop = 3\nsize = \"maximized\""))?;
    let (_parent, handle) = exported(&mut bed.hp)?;
    let dialog = common::client(bed.hp.socket(), None)?;
    dialog.commands.send(Command::OpenDialog {
        app_id: "test.dialog",
        size: (300, 200),
        handle,
    })?;
    let mapped = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.all_windows().len() == 2)?;
    anyhow::ensure!(mapped, "no dialog");
    assert_eq!(
        bed.on("test.dialog"),
        Some((0, true)),
        "on its parent's desktop"
    );
    let rect = rect_of(&bed.hp, "test.dialog").context("not shown")?;
    assert_eq!((rect.width, rect.height), (300, 200));
    Ok(())
}
