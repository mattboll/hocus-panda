//! What the compositor said before its words moved to the catalogue
//! (#775), copied from the code as it was: every key, French then
//! English.

pub const BEFORE: &[(&str, &str, &str)] = &[
    (
        "compositor.capture_save.screenshots",
        "Captures d'écran",
        "Screenshots",
    ),
    ("compositor.magnifier_draw.magnifier", "Loupe", "Magnifier"),
    (
        "compositor.magnifier_draw.super_esc_to_exit",
        "Super+Échap quitte",
        "Super+Esc to exit",
    ),
    ("compositor.radial_draw.click", "clic", "click"),
    (
        "compositor.record_save.screen_recordings",
        "Enregistrements d'écran",
        "Screen recordings",
    ),
    ("compositor.session_restore.and", " et ", " and "),
];

/// The format strings of arms moved, placeholders aside.
pub const FORMATS: &[(&str, &str, &str)] = &[
    (
        "compositor.away_power.power_saver_on_battery_at",
        "Économie d’énergie activée : batterie à {percent} %.",
        "Power saver on: battery at {percent} %.",
    ),
    (
        "compositor.capture_save.screenshot_from",
        "Capture du {day} à {time}.{extension}",
        "Screenshot from {day} {time}.{extension}",
    ),
    ("compositor.opacity.text", "{percent} %", "{percent}%"),
    ("compositor.radial_draw.desktop", "Bureau {}", "Desktop {}"),
    (
        "compositor.record_save.screen_recording_from",
        "Enregistrement du {day} à {time}.{ext}",
        "Screen recording from {day} {time}.{ext}",
    ),
];

/// Every key the compositor moved to the catalogue says, byte for byte,
/// what its pair said in the code before (#775).
#[test]
fn the_catalogue_says_what_the_code_said() {
    hocus_words::check_before(BEFORE, FORMATS);
}
