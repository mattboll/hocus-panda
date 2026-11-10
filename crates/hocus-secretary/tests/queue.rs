//! Her triage queue shown and senders set aside (#754), her loop driven
//! directly on a clock of her own, the local triage scripted (39 s an
//! item, as the maintainer's laptop measured it).
//!
//! - The view: the item triaged now (sender, subject, source), the rest
//!   by sender with their counts (the most first), the pace measured and
//!   the time left, sent again while open; never more of an item than its
//!   sender, subject and date.
//! - Her proposal: a sender whose mails say they come from a machine, its
//!   count; "Ne pas trier, chercher si besoin" from the view answers it: a
//!   rule in "Ce qu'elle sait", its items out of the queue, its later
//!   ones too; one the user asks about is triaged first.
//! - The rule ended in "Ce qu'elle sait": its items come back; set aside
//!   again, then "Trier quand même": back again. A restart keeps them
//!   aside.
//! - The pause and its reason (heat, the model unreachable).

mod common;

use std::sync::Arc;

use hocus_secretary::{
    model::{Fake, Script},
    store::Store,
    triage::{Triager, Verdict},
};
use hocus_secretary_link::{
    Body, FromSecretary, Item, ToSecretary,
    knows::{Act as KnownAct, Do, Knowledge, Query},
    queue::{Act, Sender, View},
};

const MAIL: &str = r#"
name = "fixture-mail"
notebook = "pro"
every = 3600
[[items]]
id = "lea"
at = "2026-11-09T06:00:00Z"
kind = "mail"
digest = "Le devis de Chapitô"
sender = "Léa Martin <lea@atelier-lune.fr>"
body = "Tu peux relire le devis avant jeudi ?"
[[items]]
id = "gh1"
at = "2026-11-09T06:10:00Z"
kind = "mail"
digest = "[hocus] le tiroir du tri (PR #42)"
sender = "GitHub <notifications@github.com>"
body = "mattboll commented: the queue view looks right."
mail = { headers = { "X-GitHub-Reason" = "subscribed", "List-Unsubscribe" = "<https://github.com/notifications/unsubscribe/x>" } }
[[items]]
id = "gh2"
at = "2026-11-09T06:20:00Z"
kind = "mail"
digest = "[hocus] CI passed on sprint/11"
sender = "GitHub <notifications@github.com>"
body = "All checks have passed."
mail = { headers = { "X-GitHub-Reason" = "ci_activity" } }
[[items]]
id = "gh3"
at = "2026-11-09T06:30:00Z"
kind = "mail"
digest = "[hocus] Re: birthdays over CardDAV (#754)"
sender = "GitHub <notifications@github.com>"
body = "A comment."
mail = { headers = { "X-GitHub-Reason" = "mention" } }
[[items]]
id = "lea2"
at = "2026-11-09T06:40:00Z"
kind = "mail"
digest = "Re: le devis"
sender = "Léa Martin <lea@atelier-lune.fr>"
body = "Merci !"
[[items]]
id = "news"
at = "2026-11-09T06:50:00Z"
kind = "mail"
digest = "La lettre du mardi"
sender = "La Lettre <lettre@exemple.fr>"
body = "Au sommaire cette semaine."
mail = { headers = { "List-Unsubscribe" = "<mailto:unsub@exemple.fr>" } }
"#;

const SCRIPT: &str = r#"
[default]
text = "Je regarde."
[[triage]]
delay = 39
verdict = { for_me = false, needs_reply = false, urgency = "low", digest = "trié" }
"#;

/// The machine on mains, at `heat` °C.
fn sensors(dir: &std::path::Path, heat: u32) -> hocus_secretary::power::Sensors {
    let supplies = dir.join("power_supply");
    std::fs::create_dir_all(supplies.join("AC")).expect("a folder");
    std::fs::write(supplies.join("AC/type"), "Mains").expect("written");
    std::fs::write(supplies.join("AC/online"), "1").expect("written");
    let zone = dir.join("thermal/thermal_zone0");
    std::fs::create_dir_all(&zone).expect("a folder");
    std::fs::write(zone.join("type"), "x86_pkg_temp").expect("written");
    std::fs::write(zone.join("temp"), format!("{}", heat * 1000)).expect("written");
    hocus_secretary::power::Sensors {
        supplies,
        thermal: dir.join("thermal"),
        hwmon: dir.join("hwmon"),
        nvidia_smi: dir.join("no-nvidia-smi"),
    }
}

