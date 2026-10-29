//! What the connections show: the network as NetworkManager last said it
//! ([`Network`]), the bar's row for it ([`Link`]), the networks around,
//! one per name ([`listed`]), and the words, in English and French.

use crate::levels::model::Words;

/// What a network asks to join, from its access points' flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// Nothing (or OWE, "enhanced open", which asks nothing either).
    Open,
    /// A password: WPA-PSK, or WPA3's SAE (`sae`: SAE only, no PSK).
    Psk { sae: bool },
    /// 802.1X (eduroam and companies'): set up in the Settings.
    Enterprise,
    /// WEP, from before WPA: left to the Settings too.
    Wep,
}

/// NetworkManager's access point flags (`NM80211ApFlags`,
/// `NM80211ApSecurityFlags`).
const PRIVACY: u32 = 0x1;
const KEY_MGMT_PSK: u32 = 0x100;
const KEY_MGMT_8021X: u32 = 0x200;
const KEY_MGMT_SAE: u32 = 0x400;
const KEY_MGMT_EAP_SUITE_B: u32 = 0x2000;

impl Security {
    /// From an access point's `Flags`, `WpaFlags` and `RsnFlags`.
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        let both = wpa | rsn;
        if both & (KEY_MGMT_8021X | KEY_MGMT_EAP_SUITE_B) != 0 {
            Self::Enterprise
        } else if both & KEY_MGMT_PSK != 0 {
            Self::Psk { sae: false }
        } else if both & KEY_MGMT_SAE != 0 {
            Self::Psk { sae: true }
        } else if both == 0 && flags & PRIVACY != 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    /// Whether a lock shows beside it.
    pub fn locked(self) -> bool {
        self != Self::Open
    }

    /// Whether the popover can join it (else: the Settings).
    pub fn joinable(self) -> bool {
        matches!(self, Self::Open | Self::Psk { .. })
    }
}

/// How many of the icon's four parts (the dot and three arcs) a strength
/// lights, NetworkManager's 0 to 100: 70 and up all four, 50 three, 30
/// two, below that the dot alone.
pub fn bars(strength: u8) -> u8 {
    match strength {
        70.. => 4,
        50..=69 => 3,
        30..=49 => 2,
        _ => 1,
    }
}

/// An access point as NetworkManager lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    /// Its object's path.
    pub path: String,
    pub ssid: Vec<u8>,
    pub strength: u8,
    pub security: Security,
}

/// A network around, one per name: its strongest access point's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wifi {
    pub ssid: Vec<u8>,
    /// Its name as shown (an SSID is bytes, mostly UTF-8).
    pub name: String,
    pub strength: u8,
    pub security: Security,
    /// A connection is saved for it.
    pub saved: bool,
    /// The strongest access point, to connect through.
    pub ap: String,
}

impl Wifi {
    pub fn bars(&self) -> u8 {
        bars(self.strength)
    }
}

/// The name an SSID shows.
pub fn name_of(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid).into_owned()
}

/// The networks around, from `aps`: one per SSID (its strongest access
/// point's), hidden ones (no SSID) left out, `saved` those with a saved
/// connection; strongest first, then by name.
pub fn listed(aps: &[AccessPoint], saved: impl Fn(&[u8]) -> bool) -> Vec<Wifi> {
    let mut networks: Vec<Wifi> = Vec::new();
    for ap in aps.iter().filter(|ap| !ap.ssid.iter().all(|b| *b == 0)) {
        match networks.iter_mut().find(|n| n.ssid == ap.ssid) {
            Some(known) if known.strength >= ap.strength => {}
            Some(known) => {
                known.strength = ap.strength;
                known.security = ap.security;
                known.ap.clone_from(&ap.path);
            }
            None => networks.push(Wifi {
                ssid: ap.ssid.clone(),
                name: name_of(&ap.ssid),
                strength: ap.strength,
                security: ap.security,
                saved: saved(&ap.ssid),
                ap: ap.path.clone(),
            }),
        }
    }
    networks.sort_by(|a, b| {
        b.strength
            .cmp(&a.strength)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    networks
}

/// Why a connection the popover asked for did not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// NetworkManager wanted a secret it did not get, or the network
    /// refused the one given: the password is wrong.
    Secret,
    /// Anything else (out of reach, no address, a timeout, refused).
    Other,
}

/// A connection the popover asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Each asked has its own number: the popover reacts once to each end.
    pub id: u64,
    pub ssid: Vec<u8>,
    pub state: Progress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Connecting,
    Connected,
    Failed(Failure),
}

/// The network as NetworkManager last said it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Network {
    /// NetworkManager is there: else the row is hidden, as a level's whose
    /// service is missing.
    pub manager: bool,
    /// A Wi-Fi device is there.
    pub hardware: bool,
    /// NetworkManager's `WirelessEnabled` (and the radio not blocked).
    pub enabled: bool,
    /// A wired device is connected.
    pub wired: bool,
    /// The Wi-Fi network connected, if any.
    pub current: Option<Wifi>,
    /// The Wi-Fi device is on its way to a network.
    pub connecting: bool,
    /// NetworkManager found a sign-in page (`Connectivity` 2, portal).
    pub portal: bool,
    /// The address NetworkManager checks connectivity at: where a sign-in
    /// page shows itself.
    pub check_uri: Option<String>,
    /// The networks around, strongest first, the current one too.
    pub networks: Vec<Wifi>,
    /// The scan asked for runs.
    pub scanning: bool,
    /// The last connection the popover asked for.
    pub attempt: Option<Attempt>,
}

