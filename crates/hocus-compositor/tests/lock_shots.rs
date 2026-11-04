//! The lock screen's screenshots for its pull request (7.1, issue #412):
//! the real shell on the CPU against the fakes of its services (UPower's
//! battery at 82 %, NetworkManager's Wi-Fi, a PipeWire and a D-Bus of the
//! test's own), the clock at 13:35 on Wednesday 4 November, the user
//! Alex, two agents waiting. Written to `$HOCUS_SHOTS` when it names a
//! folder; run by hand (`--run-ignored only`).

#[path = "levels/bluez.rs"]
mod bluez;
mod common;
#[path = "levels/fakes.rs"]
mod fakes;
#[path = "levels/nm.rs"]
mod nm;

use std::{io::Write, path::PathBuf, time::Duration};

use anyhow::Context;
use common::{WAIT, chord, wait};
use fakes::Fakes;
use hocus_compositor::{Authenticator, Config, Headless, HeadlessConfig, RendererChoice};
use nm::{FakeAp, Nm, Security, Setup};

const KEY_ENTER: u32 = 28;
const KEY_CAPSLOCK: u32 = 58;

fn shoot(hp: &mut Headless, index: usize, out: &std::path::Path, name: &str) -> anyhow::Result<()> {
    common::at_rest(hp)?;
    let (w, h, px) = hp.screenshot(index)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
    let file = out.join(format!("{name}.png"));
    image.save(&file)?;
    eprintln!("{}", file.display());
    // The form, ×1.5 as the mockup's detail shows it.
    if index == 0 {
        let (cw, ch) = (720u32, 760u32);
        let (x, y) = ((w as u32 - cw) / 2, (h as u32 / 2).saturating_sub(440));
        let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let zoomed = image::imageops::resize(
            &crop,
            cw * 3 / 2,
            ch * 3 / 2,
            image::imageops::FilterType::CatmullRom,
        );
        let file = out.join(format!("{name}-zoom.png"));
        zoomed.save(&file)?;
        eprintln!("{}", file.display());
    }
    Ok(())
}

fn type_text(hp: &mut Headless, keys: &[u32]) {
    for key in keys {
        chord(hp, &[*key]);
    }
}

#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let Some(shell) = common::shell_program() else {
        return Ok(());
    };
    if !fakes::available() {
        eprintln!("the fakes' programs are missing: skipped");
        return Ok(());
    }
    std::fs::create_dir_all(&out)?;
    let fakes = Fakes::start()?;
    let _nm = Nm::start(
        fakes.runtime(),
        &fakes.address,
        Setup {
            aps: vec![FakeAp::new(
                "Livebox-A3F2",
                80,
                Security::Psk("livebox-test"),
            )],
            saved: vec![("Livebox-A3F2", Some("livebox-test"))],
            current: Some("Livebox-A3F2"),
            ..Setup::default()
        },
    )?;
    let home = tempfile::tempdir()?;
    let at = |sub: &str| home.path().join(sub).display().to_string();
    for sub in ["home", "config", "cache", "state", "data"] {
        std::fs::create_dir_all(home.path().join(sub))?;
    }
    let script = fakes.shell_with(
        &shell,
        "fr_FR.UTF-8",
        &[
            ("HOME", at("home")),
            ("XDG_CONFIG_HOME", at("config")),
            ("XDG_CACHE_HOME", at("cache")),
            ("XDG_STATE_HOME", at("state")),
            ("XDG_DATA_HOME", at("data")),
            ("HOCUS_USER_NAME", "Alex".to_owned()),
            ("HOCUS_CLOCK_AT", "2026-11-04T13:35:00".to_owned()),
        ],
        Some(&home.path().join("shell.log")),
    )?;
    let log = home.path().join("shell.log");
    // Until the shell drew what the line holds on the form's screen.
    let shell_drew = |hp: &mut Headless, what: &str| -> anyhow::Result<()> {
        let came = hp.dispatch_until(WAIT, |_| {
            String::from_utf8_lossy(&std::fs::read(&log).unwrap_or_default())
                .lines()
                .rev()
                .find(|l| l.contains("the lock is drawn") && l.contains("HEADLESS-1"))
                .is_some_and(|l| l.contains(what))
        })?;
        anyhow::ensure!(came, "the shell never drew {what}");
        // On the CPU, its buffer comes a while after it drew.
        let before = hp.lock_commits();
        let committed = hp.dispatch_until(WAIT, |hp| hp.lock_commits() > before)?;
        anyhow::ensure!(committed, "the shell never showed {what}");
        Ok(())
    };
    let mut config = Config::default();
    config.keyboard.layouts = vec!["fr".to_owned(), "us".to_owned()];
    config.lock_auth = Authenticator::test("panda");
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["2560x1600".parse()?, "1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        Some(script),
    )?;
    let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.levels_commits() > 0)?;
    anyhow::ensure!(up, "no bar");
    // Two agents waiting for the user.
    let control = hp.control_socket().context("no control socket")?.to_owned();
    for (key, name) in [("a", "site-vitrine"), ("b", "api")] {
        let mut stream = std::os::unix::net::UnixStream::connect(&control)?;
        writeln!(stream, "agent command {key} waiting - {name}")?;
    }
    let told = hp.dispatch_until(WAIT, |hp| {
        hp.lock_shown().is_none() && hp.agents_commits() > 0
    })?;
    wait(&mut hp, Duration::from_millis(300))?;
    anyhow::ensure!(told, "the agents never came");
    // A window, as the desktop was.
    let (_, window) = common::open_with(&mut hp, "kitty", (1400, 900), false)?;
    hp.pointer_move(2400.0, 1500.0);
    hp.lock();
    let drawn = hp.dispatch_until(WAIT, |hp| hp.lock_drawn_on().len() == 2)?;
    anyhow::ensure!(drawn, "the lock never drew");
    wait(&mut hp, Duration::from_secs(1))?;
    shoot(&mut hp, 0, &out, "lock-rest")?;
    shoot(&mut hp, 1, &out, "lock-second-screen")?;
    // Typing: "pand".
    type_text(&mut hp, &[25, 16, 49, 32]);
    shell_drew(&mut hp, "chars: 4")?;
    shoot(&mut hp, 0, &out, "lock-typing")?;
    chord(&mut hp, &[KEY_ENTER]);
    let wrong = hp.dispatch_until(WAIT, |hp| {
        hp.lock_shown()
            .is_some_and(|s| s.phase == hocus_link::LockPhase::Wrong)
    })?;
    anyhow::ensure!(wrong, "never wrong");
    shell_drew(&mut hp, "phase: Wrong")?;
    shoot(&mut hp, 0, &out, "lock-wrong")?;
    chord(&mut hp, &[KEY_CAPSLOCK]);
    type_text(&mut hp, &[25, 16]);
    shell_drew(&mut hp, "caps: true")?;
    shoot(&mut hp, 0, &out, "lock-caps")?;
    drop(window);
    Ok(())
}
