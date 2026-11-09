//! Her name (#752): `[secretary] name` in the configuration, followed
//! live: her face says it to the shell as soon as the file changes (her
//! drawer's header, her peek), and her persona's first line is it.

mod common;

use std::sync::Arc;

use hocus_secretary::{
    brain::{Brain, Options},
    model::{Message, Role},
    settings,
    store::Store,
};
use hocus_secretary_link::FromSecretary;

fn write(path: &std::path::Path, text: &str) {
    // Through a file beside it, as the Settings write.
    let new = path.with_extension("new");
    std::fs::write(&new, text).expect("written");
    std::fs::rename(&new, path).expect("renamed");
}

#[test]
fn her_name_is_followed_live_and_is_her_persona_s_first_line() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    write(&config, "[secretary]\nmode = \"assistant\"\n");
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            settings: Arc::new(settings::File::at(Some(config.clone()))),
            ..common::options(Vec::new())
        },
    );
    let first = loop {
        if let FromSecretary::Look { look } = driver.receive() {
            break look;
        }
    };
    assert_eq!(first.name, "", "none: the drawer says « Ta secrétaire »");
    driver.settle();
    // The Settings write her name: her face says it at once, nothing else
    // happening meanwhile.
    write(
        &config,
        "[secretary]\nmode = \"assistant\"\nname = \"  Margot \"\n",
    );
    let named = loop {
        if let FromSecretary::Look { look } = driver.receive()
            && !look.name.is_empty()
        {
            break look;
        }
    };
    assert_eq!(named.name, "Margot");
    driver.stop();

    let brain = Brain::new(
        Store::in_memory(common::clock())?,
        Options {
            settings: Arc::new(settings::File::at(Some(config))),
            ..common::options(Vec::new())
        },
    );
    let request = brain
        .request(Role::Answer, &[Message::user("Bonjour")])
        .map_err(anyhow::Error::msg)?;
    assert!(
        request.system.starts_with("Tu t'appelles Margot.\n"),
        "{}",
        &request.system[..80.min(request.system.len())]
    );
    Ok(())
}
