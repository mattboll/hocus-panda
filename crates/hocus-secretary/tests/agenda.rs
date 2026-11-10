//! #762, through her loop and the scripted fake model's recorder:
//!
//! - her day's part lists every event of today and tomorrow from the
//!   calendars she reads, whatever the mails of the day and the calendars
//!   others share; the user's own first when it has to be cut, what is
//!   cut said with the tool that gives it;
//! - her read-only `agenda(from, to)` lists another day's events;
//! - her `remember`: what the user tells her is kept at once, "Noté : …"
//!   with "Annuler", in the notebook they said, and shows in "Ce qu'elle
//!   sait"; what a source says is never kept by it;
//! - her graph fills: what she was told becomes nodes, and a mail's fact
//!   linking someone she does not know to it is asked about, then kept;
//! - her persona names only the tools she has, and never a setting that
//!   does not exist.

use crate::common;

use std::sync::Arc;

use hocus_secretary::{
    context::{self, Language},
    model::{Fake, Recorded, Recorder, Request, Script, ToolCall},
    power::Sensors,
    store::Store,
    tools,
    triage::{Fact, Triager, Verdict},
};
use hocus_secretary_link::{
    Body, FromSecretary, GraphQuery, Item, Notebook, ToSecretary,
    knows::{Act, Do, Knowledge, Query},
};
use jiff::{Timestamp, civil::Date, tz::TimeZone};

fn options(
    fixtures: Vec<hocus_secretary::sources::Fixture>,
    script: &str,
    recorder: &Arc<Recorder>,
    power: &std::path::Path,
) -> hocus_secretary::brain::Options {
    let script: Script = toml::from_str(script).expect("a script");
    let mut options = common::options(fixtures);
    options.model = Some(Arc::new(Fake::new(script, Some(recorder.clone()))));
    options.effects = Some(recorder.clone());
    // No battery: mains power.
    options.sensors = Sensors {
        supplies: power.join("power_supply"),
        thermal: power.join("thermal"),
        hwmon: power.join("hwmon"),
        nvidia_smi: power.join("no-nvidia-smi"),
    };
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

fn part<'a>(request: &'a Request, label: &str) -> &'a str {
    request
        .context
        .iter()
        .find(|p| p.label == label)
        .map(|p| p.text.as_str())
        .unwrap_or_default()
}

/// The first day of her clock, where the user is.
fn today() -> Date {
    common::START
        .parse::<Timestamp>()
        .expect("a time")
        .to_zoned(TimeZone::system())
        .date()
}

/// `day` at `hour:minute` where the user is.
fn local(day: Date, hour: i8, minute: i8) -> Timestamp {
    day.at(hour, minute, 0, 0)
        .to_zoned(TimeZone::system())
        .expect("a local time")
        .timestamp()
}

/// An event item of a fixture, `minutes` long.
fn event(
    id: &str,
    at: Timestamp,
    minutes: i64,
    title: &str,
    calendar: &str,
    extra: &str,
) -> String {
    let end = at
        .checked_add(jiff::SignedDuration::from_mins(minutes))
        .expect("an end");
    format!(
        "[[items]]\nid = \"{id}\"\nat = \"{at}\"\nkind = \"event\"\ndigest = \"{title}\"\n\
         [items.event]\nend = \"{end}\"\ncalendar = \"{calendar}\"\n{extra}\n"
    )
}

