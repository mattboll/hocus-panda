//! The music player's popover (s12), from the tile over the desktops: the
//! player in front, its cover, its progress and its buttons, the other
//! players, and its window brought forward.
//!
//! Built as the Wi-Fi's spike (#770): iced's stock widgets (`container`,
//! `column`, `row`, `button`, `slider`, `image`, `text`) on the spike's
//! surface ([`Around`]) and keyboard's ring; a canvas only for the
//! buttons' glyphs and the second's tick while it plays.
//!
//! The keyboard: `Tab` goes through the buttons, `Space` plays or pauses,
//! `←` / `→` move 5 s in the track, `Enter` acts on what holds the ring,
//! `Esc` closes.

use std::{
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use iced::{
    Background, Border, Color, Degrees, Element, Font, Length, Point, Rectangle, Renderer, Theme,
    advanced::widget::{Id, Operation},
    alignment::Vertical,
    gradient, keyboard, mouse,
    widget::{Space, button, canvas, column, container, image, row, slider, space},
};

use super::{
    Event as Seen, Item, Pressed, event, kit,
    wifi::{
        custom::{Around, ring},
        widgets::{boxed, clear, filled, lit, said},
    },
};
use crate::{
    levels::draw::{ACCENT, DIM, FAINT, MANROPE, MANROPE_BOLD, TEXT},
    music::{self, Ask, Player, Words},
    theme::Ink,
};

type El<'a> = Element<'a, Msg>;

const MONO: Font = Font::with_name("Martian Mono");

/// The popover's width, its padding, the gap between its blocks.
pub const WIDTH: u32 = 280;
const PAD: f32 = 14.0;
const GAP: f32 = 12.0;
const HEADER: f32 = 20.0;
const COVER: f32 = 72.0;
const COVER_RADIUS: f32 = 10.0;
const COVER_GAP: f32 = 12.0;
const TITLE: f32 = 14.0;
const ARTIST: f32 = 12.5;
const ALBUM: f32 = 12.0;
const TRACK: f32 = 4.0;
const KNOB: f32 = 12.0;
const TIMES: f32 = 10.5;
const BUTTON: f32 = 36.0;
const MAIN: f32 = 46.0;
const BUTTONS_GAP: f32 = 18.0;
const LABEL: f32 = 11.0;
const OTHER: f32 = 40.0;
const OTHER_COVER: f32 = 28.0;
const MORE: f32 = 13.5;
const MORE_HEIGHT: f32 = 38.0;
/// A line's height, of its font's size.
const LINE: f32 = 1.3;
/// `←` and `→` move this far.
const STEP: Duration = Duration::from_secs(5);

const RAIL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.12));
const ROW_FILL: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.09));
const SEP: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.07));
const ON_ACCENT: Ink = Ink::Dark(Color::from_rgb8(0x0B, 0x10, 0x20));

/// What holds the keyboard's ring.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Previous,
    Play,
    Next,
    Other(usize),
    Open,
}

#[derive(Debug, Default)]
struct Ui {
    focus: Option<Target>,
    /// The track's position while the knob is dragged, seconds.
    dragging: Option<f32>,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// The popover opens: no ring, nothing dragged.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    Act(Target),
    /// The knob dragged to, seconds; let go.
    Drag(f32),
    Release,
    Key(Pressed),
    Pressed,
    Stale,
}

/// Counts the view's builds; the height it took when last laid out.
static BUILDS: AtomicU64 = AtomicU64::new(0);
static NATURAL: AtomicU32 = AtomicU32::new(0);

/// Its height, logical pixels: as the widgets took it, else as counted.
pub fn height() -> u32 {
    let natural = f32::from_bits(NATURAL.load(Ordering::Relaxed));
    if natural > 0.0 {
        return natural.round() as u32;
    }
    let others = music::shown().others.len() as f32;
    let list = if others > 0.0 {
        LABEL * LINE + GAP + others * (OTHER + GAP)
    } else {
        0.0
    };
    (2.0 * (PAD + kit::BORDER)
        + HEADER
        + COVER
        + KNOB
        + 6.0
        + TIMES * LINE
        + MAIN
        + 1.0
        + list
        + MORE_HEIGHT
        + 6.0 * GAP)
        .round() as u32
}

