//! The interface in French (#51): the column, quick settings, the
//! launcher and the palette say nothing in English. The same screens are shown in both languages; a
//! text the same in both is English left over, unless it is a name, a
//! number or a symbol.

use std::collections::BTreeSet;

use hocus_api::{Reply, Request};
use hocus_compositor::{FRAME, HarnessOptions, Headless, HeadlessConfig};

/// Texts the same in both languages: names, and words French shares.
const SAME: &[&str] = &[
    "Alex",
    "Bluetooth",
    "HEADLESS-1",
    "Agenda",
    "NOTIFICATIONS",
    "Notifications · 0",
    "Pandas",
    "Performance",
    // The slider's name and value, as the toolkit says it.
    "Volume 40%",
];

fn texts(language: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig::default(),
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
        polkit_helper: None,
        motion: false,
    })?;
    compositor.write_config(&format!("[language]\ninterface = \"{language}\"\n"))?;
    compositor.advance(FRAME * 4)?;
    let mut all = BTreeSet::new();
    let mut read = |compositor: &mut Headless| -> anyhow::Result<()> {
        let Reply::Ui(nodes) = compositor.request(Request::Ui)? else {
            anyhow::bail!("no interface");
        };
        all.extend(nodes.into_iter().filter_map(|n| n.label));
        Ok(())
    };
    read(&mut compositor)?;
    compositor.on_loop("quick settings", |state| state.toggle_quick());
    compositor.advance(FRAME * 4)?;
    read(&mut compositor)?;
    compositor.on_loop("launcher", |state| {
        state.toggle_quick();
        state.toggle_launcher();
    });
    compositor.advance(FRAME * 4)?;
    read(&mut compositor)?;
    compositor.on_loop("palette", |state| {
        state.toggle_launcher();
        state.toggle_palette();
    });
    compositor.advance(FRAME * 4)?;
    read(&mut compositor)?;
    Ok(all)
}

/// Whether a text is keys, which have the same names: `Super+P`.
fn keys(text: &str) -> bool {
    text.split('+')
        .all(|k| matches!(k, "Super" | "Ctrl" | "Alt" | "Shift") || k.chars().count() <= 2)
}

/// Whether a text says nothing in a language: digits, symbols, units.
fn wordless(text: &str) -> bool {
    !text
        .split(|c: char| !c.is_alphabetic())
        .any(|word| word.chars().count() > 2)
}

#[test]
fn the_shell_says_nothing_in_english() -> anyhow::Result<()> {
    let english = texts("en")?;
    let french = texts("fr")?;
    assert!(
        french.iter().any(|t| t.contains("aujourd")),
        "the interface is French: {french:?}"
    );
    let left: Vec<&String> = french
        .intersection(&english)
        .filter(|t| !wordless(t) && !keys(t) && !SAME.contains(&t.as_str()))
        .collect();
    assert!(left.is_empty(), "English left in French: {left:#?}");
    Ok(())
}
