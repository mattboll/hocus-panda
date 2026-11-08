//! Her temporal graph (§7.1): edges closed, never deleted; as-of queries
//! on the valid time, beliefs on the recorded time; aliases; provenance
//! and proposals; notebooks; undo; forget; export; encryption at rest.

mod common;

use hocus_secretary::{
    graph::{NewEdge, NewEpisode, NewRule, NodeKind, Origin, Target},
    store::{Item, Store, Who},
};
use hocus_secretary_link::{Notebook, Provenance};
use jiff::{Timestamp, civil::Date};

fn day(text: &str) -> Date {
    text.parse().expect("a date")
}

fn edge(src: i64, kind: &str, target: Target, from: Option<&str>, origin: Origin) -> NewEdge {
    NewEdge {
        src,
        kind: kind.to_owned(),
        target,
        valid_from: from.map(day),
        valid_to: None,
        origin,
        notebook: Notebook::Pro,
    }
}

/// The spec's Chapitô (AC45's fixture): Marc leads it until 2026-03-01,
/// Nadia from then; Sophie works on it from 2023, Julie from 2026-11-09;
/// v2 shipped on 2026-04-15.
struct Chapito {
    store: Store,
    chapito: i64,
    marc: i64,
    nadia: i64,
    sophie: i64,
    julie: i64,
}

fn chapito() -> anyhow::Result<Chapito> {
    let mut store = Store::in_memory(common::clock())?;
    let person = |store: &mut Store, name: &str| -> anyhow::Result<i64> {
        Ok(store
            .add_node(NodeKind::Person, name, Notebook::Pro, &Who::User, "test")?
            .0)
    };
    let (chapito, _) = store.add_node(
        NodeKind::Project,
        "Chapitô",
        Notebook::Pro,
        &Who::User,
        "test",
    )?;
    let marc = person(&mut store, "Marc")?;
    let nadia = person(&mut store, "Nadia Ferreira")?;
    let sophie = person(&mut store, "Sophie")?;
    let julie = person(&mut store, "Julie")?;
    let told = || Origin::told("conversation:12-oct");
    store.add_edge(&edge(
        marc,
        "leads",
        Target::Node(chapito),
        Some("2023-01-09"),
        told(),
    ))?;
    store.add_edge(&edge(
        nadia,
        "leads",
        Target::Node(chapito),
        Some("2026-03-01"),
        told(),
    ))?;
    store.add_edge(&edge(
        sophie,
        "works_on",
        Target::Node(chapito),
        Some("2023-01-01"),
        told(),
    ))?;
    store.add_edge(&edge(
        julie,
        "works_on",
        Target::Node(chapito),
        Some("2026-11-09"),
        told(),
    ))?;
    store.add_edge(&edge(
        chapito,
        "shipped_on",
        Target::Value("v2".to_owned()),
        Some("2026-04-15"),
        Origin::extracted("fixture-mail", "release", "La v2 est livrée"),
    ))?;
    Ok(Chapito {
        store,
        chapito,
        marc,
        nadia,
        sophie,
        julie,
    })
}

fn people_on(store: &Store, project: i64, on: &str) -> anyhow::Result<Vec<String>> {
    let around = store.neighbourhood(project, 1, day(on), None)?;
    let mut names: Vec<String> = around
        .edges
        .iter()
        .filter(|e| e.target == Target::Node(project))
        .map(|e| Ok(format!("{} {}", store.node(e.src)?.name, e.kind)))
        .collect::<anyhow::Result<_>>()?;
    names.sort();
    Ok(names)
}

#[test]
fn a_told_lead_closes_the_older_one_and_nothing_is_deleted() -> anyhow::Result<()> {
    let c = chapito()?;
    // Marc's lead: superseded, and a version closed at Nadia's start.
    let history: Vec<_> = c
        .store
        .history(c.marc)?
        .into_iter()
        .filter(|e| e.kind == "leads")
        .collect();
    assert_eq!(history.len(), 2, "{history:?}");
    assert!(history[0].superseded_at.is_some() && history[0].valid_to.is_none());
    assert_eq!(history[1].valid_to, Some(day("2026-03-01")));
    assert_eq!(history[0].lineage, history[1].lineage);
    assert!(history[1].superseded_at.is_none());
    // Sophie's and Julie's works_on are not exclusive: nothing closed.
    assert!(
        c.store
            .history(c.sophie)?
            .iter()
            .all(|e| e.valid_to.is_none() && e.superseded_at.is_none())
    );
    assert_eq!(c.store.count("edges")?, 6, "five said, one more version");
    Ok(())
}

