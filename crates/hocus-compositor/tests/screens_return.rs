//! Screens plugged and unplugged, windows coming back where they were
//! (docs/design/windows.md §12, issue #417), headless: a screen is known
//! by its EDID's make, model and serial (the harness gives one), so that
//! its windows go back to it on any connector and never to another screen
//! on its cable; they fly back in 320 ms (in place with reduced motion);
//! each set of screens keeps its arrangement from `[[screen-sets]]`; the
//! lid closed with another screen on takes the laptop's screen away as
//! an unplug; the winking panda says what moved, for 4 s.

use crate::common;

use std::{os::unix::fs::PermissionsExt, path::PathBuf, time::Duration};

use anyhow::Context;
use common::{KEY_LEFT, KEY_LEFTMETA, KEY_RIGHT, WAIT, chord, open, two_screens, wait};
use hocus_compositor::{
    Config, Headless, HeadlessConfig, OutputSpec, Placed, RendererChoice, ScreenIdentity,
    model::{Mode, Rect, Zone},
};

const FIRST: &str = "HEADLESS-1";
const SECOND: &str = "HEADLESS-2";

fn placed(hp: &Headless, app_id: &str) -> Placed {
    hp.placements()
        .into_iter()
        .find(|p| p.app_id.as_deref() == Some(app_id))
        .unwrap_or_else(|| panic!("no window {app_id}"))
}

fn geometry(hp: &Headless, name: &str) -> Option<Rect> {
    hp.outputs()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, g)| g)
}

fn external() -> OutputSpec {
    OutputSpec::new(2560, 1440).physical_size(597, 336)
}

fn dell() -> ScreenIdentity {
    ScreenIdentity::new("DEL", "U3423WE", "7XY1234")
}

fn other() -> ScreenIdentity {
    ScreenIdentity::new("GSM", "LG ULTRAFINE", "205NTAB1C123")
}

/// Two windows halved on the second screen, desktop 1, the second one
/// focused; the screen known as `identity`.
fn halves_on(hp: &mut Headless, identity: ScreenIdentity) -> anyhow::Result<(Placed, Placed)> {
    hp.unplug(SECOND)?;
    hp.plug_screen(SECOND, external(), Some(identity))?;
    let area = hp.work_area_of(SECOND).expect("an area");
    hp.pointer_move(f64::from(area.x + 200), f64::from(area.y + 200));
    open(hp, "test.left", (600, 400))?;
    chord(hp, &[KEY_LEFTMETA, KEY_LEFT]);
    open(hp, "test.right", (600, 400))?;
    chord(hp, &[KEY_LEFTMETA, KEY_RIGHT]);
    wait(hp, Duration::from_millis(50))?;
    let left = placed(hp, "test.left");
    let right = placed(hp, "test.right");
    assert_eq!(left.mode, Mode::Snapped(Zone::Left));
    assert_eq!(right.mode, Mode::Snapped(Zone::Right));
    assert_eq!(left.output.as_deref(), Some(SECOND));
    Ok((left, right))
}

fn focused(hp: &Headless) -> Option<String> {
    hp.windows()
        .into_iter()
        .find(|w| w.focused)
        .and_then(|w| w.app_id)
}

