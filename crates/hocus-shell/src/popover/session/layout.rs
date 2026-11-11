//! Where the session menu's parts are (7.4, variant A), logical pixels
//! in the popover, from what it shows: its rows, a confirmation counting
//! down, or the applications holding it back. Pure: what measures text is
//! handed in.

use hocus_link::{HolderKind, LeaveAction, LeaveCan, LeaveStep};
use iced::{Point, Rectangle, Size};

use super::words::Words;
use crate::popover::kit;

/// The header: 40 px, the avatar 40 px (#649), the name 16 px.
pub const HEADER: f32 = 40.0;
pub const AVATAR: f32 = 40.0;
/// The rows: 6 px under the header, 44 px each, a 1 px divider 6 px
/// from each group; reaching 6 px past the content's sides.
const ROWS_GAP: f32 = 6.0;
pub const ROW: f32 = 44.0;
const DIVIDER_GAP: f32 = 6.0;
/// The confirmation: 8 px under the header, its parts 10 px apart; the
/// title's tile 30 px; the sentence's line; the bar 4 px; the buttons
/// 36 px, 6 px between them.
const CONFIRM_GAP: f32 = 8.0;
const PART_GAP: f32 = 10.0;
pub const TILE: f32 = 30.0;
pub const TITLE: f32 = 15.0;
pub const SENTENCE: f32 = 13.5;
pub const HELD_SENTENCE: f32 = 13.0;
pub const BAR: f32 = 4.0;
pub const BUTTON: f32 = 36.0;
const BUTTON_GAP: f32 = 6.0;
/// The text's lines, as the popovers' notes: 1.45.
pub const LINE: f32 = 1.45;
/// An application's block: padded 10, its icon 32 px, 10 px before its
/// words; its name 13.5 px, its reason 12.5 px, then 8 px and its buttons
/// (28 px, 10 px of padding round their word); 6 px between blocks.
const BLOCK_PAD: f32 = 10.0;
pub const ICON: f32 = 32.0;
const ICON_GAP: f32 = 10.0;
pub const NAME: f32 = 13.5;
pub const REASON: f32 = 12.5;
const BUTTONS_GAP: f32 = 8.0;
pub const SMALL: f32 = 28.0;
const SMALL_PAD: f32 = 10.0;
const SMALL_APART: f32 = 6.0;
const BLOCK_GAP: f32 = 6.0;

/// The menu's rows, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Settings,
    Lock,
    Suspend,
    Leave(LeaveAction),
}

/// What can be clicked, or hold the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Row(Row),
    /// "… maintenant".
    Now,
    Cancel,
    /// A block's "Revenir à …", by its place in the list.
    Back(usize),
    /// A window's "Fermer quand même".
    Force(usize),
    /// "… quand même".
    Anyway,
}

/// A row's place, and whether it can be chosen.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub row: Row,
    pub rect: Rectangle,
    pub enabled: bool,
}

/// An application holding it back, as the shell names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    pub kind: HolderKind,
    /// Its id, for its icon.
    pub app: String,
    pub name: String,
    pub reason: String,
}

/// One block of the list.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub rect: Rectangle,
    pub icon: Rectangle,
    pub name: String,
    pub name_middle: f32,
    pub reason: Vec<String>,
    pub reason_top: f32,
    pub back: Rectangle,
    pub back_word: String,
    pub force: Option<Rectangle>,
}

/// What the popover shows under its header.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Rows {
        rows: Vec<Placed>,
        dividers: Vec<f32>,
    },
    /// The confirmation: its title's row at `top`, its sentence's middle,
    /// and the bar and buttons it has at this step.
    Confirm {
        action: LeaveAction,
        top: f32,
        sentence: f32,
        bar: Option<Rectangle>,
        now: Option<Rectangle>,
        cancel: Option<Rectangle>,
    },
    /// The applications holding it back.
    Held {
        action: LeaveAction,
        top: f32,
        title: Vec<String>,
        sentence: Vec<String>,
        sentence_top: f32,
        blocks: Vec<Block>,
        anyway: Rectangle,
        cancel: Rectangle,
    },
}

/// The popover's parts, and where its content ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub view: View,
    pub end: f32,
}

/// Text widths: `(text, size, bold)`.
pub type Measure<'a> = &'a mut dyn FnMut(&str, f32, bool) -> f32;

const fn row_rect(top: f32) -> Rectangle {
    Rectangle {
        x: kit::LEFT - kit::ROW_OUT,
        y: top,
        width: kit::RIGHT - kit::LEFT + 2.0 * kit::ROW_OUT,
        height: ROW,
    }
}

