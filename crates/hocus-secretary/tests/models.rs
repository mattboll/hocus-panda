//! Her models (§11, lot 4), against fake servers that record every
//! request: the Anthropic Messages API and a local OpenAI-compatible
//! model server. No real API is called and no model runs.
//!
//! - AC21: the triage goes only to the local endpoint, with its JSON
//!   schema and no tools; on battery below the threshold the queue
//!   pauses and resumes on mains; the model is told to unload after two
//!   idle minutes.
//! - AC22: each role uses the provider and model set for it; a change
//!   applies to the next request without a restart.
//! - AC51: fail closed: an item not checked by the second pass leaves as
//!   its metadata only; after the check, its redacted text; a
//!   "summaries only" source, only the local summary.
//! - AC18 and AC19's recorder halves: no secret of a mail, no key of the
//!   keyring ever reaches a request body.

mod common;

use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};

use common::http::{Answer, Fake, Seen};
use hocus_secretary::{
    brain::Options,
    model::{Message, Model, Request, Role},
    power::Sensors,
    providers::{Given, Router, Secret},
    settings::{self, Settings},
    sources::{Fixture, Source},
    store::Store,
    triage::Triager,
};
use hocus_secretary_link::Status;
use serde_json::json;

/// A made-up key: it must reach the API's header and nothing else.
const KEY: &str = "sk-ant-test-made-up-0123456789";

/// The local model's verdict on an item: it flags the door code pass 1
/// misses, and writes it in its digest too (a model repeats what it saw).
fn verdict(seen: &Seen) -> serde_json::Value {
    let asked = seen.json()["messages"][1]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let content = if asked.contains("Free") {
        json!({
            "for_me": true, "project": null, "urgency": "normal", "needs_reply": false,
            "digest": "Free donne l'accès : entre 4417 sur le clavier",
            "commitments": [], "facts": [], "suspicious": false,
            "secrets": [{"text": "4417", "kind": "door-code"}]
        })
    } else {
        json!({
            "for_me": true, "project": "Chapitô", "urgency": "low", "needs_reply": true,
            "digest": "Léa demande la recette de jeudi",
            "commitments": [], "facts": [], "suspicious": false, "secrets": []
        })
    };
    json!({
        "choices": [{"message": {"role": "assistant", "content": content.to_string()}}],
        "usage": {"prompt_tokens": 300, "completion_tokens": 60}
    })
}

/// The local model's server.
fn local() -> Fake {
    Fake::start(|seen, _| match seen.path.as_str() {
        "/v1/chat/completions" => Answer::json(&verdict(seen)),
        "/api/generate" => Answer::json(&json!({"done": true})),
        _ => Answer::status(404, "{}"),
    })
}

fn anthropic() -> Fake {
    Fake::start(|_, _| Answer::stream("Le mail de Free contient un code : je l'ai masqué."))
}

fn settings(anthropic: &Fake, local: &Fake) -> Settings {
    let mut text = format!(
        "[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{}\"\n\
         [secretary.providers.local]\nkind = \"ollama\"\nurl = \"{}/v1\"\n",
        anthropic.url, local.url
    );
    text.push_str("[secretary.triage]\npace = 2\n");
    Settings::of_config(&text).expect("settings")
}

fn keys() -> Arc<Given> {
    Arc::new(Given(BTreeMap::from([(
        "anthropic".to_owned(),
        Secret::new(KEY),
    )])))
}

/// A laptop's power supplies: its mains `online`, its battery's charge.
fn power(dir: &Path, online: bool, charge: u8) -> Sensors {
    let supplies = dir.join("power_supply");
    for (name, kind) in [("AC", "Mains"), ("BAT0", "Battery")] {
        std::fs::create_dir_all(supplies.join(name)).expect("a folder");
        std::fs::write(supplies.join(name).join("type"), kind).expect("written");
    }
    std::fs::write(supplies.join("AC/online"), if online { "1" } else { "0" }).expect("written");
    std::fs::write(supplies.join("BAT0/capacity"), charge.to_string()).expect("written");
    Sensors {
        supplies,
        thermal: dir.join("thermal"),
    }
}

