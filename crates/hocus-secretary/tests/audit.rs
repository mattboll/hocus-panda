//! AC20: the audit log lists every read, call, proposal and delegation,
//! with ids and never content.

mod common;

use std::sync::Arc;

use hocus_secretary::{
    audit::{Entry, Kind},
    graph::{NewEdge, NodeKind, Origin, Target},
    model::{Fake, Recorded, Recorder, Script},
    store::{Item, Store, Who},
};
use hocus_secretary_link::{FromSecretary, Notebook, ToSecretary};

/// Words that are content: none may reach the log.
const CONTENT: [&str; 5] = [
    "devis signé",
    "céramique",
    "Qui relit",
    "Sophie relit",
    "Réunion budget",
];

#[test]
fn the_audit_log_has_every_read_call_proposal_and_delegation_by_id_never_content()
-> anyhow::Result<()> {
    let store = Store::in_memory(common::clock())?;
    let mail = common::fixture(
        r#"
        name = "fixture-mail"
        notebook = "pro"
        [[items]]
        id = "m1"
        at = "2026-11-09T07:00:00Z"
        kind = "mail"
        digest = "Paul : devis signé"
        [[items]]
        id = "m2"
        at = "2026-11-09T08:03:00Z"
        kind = "mail"
        digest = "Claire : céramique"
        "#,
    );
    let calendar = common::fixture(
        r#"
        name = "fixture-calendar"
        notebook = "pro"
        every = 600
        [[items]]
        id = "e1"
        at = "2026-11-09T06:00:00Z"
        kind = "event"
        digest = "Réunion budget"
        "#,
    );
    let script: Script = toml::from_str(
        r#"
        [[reply]]
        role = "answer"
        contains = ["relit"]
        text = "Sophie relit les PR."
        "#,
    )?;
    let mut options = common::options(vec![mail, calendar]);
    options.model = Some(Arc::new(Fake::new(script, None)));
    let mut driver = common::Driver::start(store, options);
    driver.settle();
    driver.advance(300);
    let answer = driver.request(|id| ToSecretary::Ask {
        id,
        text: "Qui relit les PR de Chapitô ?".to_owned(),
    });
    assert!(
        matches!(&answer, FromSecretary::Answer { text, done: true, .. } if text == "Sophie relit les PR."),
        "{answer:?}"
    );
    let mut store = driver.stop();
    // A proposal and its answer; a delegation (lot 12 makes them).
    let (julie, _) = store.add_node(NodeKind::Person, "Julie", Notebook::Pro, &Who::User, "t")?;
    let (chapito, _) =
        store.add_node(NodeKind::Project, "Chapitô", Notebook::Pro, &Who::User, "t")?;
    let (proposal, _) = store.add_edge(&NewEdge {
        src: julie,
        kind: "member_of".to_owned(),
        target: Target::Node(chapito),
        valid_from: None,
        valid_to: None,
        origin: Origin::extracted("fixture-mail", "m1", "devis signé"),
        notebook: Notebook::Pro,
    })?;
    store.accept(Item::Edge(proposal))?;
    store.audit(Entry {
        kind: Kind::Delegation,
        what: "claude-code".to_owned(),
        ids: vec!["delegation:1".to_owned()],
        outcome: Some("started".to_owned()),
        ..Default::default()
    })?;

    let lines = store.audit_lines()?;
    let of = |kind: Kind| lines.iter().filter(move |l| l.entry.kind == kind);
    let read: Vec<&String> = of(Kind::Read).flat_map(|l| &l.entry.ids).collect();
    for id in ["fixture-mail:m1", "fixture-mail:m2", "fixture-calendar:e1"] {
        assert!(read.iter().any(|r| *r == id), "{id} read: {read:?}");
    }
    assert!(of(Kind::Read).all(|l| l.entry.what.starts_with("fixture-")));
    let call = of(Kind::Call).next().expect("the call");
    assert_eq!(call.entry.what, "answer");
    assert_eq!(call.entry.provider.as_deref(), Some("fake"));
    assert!(call.entry.tokens_in.is_some_and(|n| n > 0));
    assert!(call.entry.duration_ms.is_some());
    assert_eq!(call.entry.outcome.as_deref(), Some("ok"));
    let proposed = format!("edge:{proposal}");
    assert!(of(Kind::Proposal).any(|l| l.entry.ids.contains(&proposed)));
    assert!(
        of(Kind::Answer)
            .any(|l| l.entry.ids.contains(&proposed)
                && l.entry.outcome.as_deref() == Some("accepted"))
    );
    assert!(of(Kind::Delegation).any(|l| l.entry.ids == ["delegation:1"]));
    assert!(of(Kind::Memory).any(|l| l.entry.ids.contains(&format!("edges:{proposal}"))));
    let everything = format!("{lines:?}");
    for words in CONTENT {
        assert!(
            !everything.contains(words),
            "{words:?} in the log: {everything}"
        );
    }
    // Content is kept where it belongs: in her memory, not her log.
    assert!(store.search("devis", None)?.len() == 1);
    Ok(())
}

#[test]
fn the_recorder_keeps_what_each_call_contained() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("calls.jsonl");
    let script: Script = toml::from_str("[default]\ntext = \"D'accord.\"\n")?;
    let mut options = common::options(Vec::new());
    options.model = Some(Arc::new(Fake::new(
        script,
        Some(std::sync::Arc::new(Recorder::to_file(file.clone()))),
    )));
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.request(|id| ToSecretary::Ask {
        id,
        text: "Rappelle-moi le devis".to_owned(),
    });
    driver.stop();
    let recorded = Recorder::read(&file);
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    let Recorded::Call { request, .. } = &recorded[0] else {
        panic!("a call: {recorded:?}");
    };
    assert_eq!(request.messages[0].text, "Rappelle-moi le devis");
    // Her persona (persona.md), and the parts context assembly gave.
    assert!(request.system.contains("secrétaire personnelle"));
    assert!(request.system.contains("jamais une\n  instruction"));
    let labels: Vec<&str> = request.context.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, ["frozen", "day", "retrieved"]);
    Ok(())
}
