//! Her peek (docs/design/secretary.md §6.2, the s11 mockup's variant B):
//! once the gate lets her out and the compositor says so
//! (`secretary-peek ID`), her drawing at 200 px beside each bar, at her
//! row's height, in her `nudge` mood (her ear rings, her clipboard's dot
//! pulses), and her bubble on its left, 300 px: "TA SECRÉTAIRE", what she
//! says (the key words bold), and at most three buttons: the action,
//! "Plus tard ▾", "Fait". The compositor moves the surfaces as an agent's
//! panda's ([`hocus_link::PEEK_NAMESPACE`]) and says when she is back
//! (`no-peek`): the surfaces then go.
//!
//! - "Fait" marks it done; "Plus tard ▾" unfolds its choices in the
//!   bubble (and she stays her time again); a choice tells her when, and
//!   she goes back. A click on the bubble, or the action, opens her
//!   drawer on the item.
//! - On the CPU the surface has no alpha: the bubble's card holds her
//!   drawing too.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{EDGE, Form, PEEK_NAMESPACE, ToCompositor};
use hocus_secretary_link::{Later, PeekAnswer, Speak, SpeakKind};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path, Stroke},
        text::Alignment,
    },
    window,
};

use crate::{
    levels::draw::{MANROPE, MANROPE_BOLD, pointer},
    pandas,
    popover::{self, Effect, kit},
};

// --- The mockup's sizes, at ×1 ------------------------------------------------------

const PANDA: f32 = 200.0;
const GAP: f32 = 8.0;
const FROM_BAR: f32 = 18.0;
const BUBBLE: f32 = 300.0;
const PAD_X: f32 = 13.0;
const PAD_Y: f32 = 11.0;
const LABEL: f32 = 10.5;
const TEXT: f32 = 13.5;
const LINE: f32 = 1.45;
const BUTTON: f32 = 12.5;
const BUTTON_X: f32 = 11.0;
const BUTTON_Y: f32 = 5.0;
const MENU_ROW: f32 = 26.0;
const RADIUS: f32 = 12.0;
/// Room around the bubble and her for their shadows: left, top, right,
/// bottom.
const ROOM: (f32, f32, f32, f32) = (45.0, 14.0, 24.0, 30.0);
/// How long her `nudge` plays from her first frame.
const PLAYS: Duration = Duration::from_millis(2600);

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// `rgba(16,24,44,.95)`, a blue rim; amber for something due.
const FILL: Color = rgba(16, 24, 44, 0.95);
const FILL_HOVER: Color = rgba(22, 32, 58, 0.97);
const RIM: Color = rgba(143, 179, 255, 0.45);
const RIM_DUE: Color = rgba(245, 180, 74, 0.5);
const WHO: Color = rgba(185, 206, 255, 1.0);
const WHO_DUE: Color = rgba(245, 196, 110, 1.0);
const INK: Color = rgba(238, 241, 246, 1.0);
const DIM: Color = rgba(169, 178, 194, 1.0);
const GREEN: Color = rgba(108, 194, 135, 1.0);
const GREEN_HOVER: Color = rgba(126, 209, 152, 1.0);
const DARK: Color = rgba(11, 16, 32, 1.0);

/// What can be clicked on her peek.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// The bubble, or the action: the drawer opens on the item.
    Open,
    Later,
    Choice(Later),
    Done,
}

impl Target {
    fn name(self) -> String {
        match self {
            Self::Open => "Open".to_owned(),
            Self::Later => "Later".to_owned(),
            Self::Choice(later) => format!("Choice({later:?})"),
            Self::Done => "Done".to_owned(),
        }
    }
}

/// What her bubble says, laid out: its pieces of text (their words, place
/// on the bubble, boldness), its buttons, its menu.
#[derive(Debug, Clone, PartialEq)]
pub struct Said {
    pub who: String,
    pub pieces: Vec<(String, Point, bool)>,
    pub buttons: Vec<(String, Rectangle, Target, bool)>,
    pub menu: Vec<(String, Rectangle, Target)>,
    pub height: f32,
}

/// `text`'s runs, `**` around the bold ones.
fn runs(text: &str) -> Vec<(String, bool)> {
    text.split("**")
        .enumerate()
        .filter(|(_, t)| !t.is_empty())
        .map(|(n, t)| (t.to_owned(), n % 2 == 1))
        .collect()
}

