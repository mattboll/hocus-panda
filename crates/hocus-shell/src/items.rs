//! The bar's items, from one list (#833): what each bar is made of, and
//! where each item stands, read from [`ITEMS`] rather than written out
//! item by item where a bar opens, changes form, shows or goes. A later
//! item (an add-on's, #442) is an entry of the list and what its kind
//! does in [`crate::Shell`]'s `open_item`, `item_view`, `item_form` and
//! `forget_item`.
//!
//! An entry says the item's [`Kind`], the end of the bar it stands from
//! ([`Side`]: under the bar's top or over its bottom; not `[panel] side`,
//! the screen's edge the bar is on), and its `order` from that end. The
//! list's own order is the order the items are opened in, back to front:
//! a surface opened later is in front of those before it (the footer
//! over the others). The items that come and go (the tray's icons, the
//! sign of what is in use, the meeting's card, Today, the windows, the
//! music) open as they come, in front of the bar's own.
//!
//! The top's order is the column's ([`crate::column::order`]): the clock
//! heads it, then the others top to bottom, Today, the to-do list and the
//! agents taking their places in the configuration's order (`[panel]
//! order`). The bottom's order, from the bar's bottom up, is how each of
//! those places itself over the one under it.

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// The time and the date (§4 group 1).
    Clock,
    /// The next meeting's card, or its mark (Day.2).
    Meeting,
    /// The sign of what uses the microphone, the camera or the screen
    /// (5.12).
    InUse,
    /// Today's events (Day.3), in the full form only.
    Today,
    /// The to-do list (Day.4).
    Tasks,
    /// The agents' group (4.0 to 4.7).
    Agents,
    /// The windows over the desktops (#604).
    Windows,
    /// The music player's tile (s12).
    Music,
    /// The desktops' tiles (§4 group 9).
    Desktops,
    /// The connections: Wi-Fi, Bluetooth, the keyboard's layout.
    Connections,
    /// The levels: sound, brightness, battery.
    Levels,
    /// The tray's icons (5.14).
    Tray,
    /// The footer's buttons (5.5).
    Footer,
}

/// The end of the bar an item stands from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// Under the bar's top, top to bottom.
    Top,
    /// Over the bar's bottom, bottom to top.
    Bottom,
}

/// An item of the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub kind: Kind,
    pub side: Side,
    /// Its place from its side's end, the first 0.
    pub order: u8,
}

const fn item(kind: Kind, side: Side, order: u8) -> Item {
    Item { kind, side, order }
}

/// The bar's items, in the order they are opened. The tray is first:
/// it opens nothing with the bar, and as the bar changes form the items
/// over it go up by what it takes.
pub const ITEMS: [Item; 13] = [
    item(Kind::Tray, Side::Bottom, 1),
    item(Kind::Clock, Side::Top, 0),
    item(Kind::Levels, Side::Bottom, 2),
    item(Kind::Connections, Side::Bottom, 3),
    item(Kind::Desktops, Side::Bottom, 4),
    item(Kind::Agents, Side::Top, 5),
    item(Kind::Tasks, Side::Top, 4),
    item(Kind::Footer, Side::Bottom, 0),
    item(Kind::InUse, Side::Top, 2),
    item(Kind::Meeting, Side::Top, 1),
    item(Kind::Today, Side::Top, 3),
    item(Kind::Windows, Side::Bottom, 6),
    item(Kind::Music, Side::Bottom, 5),
];

/// The items of `items` on `side`, from that end, in their order.
pub fn on_side(items: &[Item], side: Side) -> Vec<Kind> {
    let mut on: Vec<Item> = items.iter().filter(|i| i.side == side).copied().collect();
    on.sort_by_key(|i| i.order);
    on.into_iter().map(|i| i.kind).collect()
}

/// The bar's items on `side`, from that end.
pub fn on(side: Side) -> Vec<Kind> {
    on_side(&ITEMS, side)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bars_items_stand_in_the_lists_order_from_each_end() {
        // Top to bottom: the clock, the meeting's card, the sign, Today,
        // To do, the agents (bar.md §4, #599).
        assert_eq!(
            on(Side::Top),
            [
                Kind::Clock,
                Kind::Meeting,
                Kind::InUse,
                Kind::Today,
                Kind::Tasks,
                Kind::Agents
            ]
        );
        // Bottom up: the footer, the tray, the levels, the connections,
        // the desktops, the music's tile over them, the windows.
        assert_eq!(
            on(Side::Bottom),
            [
                Kind::Footer,
                Kind::Tray,
                Kind::Levels,
                Kind::Connections,
                Kind::Desktops,
                Kind::Music,
                Kind::Windows
            ]
        );
        // Each item once, each place on a side once.
        for (at, a) in ITEMS.iter().enumerate() {
            for b in ITEMS.iter().skip(at + 1) {
                assert_ne!(a.kind, b.kind);
                assert!(a.side != b.side || a.order != b.order, "{a:?} {b:?}");
            }
        }
        // An entry moved in the list moves its item.
        let mut moved = ITEMS;
        for i in &mut moved {
            match i.kind {
                Kind::InUse => i.order = 1,
                Kind::Meeting => i.order = 2,
                _ => {}
            }
        }
        assert_eq!(
            on_side(&moved, Side::Top).get(1..3),
            Some([Kind::InUse, Kind::Meeting].as_slice())
        );
    }
}