/// What the bar's row says (bar.md §4 group 9).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// No NetworkManager: no row.
    Hidden,
    Wired,
    Off,
    Offline,
    Connecting,
    Connected {
        name: String,
        bars: u8,
    },
}

impl Network {
    /// The bar's row: a cable first; then the Wi-Fi off, on its way, on a
    /// network, or on none.
    pub fn link(&self) -> Link {
        if !self.manager {
            Link::Hidden
        } else if self.wired {
            Link::Wired
        } else if !self.hardware {
            Link::Offline
        } else if !self.enabled {
            Link::Off
        } else if let Some(current) = &self.current {
            Link::Connected {
                name: current.name.clone(),
                bars: current.bars(),
            }
        } else if self.connecting {
            Link::Connecting
        } else {
            Link::Offline
        }
    }

    /// The dot on the row's icon: a sign-in page waits.
    pub fn warns(&self) -> bool {
        self.portal && self.manager
    }

    /// The networks the popover lists: all but the current one.
    pub fn others(&self) -> impl Iterator<Item = &Wifi> {
        let current = self.current.as_ref().map(|c| c.ssid.as_slice());
        self.networks
            .iter()
            .filter(move |n| Some(n.ssid.as_slice()) != current)
    }

    /// The attempt under way, if one is.
    pub fn connecting_to(&self) -> Option<&[u8]> {
        self.attempt
            .as_ref()
            .filter(|a| a.state == Progress::Connecting)
            .map(|a| a.ssid.as_slice())
    }
}