/// The user's shape (#762): their own calendar with a daily at 9:15 and
/// a meeting at 17:00, a meeting declined, one both on theirs and a
/// colleague's; 23 calendars shared with them, full; 80 mails today.
fn busy_day() -> Vec<hocus_secretary::sources::Fixture> {
    let today = today();
    let tomorrow = today.tomorrow().expect("tomorrow");
    let mut own = String::from("name = \"fixture-cal-own\"\nnotebook = \"pro\"\n");
    own.push_str(&event(
        "daily",
        local(today, 9, 15),
        15,
        "Daily",
        "Travail",
        "mine = true",
    ));
    own.push_str(&event(
        "client",
        local(today, 17, 0),
        60,
        "Point client",
        "Travail",
        "mine = true",
    ));
    own.push_str(&event(
        "refus",
        local(today, 11, 0),
        30,
        "Réunion refusée",
        "Travail",
        "mine = true\ndeclined = true",
    ));
    own.push_str(&event(
        "comite",
        local(today, 14, 0),
        60,
        "Comité produit",
        "Travail",
        "mine = true",
    ));
    own.push_str(&event(
        "demo",
        local(tomorrow, 10, 0),
        45,
        "Démo",
        "Travail",
        "mine = true",
    ));
    let mut shared = String::from("name = \"fixture-cal-shared\"\nnotebook = \"pro\"\n");
    shared.push_str(&event(
        "comite-copy",
        local(today, 14, 0),
        60,
        "Comité produit",
        "Salle Everest",
        "",
    ));
    for c in 0..23 {
        for n in 0..7 {
            let day = if n < 3 { today } else { tomorrow };
            let hour = 7 + i8::try_from((n * 2) % 12).expect("an hour");
            shared.push_str(&event(
                &format!("s{c}-{n}"),
                local(day, hour, 30),
                30,
                &format!("Réunion de l'équipe {c} numéro {n}"),
                &format!("Collègue {c}"),
                "",
            ));
        }
    }
    let start: Timestamp = common::START.parse().expect("a time");
    let mut mail = String::from("name = \"fixture-mail\"\nnotebook = \"pro\"\n");
    for m in 0..80 {
        let at = start
            .checked_sub(jiff::SignedDuration::from_mins(m + 1))
            .expect("a time");
        mail.push_str(&format!(
            "[[items]]\nid = \"m{m}\"\nat = \"{at}\"\nkind = \"mail\"\ndigest = \"Alerte n° {m}\"\n"
        ));
    }
    [own, shared, mail]
        .iter()
        .map(|f| common::fixture(f))
        .collect()
}

fn hm(at: Timestamp) -> String {
    let here = at.to_zoned(TimeZone::system());
    format!("{:02}:{:02}", here.hour(), here.minute())
}

