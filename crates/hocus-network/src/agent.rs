//! NetworkManager's secret agent, its logic (docs/design/polkit.md, "NetworkManager's secrets"):
//! what a `GetSecrets` asks, read from the connection NetworkManager sends
//! (its settings, the system's secrets it holds, each secret's flags and
//! the hints a VPN's plugin gives); what may be answered without asking;
//! the reply; and what the agent keeps in the session's keyring. The shell
//! serves the interface and asks in its dialog; nothing here touches a
//! bus.
//!
//! Our own reading of NetworkManager's documented D-Bus interfaces
//! (`org.freedesktop.NetworkManager.SecretAgent`, `…AgentManager`, the
//! settings' `*-flags`) and of the keyring's attributes libnm's agents use,
//! so that what another desktop's agent kept is found; no code of
//! NetworkManager's (GPL and LGPL) is used.

use std::collections::HashMap;

use zbus::zvariant::{OwnedVariant, Variant};

pub type Props = HashMap<String, OwnedVariant>;
pub type Settings = HashMap<String, Props>;

/// Where the agent answers: NetworkManager calls this path alone.
pub const PATH: &str = "/org/freedesktop/NetworkManager/SecretAgent";
pub const INTERFACE: &str = "org.freedesktop.NetworkManager.SecretAgent";
pub const MANAGER: &str = "org.freedesktop.NetworkManager.AgentManager";
pub const MANAGER_PATH: &str = "/org/freedesktop/NetworkManager/AgentManager";
/// The agent's name for NetworkManager.
pub const IDENTIFIER: &str = "org.hocuspanda.shell";
/// The agent takes a VPN plugin's hints (what it asks, its message).
pub const VPN_HINTS: u32 = 0x1;

/// `GetSecrets`' flags: the user may be asked; the secrets held were
/// refused, new ones are asked for.
pub const ALLOW_INTERACTION: u32 = 0x1;
pub const REQUEST_NEW: u32 = 0x2;

/// A secret's flags (`KEY-flags`): kept by an agent, never kept (asked
/// each time), not needed. None of them: NetworkManager keeps it.
pub const AGENT_OWNED: u32 = 0x1;
pub const NOT_SAVED: u32 = 0x2;
pub const NOT_REQUIRED: u32 = 0x4;

/// The keyring's schema for a connection's secret, libnm's.
pub const SCHEMA: &str = "org.freedesktop.NetworkManager.Connection";

/// A VPN plugin's message among its hints.
const MESSAGE_HINT: &str = "x-vpn-message:";

/// What a secret is, for its field's label and whether it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum What {
    /// A user name: shown as typed.
    User,
    Password,
    /// A certificate's or a key's password.
    KeyPassword,
    /// A one-time code: shown as typed, never kept.
    Code,
    /// Anything else, by its key: masked.
    Other,
}

impl What {
    fn of(key: &str) -> Self {
        let k = key.to_ascii_lowercase();
        if k.contains("challenge")
            || k.contains("otp")
            || k.contains("token")
            || k.ends_with("code")
        {
            Self::Code
        } else if k == "user"
            || k == "identity"
            || k.ends_with("username")
            || k.contains("user name")
        {
            Self::User
        } else if k.contains("cert-pass")
            || k.contains("key-pass")
            || k.contains("private-key-password")
        {
            Self::KeyPassword
        } else if k.contains("pass")
            || k == "psk"
            || k.starts_with("wep-key")
            || k.contains("secret")
        {
            Self::Password
        } else {
            Self::Other
        }
    }

    /// Whether its field masks what is typed.
    pub fn masked(self) -> bool {
        !matches!(self, Self::User | Self::Code)
    }
}

/// Who keeps a secret, from its flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Keep {
    /// NetworkManager, in the connection's profile.
    System,
    /// The agent: the session's keyring.
    Agent,
    /// Nobody: asked each time.
    Never,
}

impl Keep {
    /// `None`: not needed, not asked.
    fn of(flags: u32) -> Option<Self> {
        if flags & NOT_REQUIRED != 0 {
            None
        } else if flags & NOT_SAVED != 0 {
            Some(Self::Never)
        } else if flags & AGENT_OWNED != 0 {
            Some(Self::Agent)
        } else {
            Some(Self::System)
        }
    }
}

/// A secret asked for.
#[derive(Clone, PartialEq, Eq)]
pub struct Wanted {
    pub key: String,
    pub what: What,
    pub keep: Keep,
    /// What is held: the system's secret NetworkManager sent, or the
    /// keyring's.
    pub known: Option<String>,
}

