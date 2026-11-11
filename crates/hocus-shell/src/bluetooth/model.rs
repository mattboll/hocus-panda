//! What Bluetooth shows: the adapter and the devices as BlueZ last said
//! them ([`Bluetooth`]) and the bar's row for it ([`Row`]), from
//! `hocus-bluetooth`; their words, in English and French.

pub use hocus_bluetooth::model::{Action, Bluetooth, Device, Failure, Kind, Row, Step};

use crate::levels::model::Words;
use hocus_words::shell::bluetooth_model as txt;

/// The words of Bluetooth's row and popover.
impl Words {
    pub fn bluetooth(self) -> &'static str {
        "Bluetooth"
    }

    pub fn no_device(self) -> &'static str {
        txt::NO_DEVICE.get(self.lang())
    }

    pub fn bluetooth_off(self) -> &'static str {
        txt::BLUETOOTH_OFF.get(self.lang())
    }

    pub fn bluetooth_is_off(self) -> &'static str {
        txt::BLUETOOTH_IS_OFF.get(self.lang())
    }

    /// "batterie 82 %", after "Connecté · ": kept on one line (no-break
    /// spaces) when the block's state wraps.
    pub fn battery_of(self, percent: u8) -> String {
        txt::battery(self.lang(), percent)
    }

    /// Over the paired devices, in capitals.
    pub fn my_devices(self) -> &'static str {
        txt::MY_DEVICES.get(self.lang())
    }

    pub fn all_connected(self) -> &'static str {
        txt::ALL_YOUR_DEVICES_ARE_CONNECTED.get(self.lang())
    }

    pub fn no_answer(self) -> &'static str {
        txt::IT_DOES_NOT_ANSWER_CHECK_THAT.get(self.lang())
    }

    /// Over the devices around, in capitals.
    pub fn nearby(self) -> &'static str {
        txt::NEARBY.get(self.lang())
    }

    pub fn pairing(self) -> &'static str {
        txt::PAIRING.get(self.lang())
    }

    pub fn pairing_failed(self) -> &'static str {
        txt::PAIRING_FAILED_PUT_THE_DEVICE_IN.get(self.lang())
    }

    pub fn searching_devices(self) -> &'static str {
        txt::SEARCHING_FOR_DEVICES.get(self.lang())
    }

    /// The popover's last row: the Settings' Bluetooth page.
    pub fn bluetooth_settings(self) -> &'static str {
        txt::BLUETOOTH_SETTINGS.get(self.lang())
    }

    pub fn no_new_device(self) -> &'static str {
        txt::NO_NEW_DEVICE_FOUND.get(self.lang())
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