/// AC1: every event of the user's day, theirs first when it is cut,
/// whatever the mails and the calendars shared with them; what is cut is
/// said with `agenda`.
#[test]
fn her_day_has_the_user_s_own_events_whatever_the_mails_and_shared_calendars() -> anyhow::Result<()>
{
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        options(
            busy_day(),
            "[default]\ntext = \"D'accord.\"\n",
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    driver.answer_to("Qu'est-ce que j'ai aujourd'hui ?");
    driver.stop();
    let request = calls(&recorder).pop().expect("a call");
    let day = part(&request, context::DAY);
    let today = today();
    let daily = format!(
        "- {}–{} · Daily · à toi (Travail)",
        hm(local(today, 9, 15)),
        hm(local(today, 9, 30))
    );
    let client = format!(
        "- {}–{} · Point client · à toi (Travail)",
        hm(local(today, 17, 0)),
        hm(local(today, 18, 0))
    );
    assert!(day.contains(&daily), "{day}");
    assert!(day.contains(&client), "{day}");
    assert!(
        day.contains("· Démo · à toi (Travail)"),
        "tomorrow's: {day}"
    );
    // The same meeting on a colleague's calendar: one line, the user's.
    assert_eq!(day.matches("Comité produit").count(), 1, "{day}");
    assert!(day.contains("· Comité produit · à toi (Travail)"), "{day}");
    assert!(!day.contains("Réunion refusée"), "declined: {day}");
    // Today's before tomorrow's, each under its day.
    let (at_today, at_tomorrow) = (
        day.find("Aujourd'hui, ").expect("today's heading"),
        day.find("Demain, ").expect("tomorrow's heading"),
    );
    assert!(
        at_today < day.find("Daily").expect("the daily") && day.find("Daily") < Some(at_tomorrow)
    );
    // Cut, and said so with the tool that gives the rest.
    let from = today.to_string();
    let to = today.tomorrow()?.to_string();
    assert!(
        day.contains(&format!(
            "de plus, des agendas partagés avec toi, non listés : `agenda` (from {from}, to {to})"
        )),
        "{day}"
    );
    // Some shared events are listed, and the mails still have their line.
    assert!(day.contains("agenda partagé « Collègue"), "{day}");
    assert!(day.contains("Arrivé aujourd'hui"), "{day}");
    assert!(day.contains("Alerte n° 0"), "the latest mail: {day}");
    assert!(context::tokens(day) <= context::DAY_CAP);
    assert!(request.tools.iter().any(|t| t == "agenda"));
    Ok(())
}

/// AC2: `agenda(from, to)` lists another day's events, read-only, within
/// what the moment allows.
#[test]
fn the_agenda_tool_lists_another_day_s_events() -> anyhow::Result<()> {
    let thursday = today().checked_add(jiff::ToSpan::days(3))?;
    let mut own = String::from("name = \"fixture-cal-own\"\nnotebook = \"pro\"\n");
    own.push_str(&event(
        "copil",
        local(thursday, 10, 0),
        90,
        "Comité de pilotage",
        "Travail",
        "mine = true",
    ));
    let mut perso = String::from("name = \"fixture-cal-perso\"\nnotebook = \"perso\"\n");
    perso.push_str(&event(
        "dentiste",
        local(thursday, 18, 0),
        30,
        "Dentiste",
        "Famille",
        "mine = true",
    ));
    let script = r#"
[[reply]]
contains = ["jeudi"]
text = "Je regarde."
[[reply.tool_calls]]
name = "agenda"
input = { from = "THURSDAY", to = "" }
[default]
text = "Bien."
"#
    .replace("THURSDAY", &thursday.to_string());
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        options(
            vec![common::fixture(&own), common::fixture(&perso)],
            &script,
            &recorder,
            power.path(),
        ),
    );
    driver.settle();
    driver.answer_to("Et jeudi, j'ai quoi ?");
    let store = driver.stop();
    // Its result came back to her model in the next round.
    let calls = calls(&recorder);
    let result = calls
        .iter()
        .flat_map(|r| r.messages.iter())
        .flat_map(|m| m.tool_results.iter())
        .map(|r| r.text.clone())
        .next()
        .expect("the tool's result");
    let heading = format!("{} :", hocus_secretary::agenda::long_date(thursday));
    assert!(result.contains(&heading), "{result}");
    assert!(
        result.contains(&format!(
            "- {}–{} · Comité de pilotage · à toi (Travail)",
            hm(local(thursday, 10, 0)),
            hm(local(thursday, 11, 30))
        )),
        "{result}"
    );
    assert!(
        result.contains("Dentiste"),
        "both notebooks when quiet: {result}"
    );
    // Read-only, as the moment allows: pro only in a meeting.
    let call = ToolCall {
        id: "1".to_owned(),
        name: "agenda".to_owned(),
        input: serde_json::json!({"from": thursday.to_string(), "to": ""}),
    };
    let pro = tools::run(&store, &call, Some(Notebook::Pro)).map_err(anyhow::Error::msg)?;
    assert!(
        pro.contains("Comité de pilotage") && !pro.contains("Dentiste"),
        "{pro}"
    );
    // Another day: nothing, said so.
    let empty = ToolCall {
        input: serde_json::json!({"from": "2026-12-01", "to": "2026-12-02"}),
        ..call.clone()
    };
    let none = tools::run(&store, &empty, None).map_err(anyhow::Error::msg)?;
    assert_eq!(none.matches("- rien à l'agenda").count(), 2, "{none}");
    // Too long a span, or backwards: said to her model as errors.
    let long = ToolCall {
        input: serde_json::json!({"from": "2026-11-01", "to": "2027-01-01"}),
        ..call.clone()
    };
    assert!(tools::run(&store, &long, None).is_err());
    let back = ToolCall {
        input: serde_json::json!({"from": "2026-11-12", "to": "2026-11-10"}),
        ..call
    };
    assert!(tools::run(&store, &back, None).is_err());
    Ok(())
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

/// A sentence, and the items she made of it until her answer's end.
fn ask(driver: &common::Driver, id: u64, text: &str) -> Vec<Item> {
    driver.send(&ToSecretary::Ask {
        id,
        text: text.to_owned(),
    });
    until(
        driver,
        |l| matches!(l, FromSecretary::Answer { id: i, done: true, .. } if *i == id),
    )
    .into_iter()
    .filter_map(|l| match l {
        FromSecretary::Item { item } => Some(item),
        _ => None,
    })
    .collect()
}

fn learned(items: &[Item]) -> Vec<(u64, String)> {
    items
        .iter()
        .filter_map(|i| match &i.body {
            Body::Learned { text, .. } => Some((i.id, text.clone())),
            _ => None,
        })
        .collect()
}

fn view(driver: &mut common::Driver, notebook: Option<Notebook>) -> Knowledge {
    match driver.request(|id| ToSecretary::Knows {
        id,
        query: Some(Query {
            notebook,
            ..Query::default()
        }),
    }) {
        FromSecretary::Knows { view, .. } => *view,
        other => panic!("no view: {other:?}"),
    }
}

fn projects(view: &Knowledge) -> Vec<String> {
    view.sections
        .iter()
        .filter(|s| s.key == "projects")
        .flat_map(|s| s.items.iter().map(|k| k.title.clone()))
        .collect()
}

const REMEMBER: &str = r#"
# The later turns' words first: the earlier ones are in their history.
[[reply]]
contains = ["résume mes mails"]
text = "Voici tes mails."
[[reply.tool_calls]]
name = "remember"
input = { said = "Atlas est un client", about = "Atlas", kind = "client", notebook = "pro", relation = "", object = "", object_kind = "", fact = "Atlas est un client" }
[[reply]]
contains = ["Léa Martin est ma voisine"]
text = "Noté."
[[reply.tool_calls]]
name = "remember"
input = { said = "Léa Martin est ma voisine", about = "Léa Martin", kind = "person", notebook = "perso", relation = "role", object = "voisine", object_kind = "", fact = "Léa Martin est ta voisine" }
[[reply]]
contains = ["projet perso"]
text = "C'est noté."
[[reply.tool_calls]]
name = "remember"
input = { said = "hocus-panda est un projet perso", about = "hocus-panda", kind = "project", notebook = "perso", relation = "", object = "", object_kind = "", fact = "hocus-panda est un projet perso" }
[default]
text = "Bien."
"#;

/// AC3: `remember` keeps what the user says at once, "Noté : …" with
/// "Annuler", in the notebook they said; it shows in "Ce qu'elle sait"
/// and in her next prompt; a source's words are never kept by it.
#[test]
fn what_the_user_tells_her_is_kept_in_its_notebook_and_shows_in_ce_qu_elle_sait()
-> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        options(Vec::new(), REMEMBER, &recorder, power.path()),
    );
    let said = ask(&driver, 1, "hocus-panda est un projet perso");
    let kept = learned(&said);
    assert_eq!(
        kept.iter().map(|(_, t)| t.as_str()).collect::<Vec<_>>(),
        ["Noté\u{a0}: hocus-panda est un projet perso"]
    );
    // In "Ce qu'elle sait", in the perso notebook only.
    assert!(projects(&view(&mut driver, None)).contains(&"hocus-panda".to_owned()));
    assert!(
        projects(&view(&mut driver, Some(Notebook::Perso))).contains(&"hocus-panda".to_owned())
    );
    assert!(!projects(&view(&mut driver, Some(Notebook::Pro))).contains(&"hocus-panda".to_owned()));
    let week = view(&mut driver, None);
    assert!(
        week.sections
            .iter()
            .filter(|s| s.key == "week")
            .flat_map(|s| s.items.iter())
            .any(|k| k.title == "hocus-panda"),
        "learned this week: {week:?}"
    );
    // Her next prompt knows it.
    ask(&driver, 2, "sur quoi je travaille ?");
    let call = calls(&recorder).pop().expect("a call");
    let card = part(&call, context::FROZEN);
    assert!(card.contains("Projets :\n- hocus-panda"), "{card}");
    // A fact on someone: an edge told, in its notebook.
    let voisine = ask(&driver, 3, "Léa Martin est ma voisine");
    assert_eq!(
        learned(&voisine)
            .iter()
            .map(|(_, t)| t.as_str())
            .collect::<Vec<_>>(),
        ["Noté\u{a0}: Léa Martin est ta voisine"]
    );
    let lea = match driver.request(|id| ToSecretary::Graph {
        id,
        query: GraphQuery {
            node: "Léa Martin".to_owned(),
            as_of: None,
            hops: 1,
            notebook: None,
        },
    }) {
        FromSecretary::Graph { graph, .. } => graph,
        other => panic!("no graph: {other:?}"),
    };
    let edge = lea
        .edges
        .iter()
        .find(|e| e.kind == "role")
        .expect("her role");
    assert_eq!(edge.provenance, hocus_secretary_link::Provenance::Told);
    assert_eq!(edge.notebook, Notebook::Perso);
    // "Annuler": gone.
    let (item, _) = learned(&voisine)[0].clone();
    driver.send(&ToSecretary::Undo { id: 4, item });
    until(&driver, |l| {
        matches!(
            l,
            FromSecretary::Answer {
                id: 4,
                done: true,
                ..
            }
        )
    });
    assert!(
        !view(&mut driver, None)
            .items()
            .any(|k| k.title == "Léa Martin")
    );
    // Her model led by a source's words: nothing kept.
    let led = ask(&driver, 5, "résume mes mails");
    assert!(learned(&led).is_empty(), "{led:?}");
    assert!(
        !projects(&view(&mut driver, None))
            .iter()
            .any(|p| p == "Atlas")
    );
    driver.stop();
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
                digest: "Léa : le point sur hocus-panda".to_owned(),
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