fn font(bold: bool) -> iced::Font {
    if bold { MANROPE_BOLD } else { MANROPE }
}

/// Her bubble for `speak`, its menu unfolded or not, `fr` or not,
/// `measure` giving a text's width at a size in a font.
pub fn said(
    speak: &Speak,
    menu: bool,
    fr: bool,
    mut measure: impl FnMut(&str, f32, iced::Font) -> f32,
) -> Said {
    let who = if fr {
        "TA SECRÉTAIRE"
    } else {
        "YOUR SECRETARY"
    }
    .to_owned();
    let room = BUBBLE - 2.0 * PAD_X;
    let mut y = PAD_Y + LABEL * 1.4 + 4.0;
    let line = TEXT * LINE;
    let mut pieces = Vec::new();
    let mut x = 0.0;
    let space = measure(" ", TEXT, MANROPE);
    let mut lines = 1;
    for (text, bold) in runs(&speak.text) {
        for (n, word) in text.split(' ').enumerate() {
            if word.is_empty() {
                if n > 0 {
                    x += space;
                }
                continue;
            }
            let lead = if n > 0 && x > 0.0 { space } else { 0.0 };
            let w = measure(word, TEXT, font(bold));
            if x > 0.0 && x + lead + w > room {
                x = 0.0;
                lines += 1;
            } else {
                x += lead;
            }
            let top = y + (lines - 1) as f32 * line;
            pieces.push((
                word.to_owned(),
                Point::new(PAD_X + x, top + line / 2.0),
                bold,
            ));
            x += w;
        }
    }
    y += lines as f32 * line + 10.0;
    let height = BUTTON * 1.4 + 2.0 * BUTTON_Y;
    let mut buttons = Vec::new();
    let mut bx = PAD_X;
    let mut labels: Vec<(String, Target, bool)> = Vec::new();
    if let Some(action) = &speak.action {
        labels.push((action.clone(), Target::Open, true));
    }
    labels.push((
        if fr { "Plus tard ▾" } else { "Later ▾" }.to_owned(),
        Target::Later,
        false,
    ));
    labels.push((
        if fr { "Fait" } else { "Done" }.to_owned(),
        Target::Done,
        false,
    ));
    for (label, target, primary) in labels {
        let w = measure(&label, BUTTON, MANROPE_BOLD) + 2.0 * BUTTON_X;
        if bx > PAD_X && bx + w > BUBBLE - PAD_X {
            bx = PAD_X;
            y += height + 6.0;
        }
        buttons.push((
            label,
            Rectangle::new(Point::new(bx, y), Size::new(w, height)),
            target,
            primary,
        ));
        bx += w + 6.0;
    }
    y += height;
    let mut choices = Vec::new();
    if menu {
        y += 6.0;
        for later in Later::ALL {
            choices.push((
                later.words(fr).to_owned(),
                Rectangle::new(Point::new(PAD_X - 4.0, y), Size::new(room + 8.0, MENU_ROW)),
                Target::Choice(later),
            ));
            y += MENU_ROW;
        }
    }
    Said {
        who,
        pieces,
        buttons,
        menu: choices,
        height: y + PAD_Y,
    }
}

/// Where everything is on her surface, and where it is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placed {
    pub size: (u32, u32),
    pub margin: (i32, i32, i32, i32),
    pub bubble: Rectangle,
    pub panda: Rectangle,
}

/// Her peek beside a bar in `form`, her middle `middle` under the bar's
/// top, her bubble `height` tall; `card`: on the CPU.
fn placed(form: Form, middle: f32, height: f32, card: bool) -> Placed {
    let (left, top, right, bottom) = if card { (0.0, 0.0, 8.0, 0.0) } else { ROOM };
    let tall = PANDA.max(height);
    let bubble_top = if height < PANDA {
        top + (PANDA - height) / 2.0
    } else {
        top
    };
    let bubble = Rectangle::new(Point::new(left, bubble_top), Size::new(BUBBLE, height));
    let panda = Rectangle::new(
        Point::new(left + BUBBLE + GAP, top + (tall - PANDA) / 2.0),
        Size::new(PANDA, PANDA),
    );
    let size = (
        (panda.x + PANDA + right).ceil() as u32,
        (top + tall + bottom).ceil() as u32,
    );
    let panda_top = (middle - tall / 2.0).max(0.0);
    let bar = form.width() as f32;
    let margin = (
        (EDGE as f32 + panda_top - top).round() as i32,
        (EDGE as f32 + bar + FROM_BAR - right).round() as i32,
        0,
        0,
    );
    let placed = Placed {
        size,
        margin,
        bubble,
        panda,
    };
    if crate::side::left() {
        let width = size.0 as f32;
        let flip = |r: Rectangle| Rectangle::new(Point::new(width - r.x - r.width, r.y), r.size());
        Placed {
            bubble: flip(placed.bubble),
            panda: flip(placed.panda),
            ..placed
        }
    } else {
        placed
    }
}

