//! What she learns (docs/design/secretary.md §9, §10.2, lot 6), her side,
//! through the fake model, its recorder and fixture sources, on a clock of
//! her own:
//!
//! - AC27: "Julie est arrivée lundi" with a fixture Slack join gives a
//!   person card; "Ajouter" stores her with her arrival date and source;
//!   "Annuler" within 10 s removes her, and later it is too late.
//! - AC28: "Marc est parti vendredi" closes his tenure; he stays
//!   searchable with his history and is no longer offered as a reviewer.
//! - AC30: a team change seen in a source, and a rule proposed from
//!   observation, are proposals and change nothing until accepted.
//! - AC47: a said fact contradicting an older edge closes it at its date;
//!   nothing is deleted, the history shows both.
//! - AC48: the triage's candidate edges become proposals; nothing is
//!   written as confirmed until the user accepts.
//! - Taught rules: restated, checked, kept with "Annuler", their effect
//!   computed (leaves) or applied (a tool barely used).

mod common;

use std::sync::Arc;

use hocus_secretary::{
    graph::{NewEdge, NodeKind, Origin, Target},
    learn,
    model::{Fake, Recorded, Recorder, Request, Script},
    power::Sensors,
    store::{Store, Who},
    triage::{Fact, Triager, Verdict},
};
use hocus_secretary_link::{
    Body, Decision, FromSecretary, GraphQuery, Item, Notebook, ToSecretary,
    knows::{Act, Do, Knowledge, Query},
};
use jiff::civil::{Date, date};

fn options(
    fixtures: &[&str],
    script: &str,
    recorder: &Arc<Recorder>,
    power: &std::path::Path,
) -> hocus_secretary::brain::Options {
    let script: Script = toml::from_str(script).expect("a script");
    let mut options = common::options(fixtures.iter().map(|f| common::fixture(f)).collect());
    options.model = Some(Arc::new(Fake::new(script, Some(recorder.clone()))));
    options.effects = Some(recorder.clone());
    // No battery: mains power.
    options.sensors = Sensors {
        supplies: power.join("power_supply"),
        thermal: power.join("thermal"),
    };
    options
}

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

fn ended(id: u64) -> impl FnMut(&FromSecretary) -> bool {
    move |l| matches!(l, FromSecretary::Answer { id: i, done: true, .. } if *i == id)
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

fn ask(driver: &common::Driver, id: u64, text: &str) -> Vec<FromSecretary> {
    driver.send(&ToSecretary::Ask {
        id,
        text: text.to_owned(),
    });
    until(driver, ended(id))
}

/// A person, told, with edges told from `from`.
fn person(store: &mut Store, name: &str) -> i64 {
    store
        .add_node(NodeKind::Person, name, Notebook::Pro, &Who::User, "test")
        .expect("a person")
        .0
}

fn project(store: &mut Store, name: &str) -> i64 {
    store
        .add_node(NodeKind::Project, name, Notebook::Pro, &Who::User, "test")
        .expect("a project")
        .0
}

fn told(
    store: &mut Store,
    src: i64,
    kind: &str,
    dst: Target,
    from: Option<Date>,
    to: Option<Date>,
) -> i64 {
    store
        .add_edge(&NewEdge {
            src,
            kind: kind.to_owned(),
            target: dst,
            valid_from: from,
            valid_to: to,
            origin: Origin::told("conversation:0"),
            notebook: Notebook::Pro,
        })
        .expect("an edge")
        .0
}

fn graph(
    driver: &mut common::Driver,
    node: &str,
    as_of: Option<&str>,
) -> Option<hocus_secretary_link::Graph> {
    match driver.request(|id| ToSecretary::Graph {
        id,
        query: GraphQuery {
            node: node.to_owned(),
            as_of: as_of.map(ToOwned::to_owned),
            hops: 1,
            notebook: None,
        },
    }) {
        FromSecretary::Graph { graph, .. } => Some(graph),
        _ => None,
    }
}

fn calls(recorder: &Recorder) -> Vec<Request> {
    recorder
        .lines()
        .into_iter()
        .filter_map(|l| match l {
            Recorded::Call { request, .. } => Some(request),
            Recorded::Effect { .. } => None,
        })
        .collect()
}

fn view(driver: &mut common::Driver) -> Knowledge {
    match driver.request(|id| ToSecretary::Knows {
        id,
        query: Some(Query::default()),
    }) {
        FromSecretary::Knows { view, .. } => *view,
        other => panic!("no view: {other:?}"),
    }
}

fn act(driver: &mut common::Driver, item: &str, act: Act) -> FromSecretary {
    driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Item {
            item: item.to_owned(),
            act,
        },
    })
}

