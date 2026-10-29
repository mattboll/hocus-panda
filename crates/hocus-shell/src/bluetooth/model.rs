//! What Bluetooth shows: the adapter and the devices as BlueZ last said
//! them ([`Bluetooth`]), the bar's row for it ([`Row`]), and the words, in
//! English and French.

use crate::levels::model::Words;

/// What a device is, for its icon, from BlueZ's `Icon` (else its `Class`,
/// else its `Appearance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Headphones,
    Speaker,
    Mouse,
    Keyboard,
    Phone,
    Computer,
    /// Anything else: the Bluetooth sign.
    #[default]
    Other,
}

impl Kind {
    /// From BlueZ's `Icon`, a freedesktop icon name it derives from the
    /// device's class or appearance.
    pub fn from_icon(icon: &str) -> Option<Self> {
        Some(match icon {
            "audio-headphones" | "audio-headset" => Self::Headphones,
            "audio-card" | "audio-speakers" => Self::Speaker,
            "input-mouse" | "input-tablet" => Self::Mouse,
            "input-keyboard" => Self::Keyboard,
            "phone" => Self::Phone,
            "computer" => Self::Computer,
            _ => return None,
        })
    }

    /// From a BR/EDR class of device: its major class (bits 8 to 12) and
    /// its minor one.
    pub fn from_class(class: u32) -> Option<Self> {
        let minor = (class >> 2) & 0x3F;
        Some(match (class >> 8) & 0x1F {
            0x01 => Self::Computer,
            0x02 => Self::Phone,
            0x04 => match minor {
                0x01 | 0x02 | 0x06 => Self::Headphones,
                _ => Self::Speaker,
            },
            0x05 => match (minor >> 4) & 0x03 {
                0x01 => Self::Keyboard,
                0x02 | 0x03 => Self::Mouse,
                _ => return None,
            },
            _ => return None,
        })
    }

    /// From a Bluetooth LE appearance: its category (the top ten bits).
    pub fn from_appearance(appearance: u16) -> Option<Self> {
        Some(match (appearance >> 6, appearance & 0x3F) {
            (0x001, _) => Self::Phone,
            (0x002, _) => Self::Computer,
            (0x00F, 0x01) => Self::Keyboard,
            (0x00F, 0x02) => Self::Mouse,
            (0x025, _) => Self::Headphones,
            _ => return None,
        })
    }

    /// Whether pairing it asks for a code (a phone or a computer compares
    /// or types a number): the popover leaves it to the Settings.
    pub fn asks_a_code(self) -> bool {
        matches!(self, Self::Phone | Self::Computer)
    }
}

/// A device as the popover lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Its object's path, which names it to BlueZ.
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    /// Its battery, percent, when it reports one (BlueZ says it while it
    /// is connected).
    pub battery: Option<u8>,
    /// Pairing it needs a code: left to the Settings.
    pub code: bool,
}

impl Device {
    /// Its battery at 15 % or less: a dot on the bar's icon, the
    /// percentage in orange.
    pub fn low(&self) -> bool {
        self.battery.is_some_and(|b| b <= LOW)
    }
}

/// A battery at this or less is low.
pub const LOW: u8 = 15;

/// What the popover asked of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Connect,
    /// Pair, trust, then connect.
    Pair,
}

/// Why it did not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The device did not answer the connection (off, out of range,
    /// refused, timed out).
    NoAnswer,
    /// Pairing failed (not in pairing mode, refused, timed out).
    Pairing,
    /// Pairing asked for a code: left to the Settings.
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Under way: pairing, then connecting (a paired device connects).
    Running(Action),
    Done,
    Failed(Failure),
}

/// What the popover asked last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Each asked has its own number: the popover reacts once to each end.
    pub id: u64,
    pub path: String,
    pub step: Step,
}

/// Bluetooth as BlueZ last said it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bluetooth {
    /// BlueZ is there with an adapter: else no row.
    pub adapter: bool,
    /// The adapter is on (off when blocked by rfkill too).
    pub powered: bool,
    /// The popover's search runs its first pass.
    pub searching: bool,
    /// Connected, the last connected first.
    pub connected: Vec<Device>,
    /// Paired and not connected, by name.
    pub paired: Vec<Device>,
    /// Around, not paired, named, the strongest first.
    pub nearby: Vec<Device>,
    pub attempt: Option<Attempt>,
}

/// What the bar's row says (bar.md §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// No BlueZ or no adapter: no row.
    Hidden,
    Off,
    /// On, nothing connected.
    Nothing,
    /// On its way, nothing connected yet.
    Connecting,
    Connected {
        /// The last connected's name and battery.
        name: String,
        battery: Option<u8>,
        /// How many more are connected.
        more: usize,
        /// A connected device's battery is low.
        low: bool,
    },
}

impl Bluetooth {
    pub fn row(&self) -> Row {
        if !self.adapter {
            return Row::Hidden;
        }
        if !self.powered {
            return Row::Off;
        }
        match self.connected.first() {
            Some(first) => Row::Connected {
                name: first.name.clone(),
                battery: first.battery,
                more: self.connected.len() - 1,
                low: self.connected.iter().any(Device::low),
            },
            None if self.running().is_some() => Row::Connecting,
            None => Row::Nothing,
        }
    }

    /// The device something runs for, and what.
    pub fn running(&self) -> Option<(&str, Action)> {
        let attempt = self.attempt.as_ref()?;
        match attempt.step {
            Step::Running(action) => Some((attempt.path.as_str(), action)),
            _ => None,
        }
    }
}

