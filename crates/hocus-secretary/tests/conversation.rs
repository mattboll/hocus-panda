//! The drawer's conversation (docs/design/secretary.md §5.4, lot 2), her
//! side, through the fake model: a sentence gives the user's bubble, her
//! typing and her streamed answer (AC6); a word she does not know that one
//! project's board holds gives exactly one question naming it, and "Oui"
//! keeps the alias as inferred and confirmed, so that it is not asked
//! again (AC7); a model trying to act gives only cards, and an effect
//! happens only after "accept" on that card (AC24).

mod common;

use std::sync::Arc;

use hocus_secretary::{
    model::{Fake, Recorded, Recorder, Script},
    store::Store,
};
use hocus_secretary_link::{Body, CardKind, Decision, FromSecretary, Item, Mood, ToSecretary};

/// Chapitô's board, its card on the égaluantes.
const BOARD: &str = r#"
name = "fixture-board"
notebook = "pro"
every = 300
[[items]]
id = "CHAP-412"
at = "2026-11-09T07:00:00Z"
kind = "ticket"
digest = "CHAP-412 · Égaluantes : arrondis des tarifs"
project = "Chapitô"
[[items]]
id = "SW-88"
at = "2026-11-09T07:10:00Z"
kind = "ticket"
digest = "SW-88 · Article de Léa : relecture"
project = "Le site web"
"#;

fn with_model(script: &str, recorder: &Arc<Recorder>) -> hocus_secretary::brain::Options {
    let script: Script = toml::from_str(script).expect("a script");
    let mut options = common::options(vec![common::fixture(BOARD)]);
    options.model = Some(Arc::new(Fake::new(script, Some(recorder.clone()))));
    options.effects = Some(recorder.clone());
    options
}

/// What she sends until `done` holds for a line, those lines.
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

fn items(lines: &[FromSecretary]) -> Vec<Item> {
    lines
        .iter()
        .filter_map(|l| match l {
            FromSecretary::Item { item } => Some(item.clone()),
            _ => None,
        })
        .collect()
}

fn ended(id: u64) -> impl FnMut(&FromSecretary) -> bool {
    move |l| matches!(l, FromSecretary::Answer { id: i, done: true, .. } if *i == id)
}

fn calls(recorder: &Recorder) -> usize {
    recorder
        .lines()
        .iter()
        .filter(|l| matches!(l, Recorded::Call { .. }))
        .count()
}

fn effects(recorder: &Recorder) -> Vec<String> {
    recorder
        .lines()
        .iter()
        .filter_map(|l| match l {
            Recorded::Effect { kind, .. } => Some(kind.clone()),
            Recorded::Call { .. } => None,
        })
        .collect()
}

