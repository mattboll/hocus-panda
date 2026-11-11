//! The magnifier (#424, docs/design/accessibility.md): `Super+=`,
//! `Super+-`, `Super+Escape` and `Super` with the wheel; the screen's
//! pixels drawn magnified around the pointer at 2× and 8×; the view
//! pushed at its edge, centred or proportional; jumping to the keyboard's
//! focus and the text caret; a click reaching the window under the
//! pointer; `Super+minus` on AZERTY, which shares its key with `6`, not
//! going to desktop 6; the file turning it on; and no frame at rest while
//! it is on.
//!
//! Pixman here, the same pixels everywhere; the pixels' test again with
//! GLES on Mesa's software device, where there is one.

use crate::common;

use std::{path::PathBuf, time::Duration};

use common::{
    BTN_LEFT, Event, KEY_ESC, KEY_LEFTALT, KEY_LEFTMETA, KEY_TAB, Pointed, Shows, WAIT, chord,
    open, open_showing,
};
use hocus_compositor::{
    Config, Desktops, FollowPointer, Headless, HeadlessConfig, MagnifierSettings, RendererChoice,
};

const KEY_EQUAL: u32 = 13;
const KEY_MINUS: u32 = 12;
const KEY_6: u32 = 7;

/// The magnifier's zoom and view (x, y, width, height), as it goes.
fn seen(hp: &Headless) -> Option<(f64, (f64, f64, f64, f64))> {
    hp.state()
        .magnifier_seen()
        .map(|(_, zoom, view)| (zoom, view))
}

fn zoom(hp: &Headless) -> Option<f64> {
    seen(hp).map(|(z, _)| z)
}

fn pixman() -> HeadlessConfig {
    HeadlessConfig {
        renderer: RendererChoice::Pixman,
        ..HeadlessConfig::default()
    }
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    file: PathBuf,
}

/// The file the tests that follow it start with: what `Config` says too.
const FILE: &str = "[keyboard]\nlayouts = [\"us\"]\n";

impl Bed {
    /// A compositor whose configuration `edit` makes; with `followed`, its
    /// file is followed, starting as [`FILE`] (which `edit` must agree
    /// with: the file read at the start replaces it).
    fn start(
        renderer: HeadlessConfig,
        followed: bool,
        edit: impl FnOnce(&mut Config),
    ) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, FILE)?;
        let mut config = Config {
            file: followed.then(|| file.clone()),
            plain_background: true,
            ..Config::default()
        };
        config.keyboard.layouts = vec!["us".to_owned()];
        edit(&mut config);
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                ..renderer
            },
            config,
        )?;
        Ok(Self {
            _dir: dir,
            hp,
            file,
        })
    }

    fn pixman(edit: impl FnOnce(&mut Config)) -> anyhow::Result<Self> {
        Self::start(pixman(), false, edit)
    }

    /// The file now says [`FILE`] and `text`, renamed over the old as the
    /// Settings write it; once the compositor has read it, as `read` says.
    fn write(&mut self, text: &str, read: impl Fn(&Config) -> bool) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, format!("{FILE}{text}"))?;
        std::fs::rename(&new, &self.file)?;
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| read(&hp.state().config))?;
        anyhow::ensure!(done, "the file is not read: {text}");
        Ok(())
    }

    /// `Super` and `key`.
    fn super_key(&mut self, key: u32) {
        chord(&mut self.hp, &[KEY_LEFTMETA, key]);
    }

    /// The held clock stepped `time` on, a tenth of a second at a time:
    /// the pill and the minimap gone after 2 s.
    fn pass(&mut self, time: Duration) -> anyhow::Result<()> {
        self.hp.hold_clock();
        let step = Duration::from_millis(100);
        let mut done = Duration::ZERO;
        while done < time {
            self.hp.step(step)?;
            done += step;
        }
        Ok(())
    }
}