/// The words of Bluetooth's row and popover.
impl Words {
    pub fn bluetooth(self) -> &'static str {
        "Bluetooth"
    }

    pub fn no_device(self) -> &'static str {
        if self.fr() {
            "Aucun appareil"
        } else {
            "No device"
        }
    }

    pub fn bluetooth_off(self) -> &'static str {
        if self.fr() {
            "Bluetooth coupé"
        } else {
            "Bluetooth off"
        }
    }

    pub fn bluetooth_is_off(self) -> &'static str {
        if self.fr() {
            "Le Bluetooth est coupé."
        } else {
            "Bluetooth is off."
        }
    }

    /// "batterie 82 %", after "Connecté · ": kept on one line (no-break
    /// spaces) when the block's state wraps.
    pub fn battery_of(self, percent: u8) -> String {
        if self.fr() {
            format!("batterie\u{a0}{percent}\u{a0}%")
        } else {
            format!("battery\u{a0}{percent}\u{a0}%")
        }
    }

    /// Over the paired devices, in capitals.
    pub fn my_devices(self) -> &'static str {
        if self.fr() {
            "MES APPAREILS"
        } else {
            "MY DEVICES"
        }
    }

    pub fn all_connected(self) -> &'static str {
        if self.fr() {
            "Tous tes appareils sont connectés."
        } else {
            "All your devices are connected."
        }
    }

    pub fn no_answer(self) -> &'static str {
        if self.fr() {
            "Il ne répond pas. Vérifie qu'il est allumé et à portée."
        } else {
            "It does not answer. Check that it is on and in range."
        }
    }

    /// Over the devices around, in capitals.
    pub fn nearby(self) -> &'static str {
        if self.fr() { "AUTOUR" } else { "NEARBY" }
    }

    pub fn pairing(self) -> &'static str {
        if self.fr() {
            "Association…"
        } else {
            "Pairing…"
        }
    }

    pub fn pairing_failed(self) -> &'static str {
        if self.fr() {
            "L'association a échoué. Mets l'appareil en mode association et réessaie."
        } else {
            "Pairing failed. Put the device in pairing mode and try again."
        }
    }

    pub fn searching_devices(self) -> &'static str {
        if self.fr() {
            "Recherche des appareils…"
        } else {
            "Searching for devices…"
        }
    }

    pub fn no_new_device(self) -> &'static str {
        if self.fr() {
            "Aucun nouvel appareil trouvé."
        } else {
            "No new device found."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::time::Lang;

    fn device(name: &str, battery: Option<u8>) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            name: name.to_owned(),
            kind: Kind::Headphones,
            paired: true,
            connected: true,
            battery,
            code: false,
        }
    }

    #[test]
    fn kinds_from_icons_classes_and_appearances() {
        assert_eq!(Kind::from_icon("audio-headset"), Some(Kind::Headphones));
        assert_eq!(Kind::from_icon("audio-card"), Some(Kind::Speaker));
        assert_eq!(Kind::from_icon("input-mouse"), Some(Kind::Mouse));
        assert_eq!(Kind::from_icon("printer"), None);
        // Headphones (0x240418), a phone (0x5A020C), a keyboard
        // (0x002540), a mouse (0x002580), a speaker (0x240414).
        assert_eq!(Kind::from_class(0x24_0418), Some(Kind::Headphones));
        assert_eq!(Kind::from_class(0x5A_020C), Some(Kind::Phone));
        assert_eq!(Kind::from_class(0x00_2540), Some(Kind::Keyboard));
        assert_eq!(Kind::from_class(0x00_2580), Some(Kind::Mouse));
        assert_eq!(Kind::from_class(0x24_0414), Some(Kind::Speaker));
        assert_eq!(Kind::from_appearance(0x03C1), Some(Kind::Keyboard));
        assert_eq!(Kind::from_appearance(0x03C2), Some(Kind::Mouse));
        assert_eq!(Kind::from_appearance(0x0941), Some(Kind::Headphones));
        assert!(Kind::Phone.asks_a_code() && !Kind::Speaker.asks_a_code());
    }

    #[test]
    fn the_row_says_off_nothing_connecting_or_the_last_connected() {
        let mut bt = Bluetooth::default();
        assert_eq!(bt.row(), Row::Hidden);
        bt.adapter = true;
        assert_eq!(bt.row(), Row::Off);
        bt.powered = true;
        assert_eq!(bt.row(), Row::Nothing);
        bt.attempt = Some(Attempt {
            id: 1,
            path: "/org/bluez/hci0/dev_x".to_owned(),
            step: Step::Running(Action::Connect),
        });
        assert_eq!(bt.row(), Row::Connecting);
        bt.connected = vec![device("WH-1000XM4", Some(82)), device("MX", Some(12))];
        assert_eq!(
            bt.row(),
            Row::Connected {
                name: "WH-1000XM4".to_owned(),
                battery: Some(82),
                more: 1,
                low: true,
            }
        );
    }

    #[test]
    fn the_words_in_both_languages() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.bluetooth_off(), "Bluetooth coupé");
        assert_eq!(en.no_device(), "No device");
        assert_eq!(fr.battery_of(82), "batterie\u{a0}82\u{a0}%");
        assert_eq!(en.my_devices(), "MY DEVICES");
        assert_eq!(fr.nearby(), "AUTOUR");
        assert_eq!(
            en.pairing_failed(),
            "Pairing failed. Put the device in pairing mode and try again."
        );
    }
}
