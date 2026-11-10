//! Her first run and her sources' consent in the drawer (§10.1, §15,
//! #750), against a fake Online Accounts on a private `dbus-daemon` (two
//! Google accounts: a company's and a gmail.com one) and Google's services
//! faked on this machine: never the network, never the session's bus.
//!
//! - One account at a time (#758): a card per account, a switch per kind,
//!   the next only once the last is answered, her first run's steps saying
//!   where it is; then « Dis-moi sur quoi tu travailles, et avec qui. »,
//!   never asked again, a restart included.
//! - Pro or perso suggested by the account's domain, chosen on the card.
//! - "Autoriser" writes the grant in her consent book and her source
//!   reads it at once; "Pas maintenant" writes nothing; "Ne plus lire" a
//!   revocation.

use crate::common;

use crate::fakes;

use std::path::{Path, PathBuf};

use fakes::{FakeApi, FakeCardDav, FakeGoa, PrivateBus};
use hocus_secretary::{brain::Options, google::Google, store::Store};
use hocus_secretary_link::{
    Body, Card, CardKind, Decision, FromSecretary, Item, Notebook, ToSecretary,
    consent::{ConsentBook, SourceKey, SourceKind},
    goa::Imap,
};
use hocus_sources::cycle::Env;

const WORK: &str = "alex@atelier-lune.fr";
const PERSO: &str = "alex.martin@gmail.com";
const WORK_ID: &str = "account_1700000000_0";
const PERSO_ID: &str = "account_1700000000_1";

struct Bed {
    dir: tempfile::TempDir,
    api: FakeApi,
    carddav: FakeCardDav,
    goa: FakeGoa,
    bus: PrivateBus,
}

impl Bed {
    fn start() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let bus = PrivateBus::start(dir.path())?;
        let goa = FakeGoa::start(&bus.address)?;
        goa.add(WORK_ID, WORK)?;
        goa.add(PERSO_ID, PERSO)?;
        Ok(Self {
            api: FakeApi::start()?,
            carddav: FakeCardDav::start(Vec::new())?,
            goa,
            bus,
            dir,
        })
    }

    fn book(&self) -> PathBuf {
        self.dir.path().join("consents.json")
    }

    fn memory(&self) -> PathBuf {
        self.dir.path().join("memory.db")
    }

    fn options(&self) -> Options {
        let env = Env {
            bus: self.bus.address.clone(),
            calendar_api: hocus_day::google::api(Some(&self.api.address)),
            carddav_api: hocus_sources::contacts::api(Some(&self.carddav.address)),
            mail_text: true,
            // Nothing listens there: a mail's read fails at once.
            imap_override: Some(Imap {
                host: "127.0.0.1".to_owned(),
                port: 9,
                user: WORK.to_owned(),
                tls: false,
            }),
        };
        Options {
            sources: vec![Box::new(Google::new(
                self.bus.address.clone(),
                self.book(),
                env,
            ))],
            consents: Some(self.book()),
            ..common::options(Vec::new())
        }
    }
}

/// The thread now.
fn thread(driver: &mut common::Driver) -> Vec<Item> {
    match driver.request(|id| ToSecretary::Thread { id }) {
        FromSecretary::Thread { items, .. } => items,
        other => panic!("not her thread: {other:?}"),
    }
}

