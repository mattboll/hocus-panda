//! The night light (#436, docs/design/windows.md §14), headless: no
//! screen has a gamma ramp here, so the tests read the ramps the backend
//! was handed, on a wall clock they hold. Off by default; from 21:00 it
//! warms over 30 min to its temperature, from 7:00 it goes back over
//! 30 min; the file changed turns it off or moves it at once. With the
//! weather's place, from sunset to sunrise there (#929).

use crate::common;

use common::WAIT;
use hocus_compositor::{
    Config, Headless, HeadlessConfig, NightLight, RendererChoice, WeatherSettings,
};

/// The ramps' last values (red, green, blue), each out of 65535.
fn whites(hp: &Headless) -> Option<[u16; 3]> {
    hp.gamma()
        .map(|[r, g, b]| [r, g, b].map(|c| c.last().copied().unwrap_or(0)))
}

fn start(night_light: NightLight, file: Option<std::path::PathBuf>) -> anyhow::Result<Headless> {
    start_at(night_light, file, WeatherSettings::default())
}

fn start_at(
    night_light: NightLight,
    file: Option<std::path::PathBuf>,
    weather: WeatherSettings,
) -> anyhow::Result<Headless> {
    Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        Config {
            night_light,
            file,
            weather,
            ..Config::default()
        },
    )
}

#[test]
fn off_by_default_the_screens_keep_their_white() -> anyhow::Result<()> {
    let mut hp = start(NightLight::default(), None)?;
    hp.night_light_clock(23, 0);
    assert_eq!(hp.gamma(), None);
    Ok(())
}

#[test]
fn it_warms_from_nine_and_goes_back_from_seven() -> anyhow::Result<()> {
    let on = NightLight {
        enabled: true,
        ..NightLight::default()
    };
    let mut hp = start(on, None)?;
    hp.night_light_clock(12, 0);
    assert_eq!(hp.gamma(), None, "the day");
    hp.night_light_clock(23, 0);
    let night = whites(&hp).expect("a ramp at night");
    // 4000 K: red whole, green about 81 %, blue about 66 %.
    assert_eq!(night[0], 65535);
    assert!((52_000..54_500).contains(&night[1]), "{night:?}");
    assert!((41_000..45_000).contains(&night[2]), "{night:?}");
    let [red, green, blue] = hp.gamma().expect("a ramp");
    assert_eq!(red.len(), 256);
    assert_eq!((red[0], green[0], blue[0]), (0, 0, 0));
    // Half-way in: between the day and the night.
    hp.night_light_clock(21, 15);
    let fading = whites(&hp).expect("fading in");
    assert!(fading[2] > night[2] && fading[2] < 65535, "{fading:?}");
    hp.night_light_clock(21, 30);
    assert_eq!(whites(&hp), Some(night));
    // Past midnight, still night; from 7:00 back over 30 min.
    hp.night_light_clock(3, 0);
    assert_eq!(whites(&hp), Some(night));
    hp.night_light_clock(7, 15);
    let fading = whites(&hp).expect("fading out");
    assert!(fading[2] > night[2] && fading[2] < 65535, "{fading:?}");
    hp.night_light_clock(7, 30);
    assert_eq!(hp.gamma(), None, "the day again");
    Ok(())
}

#[test]
fn the_file_turns_it_off_and_sets_its_temperature() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let write = |text: &str| -> anyhow::Result<()> {
        let new = file.with_extension("new");
        std::fs::write(&new, text)?;
        std::fs::rename(&new, &file)?;
        Ok(())
    };
    write("[night-light]\nenabled = true\n")?;
    let on = NightLight {
        enabled: true,
        ..NightLight::default()
    };
    let mut hp = start(on, Some(file.clone()))?;
    hp.night_light_clock(23, 0);
    let at_4000 = whites(&hp).expect("on");
    // Warmer: less blue.
    write("[night-light]\nenabled = true\ntemperature = 2700\n")?;
    let read = hp.dispatch_until(WAIT, |hp| hp.state().config.night_light.temperature == 2700)?;
    anyhow::ensure!(read, "the file is not read");
    hp.dispatch_until(WAIT, |hp| whites(hp) != Some(at_4000))?;
    let at_2700 = whites(&hp).expect("on");
    assert!(at_2700[2] < at_4000[2], "{at_2700:?} {at_4000:?}");
    // Hours that leave 23:00 out: the day.
    write("[night-light]\nenabled = true\nfrom = \"23:30\"\nto = \"6:00\"\n")?;
    let day = hp.dispatch_until(WAIT, |hp| hp.gamma().is_none())?;
    anyhow::ensure!(day, "still warm: {:?}", whites(&hp));
    // Off.
    write("[night-light]\nenabled = true\n")?;
    hp.dispatch_until(WAIT, |hp| hp.gamma().is_some())?;
    write("[night-light]\nenabled = false\n")?;
    let off = hp.dispatch_until(WAIT, |hp| hp.gamma().is_none())?;
    anyhow::ensure!(off, "still warm");
    Ok(())
}

/// #929: by default it follows the sun at the weather's place: in Paris
/// on 20 November 2026 it sets about 16:04 UTC and rises about 07:10, so
/// it warms long before 21:00; the file asking the fixed hours takes it
/// back to them at once.
#[test]
fn with_the_weather_s_place_it_warms_from_sunset_to_sunrise() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("config.toml");
    let write = |text: &str| -> anyhow::Result<()> {
        let new = file.with_extension("new");
        std::fs::write(&new, text)?;
        std::fs::rename(&new, &file)?;
        Ok(())
    };
    let paris = "[weather]\nplace = \"Paris\"\nlatitude = 48.8566\nlongitude = 2.3522\n";
    write(&format!("{paris}[night-light]\nenabled = true\n"))?;
    let on = NightLight {
        enabled: true,
        ..NightLight::default()
    };
    let weather = WeatherSettings {
        place: "Paris".to_owned(),
        latitude: 48.8566,
        longitude: 2.3522,
        ..WeatherSettings::default()
    };
    let mut hp = start_at(on, Some(file.clone()), weather)?;
    hp.night_light_at("2026-11-20T15:30:00Z")?;
    assert_eq!(hp.gamma(), None, "before sunset");
    hp.night_light_at("2026-11-20T16:50:00Z")?;
    let night = whites(&hp).expect("warm past sunset");
    hp.night_light_at("2026-11-21T03:00:00Z")?;
    assert_eq!(whites(&hp), Some(night), "the night through");
    hp.night_light_at("2026-11-21T08:00:00Z")?;
    assert_eq!(hp.gamma(), None, "after sunrise");
    // The fixed hours asked: 16:50 is the day again.
    hp.night_light_at("2026-11-20T16:50:00Z")?;
    assert_eq!(whites(&hp), Some(night));
    write(&format!(
        "{paris}[night-light]\nenabled = true\nschedule = \"hours\"\n"
    ))?;
    let day = hp.dispatch_until(WAIT, |hp| hp.gamma().is_none())?;
    anyhow::ensure!(day, "still warm at the fixed hours: {:?}", whites(&hp));
    Ok(())
}
