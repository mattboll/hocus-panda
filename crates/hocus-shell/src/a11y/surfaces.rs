//! The shell's other windows for screen readers: the open popover, the
//! notifications' bubbles, the lock screen and the authentication dialog.

use hocus_a11y::{Act, Node, Role, Window};
use hocus_link::{LockFocus, LockPhase, LockShown};

use super::words::Words;
use crate::popover::Item;

/// The open popover: a dialog named by its title, described by its
/// subtitle, its rows by the kit's kinds where the popover says them.
#[derive(Debug, Clone, Default)]
pub struct Popover {
    pub title: String,
    pub subtitle: Option<String>,
    pub rows: Vec<Node>,
    /// The row holding the keyboard, by its key.
    pub focus: Option<String>,
    /// It holds the keyboard (opened by a click or a key, not a hover).
    pub active: bool,
}

pub fn popover(p: &Popover) -> Window {
    let mut window = Window::new("popover", p.title.clone());
    let mut dialog = Node::new("dialog", Role::Dialog, p.title.clone()).children(p.rows.clone());
    if let Some(subtitle) = &p.subtitle {
        dialog = dialog.describe(subtitle.clone());
    }
    window.nodes = vec![dialog];
    window.active = p.active;
    window.focus = match (&p.focus, p.active) {
        (Some(focus), _) => Some(focus.clone()),
        (None, true) => Some("dialog".to_owned()),
        (None, false) => None,
    };
    window
}

/// The popover's title for `item`.
pub fn popover_title(item: Item, words: Words) -> String {
    let lang = words.lang();
    let levels = crate::levels::model::Words::new(lang);
    let pick = |fr: &str, en: &str| if words.fr { fr } else { en }.to_owned();
    match item {
        Item::Sound => levels.sound().to_owned(),
        Item::Wifi => "Wi-Fi".to_owned(),
        Item::Bluetooth => "Bluetooth".to_owned(),
        Item::Brightness => levels.brightness().to_owned(),
        Item::Battery => levels
            .battery_title(crate::levels::snapshot().battery.is_some())
            .to_owned(),
        Item::Keyboard => pick("Disposition du clavier", "Keyboard layout"),
        Item::Desktop { index, .. } => levels.desktop_title(
            usize::from(index) + 1,
            crate::desktops::name(usize::from(index)).as_deref(),
        ),
        Item::Sources | Item::AnotherTool => "Sources".to_owned(),
        Item::Plug(tool) => format!(
            "{} {}",
            crate::agents::Words::new(lang).plug(),
            tool.title()
        ),
        Item::Decline => pick("Ne rien brancher", "Plug nothing in"),
        Item::Agent(_) | Item::Secretary => crate::agents::Words::new(lang).briefing().to_owned(),
        Item::Notifications => "Notifications".to_owned(),
        Item::Clipboard => pick("Presse-papiers", "Clipboard"),
        Item::Menu => "Menu".to_owned(),
        Item::User => pick("Session", "Session"),
        Item::InUse => pick("En ce moment", "Right now"),
        Item::Music => pick("Musique", "Music"),
        Item::Tray(_) => pick("Menu de l'icône", "Icon's menu"),
        Item::Meeting | Item::MeetingMark | Item::Join => pick("Réunion", "Meeting"),
        Item::TodayLine(_) | Item::TodayMore | Item::Calendar => words.calendar().to_owned(),
        Item::Tasks | Item::TasksMore | Item::TaskAdd | Item::Task(_) => pick("À faire", "To do"),
        Item::DayChoose | Item::DayLater => pick("Ta journée", "Your day"),
        Item::Window(_) | Item::WindowClose(_) | Item::WindowsMore | Item::Windows => {
            pick("Fenêtres", "Windows")
        }
        Item::WindowMenu(_) => pick("Menu de la fenêtre", "Window's menu"),
    }
}

