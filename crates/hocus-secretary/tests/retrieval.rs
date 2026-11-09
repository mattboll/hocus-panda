//! Context assembly and hybrid retrieval (§7.3), through her loop and the
//! scripted fake model's recorder: what each request held.
//!
//! - AC45: an as-of question; the date resolved in code from the graph,
//!   the edges valid on that day in the context, never one that was not.
//! - AC46: a chain of relations the question names, found whole.
//! - AC31: under its cap (25k tokens) with 10,000 log lines and 200
//!   people.

mod common;

use std::sync::Arc;

use hocus_secretary::{
    context::{self, Inbox, Scope},
    graph::{NewEdge, NewEpisode, NewRule, NodeKind, Origin, Target},
    model::{Fake, Message, Recorded, Recorder, Role, Script},
    retrieve,
    store::{Store, Who},
};
use hocus_secretary_link::Notebook;
use jiff::{Timestamp, ToSpan, civil::Date};

fn day(text: &str) -> Date {
    text.parse().expect("a date")
}

fn node(store: &mut Store, kind: NodeKind, name: &str) -> anyhow::Result<i64> {
    Ok(store
        .add_node(kind, name, Notebook::Pro, &Who::User, "test")?
        .0)
}

fn told(
    store: &mut Store,
    src: i64,
    kind: &str,
    target: Target,
    from: &str,
) -> anyhow::Result<i64> {
    Ok(store
        .add_edge(&NewEdge {
            src,
            kind: kind.to_owned(),
            target,
            valid_from: Some(day(from)),
            valid_to: None,
            origin: Origin::told("conversation:test"),
            notebook: Notebook::Pro,
        })?
        .0)
}

/// What the fake model was asked, through her loop.
fn asked(store: Store, question: &str) -> anyhow::Result<hocus_secretary::model::Request> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("calls.jsonl");
    let script: Script = toml::from_str("[default]\ntext = \"D'accord.\"\n")?;
    let mut options = common::options(Vec::new());
    options.model = Some(Arc::new(Fake::new(
        script,
        Some(Recorder::to_file(file.clone())),
    )));
    let mut driver = common::Driver::start(store, options);
    driver.answer_to(question);
    driver.stop();
    let recorded = Recorder::read(&file);
    match recorded.into_iter().next() {
        Some(Recorded::Call { request, .. }) => Ok(request),
        other => anyhow::bail!("no call: {other:?}"),
    }
}

fn part<'a>(request: &'a hocus_secretary::model::Request, label: &str) -> &'a str {
    request
        .context
        .iter()
        .find(|p| p.label == label)
        .map(|p| p.text.as_str())
        .unwrap_or_default()
}

#[test]
fn ac45_who_was_on_chapito_when_v2_shipped() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let chapito = node(&mut store, NodeKind::Project, "Chapitô")?;
    let marc = node(&mut store, NodeKind::Person, "Marc")?;
    let nadia = node(&mut store, NodeKind::Person, "Nadia Ferreira")?;
    let sophie = node(&mut store, NodeKind::Person, "Sophie")?;
    let julie = node(&mut store, NodeKind::Person, "Julie")?;
    let marc_led = told(
        &mut store,
        marc,
        "leads",
        Target::Node(chapito),
        "2023-01-09",
    )?;
    let nadia_leads = told(
        &mut store,
        nadia,
        "leads",
        Target::Node(chapito),
        "2026-03-01",
    )?;
    let sophie_on = told(
        &mut store,
        sophie,
        "works_on",
        Target::Node(chapito),
        "2023-01-01",
    )?;
    let julie_on = told(
        &mut store,
        julie,
        "works_on",
        Target::Node(chapito),
        "2026-11-09",
    )?;
    let (shipped, _) = store.add_edge(&NewEdge {
        src: chapito,
        kind: "shipped_on".to_owned(),
        target: Target::Value("v2".to_owned()),
        valid_from: Some(day("2026-04-15")),
        valid_to: None,
        origin: Origin::extracted("fixture-mail", "release", "La v2 est livrée"),
        notebook: Notebook::Pro,
    })?;
    // Marc's lead, as closed when Nadia's began.
    let marc_closed: Vec<i64> = store
        .history(marc)?
        .into_iter()
        .filter(|e| e.kind == "leads")
        .map(|e| e.id)
        .collect();
    assert!(marc_closed.len() == 2 && marc_closed.contains(&marc_led));

    let request = asked(store, "Qui était sur Chapitô quand on a livré la v2 ?")?;
    let retrieved = part(&request, context::RETRIEVED);
    assert!(retrieved.contains("as_of=\"2026-04-15\""), "{retrieved}");
    assert!(
        retrieved.contains("[fait calculé] La question porte sur le 2026-04-15"),
        "the date is resolved in code: {retrieved}"
    );
    let items = &request
        .context
        .iter()
        .find(|p| p.label == context::RETRIEVED)
        .expect("retrieved")
        .items;
    for valid in [nadia_leads, sophie_on, shipped] {
        assert!(
            items.contains(&format!("edge:{valid}")),
            "edge {valid} in {items:?}"
        );
    }
    for invalid in marc_closed.iter().chain([&julie_on]) {
        assert!(
            !items.contains(&format!("edge:{invalid}")),
            "edge {invalid} was not valid then: {retrieved}"
        );
    }
    assert!(
        retrieved.contains("Nadia Ferreira —leads→ Chapitô"),
        "{retrieved}"
    );
    assert!(
        retrieved.contains("Sophie —works_on→ Chapitô"),
        "{retrieved}"
    );
    assert!(!retrieved.contains("Julie"), "{retrieved}");
    assert!(!retrieved.contains("Marc —leads→"), "{retrieved}");
    Ok(())
}