#[test]
fn the_keys_step_the_zoom_and_escape_turns_it_off() -> anyhow::Result<()> {
    let mut bed = Bed::pixman(|_| {})?;
    bed.hp.pointer_move(960.0, 540.0);
    assert_eq!(zoom(&bed.hp), None);
    // Super+- with it off: nothing.
    bed.super_key(KEY_MINUS);
    assert_eq!(zoom(&bed.hp), None);
    bed.super_key(KEY_EQUAL);
    assert_eq!(zoom(&bed.hp), Some(1.5));
    bed.super_key(KEY_EQUAL);
    let (z, view) = seen(&bed.hp).expect("on");
    assert_eq!(z, 2.0);
    // About the pointer, in the middle: the middle quarter.
    assert_eq!(view, (480.0, 270.0, 960.0, 540.0));
    bed.super_key(KEY_MINUS);
    assert_eq!(zoom(&bed.hp), Some(1.5));
    bed.super_key(KEY_MINUS);
    assert_eq!(zoom(&bed.hp), None, "off at 1×");
    // Up to 8×, then off: back at 8× with the next Super+=.
    for _ in 0..6 {
        bed.super_key(KEY_EQUAL);
    }
    assert_eq!(zoom(&bed.hp), Some(8.0));
    bed.super_key(KEY_ESC);
    assert_eq!(zoom(&bed.hp), None);
    bed.super_key(KEY_EQUAL);
    assert_eq!(zoom(&bed.hp), Some(8.0), "back at its last zoom");
    // Never past 16×.
    for _ in 0..4 {
        bed.super_key(KEY_EQUAL);
    }
    assert_eq!(zoom(&bed.hp), Some(16.0));
    // `hocus msg outputs` says it.
    let said = bed.hp.outputs_said();
    assert!(said.contains("\"magnifier\":{"), "{said}");
    assert!(said.contains("\"zoom\":16.0"), "{said}");
    // Locked, the keys still work.
    bed.hp.lock();
    bed.super_key(KEY_MINUS);
    assert_eq!(zoom(&bed.hp), Some(12.0), "locked");
    bed.super_key(KEY_ESC);
    assert_eq!(zoom(&bed.hp), None, "locked");
    Ok(())
}

#[test]
fn super_and_the_wheel_zoom_by_ten_percent() -> anyhow::Result<()> {
    let mut bed = Bed::pixman(|_| {})?;
    bed.hp.pointer_move(960.0, 540.0);
    // Without Super, the wheel scrolls.
    bed.hp.wheel(-1);
    assert_eq!(zoom(&bed.hp), None);
    bed.hp.key(KEY_LEFTMETA, true);
    bed.hp.wheel(-1);
    assert!(
        zoom(&bed.hp).is_some_and(|z| near(z, 1.1)),
        "{:?}",
        zoom(&bed.hp)
    );
    bed.hp.wheel(-1);
    assert!(
        zoom(&bed.hp).is_some_and(|z| near(z, 1.21)),
        "{:?}",
        zoom(&bed.hp)
    );
    // Super+= from between two steps: the next.
    bed.hp.key(KEY_EQUAL, true);
    bed.hp.key(KEY_EQUAL, false);
    assert_eq!(zoom(&bed.hp), Some(1.5));
    // Down to 1×: off.
    bed.hp.wheel(5);
    assert_eq!(zoom(&bed.hp), None);
    bed.hp.key(KEY_LEFTMETA, false);
    Ok(())
}

/// What an ARGB screenshot shows at (`x`, `y`).
fn pixel((w, _, px): &(i32, i32, Vec<u8>), x: i32, y: i32) -> [u8; 4] {
    let at = ((y * w + x) * 4) as usize;
    [px[at], px[at + 1], px[at + 2], px[at + 3]]
}