fn start(dir: &std::path::Path, file: &std::path::Path, heat: u32) -> common::Driver {
    start_with_fake(dir, file, heat).0
}

/// Her loop, and the scripted model it triages with.
fn start_with_fake(
    dir: &std::path::Path,
    file: &std::path::Path,
    heat: u32,
) -> (common::Driver, Arc<Fake>) {
    let clock = common::clock();
    let script: Script = toml::from_str(SCRIPT).expect("a script");
    let fake = Arc::new(Fake::new(script, None).with_clock(clock.clone()));
    let mut options = common::options(vec![common::fixture(MAIL)]);
    options.model = Some(fake.clone());
    options.triage = Some(fake.clone());
    options.sensors = sensors(dir, heat);
    let store = Store::open(file, &[3u8; 32], clock).expect("her memory");
    (common::Driver::start(store, options), fake)
}

/// Waits until the triage under way, if any, counts its delay on her
/// clock: her clock moved before its thread read it, the delay would start
/// after the move and the item never be done (#760).
fn on_her_clock(driver: &mut common::Driver, fake: &Fake) {
    let start = std::time::Instant::now();
    loop {
        let state = driver.status().triage.map(|t| t.state).unwrap_or_default();
        if state != "running" || fake.timing() > 0 {
            return;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "the triage under way never read her clock"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Her next view of the queue that `done` says.
fn view_until(driver: &common::Driver, what: &str, done: impl Fn(&View) -> bool) -> View {
    let start = std::time::Instant::now();
    loop {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "never {what}"
        );
        if let FromSecretary::Queue { view, .. } = driver.receive()
            && done(&view)
        {
            return *view;
        }
    }
}

fn question(driver: &mut common::Driver, words: &str) -> Item {
    let thread = match driver.request(|id| ToSecretary::Thread { id }) {
        FromSecretary::Thread { items, .. } => items,
        other => panic!("not her thread: {other:?}"),
    };
    thread
        .into_iter()
        .find(|i| matches!(&i.body, Body::Question { text, .. } if text.contains(words)))
        .unwrap_or_else(|| panic!("never asked {words}"))
}

fn knows(driver: &mut common::Driver) -> Knowledge {
    match driver.request(|id| ToSecretary::Knows {
        id,
        query: Some(Query::default()),
    }) {
        FromSecretary::Knows { view, .. } => *view,
        other => panic!("not what she knows: {other:?}"),
    }
}

/// The view now (asked again: the open view's id is the new one).
fn view_now(driver: &mut common::Driver) -> View {
    match driver.request(|id| ToSecretary::Queue { id, open: true }) {
        FromSecretary::Queue { view, .. } => *view,
        other => panic!("not the queue: {other:?}"),
    }
}

fn group(view: &View, key: &str) -> u64 {
    view.groups
        .iter()
        .find(|g| g.key == key)
        .map_or(0, |g| g.count)
}

fn aside_since(view: &View, key: &str) -> String {
    view.aside
        .iter()
        .find(|g| g.key == key)
        .map(|g| g.since.clone())
        .unwrap_or_default()
}

fn aside(view: &View, key: &str) -> u64 {
    view.aside
        .iter()
        .find(|g| g.key == key)
        .map_or(0, |g| g.count)
}

#[test]
fn the_queue_shows_its_senders_its_pace_and_a_sender_set_aside_waits_apart() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("memory.db");
    let (mut driver, fake) = start_with_fake(dir.path(), &file, 60);
    driver.settle();

    // Her proposal: GitHub's three mails, sent by a machine.
    let asked = question(&mut driver, "GitHub t’a envoyé");
    let Body::Question {
        text,
        note,
        replies,
        ..
    } = &asked.body
    else {
        unreachable!()
    };
    // Said plainly (#758): her name for the sender, no header's.
    assert!(
        text.starts_with("GitHub t’a envoyé 3 notifications automatiques. Je les laisse sans tri"),
        "{text}"
    );
    assert!(
        note.as_deref()
            .is_some_and(|n| !n.contains("X-GitHub-Reason")),
        "{note:?}"
    );
    assert_eq!(replies[0], "Oui, ne pas les trier");

    // The view, as the drawer asks for it.
    driver.send(&ToSecretary::Queue { id: 90, open: true });
    let view = view_until(&driver, "the view", |v| v.now.is_some());
    let now = view.now.clone().expect("the item triaged now");
    assert_eq!(now.sender, "Léa Martin <lea@atelier-lune.fr>");
    assert_eq!(now.subject, "Le devis de Chapitô");
    assert_eq!(now.source, "Gmail");
    assert_eq!(view.state, "Je trie");
    // The mail read now counts with those waiting (#758): the same number
    // in her header, her question and the view.
    assert_eq!(view.waiting, 6);
    assert_eq!(view.groups[0].key, "notifications@github.com");
    assert_eq!(view.groups[0].count, 3);
    assert_eq!(view.groups[0].automated.as_deref(), Some("X-GitHub-Reason"));
    assert!(view.groups[0].can_aside);
    // Grouped by what sends them (#758): a machine, a newsletter, a
    // person, who is never offered to be left unsorted.
    assert_eq!(view.groups[0].name, "GitHub");
    assert_eq!(view.groups[0].sender, Sender::Machine);
    assert!(!view.groups[0].note.contains("X-GitHub-Reason"));
    let lea = view
        .groups
        .iter()
        .find(|g| g.key == "lea@atelier-lune.fr")
        .expect("Léa");
    assert_eq!(
        (lea.name.as_str(), lea.sender),
        ("Léa Martin", Sender::Person)
    );
    assert!(!lea.can_aside);
    let letter = view
        .groups
        .iter()
        .find(|g| g.key == "lettre@exemple.fr")
        .expect("the newsletter");
    assert_eq!(letter.sender, Sender::Letter);
    assert!(letter.can_aside);
    assert!(view.rate.is_empty(), "nothing measured yet");
    // Never more of an item than its sender, subject and date.
    let shown = serde_json::to_string(&view).expect("json");
    for body in ["relire le devis", "All checks", "Au sommaire", "commented"] {
        assert!(!shown.contains(body), "{body}: {shown}");
    }

    // 39 s: the first one triaged, the pace measured (the rest between
    // two included), the time left from it.
    on_her_clock(&mut driver, &fake);
    driver.send(&ToSecretary::Clock { advance: 39 });
    let view = view_until(&driver, "measured", |v| v.done == 1);
    assert_eq!(view.per_item, Some(41));
    assert_eq!(view.rate, "≈ 41 s par élément");
    assert_eq!(view.left_seconds, Some(41 * view.waiting));
    assert!(view.left.starts_with("encore ≈ "), "{}", view.left);
    assert!(
        view.pace.starts_with("Encore ")
            && view.pace.ends_with("(41 s par mail sur cet ordinateur)."),
        "{}",
        view.pace
    );

    // "Ne pas trier, chercher si besoin" on GitHub, from the view: her
    // question answered, a rule, its items apart.
    let view = match driver.request(|id| ToSecretary::QueueAct {
        id,
        act: Act::Aside {
            key: "notifications@github.com".to_owned(),
        },
    }) {
        FromSecretary::Queue { view, .. } => *view,
        other => panic!("not the queue: {other:?}"),
    };
    assert_eq!(group(&view, "notifications@github.com"), 0, "{view:?}");
    assert_eq!(aside(&view, "notifications@github.com"), 3);
    assert_eq!(view.set_aside, 3);
    assert!(!aside_since(&view, "notifications@github.com").is_empty());
    let thread = match driver.request(|id| ToSecretary::Thread { id }) {
        FromSecretary::Thread { items, .. } => items,
        other => panic!("not her thread: {other:?}"),
    };
    let answered = thread
        .iter()
        .find(|i| i.id == asked.id)
        .expect("her question");
    assert!(
        matches!(
            answered.body,
            Body::Question {
                chosen: Some(0),
                ..
            }
        ),
        "answered Oui: {answered:?}"
    );
    let known = knows(&mut driver);
    let rule = known
        .items()
        .find(|k| k.text.contains("notifications@github.com") && k.text.contains("pas triés"))
        .unwrap_or_else(|| panic!("the rule: {known:?}"))
        .clone();
    assert!(rule.item.starts_with("rule:"), "{rule:?}");

    // Asked about one: it goes first, set aside or not.
    driver.answer_to("Où en est le tiroir du tri ?");
    let view = view_now(&mut driver);
    assert_eq!(view.set_aside, 2, "{view:?}");
    assert_eq!(group(&view, "notifications@github.com"), 1);
    on_her_clock(&mut driver, &fake);
    driver.send(&ToSecretary::Clock { advance: 41 });
    let view = view_until(&driver, "the next one", |v| {
        v.now.as_ref().is_some_and(|n| n.sender.contains("github"))
    });
    assert_eq!(
        view.now.as_ref().map(|n| n.subject.as_str()),
        Some("[hocus] le tiroir du tri (PR #42)")
    );

    // The rule ended in "Ce qu'elle sait": its items come back.
    driver.send(&ToSecretary::Memory {
        id: 91,
        act: Do::Item {
            item: rule.item.clone(),
            act: KnownAct::Close,
        },
    });
    let view = view_until(&driver, "brought back", |v| v.set_aside == 0);
    // The one read now included.
    assert_eq!(group(&view, "notifications@github.com"), 3);

    // Set aside again, and "Trier quand même".
    let view = match driver.request(|id| ToSecretary::QueueAct {
        id,
        act: Act::Aside {
            key: "notifications@github.com".to_owned(),
        },
    }) {
        FromSecretary::Queue { view, .. } => *view,
        other => panic!("not the queue: {other:?}"),
    };
    assert_eq!(view.set_aside, 2);
    let view = match driver.request(|id| ToSecretary::QueueAct {
        id,
        act: Act::Back {
            key: "notifications@github.com".to_owned(),
        },
    }) {
        FromSecretary::Queue { view, .. } => *view,
        other => panic!("not the queue: {other:?}"),
    };
    assert_eq!(view.set_aside, 0);
    assert_eq!(group(&view, "notifications@github.com"), 3);

    // Closed: nothing more is sent, whatever the queue does.
    driver.send(&ToSecretary::Queue {
        id: 92,
        open: false,
    });
    // What was sent before it closed, read.
    let _ = driver.status();
    on_her_clock(&mut driver, &fake);
    driver.send(&ToSecretary::Clock { advance: 41 });
    // Her status asked until the item under way is done: a view its end
    // sent would come before the status that says it.
    let start = std::time::Instant::now();
    for asked in 999.. {
        driver.send(&ToSecretary::Status { id: asked });
        let done = loop {
            match driver.receive() {
                FromSecretary::Queue { id, view } => {
                    panic!("a view sent once closed: {id} {view:?}")
                }
                FromSecretary::Status { id, status } if id == asked => {
                    break status.triage.map_or(0, |t| t.done);
                }
                _ => {}
            }
        };
        if done >= 2 {
            break;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "never done: {done}"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    driver.stop();
}

#[test]
fn a_sender_set_aside_stays_aside_after_a_restart_and_later_mails_too() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("memory.db");
    let mut driver = start(dir.path(), &file, 60);
    driver.settle();
    let view = match driver.request(|id| ToSecretary::QueueAct {
        id,
        act: Act::Aside {
            key: "lettre@exemple.fr".to_owned(),
        },
    }) {
        FromSecretary::Queue { view, .. } => *view,
        other => panic!("not the queue: {other:?}"),
    };
    assert_eq!(aside(&view, "lettre@exemple.fr"), 1);
    driver.stop();

    let mut driver = start(dir.path(), &file, 60);
    driver.settle();
    let status = driver.status();
    let triage = status.triage.expect("her triage");
    assert_eq!(triage.set_aside, 1, "{triage:?}");
    driver.send(&ToSecretary::Queue { id: 7, open: true });
    let view = view_until(&driver, "the view", |_| true);
    assert_eq!(aside(&view, "lettre@exemple.fr"), 1);
    assert_eq!(group(&view, "lettre@exemple.fr"), 0);
    driver.stop();
}

/// The scripted local model, held whole on the graphics card.
#[derive(Debug)]
struct OnCard(Arc<Fake>);

impl Triager for OnCard {
    fn endpoint(&self) -> (String, String) {
        self.0.endpoint()
    }

    fn triage(&self, input: &str) -> Result<(Verdict, u64, u64), String> {
        self.0.triage(input)
    }

    fn unload(&self) -> Result<(), String> {
        self.0.unload()
    }

    fn placement(&self) -> Option<hocus_secretary::providers::openai::Placement> {
        Some(hocus_secretary::providers::openai::Placement::Gpu)
    }
}

/// The model whole on the graphics card (#756): the card's temperature is
/// watched instead of the processor's, through `nvidia-smi` (a fake
/// script: no GPU is touched), at most every ten seconds; over its
/// slowdown point on average, the triage pauses and says the card's.
#[test]
fn on_the_graphics_card_the_cards_heat_pauses_it() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("a folder");
    let clock = common::clock();
    let script: Script = toml::from_str(SCRIPT).expect("a script");
    let fake = Arc::new(Fake::new(script, None).with_clock(clock.clone()));
    let mut options = common::options(vec![common::fixture(MAIL)]);
    options.model = Some(fake.clone());
    options.triage = Some(Arc::new(OnCard(fake.clone())));
    // The processor cool; the card at 90 °C, its slowdown point 3 °C below.
    options.sensors = sensors(dir.path(), 60);
    let asked = dir.path().join("nvidia-smi.log");
    let smi = dir.path().join("nvidia-smi");
    std::fs::write(
        &smi,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\necho '90, -3'\n",
            asked.display()
        ),
    )
    .expect("written");
    std::fs::set_permissions(&smi, std::fs::Permissions::from_mode(0o755)).expect("executable");
    options.sensors.nvidia_smi = smi;
    let store = Store::open(&dir.path().join("memory.db"), &[5u8; 32], clock).expect("her memory");
    let mut driver = common::Driver::start(store, options);
    driver.settle();
    let mut waited: usize = 0;
    let state = loop {
        let state = driver.status().triage.map(|t| t.state).unwrap_or_default();
        if state.starts_with("paused") {
            break state;
        }
        assert!(waited < 600, "never paused: {state}");
        on_her_clock(&mut driver, &fake);
        driver.advance(5);
        waited += 5;
        // `nvidia-smi` runs on the wall clock: given its time.
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(
        state,
        "paused: graphics card 90 °C on average, resumes below 77 °C"
    );
    let asked = std::fs::read_to_string(&asked).expect("nvidia-smi asked");
    assert!(
        asked.lines().all(|l| l
            == "--query-gpu=temperature.gpu,temperature.gpu.tlimit --format=csv,noheader,nounits"),
        "{asked}"
    );
    assert!(
        asked.lines().count() <= waited / 10 + 1,
        "{} times in {waited} s",
        asked.lines().count()
    );
    driver.send(&ToSecretary::Queue { id: 8, open: true });
    let view = view_until(&driver, "paused", |v| v.pause.is_some());
    assert_eq!(
        view.pause.as_deref(),
        Some(
            "La carte graphique chauffe (90 °C). Je reprends quand elle redescend sous 77 °C\u{a0}; rien n’est perdu."
        )
    );
    driver.stop();
}

/// A local model that does not answer (Ollama not running).
#[derive(Debug)]
struct Down;

impl Triager for Down {
    fn endpoint(&self) -> (String, String) {
        ("local".to_owned(), "qwen3:8b".to_owned())
    }

    fn triage(&self, _input: &str) -> Result<(Verdict, u64, u64), String> {
        Err("http://127.0.0.1:11434: connection refused".to_owned())
    }

    fn unload(&self) -> Result<(), String> {
        Ok(())
    }
}

/// Her clock moved on five seconds at a time until her triage's state
/// says `done`: that state, and how long it took.
fn advance_until(
    driver: &mut common::Driver,
    fake: &Fake,
    what: &str,
    done: impl Fn(&str) -> bool,
) -> (String, u64) {
    let mut waited = 0;
    loop {
        let state = driver.status().triage.map(|t| t.state).unwrap_or_default();
        if done(&state) {
            return (state, waited);
        }
        assert!(waited < 600, "never {what}: {state}");
        on_her_clock(driver, fake);
        driver.advance(5);
        waited += 5;
    }
}

/// The package's temperature, in the folder [`sensors`] made.
fn package_at(dir: &std::path::Path, heat: u32) {
    std::fs::write(
        dir.join("thermal/thermal_zone0/temp"),
        format!("{}", heat * 1000),
    )
    .expect("written");
}

#[test]
fn the_laptops_swinging_package_never_pauses_it() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("memory.db");
    let (mut driver, fake) = start_with_fake(dir.path(), &file, 64);
    driver.settle();
    // The package of #756, its samples five seconds apart, for as long as
    // the six items take (39 s each and a rest).
    let swing = [101, 100, 100, 89, 72, 65, 64, 80, 64, 100];
    for heat in swing.iter().cycle().take(80) {
        package_at(dir.path(), *heat);
        on_her_clock(&mut driver, &fake);
        let state = driver
            .advance(5)
            .triage
            .map(|t| t.state)
            .unwrap_or_default();
        assert!(!state.starts_with("paused"), "{heat} °C: {state}");
    }
    assert_eq!(driver.status().triage.map(|t| t.done), Some(6));
    driver.stop();
}

