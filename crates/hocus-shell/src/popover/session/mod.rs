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

mod layout;
mod words;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    HolderKind, LEAVE_COUNTDOWN, Launch, LeaveAction, LeaveAsk, LeaveCan, LeaveShown, LeaveStep,
    LockAsk, POPOVER_WIDTH, ToCompositor,
};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Stroke},
        text::Alignment,
    },
    window,
};

use self::layout::{Layout, Named, Placed, Row, Target, View};
pub use self::words::Words;
use super::{
    Effect, Event as Seen, Item, Pressed, event,
    kit::{self, Input, Pointer, text, text_in},
};
use crate::levels::draw::{DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT, ring};

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();

const ROW_RADIUS: f32 = 8.0;
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
const ROW_TEXT: f32 = 14.0;
const ROW_PAD: f32 = 6.0;
const TILE_RADIUS: f32 = 8.0;
const TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const TILE_GAP: f32 = 10.0;
const GLYPH: f32 = 17.0;
const GLYPH_INK: &str = "#C9D0DC";
const SHORTCUT: f32 = 11.0;
const MARTIAN: iced::Font = iced::Font::with_name("Martian Mono");
const GREYED: Color = Color::from_rgba8(255, 255, 255, 0.28);
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);
const NAME_SIZE: f32 = 16.0;
const AMBER: Color = crate::levels::draw::rgb(0xF5C46E);
const AMBER_TILE: Color = Color::from_rgba8(245, 196, 110, 0.14);
const AMBER_INK: Color = crate::levels::draw::rgb(0x1C1406);
const ANYWAY_FILL: Color = Color::from_rgba8(245, 196, 110, 0.12);
const ANYWAY_BORDER: Color = Color::from_rgba8(245, 196, 110, 0.45);
const TRACK: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BUTTON_RADIUS: f32 = 9.0;
const BUTTON_FILL: Color = Color::from_rgba8(255, 255, 255, 0.06);
const BUTTON_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.10);
const BUTTON_TEXT: f32 = 13.5;
const BLOCK_RADIUS: f32 = 10.0;
const BLOCK_FILL: Color = Color::from_rgba8(255, 255, 255, 0.05);
const SMALL_RADIUS: f32 = 7.0;
const SMALL_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
const SMALL_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.12);
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

/// The popover opens: what logind allows is read again.
pub(super) fn opening() {
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

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// The name, as cut to the header.
    name: Option<String>,
    pointer: Pointer<Target>,
    /// What holds the keyboard.
    focused: Option<Target>,
    layout: Option<(Layout, u64)>,
    /// What was said last of what it shows, for the log.
    said: Option<String>,
}