const SLACK: &str = r##"
name = "fixture-slack"
notebook = "pro"
[[items]]
id = "n1"
at = "2026-11-09T07:30:00Z"
kind = "message"
digest = "#général : Julie Berthier a rejoint le canal · Développeuse front"
[[items]]
id = "n2"
at = "2026-11-09T07:40:00Z"
kind = "message"
digest = "#site-web : Julie Berthier @julie.b : bonjour, je commence sur le site"
"##;

/// AC27: a person card from the Slack join; "Ajouter" keeps her with her
/// arrival date and where it was read; "Annuler" within ten seconds takes
/// her back; after, it is too late (corrected in "Ce qu'elle sait").
#[test]
fn an_arrival_said_is_searched_proposed_kept_and_undone_within_ten_seconds() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    project(&mut store, "Le site web");
    let mut driver = common::Driver::start(
        store,
        options(
            &[SLACK],
            "[default]\ntext = \"D’accord.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    let said = ask(&driver, 1, "Julie est arrivée lundi");
    let card = items(&said)
        .into_iter()
        .find(|i| matches!(i.body, Body::Card { .. }))
        .expect("a person card");
    let Body::Card { card: shown } = &card.body else {
        unreachable!()
    };
    let lines = shown.text.replace('\n', " | ");
    assert!(lines.contains("Julie Berthier"), "{lines}");
    assert!(lines.contains("Développeuse front"), "{lines}");
    assert!(
        lines.contains("Le site web") && lines.contains("#site-web"),
        "{lines}"
    );
    assert!(lines.contains("@julie.b"), "{lines}");
    assert_eq!(
        shown
            .actions
            .iter()
            .map(|a| a.label.as_str())
            .collect::<Vec<_>>(),
        ["Ajouter", "Modifier", "Ce n’est pas ça"]
    );
    // Nothing kept before the click, and no model call.
    assert!(graph(&mut driver, "Julie Berthier", None).is_none());
    assert!(calls(&recorder).is_empty());
    driver.send(&ToSecretary::Decide {
        id: 2,
        item: card.id,
        decision: Decision::Accept,
        notebook: None,
    });
    let kept = until(&driver, ended(2));
    let learned = items(&kept)
        .into_iter()
        .find(|i| matches!(i.body, Body::Learned { .. }))
        .expect("Retenu");
    let julie = graph(&mut driver, "Julie Berthier", None).expect("she is kept");
    let arrived = julie
        .edges
        .iter()
        .find(|e| e.kind == "arrived")
        .expect("her arrival");
    assert_eq!(arrived.valid_from.as_deref(), Some("2026-11-09"));
    let role = julie
        .edges
        .iter()
        .find(|e| e.kind == "role")
        .expect("her role");
    assert_eq!(role.value.as_deref(), Some("Développeuse front"));
    assert_eq!(role.pointer, "fixture-slack:n1", "where it was read");
    assert!(julie.edges.iter().any(|e| e.kind == "member_of"));
    // "Annuler" within ten seconds: she is gone.
    driver.advance(5);
    driver.send(&ToSecretary::Undo {
        id: 3,
        item: learned.id,
    });
    until(&driver, ended(3));
    assert!(
        graph(&mut driver, "Julie Berthier", None).is_none(),
        "undone"
    );
    // Again, and after ten seconds it is too late.
    let again = ask(&driver, 4, "Julie est arrivée lundi");
    let card = items(&again)
        .into_iter()
        .find(|i| matches!(i.body, Body::Card { .. }))
        .expect("a person card");
    driver.send(&ToSecretary::Decide {
        id: 5,
        item: card.id,
        decision: Decision::Accept,
        notebook: None,
    });
    let kept = until(&driver, ended(5));
    let learned = items(&kept)
        .into_iter()
        .find(|i| matches!(i.body, Body::Learned { .. }))
        .expect("Retenu");
    driver.send(&ToSecretary::Clock { advance: 11 });
    let settled = until(&driver, |l| {
        matches!(l, FromSecretary::Item { item } if item.id == learned.id
            && matches!(item.body, Body::Learned { settled: true, .. }))
    });
    assert!(!settled.is_empty());
    match driver.request(|id| ToSecretary::Undo {
        id,
        item: learned.id,
    }) {
        FromSecretary::Error { message, .. } => assert!(message.contains("too late"), "{message}"),
        other => panic!("not refused: {other:?}"),
    }
    assert!(
        graph(&mut driver, "Julie Berthier", None).is_some(),
        "still kept"
    );
    Ok(())
}

/// AC28: "Marc est parti vendredi" closes his tenure at that day; his
/// history stays (he is found, as of before); he is no longer offered as
/// a reviewer to her model.
#[test]
fn a_departure_closes_the_tenure_and_keeps_the_history() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let marc = person(&mut store, "Marc Delaunay");
    let chapito = project(&mut store, "Chapitô");
    told(
        &mut store,
        marc,
        "works_on",
        Target::Node(chapito),
        Some(date(2023, 1, 9)),
        None,
    );
    told(
        &mut store,
        marc,
        "reviews",
        Target::Node(chapito),
        Some(date(2024, 1, 1)),
        None,
    );
    let mut driver = common::Driver::start(
        store,
        options(
            &[],
            "[default]\ntext = \"Je regarde.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    // Before: offered as a reviewer.
    ask(&driver, 1, "qui peut relire les PR de Chapitô ?");
    let before = calls(&recorder).pop().expect("a call").text();
    assert!(
        before.contains("Marc Delaunay —reviews→ Chapitô"),
        "{before}"
    );
    let said = ask(&driver, 2, "Marc est parti vendredi");
    assert!(
        items(&said)
            .iter()
            .any(|i| matches!(&i.body, Body::Learned { text, .. }
            if text.contains("Marc Delaunay") && text.contains("6\u{a0}nov."))),
        "{said:?}"
    );
    ask(&driver, 3, "qui peut relire les PR de Chapitô ?");
    let after = calls(&recorder).pop().expect("a call").text();
    assert!(
        !after.contains("Marc Delaunay —reviews→"),
        "no longer offered: {after}"
    );
    assert!(!after.contains("Marc Delaunay —works_on→"), "{after}");
    // Searchable, his history whole.
    let then =
        graph(&mut driver, "Marc Delaunay", Some("2026-10-01")).expect("found, as of before");
    assert!(then.edges.iter().any(|e| e.kind == "reviews"));
    let now = graph(&mut driver, "Marc Delaunay", None).expect("found");
    assert!(
        now.edges.iter().all(|e| e.kind == "left"),
        "{:?}",
        now.edges
    );
    let store = driver.stop();
    let history = store.history(marc)?;
    let reviews: Vec<_> = history.iter().filter(|e| e.kind == "reviews").collect();
    assert_eq!(reviews.len(), 2, "the old version and the closed one");
    assert!(
        reviews
            .iter()
            .any(|e| e.superseded_at.is_none() && e.valid_to == Some(date(2026, 11, 6)))
    );
    assert_eq!(learn::left_on(&store, marc)?, Some(date(2026, 11, 6)));
    Ok(())
}

/// AC47: "Nadia dirige le site web depuis lundi" closes Marc's lead at
/// that day; nothing deleted, both in the history.
#[test]
fn a_said_fact_closes_the_one_it_contradicts_at_its_date() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let marc = person(&mut store, "Marc Delaunay");
    let nadia = person(&mut store, "Nadia Ferreira");
    let site = project(&mut store, "Le site web");
    told(
        &mut store,
        marc,
        "leads",
        Target::Node(site),
        Some(date(2024, 1, 1)),
        None,
    );
    let edges_before = store.count("edges")?;
    let mut driver = common::Driver::start(
        store,
        options(
            &[],
            "[default]\ntext = \"Bien.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    driver.advance(2 * 24 * 3600);
    let said = ask(&driver, 1, "Nadia dirige le site web depuis lundi");
    let texts: Vec<String> = items(&said)
        .iter()
        .map(|i| format!("{:?}", i.body))
        .collect();
    assert!(
        texts
            .iter()
            .any(|t| t.contains("Retenu") && t.contains("Nadia Ferreira")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("Marc Delaunay")),
        "who it was before: {texts:?}"
    );
    assert!(calls(&recorder).is_empty(), "read in code");
    let then = graph(&mut driver, "Le site web", Some("2026-11-01")).expect("the site");
    assert!(
        then.edges
            .iter()
            .any(|e| e.kind == "leads" && e.src == marc)
    );
    let now = graph(&mut driver, "Le site web", None).expect("the site");
    let leads: Vec<_> = now.edges.iter().filter(|e| e.kind == "leads").collect();
    assert_eq!(leads.len(), 1, "{leads:?}");
    assert_eq!(leads[0].src, nadia);
    assert_eq!(leads[0].valid_from.as_deref(), Some("2026-11-09"));
    let store = driver.stop();
    assert!(store.count("edges")? > edges_before, "nothing deleted");
    let history = store.history(site)?;
    assert!(
        history
            .iter()
            .any(|e| e.src == marc && e.valid_to == Some(date(2026, 11, 9)))
    );
    assert!(
        history
            .iter()
            .any(|e| e.src == marc && e.superseded_at.is_some())
    );
    Ok(())
}

const LEAVES: &str = r#"
name = "fixture-calendar"
notebook = "pro"
[[items]]
id = "c1"
at = "2026-11-09T06:00:00Z"
until = "2026-11-16T22:00:00Z"
kind = "event"
digest = "Congés Sophie"
calendar = "Congés"
[[items]]
id = "c2"
at = "2026-11-09T06:00:00Z"
until = "2026-11-10T18:00:00Z"
kind = "event"
digest = "Thomas : absent"
calendar = "Congés"
"#;

const BOARD: &str = r#"
name = "fixture-board"
notebook = "pro"
[[items]]
id = "P-1"
at = "2026-11-09T07:00:00Z"
kind = "card"
digest = "Planka : ranger les factures"
"#;

/// A rule taught in one sentence: restated as a card, checked against
/// what it names, kept with "Annuler", its effect said and in her next
/// context as a computed fact; a tool barely used leaves her day.
#[test]
fn a_rule_is_restated_checked_kept_and_applied() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    person(&mut store, "Sophie Marchand");
    person(&mut store, "Thomas Petit");
    let mut driver = common::Driver::start(
        store,
        options(
            &[LEAVES, BOARD],
            "[default]\ntext = \"Bien.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    let said = ask(
        &driver,
        1,
        "les personnes en congés sont notées dans le calendrier Congés",
    );
    let shown: Vec<String> = items(&said)
        .iter()
        .map(|i| format!("{:?}", i.body))
        .collect();
    assert!(
        shown
            .iter()
            .any(|t| t.contains("Je l’ai trouvé") && t.contains("2 personnes")),
        "checked: {shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|t| t.contains("Rule") && t.contains("Congés")),
        "{shown:?}"
    );
    assert!(
        shown.iter().any(|t| t.contains("Retenu · règle n")),
        "{shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|t| t.contains("Aujourd’hui") && t.contains("Sophie") && t.contains("Thomas")),
        "today's effect: {shown:?}"
    );
    assert!(calls(&recorder).is_empty(), "read in code");
    // Her next call has the leaves as a computed fact.
    ask(&driver, 2, "qui est là aujourd'hui ?");
    let text = calls(&recorder).pop().expect("a call").text();
    assert!(
        text.contains("[fait calculé] En congés aujourd'hui"),
        "{text}"
    );
    assert!(
        text.contains("Planka : ranger les factures"),
        "the board's card, before: {text}"
    );
    // A tool barely used: out of her day.
    let quiet = ask(&driver, 3, "on se sert très peu de Planka");
    assert!(
        items(&quiet)
            .iter()
            .any(|i| matches!(&i.body, Body::Learned { text, .. } if text.contains("Planka"))),
        "{quiet:?}"
    );
    ask(&driver, 4, "qu'est-ce qui est arrivé ce matin ?");
    let text = calls(&recorder).pop().expect("a call").text();
    assert!(!text.contains("Planka : ranger les factures"), "{text}");
    assert!(
        text.contains("Planka : à peine utilisé") || text.contains("Planka"),
        "the rule in her card"
    );
    let store = driver.stop();
    let compiled = learn::compiled(&store, date(2026, 11, 9))?;
    assert!(
        compiled
            .iter()
            .any(|(_, c)| matches!(c, learn::Compiled::Leave { calendar } if calendar == "Congés"))
    );
    assert!(
        compiled
            .iter()
            .any(|(_, c)| matches!(c, learn::Compiled::Quiet { tool } if tool == "Planka"))
    );
    Ok(())
}

const DEACTIVATED: &str = r#"
name = "fixture-slack"
notebook = "pro"
[[items]]
id = "d1"
at = "2026-11-09T07:30:00Z"
kind = "notification"
digest = "Slack : Paul Girard a été désactivé"
"#;

const OLD_BOARD: &str = r#"
name = "fixture-board"
notebook = "pro"
[[items]]
id = "P-1"
at = "2026-09-20T07:00:00Z"
kind = "card"
digest = "Planka : ranger les factures"
"#;

/// AC30: a team change seen in a source is a proposal: it changes nothing
/// (graph, context) until accepted. A rule she proposes from observation
/// (a board barely used) counts only once accepted.
#[test]
fn what_she_sees_and_notices_changes_nothing_until_accepted() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let paul = person(&mut store, "Paul Girard");
    let lune = project(&mut store, "Atelier Lune");
    told(
        &mut store,
        paul,
        "member_of",
        Target::Node(lune),
        Some(date(2021, 5, 1)),
        None,
    );
    let mut driver = common::Driver::start(
        store,
        options(
            &[DEACTIVATED, OLD_BOARD],
            "[default]\ntext = \"Bien.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    // The departure seen is a proposal, shown as one.
    let seen = view(&mut driver);
    let proposal = seen
        .items()
        .find(|k| k.proposal && k.title == "Paul Girard" && k.item.starts_with("edge:"))
        .expect("a proposal in Ce qu'elle sait")
        .clone();
    assert!(
        proposal.origin.text.contains("à confirmer"),
        "{:?}",
        proposal.origin
    );
    let now = graph(&mut driver, "Paul Girard", None).expect("Paul");
    assert!(
        now.edges.iter().any(|e| e.kind == "member_of"),
        "still in the team"
    );
    assert!(now.edges.iter().all(|e| e.kind != "left"));
    ask(&driver, 1, "qui est dans l'Atelier Lune ?");
    let text = calls(&recorder).pop().expect("a call").text();
    assert!(
        text.contains("Paul Girard —member_of→ Atelier Lune"),
        "{text}"
    );
    assert!(!text.contains("—left→"), "{text}");
    // Accepted: he is gone, his tenure closed.
    act(&mut driver, &proposal.item, Act::Accept);
    let now = graph(&mut driver, "Paul Girard", None).expect("Paul");
    assert!(
        now.edges.iter().all(|e| e.kind != "member_of"),
        "{:?}",
        now.edges
    );

    // The night: the board barely used is noticed and asked about.
    driver.send(&ToSecretary::Clock { advance: 24 * 3600 });
    let asked = until(
        &driver,
        |l| matches!(l, FromSecretary::Item { item } if matches!(&item.body, Body::Question { text, .. } if text.contains("Planka"))),
    );
    let question = items(&asked).pop().expect("her question");
    let Body::Question { replies, .. } = &question.body else {
        unreachable!()
    };
    assert_eq!(replies, &["Oui", "Non", "Plus tard"]);
    let probe = driver.stop();
    assert!(
        learn::compiled(&probe, date(2026, 11, 10))?.is_empty(),
        "no rule before the answer"
    );
    assert_eq!(learn::load_seen(&probe).len(), 1);
    // Answered "Oui" from Ce qu'elle sait (a fresh run on the same memory).
    let mut driver = common::Driver::start(
        probe,
        options(
            &[DEACTIVATED, OLD_BOARD],
            "[default]\ntext = \"Bien.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    let seen = view(&mut driver);
    let item = seen
        .items()
        .find(|k| k.item.starts_with("seen:"))
        .expect("the observation, still open")
        .item
        .clone();
    act(&mut driver, &item, Act::Accept);
    let store = driver.stop();
    let compiled = learn::compiled(&store, date(2026, 11, 10))?;
    assert!(
        compiled
            .iter()
            .any(|(_, c)| matches!(c, learn::Compiled::Quiet { tool } if tool == "Planka")),
        "{compiled:?}"
    );
    Ok(())
}

/// The local triage, scripted: each item's verdict with candidate facts.
#[derive(Debug)]
struct Facts(Vec<Fact>);

impl Triager for Facts {
    fn endpoint(&self) -> (String, String) {
        ("local".to_owned(), "fake".to_owned())
    }

    fn triage(&self, _input: &str) -> Result<(Verdict, u64, u64), String> {
        Ok((
            Verdict {
                digest: "Léa : nouvelles de l'équipe".to_owned(),
                facts: self.0.clone(),
                ..Verdict::default()
            },
            10,
            10,
        ))
    }

    fn unload(&self) -> Result<(), String> {
        Ok(())
    }
}

const MAIL: &str = r#"
name = "fixture-mail"
notebook = "pro"
[[items]]
id = "m1"
at = "2026-11-09T07:00:00Z"
kind = "mail"
digest = "Nouvelles de l'équipe"
sender = "Léa <lea@atelier-lune.fr>"
body = "Sophie passe sur Chapitô. Julie Berthier rejoint l'équipe du site lundi."
open = "https://mail.google.com/mail/u/0/#all/18c2"
"#;

/// AC48 and AC49: the triage's candidate edges become proposals; nothing
/// is confirmed before the user accepts; once accepted, "d'où je le
/// sais" names the mail and opens it; an inferred edge stays out of her
/// prompts until accepted.
#[test]
fn the_triage_s_facts_are_proposals_with_their_source() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let sophie = person(&mut store, "Sophie Marchand");
    let chapito = project(&mut store, "Chapitô");
    let site = project(&mut store, "Le site web");
    let (inferred, _) = store.add_edge(&NewEdge {
        src: sophie,
        kind: "reviews".to_owned(),
        target: Target::Node(site),
        valid_from: None,
        valid_to: None,
        origin: Origin::inferred("fixture-board:SW-88", 0.7),
        notebook: Notebook::Pro,
    })?;
    let mut options = options(
        &[MAIL],
        "[default]\ntext = \"Bien.\"\n",
        &recorder,
        power.path(),
    );
    options.triage = Some(Arc::new(Facts(vec![
        Fact {
            subject: "Sophie Marchand".to_owned(),
            relation: "travaille sur".to_owned(),
            object: "Chapitô".to_owned(),
            quote: "Sophie passe sur Chapitô".to_owned(),
        },
        Fact {
            subject: "Julie Berthier".to_owned(),
            relation: "rejoint l'équipe".to_owned(),
            object: "Le site web".to_owned(),
            quote: "Julie Berthier rejoint l'équipe du site lundi".to_owned(),
        },
    ])));
    let mut driver = common::Driver::start(store, options);
    driver.settle();
    // The triage runs; Julie, unknown, is asked about.
    let asked = until(
        &driver,
        |l| matches!(l, FromSecretary::Item { item } if matches!(&item.body, Body::Question { text, .. } if text.contains("Julie Berthier"))),
    );
    assert!(!asked.is_empty());
    let now = graph(&mut driver, "Sophie Marchand", None).expect("Sophie");
    assert!(
        now.edges.iter().all(|e| e.kind != "works_on"),
        "not confirmed: {:?}",
        now.edges
    );
    assert!(
        graph(&mut driver, "Julie Berthier", None).is_none(),
        "no node for a proposal"
    );
    // The inferred edge is in no prompt.
    ask(&driver, 1, "sur quoi travaille Sophie Marchand ?");
    let call = calls(&recorder).pop().expect("a call");
    assert!(
        !call.items().contains(&format!("edge:{inferred}")),
        "{:?}",
        call.items()
    );
    assert!(!call.text().contains("—reviews→"), "{}", call.text());
    // Accepted from Ce qu'elle sait.
    let seen = view(&mut driver);
    let works = seen
        .items()
        .find(|k| k.proposal && k.text.contains("travaille sur Chapitô"))
        .expect("the proposal")
        .clone();
    assert!(works.origin.text.contains("à confirmer"));
    act(&mut driver, &works.item, Act::Accept);
    let reviews = format!("edge:{inferred}");
    act(&mut driver, &reviews, Act::Accept);
    let now = graph(&mut driver, "Sophie Marchand", None).expect("Sophie");
    let edge = now
        .edges
        .iter()
        .find(|e| e.kind == "works_on" && e.dst == Some(chapito))
        .expect("confirmed");
    assert_eq!(edge.provenance, hocus_secretary_link::Provenance::Extracted);
    assert_eq!(edge.pointer, "fixture-mail:m1");
    let seen = view(&mut driver);
    let shown = seen
        .items()
        .find(|k| k.item == "node:1")
        .expect("Sophie in Ce qu'elle sait")
        .clone();
    assert!(!shown.origin.text.is_empty());
    let week = seen
        .sections
        .iter()
        .find(|s| s.key == "week")
        .expect("this week");
    let read = week
        .items
        .iter()
        .find(|k| k.text.contains("travaille sur Chapitô"))
        .expect("learned this week");
    assert!(read.origin.text.starts_with("lu dans"), "{:?}", read.origin);
    assert_eq!(
        read.origin.open.as_deref(),
        Some("https://mail.google.com/mail/u/0/#all/18c2"),
        "it opens its mail"
    );
    // Accepted, the inferred edge is in her next prompt.
    ask(&driver, 2, "sur quoi travaille Sophie Marchand ?");
    let call = calls(&recorder).pop().expect("a call");
    assert!(
        call.text()
            .contains("Sophie Marchand —reviews→ Le site web"),
        "{}",
        call.text()
    );
    // Every edge has a provenance and a pointer; every item of the view
    // says where it comes from.
    let store = driver.stop();
    let all = store.export_json(None)?;
    for edge in all["edges"].as_array().expect("edges") {
        assert!(
            matches!(
                edge["provenance"].as_str(),
                Some("told" | "extracted" | "inferred")
            ),
            "{edge}"
        );
        assert!(
            edge["pointer"].as_str().is_some_and(|p| !p.is_empty()),
            "{edge}"
        );
    }
    for known in seen.items() {
        assert!(!known.origin.text.is_empty(), "{known:?}");
    }
    Ok(())
}
