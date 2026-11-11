//! The bar's footer (bar.md §4 group 11, 5.5, variant A): in the full
//! form, a 34 px row along the bar's bottom with four 40 × 34 buttons
//! spread evenly across its inside: the clipboard, the notifications' bell
//! (with its badge, [`crate::notifications::bell`]), the menu and the
//! user's avatar; in the mini form, the bell over the avatar, each 44 × 34,
//! at the bottom of the column. Buttons have a radius of 9, lit `0.08`
//! under the pointer and `0.10` while their popover shows, their glyphs
//! 20 px strokes; the avatar is the account's photo, else the panda on
//! the accent's gradient (7.4, as the lock's), 24 px round.
//!
//! The clipboard, the bell and the user open their popovers as the bar's
//! items do, a hover of 150 ms or a click, in both forms
//! ([`crate::popover::clipboard`], [`crate::popover::history`],
//! [`crate::popover::session`]); the menu opens the launcher (5.1) on a
//! click only. `Tab` and the arrows reach them from the bar's other items,
//! `Enter` or `Space` acts. Each has its name, said as it is drawn.
//!
//! It is a surface of its own over the bar's bottom (it took the bell's of
//! 5.4), opened after the bar's others so that it is above them, taking
//! the pointer on its buttons only: a change of the badge draws it alone.
//! While the bar switches its form, it is the full form's width and both
//! forms' height, and the two cross-fade as the other groups do.

use std::{
    collections::HashMap,
    sync::{Mutex, OnceLock, PoisonError},
};

use hocus_link::{EDGE, FOOTER_NAMESPACE, Form};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::canvas::{self, Frame, Geometry},
    window,
};
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};

use crate::{
    Message,
    clock::time::Lang,
    levels::draw::pointer,
    notifications::bell::{self, INK},
    popover::{self, Item},
};

/// The buttons, in the full form and in the mini.
pub const FULL_BUTTON: Size = Size::new(40.0, 34.0);
pub const MINI_BUTTON: Size = Size::new(44.0, 34.0);
/// Around the buttons on the surface: the keyboard's ring (5 px out) and
/// the badge's.
const ROOM: f32 = 6.0;
/// The bar's inside from the surface's sides: its 12 px padding less its
/// rim, the surface being the bar inside its rim.
const SIDE: f32 = 11.0;
/// The full form's row's bottom over the bar's: the rim and 12 px.
const FULL_BOTTOM: f32 = 13.0;
/// The mini form's avatar's bottom over the bar's: the rim and 10 px; the
/// bell 2 px over it (as the Wi-Fi's and Bluetooth's buttons are).
const MINI_BOTTOM: f32 = 11.0;
const MINI_GAP: f32 = 2.0;
/// The surface's width: the bar inside its rim.
const FULL_WIDTH: u32 = 194;
const MINI_WIDTH: u32 = 54;
/// The avatar: 24 px round.
const AVATAR: f32 = 24.0;

/// How much room the mini form's footer takes over the bar's bottom, up to
/// the top of its bell: the levels keep 14 px over it.
pub const MINI_HEIGHT: f32 = MINI_BOTTOM + 2.0 * MINI_BUTTON.height + MINI_GAP;

/// The buttons of `form`: from the left in the full form, from the top in
/// the mini.
pub fn buttons(form: Form) -> &'static [Item] {
    match form {
        Form::Full => &[Item::Clipboard, Item::Notifications, Item::Menu, Item::User],
        Form::Mini => &[Item::Notifications, Item::User],
    }
}

fn button_size(form: Form) -> Size {
    match form {
        Form::Full => FULL_BUTTON,
        Form::Mini => MINI_BUTTON,
    }
}

