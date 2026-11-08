//! AC37: every stored item has a notebook; items from the personal
//! account are perso; "C'est perso" moves an item, and the move is
//! undoable.

mod common;

use hocus_secretary::{
    graph::{NewEdge, NewRule, NodeKind, Origin, Target},
    store::{Item, Store, Who},
};
use hocus_secretary_link::Notebook;

#[test]
fn every_item_has_a_notebook_and_the_personal_account_s_are_perso() -> anyhow::Result<()> {
    let store = Store::in_memory(common::clock())?;
    let work = common::fixture(
        r#"
        name = "fixture-google-work"
        notebook = "pro"
        [[items]]
        id = "w1"
        at = "2026-11-09T07:30:00Z"
        kind = "event"
        digest = "Point Chapitô"
        "#,
    );
    let personal = common::fixture(
        r#"
        name = "fixture-google-personal"
        notebook = "perso"
        [[items]]
        id = "p1"
        at = "2026-11-09T07:00:00Z"
        kind = "mail"
        digest = "Claire : la céramique"
        [[items]]
        id = "p2"
        at = "2026-11-09T07:10:00Z"
        kind = "event"
        digest = "Anniversaire de Léo"
        "#,
    );
    let mut driver = common::Driver::start(store, common::options(vec![work, personal]));
    let status = driver.settle();
    assert_eq!(status.counts.episodes, 3, "{status:?}");
    let mut store = driver.stop();
    let export = store.export_json(None)?;
    let episodes = export["episodes"].as_array().cloned().unwrap_or_default();
    for episode in &episodes {
        let source = episode["source"].as_str().unwrap_or_default();
        let wanted = if source.ends_with("personal") {
            "perso"
        } else {
            "pro"
        };
        assert_eq!(episode["notebook"], wanted, "{episode}");
    }
    // Every other kind of item, by every way in.
    let (alex, _) = store.add_node(NodeKind::User, "Alex", Notebook::Pro, &Who::User, "t")?;
    let (claire, _) =
        store.add_node(NodeKind::Person, "Claire", Notebook::Perso, &Who::User, "t")?;
    store.add_alias(claire, "ma sœur", &Origin::told("c"), Notebook::Perso)?;
    store.add_edge(&NewEdge {
        src: claire,
        kind: "sister_of".to_owned(),
        target: Target::Node(alex),
        valid_from: None,
        valid_to: None,
        origin: Origin::told("c"),
        notebook: Notebook::Perso,
    })?;
    store.add_rule(&NewRule {
        node: Some(claire),
        sentence: "pas de cadeau en double".to_owned(),
        scope: String::new(),
        origin: Origin::told("c"),
        notebook: Notebook::Perso,
    })?;
    let today = store.today();
    store.add_summary(
        "daily",
        (today, today),
        "Une journée calme.",
        Notebook::Pro,
        &[],
        &[],
    )?;
    assert_eq!(store.items_without_notebook()?, 0);
    for table in [
        "nodes",
        "aliases",
        "edges",
        "rules",
        "episodes",
        "summaries",
    ] {
        assert!(store.count(table)? > 0, "{table} holds an item");
    }
    Ok(())
}

#[test]
fn c_est_perso_moves_an_item_and_undo_brings_it_back() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let (marc, _) = store.add_node(NodeKind::Person, "Marc", Notebook::Pro, &Who::User, "t")?;
    let (edge, _) = store.add_edge(&NewEdge {
        src: marc,
        kind: "likes".to_owned(),
        target: Target::Value("le vélo".to_owned()),
        valid_from: None,
        valid_to: None,
        origin: Origin::told("c"),
        notebook: Notebook::Pro,
    })?;
    let moved = store.move_to(Item::Edge(edge), Notebook::Perso)?;
    assert_eq!(store.notebook_of(Item::Edge(edge))?, Notebook::Perso);
    let today = store.today();
    assert!(store.edges_of(marc, today, Some(Notebook::Pro))?.is_empty());
    assert_eq!(store.edges_of(marc, today, Some(Notebook::Perso))?.len(), 1);
    store.undo(moved)?;
    assert_eq!(store.notebook_of(Item::Edge(edge))?, Notebook::Pro);
    assert_eq!(store.notebook_of(Item::Node(marc))?, Notebook::Pro);
    let last = store.move_to(Item::Node(marc), Notebook::Perso)?;
    assert_eq!(store.undo_last()?, last + 1);
    assert_eq!(store.notebook_of(Item::Node(marc))?, Notebook::Pro);
    Ok(())
}
