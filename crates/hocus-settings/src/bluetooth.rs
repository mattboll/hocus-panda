//! The Bluetooth page's model (6.7, settings.md "Bluetooth"): what the
//! search finds on it, and the words its rows say of BlueZ's devices, as
//! the bar's popover says them (bar.md §9). Nothing of it is in the file:
//! BlueZ keeps the pairings, the names given and the trust. The window's
//! side is `app::bluetooth`.

use hocus_bluetooth::{Device, Kind};

use crate::{
    lang::{Lang, Words, w},
    model::{Outside, SectionId},
};

const POWER: Words = w("BLUETOOTH", "BLUETOOTH");
const MINE: Words = w("MES APPAREILS", "MY DEVICES");
const NEARBY: Words = w("AUTOUR", "NEARBY");

/// The switch's row, the paired devices' and those around, by their
/// search ids.
pub const SWITCH: &str = "bluetooth.power";
pub const MY_DEVICES: &str = "bluetooth.mine";
pub const PAIR: &str = "bluetooth.pair";

/// The page's settings, for the search: each lights its row (`forget`,
/// `rename` and `trust` the paired devices', `visible` the switch's).
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
            section: SectionId::Bluetooth,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 7] = [
        o(
            SWITCH,
            POWER,
            w("Bluetooth", "Bluetooth"),
            &["bt", "sans fil"],
            &["bt", "wireless"],
        ),
        o(
            MY_DEVICES,
            MINE,
            w("Mes appareils", "My devices"),
            &[
                "casque",
                "écouteurs",
                "souris",
                "clavier",
                "enceinte",
                "manette",
            ],
            &[
                "headphones",
                "earbuds",
                "mouse",
                "keyboard",
                "speaker",
                "gamepad",
            ],
        ),
        o(
            PAIR,
            NEARBY,
            w("Associer un appareil", "Pair a device"),
            &["appairer", "jumeler", "ajouter", "code", "pin"],
            &["pair", "add", "pairing"],
        ),
        o(
            "bluetooth.forget",
            MINE,
            w("Oublier un appareil", "Forget a device"),
            &["supprimer", "dissocier"],
            &["forget", "remove", "unpair"],
        ),
        o(
            "bluetooth.rename",
            MINE,
            w("Renommer un appareil", "Rename a device"),
            &["nom"],
            &["rename"],
        ),
        o(
            "bluetooth.trust",
            MINE,
            w("Se connecter seul", "Connect automatically"),
            &["automatique", "confiance"],
            &["trusted", "auto-connect"],
        ),
        o(
            "bluetooth.visible",
            POWER,
            w("Visible", "Visible"),
            &["découvrable", "nom de l’ordinateur"],
            &["discoverable"],
        ),
    ];
    &ALL
}

/// The row a search id lights: the paired devices' for what is done to
/// one, the switch's for being visible.
pub fn row_of(id: &str) -> &str {
    match id {
        "bluetooth.forget" | "bluetooth.rename" | "bluetooth.trust" => MY_DEVICES,
        "bluetooth.visible" => SWITCH,
        _ => id,
    }
}

/// What a kind is called, under a device around.
pub fn kind_name(kind: Kind) -> Words {
    match kind {
        Kind::Earbuds => w("Écouteurs", "Earbuds"),
        Kind::Headphones => w("Casque", "Headphones"),
        Kind::Speaker => w("Enceinte", "Speaker"),
        Kind::Mouse => w("Souris", "Mouse"),
        Kind::Keyboard => w("Clavier", "Keyboard"),
        Kind::Gamepad => w("Manette", "Gamepad"),
        Kind::Phone => w("Téléphone", "Phone"),
        Kind::Tablet => w("Tablette", "Tablet"),
        Kind::Computer => w("Ordinateur", "Computer"),
        Kind::Other => w("Appareil", "Device"),
    }
}

/// Whether French says the kind in the feminine ("Connectée").
fn feminine(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::Mouse | Kind::Speaker | Kind::Gamepad | Kind::Tablet
    )
}

/// "Connecté" / "Connectée" / "Connected".
pub fn connected(kind: Kind, lang: Lang) -> &'static str {
    match (lang, feminine(kind)) {
        (Lang::French, false) => "Connecté",
        (Lang::French, true) => "Connectée",
        (Lang::English, _) => "Connected",
    }
}