/// The position shown, whole seconds, to know when the view is stale.
fn second(player: Option<&Player>, now: Instant) -> u64 {
    player.map_or(0, |p| p.position_at(now).as_secs())
}

// --- The view -----------------------------------------------------------------------

/// The popover on its surface.
pub fn view<'a>(id: iced::window::Id, glass: bool) -> Element<'a, crate::Message> {
    let shown = music::shown();
    let words = Words::new(crate::clock::time::Lang::of_messages());
    let now = Instant::now();
    let built = second(shown.front.as_ref(), now);
    let bus = shown.front.as_ref().map(|p| p.bus.clone());
    let content = ui(|ui| content(&shown, words, ui, now));
    let around = Around {
        id,
        build: BUILDS.fetch_add(1, Ordering::Relaxed),
        content,
        stale: Box::new(move || {
            let shown = music::shown();
            shown.front.as_ref().map(|p| p.bus.clone()) != bus
                || second(shown.front.as_ref(), Instant::now()) != built
        }),
        on_stale: Box::new(|| Msg::Stale),
        on_key: Box::new(taken),
        on_press: Box::new(|_| Msg::Pressed),
        handle: Box::new(handle),
        pending: Box::new(Vec::<Box<dyn Operation>>::new),
        natural: Box::new(|height| {
            let was = NATURAL.swap(height.to_bits(), Ordering::Relaxed);
            if (f32::from_bits(was) - height).abs() > 0.5 {
                crate::popover::refit(Item::Music);
            }
        }),
        redraw: Box::new(|| {
            crate::popover::send(vec![super::Effect::Wake(hocus_link::POPOVER_NAMESPACE)]);
        }),
        wake: crate::Message::Keys,
    };
    container(around)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(move |_| super::wifi::widgets::background(glass))
        .into()
}

fn content<'a>(shown: &music::Shown, words: Words, ui: &Ui, now: Instant) -> El<'a> {
    let Some(front) = &shown.front else {
        return Space::new().into();
    };
    let focus = ui.focus;
    let mut all = column![].spacing(GAP);
    all = all.push(header(words, front));
    all = all.push(now_playing(front));
    all = all.push(progress(front, ui.dragging, now));
    all = all.push(controls(front, focus));
    all = all.push(rule());
    if !shown.others.is_empty() {
        all = all.push(label(words.other_player(shown.others.len())));
        for (i, other) in shown.others.iter().enumerate() {
            all = all.push(other_row(words, other, i, focus));
        }
    }
    all = all.push(open(words, front, focus));
    container(all)
        .padding(PAD + kit::BORDER)
        .width(Length::Fill)
        .into()
}

/// "Musique", and the source with its dot.
fn header<'a>(words: Words, front: &Player) -> El<'a> {
    let dot = if front.playing() { ACCENT } else { FAINT }.get();
    let source = row![
        container(Space::new().width(6.0).height(6.0)).style(move |_| boxed(dot, 3.0)),
        gap(6.0),
        said(words.source(front), 12.0, MANROPE, FAINT.get()),
    ]
    .align_y(Vertical::Center);
    container(
        row![
            said(words.music(), 15.0, MANROPE_BOLD, TEXT.get()),
            space::horizontal(),
            source
        ]
        .align_y(Vertical::Center),
    )
    .height(HEADER)
    .align_y(Vertical::Center)
    .id(Id::new("music:header"))
    .into()
}

