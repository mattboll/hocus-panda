//! The weather in the bar (#814, variant C): the real shell on the CPU,
//! headless, its forecast a saved Open-Meteo answer
//! (`HOCUS_WEATHER_FIXTURE`, 19 November 2026 in Paris: rain from 14:15 to
//! 15:45), its clock `HOCUS_CLOCK_AT`, its day a fixture's, in French; never
//! the network. The shell says what Today shows in its log ("today is
//! drawn at rest"), which the test reads.
//!
//! One test: Today's header carries the weather now, the rain is a line
//! among the events and tomorrow closes the list, and while it rains the
//! compositor is told. The pull request's screenshots are an ignored test
//! (`HOCUS_SHOTS`).

use crate::common;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use hocus_compositor::{
    Config, FormChoice, Headless, HeadlessConfig, RendererChoice, WeatherSettings, model::Rect,
};
use hocus_link::TODAY_NAMESPACE;

/// The next meeting at 15:00 (the card), then two events.
const DAY: &str = r##"
[[calendars]]
id = "work"
name = "Travail"
color = "#6F9BFF"

[[calendars]]
id = "team"
name = "Équipe"
color = "#E07AB5"

[[calendars]]
id = "perso"
name = "Perso"
color = "#3FB8AF"

[[events]]
title = "Point équipe"
start = "today 15:00"
end = "today 15:30"

[[events]]
calendar = "team"
title = "Revue sprint 24"
start = "today 16:30"
end = "today 17:30"

[[events]]
calendar = "perso"
title = "Sport"
start = "today 18:00"
end = "today 19:00"
"##;

fn forecast() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../hocus-weather/fixtures/forecast.json")
}

struct Bed {
    _dir: tempfile::TempDir,
    hp: Headless,
    log: PathBuf,
}

/// The compositor on a 1920 × 1080 screen with the real shell on the CPU,
/// its clock on 19 November 2026 at `clock` (Paris), the bar in `form`.
fn start(clock: &str, form: FormChoice) -> anyhow::Result<Bed> {
    use std::os::unix::fs::PermissionsExt;
    let shell = common::shell_program();
    let dir = tempfile::tempdir()?;
    let root = dir.path();
    for sub in ["home", "state", "config", "bin"] {
        std::fs::create_dir_all(root.join(sub))?;
    }
    let file = root.join("config/config.toml");
    std::fs::write(
        &file,
        format!(
            "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n\
             [panel]\nform = \"{}\"\n\n\
             [weather]\nplace = \"Saint-Étienne\"\nlatitude = 45.44\nlongitude = 4.39\n",
            form.name()
        ),
    )?;
    std::fs::write(root.join("config/day.toml"), DAY)?;
    let log = root.join("shell.log");
    let script = root.join("shell");
    let home = root.join("home");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
             export DBUS_SYSTEM_BUS_ADDRESS='unix:path=/nonexistent' \
             PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
             HOME='{home}' XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
             XDG_STATE_HOME='{state}' TZ=Europe/Paris HOCUS_CLOCK_AT='2026-11-19T{clock}' \
             HOCUS_WEATHER_FIXTURE='{forecast}' \
             LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 ICED_BACKEND=tiny-skia \
             PATH='{bin}:/usr/bin:/bin' RUST_LOG=warn,hocuspanda_shell=debug\n\
             exec '{shell}' \"$@\" 2>>'{log}'\n",
            home = home.display(),
            state = root.join("state").display(),
            bin = root.join("bin").display(),
            forecast = forecast().display(),
            shell = shell.display(),
            log = log.display()
        ),
    )?;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
    let mut settings = Config {
        file: Some(file),
        plain_background: true,
        lang: hocus_words::Lang::French,
        weather: WeatherSettings {
            place: "Saint-Étienne".to_owned(),
            latitude: 45.44,
            longitude: 4.39,
            ..WeatherSettings::default()
        },
        ..Config::default()
    };
    settings.keyboard.layouts = vec!["us".to_owned()];
    settings.panel.form = form;
    settings.appearance.reduced_motion = true;
    let mut hp = Headless::start_with_shell(
        HeadlessConfig {
            outputs: vec!["1920x1080".parse()?],
            renderer: RendererChoice::Pixman,
            render_node: None,
        },
        settings,
        Some(script),
    )?;
    let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
    anyhow::ensure!(drawn, "no bar");
    hp.pointer_move(400.0, 600.0);
    Ok(Bed {
        _dir: dir,
        hp,
        log,
    })
}

