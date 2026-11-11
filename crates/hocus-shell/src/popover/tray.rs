//! A tray icon's menu (5.14): its `com.canonical.dbusmenu` menu in a
//! popover beside its icon, its bottom on the icon's, as the bar's others
//! are. 220 wide, 6 px inside; its application's name over the rows (11 px
//! bold capitals spaced 0.08 em, `#7C8698`), then 30 px rows (12.5 px,
//! 8 px in, radius 8, lit `0.07` under the pointer), the mockup's; 9 px
//! separators; entries that are off greyed and inert; a check or a radio's
//! dot in a column of their own, icons in rows, a submenu's `›`.
//!
//! A submenu opens in the popover's place, "‹ its name" over its rows
//! going back. A click on a row sends its `Event("clicked")`, then the
//! popover closes. The keyboard: the arrows and `Tab` go through the rows
//! that act, `Enter` or `Space` acts, `Esc` goes back, then closes.
//!
//! Built with the shell's widget kit ([`crate::ui`]).

use std::sync::{Mutex, PoisonError};

use hocus_a11y::Role;
use iced::{
    Color, Element, Length, Padding, Point,
    alignment::Vertical,
    keyboard,
    widget::{
        Space, button,
        canvas::{self, Frame, Path, Stroke},
        column, container, row, space,
    },
    window,
};

use super::{Event as Seen, Item, Pressed, event, kit};
use crate::theme::Ink;
use crate::{
    levels::draw::{ACCENT, DIM, LINE, MANROPE, TEXT, rgb},
    tray::menu::{Entry, Kind, Toggle},
    ui::{
        self, Around, control,
        parts::{self, drawn, gap},
        style::{self, said},
    },
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = 220;
const BORDER: f32 = 1.0;
const PAD: f32 = 6.0;
/// The title: 11 px capitals, 6 px by 8 around them.
const TITLE: f32 = 11.0;
const TITLE_HEIGHT: f32 = TITLE * LINE + 12.0;
const TITLE_INK: Ink = Ink::Dark(rgb(0x7C8698));
const ROW: f32 = 30.0;
const ROW_IN: f32 = 8.0;
const ROW_RADIUS: f32 = 8.0;
const ROW_TEXT: f32 = 12.5;
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.07));
const OFF: Ink = Ink::Dark(rgb(0x6A7384));
const SEPARATOR: f32 = 9.0;
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
/// A row's icon, and the column of the checks and dots.
pub const ICON: f32 = 16.0;
const ICON_GAP: f32 = 8.0;
const TOGGLE: f32 = 18.0;
/// A submenu's chevron: 14.5 px, and its room.
const CHEVRON: f32 = ROW_TEXT + 2.0;
const CHEVRON_ROOM: f32 = 12.0;
/// The rows' width.
const INNER: f32 = WIDTH as f32 - 2.0 * (BORDER + PAD);

/// The submenus open, from the menu's root.
static PATH: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn path() -> Vec<i32> {
    PATH.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The menu opens: at its root, nothing holding the ring.
pub fn opening() {
    PATH.lock().unwrap_or_else(PoisonError::into_inner).clear();
    ui(|ui| ui.focus = None);
}

/// The entry whose rows show: the root, or the submenu open.
fn level(root: &Entry) -> &Entry {
    let mut at = root;
    for id in path() {
        match at.children.iter().find(|c| c.id == id) {
            Some(next) => at = next,
            None => break,
        }
    }
    at
}

/// What can be acted on: a submenu's way back, a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Back,
    Row(i32),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("tray:{self:?}")
    }
}

/// The rows' tops and heights under the title, in the popover.
fn rows(level: &Entry) -> Vec<(&Entry, f32, f32)> {
    let mut y = BORDER + PAD + TITLE_HEIGHT;
    level
        .children
        .iter()
        .map(|entry| {
            let h = match entry.kind {
                Kind::Separator => SEPARATOR,
                Kind::Standard => ROW,
            };
            let top = y;
            y += h;
            (entry, top, h)
        })
        .collect()
}

