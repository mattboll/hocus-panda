//! "Ce qu'elle sait" and the night (docs/design/secretary.md §7.2, §7.4,
//! §8, lot 6), her side, through the fake model and its recorder, on a
//! clock of her own:
//!
//! - AC29: "Oublier" removes an item from her memory, its index and every
//!   later prompt (with five seconds to take it back); "N'est plus vrai"
//!   keeps it with an end date.
//! - AC32: the daily compaction runs once a night on mains power (not on
//!   a low battery) and writes one summary; the duplicates of an alias
//!   pair are merged.
//! - AC39: a work draft is assembled without any perso item.
//! - AC40: "Oublier tout le perso" removes the perso items from her
//!   memory, the index and the summaries, and leaves the pro ones.

mod common;

use std::{path::Path, sync::Arc};

use hocus_secretary::{
    graph::{NewEdge, NodeKind, Origin, Target},
    model::{Fake, Recorded, Recorder, Request, Script},
    power::Sensors,
    store::{Item, Store, Who},
};
use hocus_secretary_link::{
    FromSecretary, Notebook, ToSecretary,
    knows::{Act, Do, Knowledge, Query},
};
use jiff::civil::date;

fn sensors(dir: &Path) -> Sensors {
    Sensors {
        supplies: dir.join("power_supply"),
        thermal: dir.join("thermal"),
    }
}

/// A laptop on battery at `charge`, its mains `online` or not.
fn laptop(dir: &Path, online: bool, charge: u8) {
    let supplies = dir.join("power_supply");
    for (name, kind) in [("AC", "Mains"), ("BAT0", "Battery")] {
        std::fs::create_dir_all(supplies.join(name)).expect("a folder");
        std::fs::write(supplies.join(name).join("type"), kind).expect("written");
    }
    std::fs::write(supplies.join("AC/online"), if online { "1" } else { "0" }).expect("written");
    std::fs::write(supplies.join("BAT0/capacity"), charge.to_string()).expect("written");
}

fn options(
    fixtures: &[&str],
    script: &str,
    recorder: &Arc<Recorder>,
    power: &Path,
) -> hocus_secretary::brain::Options {
    let script: Script = toml::from_str(script).expect("a script");
    let mut options = common::options(fixtures.iter().map(|f| common::fixture(f)).collect());
    options.model = Some(Arc::new(Fake::new(script, Some(recorder.clone()))));
    options.sensors = sensors(power);
    options
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

fn view(driver: &mut common::Driver, query: Query) -> Knowledge {
    match driver.request(|id| ToSecretary::Knows {
        id,
        query: Some(query),
    }) {
        FromSecretary::Knows { view, .. } => *view,
        other => panic!("no view: {other:?}"),
    }
}

fn node(store: &mut Store, kind: NodeKind, name: &str, notebook: Notebook) -> i64 {
    store
        .add_node(kind, name, notebook, &Who::User, "test")
        .expect("a node")
        .0
}

fn edge(store: &mut Store, src: i64, kind: &str, target: Target, notebook: Notebook) -> i64 {
    store
        .add_edge(&NewEdge {
            src,
            kind: kind.to_owned(),
            target,
            valid_from: Some(date(2026, 6, 14)),
            valid_to: None,
            origin: Origin::told("conversation:0"),
            notebook,
        })
        .expect("an edge")
        .0
}

/// AC29: forgotten from the view, it is gone from her memory, its index
/// and her next prompt; "Annuler" within five seconds brings it back,
/// later it is too late. "N'est plus vrai" keeps it, ended.
#[test]
fn forget_takes_it_everywhere_and_no_longer_true_keeps_it_ended() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let lea = node(&mut store, NodeKind::Person, "Léa Rousseau", Notebook::Pro);
    let site = node(&mut store, NodeKind::Project, "Le site web", Notebook::Pro);
    let works = edge(
        &mut store,
        lea,
        "works_on",
        Target::Node(site),
        Notebook::Pro,
    );
    let glaze = node(&mut store, NodeKind::Topic, "Émaillage", Notebook::Pro);
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
    let seen = view(&mut driver, Query::default());
    let lea_item = seen
        .items()
        .find(|k| k.title == "Léa Rousseau" && k.item.starts_with("node:"))
        .expect("Léa in the view")
        .clone();
    assert!(lea_item.actions.contains(&Act::Forget));
    assert!(!lea_item.origin.text.is_empty(), "d'où je le sais");
    // "N'est plus vrai" on her work on the site: ended, kept.
    let item = format!("edge:{works}");
    match driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Item {
            item: item.clone(),
            act: Act::Close,
        },
    }) {
        FromSecretary::Toast { text, undo, .. } => {
            assert!(text.contains("Plus vrai"), "{text}");
            assert!(undo.is_some());
        }
        other => panic!("no toast: {other:?}"),
    }
    // Forgotten: gone, then back with "Annuler", then gone for good.
    let forget = |driver: &mut common::Driver| match driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Item {
            item: lea_item.item.clone(),
            act: Act::Forget,
        },
    }) {
        FromSecretary::Toast { text, undo, .. } => {
            assert_eq!(text, "Oublié");
            undo.expect("Annuler")
        }
        other => panic!("no toast: {other:?}"),
    };
    let nodes = driver.status().counts.nodes;
    let token = forget(&mut driver);
    assert_eq!(
        driver.status().counts.nodes,
        nodes - 1,
        "gone from her memory"
    );
    let after = view(&mut driver, Query::default());
    assert!(
        after.items().all(|k| k.title != "Léa Rousseau"),
        "{:?}",
        after
            .items()
            .filter(|k| k.title == "Léa Rousseau")
            .collect::<Vec<_>>()
    );
    driver.advance(3);
    match driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Restore { token },
    }) {
        FromSecretary::Knows { view, .. } => {
            assert!(view.items().any(|k| k.title == "Léa Rousseau"), "back");
        }
        other => panic!("not restored: {other:?}"),
    }
    let token = forget(&mut driver);
    driver.advance(10);
    match driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Restore { token },
    }) {
        FromSecretary::Error { .. } => {}
        other => panic!("not refused: {other:?}"),
    }
    let answer = driver.answer_to("que fait Léa Rousseau sur le site web ?");
    assert_eq!(answer, "Bien.");
    let call = calls(&recorder).pop().expect("a call");
    // Her context (the question itself names her).
    let context: String = call.context.iter().map(|p| p.text.as_str()).collect();
    assert!(!context.contains("Léa Rousseau"), "{context}");
    assert!(!call.items().contains(&format!("node:{lea}")));
    let store = driver.stop();
    assert!(store.find("Léa Rousseau")?.is_empty());
    assert!(
        store.search("Rousseau", None)?.is_empty(),
        "out of the index"
    );
    assert!(store.node(glaze).is_ok(), "the rest stays");
    assert!(
        store
            .journal()?
            .iter()
            .all(|l| !format!("{:?}", l.changes).contains("Rousseau")),
        "the journal keeps only that a forget happened"
    );
    Ok(())
}