#[test]
fn as_of_the_v2_who_was_on_chapito() -> anyhow::Result<()> {
    let c = chapito()?;
    let shipped = c
        .store
        .edges_of(c.chapito, day("2026-11-09"), None)?
        .into_iter()
        .find(|e| e.kind == "shipped_on")
        .and_then(|e| e.valid_from)
        .expect("the v2's date");
    assert_eq!(shipped, day("2026-04-15"));
    assert_eq!(
        people_on(&c.store, c.chapito, "2026-04-15")?,
        ["Nadia Ferreira leads", "Sophie works_on"]
    );
    assert_eq!(
        people_on(&c.store, c.chapito, "2025-06-01")?,
        ["Marc leads", "Sophie works_on"]
    );
    assert_eq!(
        people_on(&c.store, c.chapito, "2026-11-09")?,
        ["Julie works_on", "Nadia Ferreira leads", "Sophie works_on"]
    );
    // Two hops: from Julie, through Chapitô, to the others.
    let around = c.store.neighbourhood(c.julie, 2, day("2026-11-10"), None)?;
    let names: Vec<&str> = around.nodes.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"Nadia Ferreira") && names.contains(&"Sophie"),
        "{names:?}"
    );
    assert!(
        !names.contains(&"Marc"),
        "Marc no longer leads it: {names:?}"
    );
    let _ = c.nadia;
    Ok(())
}

#[test]
fn what_she_believed_then_is_the_recorded_time() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let (chapito, _) =
        store.add_node(NodeKind::Project, "Chapitô", Notebook::Pro, &Who::User, "t")?;
    let (marc, _) = store.add_node(NodeKind::Person, "Marc", Notebook::Pro, &Who::User, "t")?;
    let (nadia, _) = store.add_node(NodeKind::Person, "Nadia", Notebook::Pro, &Who::User, "t")?;
    store.add_edge(&edge(
        marc,
        "leads",
        Target::Node(chapito),
        Some("2023-01-09"),
        Origin::told("c1"),
    ))?;
    let before: Timestamp = store.now();
    store.clock().advance(std::time::Duration::from_secs(3600));
    store.add_edge(&edge(
        nadia,
        "leads",
        Target::Node(chapito),
        Some("2026-03-01"),
        Origin::told("c2"),
    ))?;
    let then = store.believed_at(chapito, before)?;
    assert_eq!(then.len(), 1);
    assert_eq!(
        (then[0].src, then[0].valid_to),
        (marc, None),
        "then, Marc led it, open"
    );
    let now = store.believed_at(chapito, store.now())?;
    assert_eq!(now.len(), 2);
    assert!(
        now.iter()
            .any(|e| e.src == marc && e.valid_to == Some(day("2026-03-01")))
    );
    Ok(())
}

#[test]
fn aliases_are_folded_and_an_inferred_one_counts_once_accepted() -> anyhow::Result<()> {
    let mut c = chapito()?;
    assert_eq!(c.store.find("chapito")?, [c.chapito]);
    c.store
        .add_alias(c.nadia, "Nad", &Origin::told("conversation"), Notebook::Pro)?;
    assert_eq!(c.store.find("nad")?, [c.nadia]);
    let (alias, _) = c.store.add_alias(
        c.chapito,
        "Égaluantes",
        &Origin::inferred("CHAP-412", 0.8),
        Notebook::Pro,
    )?;
    assert!(
        c.store.find("egaluantes")?.is_empty(),
        "a proposal names nothing"
    );
    assert!(c.store.proposals()?.contains(&Item::Alias(alias)));
    c.store.accept(Item::Alias(alias))?;
    assert_eq!(c.store.find("  ÉGALUANTES ")?, [c.chapito]);
    assert!(c.store.accept(Item::Alias(alias)).is_err(), "accepted once");
    Ok(())
}

