//! A bar as screen readers see it: its items, in the order and at the
//! places its surfaces show them (the kit's ring's declarations, which the
//! keyboard steps through), gathered in the groups they make (Today, To
//! do, the agents, the windows, the desktops, the footer). The surfaces on
//! the kit ([`crate::ui::items`]) declare their nodes and groups
//! themselves; the others' are named here with what they draw.

use hocus_a11y::{Act, Node, Rect, Role, Window};
use hocus_bluetooth::model::Row as BluetoothRow;
use hocus_link::{AGENTS_NAMESPACE, AgentShown, TASKS_NAMESPACE};
use hocus_network::model::Link;

use super::words::{Words, plain};
use crate::{
    agents::{Brief, Lines, Look},
    clock::time::Shown,
    levels::model::Snapshot,
    popover::Item,
};

/// A task's line: its title, ticked.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    pub title: String,
    pub ticked: bool,
}

/// What a bar shows, gathered from the shell's model (or made by a test).
#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub bar: String,
    /// Its screen's name, as the Settings name it.
    pub screen: String,
    /// Its surface's size.
    pub size: Option<(f32, f32)>,
    /// Its items, from the top, on its surface, with their surface's
    /// namespace.
    pub items: Vec<(&'static str, Item, Rect)>,
    /// The same in the bar's frame, as a click on them says it.
    pub frame: Vec<(Item, iced::Rectangle)>,
    /// The nodes the kit's surfaces declare for their items, each a part
    /// of the one before it or not.
    pub nodes: Vec<(Item, Node, bool)>,
    /// The groups the kit's surfaces declare, by namespace.
    pub groups: Vec<(&'static str, Node)>,
    /// The item holding the keyboard (after `Esc` in its popover).
    pub focused: Option<Item>,
    /// The item whose popover is open.
    pub open: Option<Item>,
    pub clock: Option<Shown>,
    pub tasks: Vec<Task>,
    pub open_tasks: usize,
    pub more_tasks: usize,
    pub agents: Vec<AgentShown>,
    /// Now, on the agents' `since`'s clock (microseconds).
    pub now_us: u64,
    pub rows_shown: usize,
    pub link: Option<Link>,
    pub bluetooth: Option<BluetoothRow>,
    pub layout: Option<String>,
    pub levels: Snapshot,
    /// What is said on its own now, and its number: a new number is a new
    /// announcement.
    pub announce: Option<(u64, String)>,
}

/// An item's key in its window: its debug form, unique and stable.
pub fn key(item: Item) -> String {
    format!("{item:?}")
}

/// The bar's window.
pub fn window(facts: &Facts, words: Words) -> Window {
    let mut window = Window::new(format!("bar:{}", facts.bar), words.bar(&facts.screen));
    let mut groups: Vec<(&'static str, Vec<Node>)> = Vec::new();
    for (namespace, item, rect) in &facts.items {
        let (node, part) = match facts.nodes.iter().find(|(i, _, _)| i == item) {
            Some((_, node, part)) => {
                let node = node.clone().bounds(Some(*rect));
                let node = if facts.open == Some(*item) {
                    node.expanded(true)
                } else {
                    node
                };
                (node, *part)
            }
            None => match item_node(*item, *rect, facts, words) {
                Some(node) => (node, false),
                None => continue,
            },
        };
        match groups.last_mut() {
            // A line's ✕ is its child.
            Some((last, nodes)) if last == namespace && part => match nodes.last_mut() {
                Some(line) => line.children.push(node),
                None => nodes.push(node),
            },
            Some((last, nodes)) if last == namespace => nodes.push(node),
            _ => groups.push((namespace, vec![node])),
        }
    }
    for (namespace, nodes) in groups {
        window.nodes.extend(group(namespace, nodes, facts, words));
    }
    if let Some((n, said)) = &facts.announce {
        window
            .nodes
            .push(Node::new(format!("announce:{n}"), Role::Status, said.clone()).polite());
    }
    window.focus = facts.focused.map(key);
    window.active = facts.focused.is_some();
    window.size = facts.size;
    window
}

/// A surface's items in the group they make, if they make one: the one
/// a surface of the kit declares, else as said here.
fn group(namespace: &str, mut nodes: Vec<Node>, facts: &Facts, words: Words) -> Vec<Node> {
    if let Some((_, group)) = facts.groups.iter().find(|(n, _)| *n == namespace) {
        return vec![group.clone().children(nodes)];
    }
    let full = nodes.len() > 1;
    let container = match namespace {
        TASKS_NAMESPACE if full => Some(Node::new(
            "tasks",
            Role::List,
            words.tasks(facts.open_tasks),
        )),
        AGENTS_NAMESPACE if full || facts.agents.len() > facts.rows_shown => {
            let lines = Lines::of_at_most(&facts.agents, facts.now_us, facts.rows_shown);
            if lines.hidden > 0 {
                nodes.push(Node::new(
                    "agents-more",
                    Role::Label,
                    words.other_agents(lines.hidden, lines.hidden_rest),
                ));
            }
            Some(Node::new("agents", Role::List, "Agents"))
        }
        _ => None,
    };
    match container {
        Some(container) => vec![container.children(nodes)],
        None => nodes,
    }
}

/// One item's node, named with what it draws.
fn item_node(item: Item, rect: Rect, facts: &Facts, words: Words) -> Option<Node> {
    let lang = words.lang();
    let button = |name: String| {
        Node::new(key(item), Role::Button, name)
            .bounds(Some(rect))
            .act(Act::Click)
    };
    let node = match item {
        Item::Calendar => button(facts.clock.as_ref().map(|c| words.clock(c))?),
        Item::Music => button(crate::music::Words::new(lang).music().to_owned()),
        Item::Tasks => button(words.tasks(facts.open_tasks)),
        Item::TasksMore => button(words.more_tasks(facts.more_tasks)),
        Item::TaskAdd => button(words.add_task().to_owned()),
        Item::Task(slot) => {
            let task = facts.tasks.get(usize::from(slot))?;
            Node::new(key(item), Role::CheckBox, task.title.clone())
                .checked(task.ticked)
                .bounds(Some(rect))
                .act(Act::Click)
        }
        Item::DayChoose => button(words.choose_sources().to_owned()),
        Item::DayLater => button(words.later().to_owned()),
        Item::Secretary => {
            let brief = Brief::of(&facts.agents);
            button(words.agents(
                brief.waiting.len(),
                brief.stuck.len(),
                brief.working.len(),
                brief.to_see.len(),
            ))
        }
        Item::Agent(id) => {
            let agent = facts.agents.iter().find(|a| a.id == id)?;
            let look = Look::of(&agent.state);
            let minutes = facts.now_us.saturating_sub(agent.since) / 60_000_000;
            button(words.agent(&plain(&agent.name), look, minutes))
        }
        Item::Sources => button("Sources".to_owned()),
        Item::AnotherTool => button(crate::agents::Words::new(lang).another().to_owned()),
        Item::Plug(tool) => button(format!(
            "{} {}",
            crate::agents::Words::new(lang).plug(),
            tool.title()
        )),
        Item::Decline => button(crate::agents::Words::new(lang).decline().to_owned()),
        Item::Wifi => button(words.wifi(facts.link.as_ref()?)),
        Item::Bluetooth => button(words.bluetooth(facts.bluetooth.as_ref()?)),
        Item::Keyboard => button(words.layout(facts.layout.as_deref()?)),
        Item::Sound => {
            let sound = facts.levels.sound?;
            let value = if sound.muted {
                words.muted().to_owned()
            } else {
                words.percent(sound.volume)
            };
            Node::new(key(item), Role::Slider, words.volume())
                .value(value)
                .range(f64::from(sound.volume), 0.0, 100.0, Some(5.0))
                .bounds(Some(rect))
                .act(Act::Click)
                .act(Act::Increment)
                .act(Act::Decrement)
        }
        Item::Brightness => {
            let brightness = facts.levels.brightness?;
            Node::new(
                key(item),
                Role::Slider,
                crate::levels::model::Words::new(lang).brightness(),
            )
            .value(words.percent(brightness))
            .range(f64::from(brightness), 0.0, 100.0, Some(5.0))
            .bounds(Some(rect))
            .act(Act::Click)
            .act(Act::Increment)
            .act(Act::Decrement)
        }
        Item::Battery => match facts.levels.battery {
            Some(battery) => Node::new(key(item), Role::ProgressBar, words.battery())
                .value(words.battery_value(&battery, &facts.levels))
                .range(f64::from(battery.shown()), 0.0, 100.0, None)
                .bounds(Some(rect))
                .act(Act::Click),
            None => button(words.power(&facts.levels)),
        },
        // Declared by their surfaces on the kit.
        Item::Notifications
        | Item::Clipboard
        | Item::Menu
        | Item::User
        | Item::InUse
        | Item::Meeting
        | Item::MeetingMark
        | Item::Join
        | Item::TodayLine(_)
        | Item::TodayMore
        | Item::Tray(_)
        | Item::Window(_)
        | Item::WindowClose(_)
        | Item::WindowsMore
        | Item::Windows
        | Item::Desktop { .. } => return None,
        // Not the bar's own: shown in a popover.
        Item::WindowMenu(_) => return None,
    };
    Some(if facts.open == Some(item) {
        node.expanded(true)
    } else {
        node
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_link::{
        CLOCK_NAMESPACE, CONNECTIONS_NAMESPACE, DESKTOPS_NAMESPACE, FOOTER_NAMESPACE,
        LEVELS_NAMESPACE, WINDOWS_NAMESPACE,
    };

    use crate::levels::model::{Battery, Charge, Profile, Sound};

    const FR: Words = Words {
        lang: hocus_words::Lang::French,
    };
    const EN: Words = Words {
        lang: hocus_words::Lang::English,
    };

    fn agent(id: u64, name: &str, state: &str, since: u64) -> AgentShown {
        AgentShown {
            id,
            state: state.to_owned(),
            since,
            source: "test".to_owned(),
            window: 0,
            name: name.to_owned(),
            note: None,
        }
    }

    fn at(y: f32) -> Rect {
        Rect::new(8.0, y, 240.0, 24.0)
    }

    /// The mockup's bar: 14:32 on Thursday 6 November, three agents
    /// waiting and ten at work, four desktops, the Wi-Fi, the volume at
    /// 62 %, the battery at 82 %.
    fn facts(date: &str) -> Facts {
        let fr = !date.starts_with("Thursday");
        let desktop = if fr { "Bureau" } else { "Desktop" };
        let minute = 60_000_000;
        let mut agents = vec![
            agent(1, "site-web", "waiting", 8 * minute),
            agent(2, "docs", "waiting", 9 * minute),
            agent(3, "api", "waiting", 9 * minute),
        ];
        for i in 0..10 {
            agents.push(agent(10 + i, &format!("w{i}"), "working", 0));
        }
        let words = if fr { FR } else { EN };
        // The desktops' tiles as their surface declares them.
        let desk = |index: u8, current: bool, windows: &[&str]| {
            let item = Item::Desktop { row: 0, index };
            let windows: Vec<String> = windows.iter().map(|w| (*w).to_owned()).collect();
            let title = format!("{desktop} {}", index + 1);
            let node = Node::new(
                key(item),
                Role::RadioButton,
                words.desktop(&title, current, &windows),
            )
            .checked(current)
            .set(usize::from(index) + 1, 4)
            .act(Act::Click);
            (item, node, false)
        };
        Facts {
            bar: "eDP-1".to_owned(),
            screen: if fr {
                "Écran du portable"
            } else {
                "Laptop screen"
            }
            .to_owned(),
            items: vec![
                (CLOCK_NAMESPACE, Item::Calendar, at(10.0)),
                (AGENTS_NAMESPACE, Item::Secretary, at(100.0)),
                (AGENTS_NAMESPACE, Item::Agent(1), at(130.0)),
                (AGENTS_NAMESPACE, Item::Agent(2), at(160.0)),
                (
                    DESKTOPS_NAMESPACE,
                    Item::Desktop { row: 0, index: 0 },
                    at(300.0),
                ),
                (
                    DESKTOPS_NAMESPACE,
                    Item::Desktop { row: 0, index: 1 },
                    at(330.0),
                ),
                (CONNECTIONS_NAMESPACE, Item::Wifi, at(400.0)),
                (CONNECTIONS_NAMESPACE, Item::Keyboard, at(430.0)),
                (LEVELS_NAMESPACE, Item::Sound, at(500.0)),
                (LEVELS_NAMESPACE, Item::Battery, at(560.0)),
                (FOOTER_NAMESPACE, Item::Notifications, at(700.0)),
            ],
            clock: Some(Shown {
                hm: ['1', '4', ':', '3', '2'],
                ss: ['0', '7'],
                half: None,
                date: date.to_owned(),
            }),
            agents,
            now_us: 10 * minute,
            rows_shown: 2,
            link: Some(Link::Connected {
                name: "Livebox-A3F2".to_owned(),
                bars: 4,
            }),
            layout: Some("Français".to_owned()),
            levels: Snapshot {
                sound: Some(Sound {
                    volume: 62.0,
                    muted: false,
                }),
                battery: Some(Battery {
                    percent: 82.0,
                    charge: Charge::Discharging,
                    left: Some(std::time::Duration::from_secs(3 * 3600 + 600)),
                    health: None,
                }),
                profile: Some(Profile::Balanced),
                ..Snapshot::default()
            },
            nodes: vec![
                desk(0, true, &["Terminal", "Firefox"]),
                desk(1, false, &["Firefox"]),
                (
                    Item::Notifications,
                    Node::new("Notifications", Role::Button, "Notifications, 2 non vues"),
                    false,
                ),
            ],
            groups: vec![
                (
                    DESKTOPS_NAMESPACE,
                    Node::new("desktops", Role::RadioGroup, words.desktops()),
                ),
                (
                    FOOTER_NAMESPACE,
                    Node::new("footer", Role::Toolbar, "Pied de la barre"),
                ),
            ],
            ..Facts::default()
        }
    }

    #[test]
    fn the_bar_reads_as_the_mockup_says_in_french() {
        let window = window(&facts("jeudi 6 novembre"), FR);
        assert_eq!(window.name, "Barre — Écran du portable");
        let dump = window.dump();
        for line in [
            "button \"14 h 32, jeudi 6 novembre\" @8,10 240x24 <click>",
            "list \"Agents\"",
            "button \"Agents : 3 t'attendent, 10 au travail\"",
            "button \"site-web, t'attend depuis 2 minutes\"",
            "label \"11 autres agents\"",
            "radio group \"Bureaux\"",
            "radio button \"Bureau 1, actuel, 2 fenêtres : Terminal, Firefox\" [checked] 1/4",
            "radio button \"Bureau 2, 1 fenêtre : Firefox\" [not checked] 2/4",
            "button \"Wi-Fi, connecté à Livebox-A3F2, signal fort\"",
            "button \"Disposition du clavier, Français\"",
            "slider \"Volume\" = \"62\u{a0}%\" (62 in 0..100 by 5)",
            "progress bar \"Batterie\" = \"82\u{a0}%, 3 h 10 restantes, équilibré\" (82 in 0..100)",
            "toolbar \"Pied de la barre\"",
            "button \"Notifications, 2 non vues\"",
        ] {
            assert!(dump.contains(line), "no {line:?} in\n{dump}");
        }
        // The volume can be moved from the bar.
        let volume = window.find("Sound").expect("the volume");
        assert!(volume.acts.contains(&Act::Increment));
        assert!(volume.acts.contains(&Act::Decrement));
    }

    #[test]
    fn the_bar_reads_in_english_too() {
        let window = window(&facts("Thursday 6 November"), EN);
        let dump = window.dump();
        for line in [
            "button \"14:32, Thursday 6 November\"",
            "button \"Agents: 3 waiting, 10 working\"",
            "button \"site-web, waiting for 2 minutes\"",
            "radio button \"Desktop 2, 1 window: Firefox\"",
            "button \"Wi-Fi, connected to Livebox-A3F2, strong signal\"",
            "progress bar \"Battery\" = \"82\u{a0}%, 3 h 10 left, balanced\"",
        ] {
            assert!(dump.contains(line), "no {line:?} in\n{dump}");
        }
    }

    #[test]
    fn the_item_holding_the_keyboard_is_the_focus() {
        let mut facts = facts("jeudi 6 novembre");
        let window = super::window(&facts, FR);
        assert!(!window.active);
        assert_eq!(window.focus, None);
        facts.focused = Some(Item::Sound);
        let window = super::window(&facts, FR);
        assert!(window.active);
        assert_eq!(window.focus.as_deref(), Some("Sound"));
        // Its popover open: the item says so.
        facts.open = Some(Item::Sound);
        let window = super::window(&facts, FR);
        assert_eq!(window.find("Sound").and_then(|n| n.expanded), Some(true));
    }

    #[test]
    fn an_announcement_is_a_polite_status_of_its_own() {
        let mut facts = facts("jeudi 6 novembre");
        facts.announce = Some((3, "site-web t'attend.".to_owned()));
        let window = super::window(&facts, FR);
        let said = window.find("announce:3").expect("the announcement");
        assert_eq!(said.role, Role::Status);
        assert!(said.polite);
        assert_eq!(said.name, "site-web t'attend.");
    }

    #[test]
    fn a_part_a_surface_declares_is_its_controls_child() {
        let mut facts = facts("jeudi 6 novembre");
        facts.items = vec![
            (WINDOWS_NAMESPACE, Item::Window(7), at(10.0)),
            (WINDOWS_NAMESPACE, Item::WindowClose(7), at(10.0)),
            (WINDOWS_NAMESPACE, Item::Window(8), at(40.0)),
        ];
        let node = |item: Item, role: Role, name: &str| Node::new(key(item), role, name);
        facts.nodes = vec![
            (
                Item::Window(7),
                node(Item::Window(7), Role::ListItem, "Slack — #produit").selected(true),
                false,
            ),
            (
                Item::WindowClose(7),
                node(
                    Item::WindowClose(7),
                    Role::Button,
                    "Fermer Slack — #produit",
                ),
                true,
            ),
            (
                Item::Window(8),
                node(Item::Window(8), Role::ListItem, "Terminal"),
                false,
            ),
        ];
        facts.groups = vec![(
            WINDOWS_NAMESPACE,
            Node::new("windows", Role::List, "Fenêtres, 2"),
        )];
        let window = super::window(&facts, FR);
        let list = window.find("windows").expect("the list");
        assert_eq!(list.role, Role::List);
        assert_eq!(list.name, "Fenêtres, 2");
        assert_eq!(list.children.len(), 2);
        let slack = &list.children[0];
        assert_eq!(slack.selected, Some(true));
        assert_eq!(slack.children[0].name, "Fermer Slack — #produit");
        // Placed where the bar shows it.
        assert_eq!(slack.bounds, Some(at(10.0)));
    }

    #[test]
    fn a_name_with_control_characters_is_said_as_drawn() {
        let mut facts = facts("jeudi 6 novembre");
        facts.agents[0].name = "site\u{1b}-web\n".to_owned();
        let window = super::window(&facts, FR);
        let row = window.find("Agent(1)").expect("the row");
        assert_eq!(row.name, "site-web, t'attend depuis 2 minutes");
    }
}
