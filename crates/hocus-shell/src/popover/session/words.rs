//! The session menu's words (7.4), in French and English, as the spec's
//! table gives them.

use hocus_link::LeaveAction;
use hocus_words::Lang;
use hocus_words::shell::popover_session_words as txt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self { lang }
    }

    #[cfg(test)]
    pub(crate) const fn french() -> Self {
        Self { lang: Lang::French }
    }

    #[cfg(test)]
    pub(crate) const fn english() -> Self {
        Self {
            lang: Lang::English,
        }
    }

    const fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    pub fn settings(self) -> &'static str {
        self.pick(txt::SETTINGS)
    }

    pub fn lock(self) -> &'static str {
        self.pick(txt::LOCK)
    }

    pub fn suspend(self) -> &'static str {
        self.pick(txt::SUSPEND)
    }

    pub fn row(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick(txt::LOG_OUT),
            LeaveAction::Restart => self.pick(txt::RESTART),
            LeaveAction::ShutDown => self.pick(txt::SHUT_DOWN),
        }
    }

    /// The confirmation's title.
    pub fn title(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick(txt::LOG_OUT_2),
            LeaveAction::Restart => self.pick(txt::RESTART_2),
            LeaveAction::ShutDown => self.pick(txt::SHUT_DOWN_2),
        }
    }

    /// The countdown's sentence, `seconds` left.
    pub fn counting(self, action: LeaveAction, seconds: u32) -> String {
        match (action, (self.lang == Lang::French)) {
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
            LeaveAction::LogOut => self.pick(txt::LOG_OUT_NOW),
            LeaveAction::Restart => self.pick(txt::RESTART_NOW),
            LeaveAction::ShutDown => self.pick(txt::SHUT_DOWN_NOW),
        }
    }

    pub fn anyway(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick(txt::LOG_OUT_ANYWAY),
            LeaveAction::Restart => self.pick(txt::RESTART_ANYWAY),
            LeaveAction::ShutDown => self.pick(txt::SHUT_DOWN_ANYWAY),
        }
    }

    pub fn cancel(self) -> &'static str {
        self.pick(txt::CANCEL)
    }

    /// What is held back, after "retient" / "is holding back".
    fn held_what(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick(txt::LOGGING_OUT),
            LeaveAction::Restart => self.pick(txt::RESTARTING),
            LeaveAction::ShutDown => self.pick(txt::SHUTTING_DOWN),
        }
    }

    /// The list's title: `app` alone, or `count` applications.
    pub fn held(self, action: LeaveAction, app: &str, count: usize) -> String {
        let what = self.held_what(action);
        match (count, (self.lang == Lang::French)) {
            (1, true) => format!("{app} retient {what}"),
            (1, false) => format!("{app} is holding back {what}"),
            (n, true) => format!("{n} applications retiennent {what}"),
            (n, false) => format!("{n} applications are holding back {what}"),
        }
    }

    pub fn held_sentence(self) -> &'static str {
        self.pick(txt::NOTHING_COUNTS_DOWN_WHILE_IT_HOLDS)
    }

    pub fn back_to(self, app: &str) -> String {
        txt::go_back_to(self.lang, app)
    }

    /// A window's button, when it does not close.
    pub fn close_anyway(self) -> &'static str {
        self.pick(txt::CLOSE_ANYWAY)
    }

    /// What a window that did not close says, its title `title`.
    pub fn window_open(self, title: &str) -> String {
        match (title.trim().is_empty(), (self.lang == Lang::French)) {
            (true, true) => "Sa fenêtre ne s’est pas fermée.".to_owned(),
            (true, false) => "Its window did not close.".to_owned(),
            (false, true) => format!("« {title} » ne s’est pas fermée."),
            (false, false) => format!("“{title}” did not close."),
        }
    }

    /// While polkit asks for the password.
    pub fn authorizing(self) -> &'static str {
        self.pick(txt::YOUR_PASSWORD_IS_ASKED_FOR)
    }

    /// While the windows close.
    pub fn closing(self) -> &'static str {
        self.pick(txt::CLOSING_THE_APPLICATIONS)
    }

    /// As it happens.
    pub fn acting(self, action: LeaveAction) -> &'static str {
        match action {
            LeaveAction::LogOut => self.pick(txt::LOGGING_OUT_2),
            LeaveAction::Restart => self.pick(txt::RESTARTING_2),
            LeaveAction::ShutDown => self.pick(txt::SHUTTING_DOWN_2),
        }
    }

    /// An application without a name.
    pub fn an_app(self) -> &'static str {
        self.pick(txt::AN_APPLICATION)
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