impl State {
    fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref().map(|(l, _)| l)
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
            Input::Redraw(now) => {
                self.refresh(state);
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
                }
                // The bar drains, a frame at a time, on a GPU with motion.
                let counting = store(|s| {
                    s.shown
                        .as_ref()
                        .is_some_and(|(s, _)| matches!(s.step, LeaveStep::Counting { .. }))
                });
                (self.motion && counting).then(|| canvas::Action::request_redraw_at(now + FRAME))
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
        let f = &mut frame;
        self.header(f, state);
        let Some(layout) = state.layout() else {
            return vec![frame.into_geometry()];
        };
        let hover = state.pointer.hover;
        let focus = state.focused;
        match &layout.view {
            View::Rows { rows, dividers } => {
                for y in dividers {
                    f.fill_rectangle(
                        Point::new(kit::LEFT, *y),
                        Size::new(kit::RIGHT - kit::LEFT, 1.0),
                        RULE,
                    );
                }
                for placed in rows {
                    self.row(f, placed, hover, focus);
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
                let step = store(|s| s.shown.clone());
                let Some((shown, told)) = step else {
                    return vec![frame.into_geometry()];
                };
                title(f, *action, *top, &[self.words.title(*action).to_owned()]);
                let (said, left) = match &shown.step {
                    LeaveStep::Counting { left_ms } => {
                        // Told each whole second: between two, a GPU with
                        // motion drains the bar on its own clock, a second
                        // at most; else it shows what was told.
                        let said = Duration::from_millis(u64::from(*left_ms));
                        let left = if self.motion {
                            said.saturating_sub(told.elapsed().min(Duration::from_secs(1)))
                        } else {
                            said
                        };
                        let seconds = left.as_millis().div_ceil(1000) as u32;
                        (self.words.counting(*action, seconds), Some(left))
                    }
                    LeaveStep::Authorizing => (self.words.authorizing().to_owned(), None),
                    LeaveStep::Closing => (self.words.closing().to_owned(), Some(Duration::ZERO)),
                    LeaveStep::Acting => (self.words.acting(*action).to_owned(), None),
                    LeaveStep::Held(_) => (String::new(), None),
                };
                f.fill_text(text(
                    said,
                    Point::new(kit::LEFT, *sentence),
                    layout::SENTENCE,
                    DIM,
                    Alignment::Left,
                ));
                if let Some(bar) = bar {
                    f.fill(&kit::rounded(*bar, 2.0), TRACK);
                    let left = left.unwrap_or(LEAVE_COUNTDOWN);
                    // Smooth on a GPU with motion; else a step a second.
                    let left = if self.motion {
                        left.as_secs_f32()
                    } else {
                        left.as_secs_f32().ceil()
                    };
                    let part = (left / LEAVE_COUNTDOWN.as_secs_f32()).clamp(0.0, 1.0);
                    if part > 0.0 {
                        let mut drained = *bar;
                        drained.width *= part;
                        f.fill(&kit::rounded(drained, 2.0), AMBER);
                    }
                }
                if let Some(now) = now {
                    button(
                        f,
                        *now,
                        self.words.now(*action),
                        Button::Amber,
                        hover == Some(Target::Now),
                        focus == Some(Target::Now),
                    );
                }
                if let Some(cancel) = cancel {
                    button(
                        f,
                        *cancel,
                        self.words.cancel(),
                        Button::Plain,
                        hover == Some(Target::Cancel),
                        focus == Some(Target::Cancel),
                    );
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
                title(f, *action, *top, lines);
                for (i, line) in sentence.iter().enumerate() {
                    let line_height = layout::HELD_SENTENCE * layout::LINE;
                    f.fill_text(text(
                        line.clone(),
                        Point::new(kit::LEFT, sentence_top + line_height * (i as f32 + 0.5)),
                        layout::HELD_SENTENCE,
                        DIM,
                        Alignment::Left,
                    ));
                }
                let holders = store(|s| match s.shown.as_ref().map(|(s, _)| &s.step) {
                    Some(LeaveStep::Held(holders)) => holders.clone(),
                    _ => Vec::new(),
                });
                for (i, block) in blocks.iter().enumerate() {
                    let app = holders.get(i).map_or("", |h| h.app.as_str());
                    self.block(f, block, app, i, hover, focus);
                }
                button(
                    f,
                    *anyway,
                    self.words.anyway(*action),
                    Button::Anyway,
                    hover == Some(Target::Anyway),
                    focus == Some(Target::Anyway),
                );
                button(
                    f,
                    *cancel,
                    self.words.cancel(),
                    Button::Plain,
                    hover == Some(Target::Cancel),
                    focus == Some(Target::Cancel),
                );
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

/// The confirmation's title row at `top`: the action's glyph in amber on
/// its tile, the title (on several lines when it must).
fn title(f: &mut Frame, action: LeaveAction, top: f32, lines: &[String]) {
    let tile = Rectangle::new(
        Point::new(kit::LEFT, top),
        Size::new(layout::TILE, layout::TILE),
    );
    f.fill(&kit::rounded(tile, TILE_RADIUS), AMBER_TILE);
    draw_glyph(f, tile, Row::Leave(action), true);
    let line = layout::TITLE * layout::LINE;
    let block = (lines.len() as f32 * line).max(layout::TILE);
    let first = top + (block - lines.len() as f32 * line) / 2.0;
    for (i, said) in lines.iter().enumerate() {
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                said.clone(),
                Point::new(
                    tile.x + layout::TILE + TILE_GAP,
                    first + line * (i as f32 + 0.5),
                ),
                layout::TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
    }
}

fn draw_glyph(f: &mut Frame, tile: Rectangle, row: Row, amber: bool) {
    let centre = tile.center();
    f.draw_svg(
        Rectangle::new(
            Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
            Size::new(GLYPH, GLYPH),
        ),
        svg::Svg::new(glyph(row, amber)),
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Button {
    /// "… maintenant": amber.
    Amber,
    /// "Annuler".
    Plain,
    /// "… quand même": amber's border on its tint.
    Anyway,
}

/// A full-width button at `rect`.
fn button(f: &mut Frame, rect: Rectangle, word: &str, look: Button, hover: bool, focus: bool) {
    let shape = kit::rounded(rect, BUTTON_RADIUS);
    let (ink, font) = match look {
        Button::Amber => {
            let fill = if hover { AMBER.scale_alpha(0.9) } else { AMBER };
            f.fill(&shape, fill);
            (AMBER_INK, MANROPE_BOLD)
        }
        Button::Plain => {
            f.fill(&shape, if hover { BUTTON_HOVER } else { BUTTON_FILL });
            (TEXT, kit::MANROPE_SEMIBOLD)
        }
        Button::Anyway => {
            f.fill(&shape, if hover { AMBER_TILE } else { ANYWAY_FILL });
            f.stroke(
                &kit::rounded(
                    Rectangle::new(
                        Point::new(rect.x + 0.5, rect.y + 0.5),
                        Size::new(rect.width - 1.0, rect.height - 1.0),
                    ),
                    BUTTON_RADIUS - 0.5,
                ),
                Stroke::default().with_color(ANYWAY_BORDER).with_width(1.0),
            );
            (AMBER, MANROPE_BOLD)
        }
    };
    if focus {
        ring(f, rect, BUTTON_RADIUS);
    }
    f.fill_text(text_in(
        font,
        text(
            word.to_owned(),
            rect.center(),
            BUTTON_TEXT,
            ink,
            Alignment::Center,
        ),
    ));
}

/// A small button of a block.
fn small(f: &mut Frame, rect: Rectangle, word: &str, hover: bool, focus: bool) {
    f.fill(
        &kit::rounded(rect, SMALL_RADIUS),
        if hover { SMALL_HOVER } else { SMALL_FILL },
    );
    if focus {
        ring(f, rect, SMALL_RADIUS);
    }
    f.fill_text(text_in(
        kit::MANROPE_SEMIBOLD,
        text(
            word.to_owned(),
            rect.center(),
            layout::REASON,
            TEXT,
            Alignment::Center,
        ),
    ));
}

impl Panel {
    /// The header: the avatar, the name.
    fn header(&self, f: &mut Frame, state: &State) {
        let middle = kit::HEADER_TOP + layout::HEADER / 2.0;
        let avatar = Rectangle::new(
            Point::new(kit::LEFT, middle - layout::AVATAR / 2.0),
            Size::new(layout::AVATAR, layout::AVATAR),
        );
        crate::lock::draw::avatar_round(f, avatar, 1.0);
        if let Some(name) = &state.name {
            f.fill_text(text_in(
                MANROPE_BOLD,
                text(
                    name.clone(),
                    Point::new(avatar.x + layout::AVATAR + TILE_GAP, middle),
                    NAME_SIZE,
                    TEXT,
                    Alignment::Left,
                ),
            ));
        }
    }

    /// A row of the menu.
    fn row(&self, f: &mut Frame, placed: &Placed, hover: Option<Target>, focus: Option<Target>) {
        let rect = placed.rect;
        let target = Some(Target::Row(placed.row));
        if placed.enabled && hover == target {
            f.fill(&kit::rounded(rect, ROW_RADIUS), ROW_HOVER);
        }
        if focus == target {
            ring(f, rect, ROW_RADIUS);
        }
        let tile = Rectangle::new(
            Point::new(rect.x + ROW_PAD, rect.center_y() - layout::TILE / 2.0),
            Size::new(layout::TILE, layout::TILE),
        );
        let alpha = if placed.enabled { 1.0 } else { 0.4 };
        f.fill(
            &kit::rounded(tile, TILE_RADIUS),
            TILE_FILL.scale_alpha(alpha),
        );
        if placed.enabled {
            draw_glyph(f, tile, placed.row, false);
        } else {
            f.with_save(|f| {
                // The greyed rows' glyph, faded as their words.
                let centre = tile.center();
                f.draw_svg(
                    Rectangle::new(
                        Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
                        Size::new(GLYPH, GLYPH),
                    ),
                    svg::Svg::new(glyph(placed.row, false)).opacity(0.35_f32),
                );
            });
        }
        f.fill_text(text_in(
            kit::MANROPE_SEMIBOLD,
            text(
                self.row_word(placed.row).to_owned(),
                Point::new(tile.x + layout::TILE + TILE_GAP, rect.center_y()),
                ROW_TEXT,
                if placed.enabled { TEXT } else { GREYED },
                Alignment::Left,
            ),
        ));
        if placed.row == Row::Lock {
            f.fill_text(text_in(
                MARTIAN,
                text(
                    "Super+L".to_owned(),
                    Point::new(rect.x + rect.width - ROW_PAD, rect.center_y()),
                    SHORTCUT,
                    FAINT,
                    Alignment::Right,
                ),
            ));
        }
    }

    fn row_word(&self, row: Row) -> &'static str {
        match row {
            Row::Settings => self.words.settings(),
            Row::Lock => self.words.lock(),
            Row::Suspend => self.words.suspend(),
            Row::Leave(action) => self.words.row(action),
        }
    }

    /// Block `i` of the list: application `app`'s.
    fn block(
        &self,
        f: &mut Frame,
        block: &layout::Block,
        app: &str,
        i: usize,
        hover: Option<Target>,
        focus: Option<Target>,
    ) {
        f.fill(&kit::rounded(block.rect, BLOCK_RADIUS), BLOCK_FILL);
        self.icon(f, block.icon, app, &block.name);
        let words_left = block.back.x;
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                block.name.clone(),
                Point::new(words_left, block.name_middle),
                layout::NAME,
                TEXT,
                Alignment::Left,
            ),
        ));
        let line = layout::REASON * layout::LINE;
        for (n, said) in block.reason.iter().enumerate() {
            f.fill_text(text(
                said.clone(),
                Point::new(words_left, block.reason_top + line * (n as f32 + 0.5)),
                layout::REASON,
                DIM,
                Alignment::Left,
            ));
        }
        small(
            f,
            block.back,
            &block.back_word,
            hover == Some(Target::Back(i)),
            focus == Some(Target::Back(i)),
        );
        if let Some(force) = block.force {
            small(
                f,
                force,
                self.words.close_anyway(),
                hover == Some(Target::Force(i)),
                focus == Some(Target::Force(i)),
            );
        }
    }

    /// An application's icon: its entry's, else its letter on a colour of
    /// its own (3.4b).
    fn icon(&self, f: &mut Frame, rect: Rectangle, app: &str, name: &str) {
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
        match handle {
            Some(handle) => f.draw_image(rect, canvas::Image::new(handle)),
            None => kit::app_tile(f, rect, (None, name), (ICON_RADIUS, 14.0), 1.0),
        }
    }

    /// What it shows, read afresh while it is open: the name cut to the
    /// header, the layout of what the compositor said.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let name = crate::user::name();
        let room = kit::RIGHT - (kit::LEFT + layout::AVATAR + TILE_GAP);
        let name = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => kit::elide(&name, room, |t| {
                kit::measure_in(system.raw(), t, NAME_SIZE, MANROPE_BOLD)
            }),
            Err(_) => name,
        };
        let generation = store(|s| s.generation);
        if state.name.as_ref() == Some(&name)
            && state.layout.as_ref().is_some_and(|(_, g)| *g == generation)
        {
            return;
        }
        let kind = |view: &View| std::mem::discriminant(view);
        let was = state.layout().map(|l| kind(&l.view));
        let (layout, generation) = lay_out(self.words);
        // Into a confirmation, or the list: the keyboard on "Annuler";
        // back to the rows, on none. What held it and is gone lets it go.
        if was != Some(kind(&layout.view))
            || state.focused.is_some_and(|t| !layout.order().contains(&t))
        {
            state.focused = layout.first_focus();
        }
        super::refit(Item::User);
        self.say(state, &name, &layout);
        state.name = Some(name);
        state.layout = Some((layout, generation));
    }

    /// What it shows, said in the log once (the tests read it).
    fn say(&self, state: &mut State, name: &str, layout: &Layout) {
        let said = match &layout.view {
            View::Rows { rows, .. } => {
                let rows: Vec<String> = rows
                    .iter()
                    .map(|p| {
                        let word = self.row_word(p.row);
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
                        self.words.counting(*action, left_ms.div_ceil(1000))
                    }
                    Some(LeaveStep::Authorizing) => self.words.authorizing().to_owned(),
                    Some(LeaveStep::Closing) => self.words.closing().to_owned(),
                    Some(LeaveStep::Acting) => self.words.acting(*action).to_owned(),
                    _ => String::new(),
                };
                format!("{} « {sentence} »", self.words.title(*action))
            }
            View::Held { title, blocks, .. } => {
                let names: Vec<String> = blocks
                    .iter()
                    .map(|b| format!("{}: {}", b.name, b.reason.join(" ")))
                    .collect();
                format!("{} {names:?}", title.join(" "))
            }
        };
        if state.said.as_ref() == Some(&said) {
            return;
        }
        tracing::debug!(
            name,
            shows = %said,
            focus = ?state.focused,
            "the session's popover shows it"
        );
        state.said = Some(said);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| state.layout().and_then(|l| l.at(at)));
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
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    state.focused = None;
                    self.act(target);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named as K};
        let order = state.layout().map(Layout::order).unwrap_or_default();
        match &pressed.key {
            // A confirmation is cancelled; else the popover closes.
            Key::Named(K::Escape) => {
                if confirming() {
                    tracing::debug!("Escape: the confirmation is cancelled");
                    super::tell(ToCompositor::Leave(LeaveAsk::Cancel));
                } else {
                    event(Seen::Escape);
                    return Some(canvas::Action::capture());
                }
            }
            Key::Named(K::Tab | K::ArrowDown | K::ArrowUp) => {
                let back = matches!(&pressed.key, Key::Named(K::ArrowUp))
                    || (matches!(&pressed.key, Key::Named(K::Tab)) && pressed.modifiers.shift());
                state.focused = kit::step(&order, state.focused, back);
                tracing::debug!(focus = ?state.focused, "the keyboard in the session menu");
            }
            Key::Named(K::Enter | K::Space) => self.act(state.focused?),
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// `target` clicked, or `Enter` on it.
    fn act(&self, target: Target) {
        tracing::info!(?target, "the session menu acts");
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
}

/// The Settings open (or come forward) on `section` if given, the popover
/// closes: the program in `PATH`, else beside the shell's. The compositor
/// starts it with an activation token, which brings an open window
/// forward.
pub(in crate::popover) fn open_settings(section: Option<&str>) {
    let program = settings_program();
    tracing::info!(program, section, "the Settings open from the bar");
    let mut argv = vec![program];
    argv.extend(section.map(str::to_owned));
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