#[test]
fn provenance_says_what_counts() -> anyhow::Result<()> {
    let mut c = chapito()?;
    let today = day("2026-11-09");
    // Extracted, about a team: a proposal; it drives nothing.
    let (arrival, _) = c.store.add_edge(&edge(
        c.julie,
        "member_of",
        Target::Node(c.chapito),
        Some("2026-11-09"),
        Origin::extracted("fixture-slack", "join-1", "Julie a rejoint #chapito"),
    ))?;
    assert!(c.store.edge(arrival)?.is_proposal());
    assert!(
        c.store
            .edges_of(c.julie, today, None)?
            .iter()
            .all(|e| e.id != arrival)
    );
    // Extracted, a taste: kept at once, with its pointer and its words.
    let (claire, _) =
        c.store
            .add_node(NodeKind::Person, "Claire", Notebook::Perso, &Who::User, "t")?;
    let mut likes = edge(
        claire,
        "likes",
        Target::Value("la céramique".to_owned()),
        Some("2026-06-14"),
        Origin::extracted(
            "fixture-mail-perso",
            "m14",
            "je me suis mise à la céramique",
        ),
    );
    likes.notebook = Notebook::Perso;
    let (likes, _) = c.store.add_edge(&likes)?;
    let kept = c.store.edge(likes)?;
    assert!(!kept.is_proposal());
    assert_eq!(kept.provenance, Provenance::Extracted);
    assert_eq!(kept.pointer, "fixture-mail-perso:m14");
    assert_eq!(
        kept.quote.as_deref(),
        Some("je me suis mise à la céramique")
    );
    // Inferred: a proposal until accepted; accepted, it closes what it
    // contradicts as a told fact does.
    let (replaces, _) = c.store.add_edge(&edge(
        c.julie,
        "leads",
        Target::Node(c.chapito),
        Some("2026-12-01"),
        Origin::inferred("edge:3 edge:4", 0.6),
    ))?;
    assert!(
        c.store
            .edges_of(c.nadia, day("2026-12-02"), None)?
            .iter()
            .any(|e| e.kind == "leads"),
        "a proposal closes nothing"
    );
    c.store.accept(Item::Edge(replaces))?;
    assert!(
        !c.store
            .edges_of(c.nadia, day("2026-12-02"), None)?
            .iter()
            .any(|e| e.kind == "leads"),
        "accepted, it closed Nadia's lead"
    );
    // Rejected: gone.
    c.store.reject(Item::Edge(arrival))?;
    assert!(c.store.edge(arrival).is_err());
    Ok(())
}

#[test]
fn notebooks_filter_the_graph() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let (alex, _) = store.add_node(NodeKind::User, "Alex", Notebook::Pro, &Who::User, "t")?;
    let (claire, _) =
        store.add_node(NodeKind::Person, "Claire", Notebook::Perso, &Who::User, "t")?;
    let (chapito, _) =
        store.add_node(NodeKind::Project, "Chapitô", Notebook::Pro, &Who::User, "t")?;
    let mut sister = edge(
        claire,
        "sister_of",
        Target::Node(alex),
        None,
        Origin::told("c"),
    );
    sister.notebook = Notebook::Perso;
    store.add_edge(&sister)?;
    store.add_edge(&edge(
        alex,
        "works_on",
        Target::Node(chapito),
        None,
        Origin::told("c"),
    ))?;
    let today = day("2026-11-09");
    let pro = store.neighbourhood(alex, 2, today, Some(Notebook::Pro))?;
    assert!(pro.nodes.iter().all(|n| n.name != "Claire"), "{pro:?}");
    assert!(pro.edges.iter().all(|e| e.notebook == Notebook::Pro));
    let perso = store.neighbourhood(alex, 2, today, Some(Notebook::Perso))?;
    assert!(perso.nodes.iter().any(|n| n.name == "Claire"));
    assert!(perso.nodes.iter().all(|n| n.name != "Chapitô"));
    let all = store.neighbourhood(alex, 2, today, None)?;
    assert_eq!(all.edges.len(), 2);
    assert!(store.search("claire", Some(Notebook::Pro))?.is_empty());
    assert_eq!(
        store.search("claire", Some(Notebook::Perso))?,
        [Item::Node(claire)]
    );
    Ok(())
}

