//! Gmail over IMAP, read-only (AC15): against a fake server that holds
//! as a violation any command but the read-only ones and any fetch that
//! would set `\Seen`. A full cycle uses `EXAMINE` and `BODY.PEEK` only,
//! changes no flag, and the next cycles fetch only what is new.

mod fakes;

use std::time::Duration;

use fakes::imap::{FakeImap, Mailbox, Message, message};
use hocus_day::{google::AccessToken, secrets::Token, source::Result};
use hocus_sources::{
    ConsentBook, Content, Grant, Notebook, SourceKey, SourceKind,
    goa::Imap,
    imap::{self, Cursor},
};
use jiff::{civil::date, tz::TimeZone};

const ALEX: &str = "alex@atelier-lune.fr";

struct Fixed;

impl AccessToken for Fixed {
    fn get(&mut self) -> Result<(Token, Duration)> {
        Ok((Token::new("tok-imap-test"), Duration::from_secs(3600)))
    }
}

fn grant() -> Grant {
    let mut book = ConsentBook::default();
    let source = SourceKey::new("account_1", SourceKind::Mail);
    book.allow(
        source.clone(),
        ALEX,
        Notebook::Pro,
        jiff::Timestamp::UNIX_EPOCH,
    );
    book.grant(&source).expect("a grant")
}

fn mail(uid: u32, seen: bool, internal: &str, subject: &str) -> Message {
    Message {
        uid,
        flags: if seen {
            vec!["\\Seen".to_owned()]
        } else {
            Vec::new()
        },
        internal: internal.to_owned(),
        raw: message(
            "Léa Martin <lea@example.org>",
            ALEX,
            subject,
            "Thu, 05 Nov 2026 10:12:00 +0100",
            "Bonjour Alex,\r\n\r\nTu peux relire la spec avant jeudi ?\r\n",
        ),
        gmail_id: 1_780_000_000_000_000_000 + u64::from(uid),
    }
}

fn server() -> anyhow::Result<FakeImap> {
    FakeImap::start(vec![
        Mailbox {
            name: "INBOX".to_owned(),
            special: None,
            uidvalidity: 7,
            messages: vec![
                mail(3, false, "20-Sep-2026 09:00:00 +0200", "Ancien"),
                mail(5, true, "03-Nov-2026 09:00:00 +0100", "Lu"),
                mail(8, false, "05-Nov-2026 10:12:00 +0100", "Pas lu"),
            ],
        },
        Mailbox {
            name: "[Gmail]/Messages envoy&AOk-s".to_owned(),
            special: Some("\\Sent".to_owned()),
            uidvalidity: 9,
            messages: vec![mail(2, true, "04-Nov-2026 18:00:00 +0100", "Re: devis")],
        },
        Mailbox {
            name: "[Gmail]/Corbeille".to_owned(),
            special: Some("\\Trash".to_owned()),
            uidvalidity: 11,
            messages: vec![mail(1, false, "06-Nov-2026 08:00:00 +0100", "Jeté")],
        },
    ])
}

fn imap_of(server: &FakeImap) -> Imap {
    Imap {
        host: "127.0.0.1".to_owned(),
        port: server.port,
        user: ALEX.to_owned(),
        tls: false,
    }
}

const ALLOWED: [&str; 8] = [
    "CAPABILITY",
    "AUTHENTICATE",
    "LIST",
    "EXAMINE",
    "UID SEARCH",
    "UID FETCH",
    "NOOP",
    "LOGOUT",
];

#[test]
fn a_full_cycle_examines_and_peeks_only_and_changes_no_flag() -> anyhow::Result<()> {
    let server = server()?;
    let before = server.flags();
    let mut cursor = Cursor::default();
    for text in [false, true] {
        let read = imap::sync(
            &grant(),
            &imap_of(&server),
            &mut Fixed,
            &mut cursor,
            date(2026, 11, 10),
            &TimeZone::UTC,
            text,
        )?;
        if !text {
            // Inbox and sent, 30 days back: the September mail and the
            // trash are not read.
            let mut subjects: Vec<String> = read
                .items
                .iter()
                .filter_map(|i| match &i.content {
                    Content::Mail(m) => Some(format!(
                        "{}{}",
                        if m.sent { "sent:" } else { "" },
                        m.subject
                    )),
                    _ => None,
                })
                .collect();
            subjects.sort();
            assert_eq!(subjects, ["Lu", "Pas lu", "sent:Re: devis"]);
            let unread = read.items.iter().find_map(|i| match &i.content {
                Content::Mail(m) if m.subject == "Pas lu" => Some((m.seen, i.pointer.clone())),
                _ => None,
            });
            let (seen, pointer) = unread.expect("the unread mail");
            assert!(!seen);
            assert_eq!(pointer.id, "INBOX/7/8");
            assert_eq!(
                pointer.open.as_deref(),
                Some("https://mail.google.com/mail/u/alex@atelier-lune.fr/#all/18b3d491b4320008")
            );
        }
        // A fresh cursor each time: the text is fetched on the second.
        cursor = Cursor::default();
    }
    assert_eq!(server.violations(), Vec::<String>::new());
    assert_eq!(server.flags(), before, "no flag changed");
    let verbs = server.verbs();
    assert!(
        verbs.iter().all(|v| ALLOWED.contains(&v.as_str())),
        "{verbs:?}"
    );
    assert!(verbs.contains(&"EXAMINE".to_owned()));
    let commands = server.with(|s| s.commands.clone());
    assert!(
        commands
            .iter()
            .filter(|c| c.contains("FETCH"))
            .all(|c| c.contains("BODY.PEEK[") && !c.contains(" BODY[")),
        "{commands:?}"
    );
    assert_eq!(
        server.with(|s| s.signed_in.clone()),
        vec![(ALEX.to_owned(), "tok-imap-test".to_owned()); 2]
    );
    Ok(())
}

