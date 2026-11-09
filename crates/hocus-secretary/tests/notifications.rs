//! Slack through its notifications (lot 7, §10.1, AC16, AC17): the
//! notifications of the applications the user consented to, as the shell
//! forwards them, go through the first pass and the triage queue like a
//! mail, each in its application's notebook; another application's are
//! dropped unread; a revoked one is forgotten within one cycle. No request
//! goes anywhere but to the two fake model servers: none reaches Slack.
//!
//! That she never invokes a notification's action, nor closes or marks one,
//! is the compositor's test (`hocus-compositor/tests/secretary_notifications.rs`),
//! with the real shell on a private bus.

mod common;

use std::{collections::BTreeMap, path::Path, sync::Arc};

use common::http::{Answer, Fake, Seen};
use hocus_secretary::{
    brain::Options,
    notifications::{self, Notifications},
    power::Sensors,
    providers::{Given, Router, Secret},
    settings::{self, Settings},
    sources::Source,
    store::Store,
    triage::Triager,
};
use hocus_secretary_link::{
    Notebook, Notification, Status, ToSecretary,
    consent::{ConsentBook, SourceKey},
};
use serde_json::json;

const KEY: &str = "sk-ant-test-made-up-0123456789";
const TOKEN: &str = "ghp_0123456789abcdefghijABCDEFGHIJ012345";

