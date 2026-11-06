//! The Réseau page's model (6.6, settings.md "Réseau"): what the search
//! finds on it, and how its rows say what NetworkManager says. Nothing of
//! it is in the file: the connections live in NetworkManager (its
//! profiles), airplane mode in rfkill. The window's side is
//! `app::network`.

use hocus_network::model::{Current, Details, Saved, Security, Wifi, Wired};

use crate::{
    lang::{Lang, Words, w},
    model::{Outside, SectionId},
};

const AIRPLANE: Words = w("MODE AVION", "AIRPLANE MODE");
const WIFI: Words = w("WI-FI", "WI-FI");
const CABLE: Words = w("CÂBLE", "CABLE");
const VPN: Words = w("VPN", "VPN");

/// The page's settings, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const fn o(
        id: &'static str,
        group: Words,
        name: Words,
        fr: &'static [&'static str],
        en: &'static [&'static str],
    ) -> Outside {
        Outside {
            id,
            section: SectionId::Network,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 9] = [
        o(
            "network.airplane",
            AIRPLANE,
            w("Mode avion", "Airplane mode"),
            &["avion", "radio", "hors ligne", "rfkill"],
            &["airplane", "flight mode", "radio", "rfkill"],
        ),
        o(
            "network.wifi",
            WIFI,
            w("Wi-Fi", "Wi-Fi"),
            &["sans fil", "wlan", "internet"],
            &["wireless", "wlan", "internet"],
        ),
        o(
            "network.saved",
            WIFI,
            w("Réseaux enregistrés", "Saved networks"),
            &["oublier", "supprimer", "mot de passe", "connus"],
            &["forget", "saved", "known", "password"],
        ),
        o(
            "network.hidden",
            WIFI,
            w("Réseau masqué", "Hidden network"),
            &["caché", "ssid"],
            &["hidden", "ssid"],
        ),
        o(
            "network.enterprise",
            WIFI,
            w("Réseau d’entreprise", "Enterprise network"),
            &[
                "802.1x",
                "eduroam",
                "eap",
                "peap",
                "ttls",
                "certificat",
                "radius",
            ],
            &[
                "enterprise",
                "802.1x",
                "eduroam",
                "eap",
                "peap",
                "ttls",
                "certificate",
                "radius",
            ],
        ),
        o("network.wep", WIFI, w("WEP", "WEP"), &["wep"], &["wep"]),
        o(
            "network.cable",
            CABLE,
            w("Câble", "Cable"),
            &["ethernet", "filaire", "rj45"],
            &["ethernet", "wired", "rj45"],
        ),
        o(
            "network.vpn",
            VPN,
            w("VPN", "VPN"),
            &["wireguard", "openvpn", "tunnel", "importer"],
            &["wireguard", "openvpn", "tunnel", "import"],
        ),
        o(
            "network.address",
            WIFI,
            w("Adresse IP", "IP address"),
            &["ip", "dns", "passerelle", "mac"],
            &["address", "gateway", "dns", "mac"],
        ),
    ];
    &ALL
}

/// "WPA2", "WPA3", "Ouvert", "Entreprise (802.1X)", "WEP".
pub fn security(security: Security, lang: Lang) -> &'static str {
    match security {
        Security::Psk { sae: false } => "WPA2",
        Security::Psk { sae: true } => "WPA3",
        Security::Open => w("Ouvert", "Open").get(lang),
        Security::Enterprise => w("Entreprise (802.1X)", "Enterprise (802.1X)").get(lang),
        Security::Wep => "WEP",
    }
}

/// A network around's line, and whether it warns (WEP): "WPA2 ·
/// enregistré", "Ouvert · une page de connexion peut s'afficher".
pub fn around(wifi: &Wifi, lang: Lang) -> (String, bool) {
    let mut line = match wifi.security {
        Security::Open => w(
            "Ouvert · une page de connexion peut s’afficher",
            "Open · a sign-in page may show",
        )
        .get(lang)
        .to_owned(),
        Security::Enterprise => w(
            "Entreprise (802.1X) · identifiant et mot de passe",
            "Enterprise (802.1X) · user name and password",
        )
        .get(lang)
        .to_owned(),
        Security::Wep => w(
            "WEP · une protection ancienne, facile à casser",
            "WEP · an old protection, easy to break",
        )
        .get(lang)
        .to_owned(),
        Security::Psk { .. } => security(wifi.security, lang).to_owned(),
    };
    if wifi.saved {
        line.push_str(w(" · enregistré", " · saved").get(lang));
    }
    (line, wifi.security == Security::Wep)
}