/// The cover, the title on two lines at most, the artist, the album.
fn now_playing<'a>(front: &Player) -> El<'a> {
    let room = WIDTH as f32 - 2.0 * (PAD + kit::BORDER) - COVER - COVER_GAP;
    let semibold = kit::MANROPE_SEMIBOLD;
    let lines = kit::wrap(&front.title, room, |s| kit::measure(s, TITLE, semibold));
    let title = match lines.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [first, rest @ ..] => {
            let second = kit::elide(&rest.join(" "), room, |s| kit::measure(s, TITLE, semibold));
            format!("{first}\n{second}")
        }
    };
    let cut = |text: &str, size: f32| kit::elide(text, room, |s| kit::measure(s, size, MANROPE));
    let mut lines = column![said(title, TITLE, semibold, TEXT.get())].spacing(1.0);
    if !front.artist.is_empty() {
        lines = lines.push(said(cut(&front.artist, ARTIST), ARTIST, MANROPE, DIM.get()));
    }
    if !front.album.is_empty() {
        lines = lines.push(said(cut(&front.album, ALBUM), ALBUM, MANROPE, FAINT.get()));
    }
    container(
        row![
            cover(front, COVER, COVER_RADIUS, true),
            gap(COVER_GAP),
            lines.width(Length::Fill)
        ]
        .align_y(Vertical::Center),
    )
    .id(Id::new("music:now"))
    .into()
}

/// The track (a stock slider: a click or a drag moves in it) and the times.
fn progress<'a>(front: &Player, dragging: Option<f32>, now: Instant) -> El<'a> {
    let length = front.length.map_or(0.0, |l| l.as_secs_f32());
    let at = dragging.unwrap_or_else(|| front.position_at(now).as_secs_f32());
    let seek = front.can_seek && length > 0.0;
    let track = slider(0.0..=length.max(1.0), at.min(length.max(1.0)), Msg::Drag)
        .on_release(Msg::Release)
        .step(0.5_f32)
        .height(KNOB)
        .style(move |_, _| slider::Style {
            rail: slider::Rail {
                backgrounds: (
                    Background::Color(ACCENT.get()),
                    Background::Color(RAIL.get()),
                ),
                width: TRACK,
                border: Border {
                    radius: (TRACK / 2.0).into(),
                    ..Border::default()
                },
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: KNOB / 2.0 },
                background: Background::Color(TEXT.get()),
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        });
    let track: El<'a> = if seek {
        track.into()
    } else {
        // Without seeking, the same track, not to be moved.
        iced::widget::mouse_area(track)
            .on_press(Msg::Pressed)
            .into()
    };
    let elapsed = Duration::from_secs_f32(at.max(0.0));
    let left = front
        .length
        .map(|l| l.saturating_sub(Duration::from_secs(elapsed.as_secs())));
    let times = row![
        said(music::clock(elapsed), TIMES, MONO, FAINT.get()),
        space::horizontal(),
        said(
            left.map_or_else(String::new, |l| format!("−{}", music::clock(l))),
            TIMES,
            MONO,
            FAINT.get()
        ),
    ];
    let mut block = column![track, times].spacing(6.0);
    if front.playing() {
        // The second's tick: its surface alone wakes.
        block = block.push(canvas(Tick).width(0.0).height(0.0));
    }
    container(block).id(Id::new("music:progress")).into()
}

/// Previous, play or pause (the accent, larger), next.
fn controls<'a>(front: &Player, focus: Option<Target>) -> El<'a> {
    let side = |glyph: Glyph, target: Target, can: bool| -> El<'a> {
        let pressed =
            button(container(canvas(glyph).width(18.0).height(18.0)).center(Length::Fill))
                .padding(0)
                .width(BUTTON)
                .height(BUTTON)
                .style(move |_, status| {
                    if can && lit(status) {
                        filled(ROW_HOVER.get(), BUTTON / 2.0, TEXT.get())
                    } else {
                        clear()
                    }
                })
                .on_press_maybe(can.then_some(Msg::Act(target)));
        control(pressed, target, focus, BUTTON / 2.0)
    };
    let can = front.can_control;
    let main = button(
        container(
            canvas(if front.playing() {
                Glyph::Pause(ON_ACCENT.get())
            } else {
                Glyph::Play(ON_ACCENT.get())
            })
            .width(20.0)
            .height(20.0),
        )
        .center(Length::Fill),
    )
    .padding(0)
    .width(MAIN)
    .height(MAIN)
    .style(|_, status| {
        // Lit: 8 % brighter, as the mockup's `brightness(1.08)`.
        let accent = ACCENT.get();
        let fill = if lit(status) {
            Color {
                r: (accent.r * 1.08).min(1.0),
                g: (accent.g * 1.08).min(1.0),
                b: (accent.b * 1.08).min(1.0),
                ..accent
            }
        } else {
            accent
        };
        filled(fill, MAIN / 2.0, ON_ACCENT.get())
    })
    .on_press_maybe(can.then_some(Msg::Act(Target::Play)));
    container(
        row![
            space::horizontal(),
            side(
                Glyph::Previous(if front.can_previous { TEXT } else { FAINT }.get()),
                Target::Previous,
                front.can_previous && can
            ),
            gap(BUTTONS_GAP),
            control(main, Target::Play, focus, MAIN / 2.0),
            gap(BUTTONS_GAP),
            side(
                Glyph::Next(if front.can_next { TEXT } else { FAINT }.get()),
                Target::Next,
                front.can_next && can
            ),
            space::horizontal(),
        ]
        .align_y(Vertical::Center),
    )
    .into()
}

