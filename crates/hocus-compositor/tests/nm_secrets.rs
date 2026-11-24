//! NetworkManager's secrets asked by the desktop (#930): the real shell,
//! NetworkManager's secret agent on a D-Bus daemon of the test's own (as
//! the system bus), against a fake NetworkManager that records the agent's
//! registration and calls `GetSecrets` and `CancelGetSecrets` as
//! NetworkManager does; a fake keyring (the Secret Service) on another
//! daemon of the test's, as the session's bus. Never the machine's
//! NetworkManager, its system bus or the session's keyring.
//!
//! One test per point: a VPN asking its password gets it (the agent
//! registered first); nothing is asked when NetworkManager may not ask, nor
//! for a Wi-Fi password NetworkManager keeps (the bar's popover's); `Esc`
//! answers that the user cancelled, NetworkManager withdrawing closes the
//! dialog; "Retenir le mot de passe" keeps the password in the keyring,
//! never the one-time code, and the keyring answers next time; unticked,
//! a refused password leaves the keyring.
//!
//! What the dialog looks like is validated on screenshots
//! (`screenshots_for_the_pull_request`), not here. Pixman, the shell on the
//! CPU: as in CI. Without the shell built or dbus-daemon, the tests fail.

use crate::common;

use std::{
    collections::HashMap,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use common::{KEY_ESC, KEY_TAB, WAIT};
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use hocus_link::AUTH_NAMESPACE;
use zbus::{ObjectPath, ObjectServer, OwnedObjectPath, OwnedVariant, Variant, message::Header};

const KEY_ENTER: u32 = 28;
const KEY_SPACE: u32 = 57;
const AGENT: &str = "org.freedesktop.NetworkManager.SecretAgent";
const AGENT_PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
const CONNECTION: &str = "/org/freedesktop/NetworkManager/Settings/7";
const UUID: &str = "5e1f0c4a-930a-4b5e-9d0c-0b0e5f6a7c01";
const ERRORS: &str = "org.freedesktop.NetworkManager.SecretAgent.";

fn has(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .stdout(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

fn ready() -> PathBuf {
    let shell = common::shell_program();
    if !has("dbus-daemon") {
        common::missing("dbus-daemon");
    }
    shell
}

/// What the fakes saw: the agents registered (their bus name, identifier
/// and capabilities), how each `GetSecrets` was answered (by tag: the
/// secrets, or the error's name), and the keyring's items.
#[derive(Debug, Default)]
struct Seen {
    registered: Vec<(String, String, u32)>,
    answers: HashMap<String, Result<HashMap<String, String>, String>>,
    items: Vec<Item>,
}

#[derive(Debug, Clone)]
struct Item {
    attributes: HashMap<String, String>,
    secret: Vec<u8>,
    deleted: bool,
}

type Shared = Arc<Mutex<Seen>>;

fn seen(shared: &Shared) -> std::sync::MutexGuard<'_, Seen> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// NetworkManager's agent manager, as far as an agent sees it.
struct Manager(Shared);

#[zbus::interface(name = "org.freedesktop.NetworkManager.AgentManager")]
impl Manager {
    fn register(&self, identifier: String, #[zbus(header)] header: Header<'_>) {
        self.register_with_capabilities(identifier, 0, header);
    }

    fn register_with_capabilities(
        &self,
        identifier: String,
        capabilities: u32,
        #[zbus(header)] header: Header<'_>,
    ) {
        let sender = header.sender().map(|s| s.to_string()).unwrap_or_default();
        seen(&self.0)
            .registered
            .push((sender, identifier, capabilities));
    }

    fn unregister(&self) {}
}

/// The Secret Service, as far as a client keeping its secrets sees it:
/// one collection, items found by their attributes, nothing locked.
struct Service(Shared);

const COLLECTION_PATH: &str = "/org/freedesktop/secrets/collection/login";

fn item_path(i: usize) -> String {
    format!("/org/freedesktop/secrets/item/i{i}")
}

#[zbus::interface(name = "org.freedesktop.Secret.Service")]
impl Service {
    fn open_session(
        &self,
        _algorithm: String,
        _input: OwnedVariant,
    ) -> (OwnedVariant, OwnedObjectPath) {
        (
            OwnedVariant::try_from(Variant::from("")).expect("a value"),
            OwnedObjectPath::try_from("/org/freedesktop/secrets/session/s1").expect("a path"),
        )
    }

    fn search_items(
        &self,
        attributes: HashMap<String, String>,
    ) -> (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) {
        let found = seen(&self.0)
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                !item.deleted
                    && attributes
                        .iter()
                        .all(|(k, v)| item.attributes.get(k) == Some(v))
            })
            .filter_map(|(i, _)| OwnedObjectPath::try_from(item_path(i)).ok())
            .collect();
        (found, Vec::new())
    }

    fn read_alias(&self, _name: String) -> OwnedObjectPath {
        OwnedObjectPath::try_from(COLLECTION_PATH).expect("a path")
    }
}

struct Collection(Shared);

#[zbus::interface(name = "org.freedesktop.Secret.Collection")]
impl Collection {
    async fn create_item(
        &self,
        properties: HashMap<String, OwnedVariant>,
        secret: (OwnedObjectPath, Vec<u8>, Vec<u8>, String),
        replace: bool,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<(OwnedObjectPath, OwnedObjectPath)> {
        let attributes = properties
            .get("org.freedesktop.Secret.Item.Attributes")
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| HashMap::<String, String>::try_from(v).ok())
            .unwrap_or_default();
        let i = {
            let mut seen = seen(&self.0);
            if replace {
                for item in &mut seen.items {
                    if item.attributes == attributes {
                        item.deleted = true;
                    }
                }
            }
            seen.items.push(Item {
                attributes,
                secret: secret.2,
                deleted: false,
            });
            seen.items.len() - 1
        };
        server
            .at(item_path(i), ItemObject(self.0.clone(), i))
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok((
            OwnedObjectPath::try_from(item_path(i)).expect("a path"),
            OwnedObjectPath::try_from("/").expect("a path"),
        ))
    }
}

struct ItemObject(Shared, usize);

#[zbus::interface(name = "org.freedesktop.Secret.Item")]
impl ItemObject {
    fn get_secret(
        &self,
        session: OwnedObjectPath,
    ) -> zbus::fdo::Result<(OwnedObjectPath, Vec<u8>, Vec<u8>, String)> {
        let item = seen(&self.0).items.get(self.1).cloned();
        match item.filter(|i| !i.deleted) {
            Some(item) => Ok((session, Vec::new(), item.secret, "text/plain".to_owned())),
            None => Err(zbus::fdo::Error::UnknownObject("deleted".to_owned())),
        }
    }

    fn delete(&self) -> OwnedObjectPath {
        if let Some(item) = seen(&self.0).items.get_mut(self.1) {
            item.deleted = true;
        }
        OwnedObjectPath::try_from("/").expect("a path")
    }
}

struct Bed {
    hp: Headless,
    log: PathBuf,
    runtime: tokio::runtime::Runtime,
    nm: zbus::Connection,
    keyring: zbus::Connection,
    seen: Shared,
    /// Last, so stopped once the compositor and its connections are.
    buses: Vec<Child>,
    _dir: tempfile::TempDir,
}

impl Drop for Bed {
    fn drop(&mut self) {
        for bus in &mut self.buses {
            let _ = bus.kill();
            let _ = bus.wait();
        }
    }
}

/// A connection's settings, as NetworkManager sends them to an agent.
type Settings = HashMap<String, HashMap<String, OwnedVariant>>;

fn value<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
    OwnedVariant::try_from(value.into()).expect("a value")
}

