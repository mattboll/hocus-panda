//! The footer's user's popover, the session menu (7.4, variant A: the
//! popover grows into the menu): the user's avatar and name, "Réglages"
//! (6.1), "Verrouiller", "Mettre en veille", "Se déconnecter",
//! "Redémarrer", "Éteindre".
//!
//! The compositor decides (`crate::leave` there): what logind allows
//! ([`hocus_link::LeaveCan`]), and a confirmation under way
//! ([`hocus_link::LeaveShown`]), which this popover shows in place of the
//! rows, the header kept: the countdown with "… maintenant" and
//! "Annuler", or the applications holding it back, each with "Revenir à
//! …" (and "Fermer quand même" for a window that does not close), "…
//! quand même" and "Annuler". `Super+Shift+E` opens it on the log-out
//! confirmation. The keyboard lands on "Annuler", so that a stray
//! `Enter` ends nothing; `Esc` cancels, `Esc` again closes. While a
//! confirmation shows, the pointer leaving does not close it.
//!
//! What the compositor said is kept here ([`set_can`], [`set_shown`]),
//! from the link's task, without a message: the popover alone draws again.
//!
//! Built with the shell's widget kit ([`crate::ui`]), its parts pinned
//! where [`layout`] places them.

mod layout;
mod words;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_a11y::Role;
use hocus_link::{
    HolderKind, LEAVE_COUNTDOWN, Launch, LeaveAction, LeaveAsk, LeaveCan, LeaveShown, LeaveStep,
    LockAsk, POPOVER_WIDTH, ToCompositor,
};
use iced::{
    Border, Color, Element, Length, Padding, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    alignment::Vertical,
    keyboard, mouse,
    widget::{Space, button, canvas, container, pin, row, space},
    window,
};

use self::layout::{Block, Layout, Named, Placed, Row, Target, View};
pub use self::words::Words;
use super::{Effect, Event as Seen, Item, Pressed, event, kit};
use crate::levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT};
use crate::theme::Ink;
use crate::ui::{
    self, Around, control,
    parts::drawn,
    style::{self, said},
};

type El<'a> = Element<'a, Msg>;

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();

const ROW_RADIUS: f32 = 8.0;
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const ROW_TEXT: f32 = 14.0;
const ROW_PAD: f32 = 6.0;
const TILE_RADIUS: f32 = 8.0;
const TILE_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const TILE_GAP: f32 = 10.0;
const GLYPH: f32 = 17.0;
const GLYPH_INK: &str = "#C9D0DC";
const SHORTCUT: f32 = 11.0;
const MARTIAN: iced::Font = iced::Font::with_name("Martian Mono");
const GREYED: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.28));
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const NAME_SIZE: f32 = 16.0;
const AMBER: Ink = Ink::Dark(crate::levels::draw::rgb(0xF5C46E));
const AMBER_TILE: Ink = Ink::Dark(Color::from_rgba8(245, 196, 110, 0.14));
const AMBER_INK: Ink = Ink::Dark(crate::levels::draw::rgb(0x1C1406));
const ANYWAY_FILL: Ink = Ink::Dark(Color::from_rgba8(245, 196, 110, 0.12));
const ANYWAY_BORDER: Ink = Ink::Dark(Color::from_rgba8(245, 196, 110, 0.45));
const TRACK: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const BUTTON_RADIUS: f32 = 9.0;
const BUTTON_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.06));
const BUTTON_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.10));
const BUTTON_TEXT: f32 = 13.5;
const BLOCK_RADIUS: f32 = 10.0;
const BLOCK_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const SMALL_RADIUS: f32 = 7.0;
const SMALL_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const SMALL_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const ICON_RADIUS: f32 = 8.0;
/// The bar drains in frames this far apart, on a GPU with motion.
const FRAME: Duration = Duration::from_millis(16);

// --- What the compositor said ---------------------------------------------------

#[derive(Debug, Default)]
struct Store {
    can: LeaveCan,
    /// The confirmation under way, and when it was told (its countdown
    /// counts from then).
    shown: Option<(LeaveShown, Instant)>,
    /// Bumped on each change: the surface lays itself out again.
    generation: u64,
}

static STORE: Mutex<Option<Store>> = Mutex::new(None);

fn store<T>(with: impl FnOnce(&mut Store) -> T) -> T {
    let mut store = STORE.lock().unwrap_or_else(PoisonError::into_inner);
    with(store.get_or_insert_with(Store::default))
}

/// What logind allows, as the compositor says it.
pub fn set_can(can: LeaveCan) {
    let changed = store(|s| {
        let changed = s.can != can;
        s.can = can;
        s.generation += u64::from(changed);
        changed
    });
    if changed {
        tracing::debug!(?can, "the session menu: what logind allows");
        super::changed(Item::User);
    }
}

