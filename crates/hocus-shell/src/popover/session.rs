//! The footer's user's popover (5.5), until the session's menu (7.4)
//! takes its place: the user's name ([`crate::user`]), "Réglages", which
//! opens the Settings (6.1), and "Se déconnecter", which ends the session
//! as its shortcut does (the compositor's `log-out`). The desktop has no
//! lock of its own yet: no "Verrouiller" until it does.
//!
//! Logging out takes two steps, so that a slip loses no open work: a click
//! on the row (or `Enter` or `Space` once `Tab` gave it the keyboard)
//! turns it into "Confirmer la déconnexion" in amber for 4 s, and a second
//! one within that time logs out. `Esc`, the pointer leaving the popover,
//! or the 4 s running out let it go back; `Esc` again closes.

use std::time::{Duration, Instant};

use hocus_link::{Launch, POPOVER_WIDTH, ToCompositor};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Effect, Event as Seen, Pressed, event,
    kit::{self, Input, Pointer, RIGHT, text, text_in},
};
use crate::levels::draw::{MANROPE_BOLD, TEXT, ring};

pub const WIDTH: u32 = POPOVER_WIDTH.unsigned_abs();
/// The rows under the header: 44 px, 2 px apart, the first 4 px under it,
/// 8 px in; their 28 px tile (radius 8), 10 px before their words (13 px,
/// 600).
const ROW_TOP: f32 = kit::HEADER_TOP + kit::HEADER + 4.0;
const ROW: f32 = 44.0;
const ROW_GAP: f32 = 2.0;
const TILE: f32 = 28.0;
const LINE_TEXT: f32 = 13.0;
const ROW_PAD: f32 = 8.0;
const ROW_RADIUS: f32 = 10.0;
const ROW_HOVER: Color = Color::from_rgba8(255, 255, 255, 0.06);
const TILE_RADIUS: f32 = 8.0;
const TILE_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
const TILE_GAP: f32 = 10.0;
const GLYPH: f32 = 18.0;
/// The row asking to confirm: amber, as long as this.
const CONFIRM: Duration = Duration::from_secs(4);
const CONFIRM_INK: Color = crate::levels::draw::rgb(0xF5C46E);
const CONFIRM_FILL: Color = Color::from_rgba8(245, 196, 110, 0.12);

/// What a press on the row does, the row asking to confirm since `armed`:
/// it asks (from now), or, asked less than 4 s ago, logs out (`true`).
fn press(armed: Option<Instant>, now: Instant) -> (Option<Instant>, bool) {
    match armed {
        Some(since) if now.saturating_duration_since(since) < CONFIRM => (None, true),
        _ => (Some(now), false),
    }
}

/// Its rows, top to bottom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    Settings,
    LogOut,
}

impl Row {
    const ALL: [Self; 2] = [Self::Settings, Self::LogOut];

    /// Its place.
    fn place(self) -> Rectangle {
        let i = Self::ALL.iter().position(|r| *r == self).unwrap_or(0) as f32;
        Rectangle::new(
            Point::new(kit::LEFT - kit::ROW_OUT, ROW_TOP + i * (ROW + ROW_GAP)),
            Size::new(kit::RIGHT - kit::LEFT + 2.0 * kit::ROW_OUT, ROW),
        )
    }

    fn at(point: Point) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.place().contains(point))
    }
}

/// Its height: the header and its rows.
pub fn height() -> u32 {
    kit::height(Row::LogOut.place().y + ROW)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
}

impl Words {
    pub fn new(lang: crate::clock::time::Lang) -> Self {
        Self {
            fr: lang == crate::clock::time::Lang::French,
        }
    }

    pub fn settings(self) -> &'static str {
        if self.fr { "Réglages" } else { "Settings" }
    }

    pub fn log_out(self) -> &'static str {
        if self.fr {
            "Se déconnecter"
        } else {
            "Log out"
        }
    }

    pub fn confirm(self) -> &'static str {
        if self.fr {
            "Confirmer la déconnexion"
        } else {
            "Confirm log out"
        }
    }
}

/// The popover on its surface.
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
    /// The name, as cut to the header.
    name: Option<String>,
    pointer: Pointer<Row>,
    /// The row holding the keyboard.
    focused: Option<Row>,
    /// The row asks to confirm, since then.
    armed: Option<Instant>,
}