impl std::fmt::Debug for Wanted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wanted")
            .field("key", &self.key)
            .field("what", &self.what)
            .field("keep", &self.keep)
            .field("known", &self.known.is_some())
            .finish()
    }
}

impl Wanted {
    /// Whether the keyring may hold it: the agent's, not a code.
    pub fn in_keyring(&self) -> bool {
        self.keep == Keep::Agent && self.what != What::Code
    }
}

/// What kind of connection asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Vpn,
    Wifi,
    Other,
}

/// A `GetSecrets`, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    pub uuid: String,
    /// The connection's name (`connection.id`).
    pub name: String,
    pub setting: String,
    pub kind: Kind,
    /// A VPN plugin's message, if it gave one.
    pub message: Option<String>,
    pub wanted: Vec<Wanted>,
    pub flags: u32,
}

impl Asked {
    pub fn interactive(&self) -> bool {
        self.flags & ALLOW_INTERACTION != 0
    }

    /// The secrets held were refused: none is answered from what is held.
    pub fn new_ones(&self) -> bool {
        self.flags & REQUEST_NEW != 0
    }

    /// The keys the keyring is asked for.
    pub fn from_keyring(&self) -> Vec<String> {
        if self.new_ones() {
            return Vec::new();
        }
        self.wanted
            .iter()
            .filter(|w| w.in_keyring() && w.known.is_none())
            .map(|w| w.key.clone())
            .collect()
    }

    /// Whether everything asked is held: answered without a dialog.
    pub fn answered(&self) -> bool {
        !self.new_ones() && self.wanted.iter().all(|w| w.known.is_some())
    }

    /// A Wi-Fi network whose secrets NetworkManager keeps: its password is
    /// the bar's popover's and the Settings' (a refused one unfolds their
    /// field), never this dialog's.
    pub fn left_to_the_popover(&self) -> bool {
        self.kind == Kind::Wifi && self.wanted.iter().all(|w| w.keep == Keep::System)
    }

    /// Whether "Retenir le mot de passe" shows: a secret the agent may keep.
    pub fn rememberable(&self) -> bool {
        self.wanted.iter().any(Wanted::in_keyring)
    }
}

/// What `GetSecrets` asks of `setting`, from `connection` (with the
/// system's secrets NetworkManager holds), the plugin's `hints` and its
/// `flags`. Codes are never taken from what is held.
pub fn asked(connection: &Settings, setting: &str, hints: &[String], flags: u32) -> Asked {
    let text = |group: &str, key: &str| {
        connection
            .get(group)
            .and_then(|g| g.get(key))
            .and_then(|v| v.downcast_ref::<String>().ok())
            .unwrap_or_default()
    };
    let kind = match text("connection", "type").as_str() {
        "vpn" | "wireguard" => Kind::Vpn,
        "802-11-wireless" => Kind::Wifi,
        _ => Kind::Other,
    };
    let message = hints
        .iter()
        .find_map(|h| h.strip_prefix(MESSAGE_HINT))
        .map(|m| m.trim().to_owned())
        .filter(|m| !m.is_empty());
    let named: Vec<&str> = hints
        .iter()
        .map(String::as_str)
        .filter(|h| !h.starts_with("x-vpn-") && !h.is_empty())
        .collect();
    let props = connection.get(setting).cloned().unwrap_or_default();
    let wanted = if setting == "vpn" {
        vpn_wanted(&props, &named)
    } else {
        let keys: Vec<String> = if named.is_empty() {
            keys_of(setting, &props)
        } else {
            named.iter().map(|k| (*k).to_owned()).collect()
        };
        keys.into_iter()
            .filter_map(|key| {
                let flags = props
                    .get(&flags_key(&key))
                    .and_then(|v| v.downcast_ref::<u32>().ok())
                    .unwrap_or(0);
                let keep = Keep::of(flags)?;
                let what = What::of(&key);
                let known = props
                    .get(&key)
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .filter(|v| !v.is_empty() && what != What::Code && keep != Keep::Never);
                Some(Wanted {
                    key,
                    what,
                    keep,
                    known,
                })
            })
            .collect()
    };
    Asked {
        uuid: text("connection", "uuid"),
        name: text("connection", "id"),
        setting: setting.to_owned(),
        kind,
        message,
        wanted,
        flags,
    }
}