#[test]
fn ac46_a_chain_of_relations_is_followed_and_cited() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let sophie = node(&mut store, NodeKind::Person, "Sophie")?;
    let lea = node(&mut store, NodeKind::Person, "Léa")?;
    let paul = node(&mut store, NodeKind::Person, "Paul")?;
    let web = node(&mut store, NodeKind::Topic, "Équipe web")?;
    let marc = node(&mut store, NodeKind::Person, "Marc")?;
    let bob = node(&mut store, NodeKind::Person, "Bob")?;
    let mobile = node(&mut store, NodeKind::Topic, "Équipe mobile")?;
    let replaces = told(
        &mut store,
        lea,
        "replaces",
        Target::Node(sophie),
        "2026-10-01",
    )?;
    let member = told(
        &mut store,
        lea,
        "member_of",
        Target::Node(web),
        "2026-10-01",
    )?;
    let reviews = told(&mut store, paul, "reviews", Target::Node(web), "2025-01-01")?;
    told(
        &mut store,
        marc,
        "member_of",
        Target::Node(mobile),
        "2025-01-01",
    )?;
    let other = told(
        &mut store,
        bob,
        "reviews",
        Target::Node(mobile),
        "2025-01-01",
    )?;

    let question = "Qui relit les PR de l'équipe de la personne qui remplace Sophie ?";
    let linked = retrieve::link(&store, question)?;
    assert_eq!(linked.iter().map(|l| l.node).collect::<Vec<_>>(), [sophie]);
    let chain = retrieve::traverse(&store, &[sophie], day("2026-11-09"), None, question)?;
    let hops: Vec<(i64, u8)> = chain.iter().map(|(e, h)| (e.id, *h)).collect();
    assert_eq!(hops, [(replaces, 1), (member, 2), (reviews, 3)]);

    let request = asked(store, question)?;
    let retrieved = part(&request, context::RETRIEVED);
    for (edge, line) in [
        (replaces, "Léa —replaces→ Sophie"),
        (member, "Léa —member_of→ Équipe web"),
        (reviews, "Paul —reviews→ Équipe web"),
    ] {
        assert!(retrieved.contains(line), "{line} in {retrieved}");
        assert!(
            retrieved.contains(&format!("edge:{edge}")),
            "cited: {retrieved}"
        );
    }
    assert!(!retrieved.contains(&format!("edge:{other}")), "{retrieved}");
    Ok(())
}