const DAY: &str = r#"
name = "fixture-mail"
notebook = "pro"
[[items]]
id = "m1"
at = "2026-11-09T07:00:00Z"
kind = "mail"
digest = "Nadia : la recette de Chapitô"
[[items]]
id = "m2"
at = "2026-11-09T07:20:00Z"
kind = "mail"
digest = "Léa : l'article du blog"
"#;

const COMPACTION: &str = r#"
[[reply]]
role = "compaction"
text = "Lundi : la recette de Chapitô avec Nadia, l'article de Léa."
[default]
text = "Bien."
"#;

/// AC32: once a night, on mains power, one daily summary (her model's
/// words), and "Nad" merged into Nadia Ferreira; on a low battery it
/// waits for power.
#[test]
fn the_night_writes_one_summary_on_power_and_merges_duplicates() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    laptop(power.path(), false, 20);
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let nadia = node(
        &mut store,
        NodeKind::Person,
        "Nadia Ferreira",
        Notebook::Pro,
    );
    let (alias, _) =
        store.add_alias(nadia, "Nad", &Origin::told("conversation:0"), Notebook::Pro)?;
    let _ = alias;
    let chapito = node(&mut store, NodeKind::Project, "Chapitô", Notebook::Pro);
    // A source made another node for her, by her nickname.
    let (nad, _) = store.add_node(
        NodeKind::Person,
        "Nad",
        Notebook::Pro,
        &Who::Source("fixture-mail".to_owned()),
        "named in a mail",
    )?;
    edge(
        &mut store,
        nad,
        "works_on",
        Target::Node(chapito),
        Notebook::Pro,
    );
    let mut driver =
        common::Driver::start(store, options(&[DAY], COMPACTION, &recorder, power.path()));
    driver.settle();
    let compactions = |recorder: &Recorder| {
        calls(recorder)
            .iter()
            .filter(|c| c.role == hocus_secretary::model::Role::Compaction)
            .count()
    };
    // The night on a low battery: nothing.
    driver.advance(22 * 3600);
    assert_eq!(compactions(&recorder), 0, "it waits for power");
    assert_eq!(driver.status().counts.summaries, 0);
    // On mains: once.
    laptop(power.path(), true, 20);
    driver.advance(3600);
    driver.until("one summary", |s| {
        s.counts.summaries == 1 && s.calls_running == 0
    });
    assert_eq!(compactions(&recorder), 1);
    let call = calls(&recorder)
        .into_iter()
        .find(|c| c.role == hocus_secretary::model::Role::Compaction)
        .expect("the compaction's call");
    assert!(
        call.text().contains("la recette de Chapitô"),
        "{}",
        call.text()
    );
    driver.advance(3600);
    driver.advance(3 * 3600);
    assert_eq!(compactions(&recorder), 1, "once a night");
    assert_eq!(driver.status().counts.summaries, 1);
    // The merge shows in "Appris cette semaine".
    let seen = view(&mut driver, Query::default());
    assert!(
        seen.items().any(|k| k.item.starts_with("edit:")
            && k.text.contains("Nad")
            && k.text.contains("Nadia Ferreira")),
        "{:?}",
        seen.sections.iter().map(|s| &s.items).collect::<Vec<_>>()
    );
    let store = driver.stop();
    assert_eq!(store.find("Nad")?, vec![nadia], "merged");
    assert!(store.node(nad).is_err());
    assert!(
        store
            .history(nadia)?
            .iter()
            .any(|e| e.kind == "works_on" && e.target == Target::Node(chapito)),
        "its edges moved"
    );
    let text = store.summaries_text()?;
    assert!(text.contains("recette"), "{text}");
    Ok(())
}

