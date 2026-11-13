//! The crate's integration tests, one program (#765): each file of this
//! directory is a module here, and what they share is built once. A test
//! is named `MODULE::TEST`; `cargo nextest run -p hocus-compositor MODULE::` runs
//! a file's.
//!
//! nextest runs each test in a process of its own, and the tests count on
//! it (environment variables, a process's working directory, its
//! children): `cargo test` runs them as threads of one process, and is not
//! supported.

// A test ends on what it did not expect: its helpers too, which clippy
// does not take for tests (clippy.toml covers `#[test]` functions only).
#![allow(clippy::expect_used, clippy::indexing_slicing, clippy::panic)]

mod agents_bed;
mod common;
mod koi;
mod settings_page;

// The fake system services of the bar's levels (tests/levels/), and the
// day's fake sources (hocus-day's tests).
#[path = "levels/accounts.rs"]
mod accounts;
#[path = "levels/bluez.rs"]
mod bluez;
#[path = "../../hocus-day/tests/fakes/mod.rs"]
mod day_fakes;
#[path = "levels/fakes.rs"]
mod fakes;
#[path = "levels/nm.rs"]
mod nm;
#[path = "levels/rfkill.rs"]
mod rfkill;
#[path = "levels/timedate.rs"]
mod timedate;

mod actions;
mod agents;
mod agents_live;
mod annotate;
mod appearance;
mod autostart;
mod away;
mod away_shots;
mod bar;
mod bar_desktops;
mod bar_front;
mod bar_keys;
mod bar_live;
mod bar_order;
mod bar_side;
mod bar_windows;
mod bluetooth;
mod calendar;
mod captures_live;
mod child_signals;
mod clipboard;
mod contrast;
mod creatures;
mod cursor;
mod day;
mod day_sources;
mod daylight;
mod decorations;
mod desktop_drop;
mod desktops;
mod desktops_live;
mod desktops_screens;
mod desktops_shots;
mod effects;
mod emoji;
mod featured;
mod fireflies;
mod footer;
mod foreign;
mod garland;
mod gestures;
mod glass;
mod headless;
mod holidays;
mod idle;
mod in_use;
mod ink;
mod launcher;
mod layouts;
mod levels;
mod link;
mod live_reload;
mod lock;
mod lock_external;
mod lock_shots;
mod magnet;
mod magnifier;
mod mcp;
mod meeting;
mod moving_wallpaper;
mod music;
mod night_light;
mod notifications;
mod overview;
mod panic_hook;
mod perf;
mod pointer_focus;
mod polkit;
mod popover;
mod power;
mod power_actions;
mod radial;
mod rain;
mod rain_on_glass;
mod recording;
mod reminders;
mod scales;
mod screen_reader;
mod screens;
mod screens_return;
mod screenshots;
mod seams;
mod session;
mod session_menu;
mod session_restore;
mod settings;
mod settings_account;
mod settings_acting;
mod settings_agents;
mod settings_appearance;
mod settings_applications;
mod settings_bar;
mod settings_bluetooth;
mod settings_captures;
mod settings_desktops;
mod settings_keyboard;
mod settings_mouse;
mod settings_network;
mod settings_perf;
mod settings_power;
mod settings_screens;
mod settings_sound;
mod settings_time;
mod settings_wallpaper;
mod sharing;
mod snow;
mod tasks;
mod theme_shots;
mod themed_bar;
mod today;
mod tray;
mod typing;
mod unplug;
mod unplug_clients;
mod wifi;
mod window_effects;
mod window_opacity;
mod window_rules;
mod windows;
mod windows_keys;

/// A test file that is not a module here is not built, and its tests would
/// be missed in silence (the crate's `autotests = false`).
#[test]
fn every_test_file_is_a_module() {
    let dir =
        std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_else(|| env!("CARGO_MANIFEST_DIR").into());
    let modules = include_str!("main.rs");
    let files = std::fs::read_dir(std::path::Path::new(&dir).join("tests")).expect("tests/");
    for path in files.map(|entry| entry.expect("an entry of tests/").path()) {
        let name = path.file_stem().and_then(|n| n.to_str()).expect("a name");
        if path.extension().is_some_and(|e| e == "rs") && name != "main" {
            assert!(
                modules.contains(&format!("\nmod {name};\n")),
                "tests/{name}.rs is not a module of tests/main.rs: its tests do not run"
            );
        }
    }
}