#[test]
fn the_next_cycles_fetch_only_what_is_new() -> anyhow::Result<()> {
    let server = server()?;
    let mut cursor = Cursor::default();
    let sync = |cursor: &mut Cursor| {
        imap::sync(
            &grant(),
            &imap_of(&server),
            &mut Fixed,
            cursor,
            date(2026, 11, 10),
            &TimeZone::UTC,
            true,
        )
    };
    let first = sync(&mut cursor)?;
    assert_eq!(first.items.len(), 3);
    assert_eq!(cursor.mailboxes["INBOX"].uidnext, 9);
    let commands = |server: &FakeImap| server.with(|s| s.commands.len());

    // Nothing new: no search, no fetch.
    let seen = commands(&server);
    let second = sync(&mut cursor)?;
    assert!(second.items.is_empty());
    let after: Vec<String> = server.verbs()[seen..].to_vec();
    assert!(!after.iter().any(|v| v.starts_with("UID")), "{after:?}");

    // One new mail: only it is fetched, from the last UIDNEXT on.
    server.with(|s| {
        s.mailboxes[0]
            .messages
            .push(mail(12, false, "09-Nov-2026 07:30:00 +0100", "Nouveau"));
    });
    let seen = commands(&server);
    let third = sync(&mut cursor)?;
    assert_eq!(third.items.len(), 1);
    assert_eq!(third.items[0].pointer.id, "INBOX/7/12");
    let fetches: Vec<String> = server.with(|s| s.commands[seen..].to_vec());
    assert!(
        fetches.iter().any(|c| c.contains("UID FETCH 9:*")),
        "{fetches:?}"
    );

    // The server's UIDs changed: what was read of the inbox is void, and
    // it is read again 30 days back.
    server.with(|s| s.mailboxes[0].uidvalidity = 8);
    let fourth = sync(&mut cursor)?;
    assert_eq!(fourth.reset, ["INBOX"]);
    assert_eq!(fourth.items.len(), 3);
    assert_eq!(server.violations(), Vec::<String>::new());
    Ok(())
}

#[test]
fn a_mail_is_fetched_by_its_id_on_demand() -> anyhow::Result<()> {
    let server = server()?;
    let item = imap::fetch(
        &grant(),
        &imap_of(&server),
        &mut Fixed,
        "INBOX/7/8",
        &TimeZone::UTC,
    )?
    .expect("the mail");
    match item.content {
        Content::Mail(mail) => {
            assert!(mail.body.unwrap_or_default().contains("relire la spec"));
        }
        other => panic!("not a mail: {other:?}"),
    }
    // Its UIDs changed since: it is not taken for another.
    assert!(
        imap::fetch(
            &grant(),
            &imap_of(&server),
            &mut Fixed,
            "INBOX/6/8",
            &TimeZone::UTC
        )?
        .is_none()
    );
    assert_eq!(server.violations(), Vec::<String>::new());
    Ok(())
}

/// The fake bites: what a careless client would send is held against it.
#[test]
fn the_fake_server_holds_a_write_or_a_read_that_marks_seen() -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader, Write};
    let server = server()?;
    let stream = std::net::TcpStream::connect(("127.0.0.1", server.port))?;
    let mut out = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    use base64::Engine as _;
    let auth = base64::engine::general_purpose::STANDARD.encode("user=a\x01auth=Bearer t\x01\x01");
    for command in [
        format!("a1 AUTHENTICATE XOAUTH2 {auth}"),
        "a2 EXAMINE \"INBOX\"".to_owned(),
        "a3 UID FETCH 8 (BODY[])".to_owned(),
        "a4 STORE 1 +FLAGS (\\Seen)".to_owned(),
        "a5 SELECT INBOX".to_owned(),
    ] {
        out.write_all(format!("{command}\r\n").as_bytes())?;
        let tag = command.split(' ').next().unwrap_or("");
        loop {
            line.clear();
            if reader.read_line(&mut line)? == 0 || line.starts_with(tag) {
                break;
            }
        }
    }
    assert_eq!(server.violations().len(), 3, "{:?}", server.violations());
    assert!(
        server.flags().iter().any(|(m, uid, flags)| m == "INBOX"
            && *uid == 8
            && flags.contains(&"\\Seen".to_owned()))
    );
    Ok(())
}
