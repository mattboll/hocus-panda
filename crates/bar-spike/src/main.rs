//! Throwaway spike for issue #315: part of the bar (docs/design/bar.md)
//! drawn with iced 0.14 as a wlr-layer-shell client through iced_layershell.
//!
//! Keys while the bar has the keyboard (click it first):
//! `b` switches full/mini, `a` agent waiting (halo amber), `m` meeting
//! (blue), `n` nothing, `r` one more wave, Tab moves the focus ring, Enter
//! opens the focused item's popover, Esc closes the popover.
//! A click on the 8 px strip at the screen edge (plus the bar's first 6 px)
//! also switches the form; a click on the battery gauge opens its popover.
//!
//! `BAR_TRACE=1` logs every draw and every popover decision with its time
//! since the process started, for the measurements in
//! docs/design/spike-iced.md.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use chrono::{Datelike, Local, Timelike};
use iced::animation::{Animation, Easing};
use iced::font::{Family, Weight};
use iced::mouse;
use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke};
use iced::widget::{
    Space, column, container, image, mouse_area, row, scrollable, space, stack, text, text_input,
};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Font, Length, Padding, Point, Rectangle,
    Renderer, Shadow, Size, Subscription, Task, Theme, Vector, event, gradient, keyboard, window,
};
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use iced_layershell::to_layer_message;

// --- Geometry (logical px, docs/design/bar.md sections 2, 3, 9) ---------

/// Room left of the bar for the halo's outer glow (34 px) and the rest
/// shadow: the surface is wider than the bar, its input region is not.
const GLOW: f32 = 44.0;
/// The bar's distance from the screen's top, right and bottom edges.
const EDGE: f32 = 8.0;
const FULL: f32 = 196.0;
const MINI: f32 = 56.0;
const RADIUS: f32 = 14.0;
const SURFACE_W: f32 = GLOW + FULL + EDGE;
const POPOVER_W: f32 = 280.0;
const POPOVER_H: f32 = 280.0;
/// The popover's bottom, from the screen's bottom: aligned with the
/// battery group's bottom.
const POPOVER_BOTTOM: f32 = EDGE + 25.0;
/// The battery group's height: the gauge row, 7 px, the time-left line.
const BATTERY_H: f32 = 42.0;
/// How long the popover outlives the pointer once it really left.
const GRACE: Duration = Duration::from_millis(300);

// --- Theme tokens (section 13) ------------------------------------------

const TEXT: Color = rgb(0xEE, 0xF1, 0xF6);
const DIM: Color = rgb(0xA9, 0xB2, 0xC2);
const FAINT: Color = rgb(0x8C, 0x96, 0xA8);
const FAINTER: Color = rgb(0x7C, 0x86, 0x98);
const ACCENT: Color = rgb(0x6C, 0xC2, 0x87);
const WAIT: Color = rgb(0xF5, 0xB4, 0x4A);
const MEETING: Color = rgb(0x8F, 0xB3, 0xFF);
const STUCK: Color = rgb(0xF2, 0x94, 0x5B);

const UNBOUNDED: Font = Font {
    family: Family::Name("Unbounded"),
    ..Font::DEFAULT
};
const UNBOUNDED_LIGHT: Font = Font {
    weight: Weight::Light,
    ..UNBOUNDED
};
const MANROPE: Font = Font {
    family: Family::Name("Manrope"),
    ..Font::DEFAULT
};
const MANROPE_SEMI: Font = Font {
    weight: Weight::Semibold,
    ..MANROPE
};
const MANROPE_BOLD: Font = Font {
    weight: Weight::Bold,
    ..MANROPE
};

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb8(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

fn alpha(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}

// --- Easing (CSS cubic-bezier, solved for x by Newton then bisection) ---

fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    let bez = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let dbez = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    let x = x.clamp(0.0, 1.0);
    let mut t = x;
    for _ in 0..8 {
        let err = bez(t, x1, x2) - x;
        let d = dbez(t, x1, x2);
        if err.abs() < 1e-5 {
            return bez(t, y1, y2);
        }
        if d.abs() < 1e-6 {
            break;
        }
        t -= err / d;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    t = x;
    for _ in 0..30 {
        let v = bez(t, x1, x2);
        if (v - x).abs() < 1e-5 {
            break;
        }
        if v < x {
            lo = t;
        } else {
            hi = t;
        }
        t = (lo + hi) / 2.0;
    }
    bez(t, y1, y2)
}

/// The form switch: 240 ms, cubic-bezier(0.2, 0.9, 0.25, 1.08).
fn form_ease(x: f32) -> f32 {
    cubic_bezier(0.2, 0.9, 0.25, 1.08, x)
}

/// The wave: 600 ms, cubic-bezier(0.3, 0.6, 0.3, 1).
fn wave_ease(x: f32) -> f32 {
    cubic_bezier(0.3, 0.6, 0.3, 1.0, x)
}

// --- Tracing --------------------------------------------------------------

static START: OnceLock<Instant> = OnceLock::new();
static DRAWS: AtomicU64 = AtomicU64::new(0);

fn since_start() -> f64 {
    START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0
}

fn tracing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("BAR_TRACE").is_some())
}

macro_rules! trace {
    ($($arg:tt)*) => {
        if tracing() {
            eprintln!("[bar {:>10.1} ms] {}", since_start(), format!($($arg)*));
        }
    };
}

// --- State ------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Form {
    Full,
    Mini,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Halo {
    None,
    Agent,
    Meeting,
}

impl Halo {
    fn tint(self) -> Option<Color> {
        match self {
            Halo::None => None,
            Halo::Agent => Some(WAIT),
            Halo::Meeting => Some(MEETING),
        }
    }
}