/// A confirmation under way (none: the rows again): the popover opens on
/// its bar if it was not open (`Super+Shift+E`), and stays while it shows.
pub fn set_shown(shown: Option<LeaveShown>) {
    let opens = shown.as_ref().map(|s| s.output.clone());
    store(|s| {
        s.shown = shown.map(|shown| (shown, Instant::now()));
        s.generation += 1;
    });
    if let Some(output) = opens {
        let effects = super::with(|c| c.open_session(&output, Instant::now()));
        super::send(effects);
    }
    super::changed(Item::User);
}

/// The power button asks what to do (6.10): the menu opens on `output`'s
/// bar, as a click on the footer's user opens it.
pub fn open(output: &str) {
    let effects = super::with(|c| c.open_session(output, Instant::now()));
    super::send(effects);
    super::changed(Item::User);
}

/// The popover opens: what logind allows is read again; nothing holds
/// the ring.
pub(super) fn opening() {
    ui(|ui| *ui = Ui::default());
    super::tell(ToCompositor::Leave(LeaveAsk::Open));
}

/// The popover closes: a confirmation under way is cancelled (not one
/// that happens already).
pub(super) fn closing() {
    let under_way = store(|s| {
        s.shown
            .as_ref()
            .is_some_and(|(shown, _)| shown.step != LeaveStep::Acting)
    });
    if under_way {
        tracing::info!("the session menu closes: its confirmation is cancelled");
        super::tell(ToCompositor::Leave(LeaveAsk::Cancel));
    }
}

/// Whether a confirmation shows: the popover stays when the pointer
/// leaves.
pub(super) fn confirming() -> bool {
    store(|s| s.shown.is_some())
}

/// Its height now.
pub fn height() -> u32 {
    lay_out(Words::new(crate::clock::time::Lang::of_messages()))
        .0
        .height()
}

/// The popover's layout for what is said now, and its generation.
fn lay_out(words: Words) -> (Layout, u64) {
    let (can, shown, generation) = store(|s| (s.can, s.shown.clone(), s.generation));
    let layout = match shown {
        None => layout::rows(can),
        Some((shown, _)) => match &shown.step {
            LeaveStep::Held(holders) if !holders.is_empty() => {
                let named: Vec<Named> = holders.iter().map(|h| named(h, words)).collect();
                let mut measure = |t: &str, size: f32, bold: bool| {
                    kit::measure(t, size, if bold { MANROPE_BOLD } else { MANROPE })
                };
                layout::held(shown.action, &named, words, &mut measure)
            }
            step => layout::confirm(shown.action, step),
        },
    };
    (layout, generation)
}

/// A holder as people know it: its application's name, what it says.
fn named(holder: &hocus_link::Holder, words: Words) -> Named {
    let name = crate::share::app_name(&holder.app)
        .filter(|n| !n.trim().is_empty())
        .unwrap_or_else(|| words.an_app().to_owned());
    let reason = match holder.kind {
        HolderKind::Inhibitor { .. } => holder.text.clone(),
        HolderKind::Window { .. } => words.window_open(&holder.text),
    };
    Named {
        kind: holder.kind,
        app: holder.app.clone(),
        name,
        reason,
    }
}

/// The glyphs (the mockup's 24 px paths): the Settings' sliders, a
/// padlock, a moon, a door and an arrow leaving it, an arrow going round,
/// the power sign.
const PATHS: [&str; 6] = [
    "M4 7 H13 M17 7 H20 M15 4.5 V9.5 M4 17 H7 M11 17 H20 M9 14.5 V19.5",
    "M7.5 11 V8 a4.5 4.5 0 0 1 9 0 V11 M6.5 11 H17.5 a1.5 1.5 0 0 1 1.5 1.5 V18.5 \
     a1.5 1.5 0 0 1 -1.5 1.5 H6.5 a1.5 1.5 0 0 1 -1.5 -1.5 V12.5 a1.5 1.5 0 0 1 1.5 -1.5 Z",
    "M19.5 14.5 A7.5 7.5 0 1 1 9.5 4.5 A6 6 0 0 0 19.5 14.5 Z",
    "M10 5 H6.5 a1.5 1.5 0 0 0 -1.5 1.5 V17.5 a1.5 1.5 0 0 0 1.5 1.5 H10 \
     M14 8 L18 12 L14 16 M18 12 H9.5",
    "M19.5 12 a7.5 7.5 0 1 1 -2.2 -5.3 M19.5 4.5 V8.5 H15.5",
    "M12 3.5 V11.5 M7.2 6.6 a7.5 7.5 0 1 0 9.6 0",
];

