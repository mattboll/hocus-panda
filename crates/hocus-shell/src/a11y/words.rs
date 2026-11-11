//! What screen readers say, in the shell's language: the words the bar
//! draws, whole ("62 %", not "62"), times as sentences ("14 h 32").

use hocus_bluetooth::model::Row as BluetoothRow;
use hocus_network::model::Link;

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
    pub fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    /// `a`, `a et b`, `a, b et c`.
    pub fn and(self, names: &[&str]) -> String {
        match names {
            [] => String::new(),
            [one] => (*one).to_owned(),
            [rest @ .., last] => format!("{} {} {last}", rest.join(", "), self.pick("et", "and")),
        }
    }

    /// A bar's window: "Barre — Écran du portable".
    pub fn bar(self, screen: &str) -> String {
        format!("{} — {screen}", self.pick("Barre", "Bar"))
    }

    /// The time as said: "14 h 32" in French, "2:32 PM" or "14:32" else.
    pub fn time(self, shown: &Shown) -> String {
        let written = shown.time();
        if !self.fr || shown.half.is_some() {
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
        match (waiting, self.fr) {
            (0, _) => {}
            (1, true) => parts.push("1 t'attend".to_owned()),
            (n, true) => parts.push(format!("{n} t'attendent")),
            (n, false) => parts.push(format!("{n} waiting")),
        }
        match (stuck, self.fr) {
            (0, _) => {}
            (1, true) => parts.push("1 bloqué".to_owned()),
            (n, true) => parts.push(format!("{n} bloqués")),
            (n, false) => parts.push(format!("{n} stuck")),
        }
        if to_see > 0 {
            parts.push(format!("{to_see} {}", self.pick("à voir", "to see")));
        }
        parts.push(format!("{working} {}", self.pick("au travail", "working")));
        let colon = self.pick(" :", ":");
        format!("Agents{colon} {}", parts.join(", "))
    }

    /// An agent's row: "site-web, t'attend depuis 2 minutes".
    pub fn agent(self, name: &str, look: crate::agents::Look, minutes: u64) -> String {
        use crate::agents::Look;
        let since = self.minutes(minutes);
        let state = match (look, self.fr) {
            (Look::Waiting, true) => format!("t'attend depuis {since}"),
            (Look::Waiting, false) => format!("waiting for {since}"),
            (Look::Stuck, true) => format!("bloqué depuis {since}"),
            (Look::Stuck, false) => format!("stuck for {since}"),
            (Look::ToSee, true) => "a fini, à voir".to_owned(),
            (Look::ToSee, false) => "done, to see".to_owned(),
            (Look::Working, true) => format!("au travail depuis {since}"),
            (Look::Working, false) => format!("working for {since}"),
            (Look::Resting, true) => "au repos".to_owned(),
            (Look::Resting, false) => "resting".to_owned(),
        };
        format!("{name}, {state}")
    }

    /// "+ N autres": "5 autres agents au repos", "5 autres agents".
    pub fn other_agents(self, n: usize, resting: bool) -> String {
        match (self.fr, n, resting) {
            (true, 1, true) => "1 autre agent au repos".to_owned(),
            (true, 1, false) => "1 autre agent".to_owned(),
            (true, n, true) => format!("{n} autres agents au repos"),
            (true, n, false) => format!("{n} autres agents"),
            (false, 1, true) => "1 more agent resting".to_owned(),
            (false, 1, false) => "1 more agent".to_owned(),
            (false, n, true) => format!("{n} more agents resting"),
            (false, n, false) => format!("{n} more agents"),
        }
    }

    /// "à l'instant", "1 minute", "12 minutes", "1 h 05".
    pub fn minutes(self, minutes: u64) -> String {
        match minutes {
            0 => self.pick("à l'instant", "a moment").to_owned(),
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
        match (waiting.len(), self.fr) {
            (0, _) => {}
            (1, true) => said.push(format!("{} t'attend.", waiting[0])),
            (_, true) => said.push(format!("{} t'attendent.", self.and(waiting))),
            (1, false) => said.push(format!("{} is waiting.", waiting[0])),
            (_, false) => said.push(format!("{} are waiting.", self.and(waiting))),
        }
        match (stuck.len(), self.fr) {
            (0, _) => {}
            (1, true) => said.push(format!("{} est bloqué.", stuck[0])),
            (_, true) => said.push(format!("{} sont bloqués.", self.and(stuck))),
            (1, false) => said.push(format!("{} is stuck.", stuck[0])),
            (_, false) => said.push(format!("{} are stuck.", self.and(stuck))),
        }
        said.join(" ")
    }

    /// Today's list: "Aujourd'hui, 3 évènements".
    pub fn today(self, events: usize) -> String {
        match (self.fr, events) {
            (true, 0) => "Aujourd'hui, aucun évènement".to_owned(),
            (true, 1) => "Aujourd'hui, 1 évènement".to_owned(),
            (true, n) => format!("Aujourd'hui, {n} évènements"),
            (false, 0) => "Today, no events".to_owned(),
            (false, 1) => "Today, 1 event".to_owned(),
            (false, n) => format!("Today, {n} events"),
        }
    }

    /// The to-do list: "À faire, 5 tâches".
    pub fn tasks(self, open: usize) -> String {
        match (self.fr, open) {
            (true, 0) => "À faire, aucune tâche".to_owned(),
            (true, 1) => "À faire, 1 tâche".to_owned(),
            (true, n) => format!("À faire, {n} tâches"),
            (false, 0) => "To do, no tasks".to_owned(),
            (false, 1) => "To do, 1 task".to_owned(),
            (false, n) => format!("To do, {n} tasks"),
        }
    }

    /// The windows' list: "Fenêtres, 3".
    pub fn windows(self, n: usize) -> String {
        format!("{}, {n}", self.pick("Fenêtres", "Windows"))
    }

    pub fn desktops(self) -> &'static str {
        self.pick("Bureaux", "Desktops")
    }

    /// A desktop: "Bureau 2, 1 fenêtre : Firefox", the current one "Bureau
    /// 1, actuel, 2 fenêtres : Terminal, Firefox".
    pub fn desktop(self, title: &str, current: bool, windows: &[String]) -> String {
        let mut name = title.to_owned();
        if current {
            name.push_str(self.pick(", actuel", ", current"));
        }
        let count = match (self.fr, windows.len()) {
            (true, 0) => "aucune fenêtre".to_owned(),
            (true, 1) => "1 fenêtre".to_owned(),
            (true, n) => format!("{n} fenêtres"),
            (false, 0) => "no windows".to_owned(),
            (false, 1) => "1 window".to_owned(),
            (false, n) => format!("{n} windows"),
        };
        name.push_str(", ");
        name.push_str(&count);
        if !windows.is_empty() {
            name.push_str(self.pick(" : ", ": "));
            name.push_str(&windows.join(", "));
        }
        name
    }

    /// The Wi-Fi's row: "Wi-Fi, connecté à Livebox-A3F2, signal fort".
    pub fn wifi(self, link: &Link) -> String {
        let state = match (link, self.fr) {
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
                let signal = match (bars, self.fr) {
                    (0 | 1, true) => "signal faible",
                    (2, true) => "signal moyen",
                    (3, true) => "bon signal",
                    (_, true) => "signal fort",
                    (0 | 1, false) => "weak signal",
                    (2, false) => "fair signal",
                    (3, false) => "good signal",
                    (_, false) => "strong signal",
                };
                if self.fr {
                    format!("connecté à {name}, {signal}")
                } else {
                    format!("connected to {name}, {signal}")
                }
            }
        };
        format!("Wi-Fi, {state}")
    }

    /// Bluetooth's row: "Bluetooth, Casque, batterie 80 %".
    pub fn bluetooth(self, row: &BluetoothRow) -> String {
        let state = match (row, self.fr) {
            (BluetoothRow::Hidden, _) => return String::new(),
            (BluetoothRow::Off, true) => "désactivé".to_owned(),
            (BluetoothRow::Off, false) => "off".to_owned(),
            (BluetoothRow::Nothing, true) => "aucun appareil connecté".to_owned(),
            (BluetoothRow::Nothing, false) => "no device connected".to_owned(),
            (BluetoothRow::Connecting, true) => "connexion…".to_owned(),
            (BluetoothRow::Connecting, false) => "connecting…".to_owned(),
            (
                BluetoothRow::Connected {
                    name,
                    battery,
                    more,
                    ..
                },
                _,
            ) => {
                let mut said = name.clone();
                if let Some(battery) = battery {
                    said.push_str(&format!(
                        ", {} {battery}\u{a0}%",
                        self.pick("batterie", "battery")
                    ));
                }
                match (more, self.fr) {
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
        format!(
            "{}, {name}",
            self.pick("Disposition du clavier", "Keyboard layout")
        )
    }

    pub fn volume(self) -> &'static str {
        "Volume"
    }

    /// A level's value: "62 %", "muet".
    pub fn percent(self, percent: f32) -> String {
        format!("{}\u{a0}%", percent.round().max(0.0) as u32)
    }

    pub fn muted(self) -> &'static str {
        self.pick("muet", "muted")
    }

    pub fn battery(self) -> &'static str {
        self.pick("Batterie", "Battery")
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
        let mode = self.pick("Mode d'énergie", "Power mode");
        match levels.profile {
            Some(profile) => format!("{mode}, {}", words.profile(profile)),
            None => mode.to_owned(),
        }
    }

    pub fn lang(self) -> Lang {
        if self.fr { Lang::French } else { Lang::English }
    }

    /// The in-use sign: "Micro utilisé par Firefox, caméra utilisée par
    /// Zoom".
    pub fn in_use(self, uses: &[(crate::in_use::Kind, Vec<String>)], muted: bool) -> String {
        use crate::in_use::Kind;
        let parts: Vec<String> = uses
            .iter()
            .map(|(kind, apps)| {
                let apps = self.and(&apps.iter().map(String::as_str).collect::<Vec<_>>());
                match (kind, self.fr) {
                    (Kind::Microphone, true) if muted => format!("Micro coupé, utilisé par {apps}"),
                    (Kind::Microphone, true) => format!("Micro utilisé par {apps}"),
                    (Kind::Microphone, false) if muted => {
                        format!("Microphone muted, used by {apps}")
                    }
                    (Kind::Microphone, false) => format!("Microphone used by {apps}"),
                    (Kind::Camera, true) => format!("Caméra utilisée par {apps}"),
                    (Kind::Camera, false) => format!("Camera used by {apps}"),
                    (Kind::Screen, true) => format!("Écran partagé avec {apps}"),
                    (Kind::Screen, false) => format!("Screen shared with {apps}"),
                    (Kind::Recording, true) => "Enregistrement de l'écran".to_owned(),
                    (Kind::Recording, false) => "Recording the screen".to_owned(),
                }
            })
            .collect();
        parts.join(", ")
    }

    pub fn add_task(self) -> &'static str {
        self.pick("Ajouter une tâche", "Add a task")
    }

    /// "+ 4 autres": "4 autres tâches".
    pub fn more_tasks(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "1 autre tâche".to_owned(),
            (true, n) => format!("{n} autres tâches"),
            (false, 1) => "1 more task".to_owned(),
            (false, n) => format!("{n} more tasks"),
        }
    }

    pub fn more_today(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "1 autre évènement aujourd'hui".to_owned(),
            (true, n) => format!("{n} autres évènements aujourd'hui"),
            (false, 1) => "1 more event today".to_owned(),
            (false, n) => format!("{n} more events today"),
        }
    }

    pub fn more_windows(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "1 autre fenêtre".to_owned(),
            (true, n) => format!("{n} autres fenêtres"),
            (false, 1) => "1 more window".to_owned(),
            (false, n) => format!("{n} more windows"),
        }
    }

    pub fn footer(self) -> &'static str {
        self.pick("Pied de la barre", "Bar's footer")
    }

    pub fn tray(self) -> &'static str {
        self.pick("Icônes des applications", "Applications' icons")
    }

    pub fn calendar(self) -> &'static str {
        self.pick("Calendrier", "Calendar")
    }

    pub fn choose_sources(self) -> &'static str {
        self.pick("Choisir…", "Choose…")
    }

    pub fn later(self) -> &'static str {
        self.pick("Plus tard", "Later")
    }

    /// A notification: "Slack : Léa Martin", ", urgent" when critical.
    pub fn notification(self, app: &str, summary: &str, urgent: bool) -> String {
        let mut name = match (app.is_empty(), summary.is_empty()) {
            (true, _) => summary.to_owned(),
            (false, true) => app.to_owned(),
            (false, false) => format!("{app}{} {summary}", self.pick(" :", ":")),
        };
        if urgent {
            name.push_str(", urgent");
        }
        name
    }

    pub fn lock(self) -> &'static str {
        self.pick("Session verrouillée", "Session locked")
    }

    pub fn mute(self) -> &'static str {
        self.pick("Couper le son", "Mute")
    }

    /// An application's tile: "Couper le son de Firefox".
    pub fn mute_app(self, app: &str) -> String {
        if self.fr {
            format!("Couper le son de {app}")
        } else {
            format!("Mute {app}")
        }
    }

    pub fn mute_microphone(self) -> &'static str {
        self.pick("Couper le micro", "Mute the microphone")
    }

    pub fn microphone(self) -> &'static str {
        self.pick("Micro", "Microphone")
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
        let fr = Words { fr: true };
        assert_eq!(
            fr.clock(&shown("14:32", "jeudi 6 novembre")),
            "14 h 32, jeudi 6 novembre"
        );
        assert_eq!(fr.time(&shown("09:05", "")), "9 h 05");
        assert_eq!(fr.time(&shown("00:00", "")), "0 h");
        let en = Words { fr: false };
        assert_eq!(
            en.clock(&shown("14:32", "Thursday 6 November")),
            "14:32, Thursday 6 November"
        );
    }

    #[test]
    fn announcements_group_names() {
        let fr = Words { fr: true };
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
        let en = Words { fr: false };
        assert_eq!(
            en.announce(&["site-web", "docs"], &["infra"]),
            "site-web and docs are waiting. infra is stuck."
        );
    }

    #[test]
    fn the_agents_header_counts_who_waits_and_who_works() {
        let fr = Words { fr: true };
        assert_eq!(
            fr.agents(3, 0, 10, 0),
            "Agents : 3 t'attendent, 10 au travail"
        );
        assert_eq!(
            fr.agents(1, 1, 0, 2),
            "Agents : 1 t'attend, 1 bloqué, 2 à voir, 0 au travail"
        );
        let en = Words { fr: false };
        assert_eq!(en.agents(3, 0, 10, 0), "Agents: 3 waiting, 10 working");
    }

    #[test]
    fn rows_say_their_state_in_words() {
        let fr = Words { fr: true };
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
            Words { fr: false }.notification("Slack", "Léa Martin", true),
            "Slack: Léa Martin, urgent"
        );
    }
}