fn full(top: f32, height: f32) -> Rectangle {
    Rectangle::new(
        Point::new(kit::LEFT, top),
        Size::new(kit::RIGHT - kit::LEFT, height),
    )
}

/// The header's bottom.
pub const fn header_end() -> f32 {
    kit::HEADER_TOP + HEADER
}

/// The menu's rows: "Réglages" (6.1); "Verrouiller" and "Mettre en
/// veille" (none when logind cannot suspend); "Se déconnecter",
/// "Redémarrer", "Éteindre" (greyed when logind does not allow them).
pub fn rows(can: LeaveCan) -> Layout {
    let groups: [Vec<Placed>; 3] = [
        vec![Placed {
            row: Row::Settings,
            rect: Rectangle::default(),
            enabled: true,
        }],
        std::iter::once(Row::Lock)
            .chain(can.suspend.allowed().then_some(Row::Suspend))
            .map(|row| Placed {
                row,
                rect: Rectangle::default(),
                enabled: true,
            })
            .collect(),
        LeaveAction::ALL
            .into_iter()
            .map(|action| Placed {
                row: Row::Leave(action),
                rect: Rectangle::default(),
                enabled: can.of(action).allowed(),
            })
            .collect(),
    ];
    let mut y = header_end() + ROWS_GAP;
    let mut rows = Vec::new();
    let mut dividers = Vec::new();
    for (i, group) in groups.into_iter().enumerate() {
        if i > 0 {
            dividers.push(y + DIVIDER_GAP);
            y += 2.0 * DIVIDER_GAP + 1.0;
        }
        for mut placed in group {
            placed.rect = row_rect(y);
            y += ROW;
            rows.push(placed);
        }
    }
    Layout {
        view: View::Rows { rows, dividers },
        end: y,
    }
}

/// The confirmation at `step` (any but the list).
pub fn confirm(action: LeaveAction, step: &LeaveStep) -> Layout {
    let top = header_end() + CONFIRM_GAP;
    let sentence_top = top + TILE + PART_GAP;
    let sentence = sentence_top + SENTENCE * LINE / 2.0;
    let after = sentence_top + (SENTENCE * LINE).round() + PART_GAP;
    let (bar, now, cancel, end) = match step {
        LeaveStep::Counting { .. } | LeaveStep::Closing | LeaveStep::Held(_) => {
            let bar = full(after, BAR);
            let now = full(after + BAR + PART_GAP, BUTTON);
            let cancel = full(now.y + BUTTON + BUTTON_GAP, BUTTON);
            (Some(bar), Some(now), Some(cancel), cancel.y + BUTTON)
        }
        LeaveStep::Authorizing => {
            let cancel = full(after, BUTTON);
            (None, None, Some(cancel), cancel.y + BUTTON)
        }
        LeaveStep::Acting => (None, None, None, after - PART_GAP),
    };
    Layout {
        view: View::Confirm {
            action,
            top,
            sentence,
            bar,
            now,
            cancel,
        },
        end,
    }
}