fn path_of(row: Row) -> usize {
    match row {
        Row::Settings => 0,
        Row::Lock => 1,
        Row::Suspend => 2,
        Row::Leave(LeaveAction::LogOut) => 3,
        Row::Leave(LeaveAction::Restart) => 4,
        Row::Leave(LeaveAction::ShutDown) => 5,
    }
}

/// Row `row`'s glyph, in the rows' ink (`amber`: the confirmation's).
fn glyph(row: Row, amber: bool) -> svg::Handle {
    static GLYPHS: std::sync::OnceLock<[[svg::Handle; 6]; 2]> = std::sync::OnceLock::new();
    let make = |path: &str, ink: &str| {
        svg::Handle::from_memory(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" \
                 width=\"24\" height=\"24\"><path d=\"{path}\" \
                 fill=\"none\" stroke=\"{ink}\" stroke-width=\"1.9\" stroke-linecap=\"round\" \
                 stroke-linejoin=\"round\"/></svg>"
            )
            .into_bytes(),
        )
    };
    let glyphs =
        GLYPHS.get_or_init(|| [GLYPH_INK, "#F5C46E"].map(|ink| PATHS.map(|path| make(path, ink))));
    glyphs[usize::from(amber)][path_of(row)].clone()
}

// --- The surface ------------------------------------------------------------------

/// The popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
    /// Things move (a GPU, no reduced motion): the bar drains smoothly,
    /// else a step a second.
    pub motion: bool,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("session:{self:?}")
    }
}

/// What it keeps while it shows.
#[derive(Debug, Default)]
struct Ui {
    /// What holds the keyboard.
    focus: Option<Target>,
    /// What it was built from last: the store's generation, and the kind
    /// of view (rows, a confirmation, the list).
    built: Option<(u64, std::mem::Discriminant<View>)>,
    /// What was said last of what it shows, for the log.
    said: Option<String>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A click on it: the ring goes, it acts.
    Click(Target),
    /// `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// `Esc` while a confirmation shows: it is cancelled.
    Cancel,
    /// What the compositor said changed under the view.
    Stale,
}

/// The popover on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (words, motion) = (panel.words, panel.motion);
    let order = lay_out(words).0.order();
    Around::new(panel.id, build(words, motion), handle, Msg::Stale)
        .rebuild(move || build(words, motion))
        .stale_when(move || {
            let generation = store(|s| s.generation);
            ui(|ui| ui.built.is_none_or(|(g, _)| g != generation))
        })
        .keys(move |pressed| keys(pressed, &order))
        .on_glass(panel.glass)
}

/// `Esc` cancels a confirmation (else the kit closes the popover); the
/// arrows with nothing holding the ring, the first (down) or the last
/// (up); the kit takes them round after.
fn keys(pressed: &Pressed, order: &[Target]) -> Option<Msg> {
    use keyboard::{Key, key::Named as K};
    match &pressed.key {
        Key::Named(K::Escape) if confirming() => Some(Msg::Cancel),
        Key::Named(arrow @ (K::ArrowDown | K::ArrowUp)) if ui(|ui| ui.focus.is_none()) => {
            let target = if *arrow == K::ArrowDown {
                order.first()
            } else {
                order.last()
            };
            target.map(|t| Msg::Focus(*t))
        }
        _ => None,
    }
}

