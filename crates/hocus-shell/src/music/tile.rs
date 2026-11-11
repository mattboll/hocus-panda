//! The music player's tile over the desktops (s12), as iced's stock
//! widgets: a button holding the cover, the title over the artist ("En
//! pause" while paused), and a round button on the right that plays or
//! pauses without opening the popover. Only its small bars, which move
//! while it plays, are a canvas.

use std::time::Instant;

use iced::{
    Border, Color, Element, Length, Padding, Rectangle, Renderer, Size, Theme,
    alignment::Vertical,
    mouse,
    widget::{Space, button, canvas, column, container, row},
};

use super::{Player, ROOM, SIDE, SURFACE_WIDTH, TILE, WIDTH, Words};
use crate::{
    Message,
    levels::draw::{ACCENT, DIM, MANROPE, TEXT},
    popover::{
        self, Item, kit,
        music::{Glyph, cover},
    },
    theme::Ink,
    ui::{
        ring,
        style::{filled, lit, said},
    },
};

const COVER: f32 = 34.0;
const COVER_RADIUS: f32 = 6.0;
const GAP: f32 = 9.0;
const PAD: f32 = 7.0;
const RADIUS: f32 = 10.0;
const TITLE: f32 = 12.5;
const ARTIST: f32 = 11.5;
const BUTTON: f32 = 28.0;
const BARS: f32 = 10.0;
/// The bars' cycle, seconds, and each one's head start.
const CYCLE: f32 = 1.1;
const DELAYS: [f32; 3] = [0.0, 0.4, 0.8];

const ROW: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.09));
const LINE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.07));

/// What the tile asks.
#[derive(Debug, Clone)]
pub enum Press {
    /// The tile: its popover, on `bar`.
    Open { bar: String },
    /// Its round button: play or pause.
    PlayPause,
}

/// The tile in bar `bar`'s frame: from its right edge (negative inside
/// it), from its top.
pub fn rect_in_bar(top: f32) -> Rectangle {
    Rectangle::new(
        iced::Point::new(SIDE - SURFACE_WIDTH as f32 - 1.0, top),
        Size::new(WIDTH, TILE),
    )
}

/// What a press asks, on the loop.
pub fn pressed(press: Press) {
    match press {
        Press::Open { bar } => {
            let Some((top, true)) = super::placed(&bar) else {
                return;
            };
            popover::event(popover::Event::ItemPress {
                bar,
                item: Item::Music,
                rect: rect_in_bar(top),
            });
        }
        Press::PlayPause => {
            if let Some(bus) = super::front_bus() {
                super::ask(super::Ask::PlayPause(bus));
            }
        }
    }
}

/// Bar `bar`'s tile, on its surface.
pub fn view<'a>(bar: &str, motion: bool, backdrop: Option<Color>) -> Element<'a, Message> {
    let shown = super::shown();
    let (Some((_, true)), Some(front)) = (super::placed(bar), shown.front) else {
        return Space::new().into();
    };
    let words = Words::new(crate::clock::time::Lang::of_messages());
    let (open, focused) =
        popover::with(|c| (c.shows(bar, Item::Music), c.focused(bar, Item::Music)));
    container(ring(
        tile(bar, &front, words, open, motion),
        focused,
        RADIUS,
    ))
    .padding(Padding {
        top: ROOM,
        left: SIDE,
        ..Padding::ZERO
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .style(move |_| container::Style {
        background: backdrop.map(Into::into),
        ..container::Style::default()
    })
    .into()
}

fn tile<'a>(
    bar: &str,
    front: &Player,
    words: Words,
    open: bool,
    motion: bool,
) -> Element<'a, Message> {
    let playing = front.playing();
    let room = WIDTH - 2.0 * (PAD + 1.0) - COVER - BUTTON - 2.0 * GAP;
    let semibold = kit::MANROPE_SEMIBOLD;
    let title = kit::elide(&front.title, room, |s| kit::measure(s, TITLE, semibold));
    let second = if playing {
        kit::elide(&front.artist, room, |s| kit::measure(s, ARTIST, MANROPE))
    } else {
        words.paused_line().to_owned()
    };
    let lines = column![
        said(
            title,
            TITLE,
            semibold,
            if playing { TEXT } else { DIM }.get()
        ),
        said(second, ARTIST, MANROPE, DIM.get()),
    ]
    .spacing(1.0)
    .width(Length::Fill);
    let glyph: Element<'a, Message> = if playing {
        canvas(Bars { motion }).width(8.0).height(BARS).into()
    } else {
        canvas(Glyph::Play(TEXT.get()))
            .width(14.0)
            .height(14.0)
            .into()
    };
    let play = button(container(glyph).center(Length::Fill))
        .padding(0)
        .width(BUTTON)
        .height(BUTTON)
        .style(|_, status| {
            let fill = if lit(status) {
                ROW_HOVER.get()
            } else {
                Color::TRANSPARENT
            };
            filled(fill, BUTTON / 2.0, TEXT.get())
        })
        .on_press_maybe(
            front
                .can_control
                .then_some(Message::MusicTile(Press::PlayPause)),
        );
    let content = row![
        cover(front, COVER, COVER_RADIUS, false),
        Space::new().width(GAP),
        lines,
        Space::new().width(GAP),
        play,
    ]
    .align_y(Vertical::Center);
    button(content)
        .padding(PAD)
        .width(WIDTH)
        .height(TILE)
        .style(move |_, status| {
            let on = open || lit(status);
            let mut style = filled(if on { ROW_HOVER } else { ROW }.get(), RADIUS, TEXT.get());
            style.border = Border {
                color: if on { LINE.get() } else { Color::TRANSPARENT },
                width: 1.0,
                radius: RADIUS.into(),
            };
            style
        })
        .on_press(Message::MusicTile(Press::Open {
            bar: bar.to_owned(),
        }))
        .into()
}

/// Three small bars that move while it plays (still with reduced motion
/// and on the CPU, which draws no motion).
#[derive(Debug, Clone, Copy)]
struct Bars {
    motion: bool,
}

#[derive(Debug, Default)]
struct Moving {
    since: Option<Instant>,
    now: Option<Instant>,
}

/// A frame's time, while they move.
const FRAME: std::time::Duration = std::time::Duration::from_millis(33);

impl canvas::Program<Message> for Bars {
    type State = Moving;

    fn update(
        &self,
        state: &mut Moving,
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        state.since.get_or_insert(*now);
        state.now = Some(*now);
        self.motion
            .then(|| canvas::Action::request_redraw_at(*now + FRAME))
    }

    fn draw(
        &self,
        state: &Moving,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let t = match (state.since, state.now) {
            (Some(since), Some(now)) if self.motion => {
                now.saturating_duration_since(since).as_secs_f32()
            }
            _ => 0.0,
        };
        for (i, delay) in DELAYS.iter().enumerate() {
            let height = if self.motion {
                // 3 px, 10 px half-way, ease-in-out.
                let phase = ((t + delay) / CYCLE).fract();
                3.0 + 7.0 * (0.5 - 0.5 * (std::f32::consts::TAU * phase).cos())
            } else {
                7.0
            };
            let x = i as f32 * 3.0;
            f.fill(
                &kit::rounded(
                    Rectangle::new(
                        iced::Point::new(x, bounds.height - height),
                        Size::new(2.0, height),
                    ),
                    1.0,
                ),
                ACCENT.get(),
            );
        }
        vec![f.into_geometry()]
    }
}