const MAILS: &str = r#"
name = "fixture-mail"
notebook = "pro"
every = 3600
[[items]]
id = "free"
at = "2026-11-09T07:00:00Z"
kind = "mail"
digest = "Free : vos accès"
sender = "Free <assistance@free.example>"
body = "Bonjour, le mot de passe du wifi : chienchat. Pour l'immeuble, entre 4417 sur le clavier en bas et pousse fort. Votre jeton : ghp_0123456789abcdefghijABCDEFGHIJ012345"
[[items]]
id = "lea"
at = "2026-11-09T07:30:00Z"
kind = "mail"
digest = "Léa : la recette de jeudi"
sender = "Léa <lea@atelier-lune.example>"
body = "Tu peux valider la recette jeudi ? Le ticket est CHAP-412."
"#;

const SECRETS: [&str; 4] = [
    "chienchat",
    "4417",
    "ghp_0123456789abcdefghijABCDEFGHIJ012345",
    KEY,
];

fn bodies(fake: &Fake) -> Vec<String> {
    fake.seen().into_iter().map(|s| s.body).collect()
}

fn assert_no_secret(fake: &Fake) {
    for seen in fake.seen() {
        for secret in SECRETS {
            assert!(
                !seen.body.contains(secret),
                "{secret:?} reached {}: {}",
                seen.path,
                seen.body
            );
        }
    }
}

struct Setup {
    anthropic: Fake,
    local: Fake,
    driver: common::Driver,
    dir: tempfile::TempDir,
}

fn start(fixtures: &[&str], online: bool, charge: u8) -> Setup {
    let (anthropic, local) = (anthropic(), local());
    let dir = tempfile::tempdir().expect("a folder");
    let settings = Arc::new(settings::File::fixed(settings(&anthropic, &local)));
    let mut router = Router::new(settings.clone(), keys());
    router.retry = Duration::from_millis(10);
    let router = Arc::new(router);
    let fixtures: Vec<Fixture> = fixtures.iter().map(|f| common::fixture(f)).collect();
    let options = Options {
        model: Some(router.clone()),
        triage: Some(router as Arc<dyn Triager>),
        settings,
        sensors: power(dir.path(), online, charge),
        ..common::options(fixtures)
    };
    let mut driver =
        common::Driver::start(Store::in_memory(common::clock()).expect("a store"), options);
    driver.settle();
    Setup {
        anthropic,
        local,
        driver,
        dir,
    }
}

fn triage(status: &Status) -> hocus_secretary_link::TriageStatus {
    status.triage.clone().expect("a triage queue")
}

#[test]
fn ac21_the_triage_runs_locally_paced_on_mains_and_unloads_when_idle() {
    let mut s = start(&[MAILS], false, 20);
    let status = s.driver.status();
    assert_eq!(triage(&status).state, "paused: battery 20 %");
    assert_eq!(triage(&status).pending, 2);
    assert!(s.local.seen().is_empty(), "nothing sent on battery");

    // Mains again: the queue resumes at its next look, one item at a time.
    power(s.dir.path(), true, 20);
    s.driver.advance(60);
    s.driver
        .until("one item triaged", |st| triage(st).done == 1);
    assert_eq!(s.local.seen().len(), 1, "one request, then a rest");
    s.driver.advance(2);
    let status = s.driver.until("both triaged", |st| triage(st).done == 2);
    assert_eq!(triage(&status).pending, 0);

    // Two idle minutes after its last rest: the model is told to unload.
    s.driver.advance(121);
    assert!(!s.local.seen().iter().any(|r| r.path == "/api/generate"));
    s.driver.advance(1);
    let start = std::time::Instant::now();
    while !s.local.seen().iter().any(|r| r.path == "/api/generate") {
        assert!(start.elapsed() < Duration::from_secs(20), "never unloaded");
        std::thread::sleep(Duration::from_millis(5));
    }
    let seen = s.local.seen();
    let unload = seen
        .iter()
        .find(|r| r.path == "/api/generate")
        .expect("an unload");
    assert_eq!(unload.json(), json!({"model": "qwen3:8b", "keep_alive": 0}));
    let triages: Vec<&Seen> = seen
        .iter()
        .filter(|r| r.path == "/v1/chat/completions")
        .collect();
    assert_eq!(triages.len(), 2);
    for request in triages {
        let body = request.json();
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["schema"]["properties"]["secrets"]["type"],
            "array"
        );
        assert!(body.get("tools").is_none(), "the triage has no tools");
        // What pass 1 masks, the local model never sees.
        assert!(!request.body.contains("chienchat"));
        assert!(!request.body.contains("ghp_0123456789"));
    }
    assert!(s.anthropic.seen().is_empty(), "no triage goes to Claude");
    s.driver.stop();
}