/// Acceptance 1, 2 and 4: unplugged, the windows go to the screen still
/// there, in its halves, the focus kept; another screen on the same cable
/// takes none of them, and is a set of its own; their screen back on
/// another connector, they go back to it, at their places.
#[test]
fn a_screen_is_known_by_who_it_is_not_by_its_cable() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    let (left, right) = halves_on(&mut hp, dell())?;
    // And one floating, on another desktop.
    open(&mut hp, "test.floating", (900, 600))?;
    chord(
        &mut hp,
        &[KEY_LEFTMETA, common::KEY_LEFTSHIFT, common::KEY_2],
    );
    assert_eq!(hp.screen_key(SECOND), "DEL U3423WE 7XY1234");
    assert_eq!(hp.screen_key(FIRST), FIRST, "no EDID: its connector");
    assert_eq!(hp.screen_set(), ["DEL U3423WE 7XY1234", FIRST]);
    let area1 = hp.work_area_of(FIRST).expect("an area");
    hp.unplug(SECOND)?;
    assert_eq!(placed(&hp, "test.left").rect, Zone::Left.rect(area1));
    assert_eq!(placed(&hp, "test.right").rect, Zone::Right.rect(area1));
    assert_eq!(placed(&hp, "test.left").desktop, 0);
    assert_eq!(focused(&hp).as_deref(), Some("test.right"));
    assert_eq!(hp.screen_set(), [FIRST]);

    // Another screen on its cable: nothing moves to it.
    hp.plug_screen(SECOND, external(), Some(other()))?;
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.screen_key(SECOND), "GSM LG ULTRAFINE 205NTAB1C123");
    assert_eq!(placed(&hp, "test.left").output.as_deref(), Some(FIRST));
    assert_eq!(placed(&hp, "test.right").output.as_deref(), Some(FIRST));
    // It lies right of the first, tops aligned.
    let first = geometry(&hp, FIRST).expect("the first");
    assert_eq!(
        geometry(&hp, SECOND).map(|g| (g.x, g.y)),
        Some((first.right(), 0))
    );
    // The windows wait for theirs, as the Settings are told.
    let sets = hp.screen_sets_said();
    let waiting: Vec<u64> = sets
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| {
            s["screens"]
                .as_array()
                .is_some_and(|l| l.iter().any(|k| k["key"] == "DEL U3423WE 7XY1234"))
        })
        .filter_map(|s| s["waiting"].as_u64())
        .collect();
    assert_eq!(waiting, [3], "{sets:#}");
    hp.unplug(SECOND)?;

    // Theirs, on another connector: they go back to it.
    hp.plug_screen("HEADLESS-3", external(), Some(dell()))?;
    wait(&mut hp, Duration::from_millis(50))?;
    let area3 = hp.work_area_of("HEADLESS-3").expect("an area");
    let back_left = placed(&hp, "test.left");
    assert_eq!(back_left.output.as_deref(), Some("HEADLESS-3"));
    assert_eq!(back_left.rect, Zone::Left.rect(area3));
    assert_eq!(back_left.mode, left.mode);
    assert_eq!(placed(&hp, "test.right").rect, Zone::Right.rect(area3));
    assert_eq!(
        placed(&hp, "test.floating").output.as_deref(),
        Some("HEADLESS-3")
    );
    assert_eq!(placed(&hp, "test.floating").desktop, 1);
    assert_eq!(right.desktop, placed(&hp, "test.right").desktop);
    Ok(())
}

/// Acceptance 2: back, each window shown flies from where it was to its
/// place in 320 ms along the desktops' curve: half way at 160 ms, there
/// at 320 ms. With reduced motion it is there at once (it fades in).
#[test]
fn windows_fly_back_to_their_screen_in_320_ms() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    let opened = open(&mut hp, "test.flies", (600, 400))?;
    hp.unplug(SECOND)?;
    wait(&mut hp, Duration::from_millis(50))?;
    let away = placed(&hp, "test.flies").rect;
    hp.hold_clock();
    hp.plug(SECOND)?;
    let there = placed(&hp, "test.flies").rect;
    assert_eq!(there, opened, "its place is its own again at once");
    assert_eq!(
        hp.drawn_rect("test.flies"),
        Some(away),
        "drawn where it was"
    );
    hp.step(Duration::from_millis(160))?;
    let half = hp.drawn_rect("test.flies").expect("drawn");
    let between = |a: i32, b: i32, x: i32| (a.min(b) < x) && (x < a.max(b));
    assert!(
        between(away.x, there.x, half.x),
        "{half:?} between {away:?} and {there:?}"
    );
    // The curve eases out: past half way at half the time.
    assert!(
        (half.x - away.x).abs() * 2 > (there.x - away.x).abs(),
        "{half:?}"
    );
    hp.step(Duration::from_millis(160))?;
    assert_eq!(hp.drawn_rect("test.flies"), Some(there));
    hp.release_clock();

    // Reduced motion: in place at once.
    let mut config = Config::default();
    config.appearance.reduced_motion = true;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "test.fades", (600, 400))?;
    hp.unplug(SECOND)?;
    hp.hold_clock();
    hp.plug(SECOND)?;
    let there = placed(&hp, "test.fades").rect;
    assert_eq!(hp.drawn_rect("test.fades"), Some(there));
    Ok(())
}

/// A configuration file of the test's own holding `text`.
fn with_file(text: &str) -> anyhow::Result<(tempfile::TempDir, Config)> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    std::fs::write(&file, text)?;
    let mut config: Config = toml::from_str(text)?;
    config.file = Some(file);
    config.state_dir = Some(dir.path().join("state"));
    Ok((dir, config))
}

