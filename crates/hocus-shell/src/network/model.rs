//! What the connections show, from `hocus-network` (shared with the
//! Settings' Réseau page): the network as NetworkManager last said it
//! ([`Network`]), the bar's row for it ([`Link`]), the networks around;
//! and here, their words, in English and French.

pub use hocus_network::model::*;

use crate::levels::model::Words;
use hocus_words::shell::network_model as txt;

/// The words of the connections and the Wi-Fi's popover.
impl Words {
    pub fn wifi(self) -> &'static str {
        "Wi-Fi"
    }

    pub fn connecting(self) -> &'static str {
        txt::CONNECTING.get(self.lang())
    }

    pub fn offline(self) -> &'static str {
        txt::OFFLINE.get(self.lang())
    }

    pub fn wifi_off(self) -> &'static str {
        txt::WI_FI_OFF.get(self.lang())
    }

    pub fn wired(self) -> &'static str {
        txt::WIRED.get(self.lang())
    }

    pub fn by_cable(self) -> &'static str {
        txt::CONNECTED_BY_CABLE.get(self.lang())
    }

    pub fn airplane_mode(self) -> &'static str {
        txt::AIRPLANE_MODE.get(self.lang())
    }

    pub fn airplane_is_on(self) -> &'static str {
        txt::AIRPLANE_MODE_IS_ON.get(self.lang())
    }

    pub fn wifi_settings(self) -> &'static str {
        txt::WI_FI_SETTINGS.get(self.lang())
    }

    pub fn wifi_is_off(self) -> &'static str {
        txt::WI_FI_IS_OFF.get(self.lang())
    }

    pub fn no_adapter(self) -> &'static str {
        txt::NO_WI_FI_ADAPTER.get(self.lang())
    }

    pub fn connected(self) -> &'static str {
        txt::CONNECTED.get(self.lang())
    }

    pub fn sign_in_waits(self) -> &'static str {
        txt::SIGN_IN_PAGE_TO_OPEN.get(self.lang())
    }

    pub fn disconnect(self) -> &'static str {
        txt::DISCONNECT.get(self.lang())
    }

    pub fn open_sign_in(self) -> &'static str {
        txt::OPEN_THE_SIGN_IN_PAGE.get(self.lang())
    }

    /// Over the networks, in capitals.
    pub fn networks(self) -> &'static str {
        txt::NETWORKS.get(self.lang())
    }

    pub fn searching(self) -> &'static str {
        txt::SEARCHING.get(self.lang())
    }

    pub fn searching_networks(self) -> &'static str {
        txt::SEARCHING_FOR_NETWORKS.get(self.lang())
    }

    pub fn no_network(self) -> &'static str {
        txt::NO_NETWORK_FOUND.get(self.lang())
    }

    pub fn saved(self) -> &'static str {
        txt::SAVED.get(self.lang())
    }

    pub fn in_settings(self) -> &'static str {
        txt::IN_SETTINGS.get(self.lang())
    }

    /// A row whose connection failed, not for its password.
    pub fn failed(self) -> &'static str {
        txt::FAILED.get(self.lang())
    }

    pub fn password(self) -> &'static str {
        txt::PASSWORD.get(self.lang())
    }

    pub fn cancel(self) -> &'static str {
        txt::CANCEL.get(self.lang())
    }

    pub fn join(self) -> &'static str {
        txt::CONNECT.get(self.lang())
    }

    pub fn wrong_password(self) -> &'static str {
        txt::WRONG_PASSWORD_CHECK_IT_AND_TRY.get(self.lang())
    }

    pub fn could_not_connect(self) -> &'static str {
        txt::COULD_NOT_CONNECT_TRY_AGAIN.get(self.lang())
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