/// Item `key`'s menu's height, as it shows now.
pub fn height(key: u32) -> u32 {
    let Some(root) = crate::tray::menu(key) else {
        return (2.0 * (BORDER + PAD) + TITLE_HEIGHT).round() as u32;
    };
    let level = level(&root);
    let end = rows(level)
        .last()
        .map_or(BORDER + PAD + TITLE_HEIGHT, |(_, top, h)| top + h);
    (end + PAD + BORDER).round() as u32
}

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub key: u32,
    pub glass: bool,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// What it last said it shows.
    said: Option<Vec<String>>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// Back, into a submenu, or a row's event.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// A press anywhere on the popover.
    Pressed,
    /// The menu changed under the view.
    Stale,
}

/// What can be acted on, in order.
fn targets(root: &Entry, level: &Entry) -> Vec<Target> {
    let back = (level.id != root.id).then_some(Target::Back);
    back.into_iter()
        .chain(
            level
                .children
                .iter()
                .filter(|e| e.acts())
                .map(|e| Target::Row(e.id)),
        )
        .collect()
}

/// The menu on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let key = panel.key;
    let order = crate::tray::menu(key)
        .map(|root| targets(&root, level(&root)))
        .unwrap_or_default();
    Around::new(panel.id, build(key), move |m| handle(m, key), Msg::Stale)
        .rebuild(move || build(key))
        .keys(move |pressed| keys(pressed, &order))
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// `Esc` in a submenu goes back; the arrows with nothing holding the
/// ring, the first (down) or the last (up); the kit takes them round
/// after.
fn keys(pressed: &Pressed, order: &[Target]) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    match &pressed.key {
        Key::Named(Named::Escape) if !path().is_empty() => Some(Msg::Act(Target::Back)),
        Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) if ui(|ui| ui.focus.is_none()) => {
            let target = if *arrow == Named::ArrowDown {
                order.first()
            } else {
                order.last()
            };
            target.map(|t| Msg::Focus(*t))
        }
        _ => None,
    }
}

/// Its content, the menu as it is now.
fn build<'a>(key: u32) -> El<'a> {
    let Some(root) = crate::tray::menu(key) else {
        return Space::new().into();
    };
    let level = level(&root);
    ui(|ui| {
        say(key, level, ui);
        content(key, &root, level, ui.focus)
    })
}

fn content<'a>(key: u32, root: &Entry, level: &Entry, focus: Option<Target>) -> El<'a> {
    let mut all = column![title(key, root, level, focus)];
    let toggles = level.children.iter().any(|e| e.toggle != Toggle::None);
    let icons = level.children.iter().any(|e| e.icon.is_some());
    for entry in &level.children {
        all = all.push(match entry.kind {
            Kind::Separator => container(
                container(Space::new().width(Length::Fill).height(1.0))
                    .style(|_| style::boxed(RULE.get(), 0.0)),
            )
            .padding(Padding {
                top: (SEPARATOR / 2.0).floor(),
                right: ROW_IN,
                bottom: SEPARATOR - (SEPARATOR / 2.0).floor() - 1.0,
                left: ROW_IN,
            })
            .into(),
            Kind::Standard => entry_row(entry, (toggles, icons), focus),
        });
    }
    container(all).padding(BORDER + PAD).into()
}

/// The name over the rows, spaced: the application's; in a submenu, the
/// way back.
fn title<'a>(key: u32, root: &Entry, level: &Entry, focus: Option<Target>) -> El<'a> {
    let at_root = level.id == root.id;
    let word = if at_root {
        crate::tray::shown()
            .into_iter()
            .find(|s| s.key == key)
            .map(|s| s.name)
            .unwrap_or_default()
            .to_uppercase()
    } else {
        format!("‹ {}", level.label.to_uppercase())
    };
    let letters = parts::spaced(&word, TITLE, TITLE_INK.get(), INNER - 2.0 * ROW_IN);
    let line = container(letters)
        .padding([0.0, ROW_IN])
        .width(Length::Fill)
        .height(TITLE_HEIGHT)
        .align_y(Vertical::Center);
    if at_root {
        return ui::line(line, word).into();
    }
    let lit = focus == Some(Target::Back);
    let back = button(line)
        .padding(0)
        .style(move |_, status| lit_row(lit || style::lit(status)))
        .on_press(Msg::Act(Target::Back));
    control(back, Target::Back.id(), Role::Button, level.label.clone())
        .focus(lit, Msg::Focus(Target::Back), ROW_RADIUS)
        .activate(Msg::Act(Target::Back))
        .rows(0)
        .into()
}