/// The screen at `zoom` about the pointer at `pointer` shows the 1×
/// screen's pixels of its view, each `zoom` times larger: hard pixels
/// (`smooth = false`), every pixel but the cursor's, the pill and the
/// minimap gone.
fn magnified_pixels(renderer: HeadlessConfig) -> anyhow::Result<()> {
    let mut bed = Bed::start(renderer, false, |c| {
        c.magnifier = MagnifierSettings {
            smooth: false,
            ..MagnifierSettings::default()
        };
    })?;
    // Windows with their rims and shadows, at places of their own.
    open(&mut bed.hp, "test.one", (500, 400))?;
    open(&mut bed.hp, "test.two", (300, 200))?;
    // Views on whole pixels: (960, 544) at 2× and 8×.
    let pointer = (960.0, 544.0);
    bed.hp.pointer_move(pointer.0, pointer.1);
    common::at_rest(&mut bed.hp)?;
    let base = bed.hp.screenshot(0)?;
    let mut zoom_now = 1.0;
    for (zoom, presses) in [(2.0, 2), (8.0, 4)] {
        for _ in 0..presses {
            bed.super_key(KEY_EQUAL);
        }
        let (z, view) = seen(&bed.hp).expect("on");
        assert_eq!(z, zoom);
        zoom_now = z;
        bed.pass(Duration::from_millis(2500))?;
        common::at_rest(&mut bed.hp)?;
        let shot = bed.hp.screenshot(0)?;
        // Where the pointer shows: the cursor's image is drawn there, at
        // its size times the zoom.
        let shown = ((pointer.0 - view.0) * zoom, (pointer.1 - view.1) * zoom);
        let cursor = |x: i32, y: i32| {
            let (x, y) = (f64::from(x), f64::from(y));
            x >= shown.0 - 2.0 * zoom
                && x < shown.0 + 40.0 * zoom
                && y >= shown.1 - 2.0 * zoom
                && y < shown.1 + 40.0 * zoom
        };
        let (mut compared, mut differ) = (0, 0);
        for y in (0..1080).step_by(3) {
            for x in (0..1920).step_by(3) {
                if cursor(x, y) {
                    continue;
                }
                let sx = (view.0 + (f64::from(x) + 0.5) / zoom).floor() as i32;
                let sy = (view.1 + (f64::from(y) + 0.5) / zoom).floor() as i32;
                compared += 1;
                if pixel(&shot, x, y) != pixel(&base, sx, sy) {
                    differ += 1;
                }
            }
        }
        // A pixel's edge may round either way.
        assert!(
            differ * 100 <= compared,
            "{zoom}×: {differ} of {compared} pixels differ"
        );
        // Just above and left of the hotspot, what is under the pointer.
        let (hx, hy) = ((shown.0 - zoom) as i32, (shown.1 - zoom) as i32);
        assert_eq!(
            pixel(&shot, hx, hy),
            pixel(&base, pointer.0 as i32 - 1, pointer.1 as i32 - 1)
        );
        // The cursor is larger: something of it far down its slope.
        let far = (
            (shown.0 + 6.0 * zoom) as i32,
            (shown.1 + 12.0 * zoom) as i32,
        );
        assert_ne!(
            pixel(&shot, far.0, far.1),
            pixel(&base, pointer.0 as i32 + 6, pointer.1 as i32 + 12),
            "{zoom}×: no cursor at {far:?}"
        );
    }
    assert_eq!(zoom_now, 8.0);
    // Off: the screen as it was.
    bed.super_key(KEY_ESC);
    common::at_rest(&mut bed.hp)?;
    let back = bed.hp.screenshot(0)?;
    assert!(back.2 == base.2, "the screen is not back at 1×");
    Ok(())
}

#[test]
fn the_screen_is_drawn_magnified_around_the_pointer() -> anyhow::Result<()> {
    magnified_pixels(pixman())
}

#[test]
fn the_screen_is_drawn_magnified_with_gles_too() -> anyhow::Result<()> {
    let gles = common::gles();
    if gles.renderer == RendererChoice::Pixman {
        return Ok(());
    }
    magnified_pixels(gles)
}

#[test]
fn nothing_is_drawn_at_rest_while_magnified() -> anyhow::Result<()> {
    let mut bed = Bed::pixman(|_| {})?;
    open(&mut bed.hp, "test.one", (500, 400))?;
    bed.hp.pointer_move(960.0, 540.0);
    for _ in 0..6 {
        bed.super_key(KEY_EQUAL);
    }
    assert_eq!(zoom(&bed.hp), Some(8.0));
    // The pill and the minimap go.
    bed.pass(Duration::from_millis(2500))?;
    let before = bed.hp.frames();
    bed.pass(Duration::from_secs(5))?;
    assert_eq!(bed.hp.frames(), before, "frames drawn at rest");
    // The pointer moving inside the view: the cursor is drawn, the view
    // stays.
    let view = seen(&bed.hp).expect("on").1;
    bed.hp.pointer_move(965.0, 540.0);
    bed.pass(Duration::from_millis(200))?;
    assert!(bed.hp.frames() > before, "the cursor not drawn");
    assert_eq!(seen(&bed.hp).expect("on").1, view);
    Ok(())
}