/// The label over the other players, its letters spaced.
fn label<'a>(word: &'static str) -> El<'a> {
    // No letter-spacing in iced's text: a letter a widget.
    let mut letters = row![].spacing(0.08 * LABEL);
    for ch in word.to_uppercase().chars() {
        letters = letters.push(said(
            ch.to_string(),
            LABEL,
            kit::MANROPE_SEMIBOLD,
            FAINT.get(),
        ));
    }
    letters.into()
}

/// Another player: its cover, its title over its artist and app, its state.
fn other_row<'a>(words: Words, other: &Player, i: usize, focus: Option<Target>) -> El<'a> {
    let state = if other.playing() {
        words.playing()
    } else {
        words.paused()
    };
    let aside = kit::measure(state, 11.5, MANROPE);
    let room = WIDTH as f32 - 2.0 * (PAD + kit::BORDER) - 12.0 - OTHER_COVER - 20.0 - aside;
    let title = kit::elide(&other.title, room, |s| {
        kit::measure(s, 12.5, kit::MANROPE_SEMIBOLD)
    });
    let by = if other.artist.is_empty() {
        other.app.clone()
    } else {
        format!("{} · {}", other.artist, other.app)
    };
    let by = kit::elide(&by, room, |s| kit::measure(s, 11.5, MANROPE));
    let line = row![
        cover(other, OTHER_COVER, 5.0, false),
        gap(10.0),
        column![
            said(title, 12.5, kit::MANROPE_SEMIBOLD, TEXT.get()),
            said(by, 11.5, MANROPE, DIM.get())
        ]
        .spacing(1.0)
        .width(Length::Fill),
        gap(10.0),
        said(state, 11.5, MANROPE, FAINT.get()),
    ]
    .align_y(Vertical::Center);
    let pressed = button(container(line).center_y(Length::Fill))
        .padding([0.0, 6.0])
        .width(Length::Fill)
        .height(OTHER)
        .style(|_, status| {
            if lit(status) {
                filled(ROW_FILL.get(), 8.0, TEXT.get())
            } else {
                clear()
            }
        })
        .on_press(Msg::Act(Target::Other(i)));
    control(pressed, Target::Other(i), focus, 8.0)
}

/// "Ouvrir Spotify ›": the player's window, brought forward.
fn open<'a>(words: Words, front: &Player, focus: Option<Target>) -> El<'a> {
    let line = row![
        said(
            words.open(&front.app),
            MORE,
            kit::MANROPE_SEMIBOLD,
            TEXT.get()
        ),
        space::horizontal(),
        said("›", MORE, kit::MANROPE_SEMIBOLD, FAINT.get()),
    ]
    .align_y(Vertical::Center);
    let can = front.can_raise;
    let pressed = button(container(line).center_y(Length::Fill))
        .padding([0.0, 8.0])
        .width(Length::Fill)
        .height(MORE_HEIGHT)
        .style(move |_, status| {
            filled(
                if can && lit(status) {
                    ROW_HOVER
                } else {
                    ROW_FILL
                }
                .get(),
                8.0,
                TEXT.get(),
            )
        })
        .on_press_maybe(can.then_some(Msg::Act(Target::Open)));
    control(pressed, Target::Open, focus, 8.0)
}