/// The list of what holds it back.
pub fn held(action: LeaveAction, holders: &[Named], words: Words, measure: Measure<'_>) -> Layout {
    let top = header_end() + CONFIRM_GAP;
    let first = holders.first().map_or("", |h| h.name.as_str());
    let title_text = words.held(action, first, holders.len());
    let title_room = kit::RIGHT - (kit::LEFT + TILE + ICON_GAP);
    let title = kit::wrap(&title_text, title_room, |t| measure(t, TITLE, true));
    let title_height = (title.len() as f32 * TITLE * LINE).max(TILE);
    let sentence_top = top + title_height + PART_GAP;
    let sentence = kit::wrap(words.held_sentence(), kit::RIGHT - kit::LEFT, |t| {
        measure(t, HELD_SENTENCE, false)
    });
    let mut y = sentence_top + (sentence.len() as f32 * HELD_SENTENCE * LINE).round() + PART_GAP;
    let mut blocks = Vec::new();
    let words_left = kit::LEFT + BLOCK_PAD + ICON + ICON_GAP;
    let words_room = kit::RIGHT - BLOCK_PAD - words_left;
    for holder in holders {
        let name = kit::elide(&holder.name, words_room, |t| measure(t, NAME, true));
        let reason = if holder.reason.trim().is_empty() {
            Vec::new()
        } else {
            kit::wrap(&holder.reason, words_room, |t| measure(t, REASON, false))
        };
        let name_middle = y + BLOCK_PAD + NAME * LINE / 2.0;
        let reason_top = y + BLOCK_PAD + (NAME * LINE).round();
        let buttons_top = reason_top + (reason.len() as f32 * REASON * LINE).round() + BUTTONS_GAP;
        let back_word = words.back_to(&holder.name);
        let back_word = kit::elide(&back_word, words_room - 2.0 * SMALL_PAD, |t| {
            measure(t, REASON, true)
        });
        let back_width = (measure(&back_word, REASON, true) + 2.0 * SMALL_PAD).min(words_room);
        let back = Rectangle::new(
            Point::new(words_left, buttons_top),
            Size::new(back_width, SMALL),
        );
        let mut bottom = back.y + SMALL;
        let force = matches!(holder.kind, HolderKind::Window { .. }).then(|| {
            let width =
                (measure(words.close_anyway(), REASON, true) + 2.0 * SMALL_PAD).min(words_room);
            if back_width + SMALL_APART + width <= words_room {
                Rectangle::new(
                    Point::new(back.x + back_width + SMALL_APART, back.y),
                    Size::new(width, SMALL),
                )
            } else {
                bottom += SMALL_APART + SMALL;
                Rectangle::new(
                    Point::new(words_left, back.y + SMALL + SMALL_APART),
                    Size::new(width, SMALL),
                )
            }
        });
        let height = (bottom + BLOCK_PAD - y).max(ICON + 2.0 * BLOCK_PAD);
        blocks.push(Block {
            rect: full(y, height),
            icon: Rectangle::new(
                Point::new(kit::LEFT + BLOCK_PAD, y + BLOCK_PAD),
                Size::new(ICON, ICON),
            ),
            name,
            name_middle,
            reason,
            reason_top,
            back,
            back_word,
            force,
        });
        y += height + BLOCK_GAP;
    }
    let anyway = full(y - BLOCK_GAP + PART_GAP, BUTTON);
    let cancel = full(anyway.y + BUTTON + BUTTON_GAP, BUTTON);
    Layout {
        end: cancel.y + BUTTON,
        view: View::Held {
            action,
            top,
            title,
            sentence,
            sentence_top,
            blocks,
            anyway,
            cancel,
        },
    }
}

impl Layout {
    /// What can be clicked, and where.
    pub fn targets(&self) -> Vec<(Target, Rectangle)> {
        match &self.view {
            View::Rows { rows, .. } => rows
                .iter()
                .filter(|p| p.enabled)
                .map(|p| (Target::Row(p.row), p.rect))
                .collect(),
            View::Confirm { now, cancel, .. } => now
                .map(|r| (Target::Now, r))
                .into_iter()
                .chain(cancel.map(|r| (Target::Cancel, r)))
                .collect(),
            View::Held {
                blocks,
                anyway,
                cancel,
                ..
            } => {
                let mut targets = Vec::new();
                for (i, block) in blocks.iter().enumerate() {
                    targets.push((Target::Back(i), block.back));
                    if let Some(force) = block.force {
                        targets.push((Target::Force(i), force));
                    }
                }
                targets.push((Target::Anyway, *anyway));
                targets.push((Target::Cancel, *cancel));
                targets
            }
        }
    }

    /// The order the keyboard goes through.
    pub fn order(&self) -> Vec<Target> {
        self.targets().into_iter().map(|(t, _)| t).collect()
    }

    /// Where the keyboard lands as it shows: the confirmation's "…
    /// maintenant", since it was just chosen and `Enter` is the quick way
    /// on; the list of what holds the ending on "Annuler", since ending
    /// anyway loses what those applications hold; nothing on the rows.
    pub fn first_focus(&self) -> Option<Target> {
        let first = match &self.view {
            View::Rows { .. } => return None,
            View::Confirm { .. } => Target::Now,
            _ => Target::Cancel,
        };
        self.order().into_iter().find(|t| *t == first)
    }

    pub fn height(&self) -> u32 {
        kit::height(self.end)
    }
}

#[cfg(test)]
mod tests {
    use hocus_link::Can;

    use super::*;

    fn can(suspend: Can, power: Can) -> LeaveCan {
        LeaveCan {
            suspend,
            restart: power,
            shut_down: power,
        }
    }