#[test]
fn ac51_an_unchecked_mail_leaves_as_its_metadata_then_its_redacted_text() {
    let mut s = start(&[MAILS], false, 20);
    let answer = s.driver.answer_to("Que dit le mail de Free ?");
    assert_eq!(answer, "Le mail de Free contient un code : je l'ai masqué.");
    let first = bodies(&s.anthropic).pop().expect("a request");
    assert!(
        first.contains("peut contenir un secret, pas encore vérifié"),
        "{first}"
    );
    assert!(
        first.contains("Free : vos accès"),
        "its subject leaves: {first}"
    );
    assert!(!first.contains("clavier"), "not its text: {first}");

    power(s.dir.path(), true, 20);
    s.driver.advance(60);
    s.driver.until("checked", |st| triage(st).done >= 1);
    s.driver.answer_to("Que dit le mail de Free ?");
    let after = bodies(&s.anthropic).pop().expect("a request");
    assert!(
        after.contains("pousse fort"),
        "its text, once checked: {after}"
    );
    assert!(
        after.contains("[secret masqué — voir le mail free]"),
        "{after}"
    );
    assert_no_secret(&s.anthropic);

    let request = s.anthropic.seen().pop().expect("a request");
    assert_eq!(request.header("x-api-key"), Some(KEY));
    assert_eq!(request.header("anthropic-version"), Some("2026-10-21"));
    let body = request.json();
    assert_eq!(body["model"], "claude-sonnet-5-5");
    assert_eq!(body["thinking"]["type"], "adaptive");
    assert_eq!(body["system"][1]["cache_control"]["ttl"], "1h");
    assert!(
        body["tools"]
            .as_array()
            .is_some_and(|t| t.iter().all(|t| t["strict"] == true))
    );
    assert!(
        !body["system"].to_string().contains("<source id="),
        "content stays out of the system"
    );
    s.driver.stop();
}

#[test]
fn ac51_a_summaries_only_source_gives_only_the_local_summary() {
    let only = MAILS.replace("every = 3600\n", "every = 3600\nsummaries-only = true\n");
    let mut s = start(&[&only], true, 90);
    s.driver.until("one checked", |st| triage(st).done == 1);
    s.driver.advance(2);
    s.driver.until("both checked", |st| triage(st).done == 2);
    s.driver.answer_to("Que dit le mail de Free ?");
    let body = bodies(&s.anthropic).pop().expect("a request");
    assert!(body.contains("résumé local seulement"), "{body}");
    assert!(
        body.contains("Free donne l'accès"),
        "the local summary: {body}"
    );
    assert!(!body.contains("pousse fort"), "never the text: {body}");
    assert!(!body.contains("mot de passe du wifi"), "{body}");
    assert_no_secret(&s.anthropic);
    s.driver.stop();
}

/// A configuration file, written whole.
fn write(path: &Path, text: &str) {
    std::fs::write(path, text).expect("written");
}

