//! Her briefings and her speaking up (docs/design/secretary.md §6, lot
//! 5), her side, through the fake model (and its scripted triage) on a
//! clock of her own:
//!
//! - AC9: the first unlock after 07:00 makes exactly one request to speak,
//!   "Ton point est prêt", with the briefing as the day's thread item; a
//!   login before 07:00 none; a second unlock that day none.
//! - AC10: a taught leaves calendar and a 14:00 meeting its presenter is
//!   away from: EN CONGÉS lists her, a proposal offers a slot free for
//!   every invitee (computed in code), nothing is sent before "Proposer"
//!   then "Envoyer".
//! - AC11: of 20 mails, À RÉPONDRE lists exactly the two for the user,
//!   from people, asking something, unanswered in Sent.
//! - AC44: after a 14 h suspend, the gap's mails are caught up and
//!   triaged, one "Pendant ton absence" request comes after the unlock,
//!   and a reminder due during the gap is listed, never lost.
//! - A commitment in a mail sent becomes a card; "Mettre dans À faire"
//!   asks the shell for the task (the first effector) and says what came
//!   of it; its reminder comes due, "Plus tard" asks again, "Fait" ends
//!   it.

mod common;

use std::sync::Arc;

use hocus_secretary::{
    brain::Options,
    briefing,
    clock::Clock,
    graph::{NewRule, Origin},
    model::{Fake, Recorded, Recorder, Script},
    settings::{self, Settings, Triage},
    store::Store,
};
use hocus_secretary_link::{
    Body, Briefing, Decision, Effect, FromSecretary, Item, Later, Notebook, PeekAnswer, Speak,
    SpeakKind, Spoken, ToSecretary,
};
use jiff::{Timestamp, civil, tz::TimeZone};

/// A local time on Monday 9 November 2026 or after.
fn local(t: &str) -> Timestamp {
    t.parse::<civil::DateTime>()
        .expect("a local time")
        .to_zoned(TimeZone::system())
        .expect("a zoned time")
        .timestamp()
}

fn stamp(t: &str) -> String {
    local(t).to_string()
}

const MODEL: &str = r#"
[[reply]]
role = "briefing"
text = """Une journée chargée, mais un bon créneau ce matin.
Deux PR à relire sur Chapitô.
Le reste peut attendre."""

[default]
text = "D'accord."
"#;

fn options(script: &str, fixtures: &[String], recorder: &Arc<Recorder>) -> Options {
    let script: Script = toml::from_str(script).expect("a script");
    let fake = Arc::new(Fake::new(script, Some(recorder.clone())));
    let mut options = common::options(fixtures.iter().map(|f| common::fixture(f)).collect());
    options.model = Some(fake.clone());
    if fake.triages() {
        options.triage = Some(fake);
    }
    options.effects = Some(recorder.clone());
    // No battery, no heat known: the triage runs.
    options.sensors = hocus_secretary::power::Sensors {
        supplies: "/nonexistent".into(),
        thermal: "/nonexistent".into(),
        hwmon: "/nonexistent".into(),
        nvidia_smi: "/nonexistent".into(),
    };
    options.settings = Arc::new(settings::File::fixed(Settings {
        triage: Triage {
            pace: 0,
            ..Triage::default()
        },
        ..Settings::default()
    }));
    options
}

/// Her lines until her answer to a status request: all she said before.
fn drain(driver: &common::Driver, id: u64) -> Vec<FromSecretary> {
    driver.send(&ToSecretary::Status { id });
    let mut lines = Vec::new();
    loop {
        match driver.receive() {
            FromSecretary::Status { id: i, .. } if i == id => return lines,
            line => lines.push(line),
        }
    }
}

/// Her lines until `done` holds for one.
fn until(
    driver: &common::Driver,
    mut done: impl FnMut(&FromSecretary) -> bool,
) -> Vec<FromSecretary> {
    let mut lines = Vec::new();
    loop {
        let line = driver.receive();
        let stop = done(&line);
        lines.push(line);
        if stop {
            return lines;
        }
    }
}

/// The thread so far.
fn thread(driver: &common::Driver, id: u64) -> Vec<Item> {
    driver.send(&ToSecretary::Thread { id });
    loop {
        if let FromSecretary::Thread { id: i, items } = driver.receive()
            && i == id
        {
            return items;
        }
    }
}