/// A key's flags' key: WEP's four keys share theirs.
fn flags_key(key: &str) -> String {
    if key.starts_with("wep-key") {
        "wep-key-flags".to_owned()
    } else {
        format!("{key}-flags")
    }
}

/// The secrets a setting holds, by what it is.
fn keys_of(setting: &str, props: &Props) -> Vec<String> {
    let text = |key: &str| {
        props
            .get(key)
            .and_then(|v| v.downcast_ref::<String>().ok())
            .unwrap_or_default()
    };
    match setting {
        "802-11-wireless-security" => match text("key-mgmt").as_str() {
            "wpa-psk" | "sae" => vec!["psk".to_owned()],
            "none" => {
                let index = props
                    .get("wep-tx-keyidx")
                    .and_then(|v| v.downcast_ref::<u32>().ok())
                    .unwrap_or(0)
                    .min(3);
                vec![format!("wep-key{index}")]
            }
            "ieee8021x" if text("auth-alg") == "leap" => vec!["leap-password".to_owned()],
            _ => Vec::new(),
        },
        "802-1x" => {
            let methods = props
                .get("eap")
                .and_then(|v| v.try_clone().ok())
                .and_then(|v| Vec::<String>::try_from(v).ok())
                .unwrap_or_default();
            if methods.iter().all(|m| m == "tls") && !methods.is_empty() {
                vec!["private-key-password".to_owned()]
            } else {
                vec!["password".to_owned()]
            }
        }
        "gsm" | "cdma" | "pppoe" | "adsl" => vec!["password".to_owned()],
        "wireguard" => vec!["private-key".to_owned()],
        _ => Vec::new(),
    }
}

/// A VPN's secrets: those its plugin names in its hints, else those its
/// data gives flags to (`KEY-flags`), and OpenVPN's static challenge's
/// answer. Its data's values are text, its flags too.
fn vpn_wanted(props: &Props, named: &[&str]) -> Vec<Wanted> {
    let dict = |key: &str| {
        props
            .get(key)
            .and_then(|v| v.try_clone().ok())
            .and_then(|v| HashMap::<String, String>::try_from(v).ok())
            .unwrap_or_default()
    };
    let (data, secrets) = (dict("data"), dict("secrets"));
    let flags = |key: &str| {
        data.get(&flags_key(key))
            .and_then(|f| f.trim().parse::<u32>().ok())
            .unwrap_or(0)
    };
    let mut keys: Vec<String> = if named.is_empty() {
        let mut keys: Vec<String> = data
            .keys()
            .filter_map(|k| k.strip_suffix("-flags"))
            .map(str::to_owned)
            .collect();
        keys.sort_by_key(|k| order(k));
        keys
    } else {
        named.iter().map(|k| (*k).to_owned()).collect()
    };
    if named.is_empty()
        && data.contains_key("static-challenge")
        && !keys.iter().any(|k| k == "challenge-response")
    {
        keys.push("challenge-response".to_owned());
    }
    keys.into_iter()
        .filter_map(|key| {
            let what = What::of(&key);
            let keep = if what == What::Code {
                Keep::Never
            } else {
                Keep::of(flags(&key))?
            };
            let known = secrets
                .get(&key)
                .filter(|v| !v.is_empty() && what != What::Code && keep != Keep::Never)
                .cloned();
            Some(Wanted {
                key,
                what,
                keep,
                known,
            })
        })
        .collect()
}

/// The fields' order: the user's name, the password, the others, the code.
fn order(key: &str) -> (u8, String) {
    let rank = match What::of(key) {
        What::User => 0,
        What::Password => 1,
        What::KeyPassword => 2,
        What::Other => 3,
        What::Code => 4,
    };
    (rank, key.to_owned())
}

/// The reply to `GetSecrets` for `setting`: its keys and values, a VPN's
/// under `secrets`.
pub fn reply(setting: &str, values: &[(String, String)]) -> Settings {
    let mut props = Props::new();
    if setting == "vpn" {
        let secrets: HashMap<String, String> = values.iter().cloned().collect();
        if let Ok(value) = OwnedVariant::try_from(Variant::from(secrets)) {
            props.insert("secrets".to_owned(), value);
        }
    } else {
        for (key, value) in values {
            if let Ok(value) = OwnedVariant::try_from(Variant::from(value.as_str())) {
                props.insert(key.clone(), value);
            }
        }
    }
    HashMap::from([(setting.to_owned(), props)])
}

/// A secret the agent keeps, as `SaveSecrets` hands it.
#[derive(Clone, PartialEq, Eq)]
pub struct Owned {
    pub setting: String,
    pub key: String,
    pub value: String,
}