/// A row's look: lit (under the pointer, or holding the ring), or not.
fn lit_row(lit: bool) -> button::Style {
    if lit {
        style::hovered(ROW_HOVER.get(), ROW_RADIUS)
    } else {
        style::clear()
    }
}

/// An entry's row: its check or dot, its icon, its label, a submenu's
/// chevron; greyed and inert when off.
fn entry_row<'a>(entry: &Entry, (toggles, icons): (bool, bool), focus: Option<Target>) -> El<'a> {
    let target = Target::Row(entry.id);
    let ink = if entry.enabled { TEXT } else { OFF }.get();
    let mut line = row![].align_y(Vertical::Center);
    let mut x = ROW_IN;
    if toggles {
        let toggled = entry.toggle;
        line = line.push(drawn(TOGGLE, ROW, move |f, s| {
            toggle(
                f,
                toggled,
                Point::new(TOGGLE / 2.0 - 2.0, s.height / 2.0),
                ink,
            );
        }));
        x += TOGGLE;
    }
    if icons {
        let shown: El<'a> = match &entry.icon {
            Some(icon) => {
                let icon = icon.clone();
                let opacity = if entry.enabled { 1.0_f32 } else { 0.5 };
                drawn(ICON, ICON, move |f, s| {
                    f.draw_image(
                        iced::Rectangle::new(Point::ORIGIN, s),
                        canvas::Image::new(icon.clone()).opacity(opacity),
                    );
                })
            }
            None => Space::new().width(ICON).into(),
        };
        line = line.push(shown).push(gap(ICON_GAP));
        x += ICON + ICON_GAP;
    }
    let chevron = entry.submenu();
    let right = INNER - ROW_IN - if chevron { CHEVRON_ROOM } else { 0.0 };
    let label = kit::elide(&entry.label, right - x, |t| {
        kit::measure(t, ROW_TEXT, MANROPE)
    });
    line = line
        .push(said(label, ROW_TEXT, MANROPE, ink))
        .push(space::horizontal());
    if chevron {
        line = line.push(said("›", CHEVRON, MANROPE, DIM.get()));
    }
    let acts = entry.acts();
    let lit = acts && focus == Some(target);
    let pressed = button(container(line).center_y(Length::Fill))
        .padding(Padding {
            left: ROW_IN,
            right: ROW_IN,
            ..Padding::ZERO
        })
        .width(Length::Fill)
        .height(ROW)
        .style(move |_, status| lit_row(acts && (lit || style::lit(status))))
        .on_press_maybe(acts.then_some(Msg::Act(target)));
    let role = match entry.toggle {
        Toggle::Check(_) => Role::CheckBox,
        Toggle::Radio(_) => Role::RadioButton,
        Toggle::None => Role::Button,
    };
    let (toggled, submenu, enabled) = (entry.toggle, entry.submenu(), entry.enabled);
    let mut declared = control(pressed, target.id(), role, entry.label.clone()).node(|mut n| {
        n = n.disabled(!enabled);
        if let Toggle::Check(on) | Toggle::Radio(on) = toggled {
            n = n.checked(on);
        }
        if submenu {
            n = n.expanded(false);
        }
        n
    });
    if acts {
        declared = declared
            .focus(lit, Msg::Focus(target), ROW_RADIUS)
            .activate(Msg::Act(target))
            .rows(0);
    }
    declared.into()
}

