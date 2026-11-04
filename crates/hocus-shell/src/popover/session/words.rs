//! The session menu's words (7.4), in French and English, as the spec's
//! table gives them.

use hocus_link::LeaveAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    #[cfg(test)]
    pub(crate) const fn french() -> Self {
        Self { fr: true }
    }

    #[cfg(test)]
    pub(crate) const fn english() -> Self {
        Self { fr: false }
    }

    const fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn settings(self) -> &'static str {
        self.pick("Réglages", "Settings")
    }

    pub fn lock(self) -> &'static str {
        self.pick("Verrouiller", "Lock")
    }

    pub fn suspend(self) -> &'static str {
        self.pick("Mettre en veille", "Suspend")
    }

    pub fn row(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("Se déconnecter", "Log out"),
            LeaveAction::Restart => self.pick("Redémarrer", "Restart"),
            LeaveAction::ShutDown => self.pick("Éteindre", "Shut down"),
        }
    }

    /// The confirmation's title.
    pub fn title(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("Se déconnecter ?", "Log out?"),
            LeaveAction::Restart => self.pick("Redémarrer ?", "Restart?"),
            LeaveAction::ShutDown => self.pick("Éteindre ?", "Shut down?"),
        }
    }

    /// The countdown's sentence, `seconds` left.
    pub fn counting(self, action: LeaveAction, seconds: u32) -> String {
        match (action, self.fr) {
            (LeaveAction::LogOut, true) => format!("La session se ferme dans {seconds} s."),
            (LeaveAction::LogOut, false) => format!("The session ends in {seconds} s."),
            (LeaveAction::Restart, true) => format!("L’ordinateur redémarre dans {seconds} s."),
            (LeaveAction::Restart, false) => format!("The computer restarts in {seconds} s."),
            (LeaveAction::ShutDown, true) => format!("L’ordinateur s’éteint dans {seconds} s."),
            (LeaveAction::ShutDown, false) => {
                format!("The computer shuts down in {seconds} s.")
            }
        }
    }

    pub fn now(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("Se déconnecter maintenant", "Log out now"),
            LeaveAction::Restart => self.pick("Redémarrer maintenant", "Restart now"),
            LeaveAction::ShutDown => self.pick("Éteindre maintenant", "Shut down now"),
        }
    }

    pub fn anyway(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("Se déconnecter quand même", "Log out anyway"),
            LeaveAction::Restart => self.pick("Redémarrer quand même", "Restart anyway"),
            LeaveAction::ShutDown => self.pick("Éteindre quand même", "Shut down anyway"),
        }
    }

    pub fn cancel(self) -> &'static str {
        self.pick("Annuler", "Cancel")
    }

    /// What is held back, after "retient" / "is holding back".
    fn held_what(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("la déconnexion", "logging out"),
            LeaveAction::Restart => self.pick("le redémarrage", "restarting"),
            LeaveAction::ShutDown => self.pick("l’arrêt", "shutting down"),
        }
    }

    /// The list's title: `app` alone, or `count` applications.
    pub fn held(self, action: LeaveAction, app: &str, count: usize) -> String {
        let what = self.held_what(action);
        match (count, self.fr) {
            (1, true) => format!("{app} retient {what}"),
            (1, false) => format!("{app} is holding back {what}"),
            (n, true) => format!("{n} applications retiennent {what}"),
            (n, false) => format!("{n} applications are holding back {what}"),
        }
    }

    pub fn held_sentence(self) -> &'static str {
        self.pick(
            "Rien ne compte tant qu’il retient. Reviens-y, ou passe outre : ce qui n’est pas envoyé sera perdu.",
            "Nothing counts down while it holds back. Go back to it, or override it: what was not sent will be lost.",
        )
    }

    pub fn back_to(self, app: &str) -> String {
        if self.fr {
            format!("Revenir à {app}")
        } else {
            format!("Go back to {app}")
        }
    }

    /// A window's button, when it does not close.
    pub fn close_anyway(self) -> &'static str {
        self.pick("Fermer quand même", "Close anyway")
    }

    /// What a window that did not close says, its title `title`.
    pub fn window_open(self, title: &str) -> String {
        match (title.trim().is_empty(), self.fr) {
            (true, true) => "Sa fenêtre ne s’est pas fermée.".to_owned(),
            (true, false) => "Its window did not close.".to_owned(),
            (false, true) => format!("« {title} » ne s’est pas fermée."),
            (false, false) => format!("“{title}” did not close."),
        }
    }

    /// While polkit asks for the password.
    pub fn authorizing(self) -> &'static str {
        self.pick(
            "Ton mot de passe est demandé.",
            "Your password is asked for.",
        )
    }

    /// While the windows close.
    pub fn closing(self) -> &'static str {
        self.pick("Fermeture des applications…", "Closing the applications…")
    }

    /// As it happens.
    pub fn acting(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick("Fermeture de la session…", "Logging out…"),
            LeaveAction::Restart => self.pick("Redémarrage…", "Restarting…"),
            LeaveAction::ShutDown => self.pick("Extinction…", "Shutting down…"),
        }
    }

    /// An application without a name.
    pub fn an_app(self) -> &'static str {
        self.pick("Une application", "An application")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_are_the_spec_tables() {
        let fr = Words::french();
        let en = Words::english();
        assert_eq!(fr.title(LeaveAction::ShutDown), "Éteindre ?");
        assert_eq!(
            fr.counting(LeaveAction::ShutDown, 10),
            "L’ordinateur s’éteint dans 10 s."
        );
        assert_eq!(
            en.counting(LeaveAction::LogOut, 3),
            "The session ends in 3 s."
        );
        assert_eq!(fr.now(LeaveAction::Restart), "Redémarrer maintenant");
        assert_eq!(
            fr.held(LeaveAction::ShutDown, "Firefox", 1),
            "Firefox retient l’arrêt"
        );
        assert_eq!(
            fr.held(LeaveAction::ShutDown, "Firefox", 2),
            "2 applications retiennent l’arrêt"
        );
        assert_eq!(
            en.held(LeaveAction::Restart, "Firefox", 1),
            "Firefox is holding back restarting"
        );
        assert_eq!(fr.back_to("Firefox"), "Revenir à Firefox");
        assert_eq!(en.anyway(LeaveAction::LogOut), "Log out anyway");
        assert_eq!(fr.close_anyway(), "Fermer quand même");
    }
}