/// The bar's keyboard focus ring: iced 0.14 only focuses text inputs, so
/// the bar keeps its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Meeting,
    Working,
    Waiting,
    Resting,
    Battery,
}

const ITEMS: [Item; 5] = [
    Item::Meeting,
    Item::Working,
    Item::Waiting,
    Item::Resting,
    Item::Battery,
];

/// What keeps the popover open.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Hold {
    /// The pointer is on the item that opened it, or on the popover.
    Inside,
    /// The pointer moves toward the popover, inside the cone from its last
    /// position to the popover's near corners.
    Aiming { until: Instant },
    /// The pointer left: the popover closes at `until` unless it comes back.
    Leaving { until: Instant },
}

#[derive(Debug)]
struct Popover {
    id: window::Id,
    hold: Hold,
    /// Tells stale close timers apart.
    generation: u64,
}

struct Bar {
    bar_id: Option<window::Id>,
    bar_h: f32,
    now: Instant,
    form: Form,
    /// false = mini, true = full; drives the width.
    width: Animation<bool>,
    switched_at: Option<Instant>,
    halo: Halo,
    /// The last halo with a colour, so the glow fades out in its colour.
    last_tint: Color,
    glow: Animation<bool>,
    wave: Animation<bool>,
    wave_tint: Color,
    focus: Option<Item>,
    popover: Option<Popover>,
    /// The pointer on the bar's surface, in its coordinates.
    cursor: Option<Point>,
    draft: String,
    tasks: Vec<String>,
    pandas: [image::Handle; 3],
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    Tick,
    Frame(Instant),
    Opened(window::Id, Size),
    Resized(window::Id, Size),
    Moved(window::Id, Point),
    Entered(window::Id),
    Left(window::Id),
    Pressed(window::Id),
    Key(window::Id, keyboard::Key, bool),
    BatteryClicked,
    Deadline(u64),
    Draft(String),
    AddTask,
}

const TASK_INPUT: &str = "task-input";

fn main() -> Result<(), iced_layershell::Error> {
    let _ = START.set(Instant::now());
    trace!("process started");
    let fonts = vec![
        include_bytes!("../assets/Unbounded-wght.ttf")
            .as_slice()
            .into(),
        // Manrope's variable font is never matched (its default instance
        // is ExtraLight 200; text falls back to the system's sans): static
        // instances made with fontTools' instancer are.
        include_bytes!("../assets/Manrope-400.ttf")
            .as_slice()
            .into(),
        include_bytes!("../assets/Manrope-600.ttf")
            .as_slice()
            .into(),
        include_bytes!("../assets/Manrope-700.ttf")
            .as_slice()
            .into(),
    ];
    iced_layershell::daemon(Bar::new, || "hocus-bar".to_string(), Bar::update, Bar::view)
        .style(app_style)
        .theme(app_theme)
        .subscription(Bar::subscription)
        .settings(Settings {
            fonts,
            default_font: MANROPE,
            default_text_size: 13.into(),
            antialiasing: true,
            layer_settings: LayerShellSettings {
                anchor: Anchor::Right | Anchor::Top | Anchor::Bottom,
                layer: Layer::Top,
                size: Some((SURFACE_W as u32, 0)),
                exclusive_zone: (FULL + EDGE) as i32,
                margin: (0, 0, 0, 0),
                keyboard_interactivity: KeyboardInteractivity::OnDemand,
                start_mode: StartMode::Active,
                events_transparent: false,
            },
            ..Settings::default()
        })
        .run()
}

fn app_style(_: &Bar, _: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: TEXT,
    }
}

fn app_theme(_: &Bar, _: window::Id) -> Option<Theme> {
    Some(Theme::Dark)
}

impl Bar {
    fn new() -> Self {
        let now = Instant::now();
        let start_form = match std::env::var("BAR_FORM").as_deref() {
            Ok("mini") => Form::Mini,
            _ => Form::Full,
        };
        let halo = match std::env::var("BAR_HALO").as_deref() {
            Ok("agent") => Halo::Agent,
            Ok("meeting") => Halo::Meeting,
            _ => Halo::None,
        };
        let png = |bytes: &'static [u8]| image::Handle::from_bytes(bytes);
        Self {
            bar_id: None,
            bar_h: 1080.0,
            now,
            form: start_form,
            width: Animation::new(start_form == Form::Full)
                .duration(Duration::from_millis(240))
                .easing(Easing::Custom(form_ease)),
            switched_at: None,
            halo,
            last_tint: halo.tint().unwrap_or(WAIT),
            glow: Animation::new(halo != Halo::None).duration(Duration::from_millis(600)),
            wave: Animation::new(false)
                .duration(Duration::from_millis(600))
                .easing(Easing::Custom(wave_ease)),
            wave_tint: WAIT,
            focus: None,
            popover: None,
            cursor: None,
            draft: String::new(),
            tasks: vec!["Rappeler 07 60 27".into(), "Tag v0.2".into()],
            // iced samples images bilinearly without mipmaps: a 472 px
            // drawing shrunk to 48 px aliases. Pre-scaled copies (Lanczos)
            // at the output's pixel size look like the browser's;
            // BAR_PANDA=raw shows the difference.
            pandas: if std::env::var("BAR_PANDA").as_deref() == Ok("raw") {
                [
                    png(include_bytes!("../assets/panda-work.png")),
                    png(include_bytes!("../assets/panda-wait.png")),
                    png(include_bytes!("../assets/panda-sleep.png")),
                ]
            } else {
                [
                    png(include_bytes!("../assets/panda-work-48.png")),
                    png(include_bytes!("../assets/panda-wait-48.png")),
                    png(include_bytes!("../assets/panda-sleep-48.png")),
                ]
            },
        }
    }