/// Acceptance 5: each set keeps its own arrangement. The file arranges
/// the first screen with the Dell above it, and the first with the LG
/// on its left; `[outputs.NAME]` stays what a set not in the file takes.
#[test]
fn each_set_of_screens_takes_its_own_arrangement() -> anyhow::Result<()> {
    let text = "[outputs.HEADLESS-1]\nscale = 1\n\n\
        [[screen-sets]]\n\
        screens = [\"HEADLESS-1\", \"DEL U3423WE 7XY1234\"]\n\
        primary = \"DEL U3423WE 7XY1234\"\n\
        [screen-sets.outputs.HEADLESS-1]\nposition = [320, 1440]\n\
        [screen-sets.outputs.\"DEL U3423WE 7XY1234\"]\nposition = [0, 0]\n\n\
        [[screen-sets]]\n\
        screens = [\"HEADLESS-1\", \"GSM LG ULTRAFINE 205NTAB1C123\"]\n\
        [screen-sets.outputs.HEADLESS-1]\nposition = [1280, 0]\n\
        [screen-sets.outputs.\"GSM LG ULTRAFINE 205NTAB1C123\"]\nposition = [0, 0]\nscale = 2\n";
    let (_dir, config) = with_file(text)?;
    assert_eq!(config.screen_sets.len(), 2);
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let settle = |hp: &mut Headless| wait(hp, Duration::from_millis(50));
    settle(&mut hp)?;
    // Neither set: left to right.
    assert_eq!(geometry(&hp, SECOND).map(|g| g.x), Some(1920));
    // The Dell: above, as its set says.
    hp.unplug(SECOND)?;
    hp.plug_screen(SECOND, external(), Some(dell()))?;
    settle(&mut hp)?;
    assert_eq!(geometry(&hp, SECOND), Some(Rect::new(0, 0, 2560, 1440)));
    assert_eq!(geometry(&hp, FIRST).map(|g| (g.x, g.y)), Some((320, 1440)));
    let primary = |hp: &Headless| {
        hp.state().displays_json()["outputs"]
            .as_array()
            .and_then(|o| o.iter().find(|o| o["primary"] == true))
            .and_then(|o| o["name"].as_str().map(ToOwned::to_owned))
    };
    assert_eq!(primary(&hp).as_deref(), Some(SECOND), "the set's primary");
    // The LG on the same cable: on the left, at 2.
    hp.unplug(SECOND)?;
    hp.plug_screen(SECOND, external(), Some(other()))?;
    settle(&mut hp)?;
    assert_eq!(geometry(&hp, SECOND), Some(Rect::new(0, 0, 1280, 720)));
    assert_eq!(geometry(&hp, FIRST).map(|g| (g.x, g.y)), Some((1280, 0)));
    assert_eq!(primary(&hp).as_deref(), Some(FIRST), "the panel's own rule");
    // The Dell again: above again.
    hp.unplug(SECOND)?;
    hp.plug_screen(SECOND, external(), Some(dell()))?;
    settle(&mut hp)?;
    assert_eq!(geometry(&hp, SECOND), Some(Rect::new(0, 0, 2560, 1440)));
    // The sets seen, the one in use first, as the Settings are told them.
    let sets = hp.screen_sets_said();
    let list = sets.as_array().cloned().unwrap_or_default();
    assert!(list.len() >= 3, "{sets:#}");
    assert_eq!(list[0]["current"], true);
    assert_eq!(list[0]["in_file"], true);
    assert_eq!(
        list[0]["name"],
        format!("DEL U3423WE 7XY1234\n{FIRST}"),
        "{sets:#}"
    );
    Ok(())
}

/// The Settings forget a set they no longer want listed; the set in use
/// stays. The sets seen are kept in the state directory.
#[test]
fn a_set_forgotten_is_no_longer_listed() -> anyhow::Result<()> {
    let (dir, config) = with_file("")?;
    let mut hp = Headless::start_with(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
    )?;
    let names = |hp: &Headless| -> Vec<String> {
        hp.screen_sets_said()
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["name"].as_str().map(ToOwned::to_owned))
            .collect()
    };
    wait(&mut hp, Duration::from_millis(50))?;
    hp.unplug(SECOND)?;
    hp.plug_screen(SECOND, external(), Some(dell()))?;
    wait(&mut hp, Duration::from_millis(50))?;
    let pair = format!("DEL U3423WE 7XY1234\n{FIRST}");
    let both = format!("{FIRST}\n{SECOND}");
    assert!(names(&hp).contains(&both), "{:?}", names(&hp));
    assert_eq!(hp.forget_screen_set(&[FIRST, SECOND]), Ok(()));
    assert!(!names(&hp).contains(&both), "{:?}", names(&hp));
    assert!(
        hp.forget_screen_set(&[FIRST, "DEL U3423WE 7XY1234"])
            .is_err()
    );
    assert!(names(&hp).contains(&pair));
    // Written for the next session, on a thread.
    let file = dir.path().join("state/screen-sets");
    let written = hp.dispatch_until(WAIT, |_| {
        std::fs::read_to_string(&file).is_ok_and(|t| t.contains("U3423WE") && !t.contains(SECOND))
    })?;
    anyhow::ensure!(written, "{:?}", std::fs::read_to_string(&file));
    Ok(())
}

