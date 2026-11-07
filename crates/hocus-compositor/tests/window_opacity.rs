//! A window's opacity (#735, docs/design/windows.md §3): `Super`+`Alt` and
//! the wheel over a window change it by 10 % a notch, from 100 % down to
//! 20 %; `Super` alone stays the magnifier's; the pill shows the
//! percentage, then goes; the window is drawn see-through, over what is
//! under it. Pixman: the same pixels everywhere.

mod common;

use std::time::Duration;

use common::{KEY_LEFTALT, KEY_LEFTMETA, WINDOW_COLOUR, compositor, open, wait};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice, model::WindowId};

/// One pixel of the first screen: red, green, blue.
fn pixel(hp: &mut Headless, (x, y): (i32, i32)) -> anyhow::Result<[i32; 3]> {
    let (width, _, bytes) = hp.screenshot(0)?;
    let at = usize::try_from(y * width + x)? * 4;
    let p = bytes
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("no pixel at {x}, {y}"))?;
    Ok([i32::from(p[2]), i32::from(p[1]), i32::from(p[0])])
}

fn id_of(hp: &Headless, app: &str) -> anyhow::Result<WindowId> {
    hp.windows()
        .into_iter()
        .find(|w| w.app_id.as_deref() == Some(app))
        .map(|w| w.id)
        .ok_or_else(|| anyhow::anyhow!("no window {app}"))
}

/// `notches` of the wheel with `Super`+`Alt` held; down is positive.
fn wheel(hp: &mut Headless, notches: i32) {
    hp.key(KEY_LEFTMETA, true);
    hp.key(KEY_LEFTALT, true);
    hp.wheel(notches);
    hp.key(KEY_LEFTALT, false);
    hp.key(KEY_LEFTMETA, false);
}

#[test]
fn super_alt_and_the_wheel_change_the_window_under_the_pointer() -> anyhow::Result<()> {
    let mut hp = compositor()?;
    let a = open(&mut hp, "test.a", (500, 400))?;
    let b_rect = open(&mut hp, "test.b", (300, 200))?;
    let (a_id, b_id) = (id_of(&hp, "test.a")?, id_of(&hp, "test.b")?);
    // Over the first window, where the second is not.
    let over_a = if b_rect.contains(f64::from(a.x + 20), f64::from(a.y + 20)) {
        (
            f64::from(a.x + a.width - 20),
            f64::from(a.y + a.height - 20),
        )
    } else {
        (f64::from(a.x + 20), f64::from(a.y + 20))
    };
    hp.pointer_move(over_a.0, over_a.1);
    assert_eq!(hp.opacity(a_id), 100);
    assert_eq!(hp.opacity_pill(), None);
    // A notch down: 90 %, the pill says it.
    wheel(&mut hp, 1);
    assert_eq!(hp.opacity(a_id), 90);
    assert_eq!(hp.opacity_pill(), Some((a_id, 90)));
    // Never under 20 %.
    wheel(&mut hp, 12);
    assert_eq!(hp.opacity(a_id), 20);
    // Up, more opaque; never over 100 %.
    wheel(&mut hp, -2);
    assert_eq!(hp.opacity(a_id), 40);
    wheel(&mut hp, -9);
    assert_eq!(hp.opacity(a_id), 100);
    wheel(&mut hp, 5);
    assert_eq!(hp.opacity(a_id), 50);
    // The other window is left as it is.
    assert_eq!(hp.opacity(b_id), 100);
    // Super alone is the magnifier's, not the opacity's.
    hp.key(KEY_LEFTMETA, true);
    hp.wheel(-1);
    hp.key(KEY_LEFTMETA, false);
    assert_eq!(hp.opacity(a_id), 50);
    assert!(hp.state().magnifier_seen().is_some(), "the magnifier zooms");
    // The pill goes after 2 s; the opacity stays.
    wait(&mut hp, Duration::from_millis(2300))?;
    assert_eq!(hp.opacity_pill(), None);
    assert_eq!(hp.opacity(a_id), 50);
    Ok(())
}

