//! What the connections show, from `hocus-network` (shared with the
//! Settings' Réseau page): the network as NetworkManager last said it
//! ([`Network`]), the bar's row for it ([`Link`]), the networks around;
//! and here, their words, in English and French.

pub use hocus_network::model::*;

use crate::levels::model::Words;

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

    pub fn airplane_mode(self) -> &'static str {
        if self.fr() {
            "Mode avion"
        } else {
            "Airplane mode"
        }
    }

    pub fn airplane_is_on(self) -> &'static str {
        if self.fr() {
            "Le mode avion est activé."
        } else {
            "Airplane mode is on."
        }
    }

    pub fn wifi_settings(self) -> &'static str {
        if self.fr() {
            "Réglages du Wi-Fi…"
        } else {
            "Wi-Fi settings…"
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
