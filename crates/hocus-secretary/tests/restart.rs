//! What survives her restart (#750), in her encrypted memory:
//!
//! - her thread, its perso items forgotten with "Oublier tout le perso";
//!   a card's proposal with it, so that its button still does what it
//!   said;
//! - the items not triaged yet, with their text after the first pass,
//!   until the local model checked them; meanwhile only their metadata
//!   leaves, and her status says the triage does not answer.

mod common;

use std::sync::Arc;

use hocus_secretary::{
    model::{Fake, Recorded, Recorder, Script},
    store::Store,
    triage::{Triager, Verdict},
};
use hocus_secretary_link::{Body, Decision, FromSecretary, Item, Notebook, ToSecretary, knows::Do};

const MAIL: &str = r#"
name = "fixture-mail"
notebook = "pro"
every = 3600
[[items]]
id = "free"
at = "2026-11-09T07:00:00Z"
kind = "mail"
digest = "Free : vos accès"
sender = "Free <assistance@free.example>"
body = "Pour l'immeuble, entre 4417 sur le clavier en bas et pousse fort."
"#;

/// A local model that does not answer (Ollama not running).
#[derive(Debug)]
struct Down;

impl Triager for Down {
    fn endpoint(&self) -> (String, String) {
        ("local".to_owned(), "qwen3:8b".to_owned())
    }

    fn triage(&self, _input: &str) -> Result<(Verdict, u64, u64), String> {
        Err("http://127.0.0.1:11434: connection refused".to_owned())
    }

    fn unload(&self) -> Result<(), String> {
        Ok(())
    }
}

/// The machine on mains, cool: the triage never waits for power here.
fn mains(dir: &std::path::Path) -> hocus_secretary::power::Sensors {
    let supplies = dir.join("power_supply");
    std::fs::create_dir_all(supplies.join("AC")).expect("a folder");
    std::fs::write(supplies.join("AC/type"), "Mains").expect("written");
    std::fs::write(supplies.join("AC/online"), "1").expect("written");
    hocus_secretary::power::Sensors {
        supplies,
        thermal: dir.join("thermal"),
    }
}

fn thread(driver: &mut common::Driver) -> Vec<Item> {
    match driver.request(|id| ToSecretary::Thread { id }) {
        FromSecretary::Thread { items, .. } => items,
        other => panic!("not her thread: {other:?}"),
    }
}

#[test]
fn items_waiting_for_the_triage_keep_their_text_across_a_restart() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("memory.db");
    let key = [5u8; 32];
    let mut options = common::options(vec![common::fixture(MAIL)]);
    options.triage = Some(Arc::new(Down));
    options.sensors = mains(dir.path());
    let mut driver = common::Driver::start(Store::open(&file, &key, common::clock())?, options);
    // Her status says it, plainly, and what leaves meanwhile.
    let look = loop {
        if let FromSecretary::Look { look } = driver.receive()
            && look.status.contains("ne répond pas")
        {
            break look;
        }
    };
    assert_eq!(
        look.status,
        "Le tri sur cet ordinateur ne répond pas · 1 en attente"
    );
    assert!(look.hint.contains("seuls l’expéditeur, l’objet et la date"));
    let status = driver.status();
    let triage = status.triage.clone().expect("her triage");
    assert!(triage.state.starts_with("paused"), "{triage:?}");
    assert_eq!(triage.pending, 1);
    assert_eq!(status.counts.episodes, 1);
    let store = driver.stop();
    let pending = hocus_secretary::kept::waiting(&store)?;
    assert_eq!(pending.len(), 1, "its text kept until checked");
    drop(store);
    let bytes = std::fs::read(&file)?;
    let plain = |word: &str| bytes.windows(word.len()).any(|w| w == word.as_bytes());
    assert!(!plain("pousse fort"), "kept encrypted");

    // Started again, the local model up: the item is checked, its text
    // leaves masked, and is no longer kept.
    let script: Script = toml::from_str(
        r#"
        [default]
        text = "Free te donne le code de l'immeuble : je l'ai masqué."
        [[triage]]
        contains = ["clavier"]
        verdict = { for_me = true, needs_reply = false, urgency = "low", digest = "Free : l'accès à l'immeuble", secrets = [{ text = "4417", kind = "door-code" }] }
        "#,
    )?;
    let recorder = Arc::new(Recorder::in_memory());
    let fake = Arc::new(Fake::new(script, Some(recorder.clone())));
    let mut options = common::options(vec![common::fixture(MAIL)]);
    options.model = Some(fake.clone());
    options.triage = Some(fake);
    options.sensors = mains(dir.path());
    let later = hocus_secretary::clock::Clock::virtual_at("2026-11-09T09:00:00Z".parse()?);
    let mut driver = common::Driver::start(Store::open(&file, &key, later)?, options);
    let status = driver.until("checked", |s| {
        s.triage.as_ref().is_some_and(|t| t.done == 1)
    });
    assert_eq!(status.counts.episodes, 1, "the mail not read twice");
    driver.answer_to("Que dit le mail de Free ?");
    let asked = recorder
        .lines()
        .into_iter()
        .rev()
        .filter_map(|l| match l {
            Recorded::Call { request, .. }
                if request.role == hocus_secretary::model::Role::Answer =>
            {
                Some(request.text())
            }
            _ => None,
        })
        .next()
        .expect("her answer's request");
    assert!(
        asked.contains("pousse fort"),
        "its text, once checked: {asked}"
    );
    assert!(!asked.contains("4417"), "never its code: {asked}");
    let store = driver.stop();
    assert!(
        hocus_secretary::kept::waiting(&store)?.is_empty(),
        "checked: its text is no longer kept"
    );
    Ok(())
}