    fn animating(&self) -> bool {
        let now = Instant::now();
        self.width.is_animating(now)
            || self.glow.is_animating(now)
            || self.wave.is_animating(now)
            || self
                .switched_at
                .is_some_and(|t| now.duration_since(t) < Duration::from_millis(240))
    }

    fn subscription(&self) -> Subscription<Message> {
        let events = event::listen_with(|event, status, id| match event {
            Event::Window(window::Event::Opened { size, .. }) => Some(Message::Opened(id, size)),
            Event::Window(window::Event::Resized(size)) => Some(Message::Resized(id, size)),
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                Some(Message::Moved(id, position))
            }
            Event::Mouse(mouse::Event::CursorLeft) => Some(Message::Left(id)),
            Event::Mouse(mouse::Event::CursorEntered) => Some(Message::Entered(id)),
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                Some(Message::Pressed(id))
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) => {
                Some(Message::Key(id, key, status == event::Status::Captured))
            }
            _ => None,
        });
        // At rest: one tick a second for the seconds. While something
        // moves: one message per frame the compositor asks for.
        let clock = iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick);
        if self.animating() {
            Subscription::batch([events, clock, window::frames().map(Message::Frame)])
        } else {
            Subscription::batch([events, clock])
        }
    }

    fn bar_width(&self) -> f32 {
        self.width.interpolate(MINI, FULL, self.now)
    }

    /// The bar's own rectangle in its surface.
    fn bar_rect(&self) -> Rectangle {
        let w = self.bar_width();
        Rectangle::new(
            Point::new(SURFACE_W - EDGE - w, EDGE),
            Size::new(w, self.bar_h - 2.0 * EDGE),
        )
    }

    /// The battery group in the bar's surface (full form): the gauge row
    /// and its time-left line, above the bottom padding.
    fn battery_rect(&self) -> Rectangle {
        let bar = self.bar_rect();
        // 10 px of padding, then the 1 px draw counter and its 14 px gap.
        let bottom = bar.y + bar.height - 25.0;
        Rectangle::new(
            Point::new(bar.x + 12.0, bottom - BATTERY_H),
            Size::new(bar.width - 24.0, BATTERY_H),
        )
    }

    /// The popover's near corners (top and bottom of its edge on the bar's
    /// side), in the bar's surface: its surface covers the 8 px gap, so the
    /// near edge is the bar's left edge.
    fn popover_corners(&self) -> (Point, Point) {
        let x = SURFACE_W - EDGE - FULL;
        let bottom = self.bar_h - POPOVER_BOTTOM;
        (Point::new(x, bottom - POPOVER_H), Point::new(x, bottom))
    }

    fn input_region_task(&self) -> Task<Message> {
        let Some(id) = self.bar_id else {
            return Task::none();
        };
        let w = if self.form == Form::Full { FULL } else { MINI };
        let h = self.bar_h;
        let x = (SURFACE_W - EDGE - w) as i32;
        Task::done(Message::SetInputRegion {
            id,
            callback: iced_layershell::actions::ActionCallback::new(move |region| {
                // The bar and the edge strip; the glow's room lets clicks
                // through to the windows.
                region.add(x, 0, (w + EDGE) as i32, h as i32);
            }),
        })
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        match message {
            Message::Tick => Task::none(),
            Message::Frame(at) => {
                self.now = at.max(self.now);
                Task::none()
            }
            Message::Opened(id, size) => {
                if self.popover.as_ref().is_some_and(|p| p.id == id) {
                    trace!("popover opened {size:?}");
                    return Task::none();
                }
                trace!("bar surface opened {size:?}");
                self.bar_id = Some(id);
                self.bar_h = size.height;
                let mut tasks = vec![self.input_region_task()];
                if self.halo != Halo::None {
                    tasks.push(self.start_wave());
                }
                Task::batch(tasks)
            }
            Message::Resized(id, size) => {
                if Some(id) == self.bar_id {
                    self.bar_h = size.height;
                    return self.input_region_task();
                }
                Task::none()
            }
            Message::Moved(id, p) => self.pointer_moved(id, p),
            Message::Left(id) => self.pointer_left(id),
            // iced reports the entry without its position, and no motion
            // follows a pointer that jumps onto the popover and stays.
            Message::Entered(id) if self.popover.as_ref().is_some_and(|p| p.id == id) => {
                trace!("pointer entered the popover");
                self.set_hold(Hold::Inside)
            }
            Message::Entered(_) => Task::none(),
            Message::Pressed(id) => {
                if Some(id) == self.bar_id
                    && let Some(p) = self.cursor
                    && p.x >= SURFACE_W - EDGE - 6.0
                {
                    trace!("edge click at {p:?}");
                    return self.toggle_form();
                }
                Task::none()
            }
            Message::Key(id, key, captured) => self.key(id, key, captured),
            Message::BatteryClicked => {
                self.focus = Some(Item::Battery);
                self.open_popover()
            }
            Message::Deadline(generation) => {
                let Some(pop) = &self.popover else {
                    return Task::none();
                };
                if pop.generation != generation {
                    return Task::none();
                }
                match pop.hold {
                    Hold::Aiming { until } | Hold::Leaving { until } if until <= self.now => {
                        trace!(
                            "popover closes: the pointer left 300 ms ago ({:?})",
                            pop.hold
                        );
                        self.close_popover()
                    }
                    Hold::Aiming { until } | Hold::Leaving { until } => {
                        self.schedule(until - self.now)
                    }
                    Hold::Inside => Task::none(),
                }
            }
            Message::Draft(s) => {
                self.draft = s;
                Task::none()
            }
            Message::AddTask => {
                let t = self.draft.trim().to_string();
                if !t.is_empty() {
                    trace!("task added: {t:?}");
                    self.tasks.push(t);
                }
                self.draft.clear();
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn key(&mut self, id: window::Id, key: keyboard::Key, captured: bool) -> Task<Message> {
        use keyboard::key::Named;
        trace!("key {key:?} on {id:?} captured={captured}");
        match key.as_ref() {
            keyboard::Key::Named(Named::Escape) => {
                if self.popover.is_some() {
                    self.focus = Some(Item::Battery);
                    return self.close_popover();
                }
                self.focus = None;
                Task::none()
            }
            _ if Some(id) != self.bar_id || captured => Task::none(),
            keyboard::Key::Named(Named::Tab) => {
                let i = self
                    .focus
                    .and_then(|f| ITEMS.iter().position(|x| *x == f))
                    .map_or(0, |i| (i + 1) % ITEMS.len());
                self.focus = Some(ITEMS[i]);
                Task::none()
            }
            keyboard::Key::Named(Named::Enter) if self.focus == Some(Item::Battery) => {
                self.open_popover()
            }
            keyboard::Key::Character("b") => self.toggle_form(),
            keyboard::Key::Character("a") => self.set_halo(Halo::Agent),
            keyboard::Key::Character("m") => self.set_halo(Halo::Meeting),
            keyboard::Key::Character("n") => self.set_halo(Halo::None),
            keyboard::Key::Character("r") if self.halo != Halo::None => self.start_wave(),
            _ => Task::none(),
        }
    }

    fn toggle_form(&mut self) -> Task<Message> {
        self.form = match self.form {
            Form::Full => Form::Mini,
            Form::Mini => Form::Full,
        };
        trace!("form switch to {:?}", self.form);
        self.width.go_mut(self.form == Form::Full, self.now);
        self.switched_at = Some(self.now);
        let mut tasks = vec![self.input_region_task()];
        if let Some(id) = self.bar_id {
            let w = if self.form == Form::Full { FULL } else { MINI };
            tasks.push(Task::done(Message::ExclusiveZoneChange {
                id,
                zone_size: (w + EDGE) as i32,
            }));
        }
        if self.form == Form::Mini && self.popover.is_some() {
            tasks.push(self.close_popover());
        }
        Task::batch(tasks)
    }

    fn set_halo(&mut self, halo: Halo) -> Task<Message> {
        if halo == self.halo {
            return Task::none();
        }
        trace!("halo {:?} -> {:?}", self.halo, halo);
        self.halo = halo;
        let on = halo != Halo::None;
        if let Some(t) = halo.tint() {
            self.last_tint = t;
        }
        let ms = if on { 600 } else { 500 };
        self.glow = self
            .glow
            .clone()
            .duration(Duration::from_millis(ms))
            .go(on, self.now);
        if on { self.start_wave() } else { Task::none() }
    }

    fn start_wave(&mut self) -> Task<Message> {
        if let Some(t) = self.halo.tint() {
            self.wave_tint = t;
        }
        trace!("wave starts");
        self.wave = Animation::new(false)
            .duration(Duration::from_millis(600))
            .easing(Easing::Custom(wave_ease))
            .go(true, self.now);
        Task::none()
    }

    // --- The popover and its cone ---------------------------------------

    fn open_popover(&mut self) -> Task<Message> {
        if self.popover.is_some() || self.form != Form::Full {
            return Task::none();
        }
        let settings = NewLayerShellSettings {
            size: Some(((POPOVER_W + EDGE) as u32, POPOVER_H as u32)),
            layer: Layer::Overlay,
            anchor: Anchor::Right | Anchor::Bottom,
            exclusive_zone: Some(-1),
            margin: Some((0, (FULL + EDGE) as i32, POPOVER_BOTTOM as i32, 0)),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            namespace: Some("hocus-popover".into()),
            ..NewLayerShellSettings::default()
        };
        let (id, task) = Message::layershell_open(settings);
        trace!("popover opening {id:?}");
        self.popover = Some(Popover {
            id,
            hold: Hold::Inside,
            generation: 0,
        });
        task
    }

    fn close_popover(&mut self) -> Task<Message> {
        match self.popover.take() {
            Some(pop) => {
                trace!("popover closed");
                Task::done(Message::RemoveWindow(pop.id))
            }
            None => Task::none(),
        }
    }

    fn schedule(&self, after: Duration) -> Task<Message> {
        let Some(pop) = &self.popover else {
            return Task::none();
        };
        let generation = pop.generation;
        Task::perform(tokio::time::sleep(after), move |()| {
            Message::Deadline(generation)
        })
    }

    fn set_hold(&mut self, hold: Hold) -> Task<Message> {
        let now = self.now;
        let Some(pop) = &mut self.popover else {
            return Task::none();
        };
        // Leaving keeps its first deadline: moving about outside does not
        // postpone the close.
        let hold = match (pop.hold, hold) {
            (Hold::Leaving { until }, Hold::Leaving { .. }) => Hold::Leaving { until },
            _ => hold,
        };
        if pop.hold == hold {
            return Task::none();
        }
        if std::mem::discriminant(&pop.hold) != std::mem::discriminant(&hold) {
            trace!(
                "popover hold {:?} -> {:?} (pointer {:?})",
                pop.hold, hold, self.cursor
            );
        }
        pop.hold = hold;
        pop.generation += 1;
        match hold {
            Hold::Inside => Task::none(),
            Hold::Aiming { until } | Hold::Leaving { until } => {
                self.schedule(until.saturating_duration_since(now))
            }
        }
    }

    fn pointer_moved(&mut self, id: window::Id, p: Point) -> Task<Message> {
        if self.popover.as_ref().is_some_and(|pop| pop.id == id) {
            return self.set_hold(Hold::Inside);
        }
        if Some(id) != self.bar_id {
            return Task::none();
        }
        let previous = self.cursor.replace(p);
        if self.popover.is_none() {
            return Task::none();
        }
        if self.battery_rect().contains(p) {
            return self.set_hold(Hold::Inside);
        }
        let (top, bottom) = self.popover_corners();
        // iced reports no position with the pointer's entry: a pointer that
        // entered on the item and clicked without moving has none yet.
        let from = previous.unwrap_or_else(|| self.battery_rect().center());
        let aiming = in_triangle(p, from, top, bottom, 2.0);
        trace!("pointer {:.0},{:.0} aiming={aiming}", p.x, p.y);
        if aiming {
            self.set_hold(Hold::Aiming {
                until: self.now + GRACE,
            })
        } else {
            self.set_hold(Hold::Leaving {
                until: self.now + GRACE,
            })
        }
    }

    fn pointer_left(&mut self, id: window::Id) -> Task<Message> {
        if Some(id) == self.bar_id {
            self.cursor = None;
        }
        let Some(pop) = &self.popover else {
            return Task::none();
        };
        trace!("pointer left {id:?}");
        match pop.hold {
            // Toward the popover: its surface takes the pointer next.
            Hold::Aiming { .. } => Task::none(),
            _ => self.set_hold(Hold::Leaving {
                until: self.now + GRACE,
            }),
        }
    }

    // --- Views ------------------------------------------------------------

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        if self.popover.as_ref().is_some_and(|p| p.id == id) {
            return self.popover_view();
        }
        self.bar_view()
    }

    fn bar_view(&self) -> Element<'_, Message> {
        let now = self.now;
        let w = self.bar_width();
        let glow = self.glow.interpolate(0.0f32, 1.0, now);
        let tint = self.last_tint;
        let border = mix(rgba(255, 255, 255, 0.10), alpha(tint, 0.85), glow);
        let shadow = if glow > 0.0 {
            Shadow {
                color: alpha(tint, 0.45 * glow),
                offset: Vector::ZERO,
                blur_radius: 34.0,
            }
        } else {
            Shadow {
                color: rgba(0, 0, 0, 0.35),
                offset: Vector::new(0.0, 18.0),
                blur_radius: 50.0,
            }
        };

        // Cross-fade: the old content holds 60 ms, then both fade over 120.
        let fade = self.switched_at.map_or(1.0, |t| {
            let ms = now.duration_since(t).as_secs_f32() * 1000.0;
            ((ms - 60.0) / 120.0).clamp(0.0, 1.0)
        });
        let (full_a, mini_a) = match self.form {
            Form::Full => (fade, 1.0 - fade),
            Form::Mini => (1.0 - fade, fade),
        };
        let mut layers: Vec<Element<'_, Message>> = Vec::new();
        if full_a > 0.0 {
            // While the bar narrows, the full content keeps its 196 px and
            // is clipped instead of reflowing (iced shrinks a fixed width to
            // its parent's): a horizontal scrollable with no scrollbar is
            // the only stock widget that lets a child be wider than itself.
            layers.push(
                scrollable(
                    container(self.full_content(full_a))
                        .width(FULL)
                        .height(Length::Fill),
                )
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::hidden(),
                ))
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            );
        }
        if mini_a > 0.0 {
            layers.push(
                container(self.mini_content(mini_a))
                    .width(MINI)
                    .height(Length::Fill)
                    .into(),
            );
        }

        let wave = self.wave.is_animating(now).then(|| {
            let p = self.wave.interpolate(0.0f32, 1.0, now);
            (p, wave_opacity(p), self.wave_tint)
        });
        let overlay = canvas::Canvas::new(Overlay { tint, glow, wave })
            .width(Length::Fill)
            .height(Length::Fill);

        let body = container(stack(layers).width(Length::Fill).height(Length::Fill))
            .width(w)
            .height(Length::Fill)
            .clip(true)
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    gradient::Linear::new(iced::Degrees(180.0))
                        .add_stop(0.0, rgba(22, 27, 42, 0.70))
                        .add_stop(1.0, rgba(14, 17, 28, 0.78))
                        .into(),
                )),
                border: Border {
                    color: border,
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..container::Style::default()
            });
        // The shadow on a container of its own around the stack: a stack
        // draws its layers clipped to its bounds, which cut the glow off.
        // iced also draws a shadow under the whole shape, not only outside
        // it as CSS does: under the translucent glass it tints the inside.
        let bar = container(stack![body, overlay].width(w).height(Length::Fill))
            .width(w)
            .height(Length::Fill)
            .style(move |_| container::Style {
                border: Border::default().rounded(RADIUS),
                shadow,
                ..container::Style::default()
            });

        container(row![space::horizontal(), bar])
            .padding(Padding {
                top: EDGE,
                right: EDGE,
                bottom: EDGE,
                left: 0.0,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn ring(&self, item: Item) -> Border {
        if self.focus == Some(item) {
            Border {
                color: ACCENT,
                width: 2.0,
                radius: 10.0.into(),
            }
        } else {
            Border::default()
        }
    }

    fn full_content(&self, a: f32) -> Element<'_, Message> {
        let (hhmm, secs, date) = clock_texts();

        // No baseline alignment in iced: bottom-aligned with line height 1,
        // the seconds need 3 px to sit on the big digits' baseline
        // (Unbounded's descender is 0.245 em: 0.125 em under the box).
        let clock = column![
            row![
                text(hhmm)
                    .font(UNBOUNDED)
                    .size(38)
                    .line_height(1.0)
                    .color(alpha(TEXT, a)),
                container(
                    text(secs)
                        .font(UNBOUNDED_LIGHT)
                        .size(15)
                        .line_height(1.0)
                        .color(alpha(FAINT, a))
                )
                .padding(Padding::ZERO.bottom(3.0)),
            ]
            .spacing(3)
            .align_y(Alignment::End),
            text(date).size(13).color(alpha(DIM, a)),
        ]
        .spacing(2);

        let meeting = container(
            row![
                container(Space::new().width(6).height(26)).style(move |_| container::Style {
                    background: Some(alpha(MEETING, a).into()),
                    border: Border::default().rounded(3),
                    ..container::Style::default()
                }),
                column![
                    text("Point équipe")
                        .font(MANROPE_BOLD)
                        .size(12.5)
                        .line_height(1.25)
                        .wrapping(text::Wrapping::None)
                        .color(alpha(TEXT, a)),
                    text("15:00 · dans 23 min")
                        .size(12.5)
                        .line_height(1.25)
                        .color(alpha(rgb(0xB9, 0xC6, 0xE6), a)),
                ],
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding([7, 9])
        .width(Length::Fill)
        .style({
            let ring = self.ring(Item::Meeting);
            move |_| container::Style {
                background: Some(rgba(255, 255, 255, 0.05 * a).into()),
                border: if ring.width > 0.0 {
                    ring
                } else {
                    Border::default().rounded(10)
                },
                ..container::Style::default()
            }
        });

        let header = |l: &'static str, r: &'static str| {
            row![
                text(l).font(MANROPE_BOLD).size(11.5).color(alpha(FAINT, a)),
                space::horizontal(),
                text(r).font(MANROPE_BOLD).size(11.5).color(alpha(FAINT, a)),
            ]
        };
        let tile = |item: Item, panda: usize, count: &'static str, color: Color, waiting: bool| {
            let ring = self.ring(item);
            container(
                column![
                    image(self.pandas[panda].clone())
                        .width(48)
                        .height(46)
                        .content_fit(iced::ContentFit::Contain)
                        .opacity(a),
                    text(count)
                        .font(UNBOUNDED)
                        .size(15)
                        .line_height(1.2)
                        .color(alpha(color, a)),
                ]
                .align_x(Alignment::Center),
            )
            .padding(Padding {
                top: 4.0,
                right: 0.0,
                bottom: 6.0,
                left: 0.0,
            })
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .style(move |_| container::Style {
                background: Some(if waiting {
                    rgba(245, 180, 74, 0.08 * a).into()
                } else {
                    rgba(255, 255, 255, 0.05 * a).into()
                }),
                border: if ring.width > 0.0 {
                    ring
                } else if waiting {
                    Border {
                        color: rgba(245, 180, 74, 0.25 * a),
                        width: 1.0,
                        radius: 10.0.into(),
                    }
                } else {
                    Border::default().rounded(10)
                },
                ..container::Style::default()
            })
        };
        let line = |dot: Color, name: &'static str, rest: &'static str, c: Color, c2: Color| {
            row![
                container(Space::new().width(6).height(6)).style(move |_| container::Style {
                    background: Some(alpha(dot, a).into()),
                    border: Border::default().rounded(3),
                    ..container::Style::default()
                }),
                text(name).font(MANROPE_BOLD).size(12).color(alpha(c, a)),
                text(rest).size(12).color(alpha(c2, a)),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
        };
        let agents = column![
            header("AGENTS", "13"),
            row![
                tile(Item::Working, 0, "6", TEXT, false),
                tile(Item::Waiting, 1, "2", rgb(0xF5, 0xC4, 0x6E), true),
                tile(Item::Resting, 2, "4", DIM, false),
            ]
            .spacing(4),
            line(
                WAIT,
                "hocus-panda",
                "· 1 min",
                rgb(0xF5, 0xC4, 0x6E),
                rgb(0xC9, 0xB4, 0x8D)
            ),
            line(
                STUCK,
                "admin",
                "· bloqué",
                rgb(0xF7, 0xA7, 0x7A),
                rgb(0xC9, 0xA0, 0x8A)
            ),
        ]
        .spacing(6);

        let battery = mouse_area(
            container(
                column![
                    row![
                        canvas::Canvas::new(BatteryIcon { a }).width(14).height(14),
                        gauge(0.82, rgb(0x3F, 0xA6, 0x6B), rgb(0x7E, 0xE0, 0xA0), a),
                        text("82")
                            .size(11.5)
                            .width(28)
                            .align_x(Alignment::End)
                            .color(alpha(DIM, a)),
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    text("3 h 40 · performance")
                        .size(11)
                        .width(Length::Fill)
                        .align_x(Alignment::End)
                        .color(alpha(FAINTER, a)),
                ]
                .spacing(7),
            )
            .padding(2)
            .style({
                let ring = self.ring(Item::Battery);
                move |_| container::Style {
                    border: ring,
                    ..container::Style::default()
                }
            }),
        )
        .on_press(Message::BatteryClicked)
        .interaction(mouse::Interaction::Pointer);

        column![
            clock,
            meeting,
            agents,
            space::vertical(),
            battery,
            canvas::Canvas::new(DrawCounter).width(1).height(1),
        ]
        .spacing(14)
        .padding(Padding {
            top: 14.0,
            right: 12.0,
            bottom: 10.0,
            left: 12.0,
        })
        .into()
    }

    fn mini_content(&self, a: f32) -> Element<'_, Message> {
        let (hhmm, _, _) = clock_texts();
        let (hh, mm) = hhmm.split_at(2);
        let (hh, mm) = (hh.to_string(), mm[1..].to_string());
        let clock = column![
            text(hh)
                .font(UNBOUNDED)
                .size(17)
                .line_height(1.05)
                .color(alpha(TEXT, a)),
            text(mm)
                .font(UNBOUNDED)
                .size(17)
                .line_height(1.05)
                .color(alpha(TEXT, a)),
        ]
        .align_x(Alignment::Center);
        let mark = container(Space::new().width(30).height(4)).style(move |_| container::Style {
            background: Some(alpha(MEETING, a).into()),
            border: Border::default().rounded(2),
            ..container::Style::default()
        });
        let waiting = container(
            image(self.pandas[1].clone())
                .width(40)
                .height(38)
                .content_fit(iced::ContentFit::Contain)
                .opacity(a),
        )
        .center(44)
        .style(move |_| container::Style {
            background: Some(rgba(245, 180, 74, 0.08 * a).into()),
            border: Border {
                color: rgba(245, 180, 74, 0.25 * a),
                width: 1.0,
                radius: 12.0.into(),
            },
            ..container::Style::default()
        });
        column![
            clock,
            mark,
            waiting,
            column![
                text("6").font(MANROPE_SEMI).size(11).color(alpha(DIM, a)),
                text("au")
                    .font(MANROPE_SEMI)
                    .size(11)
                    .color(alpha(FAINTER, a)),
                text("travail")
                    .font(MANROPE_SEMI)
                    .size(11)
                    .color(alpha(FAINTER, a)),
            ]
            .align_x(Alignment::Center),
            space::vertical(),
            text("82")
                .font(MANROPE_BOLD)
                .size(11)
                .color(alpha(rgb(0x7E, 0xE0, 0xA0), a)),
            canvas::Canvas::new(DrawCounter).width(1).height(1),
        ]
        .spacing(14)
        .align_x(Alignment::Center)
        .width(Length::Fill)
        .padding(Padding {
            top: 12.0,
            right: 0.0,
            bottom: 10.0,
            left: 0.0,
        })
        .into()
    }

    fn popover_view(&self) -> Element<'_, Message> {
        let profile = |label: &'static str, on: bool| {
            container(
                text(label)
                    .font(MANROPE_SEMI)
                    .size(12)
                    .color(if on { TEXT } else { DIM }),
            )
            .padding([6, 0])
            .width(Length::Fill)
            .align_x(Alignment::Center)
            .style(move |_| container::Style {
                background: Some(if on {
                    rgba(108, 194, 135, 0.18).into()
                } else {
                    rgba(255, 255, 255, 0.05).into()
                }),
                border: Border {
                    color: if on { ACCENT } else { Color::TRANSPARENT },
                    width: 1.0,
                    radius: 8.0.into(),
                },
                ..container::Style::default()
            })
        };
        let tasks = column(self.tasks.iter().map(|t| {
            row![
                container(Space::new().width(12).height(12)).style(|_| container::Style {
                    border: Border {
                        color: FAINT,
                        width: 1.5,
                        radius: 3.0.into(),
                    },
                    ..container::Style::default()
                }),
                text(t.as_str()).size(12.5),
            ]
            .spacing(7)
            .align_y(Alignment::Center)
            .into()
        }))
        .spacing(6);
        let input = text_input("Ajouter une tâche", &self.draft)
            .id(TASK_INPUT)
            .on_input(Message::Draft)
            .on_submit(Message::AddTask)
            .size(12.5)
            .padding([7, 9])
            .style(|_, status| {
                let focused = matches!(status, text_input::Status::Focused { .. });
                text_input::Style {
                    background: rgba(255, 255, 255, 0.06).into(),
                    border: Border {
                        color: if focused {
                            ACCENT
                        } else {
                            rgba(255, 255, 255, 0.12)
                        },
                        width: 1.0,
                        radius: 8.0.into(),
                    },
                    icon: FAINT,
                    placeholder: FAINTER,
                    value: TEXT,
                    selection: rgba(108, 194, 135, 0.40),
                }
            });
        let card = container(
            column![
                text("Batterie").font(MANROPE_BOLD).size(14),
                text("82 % · 3 h 40 restantes").size(12.5).color(DIM),
                gauge(0.82, rgb(0x3F, 0xA6, 0x6B), rgb(0x7E, 0xE0, 0xA0), 1.0),
                text("PROFIL").font(MANROPE_BOLD).size(11.5).color(FAINT),
                row![
                    profile("Économie", false),
                    profile("Équilibré", false),
                    profile("Perf.", true)
                ]
                .spacing(5),
                text("À FAIRE").font(MANROPE_BOLD).size(11.5).color(FAINT),
                tasks,
                input,
            ]
            .spacing(10),
        )
        .padding(14)
        .width(POPOVER_W)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Gradient(
                gradient::Linear::new(iced::Degrees(180.0))
                    .add_stop(0.0, rgba(22, 27, 42, 0.92))
                    .add_stop(1.0, rgba(14, 17, 28, 0.95))
                    .into(),
            )),
            border: Border {
                color: rgba(255, 255, 255, 0.10),
                width: 1.0,
                radius: RADIUS.into(),
            },
            ..container::Style::default()
        });
        // The 8 px on the bar's side stay transparent but take the pointer:
        // there is no gap between the bar and the popover to fall into.
        row![card, Space::new().width(EDGE).height(Length::Fill)]
            .height(Length::Fill)
            .into()
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: a.a + (b.a - a.a) * t,
    }
}