/// Whether joining `wifi` asks nothing more: open, or saved.
pub fn joins_at_once(wifi: &Wifi) -> bool {
    wifi.saved || wifi.security == Security::Open
}

/// "signal fort", "bon signal", "signal moyen", "signal faible".
pub fn signal(strength: u8, lang: Lang) -> &'static str {
    match strength {
        70.. => w("signal fort", "strong signal"),
        50..=69 => w("bon signal", "good signal"),
        30..=49 => w("signal moyen", "fair signal"),
        _ => w("signal faible", "weak signal"),
    }
    .get(lang)
}

/// "2,4 GHz", "5 GHz", "6 GHz", from a frequency in MHz.
pub fn band(frequency: u32, lang: Lang) -> &'static str {
    match frequency {
        0..3000 => w("2,4 GHz", "2.4 GHz").get(lang),
        3000..5925 => "5 GHz",
        _ => "6 GHz",
    }
}

/// The connected network's line: "Connecté · WPA2 · 5 GHz · signal fort".
pub fn connected(wifi: &Wifi, current: Option<&Current>, lang: Lang) -> String {
    let mut parts = vec![
        w("Connecté", "Connected").get(lang).to_owned(),
        security(wifi.security, lang).to_owned(),
    ];
    if let Some(frequency) = current.and_then(|c| c.frequency) {
        parts.push(band(frequency, lang).to_owned());
    }
    parts.push(signal(wifi.strength, lang).to_owned());
    parts.join(" · ")
}

/// "Adresse 192.168.1.24 · par la box 192.168.1.1".
pub fn address(current: &Current, lang: Lang) -> Option<String> {
    let address = current.address.as_deref()?;
    Some(match (current.gateway.as_deref(), lang) {
        (Some(gateway), Lang::French) => format!("Adresse {address} · par la box {gateway}"),
        (Some(gateway), Lang::English) => format!("Address {address} · via {gateway}"),
        (None, Lang::French) => format!("Adresse {address}"),
        (None, Lang::English) => format!("Address {address}"),
    })
}

/// When a saved network was last used, `now` seconds since the epoch:
/// "Il y a 2 h", "Hier", "Il y a 12 jours", "Jamais utilisé".
pub fn ago(timestamp: u64, now: u64, lang: Lang) -> String {
    if timestamp == 0 {
        return w("Jamais utilisé", "Never used").get(lang).to_owned();
    }
    let seconds = now.saturating_sub(timestamp);
    let (minutes, hours, days) = (seconds / 60, seconds / 3600, seconds / 86_400);
    match (lang, minutes, hours, days) {
        (Lang::French, 0, ..) => "À l’instant".to_owned(),
        (Lang::English, 0, ..) => "Just now".to_owned(),
        (Lang::French, m, 0, _) => format!("Il y a {m} min"),
        (Lang::English, m, 0, _) => format!("{m} min ago"),
        (Lang::French, _, h, 0) => format!("Il y a {h} h"),
        (Lang::English, _, h, 0) => format!("{h} h ago"),
        (Lang::French, .., 1) => "Hier".to_owned(),
        (Lang::English, .., 1) => "Yesterday".to_owned(),
        (Lang::French, .., d) => format!("Il y a {d} jours"),
        (Lang::English, .., d) => format!("{d} days ago"),
    }
}

/// A saved network's line: "Connecté maintenant · WPA2", "Il y a 2 h ·
/// WPA2", "Il y a 12 jours · ouvert", "Jamais utilisé".
pub fn saved_line(saved: &Saved, now: u64, lang: Lang) -> String {
    let when = if saved.connected {
        w("Connecté maintenant", "Connected now")
            .get(lang)
            .to_owned()
    } else {
        ago(saved.timestamp, now, lang)
    };
    let kind = match saved.security {
        Security::Open => w("ouvert", "open").get(lang),
        Security::Enterprise => "802.1X",
        other => security(other, lang),
    };
    format!("{when} · {kind}")
}