#[test]
fn the_view_moves_when_the_pointer_pushes_its_edge_and_by_as_much() -> anyhow::Result<()> {
    let mut bed = Bed::start(pixman(), true, |_| {})?;
    bed.hp.pointer_move(960.0, 540.0);
    bed.super_key(KEY_EQUAL);
    bed.super_key(KEY_EQUAL);
    assert_eq!(seen(&bed.hp).expect("on").1, (480.0, 270.0, 960.0, 540.0));
    // Inside the view: it stays.
    bed.hp.pointer_move(1400.0, 300.0);
    bed.hp.pointer_move(500.0, 800.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (480.0, 270.0, 960.0, 540.0));
    // 50 px past its right edge: 50 px to the right.
    bed.hp.pointer_move(1440.0 + 50.0, 540.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (530.0, 270.0, 960.0, 540.0));
    // Past its top: up by as much; never off the screen.
    bed.hp.pointer_move(1000.0, 200.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (530.0, 200.0, 960.0, 540.0));
    bed.hp.pointer_move(1919.0, 0.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (959.0, 0.0, 960.0, 540.0));
    // Centred, then proportional, from the file.
    bed.write("[magnifier]\nfollow-pointer = \"centre\"\n", |c| {
        c.magnifier.follow_pointer == FollowPointer::Centre
    })?;
    bed.hp.pointer_move(1000.0, 600.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (520.0, 330.0, 960.0, 540.0));
    bed.write("[magnifier]\nfollow-pointer = \"proportional\"\n", |c| {
        c.magnifier.follow_pointer == FollowPointer::Proportional
    })?;
    bed.hp.pointer_move(1440.0, 270.0);
    assert_eq!(seen(&bed.hp).expect("on").1, (720.0, 135.0, 960.0, 540.0));
    Ok(())
}

#[test]
fn the_view_jumps_to_the_keyboard_focus_and_the_caret() -> anyhow::Result<()> {
    let mut bed = Bed::pixman(|_| {})?;
    let one = open(&mut bed.hp, "test.one", (500, 400))?;
    let two = open(&mut bed.hp, "test.two", (300, 200))?;
    // Over the first, clicked: it has the keyboard, the view stays.
    let over = (f64::from(one.x + 20), f64::from(one.y + 20));
    bed.hp.pointer_move(over.0, over.1);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    for _ in 0..6 {
        bed.super_key(KEY_EQUAL);
    }
    let (z, view) = seen(&bed.hp).expect("on");
    assert_eq!(z, 8.0);
    let focused = |hp: &Headless, app: &str| {
        hp.windows()
            .iter()
            .any(|w| w.keyboard && w.app_id.as_deref() == Some(app))
    };
    assert!(focused(&bed.hp, "test.one"));
    // Alt+Tab to the second, away from the view: it jumps there, the
    // window larger than the view at 8× shown from its top left corner.
    bed.hp.key(KEY_LEFTALT, true);
    bed.hp.key(KEY_TAB, true);
    bed.hp.key(KEY_TAB, false);
    bed.hp.key(KEY_LEFTALT, false);
    let moved = bed.hp.dispatch_until(WAIT, |hp| focused(hp, "test.two"))?;
    anyhow::ensure!(moved, "Alt+Tab did not focus the second window");
    let (_, after) = seen(&bed.hp).expect("on");
    assert_ne!(after, view);
    assert_eq!(
        (after.0, after.1),
        (f64::from(two.x), f64::from(two.y)),
        "{two:?}"
    );
    // A pointer move takes over again: back to the pointer, pushed.
    bed.hp.pointer_move(over.0 + 1.0, over.1);
    let (_, back) = seen(&bed.hp).expect("on");
    assert!(
        back.0 <= over.0 + 1.0 && over.0 + 1.0 <= back.0 + back.2,
        "{back:?}"
    );
    // A text field's caret far right in a window opening: the view keeps
    // it 10 % in from its right edge.
    let caret = (700, 100, 2, 20);
    let (rect, _client) = open_showing(
        &mut bed.hp,
        "test.writing",
        Shows::Writing {
            app_id: "test.writing",
            size: (800, 500),
            caret: Some(caret),
        },
    )?;
    let caret_right = f64::from(rect.x + caret.0 + caret.2);
    let followed = bed.hp.dispatch_until(WAIT, |hp| {
        seen(hp).is_some_and(|(_, v)| near(v.0, caret_right - 240.0 + 24.0))
    })?;
    anyhow::ensure!(
        followed,
        "the caret not followed: {:?} {rect:?}",
        seen(&bed.hp)
    );
    Ok(())
}