const PERSO: &str = r#"
name = "fixture-mail-perso"
notebook = "perso"
[[items]]
id = "p1"
at = "2026-11-09T06:30:00Z"
kind = "mail"
digest = "Margaux : je me suis mise au kayak"
"#;

/// AC39: a work draft's call holds no perso item; a question that is no
/// draft may.
#[test]
fn a_work_draft_is_assembled_from_the_pro_notebook_only() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let margaux = node(&mut store, NodeKind::Person, "Margaux", Notebook::Perso);
    edge(
        &mut store,
        margaux,
        "likes",
        Target::Value("le kayak".to_owned()),
        Notebook::Perso,
    );
    let lea = node(&mut store, NodeKind::Person, "Léa Rousseau", Notebook::Pro);
    let site = node(&mut store, NodeKind::Project, "Le site web", Notebook::Pro);
    edge(
        &mut store,
        lea,
        "works_on",
        Target::Node(site),
        Notebook::Pro,
    );
    let mut driver = common::Driver::start(
        store,
        options(
            &[DAY, PERSO],
            "[default]\ntext = \"Voilà.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    driver.answer_to("rédige un mail à Léa Rousseau pour lui dire que je relis son article");
    let draft = calls(&recorder).pop().expect("a call");
    assert!(
        !draft.items().contains(&format!("node:{margaux}")),
        "{:?}",
        draft.items()
    );
    assert!(!draft.text().contains("Margaux"), "{}", draft.text());
    assert!(!draft.text().contains("kayak"), "{}", draft.text());
    assert!(draft.text().contains("Léa Rousseau"));
    // Not a draft: both notebooks.
    driver.answer_to("qu'est-ce que Margaux aime ?");
    let question = calls(&recorder).pop().expect("a call");
    assert!(question.text().contains("kayak"), "{}", question.text());
    Ok(())
}

const PER_NOTEBOOK: &str = r#"
[[reply]]
role = "compaction"
contains = ["kayak"]
text = "Margaux s'est mise au kayak."
[[reply]]
role = "compaction"
text = "La recette de Chapitô et l'article de Léa."
[default]
text = "Voilà."
"#;

/// AC40: the perso notebook forgotten (after the shell's confirmation):
/// its items gone from her memory, the index and the summaries; the pro
/// ones intact.
#[test]
fn forgetting_the_perso_notebook_leaves_the_pro_one() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut store = Store::in_memory(common::clock())?;
    let margaux = node(&mut store, NodeKind::Person, "Margaux", Notebook::Perso);
    edge(
        &mut store,
        margaux,
        "likes",
        Target::Value("le kayak".to_owned()),
        Notebook::Perso,
    );
    let lea = node(&mut store, NodeKind::Person, "Léa Rousseau", Notebook::Pro);
    let mut driver = common::Driver::start(
        store,
        options(&[DAY, PERSO], PER_NOTEBOOK, &recorder, power.path()),
    );
    driver.settle();
    // A night: a summary of each notebook.
    driver.advance(24 * 3600);
    driver.until("two summaries", |s| {
        s.counts.summaries == 2 && s.calls_running == 0
    });
    let perso_view = view(
        &mut driver,
        Query {
            notebook: Some(Notebook::Perso),
            ..Query::default()
        },
    );
    assert!(perso_view.items().any(|k| k.title == "Margaux"));
    match driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::ForgetNotebook {
            notebook: Notebook::Perso,
        },
    }) {
        FromSecretary::Toast { text, undo, .. } => {
            assert!(text.contains("perso"), "{text}");
            assert!(undo.is_none(), "confirmed before, not taken back");
        }
        other => panic!("no toast: {other:?}"),
    }
    let all = view(&mut driver, Query::default());
    assert!(all.items().all(|k| k.title != "Margaux"));
    assert!(all.items().any(|k| k.title == "Léa Rousseau"), "pro intact");
    let store = driver.stop();
    assert_eq!(
        store.notebook_rows(Notebook::Perso)?,
        0,
        "no perso item left"
    );
    assert!(store.search("kayak", None)?.is_empty(), "out of the index");
    assert!(store.find("Margaux")?.is_empty());
    assert!(store.node(lea).is_ok());
    let summaries = store.summaries_text()?;
    assert!(!summaries.contains("kayak"), "{summaries}");
    assert!(
        summaries.contains("Chapitô"),
        "the pro summary stays: {summaries}"
    );
    let _ = Item::Node(margaux);
    Ok(())
}