#[test]
fn her_thread_survives_a_restart_and_its_perso_goes_with_oublier_tout_le_perso()
-> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let file = dir.path().join("memory.db");
    let key = [9u8; 32];
    let recorder = Arc::new(Recorder::in_memory());
    let options = |recorder: &Arc<Recorder>| {
        let script: Script = toml::from_str(
            r#"
            [[reply]]
            contains = ["recette"]
            text = "Je te prépare un message."
            [[reply.tool_calls]]
            name = "send_message"
            input = { to = "Léa", draft = "Je valide la recette jeudi." }
            [[reply]]
            contains = ["anniversaire"]
            text = "Je te le rappelle."
            [[reply.tool_calls]]
            name = "add_task"
            input = { lines = ["Cadeau de Claire"], notebook = "perso" }
            "#,
        )
        .expect("a script");
        let mut options = common::options(Vec::new());
        options.model = Some(Arc::new(Fake::new(script, Some(recorder.clone()))));
        options.effects = Some(recorder.clone());
        options
    };
    let mut driver = common::Driver::start(
        Store::open(&file, &key, common::clock())?,
        options(&recorder),
    );
    driver.answer_to("l'anniversaire de Claire samedi");
    driver.answer_to("réponds à Léa pour la recette");
    let before = thread(&mut driver);
    let card = |items: &[Item], perso: bool| {
        items
            .iter()
            .find(|i| {
                matches!(i.body, Body::Card { .. })
                    && (i.notebook == Some(Notebook::Perso)) == perso
            })
            .map(|i| i.id)
    };
    let message = card(&before, false).expect("the message's card");
    assert!(card(&before, true).is_some(), "the perso task's card");
    drop(driver.stop());

    // Started again: the same thread; its card still sends what it said.
    let recorder = Arc::new(Recorder::in_memory());
    let mut driver = common::Driver::start(
        Store::open(&file, &key, common::clock())?,
        options(&recorder),
    );
    let after = thread(&mut driver);
    assert_eq!(after, before, "her thread as it was");
    let answer = driver.request(|id| ToSecretary::Decide {
        id,
        item: message,
        decision: Decision::Accept,
        notebook: None,
    });
    assert!(
        matches!(answer, FromSecretary::Answer { done: true, .. }),
        "{answer:?}"
    );
    let effects: Vec<String> = recorder
        .lines()
        .into_iter()
        .filter_map(|l| match l {
            Recorded::Effect { kind, .. } => Some(kind),
            _ => None,
        })
        .collect();
    assert_eq!(effects, ["message"], "its effect, after the restart");
    // A new sentence follows on from the thread kept.
    let id = thread(&mut driver).last().map_or(0, |i| i.id);
    assert!(id >= before.last().map_or(0, |i| i.id));

    // "Oublier tout le perso": its thread items go, now and after a restart.
    driver.request(|id| ToSecretary::Memory {
        id,
        act: Do::ForgetNotebook {
            notebook: Notebook::Perso,
        },
    });
    drop(driver.stop());
    let mut driver = common::Driver::start(
        Store::open(&file, &key, common::clock())?,
        options(&recorder),
    );
    let kept = thread(&mut driver);
    assert!(card(&kept, true).is_none(), "the perso card forgotten");
    assert!(card(&kept, false).is_some(), "the work one kept");
    driver.stop();
    Ok(())
}
