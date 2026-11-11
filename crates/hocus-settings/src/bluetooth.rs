//! The Bluetooth page's model (6.7, settings.md "Bluetooth"): what the
//! search finds on it, and the words its rows say of BlueZ's devices, as
//! the bar's popover says them (bar.md §9). Nothing of it is in the file:
//! BlueZ keeps the pairings, the names given and the trust. The window's
//! side is `app::bluetooth`.

use hocus_bluetooth::{Device, Kind};
use hocus_words::settings::bluetooth as words;

use crate::{
    lang::{Lang, Words, w},
    model::{Outside, SectionId},
};

const POWER: Words = w(words::BLUETOOTH);
const MINE: Words = w(words::MY_DEVICES);
const NEARBY: Words = w(words::NEARBY);

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
            w(words::BLUETOOTH_2),
            &["bt", "sans fil"],
            &["bt", "wireless"],
        ),
        o(
            MY_DEVICES,
            MINE,
            w(words::MY_DEVICES_2),
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
            w(words::PAIR_A_DEVICE),
            &["appairer", "jumeler", "ajouter", "code", "pin"],
            &["pair", "add", "pairing"],
        ),
        o(
            "bluetooth.forget",
            MINE,
            w(words::FORGET_A_DEVICE),
            &["supprimer", "dissocier"],
            &["forget", "remove", "unpair"],
        ),
        o(
            "bluetooth.rename",
            MINE,
            w(words::RENAME_A_DEVICE),
            &["nom"],
            &["rename"],
        ),
        o(
            "bluetooth.trust",
            MINE,
            w(words::CONNECT_AUTOMATICALLY),
            &["automatique", "confiance"],
            &["trusted", "auto-connect"],
        ),
        o(
            "bluetooth.visible",
            POWER,
            w(words::VISIBLE),
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
        Kind::Earbuds => w(words::EARBUDS),
        Kind::Headphones => w(words::HEADPHONES),
        Kind::Speaker => w(words::SPEAKER),
        Kind::Mouse => w(words::MOUSE),
        Kind::Keyboard => w(words::KEYBOARD),
        Kind::Gamepad => w(words::GAMEPAD),
        Kind::Phone => w(words::PHONE),
        Kind::Tablet => w(words::TABLET),
        Kind::Computer => w(words::COMPUTER),
        Kind::Other => w(words::DEVICE),
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
        said.push_str(w(words::A_CODE_WILL_BE_CHECKED).get(lang));
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
        return w(words::NO_ADAPTER).get(lang).to_owned();
    };
    if !bt.powered {
        return w(words::OFF).get(lang).to_owned();
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