/// Its content, from what the compositor said now.
fn build<'a>(words: Words, motion: bool) -> El<'a> {
    let (layout, generation) = lay_out(words);
    let name = crate::user::name();
    let room = kit::RIGHT - (kit::LEFT + layout::AVATAR + TILE_GAP);
    let name = kit::elide(&name, room, |t| kit::measure(t, NAME_SIZE, MANROPE_BOLD));
    let kind = std::mem::discriminant(&layout.view);
    let focus = ui(|ui| {
        if ui.built.map(|(g, _)| g) != Some(generation) {
            // Into a confirmation, or the list: the keyboard on its
            // first place ("… maintenant", "Annuler"); back to the rows,
            // on none. What held it and is gone lets it go.
            if ui.built.map(|(_, k)| k) != Some(kind)
                || ui.focus.is_some_and(|t| !layout.order().contains(&t))
            {
                ui.focus = layout.first_focus();
            }
            ui.built = Some((generation, kind));
            super::refit(Item::User);
        }
        say(words, &name, &layout, ui);
        ui.focus
    });
    let base = Space::new()
        .width(Length::Fill)
        .height(layout.end + kit::PAD_BOTTOM + kit::BORDER);
    let mut layers: Vec<El<'a>> = vec![base.into()];
    layers.extend(header(name));
    match &layout.view {
        View::Rows { rows, dividers } => {
            for y in dividers {
                layers.push(at(
                    container(Space::new().width(kit::RIGHT - kit::LEFT).height(1.0))
                        .style(|_| style::boxed(RULE.get(), 0.0)),
                    Point::new(kit::LEFT, *y),
                ));
            }
            for placed in rows {
                layers.push(row_of(words, placed, focus));
            }
        }
        View::Confirm {
            action,
            top,
            sentence,
            bar,
            now,
            cancel,
        } => {
            layers.extend(title(*action, *top, &[words.title(*action).to_owned()]));
            let step = store(|s| s.shown.clone());
            if let Some((shown, told)) = step {
                let said_now = match &shown.step {
                    LeaveStep::Counting { left_ms } => {
                        let left = left_of(*left_ms, told, motion);
                        let seconds = left.as_millis().div_ceil(1000) as u32;
                        words.counting(*action, seconds)
                    }
                    LeaveStep::Authorizing => words.authorizing().to_owned(),
                    LeaveStep::Closing => words.closing().to_owned(),
                    LeaveStep::Acting => words.acting(*action).to_owned(),
                    LeaveStep::Held(_) => String::new(),
                };
                layers.push(middled(
                    said(said_now, layout::SENTENCE, MANROPE, DIM.get()),
                    kit::LEFT,
                    *sentence,
                ));
            }
            if let Some(bar) = bar {
                layers.push(at(
                    canvas(Drain { motion }).width(bar.width).height(bar.height),
                    bar.position(),
                ));
            }
            if let Some(rect) = now {
                layers.push(wide(
                    *rect,
                    words.now(*action),
                    Look::Amber,
                    Target::Now,
                    focus,
                ));
            }
            if let Some(rect) = cancel {
                layers.push(wide(
                    *rect,
                    words.cancel(),
                    Look::Plain,
                    Target::Cancel,
                    focus,
                ));
            }
        }
        View::Held {
            action,
            top,
            title: lines,
            sentence,
            sentence_top,
            blocks,
            anyway,
            cancel,
        } => {
            layers.extend(title(*action, *top, lines));
            let line = layout::HELD_SENTENCE * layout::LINE;
            for (i, said_line) in sentence.iter().enumerate() {
                layers.push(middled(
                    said(said_line.clone(), layout::HELD_SENTENCE, MANROPE, DIM.get()),
                    kit::LEFT,
                    sentence_top + line * (i as f32 + 0.5),
                ));
            }
            layers.push(at(
                ui::line(Space::new(), sentence.join(" ")),
                Point::new(kit::LEFT, *sentence_top),
            ));
            let holders = store(|s| match s.shown.as_ref().map(|(s, _)| &s.step) {
                Some(LeaveStep::Held(holders)) => holders.clone(),
                _ => Vec::new(),
            });
            for (i, block) in blocks.iter().enumerate() {
                let app = holders.get(i).map_or("", |h| h.app.as_str());
                layers.extend(block_of(words, block, app, i, focus));
            }
            layers.push(wide(
                *anyway,
                words.anyway(*action),
                Look::Anyway,
                Target::Anyway,
                focus,
            ));
            layers.push(wide(
                *cancel,
                words.cancel(),
                Look::Plain,
                Target::Cancel,
                focus,
            ));
        }
    }
    iced::widget::Stack::with_children(layers).into()
}

/// `content` with its top left corner at `point` in the popover.
fn at<'a>(content: impl Into<El<'a>>, point: Point) -> El<'a> {
    pin(content).x(point.x).y(point.y).into()
}

/// A line of text from `x`, its middle at `middle`.
fn middled<'a>(content: impl Into<El<'a>>, x: f32, middle: f32) -> El<'a> {
    const ROOM: f32 = 40.0;
    at(
        container(content).height(ROOM).align_y(Vertical::Center),
        Point::new(x, middle - ROOM / 2.0),
    )
}

/// The header: the avatar, the name.
fn header<'a>(name: String) -> [El<'a>; 2] {
    let middle = kit::HEADER_TOP + layout::HEADER / 2.0;
    let avatar = drawn(layout::AVATAR, layout::AVATAR, |f, s| {
        crate::lock::draw::avatar_round(f, Rectangle::new(Point::ORIGIN, s), 1.0);
    });
    [
        at(avatar, Point::new(kit::LEFT, middle - layout::AVATAR / 2.0)),
        middled(
            ui::line(
                said(name.clone(), NAME_SIZE, MANROPE_BOLD, TEXT.get()),
                name,
            ),
            kit::LEFT + layout::AVATAR + TILE_GAP,
            middle,
        ),
    ]
}