/// "Pas connecté", and how it comes back when it does by itself.
pub fn not_connected(device: &Device, lang: Lang) -> String {
    let mut said = match (lang, feminine(device.kind)) {
        (Lang::French, false) => "Pas connecté",
        (Lang::French, true) => "Pas connectée",
        (Lang::English, _) => "Not connected",
    }
    .to_owned();
    if device.trusted && device.kind.input() {
        said.push_str(match (lang, feminine(device.kind)) {
            (Lang::French, false) => " · se connecte seul quand il s’allume",
            (Lang::French, true) => " · se connecte seule quand elle s’allume",
            (Lang::English, _) => " · connects by itself when switched on",
        });
    }
    said
}

/// " · batterie 82 %", after "Connecté".
pub fn battery(percent: u8, lang: Lang) -> String {
    match lang {
        Lang::French => format!(" · batterie\u{a0}{percent}\u{a0}%"),
        Lang::English => format!(" · battery\u{a0}{percent}\u{a0}%"),
    }
}

/// A device around's line: its kind, and that a code will be checked for
/// a phone, a tablet or a computer.
pub fn nearby_line(device: &Device, lang: Lang) -> String {
    let mut said = kind_name(device.kind).get(lang).to_owned();
    if device.kind.asks_a_code() {
        said.push_str(w(" · un code sera à vérifier", " · a code will be checked").get(lang));
    }
    said
}

/// "4 associés".
pub fn paired_count(count: usize, lang: Lang) -> String {
    match (lang, count) {
        (Lang::French, 1) => "1 associé".to_owned(),
        (Lang::French, n) => format!("{n} associés"),
        (Lang::English, n) => format!("{n} paired"),
    }
}

/// The home's card: how many are connected, or that it is off.
pub fn card(bt: Option<&hocus_bluetooth::Bluetooth>, lang: Lang) -> String {
    let Some(bt) = bt.filter(|b| b.adapter) else {
        return w("Aucun adaptateur", "No adapter").get(lang).to_owned();
    };
    if !bt.powered {
        return w("Coupé", "Off").get(lang).to_owned();
    }
    match (lang, bt.connected.len()) {
        (Lang::French, 0) => "Aucun appareil connecté".to_owned(),
        (Lang::French, 1) => "1 appareil connecté".to_owned(),
        (Lang::French, n) => format!("{n} appareils connectés"),
        (Lang::English, 0) => "No device connected".to_owned(),
        (Lang::English, 1) => "1 device connected".to_owned(),
        (Lang::English, n) => format!("{n} devices connected"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(kind: Kind, trusted: bool) -> Device {
        Device {
            path: "/org/bluez/hci0/dev_1".to_owned(),
            name: "x".to_owned(),
            kind,
            paired: true,
            trusted,
            ..Device::default()
        }
    }

    #[test]
    fn a_device_reads_as_the_popover_says_it() {
        assert_eq!(connected(Kind::Mouse, Lang::French), "Connectée");
        assert_eq!(connected(Kind::Headphones, Lang::French), "Connecté");
        assert_eq!(
            not_connected(&device(Kind::Keyboard, true), Lang::French),
            "Pas connecté · se connecte seul quand il s’allume"
        );
        assert_eq!(
            not_connected(&device(Kind::Phone, true), Lang::English),
            "Not connected"
        );
        assert_eq!(battery(82, Lang::French), " · batterie\u{a0}82\u{a0}%");
        assert_eq!(
            nearby_line(&device(Kind::Computer, false), Lang::French),
            "Ordinateur · un code sera à vérifier"
        );
        assert_eq!(
            nearby_line(&device(Kind::Earbuds, false), Lang::English),
            "Earbuds"
        );
        assert_eq!(paired_count(4, Lang::French), "4 associés");
        let mut bt = hocus_bluetooth::Bluetooth {
            adapter: true,
            powered: true,
            ..Default::default()
        };
        assert_eq!(card(Some(&bt), Lang::French), "Aucun appareil connecté");
        bt.connected = vec![device(Kind::Mouse, true), device(Kind::Phone, false)];
        assert_eq!(card(Some(&bt), Lang::French), "2 appareils connectés");
        bt.powered = false;
        assert_eq!(card(Some(&bt), Lang::English), "Off");
        assert_eq!(card(None, Lang::French), "Aucun adaptateur");
    }

    #[test]
    fn every_setting_of_the_page_is_found_by_its_words() {
        assert_eq!(outside().len(), 7);
        let found = |q: &str, lang| {
            crate::search::find(q, lang)
                .iter()
                .map(|f| f.target.id())
                .collect::<Vec<_>>()
        };
        assert!(found("jumeler", Lang::French).contains(&PAIR.to_owned()));
        assert!(found("unpair", Lang::English).contains(&"bluetooth.forget".to_owned()));
        assert!(found("decouvrable", Lang::French).contains(&"bluetooth.visible".to_owned()));
        assert_eq!(row_of("bluetooth.rename"), MY_DEVICES);
    }
}