impl std::fmt::Debug for Owned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Owned({}/{})", self.setting, self.key)
    }
}

/// The agent-owned secrets `connection` carries, in every setting that
/// holds secrets: what `SaveSecrets` keeps.
pub fn agent_owned(connection: &Settings) -> Vec<Owned> {
    let mut owned = Vec::new();
    for (setting, props) in connection {
        if setting == "vpn" {
            for wanted in vpn_wanted(props, &[]) {
                if let (true, Some(value)) = (wanted.in_keyring(), wanted.known) {
                    owned.push(Owned {
                        setting: setting.clone(),
                        key: wanted.key,
                        value,
                    });
                }
            }
            continue;
        }
        for (key, value) in props {
            let Some(flags) = props
                .get(&flags_key(key))
                .filter(|_| !key.ends_with("-flags"))
                .and_then(|v| v.downcast_ref::<u32>().ok())
            else {
                continue;
            };
            let Ok(value) = value.downcast_ref::<String>() else {
                continue;
            };
            if Keep::of(flags) == Some(Keep::Agent) && !value.is_empty() {
                owned.push(Owned {
                    setting: setting.clone(),
                    key: key.clone(),
                    value,
                });
            }
        }
    }
    owned
}

/// The connection's UUID, from its settings.
pub fn uuid_of(connection: &Settings) -> Option<String> {
    connection
        .get("connection")?
        .get("uuid")?
        .downcast_ref::<String>()
        .ok()
        .filter(|u| !u.is_empty())
}

/// The keyring's attributes for a secret, libnm's.
pub fn attributes(uuid: &str, setting: &str, key: &str) -> HashMap<String, String> {
    HashMap::from([
        ("xdg:schema".to_owned(), SCHEMA.to_owned()),
        ("connection-uuid".to_owned(), uuid.to_owned()),
        ("setting-name".to_owned(), setting.to_owned()),
        ("setting-key".to_owned(), key.to_owned()),
    ])
}

/// The keyring's attributes for every secret of a connection.
pub fn connection_attributes(uuid: &str) -> HashMap<String, String> {
    HashMap::from([
        ("xdg:schema".to_owned(), SCHEMA.to_owned()),
        ("connection-uuid".to_owned(), uuid.to_owned()),
    ])
}