#[test]
fn the_pause_says_why_heat_or_a_model_that_does_not_answer() {
    let dir = tempfile::tempdir().expect("a folder");
    let file = dir.path().join("memory.db");
    let (mut driver, fake) = start_with_fake(dir.path(), &file, 100);
    driver.settle();
    // A steady 100 °C: paused once the minute's average says so, after the
    // item under way.
    let (state, _) = advance_until(&mut driver, &fake, "paused", |s| s.starts_with("paused"));
    assert_eq!(
        state,
        "paused: processor 100 °C on average, resumes below 90 °C"
    );
    driver.send(&ToSecretary::Queue { id: 5, open: true });
    let view = view_until(&driver, "paused", |v| v.pause.is_some());
    assert_eq!(view.state, "En pause");
    assert_eq!(
        view.pause.as_deref(),
        Some(
            "L’ordinateur chauffe (100 °C). Je reprends quand il redescend sous 90 °C\u{a0}; rien n’est perdu."
        )
    );
    assert!(view.now.is_none());
    assert!((1..6).contains(&view.waiting), "{}", view.waiting);
    // 85 °C: it goes on once the average is below 90, not on the first
    // cool sample.
    package_at(dir.path(), 85);
    let (_, waited) = advance_until(&mut driver, &fake, "going on", |s| !s.starts_with("paused"));
    assert!(waited >= 40, "after {waited} s");
    driver.stop();

    let other = tempfile::tempdir().expect("a folder");
    let mut options = common::options(vec![common::fixture(MAIL)]);
    options.triage = Some(Arc::new(Down));
    options.sensors = sensors(other.path(), 60);
    let store = Store::open(&other.path().join("memory.db"), &[4u8; 32], common::clock())
        .expect("her memory");
    let mut driver = common::Driver::start(store, options);
    driver.settle();
    driver.send(&ToSecretary::Queue { id: 6, open: true });
    let view = view_until(&driver, "unavailable", |v| v.pause.is_some());
    assert!(
        view.pause
            .as_deref()
            .is_some_and(|p| p.starts_with("Le modèle local ne répond pas")),
        "{view:?}"
    );
    assert_eq!(view.model, "local/qwen3:8b");
    driver.stop();
}