/// The prototype's hpSweep opacity: 0 → 1 over the first 12 %, back to 0
/// at the end (applied to the eased progress, close enough for a spike).
fn wave_opacity(p: f32) -> f32 {
    if p < 0.12 {
        p / 0.12
    } else {
        (1.0 - (p - 0.12) / 0.88).max(0.0)
    }
}

fn in_triangle(p: Point, a: Point, b: Point, c: Point, slack: f32) -> bool {
    let sign = |p1: Point, p2: Point, p3: Point| {
        (p1.x - p3.x) * (p2.y - p3.y) - (p2.x - p3.x) * (p1.y - p3.y)
    };
    // A pointer that did not move is not moving toward anything.
    if (p.x - a.x).abs() < 0.5 && (p.y - a.y).abs() < 0.5 {
        return false;
    }
    // Widen the triangle a little: a hand does not draw straight lines.
    let b = Point::new(b.x, b.y - slack);
    let c = Point::new(c.x, c.y + slack);
    let d1 = sign(p, a, b);
    let d2 = sign(p, b, c);
    let d3 = sign(p, c, a);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

fn gauge<'a>(value: f32, from: Color, to: Color, a: f32) -> Element<'a, Message> {
    let fill = container(Space::new().width(Length::Fill).height(6))
        .width(Length::FillPortion((value * 100.0) as u16))
        .style(move |_| container::Style {
            background: Some(Background::Gradient(
                gradient::Linear::new(iced::Degrees(90.0))
                    .add_stop(0.0, alpha(from, a))
                    .add_stop(1.0, alpha(to, a))
                    .into(),
            )),
            border: Border::default().rounded(3),
            ..container::Style::default()
        });
    container(row![
        fill,
        Space::new()
            .width(Length::FillPortion(((1.0 - value) * 100.0).round() as u16))
            .height(6)
    ])
    .width(Length::Fill)
    .height(6)
    .style(move |_| container::Style {
        background: Some(rgba(255, 255, 255, 0.10 * a).into()),
        border: Border::default().rounded(3),
        ..container::Style::default()
    })
    .into()
}

/// The clock's texts; `BAR_FAKE_TIME=14:37:46` freezes them as on the
/// mockup, for side-by-side screenshots.
fn clock_texts() -> (String, String, String) {
    if let Ok(fake) = std::env::var("BAR_FAKE_TIME") {
        let (hm, s) = fake.rsplit_once(':').unwrap_or((fake.as_str(), "00"));
        return (
            hm.to_string(),
            s.to_string(),
            "mardi 27 octobre".to_string(),
        );
    }
    let t = Local::now();
    (
        format!("{:02}:{:02}", t.hour(), t.minute()),
        format!("{:02}", t.second()),
        format!(
            "{} {} {}",
            weekday_fr(t.weekday()),
            t.day(),
            month_fr(t.month())
        ),
    )
}

fn weekday_fr(d: chrono::Weekday) -> &'static str {
    use chrono::Weekday::*;
    match d {
        Mon => "lundi",
        Tue => "mardi",
        Wed => "mercredi",
        Thu => "jeudi",
        Fri => "vendredi",
        Sat => "samedi",
        Sun => "dimanche",
    }
}