#[test]
fn a_click_reaches_the_window_under_the_pointer() -> anyhow::Result<()> {
    let mut bed = Bed::pixman(|_| {})?;
    let (rect, client) = open_showing(
        &mut bed.hp,
        "test.pointing",
        Shows::Pointing {
            app_id: "test.pointing",
            size: (600, 400),
        },
    )?;
    let middle = (
        f64::from(rect.x + rect.width / 2),
        f64::from(rect.y + rect.height / 2),
    );
    // Magnified about a point outside it: the window fills part of the
    // view; then the pointer on it, as drawn.
    bed.hp.pointer_move(f64::from(rect.x) - 20.0, middle.1);
    for _ in 0..4 {
        bed.super_key(KEY_EQUAL);
    }
    bed.hp.pointer_move(f64::from(rect.x) + 30.0, middle.1);
    bed.hp.button(BTN_LEFT, true);
    bed.hp.button(BTN_LEFT, false);
    let mut got = Vec::new();
    let pressed = bed.hp.dispatch_until(WAIT, |_| {
        got.extend(client.events.try_iter());
        got.iter()
            .any(|e| matches!(e, Event::Pointer(Pointed::Press)))
    })?;
    assert!(pressed, "{got:?}");
    Ok(())
}

#[test]
fn super_minus_on_azerty_zooms_out_and_does_not_go_to_desktop_6() -> anyhow::Result<()> {
    let desktops = |c: &mut Config| {
        c.desktops = Desktops {
            count: 9,
            ..Desktops::default()
        };
    };
    let mut bed = Bed::pixman(|c| {
        desktops(c);
        c.keyboard.layouts = vec!["fr".to_owned()];
    })?;
    bed.hp.pointer_move(960.0, 540.0);
    // On AZERTY, `=` has a key of its own; `-` is the `6` key's.
    bed.super_key(KEY_EQUAL);
    bed.super_key(KEY_EQUAL);
    assert_eq!(zoom(&bed.hp), Some(2.0));
    bed.super_key(KEY_6);
    assert_eq!(zoom(&bed.hp), Some(1.5));
    assert_eq!(bed.hp.desktop().0, 0, "Super+minus went to desktop 6");
    bed.super_key(KEY_6);
    assert_eq!(zoom(&bed.hp), None);
    // Off, still not desktop 6.
    bed.super_key(KEY_6);
    assert_eq!(bed.hp.desktop().0, 0, "Super+minus went to desktop 6");
    // The other digits' keys still go to their desktops.
    bed.super_key(common::KEY_3);
    let gone = bed.hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 2)?;
    anyhow::ensure!(gone, "Super+3 on AZERTY: {:?}", bed.hp.desktop());
    // On QWERTY, `6` is desktop 6 and `-` its own key.
    let mut bed = Bed::pixman(desktops)?;
    bed.hp.pointer_move(960.0, 540.0);
    bed.super_key(KEY_EQUAL);
    bed.super_key(KEY_MINUS);
    assert_eq!(zoom(&bed.hp), None);
    bed.super_key(KEY_6);
    let gone = bed.hp.dispatch_until(WAIT, |hp| hp.desktop().0 == 5)?;
    anyhow::ensure!(gone, "Super+6 on QWERTY: {:?}", bed.hp.desktop());
    Ok(())
}

#[test]
fn the_file_turns_it_on_at_its_zoom_and_off() -> anyhow::Result<()> {
    let mut bed = Bed::start(pixman(), true, |_| {})?;
    bed.hp.pointer_move(960.0, 540.0);
    bed.write("[magnifier]\nenabled = true\nzoom = 4.0\n", |c| {
        c.magnifier.enabled
    })?;
    assert_eq!(zoom(&bed.hp), Some(4.0));
    bed.write("[magnifier]\nenabled = true\nzoom = 6.0\n", |c| {
        c.magnifier.zoom == 6.0
    })?;
    assert_eq!(zoom(&bed.hp), Some(6.0));
    bed.write("[magnifier]\nenabled = false\nzoom = 6.0\n", |c| {
        !c.magnifier.enabled
    })?;
    assert_eq!(zoom(&bed.hp), None);
    // On from the start.
    let bed = Bed::pixman(|c| {
        c.magnifier.enabled = true;
        c.magnifier.zoom = 3.0;
    })?;
    assert_eq!(zoom(&bed.hp), Some(3.0));
    Ok(())
}