fn pairs(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

/// An OpenVPN connection, its data and the secrets NetworkManager holds.
fn vpn(data: &[(&str, &str)], secrets: &[(&str, &str)]) -> Settings {
    HashMap::from([
        (
            "connection".to_owned(),
            HashMap::from([
                ("id".to_owned(), value("VPN Bureau")),
                ("uuid".to_owned(), value(UUID)),
                ("type".to_owned(), value("vpn")),
            ]),
        ),
        (
            "vpn".to_owned(),
            HashMap::from([
                (
                    "service-type".to_owned(),
                    value("org.freedesktop.NetworkManager.openvpn"),
                ),
                ("data".to_owned(), value(pairs(data))),
                ("secrets".to_owned(), value(pairs(secrets))),
            ]),
        ),
    ])
}

fn ok(secrets: &[(&str, &str)]) -> Result<HashMap<String, String>, String> {
    Ok(pairs(secrets))
}

fn refused(error: &str) -> Result<HashMap<String, String>, String> {
    Err(format!("{ERRORS}{error}"))
}

impl Bed {
    fn start(shell: &Path) -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let state = dir.path().join("state");
        let session_dir = dir.path().join("session");
        for sub in [&home, &state.join("hocuspanda"), &session_dir] {
            std::fs::create_dir_all(sub)?;
        }
        let (system, address) = common::private_bus(dir.path())?;
        let (session, session_address) = common::private_bus(&session_dir)?;
        let log = dir.path().join("shell.log");
        let script = dir.path().join("shell");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nunset DBUS_SESSION_BUS_ADDRESS\n\
                 export DBUS_SYSTEM_BUS_ADDRESS='{address}' \
                 HOCUS_NOTIFICATIONS_BUS='{session_address}' \
                 PIPEWIRE_REMOTE=/nonexistent HOCUS_BACKLIGHT_DIR=/nonexistent \
                 LC_ALL=fr_FR.UTF-8 LANG=fr_FR.UTF-8 HOME='{home}' \
                 XDG_CONFIG_HOME='{home}/.config' XDG_CACHE_HOME='{home}/.cache' \
                 XDG_DATA_HOME='{home}/.local/share' XDG_STATE_HOME='{state}' \
                 TZ=UTC ICED_BACKEND=tiny-skia HOCUS_AUTH_AGENT=1 XDG_SESSION_ID=7 \
                 RUST_LOG=warn,hocuspanda_shell=debug\n\
                 exec '{shell}' \"$@\" 2>>'{log}'\n",
                home = home.display(),
                state = state.display(),
                shell = shell.display(),
                log = log.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;
        let seen = Shared::default();
        let nm = runtime.block_on(
            zbus::connection::Builder::address(address.as_str())
                .name("org.freedesktop.NetworkManager")
                .serve_at(
                    "/org/freedesktop/NetworkManager/AgentManager",
                    Manager(seen.clone()),
                )
                .build(),
        )?;
        let keyring = runtime.block_on(
            zbus::connection::Builder::address(session_address.as_str())
                .name("org.freedesktop.secrets")
                .serve_at("/org/freedesktop/secrets", Service(seen.clone()))
                .serve_at(COLLECTION_PATH, Collection(seen.clone()))
                .build(),
        )?;
        let mut hp = Headless::start_with_shell(
            HeadlessConfig {
                outputs: vec!["1920x1080".parse()?],
                renderer: RendererChoice::Pixman,
                render_node: None,
            },
            Config {
                plain_background: true,
                ..Config::default()
            },
            Some(script),
        )?;
        let drawn = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(drawn, "no bar");
        let mut bed = Self {
            hp,
            log,
            runtime,
            nm,
            keyring,
            seen,
            buses: vec![session, system],
            _dir: dir,
        };
        bed.registered()?;
        bed.hp.pointer_move(400.0, 600.0);
        Ok(bed)
    }

    fn idle(&mut self, time: Duration) -> anyhow::Result<()> {
        self.hp.dispatch_until(time, |_| false)?;
        Ok(())
    }

    /// Waits until the shell registered as NetworkManager's agent.
    fn registered(&mut self) -> anyhow::Result<(String, String, u32)> {
        let seen = self.seen.clone();
        let done = self.hp.dispatch_until(Duration::from_secs(15), |_| {
            !self::seen(&seen).registered.is_empty()
        })?;
        anyhow::ensure!(done, "the shell never registered:\n{}", self.log());
        Ok(self::seen(&self.seen).registered[0].clone())
    }

    /// NetworkManager asks for `setting`'s secrets of `connection`: its
    /// answer recorded under `tag` once it comes.
    fn ask(
        &mut self,
        tag: &str,
        connection: Settings,
        setting: &str,
        hints: &[&str],
        flags: u32,
    ) -> anyhow::Result<()> {
        let (agent, _, _) = self.registered()?;
        let nm = self.nm.clone();
        let seen = self.seen.clone();
        let (tag, setting) = (tag.to_owned(), setting.to_owned());
        let hints: Vec<String> = hints.iter().map(|h| (*h).to_owned()).collect();
        self.runtime.spawn(async move {
            let path = ObjectPath::try_from(CONNECTION).expect("a path");
            let reply = nm
                .call_method(
                    Some(agent.as_str()),
                    AGENT_PATH,
                    Some(AGENT),
                    "GetSecrets",
                    &(connection, path, setting.as_str(), hints, flags),
                )
                .await;
            let answer = match reply {
                Ok(reply) => {
                    let settings: Settings = reply.body().deserialize().unwrap_or_default();
                    let props = settings.get(&setting).cloned().unwrap_or_default();
                    let secrets = if setting == "vpn" {
                        props
                            .get("secrets")
                            .and_then(|v| v.try_clone().ok())
                            .and_then(|v| HashMap::<String, String>::try_from(v).ok())
                            .unwrap_or_default()
                    } else {
                        props
                            .iter()
                            .filter_map(|(k, v)| {
                                Some((k.clone(), v.downcast_ref::<String>().ok()?))
                            })
                            .collect()
                    };
                    Ok(secrets)
                }
                Err(zbus::Error::MethodError(name, _, _)) => Err(name.to_string()),
                Err(err) => Err(format!("{err}")),
            };
            self::seen(&seen).answers.insert(tag, answer);
        });
        Ok(())
    }

    /// NetworkManager withdraws its request for `setting`.
    fn withdraw(&mut self, setting: &str) -> anyhow::Result<()> {
        let (agent, _, _) = self.registered()?;
        let nm = self.nm.clone();
        let setting = setting.to_owned();
        self.runtime.block_on(async move {
            nm.call_method(
                Some(agent.as_str()),
                AGENT_PATH,
                Some(AGENT),
                "CancelGetSecrets",
                &(ObjectPath::try_from(CONNECTION)?, setting.as_str()),
            )
            .await
        })?;
        Ok(())
    }

    /// How the request `tag` was answered, once it is.
    fn answered(&mut self, tag: &str) -> anyhow::Result<Result<HashMap<String, String>, String>> {
        let seen = self.seen.clone();
        let done = self
            .hp
            .dispatch_until(WAIT, |_| self::seen(&seen).answers.contains_key(tag))?;
        anyhow::ensure!(done, "{tag} never answered:\n{}", self.log());
        Ok(self::seen(&self.seen).answers[tag].clone())
    }

    /// The keyring's items of the test's connection, by their key, once
    /// `until` holds of them.
    fn kept_until(
        &mut self,
        until: impl Fn(&HashMap<String, Vec<u8>>) -> bool,
    ) -> anyhow::Result<HashMap<String, Vec<u8>>> {
        let seen = self.seen.clone();
        let kept = move || -> HashMap<String, Vec<u8>> {
            self::seen(&seen)
                .items
                .iter()
                .filter(|i| {
                    !i.deleted
                        && i.attributes.get("connection-uuid").map(String::as_str) == Some(UUID)
                })
                .filter_map(|i| Some((i.attributes.get("setting-key")?.clone(), i.secret.clone())))
                .collect()
        };
        let done = self.hp.dispatch_until(WAIT, |_| until(&kept()))?;
        anyhow::ensure!(
            done,
            "the keyring holds {:?}:\n{}",
            kept().keys(),
            self.log()
        );
        Ok(kept())
    }

    /// An item in the keyring as another agent would have kept it.
    fn keep(&mut self, key: &str, secret: &str) -> anyhow::Result<()> {
        let attributes = pairs(&[
            ("xdg:schema", "org.freedesktop.NetworkManager.Connection"),
            ("connection-uuid", UUID),
            ("setting-name", "vpn"),
            ("setting-key", key),
        ]);
        let i = {
            let mut seen = seen(&self.seen);
            seen.items.push(Item {
                attributes,
                secret: secret.as_bytes().to_vec(),
                deleted: false,
            });
            seen.items.len() - 1
        };
        let keyring = self.keyring.clone();
        let shared = self.seen.clone();
        self.runtime.block_on(async move {
            keyring
                .object_server()
                .at(item_path(i), ItemObject(shared, i))
                .await
        })?;
        Ok(())
    }

    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    /// Waits until the shell logged `what` after `from` bytes of its log.
    fn logged_after(&mut self, from: usize, what: &str) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(15) {
            if self.log().get(from..).is_some_and(|l| l.contains(what)) {
                return Ok(());
            }
            self.idle(Duration::from_millis(30))?;
        }
        anyhow::bail!("the shell did not log {what:?}:\n{}", self.log())
    }

    /// The dialog, once it shows and holds the keyboard.
    fn dialog(&mut self) -> anyhow::Result<()> {
        let shown = self.hp.dispatch_until(WAIT, |hp| {
            !hp.drawn_layers(AUTH_NAMESPACE).is_empty()
                && hp.auth_holds()
                && hp.keyboard_layer().as_deref() == Some(AUTH_NAMESPACE)
        })?;
        anyhow::ensure!(shown, "no dialog:\n{}", self.log());
        Ok(())
    }

    /// Waits until the dialog is gone.
    fn gone(&mut self) -> anyhow::Result<()> {
        let gone = self
            .hp
            .dispatch_until(WAIT, |hp| hp.layer_rects(AUTH_NAMESPACE).is_empty())?;
        anyhow::ensure!(gone, "the dialog stays:\n{}", self.log());
        anyhow::ensure!(!self.hp.auth_holds(), "the dialog still holds the input");
        Ok(())
    }

    fn key(&mut self, code: u32) {
        self.hp.key(code, true);
        self.hp.key(code, false);
    }

    /// `word` typed, a key each (evdev codes of the default keymap's
    /// letters and digits).
    fn type_word(&mut self, word: &str) {
        for c in word.chars() {
            let code = match c {
                'p' => 25,
                'a' => 30,
                'n' => 49,
                'd' => 32,
                'o' => 24,
                'e' => 18,
                '1'..='9' => 2 + c as u32 - '1' as u32,
                '0' => 11,
                _ => continue,
            };
            self.key(code);
        }
    }

    /// `word` typed in the field holding the keyboard, once the dialog says
    /// it holds as many characters as `label`.
    fn fill(&mut self, label: &str, word: &str) -> anyhow::Result<()> {
        let from = self.log().len();
        self.type_word(word);
        self.logged_after(from, &format!("{label}: {}", word.chars().count()))
    }
}