/// Acceptance 7: the lid closed with another screen on takes the
/// laptop's screen away as an unplug, its windows with it; opened, they
/// come back.
#[test]
fn the_lid_closed_with_another_screen_on_is_an_unplug() -> anyhow::Result<()> {
    let mut hp = two_screens()?;
    hp.set_internal(FIRST);
    hp.pointer_move(960.0, 540.0);
    let opened = open(&mut hp, "test.laptop", (600, 400))?;
    hp.lid(true);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.screens_on(), [SECOND]);
    let moved = placed(&hp, "test.laptop");
    assert_eq!(moved.output.as_deref(), Some(SECOND));
    assert_eq!(hp.screen_set(), [SECOND]);
    hp.lid(false);
    wait(&mut hp, Duration::from_millis(50))?;
    assert_eq!(hp.screens_on(), [FIRST, SECOND]);
    let back = placed(&hp, "test.laptop");
    assert_eq!(back.output.as_deref(), Some(FIRST));
    assert_eq!(back.rect, opened);
    Ok(())
}

/// The shell, logging what it shows into `log`; the applications' names
/// from no desktop entry but their ids, whatever the machine has
/// installed (a CI runner's `firefox.desktop` names it "Navigateur Web
/// Firefox" in French, #731).
fn shell_script(dir: &std::path::Path, log: &std::path::Path) -> Option<PathBuf> {
    let shell = common::shell_program();
    let script = dir.join("shell");
    let home = dir.join("home");
    std::fs::create_dir_all(&home).ok()?;
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}' XDG_CACHE_HOME='{home}' \
             XDG_STATE_HOME='{home}' XDG_DATA_HOME='{home}' XDG_DATA_DIRS='{home}' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            shell = shell.display(),
            log = log.display()
        ),
    )
    .ok()?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).ok()?;
    Some(script)
}

/// Acceptance 6: the winking panda says the windows moved, on the bar,
/// for 4 s on the compositor's clock; not when no window moved, not while
/// locked.
#[test]
fn the_winking_panda_says_what_moved_for_4_s() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let log = dir.path().join("shell.log");
    let script = shell_script(dir.path(), &log).context("the shell's script is not written")?;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec![OutputSpec::new(1920, 1080), OutputSpec::new(2560, 1440)],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config::default(),
        Some(script),
    )?;
    let bar = hp.dispatch_until(WAIT, |hp| !hp.bar_surfaces().is_empty())?;
    anyhow::ensure!(bar, "no bar: {}", common::logged(&log));
    // No window: nothing to say.
    hp.unplug(SECOND)?;
    wait(&mut hp, Duration::from_millis(100))?;
    assert!(!hp.screens_peek_out());
    hp.plug_screen(SECOND, external(), Some(dell()))?;
    hp.pointer_move(1920.0 + 1280.0, 720.0);
    open(&mut hp, "firefox", (600, 400))?;
    open(&mut hp, "foot", (600, 400))?;
    hp.unplug(SECOND)?;
    assert!(hp.screens_peek_out(), "the panda comes out");
    let shown = hp.dispatch_until(WAIT, |hp| {
        hp.peek_since().is_some() && !hp.peek_surfaces().is_empty()
    })?;
    anyhow::ensure!(shown, "no peek drawn: {}", common::logged(&log));
    // Beside the bar, on the screen still there.
    let (screen, _) = hp.peek_surfaces()[0].clone();
    assert_eq!(screen, FIRST);
    // The shell says it once it drew it, which can come after the
    // surface shows on a loaded machine.
    let line = "L’écran externe 27 pouces est parti. Foot et Firefox sont ici, sur le bureau 1.";
    let said = hp.dispatch_until(WAIT, |_| common::logged(&log).contains(line))?;
    assert!(said, "{}", common::logged(&log));
    // Still out at 3.9 s, going back from 4 s.
    let since = hp.peek_since().unwrap_or_default();
    hp.hold_clock();
    while hp.now() < since + Duration::from_millis(3900) {
        hp.step(common::STEP)?;
    }
    assert!(hp.screens_peek_out());
    assert!(!hp.peek_going_back());
    hp.step(Duration::from_millis(200))?;
    assert!(hp.peek_going_back(), "back after 4 s");
    hp.step(Duration::from_millis(400))?;
    hp.release_clock();
    let gone = hp.dispatch_until(WAIT, |hp| hp.peek_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the peek stays");
    // Back: said again.
    hp.plug_screen(SECOND, external(), Some(dell()))?;
    assert!(hp.screens_peek_out());
    let said = hp.dispatch_until(WAIT, |_| {
        common::logged(&log)
            .contains("Re-bonjour, écran externe 27 pouces\u{a0}! Foot et Firefox y retournent.")
    })?;
    anyhow::ensure!(said, "{}", common::logged(&log));
    // Locked: they move, nothing is said.
    let told = hp.peeks_told();
    hp.lock();
    hp.unplug(SECOND)?;
    assert_eq!(hp.peeks_told(), told);
    assert_eq!(placed(&hp, "foot").output.as_deref(), Some(FIRST));
    Ok(())
}

