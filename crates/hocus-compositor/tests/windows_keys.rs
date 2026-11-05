//! How windows move, snap and switch, and the ink, followed live (6.14,
//! issue #448; docs/design/windows.md §6, §8, §9, §11, ink.md): the keys
//! that move and resize a window from anywhere in it, Super to move
//! turning the ink off; the magnet, snapping by dragging and the wobble
//! off; Alt+Tab listing this desktop only; the ink off passing Super+drag
//! to the window and taking what it shows.
//!
//! The file is written by hand after the start, as the Settings' worker
//! would, and each test waits until the compositor has read it. Motions
//! are seen on the held clock, a step at a time.

mod common;

use std::path::PathBuf;

use common::{
    BTN_LEFT, ClientHandle, Event, KEY_2, KEY_LEFT, KEY_LEFTALT, KEY_LEFTMETA, KEY_LEFTSHIFT,
    KEY_TAB, Pointed, Shows, WAIT, chord, open, open_showing, rect_of, wait_for_rect,
};
use hocus_compositor::{
    AltTab, Config, DragKey, Headless, HeadlessConfig, RendererChoice, Windows,
    model::{GAP, Rect, Zone},
};

const BTN_RIGHT: u32 = 0x111;
/// The work area of the headless 1920×1080 output.
const AREA: Rect = Rect::new(8, 8, 1700, 1064);

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    file: PathBuf,
}

impl Bed {
    fn start() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "")?;
        let config = Config {
            file: Some(file.clone()),
            plain_background: true,
            ..Config::default()
        };
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

    /// The file now says `text`, renamed over the old as the Settings
    /// write it; once the compositor has read it, as `read` says.
    fn write(&mut self, text: &str, read: impl Fn(&Config) -> bool) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, text)?;
        std::fs::rename(&new, &self.file)?;
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| read(&hp.state().config))?;
        anyhow::ensure!(done, "the file is not read: {text}");
        Ok(())
    }

    /// `[windows]` says `line`.
    fn windows(&mut self, line: &str, read: impl Fn(&Windows) -> bool) -> anyhow::Result<()> {
        self.write(&format!("[windows]\n{line}\n"), |c| read(&c.windows))
    }

    /// A window that says what its pointer gets.
    fn pointing(&mut self, app_id: &'static str) -> anyhow::Result<(Rect, ClientHandle)> {
        open_showing(
            &mut self.hp,
            app_id,
            Shows::Pointing {
                app_id,
                size: (600, 400),
            },
        )
    }

    /// A drag of `button` from `from` by (`dx`, `dy`) with `keys` held, in
    /// steps of 10 px.
    fn drag(&mut self, keys: &[u32], button: u32, from: (f64, f64), (dx, dy): (f64, f64)) {
        self.hp.pointer_move(from.0, from.1);
        for key in keys {
            self.hp.key(*key, true);
        }
        self.hp.button(button, true);
        let steps = (dx.hypot(dy) / 10.0).ceil().max(1.0);
        for i in 1..=steps as i32 {
            let t = f64::from(i) / steps;
            self.hp.pointer_move(from.0 + dx * t, from.1 + dy * t);
        }
        self.hp.button(button, false);
        for key in keys.iter().rev() {
            self.hp.key(*key, false);
        }
    }
}

/// What `client`'s pointer got, until `last` came.
fn pointed_until(
    hp: &mut Headless,
    client: &ClientHandle,
    last: Pointed,
) -> anyhow::Result<Vec<Pointed>> {
    let mut seen = Vec::new();
    let came = hp.dispatch_until(WAIT, |_| {
        while let Ok(event) = client.events.try_recv() {
            if let Event::Pointer(p) = event {
                seen.push(p);
            }
        }
        seen.last() == Some(&last)
    })?;
    anyhow::ensure!(came, "{last:?} never came: {seen:?}");
    Ok(seen)
}

fn middle(rect: Rect) -> (f64, f64) {
    let (x, y) = rect.centre();
    (x, y)
}

