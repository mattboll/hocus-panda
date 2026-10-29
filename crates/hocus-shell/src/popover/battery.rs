//! The battery's popover (bar.md §9, the 3.7 mockup): "Batterie" and
//! whether the machine is on battery or plugged in; a large battery filled
//! to its level in its band's colour (a bolt on AC), the level, and what
//! it does ("3 h 40 restantes", "En charge · plein dans 1 h 10"…); "MODE
//! D'ÉNERGIE" and three tiles, the power profiles, the active one lit, one
//! the machine does not have greyed with a line saying so; the battery's
//! health, when known. Without a battery but with power profiles, the
//! modes alone, titled "Énergie".
//!
//! One canvas draws it, as the sound's: what changes draws this surface
//! alone. A tile asks power-profiles-daemon for its profile and shows it
//! chosen until the daemon says it (3 s at most: it may refuse).
//!
//! The keyboard: `Tab` and `Shift+Tab` go through the tiles the machine
//! has; the arrows choose the next one; `Enter` or `Space` chooses; `Esc`
//! closes.

use std::time::{Duration, Instant};

use hocus_link::POPOVER_WIDTH;
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme, keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry, Path},
        text::{Alignment, LineHeight},
    },
    window,
};

use super::{
    Event as Seen, event,
    wifi::{MANROPE_SEMIBOLD, background, measure, text, wrap},
};
use crate::levels::{
    self,
    draw::{
        ACCENT, DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, Sign, TEXT, band_colour, battery_sign,
        polygon, ring, stroke,
    },
    model::{Band, Battery, Profile, Snapshot, Words},
    power::Ask,
};

// --- The mockup's sizes, at ×1 ------------------------------------------------

const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const GAP: f32 = 8.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = POPOVER_WIDTH as f32 - BORDER - PAD_SIDE;
const HEADER: f32 = 32.0;
const HEADER_TOP: f32 = BORDER + PAD_TOP;
const TITLE: f32 = 16.0;
const NAME: f32 = 13.0;
/// The large battery, 42 × 24, 12 px before its level (26 px on 1.1
/// lines) over its line (13.5 px), 1 px apart; 2 px above, 4 px under.
const BIG: Size = Size::new(42.0, 24.0);
const BIG_GAP: f32 = 12.0;
const LEVEL: f32 = 26.0;
const LEVEL_LINE: f32 = 1.1;
const STATE: f32 = 13.5;
const BLOCK: f32 = 2.0 + LEVEL * LEVEL_LINE + 1.0 + STATE * LINE + 4.0;
/// The room its line has, beside the large battery; a line too long
/// breaks between its parts (" · "), the block growing.
const STATE_ROOM: f32 = RIGHT - LEFT - BIG.width - BIG_GAP;
/// The divider: 1 px, 2 px around it.
const RULE: f32 = 2.0;
const LABEL: f32 = 11.5;
const LABEL_SPACING: f32 = 0.08 * LABEL;
const LABEL_PAD: f32 = 2.0;
/// The modes' tiles: three in a row, 6 px apart, 58 px high.
const TILE: f32 = 58.0;
const TILE_GAP: f32 = 6.0;
const TILE_RADIUS: f32 = 10.0;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 4.0;
const MODE: f32 = 12.5;
/// The lines under them: 12.5 px on 1.4 lines; the health's on normal
/// lines, 2 px down.
const NOTE: f32 = 12.5;
const NOTE_LINE: f32 = 1.4;
const HEALTH_PAD: f32 = 2.0;

const CHOSEN: Color = Color::from_rgba8(108, 194, 135, 0.16);
const TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.04);
const TILE_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.08);
const TILE_EDGE: Color = Color::from_rgba8(255, 255, 255, 0.08);
const MISSING: Color = Color::from_rgba8(255, 255, 255, 0.28);
const RULE_COLOUR: Color = Color::from_rgba8(255, 255, 255, 0.08);

/// A tile shows the profile asked this long at most, until it is said.
const ASKED_FOR: Duration = Duration::from_secs(3);

const MODES: [Profile; 3] = [Profile::PowerSaver, Profile::Balanced, Profile::Performance];

/// The words of the shell's language, for what the height measures.
fn words() -> Words {
    Words::new(crate::clock::time::Lang::of_messages())
}