/// The keyring's label for a secret, libnm's.
pub fn label(name: &str, setting: &str, key: &str) -> String {
    format!("Network secret for {name}/{setting}/{key}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v<'a>(value: impl Into<Variant<'a>>) -> OwnedVariant {
        OwnedVariant::try_from(value.into()).expect("a value")
    }

    fn vpn(data: &[(&str, &str)], secrets: &[(&str, &str)]) -> Settings {
        let map = |pairs: &[(&str, &str)]| -> HashMap<String, String> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect()
        };
        HashMap::from([
            (
                "connection".to_owned(),
                HashMap::from([
                    ("id".to_owned(), v("VPN Bureau")),
                    ("uuid".to_owned(), v("u-1")),
                    ("type".to_owned(), v("vpn")),
                ]),
            ),
            (
                "vpn".to_owned(),
                HashMap::from([
                    ("data".to_owned(), v(map(data))),
                    ("secrets".to_owned(), v(map(secrets))),
                ]),
            ),
        ])
    }

    fn wifi(psk_flags: u32, psk: Option<&str>) -> Settings {
        let mut security = HashMap::from([
            ("key-mgmt".to_owned(), v("wpa-psk")),
            ("psk-flags".to_owned(), v(psk_flags)),
        ]);
        if let Some(psk) = psk {
            security.insert("psk".to_owned(), v(psk));
        }
        HashMap::from([
            (
                "connection".to_owned(),
                HashMap::from([
                    ("id".to_owned(), v("Café Lomi")),
                    ("uuid".to_owned(), v("u-2")),
                    ("type".to_owned(), v("802-11-wireless")),
                ]),
            ),
            ("802-11-wireless-security".to_owned(), security),
        ])
    }

    #[test]
    fn a_vpn_asks_what_its_data_gives_flags_to() {
        let connection = vpn(
            &[
                ("username", "alex"),
                ("password-flags", "1"),
                ("cert-pass-flags", "4"),
                ("static-challenge", "Code, true"),
            ],
            &[],
        );
        let asked = asked(&connection, "vpn", &[], ALLOW_INTERACTION);
        assert_eq!(asked.name, "VPN Bureau");
        assert_eq!(asked.kind, Kind::Vpn);
        let keys: Vec<_> = asked
            .wanted
            .iter()
            .map(|w| (w.key.as_str(), w.what, w.keep))
            .collect();
        assert_eq!(
            keys,
            [
                ("password", What::Password, Keep::Agent),
                ("challenge-response", What::Code, Keep::Never),
            ]
        );
        assert!(asked.rememberable());
        assert_eq!(asked.from_keyring(), ["password"]);
        assert!(!asked.answered());
    }

    #[test]
    fn hints_say_what_a_plugin_asks_and_its_message() {
        let connection = vpn(&[("password-flags", "2")], &[("password", "old")]);
        let hints = [
            "x-vpn-message:Entrez le code".to_owned(),
            "challenge-response".to_owned(),
        ];
        let asked = asked(&connection, "vpn", &hints, ALLOW_INTERACTION);
        assert_eq!(asked.message.as_deref(), Some("Entrez le code"));
        assert_eq!(asked.wanted.len(), 1);
        assert_eq!(asked.wanted[0].what, What::Code);
        assert!(!asked.rememberable());
    }

    #[test]
    fn what_the_system_holds_answers_unless_new_ones_are_asked() {
        let connection = vpn(&[("password-flags", "0")], &[("password", "kept")]);
        let held = asked(&connection, "vpn", &[], ALLOW_INTERACTION);
        assert!(held.answered());
        assert!(!held.rememberable());
        let refused = asked(&connection, "vpn", &[], ALLOW_INTERACTION | REQUEST_NEW);
        assert!(!refused.answered());
        assert!(refused.from_keyring().is_empty());
        // Never kept: asked each time.
        let never = asked(
            &vpn(&[("password-flags", "2")], &[("password", "x")]),
            "vpn",
            &[],
            ALLOW_INTERACTION,
        );
        assert!(!never.answered());
    }

    #[test]
    fn a_wifi_password_the_system_keeps_is_the_popovers() {
        let system = asked(&wifi(0, None), "802-11-wireless-security", &[], 3);
        assert_eq!(system.kind, Kind::Wifi);
        assert_eq!(system.wanted[0].key, "psk");
        assert!(system.left_to_the_popover());
        let agent = asked(&wifi(1, None), "802-11-wireless-security", &[], 1);
        assert!(!agent.left_to_the_popover());
        assert!(agent.rememberable());
    }

    #[test]
    fn the_reply_puts_a_vpns_secrets_under_secrets() {
        let reply = reply("vpn", &[("password".to_owned(), "panda".to_owned())]);
        let secrets = reply["vpn"]["secrets"]
            .try_clone()
            .ok()
            .and_then(|v| HashMap::<String, String>::try_from(v).ok())
            .expect("a dictionary");
        assert_eq!(secrets["password"], "panda");
        let wifi = super::reply(
            "802-11-wireless-security",
            &[("psk".to_owned(), "panda".to_owned())],
        );
        assert_eq!(
            wifi["802-11-wireless-security"]["psk"]
                .downcast_ref::<String>()
                .ok()
                .as_deref(),
            Some("panda")
        );
    }

    #[test]
    fn save_secrets_keeps_the_agents_own() {
        let mut owned = agent_owned(&vpn(
            &[("password-flags", "1"), ("cert-pass-flags", "2")],
            &[("password", "panda"), ("cert-pass", "x")],
        ));
        owned.extend(agent_owned(&wifi(1, Some("lomi1234"))));
        owned.extend(agent_owned(&wifi(0, Some("system"))));
        let keys: Vec<_> = owned
            .iter()
            .map(|o| (o.setting.as_str(), o.key.as_str(), o.value.as_str()))
            .collect();
        assert_eq!(
            keys,
            [
                ("vpn", "password", "panda"),
                ("802-11-wireless-security", "psk", "lomi1234"),
            ]
        );
        assert_eq!(format!("{:?}", owned[0]), "Owned(vpn/password)");
    }

    #[test]
    fn a_secrets_kind_is_read_from_its_key() {
        assert_eq!(What::of("password"), What::Password);
        assert_eq!(What::of("Xauth password"), What::Password);
        assert_eq!(What::of("form:main:username"), What::User);
        assert_eq!(What::of("cert-pass"), What::KeyPassword);
        assert_eq!(What::of("challenge-response"), What::Code);
        assert_eq!(What::of("psk"), What::Password);
        assert!(!What::Code.masked() && What::Password.masked());
    }
}
