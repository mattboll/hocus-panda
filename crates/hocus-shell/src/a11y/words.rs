//! What screen readers say, in the shell's language: the words the bar
//! draws, whole ("62 %", not "62"), times as sentences ("14 h 32").

use hocus_bluetooth::model::Row as BluetoothRow;
use hocus_network::model::Link;
use hocus_words::shell::a11y_words as txt;

use crate::{
    clock::time::{Lang, Shown},
    levels::model::{Battery, Snapshot},
};

/// A name as drawn: its control characters left out (an agent's name
/// from a tool may hold some).
pub fn plain(name: &str) -> String {
    name.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

/// The words, in the bar's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    /// `a`, `a et b`, `a, b et c`.
    pub fn and(self, names: &[&str]) -> String {
        match names {
            [] => String::new(),
            [one] => (*one).to_owned(),
            [rest @ .., last] => format!("{} {} {last}", rest.join(", "), self.pick(txt::AND)),
        }
    }

    /// A bar's window: "Barre — Écran du portable".
    pub fn bar(self, screen: &str) -> String {
        format!("{} — {screen}", self.pick(txt::BAR))
    }

    /// The time as said: "14 h 32" in French, "2:32 PM" or "14:32" else.
    pub fn time(self, shown: &Shown) -> String {
        let written = shown.time();
        if !(self.lang == Lang::French) || shown.half.is_some() {
            return written;
        }
        match written.split_once(':') {
            Some((hours, minutes)) => {
                let hours = hours.trim_start_matches('0');
                let hours = if hours.is_empty() { "0" } else { hours };
                if minutes == "00" {
                    format!("{hours} h")
                } else {
                    format!("{hours} h {minutes}")
                }
            }
            None => written,
        }
    }

    /// The clock: "14 h 32, jeudi 6 novembre".
    pub fn clock(self, shown: &Shown) -> String {
        format!("{}, {}", self.time(shown), shown.date)
    }

    /// The agents' header: "Agents : 3 t'attendent, 10 au travail".
    pub fn agents(self, waiting: usize, stuck: usize, working: usize, to_see: usize) -> String {
        let mut parts = Vec::new();
        match (waiting, (self.lang == Lang::French)) {
            (0, _) => {}
            (1, true) => parts.push("1 t'attend".to_owned()),
            (n, true) => parts.push(format!("{n} t'attendent")),
            (n, false) => parts.push(format!("{n} waiting")),
        }
        match (stuck, (self.lang == Lang::French)) {
            (0, _) => {}
            (1, true) => parts.push("1 bloqué".to_owned()),
            (n, true) => parts.push(format!("{n} bloqués")),
            (n, false) => parts.push(format!("{n} stuck")),
        }
        if to_see > 0 {
            parts.push(format!("{to_see} {}", self.pick(txt::TO_SEE)));
        }
        parts.push(format!("{working} {}", self.pick(txt::WORKING)));
        let colon = self.pick(txt::TEXT);
        format!("Agents{colon} {}", parts.join(", "))
    }

    /// An agent's row: "site-web, t'attend depuis 2 minutes".
    pub fn agent(self, name: &str, look: crate::agents::Look, minutes: u64) -> String {
        use crate::agents::Look;
        let since = self.minutes(minutes);
        let state = match look {
            Look::Waiting => txt::waiting_for(self.lang, &since),
            Look::Stuck => txt::stuck_for(self.lang, &since),
            Look::ToSee => txt::DONE_TO_SEE.get(self.lang).to_owned(),
            Look::Working => txt::working_for(self.lang, &since),
            Look::Resting => txt::RESTING.get(self.lang).to_owned(),
        };
        format!("{name}, {state}")
    }

    /// "+ N autres": "5 autres agents au repos", "5 autres agents".
    pub fn other_agents(self, n: usize, resting: bool) -> String {
        match (n, resting) {
            (1, true) => txt::N_1_MORE_AGENT_RESTING.get(self.lang).to_owned(),
            (1, false) => txt::N_1_MORE_AGENT.get(self.lang).to_owned(),
            (n, true) => txt::more_agents_resting(self.lang, n),
            (n, false) => txt::more_agents(self.lang, n),
        }
    }

    /// "à l'instant", "1 minute", "12 minutes", "1 h 05".
    pub fn minutes(self, minutes: u64) -> String {
        match minutes {
            0 => self.pick(txt::A_MOMENT).to_owned(),
            1 => "1 minute".to_owned(),
            m if m < 60 => format!("{m} minutes"),
            m if m % 60 == 0 => format!("{} h", m / 60),
            m => format!("{} h {:02}", m / 60, m % 60),
        }
    }

    /// What is said on its own when agents start waiting or get stuck:
    /// "site-web t'attend.", "site-web et docs t'attendent.", "infra est
    /// bloqué.".
    pub fn announce(self, waiting: &[&str], stuck: &[&str]) -> String {
        let mut said = Vec::new();
        match waiting.len() {
            0 => {}
            1 => said.push(txt::is_waiting(self.lang, waiting[0])),
            _ => said.push(txt::are_waiting(self.lang, self.and(waiting))),
        }
        match stuck.len() {
            0 => {}
            1 => said.push(txt::is_stuck(self.lang, stuck[0])),
            _ => said.push(txt::are_stuck(self.lang, self.and(stuck))),
        }
        said.join(" ")
    }

    /// Today's list: "Aujourd'hui, 3 évènements".
    pub fn today(self, events: usize) -> String {
        match events {
            0 => txt::TODAY_NO_EVENTS.get(self.lang).to_owned(),
            1 => txt::TODAY_1_EVENT.get(self.lang).to_owned(),
            n => txt::today_events(self.lang, n),
        }
    }

    /// The to-do list: "À faire, 5 tâches".
    pub fn tasks(self, open: usize) -> String {
        match open {
            0 => txt::TO_DO_NO_TASKS.get(self.lang).to_owned(),
            1 => txt::TO_DO_1_TASK.get(self.lang).to_owned(),
            n => txt::to_do_tasks(self.lang, n),
        }
    }

    /// The windows' list: "Fenêtres, 3".
    pub fn windows(self, n: usize) -> String {
        format!("{}, {n}", self.pick(txt::WINDOWS))
    }

    pub fn desktops(self) -> &'static str {
        self.pick(txt::DESKTOPS)
    }

    /// A desktop: "Bureau 2, 1 fenêtre : Firefox", the current one "Bureau
    /// 1, actuel, 2 fenêtres : Terminal, Firefox".
    pub fn desktop(self, title: &str, current: bool, windows: &[String]) -> String {
        let mut name = title.to_owned();
        if current {
            name.push_str(self.pick(txt::CURRENT));
        }
        let count = match windows.len() {
            0 => txt::NO_WINDOWS.get(self.lang).to_owned(),
            1 => txt::N_1_WINDOW.get(self.lang).to_owned(),
            n => txt::windows_2(self.lang, n),
        };
        name.push_str(", ");
        name.push_str(&count);
        if !windows.is_empty() {
            name.push_str(self.pick(txt::TEXT_2));
            name.push_str(&windows.join(", "));
        }
        name
    }

    /// The Wi-Fi's row: "Wi-Fi, connecté à Livebox-A3F2, signal fort".
    pub fn wifi(self, link: &Link) -> String {
        let state = match (link, (self.lang == Lang::French)) {
            (Link::Hidden, _) => return String::new(),
            (Link::Wired, true) => return "Réseau, filaire".to_owned(),
            (Link::Wired, false) => return "Network, wired".to_owned(),
            (Link::Airplane, true) => "mode avion".to_owned(),
            (Link::Airplane, false) => "airplane mode".to_owned(),
            (Link::Off, true) => "désactivé".to_owned(),
            (Link::Off, false) => "off".to_owned(),
            (Link::Offline, true) => "non connecté".to_owned(),
            (Link::Offline, false) => "not connected".to_owned(),
            (Link::Connecting, true) => "connexion…".to_owned(),
            (Link::Connecting, false) => "connecting…".to_owned(),
            (Link::Connected { name, bars }, _) => {
                let signal = match bars {
                    0 | 1 => txt::WEAK_SIGNAL.get(self.lang),
                    2 => txt::FAIR_SIGNAL.get(self.lang),
                    3 => txt::GOOD_SIGNAL.get(self.lang),
                    _ => txt::STRONG_SIGNAL.get(self.lang),
                };
                txt::connected_to(self.lang, txt::ConnectedTo { name, signal })
            }
        };
        format!("Wi-Fi, {state}")
    }

    /// Bluetooth's row: "Bluetooth, Casque, batterie 80 %".
    pub fn bluetooth(self, row: &BluetoothRow) -> String {
        let state = match row {
            BluetoothRow::Hidden => return String::new(),
            BluetoothRow::Off => txt::OFF.get(self.lang).to_owned(),
            BluetoothRow::Nothing => txt::NO_DEVICE_CONNECTED.get(self.lang).to_owned(),
            BluetoothRow::Connecting => txt::CONNECTING.get(self.lang).to_owned(),
            BluetoothRow::Connected {
                name,
                battery,
                more,
                ..
            } => {
                let mut said = name.clone();
                if let Some(battery) = battery {
                    said.push_str(&format!(", {} {battery}\u{a0}%", self.pick(txt::BATTERY)));
                }
                match (more, (self.lang == Lang::French)) {
                    (0, _) => {}
                    (n, true) => said.push_str(&format!(", et {n} autres")),
                    (n, false) => said.push_str(&format!(", and {n} more")),
                }
                said
            }
        };
        format!("Bluetooth, {state}")
    }

    /// The keyboard layout's button: "Disposition du clavier, Français".
    pub fn layout(self, name: &str) -> String {
        format!("{}, {name}", self.pick(txt::KEYBOARD_LAYOUT))
    }

    pub fn volume(self) -> &'static str {
        "Volume"
    }

    /// A level's value: "62 %", "muet".
    pub fn percent(self, percent: f32) -> String {
        format!("{}\u{a0}%", percent.round().max(0.0) as u32)
    }

    pub fn muted(self) -> &'static str {
        self.pick(txt::MUTED)
    }

    pub fn battery(self) -> &'static str {
        self.pick(txt::BATTERY_2)
    }

    /// The battery's value: "82 %, 3 h 10 restantes, équilibré".
    pub fn battery_value(self, battery: &Battery, levels: &Snapshot) -> String {
        let words = crate::levels::model::Words::new(self.lang());
        let mut value = self.percent(battery.percent);
        if let Some(status) = words.status(levels) {
            value.push_str(", ");
            value.push_str(&status.replace(" · ", ", ").to_lowercase());
        }
        value
    }

    /// The power modes alone, without a battery: "Mode d'alimentation,
    /// équilibré".
    pub fn power(self, levels: &Snapshot) -> String {
        let words = crate::levels::model::Words::new(self.lang());
        let mode = self.pick(txt::POWER_MODE);
        match levels.profile {
            Some(profile) => format!("{mode}, {}", words.profile(profile)),
            None => mode.to_owned(),
        }
    }

    pub fn lang(self) -> Lang {
        if self.lang == Lang::French {
            Lang::French
        } else {
            Lang::English
        }
    }

    /// The in-use sign: "Micro utilisé par Firefox, caméra utilisée par
    /// Zoom".
    pub fn in_use(self, uses: &[(crate::in_use::Kind, Vec<String>)], muted: bool) -> String {
        use crate::in_use::Kind;
        let parts: Vec<String> = uses
            .iter()
            .map(|(kind, apps)| {
                let apps = self.and(&apps.iter().map(String::as_str).collect::<Vec<_>>());
                match kind {
                    Kind::Microphone if muted => txt::microphone_muted_used_by(self.lang, &apps),
                    Kind::Microphone => txt::microphone_used_by(self.lang, &apps),
                    Kind::Camera => txt::camera_used_by(self.lang, &apps),
                    Kind::Screen => txt::screen_shared_with(self.lang, &apps),
                    Kind::Recording => txt::RECORDING_THE_SCREEN.get(self.lang).to_owned(),
                }
            })
            .collect();
        parts.join(", ")
    }

    pub fn add_task(self) -> &'static str {
        self.pick(txt::ADD_A_TASK)
    }

    /// "+ 4 autres": "4 autres tâches".
    pub fn more_tasks(self, n: usize) -> String {
        match n {
            1 => txt::N_1_MORE_TASK.get(self.lang).to_owned(),
            n => txt::more_tasks(self.lang, n),
        }
    }

    pub fn more_today(self, n: usize) -> String {
        match n {
            1 => txt::N_1_MORE_EVENT_TODAY.get(self.lang).to_owned(),
            n => txt::more_events_today(self.lang, n),
        }
    }

    pub fn more_windows(self, n: usize) -> String {
        match n {
            1 => txt::N_1_MORE_WINDOW.get(self.lang).to_owned(),
            n => txt::more_windows(self.lang, n),
        }
    }

    pub fn footer(self) -> &'static str {
        self.pick(txt::BAR_S_FOOTER)
    }

    pub fn tray(self) -> &'static str {
        self.pick(txt::APPLICATIONS_ICONS)
    }

    pub fn calendar(self) -> &'static str {
        self.pick(txt::CALENDAR)
    }

    pub fn choose_sources(self) -> &'static str {
        self.pick(txt::CHOOSE)
    }

    pub fn later(self) -> &'static str {
        self.pick(txt::LATER)
    }

    /// A notification: "Slack : Léa Martin", ", urgent" when critical.
    pub fn notification(self, app: &str, summary: &str, urgent: bool) -> String {
        let mut name = match (app.is_empty(), summary.is_empty()) {
            (true, _) => summary.to_owned(),
            (false, true) => app.to_owned(),
            (false, false) => format!("{app}{} {summary}", self.pick(txt::TEXT)),
        };
        if urgent {
            name.push_str(", urgent");
        }
        name
    }

    pub fn lock(self) -> &'static str {
        self.pick(txt::SESSION_LOCKED)
    }

    pub fn mute(self) -> &'static str {
        self.pick(txt::MUTE)
    }

    /// An application's tile: "Couper le son de Firefox".
    pub fn mute_app(self, app: &str) -> String {
        txt::mute_2(self.lang, app)
    }

    pub fn mute_microphone(self) -> &'static str {
        self.pick(txt::MUTE_THE_MICROPHONE)
    }

    pub fn microphone(self) -> &'static str {
        self.pick(txt::MICROPHONE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(hm: &str, date: &str) -> Shown {
        let c: Vec<char> = hm.chars().collect();
        Shown {
            hm: [c[0], c[1], c[2], c[3], c[4]],
            ss: ['0', '0'],
            half: None,
            date: date.to_owned(),
        }
    }

    #[test]
    fn the_clock_says_a_sentence() {
        let fr = Words {
            lang: hocus_words::Lang::French,
        };
        assert_eq!(
            fr.clock(&shown("14:32", "jeudi 6 novembre")),
            "14 h 32, jeudi 6 novembre"
        );
        assert_eq!(fr.time(&shown("09:05", "")), "9 h 05");
        assert_eq!(fr.time(&shown("00:00", "")), "0 h");
        let en = Words {
            lang: hocus_words::Lang::English,
        };
        assert_eq!(
            en.clock(&shown("14:32", "Thursday 6 November")),
            "14:32, Thursday 6 November"
        );
    }

    #[test]
    fn announcements_group_names() {
        let fr = Words {
            lang: hocus_words::Lang::French,
        };
        assert_eq!(fr.announce(&["site-web"], &[]), "site-web t'attend.");
        assert_eq!(
            fr.announce(&["site-web", "docs"], &[]),
            "site-web et docs t'attendent."
        );
        assert_eq!(fr.announce(&[], &["infra"]), "infra est bloqué.");
        assert_eq!(
            fr.announce(&["a"], &["b", "c", "d"]),
            "a t'attend. b, c et d sont bloqués."
        );
        let en = Words {
            lang: hocus_words::Lang::English,
        };
        assert_eq!(
            en.announce(&["site-web", "docs"], &["infra"]),
            "site-web and docs are waiting. infra is stuck."
        );
    }

    #[test]
    fn the_agents_header_counts_who_waits_and_who_works() {
        let fr = Words {
            lang: hocus_words::Lang::French,
        };
        assert_eq!(
            fr.agents(3, 0, 10, 0),
            "Agents : 3 t'attendent, 10 au travail"
        );
        assert_eq!(
            fr.agents(1, 1, 0, 2),
            "Agents : 1 t'attend, 1 bloqué, 2 à voir, 0 au travail"
        );
        let en = Words {
            lang: hocus_words::Lang::English,
        };
        assert_eq!(en.agents(3, 0, 10, 0), "Agents: 3 waiting, 10 working");
    }

    #[test]
    fn rows_say_their_state_in_words() {
        let fr = Words {
            lang: hocus_words::Lang::French,
        };
        assert_eq!(
            fr.wifi(&Link::Connected {
                name: "Livebox-A3F2".into(),
                bars: 4
            }),
            "Wi-Fi, connecté à Livebox-A3F2, signal fort"
        );
        assert_eq!(
            fr.desktop("Bureau 2", false, &["Firefox".to_owned()]),
            "Bureau 2, 1 fenêtre : Firefox"
        );
        assert_eq!(
            fr.desktop(
                "Bureau 1",
                true,
                &["Terminal".to_owned(), "Firefox".to_owned()]
            ),
            "Bureau 1, actuel, 2 fenêtres : Terminal, Firefox"
        );
        assert_eq!(
            fr.agent("site-web", crate::agents::Look::Waiting, 2),
            "site-web, t'attend depuis 2 minutes"
        );
        assert_eq!(
            fr.notification("Slack", "Léa Martin", false),
            "Slack : Léa Martin"
        );
        assert_eq!(
            Words {
                lang: hocus_words::Lang::English
            }
            .notification("Slack", "Léa Martin", true),
            "Slack: Léa Martin, urgent"
        );
    }
}
