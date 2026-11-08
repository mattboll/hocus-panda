//! The secretary's Google sources end to end (lot 3): Online Accounts on
//! a private bus with a work and a personal account, the Calendar API,
//! CardDAV and IMAP faked on 127.0.0.1, read through [`Sources::cycle`].
//! Never the network, never the session's bus. The clock is Tuesday 10
//! November 2026, 08:00 UTC.
//!
//! AC14 (a holiday is a computed fact), AC17 (consent before any read,
//! revocation purges), AC18 (secrets masked before any model), AC19
//! (attachments and tokens never leave), AC43 (identity and bank numbers
//! masked, health and family words kept).

mod fakes;

use std::collections::BTreeMap;

use fakes::{
    FakeApi, FakeCardDav, FakeGoa, PrivateBus, TOKEN,
    day::calendar,
    imap::{FakeImap, Mailbox, Message, message, with_attachment},
};
use hocus_sources::{
    ConsentBook, Content, Notebook, Redactor, SourceKey, SourceKind,
    cycle::{Cursors, Env, Event, Sources},
    facts::{self, Place, Region, SchoolCalendar},
    goa::{self, Imap},
    redact::{Check, NOT_CHECKED, Outbound, SecondPass},
};
use jiff::{Zoned, civil::date, tz::TimeZone};
use serde_json::json;

const WORK: &str = "alex@atelier-lune.fr";
const PERSO: &str = "alex.martin@gmail.com";

fn now() -> Zoned {
    date(2026, 11, 10)
        .at(8, 0, 0, 0)
        .to_zoned(TimeZone::UTC)
        .unwrap_or_else(|_| Zoned::now())
}

struct Bed {
    _dir: tempfile::TempDir,
    sources: Sources,
    consents: ConsentBook,
    imap: FakeImap,
    api: FakeApi,
    carddav: FakeCardDav,
    goa: FakeGoa,
    _bus: PrivateBus,
}

impl Bed {
    fn accounts(&self) -> Vec<goa::GoogleAccount> {
        goa::discover(&self.sources.env.bus).expect("Online Accounts answers")
    }

    fn cycle(&mut self) -> Vec<Event> {
        let accounts = self.accounts();
        self.sources.cycle(&accounts, &self.consents, &now())
    }

    fn allow(&mut self, account: &str, identity: &str, kind: SourceKind, notebook: Notebook) {
        self.consents.allow(
            SourceKey::new(account, kind),
            identity,
            notebook,
            now().timestamp(),
        );
    }
}

fn inbox(messages: Vec<Message>) -> Vec<Mailbox> {
    vec![
        Mailbox {
            name: "INBOX".to_owned(),
            special: None,
            uidvalidity: 3,
            messages,
        },
        Mailbox {
            name: "[Gmail]/Sent Mail".to_owned(),
            special: Some("\\Sent".to_owned()),
            uidvalidity: 4,
            messages: Vec::new(),
        },
    ]
}

fn mail(uid: u32, raw: Vec<u8>) -> Message {
    Message {
        uid,
        flags: Vec::new(),
        internal: "05-Nov-2026 10:12:00 +0100".to_owned(),
        raw,
        gmail_id: 1_780_000_000_000_000_000 + u64::from(uid),
    }
}

