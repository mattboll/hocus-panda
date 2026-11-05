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

use std::sync::{Mutex, PoisonError};

use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{self, Input, Pointer, text, text_in},
};
use crate::theme::Ink;
use crate::{
    levels::draw::{ACCENT, DIM, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring},
    tray::menu::{Entry, Kind, Toggle},
};

pub const WIDTH: u32 = 220;
const BORDER: f32 = 1.0;
const PAD: f32 = 6.0;
/// The title: 11 px capitals, 6 px by 8 around them.
const TITLE: f32 = 11.0;
const TITLE_SPACING: f32 = 0.08 * TITLE;
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

/// The submenus open, from the menu's root.
static PATH: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn path() -> Vec<i32> {
    PATH.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The menu opens: at its root.
pub fn opening() {
    PATH.lock().unwrap_or_else(PoisonError::into_inner).clear();
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

/// What can be pointed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Back,
    Row(i32),
}

/// The rows of `level` and where they are, from the title's foot.
fn rows(level: &Entry) -> Vec<(&Entry, Rectangle)> {
    let mut y = BORDER + PAD + TITLE_HEIGHT;
    let x = BORDER + PAD;
    let w = WIDTH as f32 - 2.0 * x;
    level
        .children
        .iter()
        .map(|entry| {
            let h = match entry.kind {
                Kind::Separator => SEPARATOR,
                Kind::Standard => ROW,
            };
            let rect = Rectangle::new(Point::new(x, y), Size::new(w, h));
            y += h;
            (entry, rect)
        })
        .collect()
}

/// The title's row, which goes back from a submenu.
fn title_rect() -> Rectangle {
    Rectangle::new(
        Point::new(BORDER + PAD, BORDER + PAD),
        Size::new(WIDTH as f32 - 2.0 * (BORDER + PAD), TITLE_HEIGHT),
    )
}

/// Item `key`'s menu's height, as it shows now.
pub fn height(key: u32) -> u32 {
    let Some(root) = crate::tray::menu(key) else {
        return (2.0 * (BORDER + PAD) + TITLE_HEIGHT).round() as u32;
    };
    let level = level(&root);
    let end = rows(level)
        .last()
        .map_or(BORDER + PAD + TITLE_HEIGHT, |(_, r)| r.y + r.height);
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
pub struct State {
    pointer: Pointer<Target>,
    focus: Option<Target>,
    /// What it last said it shows.
    said: Option<Vec<String>>,
}

impl Panel {
    /// The name over the rows: the application's, or the submenu's.
    fn title(&self, root: &Entry, level: &Entry) -> String {
        if level.id == root.id {
            crate::tray::shown()
                .into_iter()
                .find(|s| s.key == self.key)
                .map(|s| s.name)
                .unwrap_or_default()
                .to_uppercase()
        } else {
            format!("‹ {}", level.label.to_uppercase())
        }
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

    fn target_at(root: &Entry, level: &Entry, at: Point) -> Option<Target> {
        if level.id != root.id && title_rect().contains(at) {
            return Some(Target::Back);
        }
        rows(level)
            .into_iter()
            .find(|(e, r)| e.acts() && r.contains(at))
            .map(|(e, _)| Target::Row(e.id))
    }

    /// `target` acted on: back, into a submenu, or the row's event, which
    /// closes the popover.
    fn act<Message>(&self, state: &mut State, target: Target) -> Option<canvas::Action<Message>> {
        let root = crate::tray::menu(self.key)?;
        match target {
            Target::Back => {
                PATH.lock().unwrap_or_else(PoisonError::into_inner).pop();
                state.focus = None;
                super::refit(Item::Tray(self.key));
            }
            Target::Row(id) => {
                let entry = level(&root).children.iter().find(|e| e.id == id)?;
                if entry.submenu() {
                    tracing::debug!(id, "the tray's menu opens a submenu");
                    PATH.lock().unwrap_or_else(PoisonError::into_inner).push(id);
                    state.focus = None;
                    super::refit(Item::Tray(self.key));
                } else {
                    tracing::debug!(id, label = entry.label, "a row of the tray's menu");
                    crate::tray::ask(crate::tray::Ask::Event { key: self.key, id });
                    event(Seen::Chosen);
                }
            }
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let root = crate::tray::menu(self.key)?;
        let target = at.and_then(|at| Self::target_at(&root, level(&root), at));
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                match state.pointer.release(target) {
                    Some(target) => self.act(state, target),
                    None => redraw,
                }
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let root = crate::tray::menu(self.key);
        let targets = root
            .as_ref()
            .map(|r| Self::targets(r, level(r)))
            .unwrap_or_default();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                if path().is_empty() {
                    event(Seen::Escape);
                    return Some(canvas::Action::capture());
                }
                return self.act(state, Target::Back);
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&targets, state.focus, pressed.modifiers.shift());
            }
            Key::Named(Named::ArrowDown) => state.focus = kit::step(&targets, state.focus, false),
            Key::Named(Named::ArrowUp) => state.focus = kit::step(&targets, state.focus, true),
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(focus) = state.focus {
                    return self.act(state, focus);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// What it shows, said once as it changes.
    fn say(&self, state: &mut State) {
        let Some(root) = crate::tray::menu(self.key) else {
            return;
        };
        let level = level(&root);
        let rows: Vec<String> = rows(level)
            .into_iter()
            .map(|(e, r)| match e.kind {
                Kind::Separator => "—".to_owned(),
                Kind::Standard => format!("{}@{:.0}", e.label, r.center_y()),
            })
            .collect();
        if state.said.as_ref() == Some(&rows) {
            return;
        }
        tracing::debug!(key = self.key, ?rows, "the tray's menu shows it");
        state.said = Some(rows);
    }
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(_) => {
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                self.say(state);
                None
            }
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        let Some(root) = crate::tray::menu(self.key) else {
            return vec![frame.into_geometry()];
        };
        let level = level(&root);
        let f = &mut frame;
        let lit =
            |target: Target| state.pointer.hover == Some(target) || state.focus == Some(target);
        // The title, spaced, or the way back.
        let title = title_rect();
        if lit(Target::Back) {
            f.fill(&kit::rounded(title, ROW_RADIUS), ROW_HOVER.get());
        }
        if state.focus == Some(Target::Back) {
            ring(f, title, ROW_RADIUS);
        }
        let mut x = title.x + ROW_IN;
        let room = title.x + title.width - ROW_IN;
        for ch in self.title(&root, level).chars() {
            let ch = ch.to_string();
            let w = kit::measure(&ch, TITLE, MANROPE_BOLD);
            if x + w > room {
                break;
            }
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    ch,
                    Point::new(x, title.center_y()),
                    TITLE,
                    TITLE_INK.get(),
                    Alignment::Left,
                ),
            ));
            x += w + TITLE_SPACING;
        }
        let toggles = level.children.iter().any(|e| e.toggle != Toggle::None);
        let icons = level.children.iter().any(|e| e.icon.is_some());
        for (entry, rect) in rows(level) {
            if entry.kind == Kind::Separator {
                f.fill_rectangle(
                    Point::new(rect.x + ROW_IN, rect.center_y().floor()),
                    Size::new(rect.width - 2.0 * ROW_IN, 1.0),
                    RULE.get(),
                );
                continue;
            }
            let target = Target::Row(entry.id);
            if entry.acts() && lit(target) {
                f.fill(&kit::rounded(rect, ROW_RADIUS), ROW_HOVER.get());
            }
            if state.focus == Some(target) {
                ring(f, rect, ROW_RADIUS);
            }
            let ink = if entry.enabled { TEXT } else { OFF };
            let middle = rect.center_y();
            let mut x = rect.x + ROW_IN;
            if toggles {
                toggle(
                    f,
                    entry.toggle,
                    Point::new(x + TOGGLE / 2.0 - 2.0, middle),
                    ink.get(),
                );
                x += TOGGLE;
            }
            if icons {
                if let Some(icon) = &entry.icon {
                    f.draw_image(
                        Rectangle::new(Point::new(x, middle - ICON / 2.0), Size::new(ICON, ICON)),
                        canvas::Image::new(icon.clone()).opacity(if entry.enabled {
                            1.0_f32
                        } else {
                            0.5
                        }),
                    );
                }
                x += ICON + ICON_GAP;
            }
            let chevron = entry.submenu().then_some("›");
            let right = rect.x + rect.width - ROW_IN - if chevron.is_some() { 12.0 } else { 0.0 };
            let label = kit::elide(&entry.label, right - x, |t| {
                kit::measure(t, ROW_TEXT, MANROPE)
            });
            f.fill_text(text(
                label,
                Point::new(x, middle),
                ROW_TEXT,
                ink.get(),
                Alignment::Left,
            ));
            if let Some(chevron) = chevron {
                f.fill_text(text(
                    chevron.to_owned(),
                    Point::new(rect.x + rect.width - ROW_IN, middle),
                    ROW_TEXT + 2.0,
                    DIM.get(),
                    Alignment::Right,
                ));
            }
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
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
        let ys: Vec<f32> = rows.iter().map(|(_, r)| r.y).collect();
        let top = BORDER + PAD + TITLE_HEIGHT;
        assert_eq!(ys, [top, top + 30.0, top + 39.0, top + 69.0]);
        assert_eq!(rows[0].1.width, 220.0 - 14.0);
        // Back, then the rows that act: not the separator, nor the one off.
        assert_eq!(
            Panel::targets(&menu, &menu),
            [Target::Row(1), Target::Row(3)]
        );
        let sub = &menu.children[2];
        assert_eq!(Panel::targets(&menu, sub), [Target::Back, Target::Row(5)]);
    }
}