#[test]
fn a_name_misspelt_or_a_first_name_still_links() -> anyhow::Result<()> {
    let mut store = Store::in_memory(common::clock())?;
    let nadia = node(&mut store, NodeKind::Person, "Nadia Ferreira")?;
    let chapito = node(&mut store, NodeKind::Project, "Chapitô")?;
    let site = node(&mut store, NodeKind::Project, "Le site web")?;
    let linked = |q: &str| -> anyhow::Result<Vec<i64>> {
        Ok(retrieve::link(&store, q)?
            .into_iter()
            .map(|l| l.node)
            .collect())
    };
    assert_eq!(linked("Nadia est sur quoi ?")?, [nadia]);
    assert_eq!(linked("où en est chapito")?, [chapito]);
    assert_eq!(linked("et Chapittô ?")?, [chapito]);
    assert_eq!(linked("qui fait le site web ?")?, [site]);
    assert!(linked("le site de la mairie")?.is_empty());
    let today = day("2026-11-09");
    let when = |q: &str| retrieve::when(&store, q, &[], today).map(|w| w.map(|w| w.day));
    assert_eq!(
        when("qui était là le 15 avril 2026 ?")?,
        Some(day("2026-04-15"))
    );
    assert_eq!(when("et au 2026-03-02 ?")?, Some(day("2026-03-02")));
    assert_eq!(
        when("en mars 2026, qui dirigeait ?")?,
        Some(day("2026-03-15"))
    );
    assert_eq!(
        when("l'an dernier, qui était là ?")?,
        Some(day("2025-11-09"))
    );
    assert_eq!(when("qui est là ?")?, None);
    Ok(())
}

#[test]
fn ac31_the_context_stays_under_its_cap_with_a_big_memory() -> anyhow::Result<()> {
    let clock = common::clock();
    let mut store = Store::in_memory(clock.clone())?;
    let project = node(&mut store, NodeKind::Project, "Chapitô")?;
    let user = node(&mut store, NodeKind::User, "Alex")?;
    let mut people = Vec::new();
    for i in 0..200 {
        let person = node(&mut store, NodeKind::Person, &format!("Personne Numéro{i}"))?;
        told(
            &mut store,
            person,
            "works_on",
            Target::Node(project),
            "2025-01-01",
        )?;
        told(
            &mut store,
            person,
            "member_of",
            Target::Node(user),
            "2024-01-01",
        )?;
        people.push(person);
    }
    for i in 0..30 {
        store.add_rule(&NewRule {
            node: Some(project),
            sentence: format!("Règle apprise numéro {i} : on relit les PR le jeudi"),
            scope: String::new(),
            origin: Origin::told("conversation:test"),
            notebook: Notebook::Pro,
        })?;
    }
    let now: Timestamp = common::START.parse()?;
    for i in 0..10_000_i64 {
        let at = now.checked_sub((i * 7).minutes())?;
        let mention = people[usize::try_from(i % 200)?];
        store.add_episode(
            &NewEpisode {
                at,
                kind: if i % 5 == 0 { "event" } else { "mail" }.to_owned(),
                digest: format!(
                    "Réunion Chapitô numéro {i} avec Personne Numéro{} sur la recette et le devis",
                    i % 200
                ),
                source: Some("fixture-mail".to_owned()),
                pointer: format!("fixture-mail:m{i}"),
                notebook: Notebook::Pro,
            },
            &[mention, project],
        )?;
    }
    let started = std::time::Instant::now();
    let request = context::assemble(
        &store,
        &Inbox::default(),
        Role::Answer,
        &[
            Message::user("Bonjour"),
            Message::assistant("Bonjour Alex."),
            Message::user("Où en est la recette de Chapitô avec Personne Numéro12 ?"),
        ],
        &Scope::default(),
        now,
    )?;
    let took = started.elapsed();
    let cost = context::cost(&request);
    println!(
        "AC31: {cost} tokens (cap {}), assembled in {took:?}",
        context::CAP
    );
    assert!(cost <= context::CAP, "{cost} tokens");
    let frozen =
        context::tokens(&request.system) + context::tokens(part(&request, context::FROZEN));
    assert!(frozen <= context::FROZEN_CAP + 600, "frozen: {frozen}");
    assert!(context::tokens(part(&request, context::DAY)) <= context::DAY_CAP);
    assert!(context::tokens(part(&request, context::RETRIEVED)) <= context::RETRIEVED_CAP);
    assert_eq!(request.messages.len(), 3);
    let retrieved = part(&request, context::RETRIEVED);
    assert!(retrieved.contains("Personne Numéro12"), "{retrieved}");
    assert!(
        retrieved.contains("de plus"),
        "what does not fit is said, not sent"
    );
    Ok(())
}
