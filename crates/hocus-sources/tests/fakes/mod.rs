//! Fakes for the sources' tests, never the network nor the live session
//! bus: a private `dbus-daemon` with a fake GNOME Online Accounts on it
//! (accounts with their calendar, mail and contacts switches, and the
//! `Mail` interface), the Calendar API (`hocus-day`'s fake), a CardDAV
//! server, and an IMAP server that records every command and holds what
//! a read-only client must never do as a violation.

#![allow(dead_code, unused_imports, reason = "each test uses some of the fakes")]

#[path = "../../../hocus-day/tests/fakes/mod.rs"]
pub mod day;

pub mod imap;

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

pub use day::{FakeApi, PrivateBus};

// --- Online Accounts ---------------------------------------------------------------

const GOA: &str = "org.gnome.OnlineAccounts";
const ROOT: &str = "/org/gnome/OnlineAccounts";

/// The token every account's `GetAccessToken` gives, numbered.
pub const TOKEN: &str = "ya29.goa-fake-7f3a9c";

#[derive(Default)]
struct GoaState {
    given: u32,
}

struct Account {
    id: String,
    identity: String,
    mail_disabled: bool,
}

#[zbus::interface(name = "org.gnome.OnlineAccounts.Account")]
impl Account {
    #[zbus(property)]
    fn provider_type(&self) -> String {
        "google".to_owned()
    }

    #[zbus(property)]
    fn id(&self) -> String {
        self.id.clone()
    }

    #[zbus(property)]
    fn identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn presentation_identity(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn calendar_disabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn contacts_disabled(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn mail_disabled(&self) -> bool {
        self.mail_disabled
    }

    #[zbus(property)]
    fn attention_needed(&self) -> bool {
        false
    }
}

struct OAuth2(Arc<Mutex<GoaState>>);

#[zbus::interface(name = "org.gnome.OnlineAccounts.OAuth2Based")]
impl OAuth2 {
    fn get_access_token(&self) -> (String, i32) {
        let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        state.given += 1;
        (format!("{TOKEN}-{}", state.given), 3600)
    }
}

struct MailSettings {
    identity: String,
}

#[zbus::interface(name = "org.gnome.OnlineAccounts.Mail")]
impl MailSettings {
    #[zbus(property)]
    fn email_address(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn imap_host(&self) -> String {
        "imap.gmail.com".to_owned()
    }

    #[zbus(property)]
    fn imap_user_name(&self) -> String {
        self.identity.clone()
    }

    #[zbus(property)]
    fn imap_use_ssl(&self) -> bool {
        true
    }
}

/// Online Accounts on a private bus, its Google accounts set by the test.
pub struct FakeGoa {
    runtime: tokio::runtime::Runtime,
    connection: zbus::Connection,
    state: Arc<Mutex<GoaState>>,
}

impl FakeGoa {
    pub fn start(bus: &str) -> anyhow::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let connection = runtime.block_on(async {
            zbus::connection::Builder::address(bus)
                .serve_at(ROOT, zbus::fdo::ObjectManager)
                .name(GOA)
                .build()
                .await
        })?;
        Ok(Self {
            runtime,
            connection,
            state: Arc::default(),
        })
    }

    /// A Google account `id` (its object's last segment too), with its
    /// mail on.
    pub fn add(&self, id: &str, identity: &str) -> anyhow::Result<()> {
        let path = format!("{ROOT}/Accounts/{id}");
        let server = self.connection.object_server();
        self.runtime.block_on(async {
            server
                .at(
                    path.as_str(),
                    Account {
                        id: id.to_owned(),
                        identity: identity.to_owned(),
                        mail_disabled: false,
                    },
                )
                .await?;
            server.at(path.as_str(), OAuth2(self.state.clone())).await?;
            server
                .at(
                    path.as_str(),
                    MailSettings {
                        identity: identity.to_owned(),
                    },
                )
                .await?;
            anyhow::Ok(())
        })
    }

    /// How many tokens it gave.
    pub fn given(&self) -> u32 {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .given
    }
}

// --- CardDAV ------------------------------------------------------------------------

/// A CardDAV server on 127.0.0.1 that answers as Google's does
/// (developers.google.com/people/carddav): a `PROPFIND` of depth 1 lists
/// the address book's cards (their `href` and `getetag`, the address
/// book first); an `addressbook-multiget` `REPORT` gives the cards it
/// names; an `addressbook-query` `REPORT`, which Google does not
/// implement, is answered `400` (#754).
pub struct FakeCardDav {
    pub address: String,
    /// What it was asked: `METHOD path`.
    pub asked: Arc<Mutex<Vec<String>>>,
}

impl FakeCardDav {
    pub fn start(cards: Vec<(String, String)>) -> anyhow::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = format!("http://{}", listener.local_addr()?);
        let asked: Arc<Mutex<Vec<String>>> = Arc::default();
        let served = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                carddav(stream, &cards, &served);
            }
        });
        Ok(Self { address, asked })
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

fn carddav(mut stream: TcpStream, cards: &[(String, String)], asked: &Mutex<Vec<String>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let Ok(reader) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(reader);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut words = line.split_whitespace();
    let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
    let mut length = 0usize;
    let mut depth = String::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).is_err() || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            } else if name.trim().eq_ignore_ascii_case("depth") {
                depth = value.trim().to_owned();
            }
        }
    }
    let mut body = vec![0; length];
    let _ = reader.read_exact(&mut body);
    let body = String::from_utf8_lossy(&body).into_owned();
    asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(format!("{method} {path}"));
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<d:multistatus xmlns:d=\"DAV:\" xmlns:card=\"urn:ietf:params:xml:ns:carddav\">";
    let answer = |stream: &mut TcpStream, status: &str, xml: &str| {
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: application/xml; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{xml}",
            xml.len()
        );
    };
    match method {
        "PROPFIND" if depth == "1" => {
            let mut xml = String::from(head);
            xml.push_str(&format!(
                "<d:response><d:href>/carddav/v1{path}</d:href><d:propstat><d:prop><d:resourcetype><d:collection/><card:addressbook/></d:resourcetype></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
            ));
            for (href, _) in cards {
                xml.push_str(&format!(
                    "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:getetag>\"1\"</d:getetag><d:resourcetype/></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                ));
            }
            xml.push_str("</d:multistatus>");
            answer(&mut stream, "207 Multi-Status", &xml);
        }
        "REPORT" if body.contains("addressbook-multiget") => {
            let mut xml = String::from(head);
            for (href, card) in cards.iter().filter(|(h, _)| body.contains(&format!(">{h}<"))) {
                let card = card
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                xml.push_str(&format!(
                    "<d:response><d:href>{href}</d:href><d:propstat><d:prop><d:getetag>\"1\"</d:getetag><card:address-data>{card}</card:address-data></d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                ));
            }
            xml.push_str("</d:multistatus>");
            answer(&mut stream, "207 Multi-Status", &xml);
        }
        // What Google answers a report it does not implement.
        "REPORT" => answer(
            &mut stream,
            "400 Bad Request",
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<errors xmlns=\"http://schemas.google.com/g/2005\"><error><domain>GData</domain><code>badRequest</code></error></errors>",
        ),
        _ => answer(&mut stream, "405 Method Not Allowed", ""),
    }
}