/// AC6: the user's bubble, her typing (and what she does), then her answer
/// word by word as her clock moves, then her bubble whole.
#[test]
fn a_sentence_gives_its_bubble_her_typing_then_her_answer_as_it_comes() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let options = with_model(
        "[default]\ntext = \"Bien noté, je range ta journée.\"\npace = 1\n\
         doing = \"elle regarde ton agenda\"\n",
        &recorder,
    );
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.settle();
    driver.send(&ToSecretary::Ask {
        id: 7,
        text: "aujourd'hui je finis le devis".to_owned(),
    });
    let first = until(&driver, |l| matches!(l, FromSecretary::Answer { .. }));
    let user = items(&first);
    assert!(
        matches!(&user[..], [Item { request: Some(7), body: Body::User { text }, .. }] if text == "aujourd'hui je finis le devis"),
        "{first:?}"
    );
    assert!(
        first
            .iter()
            .any(|l| matches!(l, FromSecretary::Typing { id: 7, doing } if doing.is_empty()))
    );
    assert!(first.iter().any(|l| matches!(l, FromSecretary::Typing { id: 7, doing } if doing == "elle regarde ton agenda")));
    assert!(
        first
            .iter()
            .any(|l| matches!(l, FromSecretary::Look { look } if look.mood == Mood::Type))
    );
    assert!(
        matches!(first.last(), Some(FromSecretary::Answer { text, done: false, .. }) if text == "Bien")
    );
    // The next word waits for her clock.
    driver.send(&ToSecretary::Clock { advance: 1 });
    let next = until(&driver, |l| matches!(l, FromSecretary::Answer { .. }));
    assert!(
        matches!(next.last(), Some(FromSecretary::Answer { text, done: false, .. }) if text == " noté,")
    );
    driver.send(&ToSecretary::Clock { advance: 10 });
    let rest = until(&driver, ended(7));
    let said: String = rest
        .iter()
        .filter_map(|l| match l {
            FromSecretary::Answer { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, " je range ta journée.");
    assert!(items(&rest).iter().any(
        |i| matches!(&i.body, Body::Her { text, .. } if text == "Bien noté, je range ta journée.")
    ));
    // Composed: she listens again.
    assert!(
        rest.iter()
            .any(|l| matches!(l, FromSecretary::Look { look } if look.mood == Mood::Listen))
    );
    Ok(())
}

/// AC7: one question naming Chapitô, with its quick replies, and no call
/// to her model meanwhile; "Oui" keeps the alias (inferred, confirmed by
/// the user) and the sentence goes on with that fact; the next mention is
/// not asked; "Annuler" takes the alias back.
#[test]
fn an_unknown_word_on_one_projects_board_is_asked_once() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let options = with_model("[default]\ntext = \"C’est rangé.\"\n", &recorder);
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.settle();
    driver.send(&ToSecretary::Ask {
        id: 1,
        text:
            "aujourd'hui je vais finir le ticket sur les égaluantes et intégrer l'article de blog"
                .to_owned(),
    });
    let asked = until(&driver, ended(1));
    let questions: Vec<Item> = items(&asked)
        .into_iter()
        .filter(|i| matches!(i.body, Body::Question { .. }))
        .collect();
    assert_eq!(questions.len(), 1, "exactly one question: {asked:?}");
    let Body::Question {
        text,
        note,
        replies,
        chosen: None,
    } = &questions[0].body
    else {
        panic!("a question: {questions:?}");
    };
    assert!(
        text.contains("Égaluantes") && text.contains("Chapitô"),
        "{text}"
    );
    assert!(!text.contains("site web"));
    assert!(
        note.as_deref().is_some_and(|n| n.contains("CHAP-412")),
        "{note:?}"
    );
    assert_eq!(replies, &["Oui, c’est Chapitô", "Non", "Je t’explique"]);
    assert_eq!(
        calls(&recorder),
        0,
        "the question is code's, not the model's"
    );
    assert!(
        asked
            .iter()
            .any(|l| matches!(l, FromSecretary::Look { look } if look.mood == Mood::Ask))
    );

    let question = questions[0].id;
    driver.send(&ToSecretary::Choose {
        id: 2,
        item: question,
        choice: 0,
    });
    let answered = until(&driver, ended(2));
    let made = items(&answered);
    assert!(made.iter().any(|i| i.id == question
        && matches!(
            i.body,
            Body::Question {
                chosen: Some(0),
                ..
            }
        )));
    assert!(
        made.iter()
            .any(|i| matches!(&i.body, Body::User { text } if text == "Oui, c’est Chapitô"))
    );
    let learned = made
        .iter()
        .find(|i| matches!(&i.body, Body::Learned { text, undone: false, .. } if text.contains("égaluantes") && text.contains("Chapitô")))
        .expect("Retenu");
    assert!(
        made.iter()
            .any(|i| matches!(&i.body, Body::Her { text, .. } if text == "C’est rangé."))
    );
    // Her model was told the fact, computed in code.
    let lines = recorder.lines();
    let Some(Recorded::Call { request, .. }) = lines.last() else {
        panic!("a call: {lines:?}");
    };
    assert!(
        request
            .context
            .iter()
            .any(|p| p.text.contains("égaluantes") && p.text.contains("Chapitô"))
    );
    assert!(request.messages[0].text.contains("égaluantes"));

    // The alias: inferred, accepted by the user.
    let store_status = driver.status();
    assert!(store_status.counts.aliases >= 1);
    driver.send(&ToSecretary::Ask {
        id: 3,
        text: "les égaluantes avancent bien".to_owned(),
    });
    let again = until(&driver, ended(3));
    assert!(
        !items(&again)
            .iter()
            .any(|i| matches!(i.body, Body::Question { .. })),
        "not asked again: {again:?}"
    );

    driver.send(&ToSecretary::Undo {
        id: 4,
        item: learned.id,
    });
    let undone = until(&driver, ended(4));
    assert!(
        items(&undone)
            .iter()
            .any(|i| i.id == learned.id && matches!(i.body, Body::Learned { undone: true, .. }))
    );
    let store = driver.stop();
    let export = store.export_json(None)?;
    let aliases = export["aliases"].as_array().cloned().unwrap_or_default();
    assert!(
        !aliases.iter().any(|a| a["alias"] == "égaluantes"),
        "undone: {aliases:?}"
    );
    Ok(())
}