// --- Her peek out, and its surfaces --------------------------------------------------

#[derive(Debug, Clone)]
struct Out {
    speak: Speak,
    said: Said,
    menu: bool,
    surfaces: HashMap<String, (window::Id, Placed)>,
    /// Her mood plays from her first frame.
    since: Option<Instant>,
}

static OUT: Mutex<Option<Out>> = Mutex::new(None);
/// What the gate let out, until the compositor brings it out.
static READY: Mutex<Option<Speak>> = Mutex::new(None);

fn out<T>(with: impl FnOnce(&mut Option<Out>) -> T) -> T {
    with(&mut OUT.lock().unwrap_or_else(PoisonError::into_inner))
}

fn fr() -> bool {
    crate::clock::time::Lang::of_messages() == crate::clock::time::Lang::French
}

fn measure(text: &str, size: f32, font: iced::Font) -> f32 {
    kit::measure(text, size, font)
}

/// The gate let `speak` out: the compositor says when it comes.
pub fn ready(speak: Speak) {
    *READY.lock().unwrap_or_else(PoisonError::into_inner) = Some(speak);
}

/// The compositor brings her peek of `id` out: beside each bar, in place
/// of the panda out.
pub fn show(id: u64) {
    let speak = READY
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take_if(|s| s.id == id);
    let Some(speak) = speak.or_else(|| out(|o| o.as_ref().map(|o| o.speak.clone()))) else {
        tracing::info!(id, "her peek, not known: nothing shown");
        return;
    };
    crate::agents::peek::back();
    let said = said(&speak, false, fr(), measure);
    lay(speak, said, false);
}

/// Her peek laid out beside each bar.
fn lay(speak: Speak, said: Said, menu: bool) {
    let bars = popover::with(|c| c.bar_forms());
    let card = crate::agents::peek::software();
    let mut effects = Vec::new();
    out(|out| {
        let since = out.as_ref().and_then(|o| o.since);
        let mut before = out.take().map(|o| o.surfaces).unwrap_or_default();
        let mut surfaces = HashMap::new();
        for (bar, form) in bars {
            let middle = crate::agents::peek::middle_of(&bar, form);
            let placed = placed(form, middle, said.height, card);
            let id = match before.remove(&bar) {
                Some((id, _)) => {
                    effects.push(Effect::Resize {
                        id,
                        size: placed.size,
                        margin: Some(placed.margin),
                    });
                    id
                }
                None => {
                    let id = window::Id::unique();
                    effects.push(Effect::Layer {
                        id,
                        bar: bar.clone(),
                        namespace: PEEK_NAMESPACE,
                        size: placed.size,
                        margin: placed.margin,
                    });
                    id
                }
            };
            effects.push(Effect::Region {
                id,
                rects: regions(&placed, card),
            });
            surfaces.insert(bar, (id, placed));
        }
        effects.extend(before.into_values().map(|(id, _)| Effect::Remove(id)));
        let words: Vec<&str> = said.pieces.iter().map(|p| p.0.as_str()).collect();
        let buttons: Vec<&str> = said.buttons.iter().map(|b| b.0.as_str()).collect();
        let places: Vec<String> = said
            .buttons
            .iter()
            .map(|(_, r, t, _)| (t, r))
            .chain(said.menu.iter().map(|(_, r, t)| (t, r)))
            .filter_map(|(t, r)| {
                let (_, placed) = surfaces.values().next()?;
                Some(format!(
                    "{}@{:.0},{:.0}",
                    t.name(),
                    placed.bubble.x + r.center_x(),
                    placed.bubble.y + r.center_y()
                ))
            })
            .collect();
        tracing::info!(
            id = speak.id,
            kind = speak.kind.word(),
            said = %words.join(" "),
            buttons = %buttons.join(" | "),
            menu = menu,
            bars = surfaces.len(),
            "her peek shows"
        );
        tracing::debug!("her peek's items {}", places.join(" "));
        *out = Some(Out {
            speak,
            said,
            menu,
            surfaces,
            since,
        });
    });
    effects.push(Effect::Wake(PEEK_NAMESPACE));
    popover::send(effects);
}

