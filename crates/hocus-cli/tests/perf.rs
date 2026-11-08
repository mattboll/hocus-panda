//! `hocus perf day`: the invented day of `hocus_perf::fixture` written
//! where the desktop records, read back as text and as JSON: the same
//! totals, peaks and signals as the Settings' page computes.

use std::{path::Path, process::Command};

use hocus_perf::{fixture, report::report};
use jiff::{civil::date, tz::TimeZone};

/// `hocus perf day ARGS` with `state` as `$XDG_STATE_HOME`, in UTC and
/// `lang`: its status and standard output.
fn perf_day(state: &Path, lang: &str, args: &[&str]) -> std::io::Result<(bool, String)> {
    let out = Command::new(env!("CARGO_BIN_EXE_hocus"))
        .args(["perf", "day"])
        .args(args)
        .env_clear()
        .env("XDG_STATE_HOME", state)
        .env("HOME", state)
        .env("TZ", "UTC")
        .env("LANG", lang)
        .output()?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    ))
}

/// `value` with its numbers to the millionth: what JSON's text keeps of a
/// float.
fn rounded(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Number(n) if n.is_f64() => n
            .as_f64()
            .map_or(Value::Null, |x| ((x * 1e6).round() / 1e6).into()),
        Value::Array(a) => a.into_iter().map(rounded).collect(),
        Value::Object(o) => o.into_iter().map(|(k, v)| (k, rounded(v))).collect(),
        other => other,
    }
}

#[test]
fn the_invented_day_as_text_and_as_json() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let dir = hocus_perf::files::dir_in(&state.path().join("hocuspanda"));
    std::fs::create_dir_all(&dir)?;
    let on = date(2026, 11, 6);
    let day = fixture::working_day(on, &TimeZone::UTC);
    std::fs::write(dir.join("2026-11-06"), fixture::text(&day))?;
    // JSON: the report the page shows.
    let (ok, json) = perf_day(state.path(), "C", &["--date", "2026-11-06", "--json"])?;
    assert!(ok, "{json}");
    let mut said: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(said["date"], "2026-11-06");
    said.as_object_mut().map(|o| o.remove("date"));
    assert_eq!(
        rounded(said.clone()),
        rounded(serde_json::to_value(report(&day))?)
    );
    let kinds: Vec<&str> = said["signals"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s["kind"].as_str())
        .collect();
    assert_eq!(kinds, ["memory-growing", "frames-at-rest"]);
    // Text, in French: the heading, the peaks, the signals.
    let (ok, text) = perf_day(state.path(), "fr_FR.UTF-8", &["--date", "2026-11-06"])?;
    assert!(ok, "{text}");
    eprintln!("{text}");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "Vendredi 6 novembre · session 08:30 → 18:30 (10 h 00, dont 1 h 05 verrouillé)"
    );
    assert!(
        text.contains("\nPics\n") && text.contains("\nSignaux\n"),
        "{text}"
    );
    assert!(text.contains("Écran DP-2 branché"), "{text}");
    assert!(text.contains("Réglages › Fond d’écran"), "{text}");
    assert!(
        text.contains("! La mémoire du shell ne fait que monter"),
        "{text}"
    );
    assert!(
        text.contains("! 15:10–15:25 : des images sans que rien ne bouge"),
        "{text}"
    );
    Ok(())
}

#[test]
fn a_day_not_recorded_says_so_and_fails() -> anyhow::Result<()> {
    let state = tempfile::tempdir()?;
    let (ok, text) = perf_day(state.path(), "C", &["--date", "2026-11-06"])?;
    assert!(!ok);
    assert!(text.contains("Nothing recorded that day."), "{text}");
    let (ok, _) = perf_day(state.path(), "C", &["--date", "yesterday"])?;
    assert!(!ok);
    Ok(())
}