/// The thread once `done` holds.
fn thread_until(driver: &mut common::Driver, done: impl Fn(&[Item]) -> bool) -> Vec<Item> {
    let start = std::time::Instant::now();
    loop {
        let items = thread(driver);
        if done(&items) {
            return items;
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "never: {items:#?}"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

fn cards(items: &[Item]) -> Vec<(u64, Card)> {
    items
        .iter()
        .filter_map(|i| match &i.body {
            Body::Card { card } if card.kind == CardKind::Consent => Some((i.id, card.clone())),
            _ => None,
        })
        .collect()
}

/// The consent cards still waiting for the user.
fn waiting(items: &[Item]) -> Vec<(u64, Card)> {
    cards(items)
        .into_iter()
        .filter(|(_, c)| c.outcome.is_none())
        .collect()
}

/// Her first run's steps: each title, state and note.
fn steps(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .find_map(|i| match &i.body {
            Body::Steps { steps } => Some(
                steps
                    .iter()
                    .map(|s| {
                        let state = match s.state {
                            hocus_secretary_link::StepState::Done => "done",
                            hocus_secretary_link::StepState::Now => "now",
                            hocus_secretary_link::StepState::Later => "later",
                        };
                        format!("{} · {state} · {}", s.title, s.note)
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn hers(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .filter_map(|i| match &i.body {
            Body::Her { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn decide(driver: &mut common::Driver, item: u64, decision: Decision, notebook: Option<Notebook>) {
    decide_off(driver, item, decision, notebook, Vec::new());
}

/// A card's button, `off` its kinds switched off.
fn decide_off(
    driver: &mut common::Driver,
    item: u64,
    decision: Decision,
    notebook: Option<Notebook>,
    off: Vec<SourceKind>,
) {
    let answer = driver.request(|id| ToSecretary::Decide {
        id,
        item,
        decision,
        notebook,
        left_out: Vec::new(),
        off: off.clone(),
    });
    assert!(
        matches!(answer, FromSecretary::Answer { done: true, .. }),
        "{answer:?}"
    );
}

fn book(path: &Path) -> ConsentBook {
    ConsentBook::load(path)
}

#[test]
fn her_first_run_asks_one_source_at_a_time_then_what_the_user_works_on() -> anyhow::Result<()> {
    let bed = Bed::start()?;
    let key = [7u8; 32];
    let mut driver = common::Driver::start(
        Store::open(&bed.memory(), &key, common::clock())?,
        bed.options(),
    );
    let items = thread_until(&mut driver, |items| !waiting(items).is_empty());
    let said = hers(&items);
    assert!(
        said[0].contains("Je ne fais rien sans ton clic."),
        "she introduces herself: {said:?}"
    );
    // Her steps (#758): no key to give here, the accounts now.
    assert_eq!(
        steps(&items),
        [
            "Ce que je peux lire de tes comptes Google · now · 1 sur 2",
            "Sur quoi tu travailles, et avec qui · later · ",
        ]
    );
    let first = waiting(&items);
    assert_eq!(first.len(), 1, "one account at a time");
    let (card_id, card) = &first[0];
    assert_eq!(card.label, "Ton compte Google · 1 sur 2");
    let sheet = card.sheet.as_ref().expect("its sheet");
    assert_eq!(sheet.title, WORK);
    assert_eq!(sheet.from, "déjà connecté à cet ordinateur");
    assert_eq!(sheet.notebook, Notebook::Pro, "a company's domain: pro");
    let reads: Vec<(SourceKind, bool)> = sheet.reads.iter().map(|r| (r.kind, r.on)).collect();
    assert_eq!(
        reads,
        [
            (SourceKind::Calendar, true),
            (SourceKind::Mail, true),
            (SourceKind::Birthdays, false)
        ]
    );
    assert_eq!(card.actions[0].label, "Autoriser l’agenda et les mails");
    assert_eq!(
        sheet.then.as_ref().map(|t| t.identity.as_str()),
        Some(PERSO),
        "the next account announced"
    );
    assert_eq!(sheet.sections.len(), 3);
    assert!(
        sheet.sections[1]
            .lines
            .iter()
            .any(|l| l.contains("invitation"))
    );
    assert!(
        book(&bed.book()).records().is_empty(),
        "nothing before Autoriser"
    );
    let asked_before = bed.api.asked().len();

    // "Autoriser", pro, the birthdays off: two grants, her source reading
    // at once.
    decide_off(
        &mut driver,
        *card_id,
        Decision::Accept,
        Some(Notebook::Pro),
        vec![SourceKind::Birthdays],
    );
    let grant = book(&bed.book())
        .grant(&SourceKey::new(WORK_ID, SourceKind::Calendar))
        .expect("the calendar granted");
    assert_eq!(grant.notebook(), Notebook::Pro);
    assert_eq!(grant.identity(), WORK);
    assert!(
        book(&bed.book())
            .grant(&SourceKey::new(WORK_ID, SourceKind::Mail))
            .is_some(),
        "the mail granted"
    );
    assert!(
        book(&bed.book())
            .grant(&SourceKey::new(WORK_ID, SourceKind::Birthdays))
            .is_none(),
        "switched off: not granted"
    );
    let start = std::time::Instant::now();
    while bed.api.asked().len() == asked_before {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "the calendar is never read"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(bed.goa.given() > 0, "its token asked of Online Accounts");
    let items = thread(&mut driver);
    let granted = cards(&items)
        .into_iter()
        .find(|(i, _)| i == card_id)
        .map(|(_, c)| c)
        .expect("the card");
    assert_eq!(
        granted.outcome.as_deref(),
        Some("Je lis ton agenda et tes mails · travail")
    );
    assert_eq!(granted.actions.len(), 1);
    assert_eq!(granted.actions[0].label, "Ne plus lire");

    // The gmail.com account: perso suggested; "Pas maintenant" writes
    // nothing.
    let perso = waiting(&items);
    assert_eq!(perso.len(), 1);
    assert_eq!(perso[0].1.label, "Ton compte Google · 2 sur 2");
    let sheet = perso[0].1.sheet.as_ref().expect("its sheet");
    assert_eq!(sheet.title, PERSO);
    assert_eq!(sheet.notebook, Notebook::Perso);
    assert_eq!(sheet.private.choice[1], "Privé");
    assert!(sheet.then.is_none());
    decide(&mut driver, perso[0].0, Decision::Reject, None);
    assert_eq!(
        book(&bed.book()).records().len(),
        2,
        "Pas maintenant writes nothing"
    );
    let items = thread(&mut driver);
    assert!(waiting(&items).is_empty());
    assert_eq!(
        hers(&items).last().map(String::as_str),
        Some("Dis-moi sur quoi tu travailles, et avec qui.")
    );
    assert_eq!(
        steps(&items),
        [
            "Ce que je peux lire de tes comptes Google · done · ",
            "Sur quoi tu travailles, et avec qui · now · ",
        ]
    );

    // "Ne plus lire" on the work account: a revocation of both.
    decide(&mut driver, *card_id, Decision::Reject, None);
    assert!(
        book(&bed.book())
            .grant(&SourceKey::new(WORK_ID, SourceKind::Calendar))
            .is_none()
    );
    let mut revoked = book(&bed.book()).revoked();
    revoked.sort();
    assert_eq!(
        revoked,
        [
            SourceKey::new(WORK_ID, SourceKind::Calendar),
            SourceKey::new(WORK_ID, SourceKind::Mail)
        ]
    );
    let before = thread(&mut driver);
    drop(driver.stop());

    // Started again: her thread as it was, her first run over.
    let mut driver = common::Driver::start(
        Store::open(&bed.memory(), &key, common::clock())?,
        bed.options(),
    );
    driver.settle();
    let after = thread(&mut driver);
    assert_eq!(after.len(), before.len(), "her thread kept: {after:#?}");
    assert_eq!(after.last(), before.last());
    assert!(waiting(&after).is_empty(), "never asked again");
    driver.stop();
    Ok(())
}

#[test]
fn a_consent_given_in_the_settings_shows_on_her_card_at_once() -> anyhow::Result<()> {
    let bed = Bed::start()?;
    // Her name given (#752): she introduces herself by it.
    let named = hocus_secretary::settings::Settings {
        name: "Margot".to_owned(),
        ..Default::default()
    };
    let mut driver = common::Driver::start(
        Store::in_memory(common::clock())?,
        Options {
            settings: std::sync::Arc::new(hocus_secretary::settings::File::fixed(named)),
            ..bed.options()
        },
    );
    let items = thread_until(&mut driver, |items| !waiting(items).is_empty());
    assert!(
        hers(&items)[0].starts_with("Bonjour, je suis Margot, ta secrétaire."),
        "{:?}",
        hers(&items)
    );
    let (card_id, _) = waiting(&items)[0].clone();
    driver.settle();
    let asked_before = bed.api.asked().len();
    // The Settings' sheet writes the same book.
    let mut written = ConsentBook::default();
    written.allow(
        SourceKey::new(WORK_ID, SourceKind::Calendar),
        WORK,
        Notebook::Pro,
        jiff::Timestamp::UNIX_EPOCH,
    );
    written.save(&bed.book())?;
    // At once (#752), not at her source's next cycle: her clock does not
    // move.
    let items = thread_until(&mut driver, |items| {
        cards(items)
            .iter()
            .any(|(i, c)| *i == card_id && c.outcome.is_some())
    });
    let card = cards(&items)
        .into_iter()
        .find(|(i, _)| *i == card_id)
        .map(|(_, c)| c)
        .expect("the card");
    assert_eq!(
        card.outcome.as_deref(),
        Some("Je lis ton agenda · travail (autorisé dans les Réglages)")
    );
    // And the first run went on to the next source.
    assert_eq!(waiting(&items).len(), 1);
    // Her source read the calendar at once too.
    let start = std::time::Instant::now();
    while bed.api.asked().len() == asked_before {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(20),
            "the calendar is never read"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    driver.stop();
    Ok(())
}

/// #752: the Agenda's card lists the calendars she will read, those
/// others share said so; one left out on it is never read; one that
/// cannot be read is said in her status (« 1 agenda illisible »), the
/// others read; the list is kept in her folder for the Settings.
#[test]
fn the_agenda_s_card_lists_its_calendars_and_one_unreadable_is_said() -> anyhow::Result<()> {
    use fakes::day::{calendar, event};
    let bed = Bed::start()?;
    bed.api.with(|api| {
        let mut own = calendar(WORK, "Travail", "#4285f4", "owner", true);
        own["primary"] = serde_json::json!(true);
        let mut design = calendar(
            "design@group.calendar.google.com",
            "Équipe design",
            "#f6bf26",
            "reader",
            true,
        );
        design["dataOwner"] = serde_json::json!("lea@atelier-lune.fr");
        let mut famille = calendar(
            "famille@group.calendar.google.com",
            "Famille",
            "#33b679",
            "reader",
            false,
        );
        famille["hidden"] = serde_json::json!(true);
        let broken = calendar(
            "salles@group.calendar.google.com",
            "Salles",
            "#616161",
            "reader",
            true,
        );
        api.calendars = vec![own, design, famille, broken];
        api.events.insert(
            WORK.to_owned(),
            (
                vec![event("e1", "Point Chapitô", "10:00", "11:00")],
                "s-w".to_owned(),
            ),
        );
        api.events.insert(
            "design@group.calendar.google.com".to_owned(),
            (
                vec![event("d1", "Revue", "14:00", "15:00")],
                "s-d".to_owned(),
            ),
        );
        api.events.insert(
            "famille@group.calendar.google.com".to_owned(),
            (
                vec![event("f1", "Kiné", "18:00", "19:00")],
                "s-f".to_owned(),
            ),
        );
    });
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, bed.options());
    let items = thread_until(&mut driver, |items| {
        waiting(items)
            .first()
            .and_then(|(_, c)| c.sheet.as_ref())
            .is_some_and(|s| s.calendars.len() == 4)
    });
    let (card_id, card) = waiting(&items)[0].clone();
    let sheet = card.sheet.expect("its sheet");
    let notes: Vec<String> = sheet
        .calendars
        .iter()
        .map(|c| format!("{} {}", c.name, c.note(true)).trim().to_owned())
        .collect();
    assert_eq!(
        notes,
        [
            "Travail",
            "Équipe design partagé par lea@atelier-lune.fr",
            "Famille partagé · masqué",
            "Salles partagé",
        ]
    );
    // Listed before the consent, nothing of them read.
    assert!(
        bed.api.asked().iter().all(|a| !a.path.ends_with("/events")),
        "no event before Autoriser"
    );
    // "Autoriser", Famille left out.
    let answer = driver.request(|id| ToSecretary::Decide {
        id,
        item: card_id,
        decision: Decision::Accept,
        notebook: Some(Notebook::Pro),
        left_out: vec!["famille@group.calendar.google.com".to_owned()],
        off: Vec::new(),
    });
    assert!(matches!(answer, FromSecretary::Answer { done: true, .. }));
    let grant = book(&bed.book())
        .grant(&SourceKey::new(WORK_ID, SourceKind::Calendar))
        .expect("granted");
    assert_eq!(grant.left_out(), ["famille@group.calendar.google.com"]);
    // Her status: one calendar she could not read; the others are.
    let look = loop {
        if let FromSecretary::Look { look } = driver.receive()
            && look.status.contains("illisible")
        {
            break look;
        }
    };
    assert!(
        look.status.ends_with("1 agenda illisible"),
        "{}",
        look.status
    );
    assert!(look.hint.contains("Salles"), "{}", look.hint);
    let asked = bed.api.asked();
    assert!(
        asked
            .iter()
            .any(|a| a.path == format!("/calendars/{WORK}/events"))
    );
    assert!(
        asked
            .iter()
            .any(|a| a.path == "/calendars/design@group.calendar.google.com/events"),
        "a shared calendar read"
    );
    assert!(
        !asked.iter().any(|a| a.path.contains("famille")),
        "Famille left out: never read"
    );
    let kept = hocus_secretary_link::calendars::Calendars::load(
        &bed.dir
            .path()
            .join(hocus_secretary_link::calendars::CALENDARS),
    );
    let salles = kept
        .of(WORK_ID)
        .iter()
        .find(|c| c.name == "Salles")
        .expect("kept for the Settings");
    assert!(salles.unreadable.is_some());
    driver.stop();
    Ok(())
}

/// #752: an account added to Online Accounts after her first run is
/// asked about in her drawer: she says she found it, then one source at
/// a time, as at her first run; the accounts asked about then are not
/// asked again.
#[test]
fn an_account_added_later_is_asked_about_in_the_drawer() -> anyhow::Result<()> {
    const ASSO: &str = "alex.asso@gmail.com";
    let bed = Bed::start()?;
    let mut driver = common::Driver::start(Store::in_memory(common::clock())?, bed.options());
    for _ in 0..2 {
        let items = thread_until(&mut driver, |items| !waiting(items).is_empty());
        decide(&mut driver, waiting(&items)[0].0, Decision::Reject, None);
    }
    let items = thread(&mut driver);
    assert!(waiting(&items).is_empty());
    assert_eq!(
        hers(&items).last().map(String::as_str),
        Some("Dis-moi sur quoi tu travailles, et avec qui.")
    );
    bed.goa.add("account_1700000000_2", ASSO)?;
    // At her source's next cycle, the account is found.
    driver.advance(300);
    let items = thread_until(&mut driver, |items| !waiting(items).is_empty());
    assert!(
        hers(&items).last().is_some_and(
            |t| t.starts_with("J’ai trouvé un nouveau compte Google : alex.asso@gmail.com.")
        ),
        "{:?}",
        hers(&items)
    );
    let new = waiting(&items);
    assert_eq!(new.len(), 1, "one at a time");
    assert_eq!(
        new[0].1.sheet.as_ref().map(|s| s.title.as_str()),
        Some(ASSO)
    );
    decide(&mut driver, new[0].0, Decision::Reject, None);
    let items = thread(&mut driver);
    assert!(waiting(&items).is_empty(), "one card for the account");
    assert_eq!(
        cards(&items)
            .iter()
            .filter(|(_, c)| c.sheet.as_ref().is_some_and(|s| s.title == WORK))
            .count(),
        1,
        "the accounts asked about before are not asked again"
    );
    driver.stop();
    Ok(())
}