impl Bed {
    /// Waits for a line of the shell's log holding `what` and all of
    /// `holds`; that line.
    fn logged(&mut self, what: &str, holds: &[&str]) -> anyhow::Result<String> {
        let log = self.log.clone();
        let find = || {
            common::logged(&log)
                .lines()
                .rev()
                .find(|l| l.contains(what) && holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned)
        };
        let seen = self.hp.dispatch_until(common::WAIT, |_| find().is_some())?;
        anyhow::ensure!(
            seen,
            "never logged {what} {holds:?}:\n{}",
            common::logged(&log)
        );
        find().ok_or_else(|| anyhow::anyhow!("gone"))
    }

    fn surface(&mut self) -> anyhow::Result<Rect> {
        Ok(common::drawn(&mut self.hp, TODAY_NAMESPACE)?[0].1)
    }
}

#[test]
fn today_holds_the_weather_and_the_compositor_hears_it_rain() -> anyhow::Result<()> {
    let mut bed = start("13:47:00", FormChoice::Full)?;
    let line = bed.logged(
        "today is drawn at rest",
        &["head=Some(\"9°\")", "14:15 Pluie → 15 h 45; 16:30 «Revue sprint 24»"],
    )?;
    assert!(
        line.contains("18:00 «Sport» #3FB8AF; Demain 3° / 11°"),
        "{line}"
    );
    bed.logged("whether it rains", &["raining=false"])?;
    drop(bed);
    // While it rains: no time, "Il pleut"; the compositor told.
    let mut bed = start("14:32:00", FormChoice::Full)?;
    bed.logged("today is drawn at rest", &["AUJOURD'HUI Il pleut → 15 h 45;"])?;
    bed.logged("whether it rains", &["raining=true"])?;
    Ok(())
}

/// The pull request's screenshots, to `$HOCUS_SHOTS` when it names a
/// folder; run by hand (`--run-ignored only`): the bar's top with Today
/// (the rain coming, raining, the evening), the mini form's item, and the
/// popover open beside the bar.
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bed: &mut Bed, name: &str, crop: (u32, u32, u32, u32)| {
        common::at_rest(&mut bed.hp)?;
        common::wait(&mut bed.hp, Duration::from_millis(500))?;
        let (w, h, px) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(w as u32, h as u32, rgba).context("image")?;
        let (x, y, cw, ch) = crop;
        let part = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
        let file = out.join(format!("{name}.png"));
        part.save(&file)?;
        eprintln!("{}", file.display());
        anyhow::Ok(())
    };
    let full = (1920 - 240, 0, 240, 520);
    for (clock, name, holds) in [
        ("13:47:00", "bar-rain-coming", "14:15 Pluie"),
        ("14:32:00", "bar-raining", "Il pleut"),
        ("19:20:00", "bar-evening", "demain 11°"),
    ] {
        let mut bed = start(clock, FormChoice::Full)?;
        bed.logged("today is drawn at rest", &[holds])?;
        shoot(&mut bed, name, full)?;
        if clock == "13:47:00" {
            // The popover, by a click on the header's weather.
            let surface = bed.surface()?;
            let at = (
                f64::from(surface.x + surface.width - 30),
                f64::from(surface.y + 5 + 9),
            );
            common::click_on(&mut bed.hp, at, TODAY_NAMESPACE)?;
            bed.logged("the weather's popover shows it", &[])?;
            bed.hp
                .dispatch_until(common::WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
            bed.hp.pointer_move(400.0, 900.0);
            shoot(&mut bed, "popover", (1920 - 600, 0, 600, 1080))?;
        }
    }
    for (clock, name) in [("13:47:00", "mini"), ("19:20:00", "mini-evening")] {
        let mut bed = start(clock, FormChoice::Mini)?;
        bed.logged("today's weather is drawn in the mini form", &[])?;
        shoot(&mut bed, name, (1920 - 120, 0, 120, 360))?;
        if clock == "13:47:00" {
            let surface = bed.surface()?;
            let at = (
                f64::from(surface.x + surface.width / 2),
                f64::from(surface.y + 20),
            );
            common::click_on(&mut bed.hp, at, TODAY_NAMESPACE)?;
            bed.logged("the weather's popover shows it", &[])?;
            bed.hp
                .dispatch_until(common::WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
            shoot(&mut bed, "mini-popover", (1920 - 480, 0, 480, 1080))?;
        }
    }
    Ok(())
}