/// AC4: her graph stops being empty. A source never makes a node before
/// the user's "Oui" (§7.1), and she had none, so every fact of the
/// triage fell through; now what the user says makes nodes, and a mail's
/// fact linking someone she does not know to one of them is asked about,
/// then kept with its source.
#[test]
fn what_she_is_told_and_what_mails_say_reach_her_graph() -> anyhow::Result<()> {
    let power = tempfile::tempdir()?;
    let recorder = Arc::new(Recorder::in_memory());
    let mail = common::fixture(
        r#"
name = "fixture-mail"
notebook = "pro"
[[items]]
id = "m1"
at = "2026-11-09T09:00:00Z"
kind = "mail"
digest = "Le point"
sender = "Léa Martin <lea@atelier-lune.fr>"
body = "Je travaille sur hocus-panda cette semaine."
"#,
    );
    let mut options = options(vec![mail], REMEMBER, &recorder, power.path());
    options.triage = Some(Arc::new(Facts(vec![Fact {
        subject: "Léa Martin".to_owned(),
        relation: "travaille sur".to_owned(),
        object: "hocus-panda".to_owned(),
        quote: "Je travaille sur hocus-panda cette semaine".to_owned(),
    }])));
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, options);
    driver.settle();
    ask(&driver, 1, "hocus-panda est un projet perso");
    // The mail arrives, is triaged; Léa, unknown, is asked about.
    driver.advance(3600);
    let start = std::time::Instant::now();
    let seen = loop {
        let known = view(&mut driver, None);
        if let Some(k) = known.items().find(|k| {
            k.item.starts_with("seen:") && k.text.contains("Léa Martin travaille sur hocus-panda")
        }) {
            break k.clone();
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "never asked: {known:?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert!(
        seen.origin.text.contains("Je travaille sur hocus-panda"),
        "{seen:?}"
    );
    driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::Item {
            item: seen.item.clone(),
            act: Act::Accept,
        },
    });
    let lea = match driver.request(|id| ToSecretary::Graph {
        id,
        query: GraphQuery {
            node: "Léa Martin".to_owned(),
            as_of: None,
            hops: 1,
            notebook: None,
        },
    }) {
        FromSecretary::Graph { graph, .. } => graph,
        other => panic!("no graph: {other:?}"),
    };
    let edge = lea
        .edges
        .iter()
        .find(|e| e.kind == "works_on")
        .expect("Léa works on it");
    assert_eq!(edge.provenance, hocus_secretary_link::Provenance::Extracted);
    assert_eq!(edge.pointer, "fixture-mail:m1");
    let store = driver.stop();
    let all = store.export_json(None)?;
    assert_eq!(all["nodes"].as_array().map(Vec::len), Some(2));
    assert_eq!(all["edges"].as_array().map(Vec::len), Some(1));
    Ok(())
}

/// AC5: her persona names no tool she lacks, and tells her never to send
/// the user to a setting that does not exist.
#[test]
fn her_persona_names_only_the_tools_she_has_and_no_invented_setting() -> anyhow::Result<()> {
    let store = Store::in_memory(common::clock())?;
    let known: Vec<&str> = tools::READ
        .iter()
        .chain(tools::TALK.iter())
        .copied()
        .collect();
    for (language, never) in [
        (
            Language::French,
            "réglage, un outil ou une fonction qui n'existe pas",
        ),
        (
            Language::English,
            "setting, tool or feature that does not exist",
        ),
    ] {
        let persona = context::persona(&store, language);
        let named: Vec<&str> = persona
            .split('`')
            .skip(1)
            .step_by(2)
            .filter(|n| !n.ends_with('*'))
            .collect();
        assert!(!named.is_empty());
        for name in named {
            assert!(
                known.contains(&name),
                "{language:?}: `{name}` is no tool of hers"
            );
        }
        assert!(persona.contains(never), "{language:?}: {persona}");
        assert!(persona.contains("`agenda`") && persona.contains("`remember`"));
    }
    Ok(())
}