#[test]
fn ac22_each_role_uses_its_provider_and_model_and_a_change_applies_at_once() {
    let (anthropic, local) = (anthropic(), local());
    let dir = tempfile::tempdir().expect("a folder");
    let config = dir.path().join("config.toml");
    let providers = format!(
        "[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{}\"\n\
         [secretary.providers.big]\nkind = \"openai\"\nurl = \"{}/v1\"\n\
         [secretary.providers.local]\nkind = \"ollama\"\nurl = \"{}/v1\"\n",
        anthropic.url, local.url, local.url
    );
    write(
        &config,
        &format!(
            "[secretary]\nmode = \"assistant\"\n[secretary.models]\n\
             briefing = \"anthropic/claude-opus-5-5\"\n{providers}"
        ),
    );
    let file = Arc::new(settings::File::at(Some(config.clone())));
    let router = Router::new(file, keys());
    let clock = common::clock();
    let ask = |role: Role| Request {
        role,
        system: "Tu es la secrétaire.".to_owned(),
        messages: vec![Message::user("Bonjour")],
        ..Request::default()
    };
    let reply = router.call(&ask(Role::Answer), &clock).expect("a reply");
    assert_eq!(
        (reply.provider.as_str(), reply.model.as_str()),
        ("anthropic", "claude-sonnet-5-5")
    );
    router.call(&ask(Role::Briefing), &clock).expect("a reply");
    let models: Vec<String> = anthropic
        .seen()
        .iter()
        .map(|s| s.json()["model"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(models, ["claude-sonnet-5-5", "claude-opus-5-5"]);
    assert_eq!(
        router.endpoint(),
        ("local".to_owned(), "qwen3:8b".to_owned())
    );

    // Settings switch the answer to another provider: the next request
    // goes there, no restart.
    write(
        &config,
        &format!(
            "[secretary]\nmode = \"assistant\"\n[secretary.models]\n\
             answer = \"big/org/model-70b\"\ntriage = \"local/qwen3:4b\"\n{providers}"
        ),
    );
    let reply = router.call(&ask(Role::Answer), &clock).expect("a reply");
    assert_eq!(
        (reply.provider.as_str(), reply.model.as_str()),
        ("big", "org/model-70b")
    );
    let last = local.seen().pop().expect("a request to it");
    assert_eq!(last.path, "/v1/chat/completions");
    assert_eq!(last.json()["model"], "org/model-70b");
    assert_eq!(anthropic.seen().len(), 2, "not to Claude any more");
    assert_eq!(
        router.endpoint(),
        ("local".to_owned(), "qwen3:4b".to_owned())
    );
    assert!(!local.seen().iter().any(|s| s.body.contains(KEY)));
}

#[test]
fn the_triage_refuses_a_model_off_this_machine() {
    let anthropic = anthropic();
    let settings = Settings::of_config(&format!(
        "[secretary.models]\ntriage = \"anthropic/claude-sonnet-5-5\"\n\
         [secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{}\"\n",
        anthropic.url
    ))
    .expect("settings");
    let router = Router::new(Arc::new(settings::File::fixed(settings)), keys());
    assert!(router.triage("Mail\nObjet : x").is_err());
    assert!(anthropic.seen().is_empty());
}

#[test]
fn an_overloaded_api_is_tried_again_and_a_refused_request_is_not() {
    let anthropic = Fake::start(|_, before| {
        if before < 2 {
            Answer::status(
                529,
                r#"{"type":"error","error":{"type":"overloaded_error"}}"#,
            )
        } else {
            Answer::stream("Bonjour.")
        }
    });
    let local = local();
    let mut router = Router::new(
        Arc::new(settings::File::fixed(settings(&anthropic, &local))),
        keys(),
    );
    router.retry = Duration::from_millis(5);
    let request = Request {
        system: "x".to_owned(),
        messages: vec![Message::user("Bonjour")],
        ..Request::default()
    };
    let mut parts = Vec::new();
    let reply = router
        .stream(&request, &common::clock(), &mut |t| {
            parts.push(t.to_owned())
        })
        .expect("a reply after two tries");
    assert_eq!(reply.text, "Bonjour.");
    assert_eq!(parts.concat(), "Bonjour.");
    assert_eq!(anthropic.seen().len(), 3);

    let refusing = Fake::start(|_, _| {
        Answer::status(
            400,
            r#"{"type":"error","error":{"type":"invalid_request_error"}}"#,
        )
    });
    let router = Router::new(
        Arc::new(settings::File::fixed(settings(&refusing, &local))),
        keys(),
    );
    let err = router
        .call(&request, &common::clock())
        .expect_err("refused");
    assert!(err.contains("400"), "{err}");
    assert_eq!(refusing.seen().len(), 1, "a refusal is not tried again");

    // No key in the keyring: no request at all.
    let router = Router::new(
        Arc::new(settings::File::fixed(settings(&refusing, &local))),
        Arc::new(Given::default()),
    );
    assert!(router.call(&request, &common::clock()).is_err());
    assert_eq!(refusing.seen().len(), 1);
}

#[test]
fn a_fixture_mail_is_redacted_by_the_first_pass_as_it_is_read() {
    let mut fixture = common::fixture(MAILS);
    let synced = fixture
        .sync(None, "2026-11-09T08:00:00Z".parse().expect("a time"))
        .expect("a sync");
    let free = synced
        .items
        .iter()
        .find(|i| i.id == "free")
        .expect("Free's");
    let text = free.text.as_ref().expect("its text");
    let hocus_sources::Content::Mail(mail) = &text.item.content else {
        panic!("a mail");
    };
    let body = mail.body.clone().unwrap_or_default();
    assert!(
        !body.contains("chienchat") && !body.contains("ghp_"),
        "{body}"
    );
    assert!(body.contains("4417"), "pass 1 misses it: pass 2's");
    assert_eq!(text.check, hocus_sources::Check::FirstPass);
}

/// Google's accounts as her one source gives them: items from several
/// parts of it, a mail with its text; then a part revoked.
#[derive(Debug)]
struct Accounts {
    round: u32,
}

impl Source for Accounts {
    fn name(&self) -> &str {
        "google"
    }

    fn notebook(&self) -> hocus_secretary_link::Notebook {
        hocus_secretary_link::Notebook::Pro
    }

    fn every(&self) -> Duration {
        Duration::from_secs(300)
    }

    fn sync(
        &mut self,
        _cursor: Option<&str>,
        now: jiff::Timestamp,
    ) -> Result<hocus_secretary::sources::Synced, String> {
        use hocus_secretary::sources::{Item, Synced};
        use hocus_sources::{Content, Pointer, Redactor, SourceKey, SourceKind, item, item::Mail};
        self.round += 1;
        if self.round > 1 {
            return Ok(Synced {
                cursor: "2".to_owned(),
                forget: vec!["account_1:mail".to_owned()],
                ..Synced::default()
            });
        }
        let mail = Redactor::new().redact(item::Item {
            pointer: Pointer {
                source: SourceKey::new("account_1", SourceKind::Mail),
                id: "INBOX/7/42".to_owned(),
                label: "le mail de Free".to_owned(),
                open: None,
            },
            notebook: hocus_secretary_link::Notebook::Perso,
            at: now,
            content: Content::Mail(Mail {
                from: "Free".to_owned(),
                subject: "Vos accès".to_owned(),
                body: Some("mdp du wifi : chienchat".to_owned()),
                ..Mail::default()
            }),
        });
        let item = |id: &str, from: &str, digest: &str| Item {
            id: id.to_owned(),
            at: now,
            kind: "mail".to_owned(),
            digest: digest.to_owned(),
            notebook: Some(hocus_secretary_link::Notebook::Perso),
            from: Some(from.to_owned()),
            project: None,
            sender: None,
            body: None,
            text: None,
            summaries_only: false,
        };
        let mut with_text = item("INBOX/7/42", "account_1:mail", "Free : Vos accès");
        with_text.text = Some(Box::new(mail));
        Ok(Synced {
            items: vec![
                with_text,
                item("e1", "account_2:calendar", "Dîner chez Claire"),
            ],
            cursor: "1".to_owned(),
            forget: Vec::new(),
        })
    }
}

#[test]
fn a_source_s_items_reach_her_memory_by_their_part_and_a_revoked_part_is_forgotten() {
    let (anthropic, local) = (anthropic(), local());
    let dir = tempfile::tempdir().expect("a folder");
    let settings = Arc::new(settings::File::fixed(settings(&anthropic, &local)));
    let router = Arc::new(Router::new(settings.clone(), keys()));
    let mut options = common::options(Vec::new());
    options.sources = vec![Box::new(Accounts { round: 0 })];
    options.triage = Some(router);
    options.settings = settings;
    options.sensors = power(dir.path(), false, 10);
    let mut driver =
        common::Driver::start(Store::in_memory(common::clock()).expect("a store"), options);
    let status = driver.settle();
    assert_eq!(status.counts.episodes, 2);
    assert_eq!((triage(&status).held, triage(&status).pending), (1, 1));

    let status = driver.advance(300);
    assert_eq!(
        status.counts.episodes, 1,
        "the revoked part's episode is gone"
    );
    assert_eq!((triage(&status).held, triage(&status).pending), (0, 0));
    assert!(local.seen().is_empty() && anthropic.seen().is_empty());
    driver.stop();
}