fn month_fr(m: u32) -> &'static str {
    [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ][(m as usize).saturating_sub(1).min(11)]
}

// --- Canvas pieces ----------------------------------------------------------

/// The halo's inner glow and the wave, clipped to the bar's rounded shape:
/// iced clips containers to rectangles only.
struct Overlay {
    tint: Color,
    glow: f32,
    wave: Option<(f32, f32, Color)>,
}

impl<Message> canvas::Program<Message> for Overlay {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let size = bounds.size();
        if self.glow > 0.0 {
            // Inset glow, 22 px at 18 %: iced has no inset shadow, so
            // eleven 2 px rings fading inward.
            for i in 0..11 {
                let inset = 1.0 + i as f32 * 2.0;
                let k = 1.0 - i as f32 / 11.0;
                let path = Path::rounded_rectangle(
                    Point::new(inset, inset),
                    Size::new(size.width - 2.0 * inset, size.height - 2.0 * inset),
                    (RADIUS - inset).max(0.0).into(),
                );
                frame.stroke(
                    &path,
                    Stroke::default()
                        .with_width(2.0)
                        .with_color(alpha(self.tint, 0.18 * self.glow * k * k * 0.6)),
                );
            }
        }
        if let Some((p, opacity, tint)) = self.wave {
            let band = 0.4 * size.height;
            // translateY(-70 %) → translateY(260 %) of the band's height.
            let top = -0.7 * band + p * (3.3 * band);
            let fill =
                canvas::gradient::Linear::new(Point::new(0.0, top), Point::new(0.0, top + band))
                    .add_stop(0.0, alpha(tint, 0.0))
                    .add_stop(0.5, alpha(tint, 0.38 * opacity))
                    .add_stop(1.0, alpha(tint, 0.0));
            let path = Path::rounded_rectangle(Point::ORIGIN, size, RADIUS.into());
            frame.fill(&path, fill);
        }
        vec![frame.into_geometry()]
    }
}

