//! Mirroring with the real shell (#931, docs/design/windows.md §12
//! "Mirroring", bar.md "Mirroring"): a set of screens never seen has the
//! winking panda offer « Recopier » for 8 s, not while locked and never
//! over an agent's panda; its button mirrors the screens; the bar's
//! « Recopie » sign, and its popover's « Arrêter la recopie », stop it.
//! The laptop (2560 × 1600 at 1.25, the built-in panel) and a 55-inch TV
//! (1920 × 1080) plugged by the harness.

use crate::common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use common::{WAIT, wait};
use hocus_compositor::{
    Authenticator, Config, Headless, HeadlessConfig, OutputSpec, RendererChoice, ScreenIdentity,
    model::Rect,
};

const LAPTOP: &str = "HEADLESS-1";
const TV: &str = "HDMI-A-1";

fn tv() -> OutputSpec {
    OutputSpec::new(1920, 1080).physical_size(1218, 685)
}

fn tv_identity() -> ScreenIdentity {
    ScreenIdentity::new("SAM", "QN55", "0001")
}

/// The real shell, in French, logging what it shows into `log`.
fn shell_script(dir: &Path, log: &Path) -> anyhow::Result<PathBuf> {
    let shell = common::shell_program();
    let script = dir.join("shell");
    let home = dir.join("home");
    std::fs::create_dir_all(home.join("hocuspanda"))?;
    // Day.1's first-run card set aside: the bar's column as it is.
    std::fs::write(home.join("hocuspanda/day-sources"), "later\n")?;
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
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    Ok(script)
}

struct Bed {
    hp: Headless,
    log: PathBuf,
    _dir: tempfile::TempDir,
}

impl Bed {
    /// The laptop alone with the real shell, its bar drawn.
    fn start() -> anyhow::Result<Self> {
        Self::start_with(Config::default())
    }

    fn start_with(mut config: Config) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("shell.log");
        let script = shell_script(dir.path(), &log)?;
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.state_dir = Some(dir.path().join("state"));
        config.lock_auth = Authenticator::test("right");
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec![
                    OutputSpec::new(2560, 1600)
                        .physical_size(302, 189)
                        .scale(1.25),
                ],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
        )?;
        hp.set_internal(LAPTOP);
        let bar = hp.dispatch_until(WAIT, |hp| !hp.bar_surfaces().is_empty())?;
        anyhow::ensure!(bar, "no bar: {}", common::logged(&log));
        Ok(Self { hp, log, _dir: dir })
    }

    fn logged(&self) -> String {
        common::logged(&self.log)
    }

    /// Waits until the shell logged a line holding every one of `all`.
    fn says(&mut self, all: &[&str]) -> anyhow::Result<()> {
        let log = self.log.clone();
        let said = self.hp.dispatch_until(WAIT, |_| {
            common::logged(&log)
                .lines()
                .any(|l| all.iter().all(|a| l.contains(a)))
        })?;
        anyhow::ensure!(said, "the shell never said {all:?}:\n{}", self.logged());
        Ok(())
    }

    fn plug_tv(&mut self) -> anyhow::Result<()> {
        self.hp.plug_screen(TV, tv(), Some(tv_identity()))?;
        let laid = self.hp.dispatch_until(WAIT, |hp| hp.outputs().len() == 2)?;
        anyhow::ensure!(laid, "the TV never lit");
        // Laid out after the turn it lit in.
        self.hp.dispatch(Duration::ZERO)?;
        Ok(())
    }

    /// The bubble drawn out, at rest: its surface.
    fn bubble_out(&mut self) -> anyhow::Result<Rect> {
        let out = self.hp.dispatch_until(WAIT, |hp| {
            hp.peek().is_some_and(|l| l.alpha >= 1.0 && l.shift == 0.0)
                && !hp.peek_surfaces().is_empty()
        })?;
        anyhow::ensure!(out, "the panda never came out:\n{}", self.logged());
        Ok(self.hp.peek_surfaces()[0].1)
    }
}

/// Acceptance 2: a set never seen has the winking panda say the TV
/// plugged with « Recopier », out still at 7.9 s, going back from 8 s;
/// the TV stays extended.
#[test]
fn a_set_never_seen_offers_to_mirror_for_8_s() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.plug_tv()?;
    assert!(bed.hp.screens_new_out(), "the panda comes out");
    bed.says(&[
        "the winking panda peeks out",
        "Écran externe 55 pouces branché.",
        "Recopier",
    ])?;
    bed.bubble_out()?;
    let since = bed.hp.peek_since().unwrap_or_default();
    bed.hp.hold_clock();
    while bed.hp.now() < since + Duration::from_millis(7900) {
        bed.hp.step(common::STEP)?;
    }
    assert!(bed.hp.screens_new_out(), "still out at 7.9 s");
    bed.hp.step(Duration::from_millis(200))?;
    assert!(bed.hp.peek_going_back(), "back from 8 s");
    bed.hp.release_clock();
    assert_eq!(bed.hp.mirroring(), None);
    assert_eq!(bed.hp.outputs().len(), 2, "still extended");
    Ok(())
}