/// The saved networks as the sub-page lists them: the one in use, then
/// the most recently used first.
pub fn by_recent(saved: &[Saved]) -> Vec<Saved> {
    let mut sorted = saved.to_vec();
    sorted.sort_by(|a, b| {
        b.connected
            .cmp(&a.connected)
            .then_with(|| b.timestamp.cmp(&a.timestamp))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    sorted
}

/// The cable's line: the device and its state, "Adaptateur Ethernet USB
/// · débranché".
pub fn wired_line(wired: &Wired, lang: Lang) -> String {
    let device = if wired.usb {
        w("Adaptateur Ethernet USB", "USB Ethernet adapter")
    } else {
        w("Port Ethernet", "Ethernet port")
    };
    let state = if wired.connected {
        w("connecté", "connected")
    } else {
        w("débranché", "unplugged")
    };
    format!("{} · {}", device.get(lang), state.get(lang))
}

/// The cable's value: "Débranché", or "Connecté · 1 Gb/s · 192.168.1.30".
pub fn wired_value(wired: &Wired, lang: Lang) -> String {
    if !wired.connected {
        return w("Débranché", "Unplugged").get(lang).to_owned();
    }
    let mut parts = vec![w("Connecté", "Connected").get(lang).to_owned()];
    if let Some(speed) = wired.speed {
        parts.push(match speed {
            s if s >= 1000 && s % 1000 == 0 => format!("{} Gb/s", s / 1000),
            s if s >= 1000 => {
                let gb = format!("{:.1}", f64::from(s) / 1000.0);
                let gb = if lang.fr() { gb.replace('.', ",") } else { gb };
                format!("{gb} Gb/s")
            }
            s => format!("{s} Mb/s"),
        });
    }
    if let Some(address) = &wired.address {
        parts.push(address.clone());
    }
    parts.join(" · ")
}

/// The home's card: "Wi-Fi · Maison", "Câble", "Hors ligne", "Mode
/// avion".
pub fn card(details: Option<&Details>, airplane: bool, lang: Lang) -> String {
    if airplane {
        return w("Mode avion", "Airplane mode").get(lang).to_owned();
    }
    let Some(details) = details.filter(|d| d.network.manager) else {
        return w("Inconnu", "Unknown").get(lang).to_owned();
    };
    if details.network.wired {
        return w("Câble", "Cable").get(lang).to_owned();
    }
    match &details.network.current {
        Some(current) => format!("Wi-Fi · {}", current.name),
        None => w("Hors ligne", "Offline").get(lang).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use hocus_network::model::Network;

    use super::*;

    fn wifi(name: &str, security: Security, saved: bool) -> Wifi {
        Wifi {
            ssid: name.as_bytes().to_vec(),
            name: name.to_owned(),
            strength: 72,
            security,
            saved,
            ap: "/1".to_owned(),
        }
    }

    #[test]
    fn the_rows_say_what_a_network_asks() {
        let fr = Lang::French;
        assert_eq!(
            around(&wifi("Livebox", Security::Psk { sae: false }, false), fr),
            ("WPA2".to_owned(), false)
        );
        assert_eq!(
            around(&wifi("Maison", Security::Psk { sae: true }, true), fr).0,
            "WPA3 · enregistré"
        );
        assert_eq!(
            around(&wifi("Imprimante", Security::Wep, false), fr),
            (
                "WEP · une protection ancienne, facile à casser".to_owned(),
                true
            )
        );
        assert_eq!(
            around(&wifi("eduroam", Security::Enterprise, false), Lang::English).0,
            "Enterprise (802.1X) · user name and password"
        );
        assert!(joins_at_once(&wifi("Gare", Security::Open, false)));
        assert!(!joins_at_once(&wifi(
            "Bbox",
            Security::Psk { sae: false },
            false
        )));
    }

    #[test]
    fn the_connected_network_says_its_band_signal_and_address() {
        let current = Current {
            frequency: Some(5180),
            address: Some("192.168.1.24".to_owned()),
            gateway: Some("192.168.1.1".to_owned()),
            ..Current::default()
        };
        let maison = wifi("Maison", Security::Psk { sae: false }, true);
        assert_eq!(
            connected(&maison, Some(&current), Lang::French),
            "Connecté · WPA2 · 5 GHz · signal fort"
        );
        assert_eq!(
            address(&current, Lang::French).as_deref(),
            Some("Adresse 192.168.1.24 · par la box 192.168.1.1")
        );
        assert_eq!(band(2437, Lang::English), "2.4 GHz");
        assert_eq!(signal(35, Lang::French), "signal moyen");
    }

    #[test]
    fn saved_networks_say_when_they_were_used_most_recent_first() {
        let now = 10_000_000;
        let fr = Lang::French;
        assert_eq!(ago(now - 2 * 3600 - 10, now, fr), "Il y a 2 h");
        assert_eq!(ago(now - 30 * 3600, now, fr), "Hier");
        assert_eq!(ago(now - 12 * 86_400, now, fr), "Il y a 12 jours");
        assert_eq!(ago(0, now, Lang::English), "Never used");
        let saved = |name: &str, timestamp: u64, connected: bool| Saved {
            uuid: name.to_owned(),
            name: name.to_owned(),
            ssid: name.as_bytes().to_vec(),
            timestamp,
            autoconnect: true,
            security: Security::Open,
            identity: None,
            connected,
        };
        let sorted = by_recent(&[
            saved("TGV", now - 12 * 86_400, false),
            saved("Bureau", now - 7200, false),
            saved("Maison", now - 9000, true),
        ]);
        let names: Vec<&str> = sorted.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Maison", "Bureau", "TGV"]);
        assert_eq!(
            saved_line(&sorted[0], now, fr),
            "Connecté maintenant · ouvert"
        );
        assert_eq!(saved_line(&sorted[2], now, fr), "Il y a 12 jours · ouvert");
    }

    #[test]
    fn the_homes_card_says_the_connection_in_use() {
        let fr = Lang::French;
        let mut details = Details {
            network: Network {
                manager: true,
                hardware: true,
                enabled: true,
                ..Network::default()
            },
            ..Details::default()
        };
        assert_eq!(card(Some(&details), false, fr), "Hors ligne");
        details.network.current = Some(wifi("Maison", Security::Open, true));
        assert_eq!(card(Some(&details), false, fr), "Wi-Fi · Maison");
        details.network.wired = true;
        assert_eq!(card(Some(&details), false, fr), "Câble");
        assert_eq!(card(Some(&details), true, fr), "Mode avion");
        assert_eq!(card(None, false, Lang::English), "Unknown");
    }

    #[test]
    fn the_cable_says_its_device_its_state_and_its_speed() {
        let mut wired = Wired {
            usb: true,
            ..Wired::default()
        };
        assert_eq!(
            wired_line(&wired, Lang::French),
            "Adaptateur Ethernet USB · débranché"
        );
        assert_eq!(wired_value(&wired, Lang::English), "Unplugged");
        wired.connected = true;
        wired.speed = Some(2500);
        wired.address = Some("192.168.1.30".to_owned());
        assert_eq!(
            wired_value(&wired, Lang::French),
            "Connecté · 2,5 Gb/s · 192.168.1.30"
        );
        wired.usb = false;
        wired.speed = Some(100);
        assert_eq!(
            wired_line(&wired, Lang::English),
            "Ethernet port · connected"
        );
        assert_eq!(
            wired_value(&wired, Lang::English),
            "Connected · 100 Mb/s · 192.168.1.30"
        );
    }

    #[test]
    fn a_wep_secret_is_a_key_or_a_passphrase() {
        assert!(hocus_network::wep_is_key("12345"));
        assert!(hocus_network::wep_is_key("0123456789"));
        assert!(!hocus_network::wep_is_key("012345678z"));
        assert!(!hocus_network::wep_is_key("une phrase de passe"));
    }

    #[test]
    fn every_setting_of_the_page_is_found_by_its_words() {
        let found = crate::search::find("avion", Lang::French);
        assert_eq!(found[0].name, "Mode avion");
        let found = crate::search::find("oublier", Lang::French);
        assert_eq!(found[0].name, "Réseaux enregistrés");
        let found = crate::search::find("gateway", Lang::English);
        assert_eq!(found[0].name, "IP address");
        let found = crate::search::find("eduroam", Lang::French);
        assert_eq!(found[0].name, "Réseau d’entreprise");
        let found = crate::search::find("rj45", Lang::French);
        assert_eq!(found[0].name, "Câble");
        let found = crate::search::find("wireguard", Lang::French);
        assert_eq!(found[0].name, "VPN");
        let found = crate::search::find("ssid", Lang::English);
        assert_eq!(found[0].name, "Hidden network");
    }
}