#[test]
fn super_to_move_moves_by_super_and_the_ink_no_longer_starts() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (start, client) = bed.pointing("test.moved")?;
    bed.windows("move-modifier = \"super\"", |w| {
        w.move_modifier == DragKey::Super
    })?;
    // Super+drag in its content: it moves, nothing drawn.
    bed.drag(&[KEY_LEFTMETA], BTN_LEFT, middle(start), (120.0, 60.0));
    let moved = Rect::new(start.x + 120, start.y + 60, start.width, start.height);
    wait_for_rect(&mut bed.hp, "test.moved", moved)?;
    assert_eq!(bed.hp.ink(), (false, false), "the ink started");
    // Super and Alt held: no laser either.
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.key(KEY_LEFTALT, true);
    bed.hp.pointer_move(500.0, 500.0);
    assert_eq!(bed.hp.ink(), (false, false), "the laser shows");
    bed.hp.key(KEY_LEFTALT, false);
    bed.hp.key(KEY_LEFTMETA, false);
    // Alt+drag is now the application's: the press reaches it.
    while client.events.try_recv().is_ok() {}
    bed.drag(&[KEY_LEFTALT], BTN_LEFT, middle(moved), (80.0, 0.0));
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    assert_eq!(rect_of(&bed.hp, "test.moved"), Some(moved));
    Ok(())
}

#[test]
fn no_key_to_move_leaves_the_drag_to_the_application() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (start, client) = bed.pointing("test.kept")?;
    bed.windows("move-modifier = \"none\"", |w| {
        w.move_modifier == DragKey::None
    })?;
    bed.drag(&[KEY_LEFTALT], BTN_LEFT, middle(start), (100.0, 0.0));
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    assert_eq!(rect_of(&bed.hp, "test.kept"), Some(start));
    // Back to Alt, live: it moves again.
    bed.windows("move-modifier = \"alt\"", |w| {
        w.move_modifier == DragKey::Alt
    })?;
    bed.drag(&[KEY_LEFTALT], BTN_LEFT, middle(start), (100.0, 0.0));
    let moved = Rect::new(start.x + 100, start.y, start.width, start.height);
    wait_for_rect(&mut bed.hp, "test.kept", moved)?;
    Ok(())
}

#[test]
fn alt_and_the_right_button_resize_when_chosen_and_none_resizes_by_the_frame_only()
-> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (start, client) = bed.pointing("test.resized")?;
    bed.windows("resize-modifier = \"alt\"", |w| {
        w.resize_modifier == DragKey::Alt
    })?;
    // Near the bottom-right corner: that corner follows.
    let near = (
        f64::from(start.right() - 40),
        f64::from(start.bottom() - 40),
    );
    bed.drag(&[KEY_LEFTALT], BTN_RIGHT, near, (50.0, 30.0));
    let grown = Rect::new(start.x, start.y, start.width + 50, start.height + 30);
    wait_for_rect(&mut bed.hp, "test.resized", grown)?;
    // Super and the right button: the application's now.
    while client.events.try_recv().is_ok() {}
    let near = (
        f64::from(grown.right() - 40),
        f64::from(grown.bottom() - 40),
    );
    bed.drag(&[KEY_LEFTMETA], BTN_RIGHT, near, (50.0, 30.0));
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    assert_eq!(rect_of(&bed.hp, "test.resized"), Some(grown));
    // No key: Alt and the right button reach the application too.
    bed.windows("resize-modifier = \"none\"", |w| {
        w.resize_modifier == DragKey::None
    })?;
    while client.events.try_recv().is_ok() {}
    bed.drag(&[KEY_LEFTALT], BTN_RIGHT, near, (50.0, 30.0));
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    assert_eq!(rect_of(&bed.hp, "test.resized"), Some(grown));
    Ok(())
}

#[test]
fn without_the_magnet_a_window_stops_where_the_pointer_puts_it() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let a = open(&mut bed.hp, "test.still", (400, 300))?;
    let b = open(&mut bed.hp, "test.free", (400, 300))?;
    bed.windows("magnet = false", |w| !w.magnet)?;
    // 10 px short of the gap right of the other: it stays there.
    let dx = f64::from(a.right() + GAP + 10 - b.x);
    bed.drag(&[KEY_LEFTALT], BTN_LEFT, middle(b), (dx, 3.0));
    let free = Rect::new(a.right() + GAP + 10, b.y + 3, 400, 300);
    wait_for_rect(&mut bed.hp, "test.free", free)?;
    Ok(())
}

#[test]
fn without_snapping_by_drag_an_edge_takes_nothing_and_the_keys_still_snap() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let start = open(&mut bed.hp, "test.unsnapped", (400, 300))?;
    bed.windows("snap-by-drag = false\nmagnet = false", |w| !w.snap_by_drag)?;
    // Held at the left edge well past the preview's 80 ms, on the held
    // clock: no preview, and it floats where it was let go.
    let (x, y) = middle(start);
    bed.hp.hold_clock();
    bed.hp.pointer_move(x, y);
    bed.hp.key(KEY_LEFTALT, true);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.pointer_move(1.0, y);
    for _ in 0..20 {
        bed.hp.step(common::STEP)?;
    }
    bed.hp.button(BTN_LEFT, false);
    bed.hp.key(KEY_LEFTALT, false);
    for _ in 0..20 {
        bed.hp.step(common::STEP)?;
    }
    bed.hp.release_clock();
    let floating = Rect::new(start.x + (1 - x as i32), start.y, 400, 300);
    wait_for_rect(&mut bed.hp, "test.unsnapped", floating)?;
    // Super+← snaps as ever.
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFT]);
    wait_for_rect(&mut bed.hp, "test.unsnapped", Zone::Left.rect(AREA))?;
    Ok(())
}