/// A divider across the content.
fn rule<'a>() -> El<'a> {
    container(Space::new().width(Length::Fill).height(1.0))
        .style(|_| boxed(SEP.get(), 0.0))
        .into()
}

// --- The pieces ------------------------------------------------------------------------

fn gap<'a>(width: f32) -> El<'a> {
    Space::new().width(width).into()
}

/// A control under the keyboard's ring while it holds it, in a container
/// named after it (tests, and a screen reader's nodes, find it by it).
fn control<'a>(
    content: impl Into<El<'a>>,
    target: Target,
    focus: Option<Target>,
    radius: f32,
) -> El<'a> {
    container(ring(content, focus == Some(target), radius))
        .id(Id::from(format!("music:{target:?}")))
        .into()
}

/// A player's cover at `size`, rounded `radius`; drawn without one.
pub(crate) fn cover<'a, M: 'a>(
    player: &Player,
    size: f32,
    radius: f32,
    big: bool,
) -> Element<'a, M> {
    match &player.cover {
        Some(cover) => image(if big {
            cover.big.clone()
        } else {
            cover.small.clone()
        })
        .width(size)
        .height(size)
        .into(),
        None => container(Space::new().width(size).height(size))
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    gradient::Linear::new(Degrees(135.0))
                        .add_stop(0.0, Color::from_rgb8(0x3A, 0x45, 0x60))
                        .add_stop(1.0, Color::from_rgb8(0x1E, 0x24, 0x36))
                        .into(),
                )),
                border: Border {
                    radius: radius.into(),
                    ..Border::default()
                },
                ..container::Style::default()
            })
            .into(),
    }
}

// --- The glyphs, and the tick ------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub(crate) enum Glyph {
    Previous(Color),
    Next(Color),
    Play(Color),
    Pause(Color),
}

impl<M> canvas::Program<M> for Glyph {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut f = canvas::Frame::new(renderer, bounds.size());
        let s = bounds.width / 24.0;
        // The mockup's paths, on its 24 px grid.
        let shapes: &[&[(f32, f32)]] = match self {
            Self::Previous(_) => &[
                &[(6.0, 5.0), (8.0, 5.0), (8.0, 19.0), (6.0, 19.0)],
                &[(9.5, 12.0), (19.0, 5.0), (19.0, 19.0)],
            ],
            Self::Next(_) => &[
                &[(16.0, 5.0), (18.0, 5.0), (18.0, 19.0), (16.0, 19.0)],
                &[(14.5, 12.0), (5.0, 19.0), (5.0, 5.0)],
            ],
            Self::Play(_) => &[&[(8.0, 5.0), (8.0, 19.0), (19.0, 12.0)]],
            Self::Pause(_) => &[
                &[(7.0, 5.0), (11.0, 5.0), (11.0, 19.0), (7.0, 19.0)],
                &[(13.0, 5.0), (17.0, 5.0), (17.0, 19.0), (13.0, 19.0)],
            ],
        };
        let (Self::Previous(colour)
        | Self::Next(colour)
        | Self::Play(colour)
        | Self::Pause(colour)) = *self;
        for shape in shapes {
            let path = canvas::Path::new(|b| {
                for (i, (x, y)) in shape.iter().enumerate() {
                    let p = Point::new(x * s, y * s);
                    if i == 0 { b.move_to(p) } else { b.line_to(p) }
                }
                b.close();
            });
            f.fill(&path, colour);
        }
        vec![f.into_geometry()]
    }
}

/// Wakes the surface at the next second of the track while it plays: the
/// view is stale then, and built again.
#[derive(Debug, Clone, Copy)]
struct Tick;

impl<M> canvas::Program<M> for Tick {
    type State = ();