/// `output`'s picture, its top-right `crop` (width, height) only when
/// given, into `HOCUS_SHOTS/name`.
fn shot(
    hp: &mut Headless,
    output: usize,
    crop: Option<(i32, i32)>,
    name: &str,
) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    let (width, height, bytes) = hp.screenshot(output)?;
    let (w, h) = crop.unwrap_or((width, height));
    let x0 = width - w;
    let mut image = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let at = usize::try_from(y * width + x0 + x)? * 4;
            let p = &bytes[at..at + 4];
            image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
        }
    }
    image.save(std::path::Path::new(&folder).join(name))?;
    Ok(())
}

/// The pull request's screenshots, into `HOCUS_SHOTS`: the laptop with
/// the winking panda as the external screen leaves, then as it comes
/// back (in English too).
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_the_winking_panda() -> anyhow::Result<()> {
    if std::env::var_os("HOCUS_SHOTS").is_none() {
        return Ok(());
    }
    for lang in ["fr_FR.UTF-8", "en_GB.UTF-8"] {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("shell.log");
        let Some(script) = shell_script(dir.path(), &log) else {
            return Ok(());
        };
        let text = std::fs::read_to_string(&script)?.replace(
            "LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8",
            &format!("LC_ALL={lang} LANG={lang}"),
        );
        std::fs::write(&script, text)?;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![
                    "1920x1200@1:302x188mm".parse()?,
                    "3440x1440@1:800x335mm".parse()?,
                ],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            Config::default(),
            Some(script),
        )?;
        hp.set_internal(FIRST);
        let bar = hp.dispatch_until(WAIT, |hp| !hp.bar_surfaces().is_empty())?;
        anyhow::ensure!(bar, "no bar: {}", common::logged(&log));
        hp.unplug(SECOND)?;
        hp.plug_screen(SECOND, "3440x1440@1:800x335mm".parse()?, Some(dell()))?;
        wait(&mut hp, Duration::from_millis(100))?;
        hp.pointer_move(1920.0 + 900.0, 600.0);
        open(&mut hp, "firefox", (900, 600))?;
        chord(&mut hp, &[KEY_LEFTMETA, KEY_LEFT]);
        open(&mut hp, "foot", (900, 600))?;
        chord(&mut hp, &[KEY_LEFTMETA, KEY_RIGHT]);
        wait(&mut hp, Duration::from_millis(100))?;
        hp.pointer_move(1700.0, 1000.0);
        hp.unplug(SECOND)?;
        let out = hp.dispatch_until(WAIT, |hp| {
            hp.peek().is_some_and(|l| l.alpha >= 1.0 && l.shift == 0.0)
        })?;
        anyhow::ensure!(out, "the panda never came out: {}", common::logged(&log));
        let tag = &lang[..2];
        shot(&mut hp, 0, None, &format!("panda-gone-{tag}.png"))?;
        shot(
            &mut hp,
            0,
            Some((760, 520)),
            &format!("panda-gone-corner-{tag}.png"),
        )?;
        let gone = hp.dispatch_until(WAIT, |hp| hp.peek_surfaces().is_empty())?;
        anyhow::ensure!(gone, "the panda stays");
        hp.plug_screen(SECOND, "3440x1440@1:800x335mm".parse()?, Some(dell()))?;
        let out = hp.dispatch_until(WAIT, |hp| {
            hp.peek().is_some_and(|l| l.alpha >= 1.0 && l.shift == 0.0)
        })?;
        anyhow::ensure!(out, "the panda never came out again");
        shot(
            &mut hp,
            0,
            Some((760, 520)),
            &format!("panda-back-corner-{tag}.png"),
        )?;
        shot(&mut hp, 1, None, &format!("external-back-{tag}.png"))?;
    }
    Ok(())
}