/// A glyph's tile `size` wide, filled with `fill`, its glyph (`amber`:
/// the confirmation's; faded by `opacity`).
fn tile<'a>(row: Row, fill: Color, amber: bool, opacity: f32) -> El<'a> {
    drawn(layout::TILE, layout::TILE, move |f, s| {
        let tile = Rectangle::new(Point::ORIGIN, s);
        f.fill(&kit::rounded(tile, TILE_RADIUS), fill);
        let centre = tile.center();
        f.draw_svg(
            Rectangle::new(
                Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
                Size::new(GLYPH, GLYPH),
            ),
            svg::Svg::new(glyph(row, amber)).opacity(opacity),
        );
    })
}

/// The confirmation's title row at `top`: the action's glyph in amber on
/// its tile, the title (on several lines when it must).
fn title<'a>(action: LeaveAction, top: f32, lines: &[String]) -> Vec<El<'a>> {
    let mut layers = vec![at(
        tile(Row::Leave(action), AMBER_TILE.get(), true, 1.0),
        Point::new(kit::LEFT, top),
    )];
    let line = layout::TITLE * layout::LINE;
    let block = (lines.len() as f32 * line).max(layout::TILE);
    let first = top + (block - lines.len() as f32 * line) / 2.0;
    for (i, said_line) in lines.iter().enumerate() {
        layers.push(middled(
            said(said_line.clone(), layout::TITLE, MANROPE_BOLD, TEXT.get()),
            kit::LEFT + layout::TILE + TILE_GAP,
            first + line * (i as f32 + 0.5),
        ));
    }
    layers.push(at(
        ui::line(Space::new(), lines.join(" ")),
        Point::new(kit::LEFT, top),
    ));
    layers
}

/// A row of the menu: its glyph on its tile, its words, Lock's keys.
fn row_of<'a>(words: Words, placed: &Placed, focus: Option<Target>) -> El<'a> {
    let rect = placed.rect;
    let target = Target::Row(placed.row);
    let enabled = placed.enabled;
    let alpha = if enabled { 1.0 } else { 0.4 };
    let glyph_alpha = if enabled { 1.0 } else { 0.35 };
    let word = row_word(words, placed.row);
    let mut line = row![
        tile(placed.row, TILE_FILL.scale_alpha(alpha), false, glyph_alpha),
        Space::new().width(TILE_GAP),
        said(
            word,
            ROW_TEXT,
            kit::MANROPE_SEMIBOLD,
            if enabled { TEXT } else { GREYED }.get()
        ),
        space::horizontal(),
    ]
    .align_y(Vertical::Center);
    if placed.row == Row::Lock {
        line = line.push(said("Super+L", SHORTCUT, MARTIAN, FAINT.get()));
    }
    let pressed = button(container(line).center_y(Length::Fill))
        .padding(Padding {
            left: ROW_PAD,
            right: ROW_PAD,
            ..Padding::ZERO
        })
        .width(rect.width)
        .height(rect.height)
        .style(move |_, status| {
            if enabled && style::lit(status) {
                style::hovered(ROW_HOVER.get(), ROW_RADIUS)
            } else {
                style::clear()
            }
        })
        .on_press_maybe(enabled.then_some(Msg::Click(target)));
    let mut declared =
        control(pressed, target.id(), Role::Button, word).node(|n| n.disabled(!enabled));
    if enabled {
        declared = declared
            .focus(focus == Some(target), Msg::Focus(target), ROW_RADIUS)
            .activate(Msg::Act(target))
            .rows(0);
    }
    at(declared, rect.position())
}

/// How a wide button looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Look {
    /// "… maintenant": amber.
    Amber,
    /// "Annuler".
    Plain,
    /// "… quand même": amber's border on its tint.
    Anyway,
}