    fn update(
        &self,
        _state: &mut (),
        event: &iced::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<M>> {
        let iced::Event::Window(iced::window::Event::RedrawRequested(now)) = event else {
            return None;
        };
        let front = music::shown().front?;
        if !front.playing() {
            return None;
        }
        let at = front.position_at(*now);
        let next = Duration::from_secs(at.as_secs() + 1).saturating_sub(at);
        Some(canvas::Action::request_redraw_at(*now + next))
    }

    fn draw(
        &self,
        _state: &(),
        _renderer: &Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        Vec::new()
    }
}

// --- What a message does ---------------------------------------------------------------

/// What can hold the ring, in `Tab`'s order.
fn order(shown: &music::Shown) -> Vec<Target> {
    let Some(front) = &shown.front else {
        return Vec::new();
    };
    let mut order = Vec::new();
    if front.can_previous && front.can_control {
        order.push(Target::Previous);
    }
    if front.can_control {
        order.push(Target::Play);
    }
    if front.can_next && front.can_control {
        order.push(Target::Next);
    }
    order.extend((0..shown.others.len()).map(Target::Other));
    if front.can_raise {
        order.push(Target::Open);
    }
    order
}

/// The keys the surface keeps from its widgets.
fn taken(pressed: Pressed) -> Option<Msg> {
    use keyboard::{Key, key::Named};
    match &pressed.key {
        Key::Named(
            Named::Escape
            | Named::Tab
            | Named::Space
            | Named::Enter
            | Named::ArrowLeft
            | Named::ArrowRight,
        ) => Some(Msg::Key(pressed)),
        _ => None,
    }
}

fn handle(message: Msg) {
    let shown = music::shown();
    let Some(front) = shown.front.clone() else {
        return;
    };
    tracing::debug!(?message, "the music popover's message");
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Act(target) => act(target, &front, &shown),
        Msg::Drag(to) => ui(|ui| ui.dragging = Some(to)),
        Msg::Release => {
            if let Some(to) = ui(|ui| ui.dragging.take()) {
                music::ask(Ask::Seek {
                    name: front.bus.clone(),
                    to: Duration::from_secs_f32(to.max(0.0)),
                });
            }
        }
        Msg::Key(pressed) => key(&pressed, &front, &shown),
    }
    let order = order(&music::shown());
    ui(|ui| {
        if ui.focus.is_some_and(|f| !order.contains(&f)) {
            ui.focus = None;
        }
    });
}

fn act(target: Target, front: &Player, shown: &music::Shown) {
    match target {
        Target::Previous => music::ask(Ask::Previous(front.bus.clone())),
        Target::Play => music::ask(Ask::PlayPause(front.bus.clone())),
        Target::Next => music::ask(Ask::Next(front.bus.clone())),
        Target::Other(i) => {
            if let Some(other) = shown.others.get(i) {
                music::choose(&other.bus);
                ui(|ui| ui.focus = ui.focus.map(|_| Target::Play));
            }
        }
        Target::Open => {
            music::ask(Ask::Raise(front.bus.clone()));
            event(Seen::Chosen);
        }
    }
}

fn key(pressed: &Pressed, front: &Player, shown: &music::Shown) {
    use keyboard::{Key, key::Named};
    let focus = ui(|ui| ui.focus);
    match &pressed.key {
        Key::Named(Named::Escape) => event(Seen::Escape),
        Key::Named(Named::Tab) => {
            let next = kit::step(&order(shown), focus, pressed.modifiers.shift());
            ui(|ui| ui.focus = next);
        }
        Key::Named(Named::Space) => match focus {
            Some(target) if target != Target::Play => act(target, front, shown),
            _ => act(Target::Play, front, shown),
        },
        Key::Named(Named::Enter) => {
            if let Some(target) = focus {
                act(target, front, shown);
            }
        }
        Key::Named(Named::ArrowLeft | Named::ArrowRight) if front.can_seek => {
            let at = front.position_at(Instant::now());
            let to = if matches!(pressed.key, Key::Named(Named::ArrowRight)) {
                let to = at + STEP;
                front.length.map_or(to, |l| to.min(l))
            } else {
                at.saturating_sub(STEP)
            };
            music::ask(Ask::Seek {
                name: front.bus.clone(),
                to,
            });
        }
        _ => {}
    }
}