/// Where the surface takes the pointer: the bubble and her (the whole
/// card on the CPU).
fn regions(placed: &Placed, card: bool) -> Vec<Rectangle> {
    if card {
        let (w, h) = placed.size;
        return vec![Rectangle::new(Point::ORIGIN, Size::new(w as f32, h as f32))];
    }
    vec![placed.bubble, placed.panda]
}

/// The compositor says the peek is back: hers goes, and the gate is told.
pub fn back() {
    let gone = out(Option::take);
    if let Some(gone) = gone {
        tracing::debug!(id = gone.speak.id, "her peek is back");
        crate::speak::back(gone.speak.id);
        popover::send(
            gone.surfaces
                .into_values()
                .map(|(id, _)| Effect::Remove(id))
                .collect(),
        );
    }
}

/// She went, or classic again: her peek goes, untold.
pub fn forget_all() {
    READY.lock().unwrap_or_else(PoisonError::into_inner).take();
    let gone = out(Option::take);
    if let Some(gone) = gone {
        popover::send(
            gone.surfaces
                .into_values()
                .map(|(id, _)| Effect::Remove(id))
                .collect(),
        );
    }
}

/// Bar `bar` went: its surface too.
pub fn forget(bar: &str) {
    let gone = out(|out| out.as_mut()?.surfaces.remove(bar));
    if let Some((id, _)) = gone {
        popover::send(vec![Effect::Remove(id)]);
    }
}

/// A bar changed form, or the agents' group moved: her peek follows.
pub fn moved() {
    let again = out(|out| {
        out.as_ref()
            .map(|o| (o.speak.clone(), o.said.clone(), o.menu))
    });
    if let Some((speak, said, menu)) = again {
        lay(speak, said, menu);
    }
}

/// Her peek's surface `id`, if it is one, and its bar.
pub fn bar_of(id: window::Id) -> Option<String> {
    out(|out| {
        out.as_ref()?
            .surfaces
            .iter()
            .find(|(_, (s, _))| *s == id)
            .map(|(bar, _)| bar.clone())
    })
}

/// `target` clicked on the surface beside `bar`.
fn act(bar: &str, target: Target) {
    let Some((speak, menu)) = out(|o| o.as_ref().map(|o| (o.speak.clone(), o.menu))) else {
        return;
    };
    tracing::info!(
        id = speak.id,
        target = target.name(),
        "her peek is answered"
    );
    match target {
        Target::Later => {
            let said = said(&speak, !menu, fr(), measure);
            let stays = 12;
            lay(speak.clone(), said, !menu);
            // Choosing takes a moment: she stays her time again.
            popover::send(vec![Effect::Tell(ToCompositor::SecretaryPeek {
                id: speak.id,
                stays,
            })]);
        }
        Target::Done => {
            crate::secretary::peeked(speak.id, PeekAnswer::Done);
            popover::send(vec![Effect::PeekClose]);
        }
        Target::Choice(later) => {
            crate::secretary::peeked(speak.id, PeekAnswer::Later { later });
            popover::send(vec![Effect::PeekClose]);
        }
        Target::Open => {
            crate::secretary::peeked(speak.id, PeekAnswer::Open);
            crate::drawer::open_on_item(bar, speak.item);
            popover::send(vec![Effect::PeekClose]);
        }
    }
}

// --- Drawing it -------------------------------------------------------------------

/// Her peek's surface beside bar `bar`.
#[derive(Debug, Clone)]
pub struct View {
    pub bar: String,
}

#[derive(Debug, Default)]
pub struct State {
    pointer: Option<Point>,
    hover: Option<Target>,
    over: bool,
    now: Option<Instant>,
}