/// A full-width button at `rect`.
fn wide<'a>(
    rect: Rectangle,
    word: &str,
    look: Look,
    target: Target,
    focus: Option<Target>,
) -> El<'a> {
    let (ink, font) = match look {
        Look::Amber => (AMBER_INK, MANROPE_BOLD),
        Look::Plain => (TEXT, kit::MANROPE_SEMIBOLD),
        Look::Anyway => (AMBER, MANROPE_BOLD),
    };
    let pressed =
        button(container(said(word.to_owned(), BUTTON_TEXT, font, ink.get())).center(Length::Fill))
            .padding(0)
            .width(rect.width)
            .height(rect.height)
            .style(move |_, status| {
                let hover = style::lit(status);
                let (fill, border) = match look {
                    Look::Amber => (
                        if hover {
                            AMBER.scale_alpha(0.9)
                        } else {
                            AMBER.get()
                        },
                        None,
                    ),
                    Look::Plain => (if hover { BUTTON_HOVER } else { BUTTON_FILL }.get(), None),
                    Look::Anyway => (
                        if hover { AMBER_TILE } else { ANYWAY_FILL }.get(),
                        Some(ANYWAY_BORDER.get()),
                    ),
                };
                button::Style {
                    background: Some(fill.into()),
                    border: Border {
                        color: border.unwrap_or(Color::TRANSPARENT),
                        width: if border.is_some() { 1.0 } else { 0.0 },
                        radius: BUTTON_RADIUS.into(),
                    },
                    ..button::Style::default()
                }
            })
            .on_press(Msg::Click(target));
    let declared = control(pressed, target.id(), Role::Button, word)
        .focus(focus == Some(target), Msg::Focus(target), BUTTON_RADIUS)
        .activate(Msg::Act(target))
        .rows(0);
    at(declared, rect.position())
}

/// A small button of a block.
fn small<'a>(
    rect: Rectangle,
    word: &str,
    name: String,
    target: Target,
    focus: Option<Target>,
) -> El<'a> {
    let pressed = button(
        container(said(
            word.to_owned(),
            layout::REASON,
            kit::MANROPE_SEMIBOLD,
            TEXT.get(),
        ))
        .center(Length::Fill),
    )
    .padding(0)
    .width(rect.width)
    .height(rect.height)
    .style(|_, status| {
        let fill = if style::lit(status) {
            SMALL_HOVER
        } else {
            SMALL_FILL
        };
        style::filled(fill.get(), SMALL_RADIUS, TEXT.get())
    })
    .on_press(Msg::Click(target));
    let declared = control(pressed, target.id(), Role::Button, name)
        .focus(focus == Some(target), Msg::Focus(target), SMALL_RADIUS)
        .activate(Msg::Act(target))
        .rows(0);
    at(declared, rect.position())
}

/// Block `i` of the list: application `app`'s.
fn block_of<'a>(
    words: Words,
    block: &Block,
    app: &str,
    i: usize,
    focus: Option<Target>,
) -> Vec<El<'a>> {
    let mut layers = vec![at(
        container(
            Space::new()
                .width(block.rect.width)
                .height(block.rect.height),
        )
        .style(|_| style::boxed(BLOCK_FILL.get(), BLOCK_RADIUS)),
        block.rect.position(),
    )];
    let icon = icon_of(app, &block.name);
    layers.push(at(
        drawn(block.icon.width, block.icon.height, move |f, s| {
            icon(f, Rectangle::new(Point::ORIGIN, s));
        }),
        block.icon.position(),
    ));
    let words_left = block.back.x;
    layers.push(middled(
        said(block.name.clone(), layout::NAME, MANROPE_BOLD, TEXT.get()),
        words_left,
        block.name_middle,
    ));
    let line = layout::REASON * layout::LINE;
    for (n, said_line) in block.reason.iter().enumerate() {
        layers.push(middled(
            said(said_line.clone(), layout::REASON, MANROPE, DIM.get()),
            words_left,
            block.reason_top + line * (n as f32 + 0.5),
        ));
    }
    layers.push(at(
        ui::line(
            Space::new(),
            format!("{}, {}", block.name, block.reason.join(" ")),
        ),
        block.rect.position(),
    ));
    layers.push(small(
        block.back,
        &block.back_word,
        block.back_word.clone(),
        Target::Back(i),
        focus,
    ));
    if let Some(force) = block.force {
        layers.push(small(
            force,
            words.close_anyway(),
            format!("{}, {}", words.close_anyway(), block.name),
            Target::Force(i),
            focus,
        ));
    }
    layers
}

/// What draws an application's icon: its entry's, else its letter on a
/// colour of its own (3.4b).
fn icon_of(app: &str, name: &str) -> impl Fn(&mut canvas::Frame, Rectangle) + 'static {
    let scale = kit::popover_scale();
    let px = crate::launcher::icons::pixels(layout::ICON as u32, scale);
    let handle = crate::share::entry_of(app)
        .and_then(|e| e.icon)
        .and_then(|icon| match crate::launcher::icons::of(&icon, px) {
            Some(found) => found,
            None => {
                crate::launcher::icons::want([icon.as_str()], layout::ICON as u32, scale);
                None
            }
        });
    let name = name.to_owned();
    move |f, rect| match &handle {
        Some(handle) => f.draw_image(rect, canvas::Image::new(handle.clone())),
        None => kit::app_tile(f, rect, (None, &name), (ICON_RADIUS, 14.0), 1.0),
    }
}