/// The surface's size and its bottom over the bar's, for `form` (the
/// full form's width and both forms' height while the bar switches).
pub fn placement(form: Form, switching: bool) -> ((u32, u32), f32) {
    let full = (
        FULL_WIDTH,
        FULL_BUTTON.height + 2.0 * ROOM,
        FULL_BOTTOM - ROOM,
    );
    let mini = (
        MINI_WIDTH,
        2.0 * MINI_BUTTON.height + MINI_GAP + 2.0 * ROOM,
        MINI_BOTTOM - ROOM,
    );
    let ((width, height), bottom) = match (switching, form) {
        (true, _) => {
            let bottom = full.2.min(mini.2);
            let top = (full.2 + full.1).max(mini.2 + mini.1);
            ((FULL_WIDTH, top - bottom), bottom)
        }
        (false, Form::Full) => ((full.0, full.1), full.2),
        (false, Form::Mini) => ((mini.0, mini.1), mini.2),
    };
    ((width, height.ceil() as u32), bottom)
}

/// The layer-shell margins of the surface, its bottom `bottom` over the
/// bar's.
fn margin(bottom: f32) -> (i32, i32, i32, i32) {
    (0, EDGE + 1, EDGE + bottom.round() as i32, 0)
}

/// Button `i` of `form` on a surface of `size` whose bottom is `bottom`
/// over the bar's, for a bar `bar_width` wide (as it is while it
/// switches): the full form's from the bar's right, the mini form's on its
/// middle.
fn rect(form: Form, i: usize, size: Size, bottom: f32, bar_width: f32) -> Rectangle {
    let button = button_size(form);
    let right = size.width;
    let left = right + 2.0 - bar_width;
    let at = |above: f32| size.height - (above - bottom);
    let n = buttons(form).len();
    match form {
        Form::Full => {
            let inside = FULL_WIDTH as f32 - 2.0 * SIDE;
            let gap = (inside - n as f32 * button.width) / (n as f32 - 1.0);
            let x = right - SIDE - button.width - (n - 1 - i) as f32 * (button.width + gap);
            Rectangle::new(Point::new(x, at(FULL_BOTTOM) - button.height), button)
        }
        Form::Mini => {
            let under = (n - 1 - i) as f32 * (button.height + MINI_GAP);
            Rectangle::new(
                Point::new(
                    (left + right - button.width) / 2.0,
                    at(MINI_BOTTOM + under) - button.height,
                ),
                button,
            )
        }
    }
}

// --- Its surface's places ------------------------------------------------------------

/// A bar's footer's surface, as last placed.
#[derive(Debug, Clone, Copy)]
struct Place {
    id: window::Id,
    form: Form,
    switching: bool,
    asked: ((u32, u32), (i32, i32, i32, i32)),
}

static PLACES: Mutex<Option<HashMap<String, Place>>> = Mutex::new(None);

fn places<T>(with: impl FnOnce(&mut HashMap<String, Place>) -> T) -> T {
    let mut places = PLACES.lock().unwrap_or_else(PoisonError::into_inner);
    with(places.get_or_insert_with(HashMap::new))
}

