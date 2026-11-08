//! Always on (§3.10): her sources sync on their schedules while the
//! session is locked; after a suspend or a restart every source catches up
//! from its cursor, and an absence over four hours makes a "Pendant ton
//! absence" briefing due; nothing arrived meanwhile is lost.

mod common;

use hocus_secretary::{clock::Clock, store::Store};
use hocus_secretary_link::ToSecretary;

const MAIL: &str = r#"
name = "fixture-mail"
notebook = "pro"
every = 300
[[items]]
id = "m1"
at = "2026-11-09T07:00:00Z"
kind = "mail"
digest = "Paul : le devis"
[[items]]
id = "m2"
at = "2026-11-09T08:04:00Z"
kind = "mail"
digest = "Léa : la recette"
[[items]]
id = "m3"
at = "2026-11-09T15:00:00Z"
kind = "mail"
digest = "Inès : le train de demain"
[[items]]
id = "m4"
at = "2026-11-09T21:30:00Z"
kind = "mail"
digest = "Free : la facture"
"#;

#[test]
fn locked_she_keeps_reading_and_a_long_sleep_is_caught_up() -> anyhow::Result<()> {
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        common::options(vec![common::fixture(MAIL)]),
    );
    let status = driver.settle();
    assert_eq!(status.counts.episodes, 1, "m1 at once");
    driver.send(&ToSecretary::Session { locked: true });
    let status = driver.advance(300);
    assert!(status.locked);
    assert_eq!(status.counts.episodes, 2, "m2 read while locked");
    let cursor = status.sources[0].cursor.clone();
    // A suspend of 14 h: every source at once, from its cursor.
    driver.send(&ToSecretary::Resumed {
        slept: Some(14 * 3600),
    });
    let status = driver.settle();
    let caught = status.catch_up.clone().expect("a catch-up");
    assert_eq!(caught.gap_seconds, 14 * 3600);
    assert_eq!(caught.items, 2, "m3 and m4, of the gap: {status:?}");
    assert!(caught.away_briefing);
    assert_ne!(status.sources[0].cursor, cursor);
    // Episodes: the four mails, and the catch-up's note.
    assert_eq!(status.counts.episodes, 5);
    let store = driver.stop();
    let export = store.export_json(None)?;
    let kinds: Vec<&str> = export["episodes"]
        .as_array()
        .map(|e| e.iter().filter_map(|e| e["kind"].as_str()).collect())
        .unwrap_or_default();
    assert_eq!(kinds.iter().filter(|k| **k == "mail").count(), 4);
    assert!(kinds.contains(&"catch-up"));
    Ok(())
}

#[test]
fn a_short_sleep_needs_no_briefing() -> anyhow::Result<()> {
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        common::options(vec![common::fixture(MAIL)]),
    );
    driver.settle();
    driver.send(&ToSecretary::Resumed { slept: Some(3600) });
    let caught = driver.settle().catch_up.expect("a catch-up");
    assert!(!caught.away_briefing);
    assert_eq!(caught.items, 1, "m2");
    driver.stop();
    Ok(())
}

#[test]
fn after_a_shutdown_she_catches_up_from_her_cursors() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("memory.db");
    let key = [3u8; 32];
    let clock = common::clock();
    let mut driver = common::Driver::start(
        Store::open(&file, &key, clock.clone())?,
        common::options(vec![common::fixture(MAIL)]),
    );
    driver.settle();
    drop(driver.stop());
    // Off for the day: started again at 22:00.
    let later = Clock::virtual_at("2026-11-09T22:00:00Z".parse()?);
    let mut driver = common::Driver::start(
        Store::open(&file, &key, later)?,
        common::options(vec![common::fixture(MAIL)]),
    );
    let status = driver.settle();
    let caught = status.catch_up.expect("a catch-up");
    assert_eq!(caught.gap_seconds, 14 * 3600);
    assert_eq!(caught.items, 3, "m2, m3 and m4: none read twice, none lost");
    assert_eq!(status.counts.episodes, 5);
    driver.stop();
    Ok(())
}

#[test]
fn at_rest_she_wakes_only_for_what_is_due() -> anyhow::Result<()> {
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        common::options(Vec::new()),
    );
    let first = driver.status().wakes;
    let again = driver.status().wakes;
    assert_eq!(again, first + 1, "one wake per line, none of her own");
    driver.stop();
    Ok(())
}