/// What it shows, said once as it changes: each row's label and middle.
fn say(key: u32, level: &Entry, ui: &mut Ui) {
    let rows: Vec<String> = rows(level)
        .into_iter()
        .map(|(e, top, h)| match e.kind {
            Kind::Separator => "—".to_owned(),
            Kind::Standard => format!("{}@{:.0}", e.label, top + h / 2.0),
        })
        .collect();
    if ui.said.as_ref() == Some(&rows) {
        return;
    }
    tracing::debug!(key, ?rows, "the tray's menu shows it");
    ui.said = Some(rows);
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg, key: u32) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Act(Target::Back) => {
            PATH.lock().unwrap_or_else(PoisonError::into_inner).pop();
            ui(|ui| ui.focus = None);
            super::refit(Item::Tray(key));
        }
        Msg::Act(Target::Row(id)) => {
            let Some(root) = crate::tray::menu(key) else {
                return;
            };
            let Some(entry) = level(&root).children.iter().find(|e| e.id == id) else {
                return;
            };
            if entry.submenu() {
                tracing::debug!(id, "the tray's menu opens a submenu");
                PATH.lock().unwrap_or_else(PoisonError::into_inner).push(id);
                ui(|ui| ui.focus = None);
                super::refit(Item::Tray(key));
            } else {
                tracing::debug!(id, label = entry.label, "a row of the tray's menu");
                crate::tray::ask(crate::tray::Ask::Event { key, id });
                event(Seen::Chosen);
            }
        }
    }
}

/// A check, or a radio's dot (a ring when off), centred on `at`.
fn toggle(f: &mut Frame, toggle: Toggle, at: Point, ink: Color) {
    match toggle {
        Toggle::None | Toggle::Check(false) => {}
        Toggle::Check(true) => {
            let check = Path::new(|p| {
                p.move_to(Point::new(at.x - 4.5, at.y));
                p.line_to(Point::new(at.x - 1.5, at.y + 3.0));
                p.line_to(Point::new(at.x + 4.5, at.y - 3.5));
            });
            f.stroke(
                &check,
                Stroke::default()
                    .with_color(if ink == TEXT.get() { ACCENT.get() } else { ink })
                    .with_width(1.8)
                    .with_line_cap(canvas::LineCap::Round)
                    .with_line_join(canvas::LineJoin::Round),
            );
        }
        Toggle::Radio(true) => f.fill(
            &Path::circle(at, 3.5),
            if ink == TEXT.get() { ACCENT.get() } else { ink },
        ),
        Toggle::Radio(false) => f.stroke(
            &Path::circle(at, 4.0),
            Stroke::default().with_color(DIM.get()).with_width(1.2),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: i32, kind: Kind, children: Vec<Entry>) -> Entry {
        Entry {
            id,
            kind,
            label: format!("row {id}"),
            enabled: id != 4,
            toggle: Toggle::None,
            icon: None,
            children,
        }
    }

    #[test]
    fn rows_stack_under_the_title_separators_thinner() {
        let menu = entry(
            0,
            Kind::Standard,
            vec![
                entry(1, Kind::Standard, vec![]),
                entry(2, Kind::Separator, vec![]),
                entry(3, Kind::Standard, vec![entry(5, Kind::Standard, vec![])]),
                entry(4, Kind::Standard, vec![]),
            ],
        );
        let rows = rows(&menu);
        let ys: Vec<f32> = rows.iter().map(|(_, top, _)| *top).collect();
        let top = BORDER + PAD + TITLE_HEIGHT;
        assert_eq!(ys, [top, top + 30.0, top + 39.0, top + 69.0]);
        assert_eq!(INNER, 220.0 - 14.0);
        // Back, then the rows that act: not the separator, nor the one off.
        assert_eq!(targets(&menu, &menu), [Target::Row(1), Target::Row(3)]);
        let sub = &menu.children[2];
        assert_eq!(targets(&menu, sub), [Target::Back, Target::Row(5)]);
    }
}