/// The footer of bar `bar` in `form`, on `output`: its surface's id, and
/// what opens it, taking the pointer on its buttons only.
pub fn open(bar: &str, form: Form, output: OutputOption) -> (window::Id, Vec<Message>) {
    let id = window::Id::unique();
    let (size, bottom) = placement(form, false);
    let margin = margin(bottom);
    places(|places| {
        places.insert(
            bar.to_owned(),
            Place {
                id,
                form,
                switching: false,
                asked: (size, margin),
            },
        )
    });
    let settings = NewLayerShellSettings {
        size: Some(size),
        layer: Layer::Top,
        anchor: crate::side::anchor(Anchor::Right | Anchor::Bottom),
        exclusive_zone: Some(0),
        margin: Some(crate::side::margin(margin)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: output,
        events_transparent: false,
        namespace: Some(FOOTER_NAMESPACE.to_owned()),
    };
    crate::side::opened(id);
    (
        id,
        vec![
            Message::NewLayerShell { settings, id },
            region(id, form, false),
        ],
    )
}

/// Bar `bar` is in `form`, switching or not: its footer's new size,
/// margins and buttons, if they change.
pub fn form(bar: &str, form: Form, switching: bool) -> Vec<Message> {
    places(|places| {
        let Some(place) = places.get_mut(bar) else {
            return Vec::new();
        };
        if place.form == form && place.switching == switching {
            return Vec::new();
        }
        place.form = form;
        place.switching = switching;
        let (size, bottom) = placement(form, switching);
        let margin = margin(bottom);
        let mut messages = Vec::new();
        if place.asked.0 != size {
            messages.push(Message::SizeChange { id: place.id, size });
        }
        if place.asked.1 != margin {
            messages.push(Message::MarginChange {
                id: place.id,
                margin: crate::side::moved(place.id, margin),
            });
        }
        place.asked = (size, margin);
        messages.push(region(place.id, form, switching));
        messages
    })
}

/// Bar `bar` went.
pub fn forget(bar: &str) {
    places(|places| places.remove(bar));
}

/// Wider than any surface: a region as large as can be.
const EVERYWHERE: i32 = 1 << 20;

type WlRegion = wayland_client::protocol::wl_region::WlRegion;

/// Surface `id` takes the pointer on `form`'s buttons only; on none while
/// the bar switches.
fn region(id: window::Id, form: Form, switching: bool) -> Message {
    let rects: Vec<(i32, i32, i32, i32)> = if switching {
        Vec::new()
    } else {
        let ((w, h), bottom) = placement(form, false);
        let size = Size::new(w as f32, h as f32);
        (0..buttons(form).len())
            .map(|i| {
                let r = rect(form, i, size, bottom, form.width() as f32);
                let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
                let right = (r.x + r.width).ceil() as i32;
                let bottom = (r.y + r.height).ceil() as i32;
                (x, y, right - x, bottom - y)
            })
            .collect()
    };
    let callback = iced_layershell::actions::ActionCallback::new(move |wl: &WlRegion| {
        wl.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
        for &(x, y, w, h) in &rects {
            wl.add(x, y, w, h);
        }
    });
    Message::SetInputRegion { id, callback }
}

// --- Words ---------------------------------------------------------------------------

/// The buttons' names, for screen readers, in the bar's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// `item`'s name, `unseen` notifications not seen, the user `user`.
    pub fn name(self, item: Item, unseen: usize, user: &str) -> String {
        match (item, (self.lang == Lang::French)) {
            (Item::Clipboard, true) => "Presse-papiers".to_owned(),
            (Item::Clipboard, false) => "Clipboard".to_owned(),
            (Item::Notifications, _) if unseen == 0 => "Notifications".to_owned(),
            (Item::Notifications, true) => format!("Notifications, {unseen} non vues"),
            (Item::Notifications, false) => format!("Notifications, {unseen} unseen"),
            (Item::Menu, true) => "Menu : toutes les applications".to_owned(),
            (Item::Menu, false) => "Menu: all applications".to_owned(),
            (Item::User, true) => format!("{user}, menu de session"),
            (Item::User, false) => format!("{user}, session menu"),
            (_, _) => String::new(),
        }
    }
}

// --- Its glyphs ------------------------------------------------------------------------

/// The clipboard's glyph and the menu's grid, the mockup's 24 px paths.
fn glyph(item: Item) -> Option<svg::Handle> {
    static GLYPHS: OnceLock<[svg::Handle; 2]> = OnceLock::new();
    let make = |paths: &str| {
        svg::Handle::from_memory(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" width=\"24\" \
                 height=\"24\" fill=\"none\" stroke=\"{INK}\" stroke-width=\"1.8\" \
                 stroke-linecap=\"round\">{paths}</svg>"
            )
            .into_bytes(),
        )
    };
    let glyphs = GLYPHS.get_or_init(|| {
        [
            make(
                "<rect x=\"6\" y=\"5\" width=\"12\" height=\"15\" rx=\"2\"/><path d=\"M9 5 V4 a1 \
                 1 0 0 1 1 -1 H14 a1 1 0 0 1 1 1 V5 M9.5 10 H14.5 M9.5 13.5 H14.5\"/>",
            ),
            make(
                "<rect x=\"4.5\" y=\"4.5\" width=\"5.5\" height=\"5.5\" rx=\"1.5\"/><rect \
                 x=\"14\" y=\"4.5\" width=\"5.5\" height=\"5.5\" rx=\"1.5\"/><rect x=\"4.5\" \
                 y=\"14\" width=\"5.5\" height=\"5.5\" rx=\"1.5\"/><rect x=\"14\" y=\"14\" \
                 width=\"5.5\" height=\"5.5\" rx=\"1.5\"/>",
            ),
        ]
    });
    match item {
        Item::Clipboard => Some(glyphs[0].clone()),
        Item::Menu => Some(glyphs[1].clone()),
        _ => None,
    }
}