    #[test]
    fn the_rows_settings_then_lock_and_suspend_then_the_three_ends() {
        let layout = rows(can(Can::Yes, Can::Yes));
        let View::Rows { rows, dividers } = &layout.view else {
            panic!("not the rows");
        };
        let order: Vec<Row> = rows.iter().map(|p| p.row).collect();
        assert_eq!(
            order,
            [
                Row::Settings,
                Row::Lock,
                Row::Suspend,
                Row::Leave(LeaveAction::LogOut),
                Row::Leave(LeaveAction::Restart),
                Row::Leave(LeaveAction::ShutDown),
            ]
        );
        // 44 px each, 6 px under the 40 px header; a 13 px divider
        // between groups.
        assert_eq!(rows[0].rect.y, header_end() + 6.0);
        assert_eq!(rows[1].rect.y, rows[0].rect.y + 44.0 + 13.0);
        assert_eq!(rows[2].rect.y, rows[1].rect.y + 44.0);
        assert_eq!(rows[3].rect.y, rows[2].rect.y + 44.0 + 13.0);
        assert_eq!(dividers.len(), 2);
        assert_eq!(dividers[1], rows[2].rect.y + 44.0 + 6.0);
        assert!(rows.iter().all(|p| p.rect.height == 44.0 && p.enabled));
        // The spec's 305 px for its five rows, Réglages's row and its
        // divider added, and the rim.
        assert_eq!(layout.height(), 305 + 44 + 13 + 2);
    }

    #[test]
    fn suspend_hides_and_power_greys_as_logind_says() {
        let layout = rows(can(Can::No, Can::No));
        let View::Rows { rows, .. } = &layout.view else {
            panic!("not the rows");
        };
        assert!(!rows.iter().any(|p| p.row == Row::Suspend));
        let enabled: Vec<bool> = rows.iter().map(|p| p.enabled).collect();
        assert_eq!(enabled, [true, true, true, false, false]);
        assert_eq!(layout.order().len(), 3);
        // With a password, they can be chosen.
        let challenge = rows_order(can(Can::Yes, Can::Challenge));
        assert_eq!(challenge.len(), 6);
    }

    fn rows_order(can: LeaveCan) -> Vec<Target> {
        rows(can).order()
    }

    #[test]
    fn the_confirmation_has_the_keyboard_on_now() {
        let layout = confirm(
            LeaveAction::ShutDown,
            &LeaveStep::Counting { left_ms: 10_000 },
        );
        assert_eq!(layout.order(), [Target::Now, Target::Cancel]);
        assert_eq!(layout.first_focus(), Some(Target::Now));
        let View::Confirm {
            bar: Some(bar),
            now: Some(now),
            cancel: Some(cancel),
            ..
        } = &layout.view
        else {
            panic!("not the confirmation");
        };
        assert_eq!(bar.height, 4.0);
        assert_eq!(now.y, bar.y + 4.0 + 10.0);
        assert_eq!(cancel.y, now.y + 36.0 + 6.0);
        assert_eq!(now.width, kit::RIGHT - kit::LEFT);
        let acting = confirm(LeaveAction::ShutDown, &LeaveStep::Acting);
        assert!(acting.order().is_empty());
        assert!(acting.height() < layout.height());
    }

    #[test]
    fn the_list_has_a_block_each_and_a_window_can_be_closed() {
        let holders = [
            Named {
                kind: HolderKind::Inhibitor { pid: 7 },
                app: "Firefox".into(),
                name: "Firefox".into(),
                reason: "Un téléchargement est en cours".into(),
            },
            Named {
                kind: HolderKind::Window { id: 3 },
                app: "gedit".into(),
                name: "Gedit".into(),
                reason: "« Sans titre » ne s’est pas fermée.".into(),
            },
        ];
        let mut measure = |t: &str, size: f32, _: bool| t.chars().count() as f32 * size * 0.5;
        let layout = held(
            LeaveAction::ShutDown,
            &holders,
            Words::french(),
            &mut measure,
        );
        assert_eq!(
            layout.order(),
            [
                Target::Back(0),
                Target::Back(1),
                Target::Force(1),
                Target::Anyway,
                Target::Cancel
            ]
        );
        assert_eq!(layout.first_focus(), Some(Target::Cancel));
        let View::Held { blocks, title, .. } = &layout.view else {
            panic!("not the list");
        };
        assert_eq!(title.join(" "), "2 applications retiennent l’arrêt");
        assert!(blocks[1].rect.y >= blocks[0].rect.y + blocks[0].rect.height);
        assert_eq!(blocks[0].back_word, "Revenir à Firefox");
    }
}