/// NetworkManager's `GetSecrets` flags.
const ALLOW_INTERACTION: u32 = 0x1;
const REQUEST_NEW: u32 = 0x2;

#[test]
fn a_vpn_asking_its_password_gets_it() -> anyhow::Result<()> {
    let shell = ready();
    let mut bed = Bed::start(&shell)?;
    let (sender, identifier, capabilities) = bed.registered()?;
    assert!(sender.starts_with(':'), "{sender}");
    assert_eq!(identifier, "org.hocuspanda.shell");
    assert_eq!(capabilities, 1, "it takes a VPN plugin's hints");
    // A password asked each time (`password-flags` 2).
    let connection = vpn(&[("username", "alex"), ("password-flags", "2")], &[]);
    bed.ask("asked", connection, "vpn", &[], ALLOW_INTERACTION)?;
    bed.dialog()?;
    bed.logged_after(
        0,
        "\"VPN Bureau\", \"Le VPN demande de quoi se connecter\", \"Mot de passe: 0\"",
    )?;
    let log = bed.log();
    assert!(log.contains("[Se connecter (inert)]"), "{log}");
    // Never kept: nothing to remember.
    assert!(!log.contains("Retenir le mot de passe"), "{log}");
    bed.fill("Mot de passe", "addone")?;
    bed.key(KEY_ENTER);
    assert_eq!(bed.answered("asked")?, ok(&[("password", "addone")]));
    bed.gone()?;
    assert!(bed.kept_until(|_| true)?.is_empty(), "nothing kept");
    // Never logged.
    assert!(!bed.log().contains("addone"), "{}", bed.log());
    Ok(())
}