/// The avatar centred on `button`, 24 px round: the account's photo,
/// else the panda on the accent's gradient (7.4, as the lock's), `alpha`
/// opaque.
fn avatar(f: &mut Frame, button: Rectangle, alpha: f32) {
    let centre = button.center();
    let rect = Rectangle::new(
        Point::new(centre.x - AVATAR / 2.0, centre.y - AVATAR / 2.0),
        Size::new(AVATAR, AVATAR),
    );
    crate::lock::draw::avatar_round(f, rect, alpha);
}

// --- The surface -------------------------------------------------------------------------

/// The footer of bar `bar` on its surface.
#[derive(Debug, Clone)]
pub struct Footer {
    pub bar: String,
    /// The forms to draw and their opacity (two while the bar switches).
    pub forms: Vec<(Form, f32)>,
    /// The bar's width as drawn.
    pub bar_width: f32,
    pub words: Words,
    /// On the CPU, the bar's colour under it (its buffer has no alpha).
    pub backdrop: Option<Color>,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: Option<Item>,
    /// What it last said it shows.
    said: Option<(Form, Vec<String>)>,
}

impl Footer {
    /// Its surface's bottom over the bar's, as placed for its forms now.
    fn bottom(&self) -> f32 {
        match self.forms.as_slice() {
            [(form, _)] => placement(*form, false).1,
            _ => placement(Form::Full, true).1,
        }
    }

    /// The buttons and where they are, unless the bar is switching.
    fn items(&self, size: Size) -> Vec<(Item, Rectangle)> {
        let [(form, _)] = self.forms.as_slice() else {
            return Vec::new();
        };
        let bottom = self.bottom();
        buttons(*form)
            .iter()
            .enumerate()
            .map(|(i, item)| (*item, rect(*form, i, size, bottom, self.bar_width)))
            .collect()
    }

    /// A point of the surface (`size`) in the bar's frame: from the bar's
    /// right edge (negative inside it), and from its top.
    fn in_bar(&self, at: Point, size: Size) -> Point {
        let height = popover::with(|c| c.bar_height(&self.bar));
        Point::new(
            at.x - size.width - 1.0,
            height - self.bottom() - size.height + at.y,
        )
    }

    fn in_bar_rect(&self, rect: Rectangle, size: Size) -> Rectangle {
        Rectangle::new(self.in_bar(rect.position(), size), rect.size())
    }

    /// What it shows changed: said, by the buttons' names.
    fn say(&self, state: &mut State) {
        let [(form, _)] = self.forms.as_slice() else {
            return;
        };
        let unseen = crate::notifications::with(|c| c.stack.badge());
        let user = crate::user::name();
        let names: Vec<String> = buttons(*form)
            .iter()
            .map(|item| self.words.name(*item, unseen, &user))
            .collect();
        let said = Some((*form, names));
        if said == state.said {
            return;
        }
        state.said = said;
        if let Some((form, names)) = &state.said {
            tracing::debug!(bar = %self.bar, form = form.name(), ?names, "the footer shows it");
        }
    }
}