/// Acceptance 3: « Recopier » mirrors: one screen for the desktop, the
/// shell told the laptop shown on the TV.
#[test]
fn recopier_mirrors_the_screens() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.plug_tv()?;
    bed.says(&["the winking panda peeks out", "Recopier"])?;
    let surface = bed.bubble_out()?;
    // The button where the shell says it put it on its surface.
    let log = bed.log.clone();
    let mut at = None;
    bed.hp.dispatch_until(WAIT, |_| {
        at = common::last_json(&log, " shows {")
            .and_then(|v| v.get("peek:Mirror")?.get("at").cloned())
            .and_then(|a| serde_json::from_value::<[f64; 4]>(a).ok());
        at.is_some()
    })?;
    let [bx, by, bw, bh] = at.ok_or_else(|| anyhow::anyhow!("no button:\n{}", bed.logged()))?;
    let (x, y) = (
        f64::from(surface.x) + bx + bw / 2.0,
        f64::from(surface.y) + by + bh / 2.0,
    );
    common::click_on(&mut bed.hp, (x, y), hocus_link::PEEK_NAMESPACE)?;
    let mirrored = bed.hp.dispatch_until(WAIT, |hp| hp.mirroring().is_some())?;
    anyhow::ensure!(mirrored, "not mirrored:\n{}", bed.logged());
    assert_eq!(
        bed.hp.mirroring(),
        Some((LAPTOP.to_owned(), vec![TV.to_owned()]))
    );
    assert_eq!(bed.hp.outputs().len(), 1);
    bed.says(&["the screens mirrored", "HEADLESS-1", "HDMI-A-1"])?;
    // The offer answered, the panda goes back.
    let back = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.peek_surfaces().is_empty())?;
    assert!(back, "the panda stays out");
    Ok(())
}

/// Acceptance 4: locked, a set never seen lights extended and nothing is
/// said.
#[test]
fn locked_no_offer() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.hp.lock();
    wait(&mut bed.hp, Duration::from_millis(100))?;
    let told = bed.hp.peeks_told();
    bed.plug_tv()?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.hp.peeks_told(), told);
    assert!(!bed.hp.screens_new_out());
    assert_eq!(bed.hp.outputs().len(), 2, "extended");
    assert_eq!(bed.hp.mirroring(), None);
    Ok(())
}

/// Acceptance 5: an agent's panda out stays; the offer does not replace
/// it.
#[test]
fn an_agents_peek_is_never_replaced() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let socket = bed
        .hp
        .control_socket()
        .ok_or_else(|| anyhow::anyhow!("no control socket"))?
        .to_path_buf();
    let hocus = common::hocus_program();
    for state in ["working", "waiting"] {
        let mut child = Command::new(&hocus)
            .arg("--socket")
            .arg(&socket)
            .args(["agent", "build", state])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()?;
        let done = bed
            .hp
            .dispatch_until(WAIT, |_| matches!(child.try_wait(), Ok(Some(_))))?;
        anyhow::ensure!(done, "hocus agent build {state} never ended");
        anyhow::ensure!(child.wait()?.success(), "hocus agent build {state}");
    }
    let out = bed.hp.dispatch_until(WAIT, |hp| hp.peek_out().is_some())?;
    anyhow::ensure!(out, "the agent's panda never came out:\n{}", bed.logged());
    let agent = bed.hp.peek_out();
    let told = bed.hp.peeks_told();
    bed.plug_tv()?;
    wait(&mut bed.hp, Duration::from_millis(200))?;
    assert_eq!(bed.hp.peek_out(), agent, "the agent's panda stays");
    assert!(!bed.hp.screens_new_out());
    assert_eq!(bed.hp.peeks_told(), told);
    Ok(())
}