fn start(messages: Vec<Message>) -> anyhow::Result<Bed> {
    let dir = tempfile::tempdir()?;
    let bus = PrivateBus::start(dir.path())?;
    let goa = FakeGoa::start(&bus.address)?;
    goa.add("account_1700000000_0", WORK)?;
    goa.add("account_1700000000_1", PERSO)?;
    let api = FakeApi::start()?;
    api.with(|api| {
        let mut hidden = calendar(
            "conges@group.calendar.google.com",
            "Congés",
            "#33b679",
            "reader",
            false,
        );
        hidden["hidden"] = json!(true);
        api.calendars = vec![calendar(WORK, "Travail", "#4285f4", "owner", true), hidden];
        let event = |id: &str, title: &str, day: &str, from: &str, to: &str| {
            json!({
                "id": id, "status": "confirmed", "summary": title,
                "start": {"dateTime": format!("2026-11-{day}T{from}:00Z")},
                "end": {"dateTime": format!("2026-11-{day}T{to}:00Z")},
            })
        };
        api.events.insert(
            WORK.to_owned(),
            (
                vec![event("e1", "Point Chapitô", "12", "10:00", "11:30")],
                "sync-work-1".to_owned(),
            ),
        );
        api.events.insert(
            "conges@group.calendar.google.com".to_owned(),
            (
                vec![json!({
                    "id": "c1", "status": "confirmed", "summary": "Sophie en congés",
                    "transparency": "transparent",
                    "start": {"date": "2026-11-12"}, "end": {"date": "2026-11-14"},
                })],
                "sync-conges-1".to_owned(),
            ),
        );
    });
    let carddav = FakeCardDav::start(vec![(
        "/carddav/v1/principals/alex.martin@gmail.com/lists/default/claire.vcf".to_owned(),
        "BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Claire Martin\r\nTEL:+33612345678\r\nBDAY:1990-03-02\r\nNOTE:digicode 12A45\r\nEND:VCARD\r\n".to_owned(),
    )])?;
    let imap = FakeImap::start(inbox(messages))?;
    let env = Env {
        bus: bus.address.clone(),
        calendar_api: hocus_day::google::api(Some(&api.address)),
        carddav_api: hocus_sources::contacts::api(Some(&carddav.address)),
        mail_text: true,
        imap_override: Some(Imap {
            host: "127.0.0.1".to_owned(),
            port: imap.port,
            user: WORK.to_owned(),
            tls: false,
        }),
    };
    Ok(Bed {
        _dir: dir,
        sources: Sources::new(env, Cursors::default()),
        consents: ConsentBook::default(),
        imap,
        api,
        carddav,
        goa,
        _bus: bus,
    })
}