/// Where its parts are, for what it shows.
#[derive(Debug, Clone, PartialEq)]
struct Layout {
    /// The battery's block's top, and its line's lines.
    battery: Option<f32>,
    state: Vec<String>,
    /// The divider's and the label's tops, with a battery above them.
    label: Option<(f32, f32)>,
    /// The tiles' top.
    tiles: Option<f32>,
    /// The lines under them (no performance mode, performance held back),
    /// and the health's: each its top and its lines.
    notes: Vec<(f32, Vec<String>)>,
    health: Option<(f32, Vec<String>)>,
    end: f32,
}

impl Layout {
    fn of(levels: &Snapshot, words: Words) -> Self {
        let room = RIGHT - LEFT;
        let lines = |text: &str| wrap(text, room, |t| measure(t, NOTE, MANROPE));
        let mut end = HEADER_TOP + HEADER;
        let state = levels.battery.map_or_else(Vec::new, |b| {
            crate::levels::draw::lines(&words.battery_line(&b), STATE_ROOM, |t| {
                measure(t, STATE, MANROPE)
            })
        });
        let battery = levels.battery.map(|_| {
            let top = end + GAP;
            end = top + BLOCK + (state.len().max(1) - 1) as f32 * STATE * LINE;
            top
        });
        let modes = levels.profiles.any();
        let label = (modes && battery.is_some()).then(|| {
            let rule = end + GAP + RULE;
            let label = rule + 1.0 + RULE + GAP;
            end = label + 2.0 * LABEL_PAD + LABEL * LINE;
            (rule, label)
        });
        let tiles = modes.then(|| {
            let top = end + GAP;
            end = top + TILE;
            top
        });
        let mut notes = Vec::new();
        if modes {
            let missing = (!levels.profiles.performance).then(|| words.no_performance());
            let degraded = levels
                .degraded
                .filter(|_| levels.profile == Some(Profile::Performance))
                .map(|why| words.degraded(why));
            for note in missing.into_iter().chain(degraded) {
                let top = end + GAP;
                let note = lines(note);
                end = top + note.len() as f32 * NOTE * NOTE_LINE;
                notes.push((top, note));
            }
        }
        let health = levels.battery.and_then(|b| b.health()).map(|percent| {
            let top = end + GAP + HEALTH_PAD;
            let health = wrap(&words.health(percent), room, |t| measure(t, NOTE, MANROPE));
            end = top + health.len() as f32 * NOTE * LINE;
            (top, health)
        });
        Self {
            battery,
            state,
            label,
            tiles,
            notes,
            health,
            end,
        }
    }

    fn height(&self) -> u32 {
        (self.end + PAD_BOTTOM + BORDER).round() as u32
    }
}

/// The popover opens: nothing to start.
pub fn opening() {}

/// Its height for what it shows now.
pub fn height() -> u32 {
    Layout::of(&levels::snapshot(), words()).height()
}

/// The battery's popover on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    /// Its surface: it reacts only while open, not while it fades out.
    pub id: window::Id,
    pub words: Words,
    /// On the compositor's glass (else opaque).
    pub glass: bool,
}

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    levels: Snapshot,
    layout: Option<Layout>,
    /// The label's letters and their places.
    label: Vec<(char, f32)>,
    hover: Option<Profile>,
    pressed: Option<Profile>,
    /// The profile asked, and since when.
    asked: Option<(Profile, Instant)>,
    focus: Option<Profile>,
    pointer: Option<Point>,
    inside: bool,
}

impl State {
    /// The tiles that can be chosen, in `Tab`'s order.
    fn targets(&self) -> Vec<Profile> {
        if self.layout.as_ref().and_then(|l| l.tiles).is_none() {
            return Vec::new();
        }
        MODES
            .into_iter()
            .filter(|p| self.levels.profiles.has(*p))
            .collect()
    }

    /// The profile shown chosen: the one asked, else the daemon's.
    fn chosen(&self) -> Option<Profile> {
        self.asked.map(|(p, _)| p).or(self.levels.profile)
    }

    fn choose(&mut self, profile: Profile) {
        if !self.levels.profiles.has(profile) {
            return;
        }
        if self.chosen() != Some(profile) {
            levels::ask_power(Ask::Profile(profile));
        }
        self.asked = Some((profile, Instant::now()));
    }