/// The local model's verdict: Karim's message is for the user.
fn verdict(seen: &Seen) -> serde_json::Value {
    let asked = seen.json()["messages"][1]["content"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let digest = if asked.contains("Karim") {
        "Karim : la prod est tombée, il demande de regarder la CI"
    } else {
        "Claire propose dimanche"
    };
    let content = json!({
        "for_me": true, "project": null, "urgency": "high", "needs_reply": true,
        "digest": digest, "commitments": [], "facts": [], "suspicious": false, "secrets": []
    });
    json!({
        "choices": [{"message": {"role": "assistant", "content": content.to_string()}}],
        "usage": {"prompt_tokens": 200, "completion_tokens": 40}
    })
}

fn local() -> Fake {
    Fake::start(|seen, _| match seen.path.as_str() {
        "/v1/chat/completions" => Answer::json(&verdict(seen)),
        "/api/generate" => Answer::json(&json!({"done": true})),
        _ => Answer::status(404, "{}"),
    })
}

fn anthropic() -> Fake {
    Fake::start(|_, _| Answer::stream("Karim dit que la prod est tombée."))
}

fn power(dir: &Path, online: bool) {
    let supplies = dir.join("power_supply");
    for (name, kind) in [("AC", "Mains"), ("BAT0", "Battery")] {
        std::fs::create_dir_all(supplies.join(name)).expect("a folder");
        std::fs::write(supplies.join(name).join("type"), kind).expect("written");
    }
    std::fs::write(supplies.join("AC/online"), if online { "1" } else { "0" }).expect("written");
    std::fs::write(supplies.join("BAT0/capacity"), "20").expect("written");
}

/// The consent book: Slack in the pro notebook, Signal in the perso one;
/// Thunderbird never consented to.
fn consent(path: &Path) -> ConsentBook {
    let mut book = ConsentBook::default();
    let at = common::START.parse().expect("a time");
    book.allow(
        SourceKey::notifications("Slack", Some("slack")),
        "Slack",
        Notebook::Pro,
        at,
    );
    book.allow(
        SourceKey::notifications("Signal", None),
        "Signal",
        Notebook::Perso,
        at,
    );
    book.save(path).expect("saved");
    book
}

struct Setup {
    anthropic: Fake,
    local: Fake,
    driver: common::Driver,
    dir: tempfile::TempDir,
}

fn start(online: bool) -> Setup {
    let (anthropic, local) = (anthropic(), local());
    let dir = tempfile::tempdir().expect("a folder");
    consent(&dir.path().join("consents.json"));
    power(dir.path(), online);
    let text = format!(
        "[secretary.providers.anthropic]\nkind = \"anthropic\"\nurl = \"{}\"\n\
         [secretary.providers.local]\nkind = \"ollama\"\nurl = \"{}/v1\"\n\
         [secretary.triage]\npace = 2\n",
        anthropic.url, local.url
    );
    let settings = Arc::new(settings::File::fixed(
        Settings::of_config(&text).expect("settings"),
    ));
    let keys = Arc::new(Given(BTreeMap::from([(
        "anthropic".to_owned(),
        Secret::new(KEY),
    )])));
    let router = Arc::new(Router::new(settings.clone(), keys));
    let (source, arrivals) = Notifications::new(dir.path().join("consents.json"));
    let options = Options {
        model: Some(router.clone()),
        triage: Some(router as Arc<dyn Triager>),
        settings,
        sensors: Sensors {
            supplies: dir.path().join("power_supply"),
            thermal: dir.path().join("thermal"),
        },
        sources: vec![Box::new(source) as Box<dyn Source>],
        notifications: Some(arrivals),
        ..common::options(Vec::new())
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

fn notification(id: u32, app: &str, title: &str, body: &str) -> ToSecretary {
    ToSecretary::Notification(Notification {
        id,
        app: app.to_owned(),
        entry: (app == "Slack").then(|| "slack".to_owned()),
        title: title.to_owned(),
        body: body.to_owned(),
        at_ms: 1_794_215_520_000 + i64::from(id) * 1000,
    })
}

/// Her notifications' source's items read so far.
fn read(status: &Status) -> u64 {
    status
        .sources
        .iter()
        .find(|s| s.name == notifications::NAME)
        .map_or(0, |s| s.items)
}

fn triage(status: &Status) -> hocus_secretary_link::TriageStatus {
    status.triage.clone().expect("a triage queue")
}

fn every_request(fake: &Fake) -> String {
    fake.seen()
        .into_iter()
        .map(|s| format!("{} {}\n", s.path, s.body))
        .collect()
}

/// AC16 and the lot's acceptance: a consented application's notification
/// is an item like a mail (the first pass, then the triage; fail closed
/// before it), an unconsented one's never reaches anything; and nothing
/// but the model servers is ever asked.
#[test]
fn a_consented_application_s_notification_goes_through_redaction_and_the_triage() {
    let mut s = start(false);
    s.driver.send(&notification(
        1,
        "Slack",
        "Karim (#prod-alertes)",
        &format!("la prod est tombée, le token de la CI : {TOKEN}, tu regardes ?"),
    ));
    s.driver.send(&notification(
        2,
        "Thunderbird",
        "Nouveau courrier",
        "Votre code de connexion : facture-secrete-thunderbird",
    ));
    let status = s.driver.until("read", |st| read(st) == 1);
    assert_eq!(status.counts.episodes, 1, "Thunderbird's never kept");
    assert_eq!(triage(&status).pending, 1, "queued for the triage");
    assert_eq!(triage(&status).state, "paused: battery 20 %");

    // Fail closed: before the second pass, only its application and its
    // title leave.
    s.driver.answer_to("Qu'a dit Karim sur Slack ?");
    let first = s.anthropic.seen().pop().expect("a request").body;
    assert!(first.contains("Karim (#prod-alertes)"), "{first}");
    assert!(
        first.contains("peut contenir un secret, pas encore vérifié"),
        "{first}"
    );
    assert!(
        !first.contains("la prod est tombée"),
        "not its text: {first}"
    );

    // On mains, the local triage reads it, after the first pass.
    power(s.dir.path(), true);
    s.driver.advance(60);
    s.driver.until("checked", |st| triage(st).done == 1);
    let triaged = s.local.seen();
    let triaged: Vec<&Seen> = triaged
        .iter()
        .filter(|r| r.path == "/v1/chat/completions")
        .collect();
    assert_eq!(triaged.len(), 1);
    assert!(
        triaged[0].body.contains("Notification"),
        "{}",
        triaged[0].body
    );
    assert!(triaged[0].body.contains("la prod est tombée"));
    assert!(
        triaged[0]
            .body
            .contains("[secret masqué — voir la notification de Slack, 9 nov.]"),
        "{}",
        triaged[0].body
    );

    s.driver.answer_to("Qu'a dit Karim sur Slack ?");
    let after = s.anthropic.seen().pop().expect("a request").body;
    assert!(after.contains("la prod est tombée"), "checked: {after}");

    let everything = every_request(&s.anthropic) + &every_request(&s.local);
    assert!(!everything.contains(TOKEN), "the token never leaves");
    assert!(
        !everything.contains("thunderbird") && !everything.contains("Thunderbird"),
        "an application not consented to reaches nothing"
    );
    // No request to anything but the two model servers: none to Slack.
    for seen in s.anthropic.seen() {
        assert_eq!(seen.path, "/v1/messages");
    }
    for seen in s.local.seen() {
        assert!(
            ["/v1/chat/completions", "/api/generate"].contains(&seen.path.as_str()),
            "{}",
            seen.path
        );
    }
    s.driver.stop();
}

/// AC17 for notifications: each in its application's notebook; a revoked
/// application's items are forgotten within one cycle, the others kept.
#[test]
fn each_application_s_notifications_go_to_its_notebook_and_a_revoked_one_is_forgotten() {
    let mut s = start(false);
    s.driver.send(&notification(
        1,
        "Slack",
        "Karim (#prod-alertes)",
        "la prod est tombée",
    ));
    s.driver.send(&notification(
        2,
        "Signal",
        "Claire",
        "Dimanche chez maman ?",
    ));
    let status = s.driver.until("both read", |st| read(st) == 2);
    assert_eq!(status.counts.episodes, 2);
    assert_eq!(triage(&status).held, 2);

    let path = s.dir.path().join("consents.json");
    let mut book = ConsentBook::load(&path);
    book.revoke(
        &SourceKey::notifications("Slack", Some("slack")),
        common::START.parse().expect("a time"),
    );
    book.save(&path).expect("saved");
    let status = s.driver.advance(300);
    assert_eq!(status.counts.episodes, 1, "Slack's forgotten");
    assert_eq!(triage(&status).held, 1, "its held text too");
    s.driver
        .send(&notification(3, "Slack", "Karim", "toujours là ?"));
    s.driver
        .send(&notification(4, "Signal", "Claire", "Et lundi ?"));
    let status = s.driver.until("Signal's read", |st| read(st) == 3);
    assert_eq!(status.counts.episodes, 2, "Slack's no longer read");

    let store = s.driver.stop();
    let perso = store
        .search("Claire", Some(Notebook::Perso))
        .expect("a search");
    assert_eq!(perso.len(), 2, "Signal's in the perso notebook");
    let pro = store
        .search("Karim", Some(Notebook::Pro))
        .expect("a search");
    assert!(pro.is_empty(), "Slack's gone: {pro:?}");
    assert!(s.local.seen().is_empty() && s.anthropic.seen().is_empty());
}