fn items(events: &[Event]) -> Vec<&hocus_sources::Redacted> {
    events
        .iter()
        .flat_map(|e| match e {
            Event::Items { items, .. } => items.iter().collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn every_google_account_is_found_each_with_its_notebook_suggested() -> anyhow::Result<()> {
    let bed = start(Vec::new())?;
    let accounts = bed.accounts();
    let found: Vec<(&str, &str, Notebook)> = accounts
        .iter()
        .map(|a| (a.id.as_str(), a.identity.as_str(), a.suggested))
        .collect();
    assert_eq!(
        found,
        [
            ("account_1700000000_0", WORK, Notebook::Pro),
            ("account_1700000000_1", PERSO, Notebook::Perso),
        ]
    );
    assert_eq!(accounts[0].imap.host, "imap.gmail.com");
    assert_eq!(accounts[0].kinds(), SourceKind::ALL);
    let sheet = goa::sheet(&accounts[1], SourceKind::Mail);
    assert_eq!(sheet.ask_notebook, Some("C'est ton compte perso ?"));
    assert!(sheet.never.contains("ne marque rien comme lu"));
    // Discovering asks no token.
    assert_eq!(bed.goa.given(), 0);
    Ok(())
}

#[test]
fn no_source_is_read_before_its_consent_and_a_revoked_one_is_purged() -> anyhow::Result<()> {
    let raw = message(
        PERSO,
        PERSO,
        "Photos de Noël",
        "Thu, 05 Nov 2026 10:12:00 +0100",
        "Les photos sont en ligne.",
    );
    let mut bed = start(vec![mail(4, raw)])?;
    // Nothing allowed: nothing read, not even a token asked.
    let events = bed.cycle();
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(bed.imap.with(|s| s.connections), 0);
    assert!(bed.api.asked().is_empty());
    assert!(bed.carddav.asked().is_empty());
    assert_eq!(bed.goa.given(), 0);

    // The personal account's mail, into the perso notebook.
    let mail_source = SourceKey::new("account_1700000000_1", SourceKind::Mail);
    bed.allow(
        "account_1700000000_1",
        PERSO,
        SourceKind::Mail,
        Notebook::Perso,
    );
    let events = bed.cycle();
    let read = items(&events);
    assert_eq!(read.len(), 1);
    assert_eq!(read[0].item.notebook, Notebook::Perso);
    assert_eq!(read[0].item.pointer.source, mail_source);
    assert_eq!(bed.imap.with(|s| s.connections), 1);
    // Only that source: no calendar, no contacts.
    assert!(bed.api.asked().is_empty());
    assert!(bed.carddav.asked().is_empty());

    // What the process keeps of it (its digests), by source.
    let mut digests: BTreeMap<SourceKey, Vec<String>> = BTreeMap::new();
    let keep = |events: Vec<Event>, digests: &mut BTreeMap<SourceKey, Vec<String>>| {
        for event in events {
            match event {
                Event::Items { source, items } => digests
                    .entry(source)
                    .or_default()
                    .extend(items.into_iter().map(|i| i.item.pointer.id)),
                Event::Purge(source) => {
                    digests.remove(&source);
                }
                _ => {}
            }
        }
    };
    keep(events, &mut digests);
    assert_eq!(digests[&mail_source], ["INBOX/3/4"]);

    // Revoked: the next cycle does not read it and says to purge it, once.
    bed.consents.revoke(&mail_source, now().timestamp());
    let events = bed.cycle();
    assert!(
        matches!(events.as_slice(), [Event::Purge(source)] if *source == mail_source),
        "{events:?}"
    );
    keep(events, &mut digests);
    assert!(digests.is_empty());
    assert_eq!(bed.imap.with(|s| s.connections), 1);
    assert!(bed.cycle().is_empty());
    assert_eq!(bed.imap.with(|s| s.connections), 1);
    Ok(())
}

#[test]
fn every_calendar_is_read_hidden_ones_too() -> anyhow::Result<()> {
    let mut bed = start(Vec::new())?;
    bed.allow(
        "account_1700000000_0",
        WORK,
        SourceKind::Calendar,
        Notebook::Pro,
    );
    let events = bed.cycle();
    let mut read: Vec<(String, bool)> = items(&events)
        .iter()
        .filter_map(|r| match &r.item.content {
            Content::Event(e) => Some((e.title.clone(), e.hidden_calendar)),
            _ => None,
        })
        .collect();
    read.sort();
    assert_eq!(
        read,
        [
            ("Point Chapitô".to_owned(), false),
            ("Sophie en congés".to_owned(), true)
        ]
    );
    let asked = bed.api.asked();
    let list = asked
        .iter()
        .find(|a| a.path == "/users/me/calendarList")
        .expect("the calendars listed");
    assert_eq!(
        list.query.get("showHidden").map(String::as_str),
        Some("true")
    );
    assert!(asked.iter().all(|a| a.token.starts_with(TOKEN)));

    // The next cycle asks what changed since each calendar's token.
    bed.api.with(|api| {
        api.syncs.insert(
            "sync-work-1".to_owned(),
            fakes::day::Sync::Changes {
                items: vec![json!({"id": "e1", "status": "cancelled"})],
                next: "sync-work-2".to_owned(),
            },
        );
        api.syncs.insert(
            "sync-conges-1".to_owned(),
            fakes::day::Sync::Changes {
                items: Vec::new(),
                next: "sync-conges-2".to_owned(),
            },
        );
    });
    let events = bed.cycle();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::Removed { ids, .. } if ids == &[format!("{WORK}/e1")])),
        "{events:?}"
    );
    Ok(())
}

#[test]
fn birthdays_are_read_once_a_day_and_nothing_else_of_the_cards() -> anyhow::Result<()> {
    let mut bed = start(Vec::new())?;
    bed.allow(
        "account_1700000000_1",
        PERSO,
        SourceKind::Birthdays,
        Notebook::Perso,
    );
    let events = bed.cycle();
    let read = items(&events);
    assert_eq!(read.len(), 1);
    match &read[0].item.content {
        Content::Birthday(b) => {
            assert_eq!(
                (b.name.as_str(), b.month, b.day, b.year),
                ("Claire Martin", 3, 2, Some(1990))
            );
        }
        other => panic!("not a birthday: {other:?}"),
    }
    let kept = serde_json::to_string(&read[0])?;
    assert!(
        !kept.contains("12A45") && !kept.contains("+33612345678"),
        "{kept}"
    );
    assert_eq!(bed.carddav.asked().len(), 1);
    assert!(
        bed.carddav.asked()[0]
            .starts_with("REPORT /principals/alex.martin@gmail.com/lists/default/")
    );
    assert!(bed.cycle().is_empty());
    assert_eq!(bed.carddav.asked().len(), 1);
    Ok(())
}

