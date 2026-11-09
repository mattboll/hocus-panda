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
        // Ollama's own API (#752): the same verdict, as it says it.
        "/api/chat" => {
            let v = verdict(seen);
            Answer::json(&json!({
                "message": v["choices"][0]["message"],
                "prompt_eval_count": v["usage"]["prompt_tokens"],
                "eval_count": v["usage"]["completion_tokens"],
            }))
        }
        "/api/ps" => Answer::json(&json!({"models": [
            {"name": "qwen3:8b", "model": "qwen3:8b", "size": 11_000_000_000_u64, "size_vram": 7_150_000_000_u64}
        ]})),
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
    assert_eq!(
        s.local.seen().iter().filter(|r| r.path == "/api/chat").count(),
        1,
        "one request, then a rest"
    );
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
    // Ollama's own API: its context asked (#752), the schema in `format`.
    let triages: Vec<&Seen> = seen.iter().filter(|r| r.path == "/api/chat").collect();
    assert_eq!(triages.len(), 2);
    assert!(!seen.iter().any(|r| r.path == "/v1/chat/completions"));
    for request in triages {
        let body = request.json();
        assert_eq!(body["model"], "qwen3:8b");
        assert_eq!(body["options"]["num_ctx"], 8192);
        assert_eq!(body["stream"], false);
        assert_eq!(body["format"]["properties"]["secrets"]["type"], "array");
        assert!(body.get("tools").is_none(), "the triage has no tools");
        // What pass 1 masks, the local model never sees.
        assert!(!request.body.contains("chienchat"));
        assert!(!request.body.contains("ghp_0123456789"));
    }
    assert!(s.anthropic.seen().is_empty(), "no triage goes to Claude");
    assert_eq!(
        triage(&s.driver.status()).placement.as_deref(),
        Some("tri : en partie sur le processeur"),
        "11 GB, 7 of them on the card"
    );
    s.driver.stop();
}