impl State {
    /// A press on the row: asks to confirm, or logs out.
    fn press<Message>(&mut self) -> Option<canvas::Action<Message>> {
        let now = Instant::now();
        let (armed, out) = press(self.armed, now);
        self.armed = armed;
        if out {
            Panel::log_out();
            return Some(canvas::Action::request_redraw().and_capture());
        }
        tracing::debug!("the session's popover asks to confirm");
        // Drawn again as the question lapses: a wake through the
        // compositor, from a thread that waits for it (the layer surface
        // takes no timed redraw of its own).
        let woke = std::thread::Builder::new()
            .name("log out".to_owned())
            .spawn(|| {
                std::thread::sleep(CONFIRM);
                super::changed(super::Item::User);
            });
        if let Err(err) = woke {
            tracing::warn!(%err, "the question to confirm will not lapse on its own");
        }
        Some(canvas::Action::request_redraw().and_capture())
    }

    /// The question goes back to "Se déconnecter", if it was asked.
    fn cancel(&mut self, why: &str) -> bool {
        let asked = self.armed.take().is_some();
        if asked {
            tracing::debug!(why, "the session's popover no longer asks to confirm");
        }
        asked
    }
}

/// The rows' glyphs: the Settings' sliders; a door and an arrow leaving
/// it.
fn glyph(row: Row) -> svg::Handle {
    static GLYPHS: std::sync::OnceLock<[svg::Handle; 2]> = std::sync::OnceLock::new();
    let make = |path: &str| {
        svg::Handle::from_memory(
            format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" \
                 width=\"24\" height=\"24\"><path d=\"{path}\" \
                 fill=\"none\" stroke=\"{}\" stroke-width=\"1.8\" stroke-linecap=\"round\" \
                 stroke-linejoin=\"round\"/></svg>",
                crate::notifications::bell::INK
            )
            .into_bytes(),
        )
    };
    let glyphs = GLYPHS.get_or_init(|| {
        [
            make("M4 7 H13 M17 7 H20 M15 4.5 V9.5 M4 17 H7 M11 17 H20 M9 14.5 V19.5"),
            make(
                "M10 5 H6.5 a1.5 1.5 0 0 0 -1.5 1.5 V17.5 a1.5 1.5 0 0 0 1.5 1.5 H10 \
                 M14 8 L18 12 L14 16 M18 12 H9.5",
            ),
        ]
    });
    glyphs[usize::from(row == Row::LogOut)].clone()
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
                for pressed in kit::keys(self.id) {
                    let _: Option<canvas::Action<Message>> = Self::key(state, &pressed);
                }
                if state
                    .armed
                    .is_some_and(|since| now.saturating_duration_since(since) >= CONFIRM)
                {
                    state.cancel("its time ran out");
                }
                self.refresh(state);
                None
            }
            Input::Mouse(seen, at) => Self::mouse(state, seen, at),
            Input::Key(pressed) => Self::key(state, &pressed),
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
        if let Some(name) = &state.name {
            kit::title(f, name);
        }
        let asking = state.armed.is_some();
        for which in Row::ALL {
            let row = which.place();
            let confirming = asking && which == Row::LogOut;
            if confirming {
                f.fill(&kit::rounded(row, ROW_RADIUS), CONFIRM_FILL);
            } else if state.pointer.hover == Some(which) {
                f.fill(&kit::rounded(row, ROW_RADIUS), ROW_HOVER);
            }
            if state.focused == Some(which) {
                ring(f, row, ROW_RADIUS);
            }
            let tile = Rectangle::new(
                Point::new(row.x + ROW_PAD, row.center_y() - TILE / 2.0),
                Size::new(TILE, TILE),
            );
            f.fill(&kit::rounded(tile, TILE_RADIUS), TILE_FILL);
            let centre = tile.center();
            f.draw_svg(
                Rectangle::new(
                    Point::new(centre.x - GLYPH / 2.0, centre.y - GLYPH / 2.0),
                    Size::new(GLYPH, GLYPH),
                ),
                svg::Svg::new(glyph(which)),
            );
            let words = match which {
                Row::Settings => self.words.settings(),
                Row::LogOut if confirming => self.words.confirm(),
                Row::LogOut => self.words.log_out(),
            };
            f.fill_text(text_in(
                kit::MANROPE_SEMIBOLD,
                text(
                    words.to_owned(),
                    Point::new(tile.x + TILE + TILE_GAP, row.center_y()),
                    LINE_TEXT,
                    if confirming { CONFIRM_INK } else { TEXT },
                    Alignment::Left,
                ),
            ));
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

impl Panel {
    /// The name, read afresh while it is open, cut to the header.
    fn refresh(&self, state: &mut State) {
        if !super::with(|c| c.is_open(self.id)) {
            return;
        }
        let name = crate::user::name();
        let room = RIGHT - kit::LEFT;
        let name = match iced::advanced::graphics::text::font_system().write() {
            Ok(mut system) => kit::elide(&name, room, |t| {
                kit::measure_in(system.raw(), t, kit::TITLE, MANROPE_BOLD)
            }),
            Err(_) => name,
        };
        if state.name.as_ref() == Some(&name) {
            return;
        }
        tracing::debug!(
            name,
            rows = ?[self.words.settings(), self.words.log_out()],
            "the session's popover shows it"
        );
        state.name = Some(name);
    }

    /// The Settings open (or come forward), the popover closes: the
    /// program in `PATH`, else beside the shell's.
    fn settings() {
        let program = settings_program();
        tracing::info!(program, "the Settings open from the footer");
        let launch = Launch {
            id: "hocus-settings".to_owned(),
            terminal: false,
            dir: String::new(),
            argv: vec![program],
        };
        super::send(vec![Effect::Tell(ToCompositor::Launch(launch))]);
        event(Seen::Chosen);
    }

    /// The session ends; the popover closes.
    fn log_out() {
        tracing::info!("the user logs out from the footer");
        super::send(vec![Effect::Tell(ToCompositor::LogOut)]);
        event(Seen::Chosen);
    }

    fn mouse<Message>(
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(Row::at);
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                state.cancel("the pointer left");
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focused = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                match state.pointer.release(target) {
                    Some(Row::LogOut) => state.press(),
                    Some(Row::Settings) => {
                        Panel::settings();
                        redraw
                    }
                    None => redraw,
                }
            }
            _ => None,
        }
    }

    fn key<Message>(state: &mut State, pressed: &Pressed) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        match &pressed.key {
            // Asking to confirm, it goes back; else it closes.
            Key::Named(Named::Escape) => {
                if !state.cancel("Escape") {
                    event(Seen::Escape);
                    return Some(canvas::Action::capture());
                }
            }
            // Its rows hold the keyboard, round and round.
            Key::Named(Named::Tab | Named::ArrowDown | Named::ArrowUp) => {
                let back = matches!(&pressed.key, Key::Named(Named::ArrowUp))
                    || (matches!(&pressed.key, Key::Named(Named::Tab))
                        && pressed.modifiers.shift());
                state.focused = Some(match (state.focused, back) {
                    (None, false) | (Some(Row::LogOut), false) => Row::Settings,
                    (None, true) | (Some(Row::Settings), _) => Row::LogOut,
                    (Some(Row::LogOut), true) => Row::Settings,
                });
            }
            Key::Named(Named::Enter | Named::Space) => match state.focused {
                Some(Row::LogOut) => return state.press(),
                Some(Row::Settings) => Panel::settings(),
                None => return None,
            },
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
    }
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
    use super::*;

    #[test]
    fn logging_out_takes_a_second_press_within_four_seconds() {
        let t = Instant::now();
        // The first asks to confirm; a second within 4 s logs out.
        assert_eq!(press(None, t), (Some(t), false));
        let later = t + Duration::from_millis(3900);
        assert_eq!(press(Some(t), later), (None, true));
        // Past 4 s, it asks again.
        let lapsed = t + CONFIRM;
        assert_eq!(press(Some(t), lapsed), (Some(lapsed), false));
        let fr = Words { fr: true };
        assert_eq!(fr.confirm(), "Confirmer la déconnexion");
        assert_eq!(fr.settings(), "Réglages");
        assert_eq!(Words { fr: false }.confirm(), "Confirm log out");
    }
}

#[cfg(test)]
mod rows {
    use super::*;

    #[test]
    fn the_settings_row_comes_first_then_logging_out() {
        let settings = Row::Settings.place();
        let out = Row::LogOut.place();
        assert_eq!(settings.y, ROW_TOP);
        assert_eq!(out.y, ROW_TOP + ROW + ROW_GAP);
        assert_eq!(Row::at(settings.center()), Some(Row::Settings));
        assert_eq!(Row::at(out.center()), Some(Row::LogOut));
        assert_eq!(height(), kit::height(out.y + ROW));
    }
}