/// The second pass, as lot 4's local model will run it: here it finds
/// nothing more.
struct NothingMore;

impl SecondPass for NothingMore {
    fn secrets(&mut self, _text: &str) -> Result<Vec<std::ops::Range<usize>>, String> {
        Ok(Vec::new())
    }
}

#[test]
fn a_mails_secrets_are_masked_before_any_model_and_it_stays_closed_until_checked()
-> anyhow::Result<()> {
    // The token split, so that a forge's secret scanning does not take
    // it for a real one.
    const API_TOKEN: &str = concat!("gh", "p_2isAjIhKtJ0RlgLKOmxgJTeKdNnFRIBXuDL7");
    let body = format!(
        "Bonjour,\r\n\r\nVotre accès au serveur est prêt.\r\nIdentifiant : alex\r\nMot de passe : Ch@ton77\r\nJeton d'API : {API_TOKEN}\r\nVotre code de vérification est 482913.\r\n\r\nL'équipe Hébergeur\r\n"
    );
    let raw = message(
        "Hébergeur <support@hebergeur.example>",
        WORK,
        "Vos accès",
        "Thu, 05 Nov 2026 10:12:00 +0100",
        &body,
    );
    let mut bed = start(vec![mail(6, raw)])?;
    bed.allow(
        "account_1700000000_0",
        WORK,
        SourceKind::Mail,
        Notebook::Pro,
    );
    let events = bed.cycle();
    let read = items(&events);
    assert_eq!(read.len(), 1);
    let mut redacted = read[0].clone();
    let secrets = ["Ch@ton77", API_TOKEN, "482913"];
    // What the triage model would be given: the redacted text.
    let Content::Mail(mail) = &redacted.item.content else {
        panic!("not a mail");
    };
    let text = mail.body.clone().unwrap_or_default();
    for secret in secrets {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    assert!(
        text.contains("[secret masqué — voir le mail de Hébergeur, 5 nov.]"),
        "{text}"
    );
    assert!(redacted.masks.len() >= 3);
    assert_eq!(
        redacted.item.pointer.open.as_deref(),
        Some("https://mail.google.com/mail/u/alex@atelier-lune.fr/#all/18b3d491b4320006")
    );
    // Fail closed: until the second pass, only its metadata may leave.
    assert_eq!(redacted.check, Check::FirstPass);
    let outbound = serde_json::to_string(&redacted.outbound())?;
    assert!(
        matches!(redacted.outbound(), Outbound::MetadataOnly { note, .. } if note == NOT_CHECKED)
    );
    assert!(!outbound.contains("Votre accès au serveur"), "{outbound}");
    redacted
        .second_pass(&mut NothingMore)
        .map_err(anyhow::Error::msg)?;
    assert_eq!(redacted.check, Check::Done);
    let outbound = serde_json::to_string(&redacted.outbound())?;
    assert!(outbound.contains("Votre accès au serveur"));
    for secret in secrets {
        assert!(!outbound.contains(secret), "{secret} leaves: {outbound}");
    }
    assert_eq!(bed.imap.violations(), Vec::<String>::new());
    Ok(())
}

#[test]
fn attachments_and_tokens_never_leave() -> anyhow::Result<()> {
    let first_token = format!("{TOKEN}-1");
    let attached = with_attachment(
        "Comptable <compta@example.org>",
        "Facture octobre",
        "Thu, 05 Nov 2026 10:12:00 +0100",
        &format!("Ci-joint la facture. (debug: {first_token})"),
        "facture-octobre.pdf",
        "CONTENU-DE-LA-PIECE-JOINTE",
    );
    let mut bed = start(vec![mail(7, attached)])?;
    bed.allow(
        "account_1700000000_0",
        WORK,
        SourceKind::Mail,
        Notebook::Pro,
    );
    let events = bed.cycle();
    let read = items(&events);
    assert_eq!(read.len(), 1);
    let all = serde_json::to_string(&read)?;
    assert!(all.contains("facture-octobre.pdf"), "its name is kept");
    assert!(!all.contains("CONTENU-DE-LA-PIECE-JOINTE"), "{all}");
    assert!(!all.contains(&first_token), "the token leaves: {all}");
    assert!(
        bed.imap
            .with(|s| s.signed_in.iter().any(|(_, t)| *t == first_token))
    );
    Ok(())
}

#[test]
fn identity_and_bank_numbers_are_masked_and_health_and_family_kept() {
    let redactor = Redactor::new();
    let kept = [
        "Claire est enceinte de 5 mois, la naissance est prévue en mars.",
        "Papa a rendez-vous chez le cardiologue mardi pour son arythmie.",
        "Léo a 39 de fièvre et une otite, je garde les enfants.",
        "Ma sœur divorce, maman est inquiète.",
    ];
    for text in kept {
        assert_eq!(redactor.mask_text(text, "x"), text);
    }
    let secrets = [
        (
            "Mon IBAN : FR76 3000 4000 0312 3456 7890 143.",
            "3000 4000 0312",
        ),
        ("Carte 4970 5260 1815 9089 exp 09/28", "4970 5260 1815 9089"),
        ("Sécu de Léo : 1 89 05 75 123 456 71", "123 456 71"),
        ("Passeport 18AB12345 à renouveler", "18AB12345"),
    ];
    for (text, secret) in secrets {
        let masked = redactor.mask_text(text, "le mail de la mutuelle, 3 nov.");
        assert!(!masked.contains(secret), "{masked}");
        assert!(masked.contains("[secret masqué — voir le mail de la mutuelle, 3 nov.]"));
    }
}

#[test]
fn a_public_holiday_tomorrow_is_a_computed_fact() -> anyhow::Result<()> {
    let mut bed = start(Vec::new())?;
    bed.allow(
        "account_1700000000_0",
        WORK,
        SourceKind::Calendar,
        Notebook::Pro,
    );
    let events = bed.cycle();
    let read: Vec<hocus_sources::Item> =
        items(&events).into_iter().map(|r| r.item.clone()).collect();
    let school =
        SchoolCalendar::from_json(include_str!("fixtures/calendrier-scolaire.json"), None)?;
    let place = Place {
        region: Region::Metropole,
        school_zone: school.zone("Paris"),
        hours: None,
    };
    let today = now().date();
    let tomorrow = today.tomorrow()?;
    let facts = facts::day(tomorrow, &place, Some(&school), &read, &TimeZone::UTC);
    assert_eq!(
        facts.holiday.as_ref().map(|h| h.name),
        Some("Armistice 1918")
    );
    assert!(facts.free.is_empty(), "no free slot on a public holiday");
    let lines = facts.lines(&TimeZone::UTC, false);
    assert_eq!(
        lines[0],
        "[fait calculé] mercredi 11 novembre : jour férié (Armistice 1918)"
    );
    // Thursday: the meeting read from the calendar takes its slot; the
    // leave (all day, marked free) takes none.
    let thursday = facts::day(
        date(2026, 11, 12),
        &place,
        Some(&school),
        &read,
        &TimeZone::UTC,
    );
    assert_eq!(
        thursday.lines(&TimeZone::UTC, false)[1],
        "[fait calculé] jeudi 12 novembre : créneaux libres 9:00–10:00, 11:30–18:00"
    );
    // A school holiday of the taught zone.
    let christmas = facts::day(
        date(2026, 12, 21),
        &place,
        Some(&school),
        &[],
        &TimeZone::UTC,
    );
    assert!(
        christmas.lines(&TimeZone::UTC, false).iter().any(|l| l
            .contains("Vacances de Noël (Zone C), du samedi 19 décembre au dimanche 3 janvier")),
        "{:?}",
        christmas.lines(&TimeZone::UTC, false)
    );
    Ok(())
}