#[test]
fn the_window_is_drawn_see_through() -> anyhow::Result<()> {
    let mut hp = Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config {
            plain_background: true,
            ..Config::default()
        },
    )?;
    let rect = open(&mut hp, "test.a", (600, 400))?;
    let id = id_of(&hp, "test.a")?;
    // Away from the pill, in the window's middle.
    let at = (rect.x + 60, rect.y + rect.height - 60);
    let below = (rect.x + rect.width + 200, rect.y + rect.height - 60);
    wait(&mut hp, Duration::from_millis(400))?;
    let window = [
        ((WINDOW_COLOUR >> 16) & 0xff) as i32,
        ((WINDOW_COLOUR >> 8) & 0xff) as i32,
        (WINDOW_COLOUR & 0xff) as i32,
    ];
    let seen = pixel(&mut hp, at)?;
    assert_eq!(seen, window, "opaque at first");
    let background = pixel(&mut hp, below)?;
    // The pointer over the window, away from where the pixel is read.
    hp.pointer_move(f64::from(rect.x + rect.width - 60), f64::from(at.1));
    wheel(&mut hp, 5);
    assert_eq!(hp.opacity(id), 50);
    wait(&mut hp, Duration::from_millis(200))?;
    let seen = pixel(&mut hp, at)?;
    for c in 0..3 {
        let want = (window[c] + background[c]) / 2;
        assert!(
            (seen[c] - want).abs() <= 4,
            "{seen:?}, not half of {window:?} over {background:?}"
        );
    }
    Ok(())
}

/// For the maintainer (`HOCUS_SHOTS=DIR`): the desktop with the shell, a
/// window behind, and a terminal at 60 % over it and the wallpaper, the
/// pill showing; then the pill gone. Needs the shell built and `foot`
/// installed.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_for_the_maintainer() -> anyhow::Result<()> {
    use std::path::PathBuf;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let Some(shell) = common::shell_program() else {
        eprintln!("hocuspanda-shell is not built");
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "config/hocuspanda", "state", "cache"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let at = |sub: &str| root.join(sub).display().to_string();
    let file = root.join("config/hocuspanda/config.toml");
    std::fs::write(&file, "[keyboard]\nlayouts = [\"us\"]\n")?;
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
    config.file = Some(file);
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    common::drawn(&mut hp, hocus_link::NAMESPACE)?;
    let behind = open(&mut hp, "test.behind", (700, 500))?;
    let text = root.join("text");
    std::fs::write(
        &text,
        "$ cargo nextest run -p hocus-compositor --test window_opacity\n\
         \x20   Starting 2 tests across 1 binary\n\
         \x20       PASS [   0.412s] window_opacity the_window_is_drawn_see_through\n\
         \x20       PASS [   2.471s] window_opacity super_alt_and_the_wheel_change_the_window\n\
         \x20    Summary [   2.480s] 2 tests run: 2 passed, 0 skipped\n$ ",
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
    let result = (|| {
        let shown = hp.dispatch_until(common::WAIT, |hp| {
            hp.windows()
                .iter()
                .any(|w| w.app_id.as_deref() == Some("foot"))
        })?;
        anyhow::ensure!(shown, "foot never showed");
        let id = id_of(&hp, "foot")?;
        // The terminal half over the window behind, half over the
        // wallpaper.
        hp.place_window(
            id,
            hocus_compositor::model::Rect::new(
                behind.x + behind.width - 380,
                behind.y + 160,
                760,
                420,
            ),
        );
        wait(&mut hp, Duration::from_secs(2))?;
        let rect = common::rect_of(&hp, "foot").ok_or_else(|| anyhow::anyhow!("no terminal"))?;
        hp.pointer_move(
            f64::from(rect.x + rect.width - 80),
            f64::from(rect.y + rect.height - 60),
        );
        wheel(&mut hp, 4);
        anyhow::ensure!(hp.opacity(id) == 60, "{}", hp.opacity(id));
        wait(&mut hp, Duration::from_millis(300))?;
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
        save(&mut hp, "opacity-pill.png")?;
        wait(&mut hp, Duration::from_millis(2600))?;
        save(&mut hp, "opacity-60.png")?;
        Ok(())
    })();
    let _ = foot.kill();
    let _ = foot.wait();
    result
}