/// The card labelled `label` in the thread, waiting for it.
fn card_labelled(driver: &mut common::Driver, label: &str) -> u64 {
    for n in 0..200 {
        let found = thread(driver, 900 + n)
            .into_iter()
            .find(|i| matches!(&i.body, Body::Card { card } if card.label == label));
        if let Some(item) = found {
            return item.id;
        }
        driver.settle();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("no card {label}");
}

fn speaks(lines: &[FromSecretary]) -> Vec<Speak> {
    lines
        .iter()
        .filter_map(|l| match l {
            FromSecretary::Speak { speak } => Some(speak.clone()),
            _ => None,
        })
        .collect()
}

fn briefings(lines: &[FromSecretary]) -> Vec<(Item, Briefing)> {
    lines
        .iter()
        .filter_map(|l| match l {
            FromSecretary::Item { item } => match &item.body {
                Body::Briefing { briefing } => Some((item.clone(), briefing.clone())),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn titles(briefing: &Briefing, key: &str) -> Vec<String> {
    briefing
        .sections
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.lines.iter().map(|l| l.title.clone()).collect())
        .unwrap_or_default()
}

fn is_speak(l: &FromSecretary) -> bool {
    matches!(l, FromSecretary::Speak { .. })
}

/// AC9.
#[test]
fn the_first_unlock_after_seven_makes_one_peek() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let calendar = format!(
        r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "daily"
at = "{}"
kind = "event"
digest = "Daily Chapitô"
event = {{ end = "{}", calendar = "Travail", attendees = ["Léa Martin"] }}
"#,
        stamp("2026-11-09T09:30"),
        stamp("2026-11-09T09:45"),
    );
    let store = Store::in_memory(Clock::virtual_at(local("2026-11-09T06:30")))?;
    let mut driver = common::Driver::start(store, options(MODEL, &[calendar], &recorder));
    driver.settle();
    // A login before 07:00: nothing.
    driver.send(&ToSecretary::Session { locked: false });
    let before = drain(&driver, 100);
    assert!(speaks(&before).is_empty(), "{before:?}");
    driver.send(&ToSecretary::Session { locked: true });
    driver.advance(2 * 3600);
    // The first unlock after 07:00: the briefing, then one request.
    driver.send(&ToSecretary::Session { locked: false });
    let lines = until(&driver, is_speak);
    let spoken = speaks(&lines);
    assert_eq!(spoken.len(), 1);
    assert_eq!(spoken[0].kind, SpeakKind::Briefing);
    assert!(
        spoken[0].text.contains("Ton point du matin est prêt"),
        "{spoken:?}"
    );
    let made = briefings(&lines);
    assert_eq!(made.len(), 1, "{lines:?}");
    let (item, briefing) = &made[0];
    assert_eq!(spoken[0].item, Some(item.id));
    assert!(briefing.title.starts_with("Ton point du matin"));
    assert_eq!(
        briefing.sentence,
        "Une journée chargée, mais un bon créneau ce matin."
    );
    assert_eq!(titles(briefing, "today"), ["Daily Chapitô"]);
    assert_eq!(
        titles(briefing, "rest"),
        ["Deux PR à relire sur Chapitô.", "Le reste peut attendre."]
    );
    // Her model was asked as the briefing's role, with the computed lines.
    let asked = recorder.lines().into_iter().any(|l| match l {
        Recorded::Call { request, .. } => {
            request.role == hocus_secretary::model::Role::Briefing
                && request.text().contains("[réunion]")
        }
        Recorded::Effect { .. } => false,
    });
    assert!(asked, "the briefing role was called with the day's facts");
    driver.send(&ToSecretary::Spoken {
        speak: spoken[0].id,
        outcome: Spoken::Shown,
    });
    // A second unlock that day: none.
    driver.send(&ToSecretary::Session { locked: true });
    driver.advance(3600);
    driver.send(&ToSecretary::Session { locked: false });
    let again = drain(&driver, 101);
    assert!(speaks(&again).is_empty(), "{again:?}");
    assert!(briefings(&again).is_empty());
    driver.stop();
    Ok(())
}

/// AC10.
#[test]
fn an_absent_presenter_gives_a_slot_free_for_everyone_and_nothing_leaves_unasked()
-> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let calendar = format!(
        r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "leave-sophie"
at = "{}"
kind = "event"
digest = "Congés Sophie Marchand"
event = {{ end = "{}", calendar = "Congés", busy = false }}
[[items]]
id = "review"
at = "{}"
kind = "event"
digest = "Revue de sprint Chapitô"
event = {{ end = "{}", calendar = "Travail", attendees = ["Sophie Marchand", "Karim Benali", "Léa Martin"], organiser = "Karim Benali", presenter = "Sophie Marchand" }}
[[items]]
id = "client"
at = "{}"
kind = "event"
digest = "Point client"
event = {{ end = "{}", calendar = "Travail", attendees = ["Léa Martin"] }}
"#,
        stamp("2026-11-09T00:00"),
        stamp("2026-11-14T00:00"),
        stamp("2026-11-09T14:00"),
        stamp("2026-11-09T15:00"),
        stamp("2026-11-10T14:00"),
        stamp("2026-11-10T15:00"),
    );
    let mut store = Store::in_memory(Clock::virtual_at(local("2026-11-09T08:30")))?;
    let (rule, _) = store.add_rule(&NewRule {
        node: None,
        sentence: "Les congés sont dans le calendrier « Congés »".to_owned(),
        scope: String::new(),
        origin: Origin::told("conversation"),
        notebook: Notebook::Pro,
    })?;
    briefing::compile_rule(&store, rule, r#"{"kind": "leave", "calendar": "Congés"}"#)?;
    let mut driver = common::Driver::start(store, options(MODEL, &[calendar], &recorder));
    driver.settle();
    driver.send(&ToSecretary::Session { locked: false });
    let lines = until(&driver, is_speak);
    let (_, briefing) = briefings(&lines).pop().expect("a briefing");
    assert_eq!(titles(&briefing, "leaves"), ["Sophie Marchand"]);
    let leaves = briefing
        .sections
        .iter()
        .find(|s| s.key == "leaves")
        .expect("EN CONGÉS");
    assert!(leaves.note.contains("Congés"));
    assert!(leaves.lines[0].detail.contains("ven. 13"), "{leaves:?}");
    let today = briefing
        .sections
        .iter()
        .find(|s| s.key == "today")
        .expect("AUJOURD'HUI");
    assert!(today.lines[0].attention, "the review needs the user");
    // The proposal: Tuesday 14 h is Léa's client, Wednesday a holiday;
    // Thursday 14 h is free for everyone.
    let card_id = leaves.proposal.expect("a proposal under EN CONGÉS");
    let card = lines
        .iter()
        .find_map(|l| match l {
            FromSecretary::Item { item } if item.id == card_id => match &item.body {
                Body::Card { card } => Some(card.clone()),
                _ => None,
            },
            _ => None,
        })
        .expect("the proposal's card");
    assert!(
        card.text.contains("jeudi 12 novembre 14 h"),
        "{}",
        card.text
    );
    assert!(card.text.contains("les 3 invités"), "{}", card.text);
    assert_eq!(card.actions[0].label, "Proposer à Karim Benali");
    let effects = |r: &Recorder| {
        r.lines()
            .into_iter()
            .filter(|l| matches!(l, Recorded::Effect { .. }))
            .count()
    };
    assert_eq!(effects(&recorder), 0);
    // "Proposer": a draft to send, still nothing sent.
    driver.send(&ToSecretary::Decide {
        id: 50,
        item: card_id,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    let after = until(&driver, |l| {
        matches!(
            l,
            FromSecretary::Answer {
                id: 50,
                done: true,
                ..
            }
        )
    });
    let draft = after
        .iter()
        .find_map(|l| match l {
            FromSecretary::Item { item } => match &item.body {
                Body::Card { card } if card.label == "Message · à Karim Benali" => {
                    Some((item.id, card.clone()))
                }
                _ => None,
            },
            _ => None,
        })
        .expect("a draft to Karim");
    assert!(draft.1.lines[0].contains("Sophie Marchand"));
    assert_eq!(
        effects(&recorder),
        0,
        "nothing sent before its confirmation"
    );
    // "Envoyer": the effect, once.
    driver.send(&ToSecretary::Decide {
        id: 51,
        item: draft.0,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    until(&driver, |l| {
        matches!(
            l,
            FromSecretary::Answer {
                id: 51,
                done: true,
                ..
            }
        )
    });
    assert_eq!(effects(&recorder), 1);
    driver.stop();
    Ok(())
}

/// Twenty mails: two for the user, from people, asking, unanswered.
fn twenty_mails() -> (String, String) {
    let mut items = String::new();
    let mut triage = String::new();
    let mut mail = |n: u32, sender: &str, subject: &str, extra: &str| {
        items.push_str(&format!(
            r#"
[[items]]
id = "m{n:02}"
at = "{}"
kind = "mail"
digest = "{subject}"
sender = "{sender}"
body = "Message [m{n:02}]."
mail = {{ to = ["Alex <alex@atelier-lune.fr>"], message_id = "<m{n:02}@mail>"{extra} }}
"#,
            stamp(&format!("2026-11-09T07:{:02}", n * 2)),
        ));
    };
    mail(
        1,
        "Cabinet Morel <morel@cabinet.fr>",
        "Notes de frais d’octobre",
        "",
    );
    mail(
        2,
        "Julie Berthier <julie@atelier-lune.fr>",
        "Accès GitLab et Planka",
        "",
    );
    // Answered in Sent (m20 replies to it).
    mail(3, "Paul Durand <paul@client.fr>", "Le devis", "");
    // Not for the user (a list), though it asks.
    mail(
        4,
        "Équipe <equipe@atelier-lune.fr>",
        "Qui vient vendredi ?",
        "",
    );
    // A robot asking.
    mail(5, "GitLab <noreply@gitlab.com>", "Merci de confirmer", "");
    for n in 6..20 {
        mail(
            n,
            "Scaleway <news@scaleway.com>",
            &format!("Lettre {n}"),
            "",
        );
    }
    items.push_str(&format!(
        r#"
[[items]]
id = "m20"
at = "{}"
kind = "mail"
digest = "Re: Le devis"
sender = "Alex <alex@atelier-lune.fr>"
body = "Voici le devis. [m20]"
mail = {{ to = ["Paul Durand <paul@client.fr>"], sent = true, in_reply_to = "<m03@mail>" }}
"#,
        stamp("2026-11-09T08:00"),
    ));
    for (n, for_me, needs, urgency) in [
        (1, true, true, "high"),
        (2, true, true, "normal"),
        (3, true, true, "normal"),
        (4, false, true, "low"),
        (5, true, true, "normal"),
    ] {
        triage.push_str(&format!(
            "[[triage]]\ncontains = [\"[m{n:02}]\"]\n\
             verdict = {{ for_me = {for_me}, needs_reply = {needs}, urgency = \"{urgency}\", digest = \"m{n:02}\" }}\n"
        ));
    }
    (items, triage)
}

/// AC11.
#[test]
fn of_twenty_mails_two_really_wait_for_an_answer() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let (items, triage) = twenty_mails();
    let mail = format!("name = \"fixture-mail\"\nnotebook = \"pro\"\n{items}");
    let script = format!("{MODEL}\n{triage}");
    let store = Store::in_memory(Clock::virtual_at(local("2026-11-09T08:30")))?;
    let mut driver = common::Driver::start(store, options(&script, &[mail], &recorder));
    driver.settle();
    driver.until("every mail triaged", |s| {
        s.triage
            .as_ref()
            .is_some_and(|t| t.pending == 0 && t.done == 20)
    });
    driver.send(&ToSecretary::Session { locked: false });
    let lines = until(&driver, is_speak);
    let (_, briefing) = briefings(&lines).pop().expect("a briefing");
    assert_eq!(
        titles(&briefing, "answer"),
        [
            "Cabinet Morel · Notes de frais d’octobre",
            "Julie Berthier · Accès GitLab et Planka"
        ]
    );
    // The others, summed up.
    assert!(!titles(&briefing, "rest").is_empty());
    driver.stop();
    Ok(())
}

/// A commitment in a mail sent: a card; the task asked of the shell; its
/// reminder said when due, again an hour later, then done.
#[test]
fn a_promise_goes_to_to_do_and_comes_back_when_due() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let mail = format!(
        r#"
name = "fixture-mail"
notebook = "pro"
[[items]]
id = "s1"
at = "{}"
kind = "mail"
digest = "Le devis"
sender = "Alex <alex@atelier-lune.fr>"
body = "Je t’envoie le devis avant 14 h. [s1]"
mail = {{ to = ["Léa Martin <lea@atelier-lune.fr>"], sent = true }}
"#,
        stamp("2026-11-09T08:10"),
    );
    let script = format!(
        "{MODEL}\n[[triage]]\ncontains = [\"[s1]\"]\n\
         verdict = {{ for_me = false, digest = \"Le devis à Léa\", commitments = [\"envoyer le devis avant 14 h\"] }}\n"
    );
    let store = Store::in_memory(Clock::virtual_at(local("2026-11-09T08:30")))?;
    let mut driver = common::Driver::start(store, options(&script, &[mail], &recorder));
    driver.settle();
    let card = card_labelled(&mut driver, "Ta promesse");
    let text = thread(&driver, 98)
        .into_iter()
        .find_map(|i| match i.body {
            Body::Card { card: c } if i.id == card => Some(c.text),
            _ => None,
        })
        .expect("the promise's card");
    assert!(
        text.contains("Léa Martin") && text.contains("12 h"),
        "{text}"
    );
    driver.send(&ToSecretary::Decide {
        id: 60,
        item: card,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    let asked = until(&driver, |l| matches!(l, FromSecretary::Effect { .. }));
    let Some(FromSecretary::Effect {
        effect: Effect::Task {
            id,
            title,
            notebook,
        },
    }) = asked.last()
    else {
        panic!("{asked:?}");
    };
    assert_eq!(*id, card);
    assert_eq!(title, "envoyer le devis avant 14 h (Léa Martin)");
    assert_eq!(*notebook, Notebook::Pro);
    driver.send(&ToSecretary::Effected {
        effect: *id,
        ok: true,
        detail: String::new(),
    });
    let done = until(
        &driver,
        |l| matches!(l, FromSecretary::Item { item } if matches!(&item.body, Body::Card { card } if card.outcome.as_deref() == Some("Mis dans À faire."))),
    );
    assert!(!done.is_empty());
    // Due at 12 h (two hours before), until 14 h.
    driver.send(&ToSecretary::Clock {
        advance: 3 * 3600 + 1800,
    });
    let lines = until(&driver, is_speak);
    let speak = speaks(&lines).pop().expect("her reminder");
    assert_eq!(speak.kind, SpeakKind::Reminder);
    assert!(speak.text.contains("envoyer le devis"), "{speak:?}");
    assert_eq!(speak.deadline, Some(stamp("2026-11-09T14:00")));
    driver.send(&ToSecretary::Spoken {
        speak: speak.id,
        outcome: Spoken::Shown,
    });
    // "Plus tard › Dans 1 heure": asked again then, not before.
    driver.send(&ToSecretary::Peeked {
        id: 61,
        speak: speak.id,
        answer: PeekAnswer::Later { later: Later::Hour },
    });
    until(&driver, |l| {
        matches!(
            l,
            FromSecretary::Answer {
                id: 61,
                done: true,
                ..
            }
        )
    });
    driver.send(&ToSecretary::Clock { advance: 1800 });
    assert!(speaks(&drain(&driver, 102)).is_empty());
    driver.send(&ToSecretary::Clock { advance: 1800 });
    let again = speaks(&until(&driver, is_speak));
    assert_eq!(again.len(), 1);
    driver.send(&ToSecretary::Peeked {
        id: 62,
        speak: again[0].id,
        answer: PeekAnswer::Done,
    });
    until(&driver, |l| {
        matches!(
            l,
            FromSecretary::Answer {
                id: 62,
                done: true,
                ..
            }
        )
    });
    driver.send(&ToSecretary::Clock { advance: 4 * 3600 });
    assert!(speaks(&drain(&driver, 103)).is_empty(), "done: never again");
    driver.stop();
    Ok(())
}

/// AC44.
#[test]
fn after_fourteen_hours_away_one_peek_tells_what_came() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let mut items = String::new();
    for (n, at) in [
        (1, "2026-11-09T22:00"),
        (2, "2026-11-10T03:00"),
        (3, "2026-11-10T06:30"),
    ] {
        items.push_str(&format!(
            r#"
[[items]]
id = "g{n}"
at = "{}"
kind = "mail"
digest = "Pendant la nuit {n}"
sender = "Léa Martin <lea@atelier-lune.fr>"
body = "Nuit [g{n}]"
mail = {{ to = ["Alex <alex@atelier-lune.fr>"], message_id = "<g{n}@mail>" }}
"#,
            stamp(at)
        ));
    }
    items.push_str(&format!(
        r#"
[[items]]
id = "s1"
at = "{}"
kind = "mail"
digest = "Le devis"
sender = "Alex <alex@atelier-lune.fr>"
body = "Je t’envoie le devis demain avant 7 h. [s1]"
mail = {{ to = ["Paul Durand <paul@client.fr>"], sent = true }}
"#,
        stamp("2026-11-09T17:00")
    ));
    let mail = format!("name = \"fixture-mail\"\nnotebook = \"pro\"\nevery = 300\n{items}");
    let script = format!(
        "{MODEL}\n[[triage]]\ncontains = [\"[s1]\"]\n\
         verdict = {{ digest = \"Le devis\", commitments = [\"envoyer le devis demain avant 7 h\"] }}\n\
         [[triage]]\ncontains = [\"[g2]\"]\n\
         verdict = {{ for_me = true, needs_reply = true, urgency = \"high\", digest = \"Léa attend la maquette\" }}\n"
    );
    let store = Store::in_memory(Clock::virtual_at(local("2026-11-09T18:00")))?;
    let mut driver = common::Driver::start(store, options(&script, &[mail], &recorder));
    driver.settle();
    // The evening's session: the day's briefing, and the promise kept.
    driver.send(&ToSecretary::Session { locked: false });
    let card = card_labelled(&mut driver, "Ta promesse");
    driver.send(&ToSecretary::Decide {
        id: 70,
        item: card,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    until(&driver, |l| matches!(l, FromSecretary::Effect { .. }));
    driver.send(&ToSecretary::Effected {
        effect: card,
        ok: true,
        detail: String::new(),
    });
    for speak in speaks(&drain(&driver, 110)) {
        driver.send(&ToSecretary::Spoken {
            speak: speak.id,
            outcome: Spoken::Shown,
        });
    }
    // Locked, then 14 h asleep.
    driver.send(&ToSecretary::Session { locked: true });
    driver.send(&ToSecretary::Resumed {
        slept: Some(14 * 3600),
    });
    let status = driver.until("caught up, the gap triaged", |s| {
        s.catch_up.as_ref().is_some_and(|c| c.away_briefing)
            && s.triage
                .as_ref()
                .is_some_and(|t| t.pending == 0 && t.done == 4)
    });
    let caught = status.catch_up.expect("a catch-up");
    assert_eq!(caught.gap_seconds, 14 * 3600);
    assert_eq!(caught.items, 3, "the gap's mails, from the source's cursor");
    // Nothing while locked; one request after the unlock.
    let locked = drain(&driver, 111);
    assert!(
        speaks(&locked).iter().all(|s| s.kind != SpeakKind::Absence),
        "{locked:?}"
    );
    driver.send(&ToSecretary::Session { locked: false });
    let lines = until(
        &driver,
        |l| matches!(l, FromSecretary::Speak { speak } if speak.kind == SpeakKind::Absence),
    );
    let absence: Vec<Speak> = speaks(&lines)
        .into_iter()
        .filter(|s| s.kind == SpeakKind::Absence)
        .collect();
    assert_eq!(absence.len(), 1);
    assert!(absence[0].text.contains("Pendant ton absence"));
    let (_, briefing) = briefings(&lines).pop().expect("the briefing");
    let away = titles(&briefing, "away");
    assert!(away[0].starts_with('3'), "{away:?}");
    assert!(
        away.iter().any(|t| t.contains("Pendant la nuit 2")),
        "what matters: {away:?}"
    );
    // The promise's reminder came due (5 h) and went (7 h) while asleep:
    // listed, not lost.
    assert_eq!(
        titles(&briefing, "reminders"),
        ["envoyer le devis demain avant 7 h"]
    );
    let more = drain(&driver, 112);
    assert!(
        speaks(&more).iter().all(|s| s.kind != SpeakKind::Absence),
        "one only"
    );
    driver.stop();
    Ok(())
}