/// The words of the connections and the Wi-Fi's popover.
impl Words {
    pub fn wifi(self) -> &'static str {
        "Wi-Fi"
    }

    pub fn connecting(self) -> &'static str {
        if self.fr() {
            "Connexion…"
        } else {
            "Connecting…"
        }
    }

    pub fn offline(self) -> &'static str {
        if self.fr() { "Hors ligne" } else { "Offline" }
    }

    pub fn wifi_off(self) -> &'static str {
        if self.fr() {
            "Wi-Fi coupé"
        } else {
            "Wi-Fi off"
        }
    }

    pub fn wired(self) -> &'static str {
        if self.fr() { "Câble" } else { "Wired" }
    }

    pub fn by_cable(self) -> &'static str {
        if self.fr() {
            "Connecté par câble"
        } else {
            "Connected by cable"
        }
    }

    pub fn wifi_is_off(self) -> &'static str {
        if self.fr() {
            "Le Wi-Fi est coupé."
        } else {
            "Wi-Fi is off."
        }
    }

    pub fn no_adapter(self) -> &'static str {
        if self.fr() {
            "Aucune carte Wi-Fi."
        } else {
            "No Wi-Fi adapter."
        }
    }

    pub fn connected(self) -> &'static str {
        if self.fr() { "Connecté" } else { "Connected" }
    }

    pub fn sign_in_waits(self) -> &'static str {
        if self.fr() {
            "Page de connexion à ouvrir"
        } else {
            "Sign-in page to open"
        }
    }

    pub fn disconnect(self) -> &'static str {
        if self.fr() {
            "Déconnecter"
        } else {
            "Disconnect"
        }
    }

    pub fn open_sign_in(self) -> &'static str {
        if self.fr() {
            "Ouvrir la page de connexion"
        } else {
            "Open the sign-in page"
        }
    }

    /// Over the networks, in capitals.
    pub fn networks(self) -> &'static str {
        if self.fr() { "RÉSEAUX" } else { "NETWORKS" }
    }

    pub fn searching(self) -> &'static str {
        if self.fr() {
            "Recherche…"
        } else {
            "Searching…"
        }
    }

    pub fn searching_networks(self) -> &'static str {
        if self.fr() {
            "Recherche des réseaux…"
        } else {
            "Searching for networks…"
        }
    }

    pub fn no_network(self) -> &'static str {
        if self.fr() {
            "Aucun réseau trouvé."
        } else {
            "No network found."
        }
    }

    pub fn saved(self) -> &'static str {
        if self.fr() { "enregistré" } else { "saved" }
    }

    pub fn in_settings(self) -> &'static str {
        if self.fr() {
            "dans les Réglages"
        } else {
            "in Settings"
        }
    }

    /// A row whose connection failed, not for its password.
    pub fn failed(self) -> &'static str {
        if self.fr() { "échec" } else { "failed" }
    }

    pub fn password(self) -> &'static str {
        if self.fr() {
            "Mot de passe"
        } else {
            "Password"
        }
    }

    pub fn cancel(self) -> &'static str {
        if self.fr() { "Annuler" } else { "Cancel" }
    }

    pub fn join(self) -> &'static str {
        if self.fr() { "Se connecter" } else { "Connect" }
    }

    pub fn wrong_password(self) -> &'static str {
        if self.fr() {
            "Mot de passe incorrect. Vérifie-le et réessaie."
        } else {
            "Wrong password. Check it and try again."
        }
    }

    pub fn could_not_connect(self) -> &'static str {
        if self.fr() {
            "La connexion a échoué. Réessaie."
        } else {
            "Could not connect. Try again."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    fn ap(path: &str, ssid: &str, strength: u8, security: Security) -> AccessPoint {
        AccessPoint {
            path: path.to_owned(),
            ssid: ssid.as_bytes().to_vec(),
            strength,
            security,
        }
    }

    const PSK: Security = Security::Psk { sae: false };

    #[test]
    fn the_flags_say_what_a_network_asks() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        // WPA2-PSK, as most home routers.
        assert_eq!(Security::from_flags(1, 0, 0x188), PSK);
        // WPA3 alone, and in transition with WPA2.
        assert_eq!(
            Security::from_flags(1, 0, 0x488),
            Security::Psk { sae: true }
        );
        assert_eq!(Security::from_flags(1, 0, 0x588), PSK);
        // eduroam.
        assert_eq!(Security::from_flags(1, 0x288, 0x288), Security::Enterprise);
        assert_eq!(Security::from_flags(1, 0, 0), Security::Wep);
        // OWE (enhanced open) asks nothing.
        assert_eq!(Security::from_flags(1, 0, 0x888), Security::Open);
        assert!(!Security::Open.locked() && Security::Enterprise.locked());
        assert!(PSK.joinable() && !Security::Wep.joinable());
    }

    #[test]
    fn strengths_light_one_to_four_parts() {
        assert_eq!(bars(100), 4);
        assert_eq!(bars(70), 4);
        assert_eq!(bars(69), 3);
        assert_eq!(bars(50), 3);
        assert_eq!(bars(49), 2);
        assert_eq!(bars(30), 2);
        assert_eq!(bars(29), 1);
        assert_eq!(bars(0), 1);
    }

    #[test]
    fn networks_are_one_per_name_strongest_first_without_hidden_ones() {
        let aps = [
            ap("/1", "Bbox-55D1", 20, PSK),
            ap("/2", "Livebox-A3F2", 60, PSK),
            ap("/3", "", 90, PSK),
            ap("/4", "Livebox-A3F2", 85, Security::Psk { sae: true }),
            ap("/5", "eduroam", 40, Security::Enterprise),
            ap("/6", "_SNCF_WIFI_INOUI", 60, Security::Open),
            ap("/7", "\0\0\0", 70, Security::Open),
        ];
        let listed = listed(&aps, |ssid| ssid == b"Bbox-55D1");
        let names: Vec<&str> = listed.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(
            names,
            ["Livebox-A3F2", "_SNCF_WIFI_INOUI", "eduroam", "Bbox-55D1"]
        );
        // The strongest access point's, for its strength, its security
        // and to connect through.
        assert_eq!(listed[0].strength, 85);
        assert_eq!(listed[0].ap, "/4");
        assert_eq!(listed[0].security, Security::Psk { sae: true });
        assert!(listed[3].saved && !listed[0].saved);
    }

    fn network(link: impl FnOnce(&mut Network)) -> Network {
        let mut network = Network {
            manager: true,
            hardware: true,
            enabled: true,
            ..Network::default()
        };
        link(&mut network);
        network
    }

    #[test]
    fn the_row_says_cable_off_connecting_connected_or_offline() {
        let livebox = listed(&[ap("/2", "Livebox-A3F2", 72, PSK)], |_| true).remove(0);
        assert_eq!(Network::default().link(), Link::Hidden);
        assert_eq!(network(|_| {}).link(), Link::Offline);
        assert_eq!(network(|n| n.enabled = false).link(), Link::Off);
        assert_eq!(network(|n| n.connecting = true).link(), Link::Connecting);
        assert_eq!(
            network(|n| n.current = Some(livebox.clone())).link(),
            Link::Connected {
                name: "Livebox-A3F2".to_owned(),
                bars: 4
            }
        );
        // A cable first, whatever the Wi-Fi does.
        assert_eq!(
            network(|n| {
                n.wired = true;
                n.current = Some(livebox.clone());
            })
            .link(),
            Link::Wired
        );
        // No Wi-Fi adapter, no cable: offline.
        assert_eq!(network(|n| n.hardware = false).link(), Link::Offline);
        assert!(network(|n| n.portal = true).warns());
    }

    #[test]
    fn the_words_in_both_languages() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.offline(), "Hors ligne");
        assert_eq!(en.offline(), "Offline");
        assert_eq!(fr.networks(), "RÉSEAUX");
        assert_eq!(
            fr.wrong_password(),
            "Mot de passe incorrect. Vérifie-le et réessaie."
        );
        assert_eq!(en.join(), "Connect");
    }
}
