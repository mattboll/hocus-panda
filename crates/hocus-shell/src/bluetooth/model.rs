//! What Bluetooth shows: the adapter and the devices as BlueZ last said
//! them ([`Bluetooth`]) and the bar's row for it ([`Row`]), from
//! `hocus-bluetooth`; their words, in English and French.

pub use hocus_bluetooth::model::{Action, Bluetooth, Device, Failure, Kind, Row, Step};

use crate::levels::model::Words;

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

    /// The popover's last row: the Settings' Bluetooth page.
    pub fn bluetooth_settings(self) -> &'static str {
        if self.fr() {
            "Réglages du Bluetooth…"
        } else {
            "Bluetooth settings…"
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