impl canvas::Program<Message> for Footer {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let size = bounds.size();
        match event {
            iced::Event::Window(window::Event::RedrawRequested(_)) => {
                self.say(state);
                None
            }
            iced::Event::Mouse(seen) => {
                let at = pointer(seen, bounds, cursor, state.pointer);
                state.pointer = at;
                let under = at.and_then(|at| {
                    self.items(size)
                        .into_iter()
                        .find(|(_, rect)| rect.contains(at))
                });
                let over = under.map(|(item, _)| item);
                match seen {
                    mouse::Event::CursorEntered
                    | mouse::Event::CursorMoved { .. }
                    | mouse::Event::CursorLeft => {
                        if over == state.hover {
                            return None;
                        }
                        if state.hover.take().is_some() {
                            let at = at.unwrap_or(Point::new(size.width / 2.0, size.height));
                            popover::event(popover::Event::ItemLeave {
                                at: self.in_bar(at, size),
                            });
                        }
                        state.hover = over;
                        if let Some((item, rect)) = under {
                            popover::event(popover::Event::ItemEnter {
                                bar: self.bar.clone(),
                                item,
                                rect: self.in_bar_rect(rect, size),
                            });
                        }
                        Some(canvas::Action::request_redraw())
                    }
                    mouse::Event::ButtonPressed(mouse::Button::Left) => {
                        let (item, rect) = under?;
                        tracing::debug!(?item, "a footer's button pressed");
                        popover::event(popover::Event::ItemPress {
                            bar: self.bar.clone(),
                            item,
                            rect: self.in_bar_rect(rect, size),
                        });
                        Some(canvas::Action::request_redraw().and_capture())
                    }
                    _ => None,
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) => popover::key(key, *modifiers, text.as_deref())
                .then(|| canvas::Action::request_redraw().and_capture()),
            _ => None,
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
        let size = bounds.size();
        let under = crate::backdrop::under(renderer, size, self.backdrop);
        // Where they are in the bar, for the keyboard going through the
        // bar's items (the bar's height read first: not under the same
        // lock).
        let in_bar: Vec<(Item, Rectangle)> = self
            .items(size)
            .into_iter()
            .map(|(item, r)| (item, self.in_bar_rect(r, size)))
            .collect();
        popover::with(|c| c.set_items(&self.bar, FOOTER_NAMESPACE, in_bar));
        let (count, dnd) = crate::notifications::with(|c| (c.stack.badge(), c.stack.dnd.is_on()));
        let still = self.forms.len() == 1;
        let bottom = self.bottom();
        // The bar's inside, as wide as it is while it switches.
        let left = (size.width + 2.0 - self.bar_width).max(0.0);
        let column = Rectangle::new(
            Point::new(left, 0.0),
            Size::new((size.width - left).max(0.0), size.height),
        );
        frame.with_clip(column, |f| {
            for &(form, alpha) in &self.forms {
                for (i, item) in buttons(form).iter().copied().enumerate() {
                    let button = rect(form, i, size, bottom, self.bar_width);
                    let (open, focused) = if still {
                        popover::with(|c| (c.shows(&self.bar, item), c.focused(&self.bar, item)))
                    } else {
                        (false, false)
                    };
                    let hover = still && state.hover == Some(item);
                    match item {
                        Item::Notifications => bell::draw(
                            f,
                            button,
                            bell::Shows {
                                count,
                                dnd,
                                open,
                                hover,
                                focused,
                            },
                            alpha,
                        ),
                        Item::User => {
                            bell::lit(f, button, open, hover, focused);
                            avatar(f, button, alpha);
                        }
                        _ => {
                            bell::lit(f, button, open, hover, focused);
                            if let Some(glyph) = glyph(item) {
                                bell::glyph_in(f, button, glyph, bell::GLYPH_INK, alpha);
                            }
                        }
                    }
                }
            }
        });
        crate::backdrop::over(under, frame)
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.hover.is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buttons_show_the_same_on_the_opaque_bar_as_on_glass() {
        let colour = Color::from_rgb8(0x1b, 0x1d, 0x22);
        for form in [Form::Full, Form::Mini] {
            let ((w, h), _) = placement(form, false);
            let drawn = crate::backdrop::tests::drawn(
                |backdrop| Footer {
                    bar: "eDP-1".to_owned(),
                    forms: vec![(form, 1.0)],
                    bar_width: form.width() as f32,
                    words: Words::new(Lang::French),
                    backdrop,
                },
                &State::default(),
                Size::new(w, h),
                colour,
            );
            crate::backdrop::tests::assert_drawn_over(&drawn, &format!("{form:?}"));
        }
    }

    fn rects(form: Form) -> Vec<Rectangle> {
        let ((w, h), bottom) = placement(form, false);
        let size = Size::new(w as f32, h as f32);
        (0..buttons(form).len())
            .map(|i| rect(form, i, size, bottom, form.width() as f32))
            .collect()
    }

    #[test]
    fn the_full_forms_four_buttons_spread_evenly_across_the_bars_inside() {
        let ((w, h), bottom) = placement(Form::Full, false);
        assert_eq!((w, h), (194, 46));
        assert_eq!(bottom, 7.0);
        assert_eq!(margin(bottom), (0, EDGE + 1, EDGE + 7, 0));
        let rects = rects(Form::Full);
        // The bar's 12 px padding on each side (the surface starts inside
        // its rim), 40 × 34, 4 px apart, the row's bottom 13 px over the
        // bar's (its rim and 12 px).
        let xs: Vec<f32> = rects.iter().map(|r| r.x).collect();
        assert_eq!(xs, [11.0, 55.0, 99.0, 143.0]);
        assert_eq!(194.0 - (rects[3].x + rects[3].width), 11.0);
        for r in &rects {
            assert_eq!(r.size(), FULL_BUTTON);
            assert_eq!(r.y, ROOM);
            assert_eq!(bottom + h as f32 - (r.y + r.height), FULL_BOTTOM);
        }
        assert_eq!(
            buttons(Form::Full),
            [Item::Clipboard, Item::Notifications, Item::Menu, Item::User]
        );
    }

    #[test]
    fn the_mini_forms_bell_sits_over_the_avatar_last_of_all() {
        let ((w, h), bottom) = placement(Form::Mini, false);
        assert_eq!((w, h), (54, 82));
        assert_eq!(bottom, 5.0);
        let rects = rects(Form::Mini);
        assert_eq!(buttons(Form::Mini), [Item::Notifications, Item::User]);
        let (bell, user) = (rects[0], rects[1]);
        assert_eq!(bell.size(), MINI_BUTTON);
        assert_eq!(user.size(), MINI_BUTTON);
        // Centred, the avatar's bottom 11 px over the bar's (its rim and
        // 10 px), the bell 2 px over it.
        assert_eq!(bell.x, 5.0);
        assert_eq!(user.x, 5.0);
        assert_eq!(bottom + h as f32 - (user.y + user.height), MINI_BOTTOM);
        assert_eq!(user.y - (bell.y + bell.height), MINI_GAP);
        assert_eq!(bell.y, ROOM);
        assert_eq!(MINI_HEIGHT, 81.0);
    }

    #[test]
    fn while_the_bar_switches_the_surface_holds_both_forms() {
        let ((w, h), bottom) = placement(Form::Mini, true);
        assert_eq!(w, 194);
        assert_eq!(bottom, 5.0);
        assert_eq!(placement(Form::Full, true), ((w, h), bottom));
        // Both forms' buttons, and their rings, on it.
        let size = Size::new(w as f32, h as f32);
        for form in [Form::Full, Form::Mini] {
            for i in 0..buttons(form).len() {
                let r = rect(form, i, size, bottom, form.width() as f32);
                assert!(r.y >= 5.0 && r.y + r.height <= size.height - 5.0, "{r:?}");
                assert!(r.x >= 5.0 && r.x + r.width <= size.width - 5.0, "{r:?}");
            }
        }
    }

    #[test]
    fn each_button_has_its_name() {
        let fr = Words {
            lang: hocus_words::Lang::French,
        };
        let en = Words {
            lang: hocus_words::Lang::English,
        };
        assert_eq!(fr.name(Item::Clipboard, 0, "Alex"), "Presse-papiers");
        assert_eq!(
            fr.name(Item::Notifications, 3, "Alex"),
            "Notifications, 3 non vues"
        );
        assert_eq!(fr.name(Item::Notifications, 0, "Alex"), "Notifications");
        assert_eq!(
            fr.name(Item::Menu, 0, "Alex"),
            "Menu : toutes les applications"
        );
        assert_eq!(fr.name(Item::User, 0, "Alex"), "Alex, menu de session");
        assert_eq!(en.name(Item::Clipboard, 0, "Alex"), "Clipboard");
        assert_eq!(
            en.name(Item::Notifications, 1, "Alex"),
            "Notifications, 1 unseen"
        );
        assert_eq!(en.name(Item::Menu, 0, "Alex"), "Menu: all applications");
        assert_eq!(en.name(Item::User, 0, "Alex"), "Alex, session menu");
    }
}