#[test]
fn undo_plays_an_edit_backwards_and_redo_is_an_undo_of_it() -> anyhow::Result<()> {
    let mut c = chapito()?;
    let today = day("2026-11-09");
    let (_, told) = c.store.add_edge(&edge(
        c.marc,
        "leads",
        Target::Node(c.chapito),
        Some("2026-11-09"),
        Origin::told("conversation:9-nov"),
    ))?;
    let lead = |s: &Store| -> anyhow::Result<Vec<i64>> {
        Ok(s.edges_of(c.chapito, today, None)?
            .iter()
            .filter(|e| e.kind == "leads")
            .map(|e| e.src)
            .collect())
    };
    assert_eq!(lead(&c.store)?, [c.marc], "Marc again, Nadia's closed");
    let undo = c.store.undo(told)?;
    assert_eq!(lead(&c.store)?, [c.nadia], "undone: Nadia's open again");
    assert!(c.store.undo(told).is_err(), "undone once");
    c.store.undo(undo)?;
    assert_eq!(lead(&c.store)?, [c.marc], "redone");
    // An edit a later one built on: undo that one first.
    let (julie_lead, add) = c.store.add_edge(&edge(
        c.julie,
        "reviews",
        Target::Node(c.chapito),
        None,
        Origin::told("c"),
    ))?;
    let (_, closed) = c
        .store
        .close_edge(julie_lead, day("2026-11-20"), "n'est plus vrai")?;
    assert!(c.store.undo(add).is_err());
    c.store.undo(closed)?;
    c.store.undo(add)?;
    assert!(c.store.edge(julie_lead).is_err());
    let journal = c.store.journal()?;
    assert!(
        journal
            .iter()
            .all(|l| !l.who.is_empty() && !l.why.is_empty())
    );
    Ok(())
}

#[test]
fn forgetting_removes_from_the_tables_the_index_and_the_journal() -> anyhow::Result<()> {
    let mut c = chapito()?;
    let (episode, _) = c.store.add_episode(
        &NewEpisode {
            at: c.store.now(),
            kind: "message".to_owned(),
            digest: "Julie arrive sur le projet".to_owned(),
            source: Some("fixture-slack".to_owned()),
            pointer: "fixture-slack:j1".to_owned(),
            notebook: Notebook::Pro,
        },
        &[c.julie, c.chapito],
    )?;
    let (summary, _) = c.store.add_summary(
        "daily",
        (day("2026-11-09"), day("2026-11-09")),
        "Julie a rejoint Chapitô.",
        Notebook::Pro,
        &[episode],
        &[c.julie],
    )?;
    c.store.add_rule(&NewRule {
        node: Some(c.julie),
        sentence: "Julie relit les PR le matin".to_owned(),
        scope: String::new(),
        origin: Origin::told("c"),
        notebook: Notebook::Pro,
    })?;
    assert_eq!(
        c.store.search("julie", None)?.first(),
        Some(&Item::Node(c.julie))
    );
    // A change the journal kept her row's snapshot for.
    c.store.move_to(Item::Node(c.julie), Notebook::Perso)?;
    assert!(format!("{:?}", c.store.journal()?).contains("Julie\""));
    c.store.forget(Item::Node(c.julie))?;
    assert!(c.store.node(c.julie).is_err());
    assert!(c.store.find("julie")?.is_empty());
    assert!(
        c.store
            .search("julie", None)?
            .iter()
            .all(|i| *i == Item::Episode(episode)),
        "the node and the summary leave the index; the episode stays, without its mention"
    );
    let export = c.store.export_json(None)?.to_string();
    assert!(
        !export.contains("Julie a rejoint"),
        "the summary is emptied until rebuilt"
    );
    assert!(!export.contains("relit les PR"), "its rules go");
    let journal = format!("{:?}", c.store.journal()?);
    assert!(
        !journal.contains("Julie\""),
        "the journal keeps no snapshot of her"
    );
    assert!(c.store.journal()?.iter().any(|l| l.op == "forget"));
    assert_eq!(c.store.count("mentions")?, 1, "Chapitô's mention stays");
    let _ = summary;
    Ok(())
}