/// Acceptance 16: mirrored, the bar shows « Recopie »; its popover's
/// « Arrêter la recopie » stops mirroring and the sign goes.
#[test]
fn the_sign_stops_mirroring() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    bed.plug_tv()?;
    bed.hp.mirror(true);
    bed.says(&["the in-use sign is drawn at rest", "kinds=[Mirror]"])?;
    let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
    common::over(
        &mut bed.hp,
        (
            f64::from(sign.x + sign.width / 2),
            f64::from(sign.y + sign.height / 2),
        ),
        hocus_link::IN_USE_NAMESPACE,
    )?;
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(open, "no popover:\n{}", bed.logged());
    bed.says(&[
        "the in-use popover shows it",
        "Recopie",
        "Écran du portable → écran externe 55 pouces",
        "Arrêter la recopie",
    ])?;
    common::at_rest(&mut bed.hp)?;
    let popover = common::drawn_popovers(&bed.hp)[0];
    // Its one button, at its foot: 12 px in and 1 px of border, 30 high.
    let (x, y) = (
        popover.x + popover.width / 2,
        popover.y + popover.height - 13 - 15,
    );
    common::click(&mut bed.hp, (f64::from(x), f64::from(y)))?;
    let stopped = bed.hp.dispatch_until(WAIT, |hp| hp.mirroring().is_none())?;
    anyhow::ensure!(stopped, "still mirrored:\n{}", bed.logged());
    assert_eq!(bed.hp.outputs().len(), 2);
    bed.says(&["the in-use sign goes"])?;
    Ok(())
}

/// Output `name`'s picture (its `crop`: x, y, width, height in pixels,
/// else all of it) into `HOCUS_SHOTS/file`.
fn shot(
    hp: &mut Headless,
    name: &str,
    crop: Option<(i32, i32, i32, i32)>,
    file: &str,
) -> anyhow::Result<()> {
    let Some(folder) = std::env::var_os("HOCUS_SHOTS") else {
        return Ok(());
    };
    let mut names = vec![LAPTOP.to_owned()];
    if hp.mirroring().is_some() || hp.outputs().len() == 2 {
        names.push(TV.to_owned());
    }
    names.sort();
    let index = names.iter().position(|n| n == name).unwrap_or(0);
    let (width, height, bytes) = hp.screenshot(index)?;
    let (x0, y0, w, h) = crop.unwrap_or((0, 0, width, height));
    let mut image = image::RgbaImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let at = usize::try_from((y0 + y) * width + x0 + x)? * 4;
            let p = &bytes[at..at + 4];
            image.put_pixel(x as u32, y as u32, image::Rgba([p[2], p[1], p[0], 255]));
        }
    }
    let path = Path::new(&folder).join(file);
    image.save(&path)?;
    eprintln!("{}", path.display());
    Ok(())
}

/// The maintainer's screenshots, into `HOCUS_SHOTS`: the bubble as the TV
/// is plugged; mirrored, the TV showing the laptop fitted with its bands;
/// the bar's sign and its popover.
#[test]
#[ignore = "screenshots for the maintainer: HOCUS_SHOTS=DIR"]
fn screenshots_of_mirroring() -> anyhow::Result<()> {
    if std::env::var_os("HOCUS_SHOTS").is_none() {
        return Ok(());
    }
    let mut bed = Bed::start()?;
    bed.hp.pointer_move(500.0, 400.0);
    common::open(&mut bed.hp, "foot", (900, 560))?;
    bed.plug_tv()?;
    bed.says(&["the winking panda peeks out", "Recopier"])?;
    bed.bubble_out()?;
    common::at_rest(&mut bed.hp)?;
    bed.hp.pointer_move(400.0, 900.0);
    wait(&mut bed.hp, Duration::from_millis(200))?;
    // The laptop's right part, where the bar and the bubble are.
    shot(
        &mut bed.hp,
        LAPTOP,
        Some((1460, 0, 1100, 760)),
        "bubble.png",
    )?;
    bed.hp.mirror(true);
    bed.says(&["the in-use sign is drawn at rest", "kinds=[Mirror]"])?;
    let gone = bed
        .hp
        .dispatch_until(WAIT, |hp| hp.peek_surfaces().is_empty())?;
    anyhow::ensure!(gone, "the panda stays");
    bed.hp.pointer_move(1000.0, 700.0);
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    shot(&mut bed.hp, TV, None, "tv-mirrored.png")?;
    shot(&mut bed.hp, LAPTOP, None, "laptop-mirrored.png")?;
    let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
    common::over(
        &mut bed.hp,
        (
            f64::from(sign.x + sign.width / 2),
            f64::from(sign.y + sign.height / 2),
        ),
        hocus_link::IN_USE_NAMESPACE,
    )?;
    let open = bed
        .hp
        .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(open, "no popover");
    common::at_rest(&mut bed.hp)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    // At 1.25: the bar's top right, the sign and its popover.
    shot(
        &mut bed.hp,
        LAPTOP,
        Some((1660, 0, 900, 700)),
        "sign-popover.png",
    )?;
    Ok(())
}