/// A notification's bubble: its application and summary, its body, and
/// what its controls declared (its ✕, its actions, its progress); said as
/// it comes.
#[derive(Debug, Clone, Default)]
pub struct Bubble {
    pub serial: u64,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub urgent: bool,
    /// A click on it does its default action.
    pub default: bool,
    /// Its controls, as the kit read them.
    pub nodes: Vec<Node>,
}

pub fn bubble(b: &Bubble, words: Words) -> Window {
    let name = words.notification(&b.app, &b.summary, b.urgent);
    let mut window = Window::new(format!("bubble:{}", b.serial), name.clone());
    let mut note = Node::new("note", Role::Notification, name)
        .describe(b.body.clone())
        .polite()
        .children(b.nodes.clone());
    if b.default {
        note = note.act(Act::Click);
    }
    window.nodes = vec![note];
    window
}

/// The lock on one screen: the screen with the form shows the clock, the
/// password's field, its button, the layout's and how many agents wait;
/// the others the clock alone.
#[derive(Debug, Clone)]
pub struct Lock {
    pub output: String,
    pub shown: LockShown,
    /// "14 h 32, jeudi 6 novembre".
    pub clock: Option<String>,
    /// The keyboard's layout, when there is a choice.
    pub layout: Option<String>,
}

pub fn lock(l: &Lock, words: Words) -> Window {
    let lock_words = crate::lock::Words::new(words.lang());
    let mut window = Window::new(format!("lock:{}", l.output), words.lock());
    if let Some(clock) = &l.clock {
        window
            .nodes
            .push(Node::new("clock", Role::Label, clock.clone()));
    }
    if l.output != l.shown.output {
        return window;
    }
    let said = match l.shown.phase {
        LockPhase::Checking | LockPhase::Opening => lock_words.checking().to_owned(),
        LockPhase::Wrong => lock_words.wrong(l.shown.caps),
        LockPhase::Waiting(seconds) => lock_words.wait(seconds),
        LockPhase::Idle | LockPhase::Typing if l.shown.caps => lock_words.caps().to_owned(),
        LockPhase::Idle | LockPhase::Typing => String::new(),
    };
    window.nodes.push(
        Node::new("field", Role::PasswordField, lock_words.password())
            .describe(said)
            .disabled(matches!(
                l.shown.phase,
                LockPhase::Checking | LockPhase::Opening | LockPhase::Waiting(_)
            ))
            .act(Act::Focus),
    );
    window
        .nodes
        .push(Node::new("unlock", Role::Button, lock_words.unlock()).act(Act::Click));
    if let Some(layout) = &l.layout {
        window
            .nodes
            .push(Node::new("layout", Role::Button, lock_words.layout(layout)).act(Act::Click));
    }
    if l.shown.agents > 0 {
        window.nodes.push(Node::new(
            "agents",
            Role::Label,
            lock_words.agents(l.shown.agents),
        ));
    }
    window.focus = Some(
        match l.shown.focus {
            LockFocus::Field => "field",
            LockFocus::Button => "unlock",
            LockFocus::Layout if l.layout.is_some() => "layout",
            LockFocus::Layout => "field",
        }
        .to_owned(),
    );
    window.active = true;
    window
}

#[cfg(test)]
mod tests {
    use super::*;

    const FR: Words = Words { fr: true };