/// Counts the bar's draws: iced draws a canvas on every redraw of its
/// window.
struct DrawCounter;

impl<Message> canvas::Program<Message> for DrawCounter {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        _renderer: &Renderer,
        _theme: &Theme,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let n = DRAWS.fetch_add(1, Ordering::Relaxed) + 1;
        if n == 1 {
            eprintln!("[bar {:>10.1} ms] first draw", since_start());
        }
        trace!("draw #{n}");
        vec![]
    }
}

struct BatteryIcon {
    a: f32,
}

impl<Message> canvas::Program<Message> for BatteryIcon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        // The mockup's icon: viewBox 28×16 drawn in 14×14.
        let mut frame = Frame::new(renderer, bounds.size());
        let s = 14.0 / 28.0;
        let oy = (14.0 - 16.0 * s) / 2.0;
        let body = Path::rounded_rectangle(
            Point::new(1.0 * s, oy + 2.0 * s),
            Size::new(22.0 * s, 12.0 * s),
            (3.0 * s).into(),
        );
        frame.stroke(
            &body,
            Stroke::default()
                .with_width(1.8 * s)
                .with_color(alpha(TEXT, self.a)),
        );
        let tip = Path::rounded_rectangle(
            Point::new(24.5 * s, oy + 6.0 * s),
            Size::new(2.0 * s, 4.0 * s),
            (1.0 * s).into(),
        );
        frame.fill(&tip, alpha(TEXT, self.a));
        vec![frame.into_geometry()]
    }
}