#[test]
fn nothing_is_asked_when_networkmanager_may_not_ask_nor_for_a_password_it_keeps()
-> anyhow::Result<()> {
    let shell = ready();
    let mut bed = Bed::start(&shell)?;
    // Without interaction, and nothing kept: no secrets.
    let connection = vpn(&[("password-flags", "1")], &[]);
    bed.ask("quiet", connection, "vpn", &[], 0)?;
    assert_eq!(bed.answered("quiet")?, refused("NoSecrets"));
    // A Wi-Fi network whose password NetworkManager keeps, refused: the
    // bar's popover unfolds its field, no dialog.
    let wifi: Settings = HashMap::from([
        (
            "connection".to_owned(),
            HashMap::from([
                ("id".to_owned(), value("Café Lomi")),
                ("uuid".to_owned(), value(UUID)),
                ("type".to_owned(), value("802-11-wireless")),
            ]),
        ),
        (
            "802-11-wireless-security".to_owned(),
            HashMap::from([
                ("key-mgmt".to_owned(), value("wpa-psk")),
                ("psk-flags".to_owned(), value(0_u32)),
            ]),
        ),
    ]);
    bed.ask(
        "wifi",
        wifi,
        "802-11-wireless-security",
        &[],
        ALLOW_INTERACTION | REQUEST_NEW,
    )?;
    assert_eq!(bed.answered("wifi")?, refused("NoSecrets"));
    bed.idle(Duration::from_millis(300))?;
    assert!(bed.hp.layer_rects(AUTH_NAMESPACE).is_empty());
    Ok(())
}

