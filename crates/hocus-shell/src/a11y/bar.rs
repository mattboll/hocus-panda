//! A bar as screen readers see it: its items, in the order and at the
//! places its surfaces show them, gathered in the groups they make (Today,
//! To do, the agents, the windows, the desktops, the footer). Each surface
//! declares its items' nodes and its group to the kit's ring as it draws
//! ([`crate::ui::ring`](mod@crate::ui::ring), [`crate::ui::items`]): here they are only put
//! together, at their places, the one holding the ring the focus.

use hocus_a11y::{Node, Rect, Role, Window};

use super::words::Words;
use crate::{popover::Item, ui::items::key};

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
    /// What is said on its own now, and its number: a new number is a new
    /// announcement.
    pub announce: Option<(u64, String)>,
}

/// The bar's window.
pub fn window(facts: &Facts, words: Words) -> Window {
    let mut window = Window::new(format!("bar:{}", facts.bar), words.bar(&facts.screen));
    let mut groups: Vec<(&'static str, Vec<Node>)> = Vec::new();
    for (namespace, item, rect) in &facts.items {
        let Some((_, node, part)) = facts.nodes.iter().find(|(i, _, _)| i == item) else {
            continue;
        };
        let node = node.clone().bounds(Some(*rect));
        let node = if facts.open == Some(*item) {
            node.expanded(true)
        } else {
            node
        };
        let part = *part;
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
        window.nodes.extend(group(namespace, nodes, facts));
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

/// A surface's items in the group it declares, if it declares one: its
/// own lines (what nothing holds, "11 autres agents") after them.
fn group(namespace: &str, nodes: Vec<Node>, facts: &Facts) -> Vec<Node> {
    match facts.groups.iter().find(|(n, _)| *n == namespace) {
        Some((_, group)) => {
            let mut group = group.clone();
            let lines = std::mem::take(&mut group.children);
            vec![group.children(nodes).children(lines)]
        }
        None => nodes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_a11y::Act;
    use hocus_link::{
        AGENTS_NAMESPACE, AgentShown, CLOCK_NAMESPACE, CONNECTIONS_NAMESPACE, DESKTOPS_NAMESPACE,
        FOOTER_NAMESPACE, LEVELS_NAMESPACE, WINDOWS_NAMESPACE,
    };
    use hocus_network::model::Link;

    use crate::{
        clock::time::Shown,
        levels::model::{Battery, Charge, Profile, Snapshot, Sound},
    };

    const FR: Words = Words {
        lang: hocus_words::Lang::French,
    };
    const EN: Words = Words {
        lang: hocus_words::Lang::English,
    };
    const MINUTE: u64 = 60_000_000;

    fn agent(id: u64, name: &str, state: &str, since: u64) -> AgentShown {
        AgentShown {
            id,
            state: state.to_owned(),
            since,
            source: "test".to_owned(),
            window: 0,
            name: name.to_owned(),
            note: None,
            news: None,
        }
    }

    /// Three agents waiting and ten at work.
    fn agents() -> Vec<AgentShown> {
        let mut agents = vec![
            agent(1, "site-web", "waiting", 8 * MINUTE),
            agent(2, "docs", "waiting", 9 * MINUTE),
            agent(3, "api", "waiting", 9 * MINUTE),
        ];
        for i in 0..10 {
            agents.push(agent(10 + i, &format!("w{i}"), "working", 0));
        }
        agents
    }

    fn at(y: f32) -> Rect {
        Rect::new(8.0, y, 240.0, 24.0)
    }

    /// The mockup's bar as its surfaces declare it: 14:32 on Thursday 6
    /// November, three agents waiting and ten at work, four desktops, the
    /// Wi-Fi, the volume at 62 %, the battery at 82 %.
    fn facts(date: &str) -> Facts {
        let fr = !date.starts_with("Thursday");
        let words = if fr { FR } else { EN };
        let desktop = if fr { "Bureau" } else { "Desktop" };
        let agents = agents();
        let now = 10 * MINUTE;
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
        let clock = Shown {
            hm: ['1', '4', ':', '3', '2'],
            ss: ['0', '7'],
            half: None,
            date: date.to_owned(),
        };
        let levels = Snapshot {
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
        };
        let link = Link::Connected {
            name: "Livebox-A3F2".to_owned(),
            bars: 4,
        };
        let declared = |item: Item, node: Option<Node>| (item, node.expect("a node"), false);
        let agent = |item| declared(item, crate::agents::draw::node(words, item, &agents, now));
        let connection = |item| {
            declared(
                item,
                crate::network::draw::node(words, item, Some(&link), None, Some("Français")),
            )
        };
        let level = |item| declared(item, crate::levels::draw::node(words, item, &levels));
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
            nodes: vec![
                (
                    Item::Calendar,
                    crate::clock::item::node(words.lang(), &clock),
                    false,
                ),
                agent(Item::Secretary),
                agent(Item::Agent(1)),
                agent(Item::Agent(2)),
                desk(0, true, &["Terminal", "Firefox"]),
                desk(1, false, &["Firefox"]),
                connection(Item::Wifi),
                connection(Item::Keyboard),
                level(Item::Sound),
                level(Item::Battery),
                (
                    Item::Notifications,
                    Node::new("Notifications", Role::Button, "Notifications, 2 non vues"),
                    false,
                ),
            ],
            groups: vec![
                (
                    AGENTS_NAMESPACE,
                    crate::agents::draw::group(words, 3, &agents, now, 2)
                        .expect("the agents' list"),
                ),
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
        // The line nothing holds after the rows it counts.
        let list = window.find("agents").expect("the agents' list");
        assert_eq!(
            list.children.last().map(|n| n.key.as_str()),
            Some("agents-more")
        );
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
        let mut agents = agents();
        agents[0].name = "site\u{1b}-web\n".to_owned();
        let row =
            crate::agents::draw::node(FR, Item::Agent(1), &agents, 10 * MINUTE).expect("the row");
        assert_eq!(row.name, "site-web, t'attend depuis 2 minutes");
    }
}