/// The maintainer's screenshots, into `HOCUS_SHOTS`: the bar, a terminal
/// and a window at 1×, then magnified 2× and 8× about the terminal's
/// text, the pill showing. Needs the shell built and `foot` installed.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_maintainer() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let shell = common::shell_program();
    std::fs::create_dir_all(&out)?;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config/hocuspanda", "state", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let at = |sub: &str| root.join(sub).display().to_string();
    let file = root.join("config/hocuspanda/config.toml");
    std::fs::write(&file, FILE)?;
    let script = common::shell_script(
        root,
        &shell,
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_CACHE_HOME", at("cache")),
            ("ICED_BACKEND", "tiny-skia".to_owned()),
            ("LC_ALL", "fr_FR.UTF-8".to_owned()),
            ("LANG", "fr_FR.UTF-8".to_owned()),
        ],
    )?;
    let mut config = Config::load(&file);
    config.keyboard.layouts = vec!["us".to_owned()];
    config.file = Some(file);
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            ..pixman()
        },
        config,
        Some(script),
    )?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    open(&mut hp, "test.window", (520, 360))?;
    // A terminal with text, as an agent's session shows.
    let text = root.join("text");
    std::fs::write(
        &text,
        "$ cargo nextest run -p hocus-compositor --test magnifier\n\
         \x20   Starting 10 tests across 1 binary\n\
         \x20       PASS [   0.014s] magnifier super_and_the_wheel_zoom_by_ten_percent\n\
         \x20       PASS [   0.018s] magnifier the_keys_step_the_zoom_and_escape_turns_it_off\n\
         \x20       PASS [   0.036s] magnifier super_minus_on_azerty_zooms_out\n\
         \x20       PASS [   2.793s] magnifier the_screen_is_drawn_magnified_around_the_pointer\n\
         \x20    Summary [   3.023s] 10 tests run: 10 passed, 0 skipped\n$ ",
    )?;
    let socket = hp.socket().to_path_buf();
    let runtime = socket.parent().map(PathBuf::from).unwrap_or_default();
    let mut foot = std::process::Command::new("foot")
        .args(["--app-id", "foot", "-e", "sh", "-c"])
        .arg(format!("cat '{}'; sleep 600", text.display()))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("WAYLAND_DISPLAY", &socket)
        .env("XDG_RUNTIME_DIR", &runtime)
        .env("HOME", at("home"))
        .env("XDG_CONFIG_HOME", at("config"))
        .env("LANG", "C.UTF-8")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.windows()
            .iter()
            .any(|w| w.app_id.as_deref() == Some("foot"))
    });
    let result = (|| {
        anyhow::ensure!(shown?, "foot never showed");
        let terminal = hp
            .windows()
            .into_iter()
            .find(|w| w.app_id.as_deref() == Some("foot"))
            .map(|w| w.rect)
            .ok_or_else(|| anyhow::anyhow!("no terminal"))?;
        common::wait(&mut hp, Duration::from_secs(2))?;
        // On the terminal's text.
        let pointer = (f64::from(terminal.x + 150), f64::from(terminal.y + 60));
        hp.pointer_move(pointer.0, pointer.1);
        common::at_rest(&mut hp)?;
        let save = |hp: &mut Headless, name: &str| -> anyhow::Result<()> {
            let (w, h, px) = hp.screenshot(0)?;
            let rgba: Vec<u8> = px
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], 255])
                .collect();
            let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba)
                .ok_or_else(|| anyhow::anyhow!("no image"))?;
            let path = out.join(name);
            image.save(&path)?;
            eprintln!("{}", path.display());
            Ok(())
        };
        save(&mut hp, "magnifier-1x.png")?;
        hp.hold_clock();
        for (name, presses) in [("magnifier-2x.png", 2), ("magnifier-8x.png", 4)] {
            for _ in 0..presses {
                chord(&mut hp, &[KEY_LEFTMETA, KEY_EQUAL]);
            }
            // The glide over, the pill still there; then the minimap too
            // with a push of the edge.
            hp.step(Duration::from_millis(300))?;
            save(&mut hp, name)?;
        }
        Ok(())
    })();
    let _ = foot.kill();
    let _ = foot.wait();
    result
}
