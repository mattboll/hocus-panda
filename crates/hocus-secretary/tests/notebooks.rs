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

/// « C'est privé » checked on a granted source's row (#752): her book
/// says the source's new notebook, she takes it at once (her book is
/// watched) and what came from the source moves with it, its parts (a
/// calendar of an account) too; another source's stays.
#[test]
fn a_source_made_private_moves_what_came_from_it_at_once() -> anyhow::Result<()> {
    use hocus_secretary_link::consent::{ConsentBook, SourceKey, SourceKind};
    let dir = tempfile::tempdir()?;
    let book = dir.path().join("consents.json");
    let calendar = SourceKey::new("account_1", SourceKind::Calendar);
    let mail = SourceKey::new("account_1", SourceKind::Mail);
    let mut consents = ConsentBook::default();
    for source in [&calendar, &mail] {
        consents.allow(
            source.clone(),
            "alex@atelier-lune.fr",
            Notebook::Pro,
            jiff::Timestamp::UNIX_EPOCH,
        );
    }
    consents.save(&book)?;
    let account = common::fixture(
        r#"
        name = "google"
        notebook = "pro"
        [[items]]
        id = "e1"
        at = "2026-11-09T07:30:00Z"
        kind = "event"
        digest = "Kiné"
        from = "account_1:calendar#famille@group.calendar.google.com"
        [[items]]
        id = "e2"
        at = "2026-11-09T07:40:00Z"
        kind = "event"
        digest = "Ostéo"
        from = "account_1:calendar"
        [[items]]
        id = "m1"
        at = "2026-11-09T07:10:00Z"
        kind = "mail"
        digest = "Léa : la recette"
        from = "account_1:mail"
        "#,
    );
    let memory = dir.path().join("memory.db");
    let key = [3u8; 32];
    let mut driver = common::Driver::start(
        Store::open(&memory, &key, common::clock())?,
        hocus_secretary::brain::Options {
            consents: Some(book.clone()),
            ..common::options(vec![account])
        },
    );
    let status = driver.settle();
    assert_eq!(status.counts.episodes, 3);
    consents.set_notebook(&calendar, Notebook::Perso, jiff::Timestamp::UNIX_EPOCH);
    consents.save(&book)?;
    // Taken at once: her clock does not move.
    let start = std::time::Instant::now();
    let moved = loop {
        // Read beside her, as another connection to her memory.
        let read = Store::open(&memory, &key, common::clock())?;
        let notebooks = (
            read.notebook_rows(Notebook::Pro)?,
            read.notebook_rows(Notebook::Perso)?,
        );
        drop(read);
        if notebooks.1 == 2 {
            break notebooks;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "never moved: {notebooks:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(moved, (1, 2), "the mail stays work");
    let store = driver.stop();
    assert_eq!(store.notebook_rows(Notebook::Perso)?, 2);
    Ok(())
}