impl View {
    fn shown(&self) -> Option<(Speak, Said, Placed, Option<Instant>)> {
        out(|out| {
            let out = out.as_mut()?;
            let (_, placed) = out.surfaces.get(&self.bar)?;
            Some((out.speak.clone(), out.said.clone(), *placed, out.since))
        })
    }

    /// Where `at` is on her bubble: a button, a choice, the bubble.
    fn target(said: &Said, placed: &Placed, at: Point) -> Option<Target> {
        let inside = |r: &Rectangle| {
            Rectangle::new(
                Point::new(placed.bubble.x + r.x, placed.bubble.y + r.y),
                r.size(),
            )
            .contains(at)
        };
        said.buttons
            .iter()
            .find(|(_, r, _, _)| inside(r))
            .map(|(_, _, t, _)| *t)
            .or_else(|| {
                said.menu
                    .iter()
                    .find(|(_, r, _)| inside(r))
                    .map(|(_, _, t)| *t)
            })
            .or_else(|| {
                (placed.bubble.contains(at) || placed.panda.contains(at)).then_some(Target::Open)
            })
    }
}

impl<Message> canvas::Program<Message> for View {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let event = match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => {
                state.now = Some(*now);
                let since = out(|o| {
                    let o = o.as_mut()?;
                    Some(*o.since.get_or_insert(*now))
                })?;
                let plays = pandas::lively()
                    && !crate::agents::peek::software()
                    && now.saturating_duration_since(since) < PLAYS;
                return plays.then(canvas::Action::request_redraw);
            }
            iced::Event::Mouse(event) => event,
            _ => return None,
        };
        let (_, said, placed, _) = self.shown()?;
        state.pointer = pointer(event, bounds, cursor, state.pointer);
        let target = state
            .pointer
            .and_then(|at| Self::target(&said, &placed, at));
        match event {
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let target = target?;
                act(&self.bar, target);
                Some(canvas::Action::capture())
            }
            _ if target != state.hover => {
                state.hover = target;
                state.over = target.is_some();
                Some(canvas::Action::request_redraw())
            }
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
        let mut f = Frame::new(renderer, bounds.size());
        let Some((speak, said, placed, since)) = self.shown() else {
            return vec![f.into_geometry()];
        };
        let card = crate::agents::peek::software();
        let due = speak.kind == SpeakKind::Reminder;
        let fill = if state.hover == Some(Target::Open) {
            FILL_HOVER
        } else {
            FILL
        };
        let rim = if due { RIM_DUE } else { RIM };
        let size = bounds.size();
        let b = placed.bubble;
        if card {
            let fill = over(fill, Color::BLACK);
            f.fill_rectangle(Point::ORIGIN, size, fill);
            stroke(&mut f, Rectangle::new(Point::ORIGIN, size), over(rim, fill));
        } else {
            // Its shadow, then the bubble.
            f.fill(
                &kit::rounded(Rectangle::new(Point::new(b.x, b.y + 8.0), b.size()), RADIUS),
                rgba(0, 0, 0, 0.22),
            );
            f.fill(&kit::rounded(b, RADIUS), fill);
            stroke(&mut f, b, rim);
        }
        let at = |x: f32, y: f32| Point::new(b.x + x, b.y + y);
        f.fill_text(kit::text_in(
            MANROPE_BOLD,
            kit::text(
                said.who.clone(),
                at(PAD_X, PAD_Y + LABEL * 0.7),
                LABEL,
                if due { WHO_DUE } else { WHO },
                Alignment::Left,
            ),
        ));
        for (word, p, bold) in &said.pieces {
            f.fill_text(kit::text_in(
                font(*bold),
                kit::text(word.clone(), at(p.x, p.y), TEXT, INK, Alignment::Left),
            ));
        }
        for (label, r, target, primary) in &said.buttons {
            let rect = Rectangle::new(at(r.x, r.y), r.size());
            let hover = state.hover == Some(*target);
            let (fill, text) = match (primary, hover) {
                (true, false) => (GREEN, DARK),
                (true, true) => (GREEN_HOVER, DARK),
                (false, false) => (rgba(255, 255, 255, 0.07), rgba(213, 219, 229, 1.0)),
                (false, true) => (rgba(255, 255, 255, 0.13), rgba(213, 219, 229, 1.0)),
            };
            let fill = if card { over(fill, FILL) } else { fill };
            f.fill(&kit::rounded(rect, 8.0), fill);
            if !primary {
                stroke(&mut f, rect, rgba(255, 255, 255, 0.14));
            }
            f.fill_text(kit::text_in(
                MANROPE_BOLD,
                kit::text(
                    label.clone(),
                    Point::new(rect.center_x(), rect.center_y()),
                    BUTTON,
                    text,
                    Alignment::Center,
                ),
            ));
        }
        for (label, r, target) in &said.menu {
            let rect = Rectangle::new(at(r.x, r.y), r.size());
            if state.hover == Some(*target) {
                f.fill(&kit::rounded(rect, 6.0), rgba(255, 255, 255, 0.08));
            }
            let colour = if *target == Target::Choice(Later::Never) {
                DIM
            } else {
                INK
            };
            f.fill_text(kit::text(
                label.clone(),
                Point::new(rect.x + 4.0, rect.center_y()),
                TEXT - 0.5,
                colour,
                Alignment::Left,
            ));
        }
        // Her drawing, in her nudge, playing from her first frame.
        let now = state.now.unwrap_or_else(Instant::now);
        let playing = since.filter(|_| !card).map(|s| (s, PLAYS));
        let (handle, _) = pandas::secretary(hocus_pandas::SecretaryMood::Nudge, playing, now);
        f.draw_svg(placed.panda, svg::Svg::new(handle));
        vec![f.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if state.over {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}

/// A 1 px border inside `rect`, rounded as the bubble.
fn stroke(f: &mut Frame, rect: Rectangle, colour: Color) {
    let path = Path::rounded_rectangle(
        Point::new(rect.x + 0.5, rect.y + 0.5),
        Size::new(rect.width - 1.0, rect.height - 1.0),
        (RADIUS - 0.5).into(),
    );
    f.stroke(&path, Stroke::default().with_color(colour).with_width(1.0));
}

/// `colour` over `under`, opaque: the card's colours on the CPU.
fn over(colour: Color, under: Color) -> Color {
    Color::from_rgb(
        colour.r * colour.a + under.r * (1.0 - colour.a),
        colour.g * colour.a + under.g * (1.0 - colour.a),
        colour.b * colour.a + under.b * (1.0 - colour.a),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_secretary_link::Notebook;

    fn speak(text: &str, action: Option<&str>) -> Speak {
        Speak {
            id: 1,
            kind: SpeakKind::Briefing,
            text: text.to_owned(),
            action: action.map(str::to_owned),
            item: Some(3),
            notebook: Notebook::Pro,
            vip: false,
            deadline: None,
        }
    }

    /// Seven pixels a letter: enough to lay it out.
    fn measure(text: &str, _: f32, _: iced::Font) -> f32 {
        text.chars().count() as f32 * 7.0
    }

    #[test]
    fn her_bubble_has_its_three_buttons_and_its_bold_words() {
        let said = said(
            &speak(
                "**Ton point est prêt** pour la journée",
                Some("Ouvrir le point"),
            ),
            false,
            true,
            measure,
        );
        assert_eq!(said.who, "TA SECRÉTAIRE");
        let labels: Vec<&str> = said.buttons.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(labels, ["Ouvrir le point", "Plus tard ▾", "Fait"]);
        assert!(said.buttons[0].3, "the action is the primary");
        let bold: Vec<&str> = said
            .pieces
            .iter()
            .filter(|p| p.2)
            .map(|p| p.0.as_str())
            .collect();
        assert_eq!(bold, ["Ton", "point", "est", "prêt"]);
        assert!(said.menu.is_empty());
        let open = said_menu();
        assert_eq!(open.menu.len(), 5);
        assert_eq!(open.menu[4].0, "Ne plus me le dire");
        assert!(open.height > said.height);
    }

    fn said_menu() -> Said {
        said(&speak("Rappel", None), true, true, measure)
    }

    #[test]
    fn her_peek_sits_beside_the_bar_at_her_row() {
        let placed = placed(Form::Full, 60.0, 120.0, false);
        assert_eq!(placed.bubble.width, BUBBLE);
        assert!(placed.panda.x > placed.bubble.x + BUBBLE);
        assert_eq!(placed.margin.1, EDGE + Form::Full.width() + 18 - 24);
    }
}