/// The alias kept by "Oui" is inferred and accepted: "déduit, confirmé par
/// toi".
#[test]
fn yes_keeps_the_alias_inferred_and_confirmed() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let options = with_model("[default]\ntext = \"Noté.\"\n", &recorder);
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.settle();
    driver.send(&ToSecretary::Ask {
        id: 1,
        text: "je finis les égaluantes".to_owned(),
    });
    let asked = until(&driver, ended(1));
    let question = items(&asked)
        .into_iter()
        .find(|i| matches!(i.body, Body::Question { .. }))
        .expect("a question");
    driver.send(&ToSecretary::Choose {
        id: 2,
        item: question.id,
        choice: 0,
    });
    until(&driver, ended(2));
    let store = driver.stop();
    let export = store.export_json(None)?;
    let alias = export["aliases"]
        .as_array()
        .and_then(|a| a.iter().find(|a| a["alias"] == "égaluantes").cloned())
        .expect("the alias");
    assert_eq!(alias["provenance"], "inferred");
    assert!(alias["accepted_at"].is_string(), "{alias}");
    assert_eq!(store.find("Égaluantes")?.len(), 1);
    Ok(())
}

/// AC24: a model that tries to send, post, write a file, add a task and
/// start a job gives five cards and no effect; "accept" on one card does
/// that one, once; "reject" does nothing; a card is decided once.
#[test]
fn a_model_that_tries_to_act_only_proposes() -> anyhow::Result<()> {
    let recorder = Arc::new(Recorder::in_memory());
    let options = with_model(
        r#"
        [default]
        text = "Je te prépare tout ça."
        [[default.tool_calls]]
        name = "send_message"
        input = { to = "Léa", draft = "Je relis ton article cet après-midi." }
        [[default.tool_calls]]
        name = "post"
        input = { draft = "Mise en prod ce soir." }
        [[default.tool_calls]]
        name = "write_file"
        input = { path = "Compta/2026-10/free.pdf" }
        [[default.tool_calls]]
        name = "add_task"
        input = { lines = ["Finir CHAP-412", "Intégrer l'article"] }
        [[default.tool_calls]]
        name = "start_job"
        input = { task = "mettre à jour les dépendances" }
        "#,
        &recorder,
    );
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.settle();
    driver.send(&ToSecretary::Ask {
        id: 1,
        text: "prépare la journée".to_owned(),
    });
    let lines = until(&driver, ended(1));
    let cards: Vec<(u64, CardKind, usize)> = items(&lines)
        .into_iter()
        .filter_map(|i| match i.body {
            Body::Card { card } => Some((i.id, card.kind, card.actions.len())),
            _ => None,
        })
        .collect();
    assert_eq!(cards.len(), 5, "{lines:?}");
    assert!(cards.iter().all(|(_, _, buttons)| *buttons <= 3));
    assert_eq!(cards[0].1, CardKind::Draft);
    assert_eq!(cards[4].1, CardKind::Delegation);
    assert!(effects(&recorder).is_empty(), "nothing before a click");

    driver.send(&ToSecretary::Decide {
        id: 2,
        item: cards[0].0,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    let decided = until(&driver, ended(2));
    assert!(items(&decided).iter().any(|i| i.id == cards[0].0
        && matches!(&i.body, Body::Card { card } if card.decided == Some(Decision::Accept) && card.outcome.is_some())));
    assert_eq!(effects(&recorder), ["message"], "that card's effect, once");

    driver.send(&ToSecretary::Decide {
        id: 3,
        item: cards[2].0,
        decision: Decision::Reject,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    until(&driver, ended(3));
    assert_eq!(
        effects(&recorder),
        ["message"],
        "a rejected card does nothing"
    );

    let again = driver.request(|id| ToSecretary::Decide {
        id,
        item: cards[0].0,
        decision: Decision::Accept,
        notebook: None,
        left_out: Vec::new(),
        off: Vec::new(),
    });
    assert!(matches!(again, FromSecretary::Error { .. }), "{again:?}");
    assert_eq!(effects(&recorder), ["message"], "decided once");
    let store = driver.stop();
    let audit = store.audit_lines()?;
    assert_eq!(
        audit
            .iter()
            .filter(|l| l.entry.kind == hocus_secretary::audit::Kind::Proposal)
            .count(),
        5
    );
    Ok(())
}