#[test]
fn forgetting_a_notebook_or_a_source_leaves_the_rest() -> anyhow::Result<()> {
    let mut c = chapito()?;
    let (claire, _) =
        c.store
            .add_node(NodeKind::Person, "Claire", Notebook::Perso, &Who::User, "t")?;
    let mut likes = edge(
        claire,
        "likes",
        Target::Value("la céramique".to_owned()),
        None,
        Origin::extracted("fixture-mail-perso", "m14", "céramique"),
    );
    likes.notebook = Notebook::Perso;
    c.store.add_edge(&likes)?;
    let pro_edges = c.store.count("edges")?;
    c.store.forget_notebook(Notebook::Perso)?;
    assert!(c.store.node(claire).is_err());
    assert_eq!(c.store.count("edges")?, pro_edges - 1);
    assert_eq!(c.store.find("chapito")?, [c.chapito]);
    // The release mail's source: its extracted edge goes.
    c.store.set_cursor("fixture-mail", "m9")?;
    c.store.forget_source("fixture-mail")?;
    assert!(
        c.store
            .edges_of(c.chapito, day("2026-11-09"), None)?
            .iter()
            .all(|e| e.kind != "shipped_on")
    );
    assert_eq!(c.store.cursor("fixture-mail")?, None);
    assert_eq!(people_on(&c.store, c.chapito, "2026-11-09")?.len(), 3);
    Ok(())
}

#[test]
fn the_export_says_everything_and_where_from() -> anyhow::Result<()> {
    let c = chapito()?;
    let json = c.store.export_json(None)?;
    assert_eq!(json["nodes"].as_array().map(Vec::len), Some(5));
    assert_eq!(json["edges"].as_array().map(Vec::len), Some(6));
    let markdown = c.store.export_markdown(None)?;
    assert!(markdown.contains("### Nadia Ferreira (pro)"), "{markdown}");
    assert!(
        markdown.contains("leads → Chapitô [2026-03-01, …) · told"),
        "{markdown}"
    );
    assert!(
        markdown.contains("[2023-01-09, 2026-03-01) (plus vrai)"),
        "{markdown}"
    );
    assert!(markdown.contains("shipped_on → « v2 »"), "{markdown}");
    let perso = c.store.export_json(Some(Notebook::Perso))?;
    assert_eq!(perso["nodes"].as_array().map(Vec::len), Some(0));
    Ok(())
}

#[test]
fn her_memory_on_disk_is_sealed() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("memory.db");
    let key = [7u8; 32];
    {
        let mut store = Store::open(&file, &key, common::clock())?;
        store.add_node(
            NodeKind::Person,
            "Nadia Ferreira",
            Notebook::Pro,
            &Who::User,
            "t",
        )?;
        store.add_episode(
            &NewEpisode {
                at: store.now(),
                kind: "mail".to_owned(),
                digest: "le devis de Chapitô".to_owned(),
                source: None,
                pointer: "x".to_owned(),
                notebook: Notebook::Pro,
            },
            &[],
        )?;
    }
    let mut bytes = std::fs::read(&file)?;
    if let Ok(wal) = std::fs::read(dir.path().join("memory.db-wal")) {
        bytes.extend(wal);
    }
    let seen = |needle: &str| bytes.windows(needle.len()).any(|w| w == needle.as_bytes());
    assert!(!seen("Nadia") && !seen("devis") && !seen("SQLite format"));
    assert!(
        Store::open(&file, &[8u8; 32], common::clock()).is_err(),
        "another key"
    );
    let store = Store::open(&file, &key, common::clock())?;
    assert_eq!(store.find("nadia ferreira")?.len(), 1);
    Ok(())
}