#[test]
fn cancelled_or_withdrawn_networkmanager_hears_why() -> anyhow::Result<()> {
    let shell = ready();
    let mut bed = Bed::start(&shell)?;
    let connection = vpn(&[("password-flags", "2")], &[]);
    bed.ask("esc", connection.clone(), "vpn", &[], ALLOW_INTERACTION)?;
    bed.dialog()?;
    bed.key(KEY_ESC);
    assert_eq!(bed.answered("esc")?, refused("UserCanceled"));
    bed.gone()?;
    bed.ask("withdrawn", connection, "vpn", &[], ALLOW_INTERACTION)?;
    bed.dialog()?;
    bed.withdraw("vpn")?;
    bed.gone()?;
    assert_eq!(bed.answered("withdrawn")?, refused("AgentCanceled"));
    Ok(())
}

#[test]
fn remembered_the_password_is_kept_never_the_code_and_answers_next_time() -> anyhow::Result<()> {
    let shell = ready();
    let mut bed = Bed::start(&shell)?;
    // The agent keeps the password (`password-flags` 1); a static
    // challenge asks a one-time code.
    let connection = vpn(
        &[
            ("username", "alex"),
            ("password-flags", "1"),
            ("static-challenge", "Code, true"),
        ],
        &[],
    );
    bed.ask("first", connection, "vpn", &[], ALLOW_INTERACTION)?;
    bed.dialog()?;
    bed.logged_after(
        0,
        "\"Mot de passe: 0\", \"Code à usage unique: 0\", \"[x] Retenir le mot de passe\"",
    )?;
    bed.fill("Mot de passe", "panda")?;
    bed.key(KEY_TAB);
    bed.fill("Code à usage unique", "123456")?;
    bed.key(KEY_ENTER);
    assert_eq!(
        bed.answered("first")?,
        ok(&[("password", "panda"), ("challenge-response", "123456")])
    );
    let kept = bed.kept_until(|k| !k.is_empty())?;
    assert_eq!(
        kept,
        HashMap::from([("password".to_owned(), b"panda".to_vec())])
    );
    bed.gone()?;
    // Next time, without a code to ask: the keyring answers, no dialog.
    let connection = vpn(&[("password-flags", "1")], &[]);
    bed.ask("next", connection, "vpn", &[], ALLOW_INTERACTION)?;
    assert_eq!(bed.answered("next")?, ok(&[("password", "panda")]));
    assert!(bed.hp.layer_rects(AUTH_NAMESPACE).is_empty());
    Ok(())
}