#[test]
fn without_the_wobble_a_moved_window_does_not_bend() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let start = open(&mut bed.hp, "test.rigid", (400, 300))?;
    let moved_bends = |bed: &mut Bed, from: (f64, f64)| -> anyhow::Result<f64> {
        bed.hp.hold_clock();
        bed.hp.pointer_move(from.0, from.1);
        bed.hp.key(KEY_LEFTALT, true);
        bed.hp.button(BTN_LEFT, true);
        let mut most: f64 = 0.0;
        for i in 1..=10 {
            bed.hp
                .pointer_move(from.0 + 30.0 * f64::from(i), from.1 + 10.0 * f64::from(i));
            bed.hp.step(common::STEP)?;
            most = most.max(bed.hp.bend().unwrap_or(0.0));
        }
        bed.hp.button(BTN_LEFT, false);
        bed.hp.key(KEY_LEFTALT, false);
        for _ in 0..60 {
            bed.hp.step(common::STEP)?;
        }
        bed.hp.release_clock();
        Ok(most)
    };
    let bend = moved_bends(&mut bed, middle(start))?;
    assert!(bend > 2.0, "it does not wobble: {bend}");
    bed.windows("wobble = false", |w| !w.wobble)?;
    let at = rect_of(&bed.hp, "test.rigid").map(middle);
    let bend = moved_bends(&mut bed, at.unwrap_or_default())?;
    assert!(bend < 0.5, "it bends by {bend} px");
    Ok(())
}

#[test]
fn alt_tab_on_this_desktop_lists_its_windows_only() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    // Sent to the second desktop; the first shows.
    open(&mut bed.hp, "test.away", (400, 300))?;
    chord(&mut bed.hp, &[KEY_LEFTMETA, KEY_LEFTSHIFT, KEY_2]);
    open(&mut bed.hp, "test.here", (400, 300))?;
    open(&mut bed.hp, "test.newest", (400, 300))?;
    bed.windows("alt-tab = \"desktop\"", |w| w.alt_tab == AltTab::Desktop)?;
    bed.hp.key(KEY_LEFTALT, true);
    bed.hp.key(KEY_TAB, true);
    bed.hp.key(KEY_TAB, false);
    let (items, _, _) = bed.hp.switcher().ok_or_else(|| anyhow::anyhow!("none"))?;
    bed.hp.key(KEY_LEFTALT, false);
    let away = bed
        .hp
        .all_windows()
        .into_iter()
        .find(|w| w.1.as_deref() == Some("test.away"))
        .map(|w| w.0);
    assert_eq!(items.len(), 2, "{items:?}");
    assert!(!items.iter().any(|i| Some(*i) == away));
    Ok(())
}

#[test]
fn the_ink_off_passes_super_drag_to_the_window_and_takes_what_it_shows() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let (start, client) = bed.pointing("test.drawn")?;
    // A stroke shows.
    bed.drag(&[KEY_LEFTMETA], BTN_LEFT, middle(start), (100.0, 40.0));
    assert!(bed.hp.ink().0, "nothing drawn");
    bed.write("[ink]\nenabled = false\n", |c| !c.ink.enabled)?;
    assert_eq!(bed.hp.ink(), (false, false), "the stroke stays");
    while client.events.try_recv().is_ok() {}
    bed.drag(&[KEY_LEFTMETA], BTN_LEFT, middle(start), (100.0, 40.0));
    let seen = pointed_until(&mut bed.hp, &client, Pointed::Release)?;
    assert!(seen.contains(&Pointed::Press), "{seen:?}");
    assert_eq!(bed.hp.ink(), (false, false), "drawn");
    // On again: Super+drag draws.
    bed.write("[ink]\nenabled = true\n", |c| c.ink.enabled)?;
    bed.drag(&[KEY_LEFTMETA], BTN_LEFT, middle(start), (100.0, 40.0));
    assert!(bed.hp.ink().0, "nothing drawn");
    Ok(())
}
