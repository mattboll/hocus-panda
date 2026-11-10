//! Her answers' key given in her drawer or the Settings (#752), against a
//! fake Anthropic API on this machine: never the network. A good key is
//! checked by the smallest request there is and kept in her keyring; her
//! face stops asking for it, and the shells hear its clipboard digest
//! only. A malformed key is said so without a request; a refused one
//! "clé refusée par Anthropic"; an API out of reach "pas de réseau". The
//! key is in no line she says, no audit row, no thread item.

mod common;

use std::sync::Arc;

use common::http::{Answer, Fake};
use hocus_secretary::{
    brain::Options,
    providers::{Held, Keys as _, Router},
    settings::{self, Settings},
    store::Store,
};
use hocus_secretary_link::{FromSecretary, KeyOutcome, Secret, ToSecretary, clip_digest};

const GOOD: &str = "sk-ant-api03-test-made-up-good";
const BAD: &str = "sk-ant-api03-test-made-up-refused";

fn settings(url: &str) -> Settings {
    Settings::of_config(&format!(
        "[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{url}\"\n"
    ))
    .expect("settings")
}

/// What the fake API says to `GET /v1/models`, by the key it is given.
fn api() -> Fake {
    Fake::start(|seen, _| {
        if seen.method != "GET" || !seen.path.starts_with("/v1/models") {
            return Answer::status(404, "{}");
        }
        match seen.header("x-api-key") {
            Some(GOOD) => Answer::json(&serde_json::json!({"data": [{"id": "claude-sonnet-5-5"}]})),
            _ => Answer::status(
                401,
                r#"{"type":"error","error":{"type":"authentication_error"}}"#,
            ),
        }
    })
}

fn given(driver: &mut common::Driver, key: Option<&str>) -> (KeyOutcome, Vec<FromSecretary>) {
    let mut before = Vec::new();
    let id = 900 + u64::from(key.map_or(0, |k| k.len() as u8));
    driver.send(&ToSecretary::Key {
        id,
        provider: "anthropic".to_owned(),
        key: key.map(|k| Secret::new(k.to_owned())),
    });
    loop {
        match driver.receive() {
            FromSecretary::Keyed { id: i, outcome, .. } if i == id => return (outcome, before),
            other => before.push(other),
        }
    }
}

#[test]
fn a_key_given_to_her_is_checked_kept_and_never_said_back() -> anyhow::Result<()> {
    let fake = api();
    let held = Arc::new(Held::default());
    let router = Arc::new(Router::new(
        Arc::new(settings::File::fixed(settings(&fake.url))),
        held.clone(),
    ));
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            model: Some(router),
            ..common::options(Vec::new())
        },
    );
    // Her face asks for it: her drawer shows its field.
    let look = loop {
        if let FromSecretary::Look { look } = driver.receive()
            && look.key.is_some()
        {
            break look;
        }
    };
    assert_eq!(look.key.as_deref(), Some("anthropic"));
    let status = driver.status();
    assert_eq!(status.key_provider.as_deref(), Some("anthropic"));
    assert!(status.key_missing);

    // Malformed: said at once, nothing asked of the API.
    let (outcome, _) = given(&mut driver, Some("ma clé"));
    assert_eq!(outcome, KeyOutcome::Malformed);
    assert!(fake.seen().is_empty());
    assert_eq!(
        outcome.words("anthropic", true),
        "Ce n’est pas une clé Anthropic : elle commence par sk-ant-."
    );
    // Refused by the API.
    let (outcome, _) = given(&mut driver, Some(BAD));
    assert_eq!(outcome, KeyOutcome::Refused);
    assert_eq!(
        outcome.words("anthropic", true),
        "Anthropic refuse cette clé. Vérifie qu’elle est complète, ou crée-en une autre dans la console."
    );
    assert!(held.key("anthropic").map_err(anyhow::Error::msg)?.is_none());
    // The good one: one tiny request, kept, her face asks no more.
    let (outcome, said) = given(&mut driver, Some(&format!("  {GOOD}\n")));
    assert_eq!(outcome, KeyOutcome::Kept);
    let asked = fake.seen().pop().expect("a check");
    assert_eq!(asked.method, "GET");
    assert!(
        asked.path.starts_with("/v1/models?limit=1"),
        "{}",
        asked.path
    );
    assert_eq!(asked.header("x-api-key"), Some(GOOD));
    assert_eq!(
        held.key("anthropic")
            .map_err(anyhow::Error::msg)?
            .map(|k| k.expose().to_owned())
            .as_deref(),
        Some(GOOD),
        "kept trimmed"
    );
    assert!(
        said.iter().any(
            |m| matches!(m, FromSecretary::ClipSecret { digest } if *digest == clip_digest(GOOD))
        ),
        "the shells hear its digest: {said:?}"
    );
    let look = loop {
        if let FromSecretary::Look { look } = driver.receive()
            && look.key.is_none()
        {
            break look;
        }
    };
    assert!(!look.status.contains("clé"), "{look:?}");
    assert!(!driver.status().key_missing);
    // Erased: her face asks for it again.
    let (outcome, _) = given(&mut driver, None);
    assert_eq!(outcome, KeyOutcome::Erased);
    assert!(held.key("anthropic").map_err(anyhow::Error::msg)?.is_none());
    let store = driver.stop();
    // Nothing of it in her memory: the audit, the thread, the journal.
    let dump = format!("{:?}", store.audit_lines()?);
    assert!(dump.contains("key"), "audited: {dump}");
    assert!(!dump.contains(GOOD) && !dump.contains(BAD), "{dump}");
    Ok(())
}

#[test]
fn an_api_out_of_reach_is_said_as_no_network() -> anyhow::Result<()> {
    // Nothing listens there.
    let router = Arc::new(Router::new(
        Arc::new(settings::File::fixed(settings("http://127.0.0.1:9"))),
        Arc::new(Held::default()),
    ));
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            model: Some(router),
            ..common::options(Vec::new())
        },
    );
    let (outcome, _) = given(&mut driver, Some(GOOD));
    assert_eq!(outcome, KeyOutcome::Offline);
    assert!(
        outcome
            .words("anthropic", true)
            .starts_with("Pas de réseau")
    );
    driver.stop();
    Ok(())
}