#[test]
fn not_remembered_a_refused_password_leaves_the_keyring() -> anyhow::Result<()> {
    let shell = ready();
    let mut bed = Bed::start(&shell)?;
    bed.keep("password", "old")?;
    let connection = vpn(&[("password-flags", "1")], &[]);
    // Refused: new ones are asked for, the old one not filled in.
    bed.ask(
        "refused",
        connection,
        "vpn",
        &[],
        ALLOW_INTERACTION | REQUEST_NEW,
    )?;
    bed.dialog()?;
    bed.logged_after(
        0,
        "\"Mot de passe: 0\", \"Mot de passe refusé\", \"[x] Retenir le mot de passe\"",
    )?;
    bed.fill("Mot de passe", "panda")?;
    // Tab to the box, Space unticks it, Tab twice to Se connecter.
    let from = bed.log().len();
    bed.key(KEY_TAB);
    bed.key(KEY_SPACE);
    bed.logged_after(from, "[ ] Retenir le mot de passe")?;
    bed.key(KEY_TAB);
    bed.key(KEY_TAB);
    bed.key(KEY_ENTER);
    assert_eq!(bed.answered("refused")?, ok(&[("password", "panda")]));
    let kept = bed.kept_until(HashMap::is_empty)?;
    assert!(kept.is_empty());
    Ok(())
}