    #[test]
    fn a_popover_is_a_dialog_named_by_its_title() {
        let p = Popover {
            title: "Son".to_owned(),
            subtitle: Some("Haut-parleurs du portable".to_owned()),
            rows: vec![
                Node::new("Mute", Role::ToggleButton, "Couper le son").checked(false),
                Node::new("Slider", Role::Slider, "Volume")
                    .value("62\u{a0}%")
                    .range(62.0, 0.0, 100.0, Some(5.0)),
                Node::new("Output(0)", Role::RadioButton, "Haut-parleurs")
                    .checked(true)
                    .set(1, 2),
            ],
            focus: Some("Slider".to_owned()),
            active: true,
        };
        let window = popover(&p);
        let dump = window.dump();
        assert!(
            dump.contains("dialog \"Son\" — \"Haut-parleurs du portable\""),
            "{dump}"
        );
        assert!(
            dump.contains("toggle button \"Couper le son\" [not pressed]"),
            "{dump}"
        );
        assert!(
            dump.contains("radio button \"Haut-parleurs\" [checked] 1/2"),
            "{dump}"
        );
        assert!(window.active);
        assert_eq!(window.focus.as_deref(), Some("Slider"));
        // Hovered open, it does not hold the keyboard.
        let window = popover(&Popover {
            active: false,
            focus: None,
            ..p
        });
        assert!(!window.active);
        assert_eq!(window.focus, None);
    }

    #[test]
    fn a_bubble_is_a_notification_with_its_actions() {
        let b = Bubble {
            serial: 4,
            app: "Slack".to_owned(),
            summary: "Léa Martin".to_owned(),
            body: "On se voit à 15 h ?".to_owned(),
            urgent: false,
            default: true,
            nodes: vec![
                Node::new("bubble:Action(reply)", Role::Button, "Répondre").act(Act::Click),
            ],
        };
        let window = bubble(&b, FR);
        assert_eq!(window.key, "bubble:4");
        let note = window.find("note").expect("the note");
        assert_eq!(note.role, Role::Notification);
        assert_eq!(note.name, "Slack : Léa Martin");
        assert_eq!(note.description.as_deref(), Some("On se voit à 15 h ?"));
        assert!(note.polite);
        let reply = window.find("bubble:Action(reply)").expect("its action");
        assert_eq!(reply.role, Role::Button);
        assert_eq!(reply.name, "Répondre");
        assert_eq!(reply.acts, vec![Act::Click]);
        let urgent = bubble(&Bubble { urgent: true, ..b }, FR);
        assert_eq!(urgent.name, "Slack : Léa Martin, urgent");
    }

    fn shown(phase: LockPhase, focus: LockFocus) -> LockShown {
        LockShown {
            phase,
            chars: 3,
            caps: false,
            shake: 0,
            focus,
            agents: 2,
            wallpaper: true,
            motion: true,
            output: "eDP-1".to_owned(),
        }
    }

    #[test]
    fn the_lock_has_a_password_field_and_its_button() {
        let l = Lock {
            output: "eDP-1".to_owned(),
            shown: shown(LockPhase::Typing, LockFocus::Field),
            clock: Some("14 h 32, jeudi 6 novembre".to_owned()),
            layout: Some("Français".to_owned()),
        };
        let window = lock(&l, FR);
        let dump = window.dump();
        assert!(
            dump.contains("label \"14 h 32, jeudi 6 novembre\""),
            "{dump}"
        );
        let field = window.find("field").expect("the field");
        assert_eq!(field.role, Role::PasswordField);
        assert_eq!(field.name, "Mot de passe");
        // Never what is typed.
        assert_eq!(field.value, None);
        let unlock = window.find("unlock").expect("the button");
        assert_eq!(unlock.name, "Déverrouiller");
        assert_eq!(unlock.acts, vec![Act::Click]);
        assert!(dump.contains("label \"2 agents t’attendent\""), "{dump}");
        assert_eq!(window.focus.as_deref(), Some("field"));
        assert!(window.active);
        let wrong = lock(
            &Lock {
                shown: shown(LockPhase::Wrong, LockFocus::Button),
                ..l.clone()
            },
            FR,
        );
        assert_eq!(
            wrong.find("field").and_then(|f| f.description.as_deref()),
            Some("Mot de passe incorrect.")
        );
        assert_eq!(wrong.focus.as_deref(), Some("unlock"));
        // Another screen: the clock alone.
        let other = lock(
            &Lock {
                output: "HDMI-1".to_owned(),
                ..l
            },
            FR,
        );
        assert_eq!(other.nodes.len(), 1);
        assert!(!other.active);
    }
}