/// What is left of the countdown told `left_ms` at `told`: between two
/// tells, a GPU with motion drains it on its own clock, a second at most;
/// else what was told.
fn left_of(left_ms: u32, told: Instant, motion: bool) -> Duration {
    let said_left = Duration::from_millis(u64::from(left_ms));
    if motion {
        said_left.saturating_sub(told.elapsed().min(Duration::from_secs(1)))
    } else {
        said_left
    }
}

/// The countdown's bar: it drains, a frame at a time on a GPU with
/// motion, else a step a second.
struct Drain {
    motion: bool,
}

impl<M> canvas::Program<M> for Drain {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let counting = store(|s| {
            s.shown
                .as_ref()
                .is_some_and(|(s, _)| matches!(s.step, LeaveStep::Counting { .. }))
        });
        (self.motion && counting).then(|| canvas::Action::request_redraw_at(*now + FRAME))
    }

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let bar = Rectangle::new(Point::ORIGIN, bounds.size());
        f.fill(&kit::rounded(bar, 2.0), TRACK.get());
        let left = store(|s| match s.shown.as_ref() {
            Some((shown, told)) => match shown.step {
                LeaveStep::Counting { left_ms } => Some(left_of(left_ms, *told, self.motion)),
                LeaveStep::Closing => Some(Duration::ZERO),
                _ => None,
            },
            None => None,
        });
        let left = left.unwrap_or(LEAVE_COUNTDOWN);
        // Smooth on a GPU with motion; else a step a second.
        let left = if self.motion {
            left.as_secs_f32()
        } else {
            left.as_secs_f32().ceil()
        };
        let part = (left / LEAVE_COUNTDOWN.as_secs_f32()).clamp(0.0, 1.0);
        if part > 0.0 {
            let mut drained = bar;
            drained.width *= part;
            f.fill(&kit::rounded(drained, 2.0), AMBER.get());
        }
        vec![f.into_geometry()]
    }
}

fn row_word(words: Words, row: Row) -> &'static str {
    match row {
        Row::Settings => words.settings(),
        Row::Lock => words.lock(),
        Row::Suspend => words.suspend(),
        Row::Leave(action) => words.row(action),
    }
}

/// What it shows, said in the log once (the tests read it).
fn say(words: Words, name: &str, layout: &Layout, ui: &mut Ui) {
    let said_now = match &layout.view {
        View::Rows { rows, .. } => {
            let rows: Vec<String> = rows
                .iter()
                .map(|p| {
                    let word = row_word(words, p.row);
                    if p.enabled {
                        word.to_owned()
                    } else {
                        format!("({word})")
                    }
                })
                .collect();
            format!("rows {rows:?}")
        }
        View::Confirm { action, .. } => {
            let step = store(|s| s.shown.as_ref().map(|(s, _)| s.step.clone()));
            let sentence = match &step {
                Some(LeaveStep::Counting { left_ms }) => {
                    words.counting(*action, left_ms.div_ceil(1000))
                }
                Some(LeaveStep::Authorizing) => words.authorizing().to_owned(),
                Some(LeaveStep::Closing) => words.closing().to_owned(),
                Some(LeaveStep::Acting) => words.acting(*action).to_owned(),
                _ => String::new(),
            };
            format!("{} « {sentence} »", words.title(*action))
        }
        View::Held { title, blocks, .. } => {
            let names: Vec<String> = blocks
                .iter()
                .map(|b| format!("{}: {}", b.name, b.reason.join(" ")))
                .collect();
            format!("{} {names:?}", title.join(" "))
        }
    };
    if ui.said.as_ref() == Some(&said_now) {
        return;
    }
    tracing::debug!(
        name,
        shows = %said_now,
        focus = ?ui.focus.map(Target::id),
        "the session's popover shows it"
    );
    ui.said = Some(said_now);
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Click(target) => {
            ui(|ui| ui.focus = None);
            act(target);
        }
        Msg::Act(target) => act(target),
        Msg::Cancel => {
            tracing::debug!("Escape: the confirmation is cancelled");
            super::tell(ToCompositor::Leave(LeaveAsk::Cancel));
        }
    }
}