    /// Tile `profile`'s place.
    fn tile(&self, profile: Profile) -> Option<Rectangle> {
        let top = self.layout.as_ref()?.tiles?;
        let i = MODES.iter().position(|p| *p == profile)? as f32;
        let width = (RIGHT - LEFT - 2.0 * TILE_GAP) / 3.0;
        Some(Rectangle::new(
            Point::new(LEFT + i * (width + TILE_GAP), top),
            Size::new(width, TILE),
        ))
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
        match event {
            iced::Event::Window(window::Event::RedrawRequested(now)) => self.refresh(state, *now),
            _ if !super::with(|c| c.is_open(self.id)) => None,
            iced::Event::Mouse(event) => {
                let at = crate::levels::draw::pointer(event, bounds, cursor, state.pointer);
                state.pointer = at;
                self.mouse(state, event, at)
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                self.key(state, key, modifiers.shift())
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
        let mut frame = Frame::new(renderer, bounds.size());
        background(&mut frame, bounds.size(), self.glass);
        let Some(layout) = &state.layout else {
            return vec![frame.into_geometry()];
        };
        self.header(&mut frame, state.levels.battery);
        if let (Some(top), Some(battery)) = (layout.battery, state.levels.battery) {
            self.battery(&mut frame, top, &battery, &layout.state);
        }
        if let Some((rule, label)) = layout.label {
            frame.fill_rectangle(
                Point::new(LEFT, rule),
                Size::new(RIGHT - LEFT, 1.0),
                RULE_COLOUR,
            );
            for (ch, x) in &state.label {
                let mut letter = text(
                    ch.to_string(),
                    Point::new(*x, label + LABEL_PAD + LABEL * LINE / 2.0),
                    LABEL,
                    DIM,
                    Alignment::Left,
                );
                letter.font = MANROPE_BOLD;
                frame.fill_text(letter);
            }
        }
        for profile in MODES {
            self.tile(&mut frame, state, profile);
        }
        for (top, lines) in &layout.notes {
            lines_at(&mut frame, *top, lines, NOTE_LINE);
        }
        if let Some((top, lines)) = &layout.health {
            lines_at(&mut frame, *top, lines, LINE);
        }
        if let Some(rect) = state.focus.and_then(|p| state.tile(p)) {
            ring(&mut frame, rect, TILE_RADIUS);
        }
        vec![frame.into_geometry()]
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

/// `lines` in `text.faint` from `top`, `height` (em) apart.
fn lines_at(f: &mut Frame, top: f32, lines: &[String], height: f32) {
    for (i, line) in lines.iter().enumerate() {
        let mut note = text(
            line.clone(),
            Point::new(LEFT, top + (i as f32 + 0.5) * NOTE * height),
            NOTE,
            FAINT,
            Alignment::Left,
        );
        note.line_height = LineHeight::Relative(height);
        f.fill_text(note);
    }
}

/// The level's and its line's colour: as the bar's (§4.2).
fn band(band: Band) -> (Color, Color) {
    match band {
        Band::Ok => (TEXT, DIM),
        _ => (band_colour(band), band_colour(band)),
    }
}

impl Panel {
    /// What it shows, read afresh.
    fn refresh<Message>(&self, state: &mut State, now: Instant) -> Option<canvas::Action<Message>> {
        // Keys that reached another of the shell's surfaces (see
        // `popover::key`).
        let keys = super::take_keys();
        if !keys.is_empty() && super::with(|c| c.is_open(self.id)) {
            for pressed in keys {
                let shift = pressed.modifiers.shift();
                let _: Option<canvas::Action<Message>> = self.key(state, &pressed.key, shift);
            }
        }
        let levels = levels::snapshot();
        if state.layout.is_none() || levels != state.levels {
            let layout = Layout::of(&levels, self.words);
            tracing::debug!(
                "the battery's popover: title={} plugged={:?} line={:?} modes={:?} chosen={:?} \
                 notes={:?} health={:?}",
                self.words.battery_title(levels.battery.is_some()),
                levels
                    .battery
                    .map(|b| self.words.plugged(b.charge.plugged())),
                levels.battery.map(|b| self.words.battery_line(&b)),
                levels.profiles,
                levels.profile,
                layout
                    .notes
                    .iter()
                    .map(|(_, l)| l.join(" "))
                    .collect::<Vec<_>>(),
                layout.health.as_ref().map(|(_, l)| l.join(" ")),
            );
            if state.layout.as_ref() != Some(&layout) {
                state.hover = None;
                state.layout = Some(layout);
                // A change of size reaches the controller, which resizes
                // the surface (without waking it again).
                super::refit(super::Item::Battery);
            }
            state.levels = levels;
        }
        if state.label.is_empty() {
            let mut x = LEFT;
            state.label = self
                .words
                .power_mode()
                .chars()
                .map(|ch| {
                    let at = x;
                    x += measure(&ch.to_string(), LABEL, MANROPE_BOLD) + LABEL_SPACING;
                    (ch, at)
                })
                .collect();
        }
        if state
            .focus
            .is_some_and(|focus| !state.targets().contains(&focus))
        {
            state.focus = None;
        }
        let (asked, since) = state.asked?;
        let until = since + ASKED_FOR;
        if state.levels.profile == Some(asked) || now >= until {
            state.asked = None;
            None
        } else {
            Some(canvas::Action::request_redraw_at(until))
        }
    }

    fn header(&self, f: &mut Frame, battery: Option<Battery>) {
        let middle = HEADER_TOP + HEADER / 2.0;
        let mut title = text(
            self.words.battery_title(battery.is_some()).to_owned(),
            Point::new(LEFT, middle),
            TITLE,
            TEXT,
            Alignment::Left,
        );
        title.font = MANROPE_BOLD;
        f.fill_text(title);
        if let Some(battery) = battery {
            f.fill_text(text(
                self.words.plugged(battery.charge.plugged()).to_owned(),
                Point::new(RIGHT, middle),
                NAME,
                FAINT,
                Alignment::Right,
            ));
        }
    }

    /// The large battery, its level and its line.
    fn battery(&self, f: &mut Frame, top: f32, battery: &Battery, lines: &[String]) {
        let column = LEVEL * LEVEL_LINE + 1.0 + lines.len().max(1) as f32 * STATE * LINE;
        let middle = top + 2.0 + column / 2.0;
        let fill = band_colour(battery.band());
        battery_sign(
            f,
            Rectangle::new(Point::new(LEFT, middle - BIG.height / 2.0), BIG),
            Sign {
                percent: battery.percent,
                fill,
                bolt: battery.bolt(),
                outline: 1.4,
            },
            1.0,
        );
        let (level, line) = band(battery.band());
        let x = LEFT + BIG.width + BIG_GAP;
        let mut big = text(
            format!("{}\u{a0}%", battery.shown()),
            Point::new(x, top + 2.0 + LEVEL * LEVEL_LINE / 2.0),
            LEVEL,
            level,
            Alignment::Left,
        );
        big.font = MANROPE_BOLD;
        big.line_height = LineHeight::Relative(LEVEL_LINE);
        f.fill_text(big);
        let first = top + 2.0 + LEVEL * LEVEL_LINE + 1.0;
        for (i, text_line) in lines.iter().enumerate() {
            f.fill_text(text(
                text_line.clone(),
                Point::new(x, first + (i as f32 + 0.5) * STATE * LINE),
                STATE,
                line,
                Alignment::Left,
            ));
        }
    }

    /// A mode's tile: lit when chosen, greyed when the machine has none.
    fn tile(&self, f: &mut Frame, state: &State, profile: Profile) {
        let Some(rect) = state.tile(profile) else {
            return;
        };
        let there = state.levels.profiles.has(profile);
        let chosen = there && state.chosen() == Some(profile);
        let (fill, edge, ink) = match (there, chosen) {
            (false, _) => (TILE_FILL, TILE_EDGE, MISSING),
            (true, true) => (CHOSEN, ACCENT, TEXT),
            (true, false) if state.hover == Some(profile) => (TILE_HOVER, TILE_EDGE, DIM),
            (true, false) => (TILE_FILL, TILE_EDGE, DIM),
        };
        let shape = Path::rounded_rectangle(rect.position(), rect.size(), TILE_RADIUS.into());
        f.fill(&shape, fill);
        let inside = Path::rounded_rectangle(
            Point::new(rect.x + 0.5, rect.y + 0.5),
            Size::new(rect.width - 1.0, rect.height - 1.0),
            (TILE_RADIUS - 0.5).into(),
        );
        f.stroke(&inside, stroke(1.0, edge));
        let column = ICON + ICON_GAP + MODE * LINE;
        let top = rect.y + (TILE - column) / 2.0;
        mode_icon(
            f,
            Point::new(rect.center_x() - ICON / 2.0, top),
            profile,
            ink,
        );
        let mut name = text(
            self.words.mode(profile).to_owned(),
            Point::new(rect.center_x(), top + ICON + ICON_GAP + MODE * LINE / 2.0),
            MODE,
            ink,
            Alignment::Center,
        );
        name.font = MANROPE_SEMIBOLD;
        f.fill_text(name);
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .targets()
                .into_iter()
                .find(|p| state.tile(*p).is_some_and(|r| r.contains(at)))
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                if !state.inside && at.is_some() {
                    state.inside = true;
                    event(Seen::PopoverEnter);
                }
                if state.hover != target {
                    state.hover = target;
                    return redraw;
                }
                None
            }
            mouse::Event::CursorLeft => {
                state.inside = false;
                event(Seen::PopoverLeave);
                state.hover = None;
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(profile) = state.pressed.take().filter(|p| Some(*p) == target) {
                    state.choose(profile);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        key: &keyboard::Key,
        shift: bool,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order = state.targets();
        let at = state.focus.and_then(|f| order.iter().position(|t| *t == f));
        match key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                let n = order.len() as isize;
                if n == 0 {
                    return None;
                }
                let by = if shift { -1 } else { 1 };
                let next = match at {
                    Some(i) => (i as isize + by).rem_euclid(n),
                    None if by > 0 => 0,
                    None => n - 1,
                };
                state.focus = order.get(next as usize).copied();
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(profile) = state.focus {
                    state.choose(profile);
                }
            }
            // Through the tiles, choosing as it goes (a radio group's way).
            Key::Named(
                arrow @ (Named::ArrowLeft | Named::ArrowRight | Named::ArrowUp | Named::ArrowDown),
            ) => {
                let i = at?;
                let up = matches!(arrow, Named::ArrowRight | Named::ArrowDown);
                let next = if up {
                    (i + 1).min(order.len() - 1)
                } else {
                    i.saturating_sub(1)
                };
                let profile = order[next];
                state.focus = Some(profile);
                state.choose(profile);
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
}

/// A mode's icon, 18 px, from the mockup's SVGs: a leaf, scales, a bolt.
fn mode_icon(f: &mut Frame, at: Point, profile: Profile, colour: Color) {
    let s = ICON / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    match profile {
        Profile::PowerSaver => {
            // `M5 19c0-8 5-13 14-14 0 9-5 14-13 14` and `M5 19l7-7`.
            let leaf = Path::new(|b| {
                b.move_to(p(5.0, 19.0));
                b.bezier_curve_to(p(5.0, 11.0), p(10.0, 6.0), p(19.0, 5.0));
                b.bezier_curve_to(p(19.0, 14.0), p(14.0, 19.0), p(6.0, 19.0));
            });
            f.stroke(&leaf, line);
            f.stroke(&Path::line(p(5.0, 19.0), p(12.0, 12.0)), line);
        }
        Profile::Balanced => {
            // `M12 3v18M5 21h14M6 7h12`, and a pan under each end:
            // `M6 7l-3 6a3 3 0 0 0 6 0z`.
            for (from, to) in [((12.0, 3.0), (12.0, 21.0)), ((5.0, 21.0), (19.0, 21.0))] {
                f.stroke(&Path::line(p(from.0, from.1), p(to.0, to.1)), line);
            }
            f.stroke(&Path::line(p(6.0, 7.0), p(18.0, 7.0)), line);
            for x in [6.0, 18.0] {
                let pan = Path::new(|b| {
                    b.move_to(p(x, 7.0));
                    b.line_to(p(x - 3.0, 13.0));
                    b.arc(canvas::path::Arc {
                        center: p(x, 13.0),
                        radius: 3.0 * s,
                        start_angle: iced::Radians(std::f32::consts::PI),
                        end_angle: iced::Radians(0.0),
                    });
                    b.line_to(p(x, 7.0));
                });
                f.stroke(&pan, line);
            }
        }
        Profile::Performance => {
            // `M13 2 4 14h7l-1 8 9-12h-7z`.
            let bolt = polygon(&[
                p(13.0, 2.0),
                p(4.0, 14.0),
                p(11.0, 14.0),
                p(10.0, 22.0),
                p(19.0, 10.0),
                p(12.0, 10.0),
            ]);
            f.stroke(&bolt, line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::levels::model::{Charge, Degraded, Profiles};

    fn levels(battery: Option<f32>, profiles: Profiles) -> Snapshot {
        Snapshot {
            battery: battery.map(|health| Battery {
                percent: 82.0,
                charge: Charge::Discharging,
                left: None,
                health: Some(health),
            }),
            profile: Some(Profile::Balanced),
            profiles,
            ..Snapshot::default()
        }
    }

    const ALL: Profiles = Profiles {
        power_saver: true,
        balanced: true,
        performance: true,
    };

    #[test]
    fn its_height_follows_what_it_shows() {
        let words = Words::new(crate::clock::time::Lang::French);
        // The header, the battery's block, the divider, the label, the
        // tiles, the health (one line wide enough, or two).
        let full = Layout::of(&levels(Some(91.0), ALL), words);
        assert_eq!(full.battery, Some(53.0));
        let tiles = full.tiles.expect("tiles");
        let (rule, label) = full.label.expect("a label");
        assert_eq!(rule, 53.0 + BLOCK + 8.0 + 2.0);
        assert!((tiles - (label + 4.0 + LABEL * LINE + 8.0)).abs() < 0.01);
        let (health, lines) = full.health.clone().expect("health");
        assert_eq!(health, tiles + TILE + 8.0 + 2.0);
        assert!(!lines.is_empty());
        // Health unknown: no line.
        let unknown = Layout::of(&levels(Some(0.0), ALL), words);
        assert_eq!(unknown.health, None);
        assert!(unknown.height() < full.height());
        // Critical: its line on two lines, the block taller.
        let mut low = levels(Some(91.0), ALL);
        if let Some(b) = low.battery.as_mut() {
            b.percent = 9.0;
            b.left = Some(std::time::Duration::from_secs(25 * 60));
        }
        let low = Layout::of(&low, words);
        assert_eq!(low.state, ["25 min restantes", "branche le chargeur"]);
        assert!((low.height() as f32 - full.height() as f32 - STATE * LINE).abs() <= 1.0);
        // No performance mode: a line under the tiles.
        let two = Profiles {
            performance: false,
            ..ALL
        };
        let missing = Layout::of(&levels(Some(0.0), two), words);
        assert_eq!(missing.notes.len(), 1);
        assert!(missing.height() > unknown.height());
        // Performance held back, while chosen.
        let mut held = levels(Some(0.0), ALL);
        held.degraded = Some(Degraded::Hot);
        assert!(Layout::of(&held, words).notes.is_empty());
        held.profile = Some(Profile::Performance);
        assert_eq!(Layout::of(&held, words).notes.len(), 1);
        // No battery: the modes alone, under the header, no label.
        let modes = Layout::of(&levels(None, ALL), words);
        assert_eq!((modes.battery, modes.label), (None, None));
        assert_eq!(modes.tiles, Some(HEADER_TOP + HEADER + GAP));
        assert_eq!(
            modes.height(),
            (HEADER_TOP + HEADER + GAP + TILE + 15.0) as u32
        );
        // No profiles: the battery alone.
        let alone = Layout::of(&levels(Some(0.0), Profiles::default()), words);
        assert_eq!((alone.label, alone.tiles), (None, None));
    }

    #[test]
    fn only_the_modes_there_can_be_chosen() {
        let words = Words::new(crate::clock::time::Lang::English);
        let two = Profiles {
            performance: false,
            ..ALL
        };
        let state = State {
            levels: levels(None, two),
            layout: Some(Layout::of(&levels(None, two), words)),
            ..State::default()
        };
        assert_eq!(state.targets(), [Profile::PowerSaver, Profile::Balanced]);
        let saver = state.tile(Profile::PowerSaver).expect("a tile");
        let performance = state.tile(Profile::Performance).expect("a tile");
        assert_eq!(saver.x, LEFT);
        assert!((performance.x + performance.width - RIGHT).abs() < 0.01);
        assert_eq!(state.chosen(), Some(Profile::Balanced));
    }
}