/// An OpenAI-compatible server (`kind = "openai"`) still gets the triage
/// on `/v1`, its schema in `response_format`; an item longer than the
/// context is cut, its start kept and the cut said, never a failure
/// (#752).
#[test]
fn an_openai_server_triages_on_v1_and_a_long_item_is_cut_to_the_context() {
    let local = local();
    let settings = Settings::of_config(&format!(
        "[secretary.models]\ntriage = \"lan/qwen3:8b\"\n\
         [secretary.providers.lan]\nkind = \"openai\"\nurl = \"{}/v1\"\n\
         [secretary.triage]\ncontext = 4096\n",
        local.url
    ))
    .expect("settings");
    let router = Router::new(Arc::new(settings::File::fixed(settings)), keys());
    let long = format!("Mail\nObjet : Léa\n{}FIN-DU-MAIL", "bla ".repeat(5000));
    router.triage(&long).expect("a verdict, cut or not");
    let seen = local.seen().pop().expect("a request");
    assert_eq!(seen.path, "/v1/chat/completions");
    let body = seen.json();
    assert_eq!(body["response_format"]["type"], "json_schema");
    assert!(body.get("options").is_none());
    let asked = body["messages"][1]["content"].as_str().unwrap_or_default();
    assert!(asked.starts_with("Mail\nObjet : Léa"));
    assert!(!asked.contains("FIN-DU-MAIL"), "its end cut");
    assert!(asked.ends_with("seul son début est lu]"), "the cut said");
    assert!(asked.chars().count() < 4096 * 3);
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
            event: None,
            mail: None,
            calendar: None,
            until: None,
            open: None,
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
            ..Synced::default()
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

/// Round 1 of a turn: Claude thinks (a signed block), then calls `graph`.
const THINKS_THEN_LOOKS: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":120}}}\n\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Chapitô ?\"}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig-sonnet\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"graph\",\"input\":{}}}\n\n\
data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"node\\\":\\\"Chapitô\\\",\\\"hops\\\":2,\\\"as_of\\\":null}\"}}\n\n\
data: {\"type\":\"content_block_stop\",\"index\":1}\n\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":40}}\n\n\
data: {\"type\":\"message_stop\"}\n\n";

/// The blocks' types of a request's turns.
fn block_types(seen: &Seen) -> Vec<String> {
    seen.json()["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|m| m["content"].as_array().cloned().unwrap_or_default())
        .filter_map(|b| b["type"].as_str().map(str::to_owned))
        .collect()
}

/// One model per answer (#750): the router's choice holds for a whole
/// turn, its tool loop included, so a change of the settings meanwhile
/// never sends Claude Sonnet's signed thinking to another model; the next
/// sentence takes the new choice, the earlier turns as plain text.
#[test]
fn a_turn_keeps_its_model_when_the_settings_change_meanwhile() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let config = dir.path().join("config.toml");
    let switch: Arc<std::sync::Mutex<Option<String>>> = Arc::default();
    let switched = switch.clone();
    let written = config.clone();
    let claude = Fake::start(move |_, n| {
        if n == 0 {
            // The user changes the answering model while Claude thinks.
            let text = switched
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            if let Some(text) = text {
                std::fs::write(&written, text).expect("written");
            }
            return Answer {
                status: 200,
                content_type: "text/event-stream",
                body: THINKS_THEN_LOOKS.to_owned(),
            };
        }
        Answer::stream(if n == 1 { "Nadia et Sophie." } else { "Léa." })
    });
    let providers = format!(
        "[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{}\"\n",
        claude.url
    );
    std::fs::write(
        &config,
        format!("[secretary]\nmode = \"assistant\"\n{providers}"),
    )?;
    *switch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(format!(
        "[secretary]\nmode = \"assistant\"\n[secretary.models]\n\
         answer = \"anthropic/claude-opus-5-5\"\n{providers}"
    ));
    let router = Arc::new(Router::new(
        Arc::new(settings::File::at(Some(config.clone()))),
        keys(),
    ));
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            model: Some(router),
            ..common::options(Vec::new())
        },
    );
    driver.settle();
    let answer = driver.answer_to("Qui est sur Chapitô ?");
    assert_eq!(answer, "Nadia et Sophie.");
    let seen = claude.seen();
    assert_eq!(
        seen.len(),
        2,
        "her turn: a look in her memory, then her answer"
    );
    let models: Vec<String> = seen
        .iter()
        .map(|s| s.json()["model"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(
        models,
        ["claude-sonnet-5-5", "claude-sonnet-5-5"],
        "the settings changed mid-turn: the turn stays on its model"
    );
    let replayed = seen[1].json()["messages"]
        .as_array()
        .and_then(|m| {
            m.iter()
                .find(|m| m["role"] == "assistant")
                .map(|m| m["content"][0].clone())
        })
        .unwrap_or_default();
    assert_eq!(replayed["type"], "thinking");
    assert_eq!(
        replayed["signature"], "sig-sonnet",
        "replayed to the model that signed it"
    );

    // The next sentence: the new model, the turn before as plain text.
    let answer = driver.answer_to("Et sur le site web ?");
    assert_eq!(answer, "Léa.");
    let next = claude.seen().pop().expect("a third request");
    assert_eq!(next.json()["model"], "claude-opus-5-5");
    let types = block_types(&next);
    assert!(
        !types
            .iter()
            .any(|t| t == "thinking" || t == "tool_use" || t == "tool_result"),
        "no block of another model's: {types:?}"
    );
    let messages = next.json()["messages"].clone();
    assert_eq!(messages[0]["role"], "user");
    assert!(messages.to_string().contains("Qui est sur Chapitô ?"));
    assert!(
        messages.to_string().contains("Nadia et Sophie."),
        "her last words, as text"
    );
    driver.stop();
    Ok(())
}

/// A turn's blocks given by another model go as plain text (#750).
#[test]
fn another_model_s_blocks_go_as_plain_text() {
    let (claude, local) = (anthropic(), local());
    let router = Router::new(
        Arc::new(settings::File::fixed(settings(&claude, &local))),
        keys(),
    );
    let mut theirs = Message::assistant("Elle a lu le mail.");
    theirs.native = Some(json!([
        {"type": "thinking", "thinking": "…", "signature": "sig-other"},
        {"type": "text", "text": "Elle a lu le mail."}
    ]));
    theirs.by = "anthropic/claude-opus-5-5".to_owned();
    let request = Request {
        role: Role::Answer,
        system: "Tu es la secrétaire.".to_owned(),
        messages: vec![
            Message::user("Le mail de Free ?"),
            theirs,
            Message::user("Et ensuite ?"),
        ],
        ..Request::default()
    };
    router
        .call(&request, &common::clock())
        .expect("a reply from Sonnet");
    let seen = claude.seen().pop().expect("a request");
    assert_eq!(seen.json()["model"], "claude-sonnet-5-5");
    assert_eq!(block_types(&seen), ["text", "text", "text"]);
    assert!(!seen.body.contains("sig-other"));
    // Its own blocks, by its own model: replayed as they were.
    let mut own = request.clone();
    own.messages[1].by = "anthropic/claude-sonnet-5-5".to_owned();
    router.call(&own, &common::clock()).expect("a reply");
    assert!(
        claude
            .seen()
            .pop()
            .expect("a request")
            .body
            .contains("sig-other")
    );
}

/// Her answers' key missing (#750): her status says so, and how to give
/// one; once in the keyring, the status goes back.
#[test]
fn a_missing_key_is_said_with_how_to_give_one() -> anyhow::Result<()> {
    let (claude, local) = (anthropic(), local());
    let router = Arc::new(Router::new(
        Arc::new(settings::File::fixed(settings(&claude, &local))),
        Arc::new(Given(BTreeMap::new())),
    ));
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            model: Some(router),
            ..common::options(Vec::new())
        },
    );
    let look = loop {
        if let hocus_secretary_link::FromSecretary::Look { look } = driver.receive()
            && look.tone == hocus_secretary_link::Tone::Warn
        {
            break look;
        }
    };
    assert_eq!(look.status, "Il me manque ma clé Anthropic");
    assert!(
        look.hint.contains("hocus-secretary key anthropic"),
        "{look:?}"
    );
    assert_eq!(look.tone, hocus_secretary_link::Tone::Warn);
    assert_eq!(look.model, "Claude Sonnet 5.5");
    driver.settle();
    driver.send(&hocus_secretary_link::ToSecretary::Ask {
        id: 7,
        text: "Bonjour".to_owned(),
    });
    let said = loop {
        if let hocus_secretary_link::FromSecretary::Item { item } = driver.receive()
            && let hocus_secretary_link::Body::Her { text, .. } = item.body
        {
            break text;
        }
    };
    assert!(said.contains("hocus-secretary key anthropic"), "{said}");
    assert!(claude.seen().is_empty(), "nothing sent without a key");
    driver.stop();
    Ok(())
}