/// `target` clicked, or `Enter` on it.
fn act(target: Target) {
    tracing::info!(target = target.id(), "the session menu acts");
    let bar = super::with(|c| c.open_on().map(|(bar, _)| bar.to_owned()));
    match target {
        Target::Row(Row::Settings) => open_settings(None),
        Target::Row(Row::Lock) => {
            super::send(vec![Effect::Tell(ToCompositor::Lock(LockAsk::Now))]);
            event(Seen::Chosen);
        }
        Target::Row(Row::Suspend) => {
            super::send(vec![Effect::Tell(ToCompositor::Leave(LeaveAsk::Suspend))]);
            event(Seen::Chosen);
        }
        Target::Row(Row::Leave(action)) => {
            let Some(output) = bar else {
                return;
            };
            // It is worked in now: it stays when the pointer leaves.
            event(Seen::Hold);
            super::tell(ToCompositor::Leave(LeaveAsk::Ask { action, output }));
        }
        Target::Now | Target::Anyway => {
            super::tell(ToCompositor::Leave(LeaveAsk::Now));
        }
        Target::Cancel => {
            super::tell(ToCompositor::Leave(LeaveAsk::Cancel));
        }
        Target::Back(i) | Target::Force(i) => {
            let holder = store(|s| match s.shown.as_ref().map(|(s, _)| &s.step) {
                Some(LeaveStep::Held(holders)) => holders.get(i).cloned(),
                _ => None,
            });
            let Some(holder) = holder else {
                return;
            };
            match (target, holder.kind) {
                (Target::Force(_), HolderKind::Window { id }) => {
                    super::tell(ToCompositor::Leave(LeaveAsk::Force(id)));
                }
                (_, HolderKind::Window { id }) => {
                    super::tell(ToCompositor::Bring(id));
                    event(Seen::Chosen);
                }
                (_, HolderKind::Inhibitor { pid }) => {
                    super::tell(ToCompositor::Raise {
                        pid,
                        app: if holder.app.is_empty() {
                            "-".to_owned()
                        } else {
                            holder.app.replace(' ', "_")
                        },
                    });
                    event(Seen::Chosen);
                }
            }
        }
    }
}

/// The Settings open (or come forward) on `section` if given, the popover
/// closes: the program in `PATH`, else beside the shell's. The compositor
/// starts it with an activation token, which brings an open window
/// forward.
pub(crate) fn open_settings(section: Option<&str>) {
    open_settings_with(section.map(str::to_owned).into_iter().collect());
}

/// The Settings opened with `args` (`bluetooth --pair ADDRESS`).
pub(crate) fn open_settings_with(args: Vec<String>) {
    let program = settings_program();
    match args.first() {
        None => tracing::info!(program, "the Settings open from the footer"),
        Some(section) => tracing::info!(program, section, "the Settings open from a popover"),
    }
    launch_settings(program, args);
}

/// The Settings on Réseau, with that network's sheet (6.6): from the
/// Wi-Fi popover's "dans les Réglages" rows.
pub(in crate::popover) fn open_settings_join(ssid: &str) {
    let program = settings_program();
    tracing::info!(program, "the Settings open on a network's sheet");
    launch_settings(
        program,
        vec!["network".to_owned(), "--join".to_owned(), ssid.to_owned()],
    );
}

fn launch_settings(program: String, args: Vec<String>) {
    let mut argv = vec![program];
    argv.extend(args);
    let launch = Launch {
        id: "hocus-settings".to_owned(),
        terminal: false,
        dir: String::new(),
        argv,
    };
    super::send(vec![Effect::Tell(ToCompositor::Launch(launch))]);
    event(Seen::Chosen);
}

/// The Settings' program: `hocus-settings` in `PATH`, else beside the
/// shell's own (a build not installed).
fn settings_program() -> String {
    const NAME: &str = "hocus-settings";
    let in_path = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(NAME))
            .find(|p| p.is_file())
    });
    let beside = || {
        std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join(NAME)))
            .filter(|p| p.is_file())
    };
    in_path
        .or_else(beside)
        .map_or_else(|| NAME.to_owned(), |p| p.display().to_string())
}

#[cfg(test)]
mod tests {
    use hocus_link::Holder;

    use super::*;

    #[test]
    fn a_holder_is_named_by_its_application_and_says_why() {
        let words = Words::french();
        let window = named(
            &Holder {
                kind: HolderKind::Window { id: 3 },
                app: "org.example.Notes".into(),
                text: "Courses".into(),
            },
            words,
        );
        assert_eq!(window.name, "Notes");
        assert_eq!(window.reason, "« Courses » ne s’est pas fermée.");
        let inhibitor = named(
            &Holder {
                kind: HolderKind::Inhibitor { pid: 1 },
                app: String::new(),
                text: "Gravure en cours".into(),
            },
            words,
        );
        assert_eq!(inhibitor.name, "Une application");
        assert_eq!(inhibitor.reason, "Gravure en cours");
    }
}