/// The pull request's screenshots, at 1920 × 1080 at scale 1, the shell
/// on the CPU: a VPN asking its password; one asking its password, kept,
/// and a one-time code; a refused Wi-Fi key the agent keeps. Written to
/// `$HOCUS_SHOTS` when it names a folder. Run by hand
/// (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    use anyhow::Context;
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    let shell = ready();
    std::fs::create_dir_all(&out)?;
    let save = |bed: &mut Bed, name: &str| -> anyhow::Result<()> {
        common::at_rest(&mut bed.hp)?;
        bed.idle(Duration::from_millis(300))?;
        common::at_rest(&mut bed.hp)?;
        let (w, h, bytes) = bed.hp.screenshot(0)?;
        let rgba: Vec<u8> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(u32::try_from(w)?, u32::try_from(h)?, rgba)
            .context("a screenshot of the wrong size")?;
        image.save(out.join(format!("{name}.png")))?;
        Ok(())
    };
    let mut bed = Bed::start(&shell)?;
    bed.ask(
        "password",
        vpn(&[("username", "alex"), ("password-flags", "2")], &[]),
        "vpn",
        &[],
        ALLOW_INTERACTION,
    )?;
    bed.dialog()?;
    bed.fill("Mot de passe", "panda")?;
    save(&mut bed, "vpn-password")?;
    bed.key(KEY_ESC);
    bed.gone()?;
    bed.keep("password", "motdepasse")?;
    bed.ask(
        "code",
        vpn(
            &[
                ("username", "alex"),
                ("password-flags", "1"),
                ("static-challenge", "Code, true"),
            ],
            &[],
        ),
        "vpn",
        &[],
        ALLOW_INTERACTION,
    )?;
    bed.dialog()?;
    bed.logged_after(0, "\"Code à usage unique: 0\"")?;
    save(&mut bed, "vpn-code")?;
    bed.key(KEY_ESC);
    bed.gone()?;
    let wifi: Settings = HashMap::from([
        (
            "connection".to_owned(),
            HashMap::from([
                ("id".to_owned(), value("Café Lomi")),
                ("uuid".to_owned(), value(UUID)),
                ("type".to_owned(), value("802-11-wireless")),
            ]),
        ),
        (
            "802-11-wireless-security".to_owned(),
            HashMap::from([
                ("key-mgmt".to_owned(), value("wpa-psk")),
                ("psk-flags".to_owned(), value(1_u32)),
            ]),
        ),
    ]);
    bed.ask(
        "wifi",
        wifi,
        "802-11-wireless-security",
        &[],
        ALLOW_INTERACTION | REQUEST_NEW,
    )?;
    bed.dialog()?;
    bed.logged_after(0, "Le réseau Wi-Fi demande son mot de passe")?;
    save(&mut bed, "wifi-refused")?;
    Ok(())
}
