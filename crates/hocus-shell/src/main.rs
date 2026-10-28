//! The Hocuspanda shell: the bar (docs/design/bar.md), drawn with iced as a
//! wlr-layer-shell client of the compositor, which starts it and restarts
//! it when it ends.
//!
//! So far the bar is its column (§2, §3, §13) and its clock (§4 group 1,
//! §5): a glass tint and a rim against the right edge of each output the
//! compositor names, in its full or its mini form, and the time at its top.
//! The compositor blurs what is behind it; the shell draws the tint over
//! that blur, or an opaque background when there is no blur. The line
//! between the two is `hocus-link`.
//!
//! Each bar's surface is the bar itself, 8 px from the screen's edges: as
//! wide as its form, and as the full form while a switch is under way, so
//! that the bar has room to move. The compositor takes the clicks on the
//! bar's edge, which switch the form. The clock has a small surface of its
//! own over the bar's top ([`clock`]): it draws every second, and iced
//! presents a surface whole, so that only its region is drawn again.

mod clock;
mod renderer;

use std::{
    borrow::Cow,
    collections::HashMap,
    process::ExitCode,
    time::{Duration, Instant},
};

use hocus_link::{
    CLOCK_NAMESPACE, ClockLook, EDGE, Form, NAMESPACE, RADIUS, ToCompositor, ToShell,
};
use iced::{
    Background, Border, Color, Element, Font, Length, Size, Subscription, Task, Theme,
    futures::{SinkExt, StreamExt, channel::mpsc},
    gradient,
    widget::{Space, canvas, container, row, space},
    window,
};
use iced_layershell::{
    reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing_subscriber::EnvFilter;

/// A frame drawn now shows about one refresh later: the bar is drawn as it
/// will be then, so that it keeps in step with what the compositor moves
/// (the windows, the glass), which it draws as of the frame it shows.
const AHEAD: Duration = Duration::from_millis(16);

// --- Theme tokens (§13) -------------------------------------------------------

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color::from_rgba8(r, g, b, a)
}

/// `bar.bg` on glass: the tint over the compositor's blur.
const GLASS_TOP: Color = rgba(22, 27, 42, 0.70);
const GLASS_BOTTOM: Color = rgba(14, 17, 28, 0.78);
/// `bar.bg` without glass (reduce transparency, no blur, CPU rendering).
const OPAQUE_TOP: Color = rgba(0x16, 0x1B, 0x2A, 1.0);
const OPAQUE_BOTTOM: Color = rgba(0x0E, 0x11, 0x1C, 1.0);
/// `bar.border`.
const RIM: Color = rgba(255, 255, 255, 0.10);

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn,hocuspanda_shell=info".into()),
        )
        .init();
    // On a CPU, the CPU renderer: this may start the program again.
    let software = renderer::choose();
    let link = std::env::var(hocus_link::SOCKET_ENV).ok();
    if link.is_none() {
        tracing::warn!(
            "{} is not set: a full bar on the active output, for trying the shell alone",
            hocus_link::SOCKET_ENV
        );
    }
    let started = iced_layershell::daemon(
        move || Shell::new(link.clone(), software, clock_shift()),
        || NAMESPACE.to_owned(),
        Shell::update,
        Shell::view,
    )
    .style(style)
    .theme(theme)
    .subscription(Shell::subscription)
    .settings(Settings {
        antialiasing: true,
        fonts: clock::FONTS.iter().map(|f| Cow::Borrowed(*f)).collect(),
        layer_settings: LayerShellSettings {
            start_mode: StartMode::Background,
            ..LayerShellSettings::default()
        },
        ..Settings::default()
    })
    .run();
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the shell stopped");
            ExitCode::FAILURE
        }
    }
}

/// The surfaces are transparent where the bar does not draw.
fn style(_: &Shell, _: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: Color::WHITE,
    }
}

fn theme(_: &Shell, _: window::Id) -> Option<Theme> {
    Some(Theme::Dark)
}

/// `HOCUS_CLOCK_AT`: the time the clock reads as the shell starts, for
/// recordings and tests (bar.md, as built); the system's clock is never
/// touched.
fn clock_shift() -> Option<String> {
    let at = std::env::var("HOCUS_CLOCK_AT").ok()?;
    tracing::info!(at, "the clock reads a time of its own");
    Some(at)
}

/// How bars look, as the compositor says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Look {
    /// The compositor blurs behind: the bar is a tint over it.
    glass: bool,
    /// The form switch animates.
    motion: bool,
}

/// A bar on an output.
#[derive(Debug, Clone)]
struct Bar {
    output: String,
    form: Form,
    /// The form switch under way: from that width, since then.
    switch: Option<(f64, Instant)>,
    /// Its surface's width, as last asked for.
    surface: u32,
    /// Its surface's height, once known.
    height: f32,
    /// Its clock's surface, and its size as last asked for.
    clock: window::Id,
    clock_surface: (u32, u32),
}

impl Bar {
    /// Its width as drawn at `at`.
    fn width(&self, at: Instant) -> f32 {
        match self.switch {
            Some((from, since)) => {
                hocus_link::width_at(from, self.form, at.saturating_duration_since(since)) as f32
            }
            None => self.form.width() as f32,
        }
    }

    /// The clock's surface's size: its form's, the full one's while the
    /// form switches.
    fn clock_size(&self) -> (u32, u32) {
        match (self.switch, self.form) {
            (Some(_), _) | (None, Form::Full) => clock::FULL_SURFACE,
            (None, Form::Mini) => clock::MINI_SURFACE,
        }
    }

    fn moving(&self, now: Instant) -> bool {
        self.switch.is_some_and(|(_, since)| {
            now.saturating_duration_since(since) < hocus_link::SWITCH + AHEAD
        })
    }

    /// The width its surface needs: the full form's while it switches,
    /// whichever way, else its form's.
    fn surface_width(&self) -> u32 {
        let form = if self.switch.is_some() {
            Form::Full
        } else {
            self.form
        };
        form.width().unsigned_abs()
    }
}

#[derive(Debug)]
struct Shell {
    /// The compositor's socket; none when the shell runs alone.
    socket: Option<String>,
    /// Lines to the compositor, once linked.
    to_compositor: Option<mpsc::UnboundedSender<String>>,
    software: bool,
    look: Look,
    bars: HashMap<window::Id, Bar>,
    now: Instant,
    clock: clock::Style,
    wall: clock::time::Wall,
    lang: clock::time::Lang,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    Linked(mpsc::UnboundedSender<String>),
    FromCompositor(String),
    Unlinked,
    Frame,
    /// The system's time zone changed.
    Zone(jiff::tz::TimeZone),
    Resized(window::Id, Size),
}

impl Shell {
    fn new(socket: Option<String>, software: bool, at: Option<String>) -> (Self, Task<Message>) {
        let zone = clock::time::system_zone();
        let shift = at
            .and_then(|at| {
                let shift = clock::time::Wall::shift_to(&at, &zone);
                if shift.is_none() {
                    tracing::warn!(at, "HOCUS_CLOCK_AT: a date and time, or a time, expected");
                }
                shift
            })
            .unwrap_or_default();
        let mut shell = Self {
            socket: socket.clone(),
            to_compositor: None,
            software,
            look: Look {
                glass: !software,
                motion: !software,
            },
            bars: HashMap::new(),
            now: Instant::now(),
            clock: clock_style(&ClockLook::default()),
            wall: clock::time::Wall { zone, shift },
            lang: clock::time::Lang::from_env(),
        };
        // Alone: one full bar where the compositor puts it.
        let task = if socket.is_none() {
            shell.open_bar(OutputOption::Active, "active".to_owned(), Form::Full)
        } else {
            Task::none()
        };
        (shell, task)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
        if let Some(socket) = &self.socket {
            subscriptions.push(Subscription::run_with(socket.clone(), link));
        }
        // Frames only while a bar moves, and one past its switch's end,
        // which narrows its surface: at rest, nothing is drawn.
        if self.bars.values().any(|b| b.switch.is_some()) {
            subscriptions.push(window::frames().map(|_| Message::Frame));
        }
        subscriptions.push(Subscription::run(zone_changes));
        subscriptions.push(window::resize_events().map(|(id, size)| Message::Resized(id, size)));
        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        match message {
            Message::Linked(sender) => {
                if self.software {
                    let _ = sender.unbounded_send(ToCompositor::Software.to_string());
                }
                self.to_compositor = Some(sender);
                Task::none()
            }
            Message::FromCompositor(line) => match line.parse::<ToShell>() {
                Ok(message) => self.compositor_says(message),
                Err(err) => {
                    tracing::warn!(%err, "a line from the compositor that does not read");
                    Task::none()
                }
            },
            Message::Unlinked => {
                tracing::info!("the compositor went: the shell ends");
                iced::exit()
            }
            Message::Frame => {
                // Past the switch's end: back to its form for good.
                let now = self.now;
                let ended: Vec<window::Id> = self
                    .bars
                    .iter_mut()
                    .filter(|(_, bar)| bar.switch.is_some() && !bar.moving(now))
                    .map(|(id, bar)| {
                        bar.switch = None;
                        *id
                    })
                    .collect();
                Task::batch(ended.into_iter().map(|id| self.fit(id)))
            }
            Message::Zone(zone) => {
                tracing::info!(zone = zone.iana_name(), "the time zone changed");
                self.wall.zone = zone;
                Task::none()
            }
            Message::Resized(id, size) => {
                if let Some(bar) = self.bars.get_mut(&id) {
                    bar.height = size.height;
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn compositor_says(&mut self, message: ToShell) -> Task<Message> {
        match message {
            ToShell::Look { glass, motion } => {
                self.look = Look {
                    glass: glass && !self.software,
                    motion: motion && !self.software,
                };
                Task::none()
            }
            ToShell::Bar {
                output,
                form,
                since,
            } => {
                let start = (since != 0).then(|| instant_of(since));
                match self.bars.iter().find(|(_, b)| b.output == output) {
                    Some((id, _)) => self.set_form(*id, form, start),
                    None => self.open_bar(OutputOption::OutputName(output.clone()), output, form),
                }
            }
            ToShell::NoBar { output } => {
                let gone: Vec<window::Id> = self
                    .bars
                    .iter()
                    .filter(|(_, b)| b.output == output)
                    .map(|(id, _)| *id)
                    .collect();
                Task::batch(gone.into_iter().filter_map(|id| {
                    let bar = self.bars.remove(&id)?;
                    Some(Task::batch([
                        Task::done(Message::RemoveWindow(bar.clock)),
                        Task::done(Message::RemoveWindow(id)),
                    ]))
                }))
            }
            ToShell::Clock(look) => {
                self.clock = clock_style(&look);
                Task::none()
            }
        }
    }

    /// A bar's surface on `output`: against its right edge, its height
    /// less the margins; and its clock's, over its top, opened after it so
    /// that it is in front.
    fn open_bar(&mut self, option: OutputOption, output: String, form: Form) -> Task<Message> {
        tracing::info!(output, form = form.name(), "a bar");
        let mut bar = Bar {
            output,
            form,
            switch: None,
            surface: 0,
            height: 0.0,
            clock: window::Id::unique(),
            clock_surface: (0, 0),
        };
        bar.surface = bar.surface_width();
        bar.clock_surface = bar.clock_size();
        let settings = NewLayerShellSettings {
            size: Some((bar.surface, 0)),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Top | Anchor::Bottom,
            // The compositor keeps the bar's space itself, shell or not.
            exclusive_zone: Some(0),
            // Top, right, bottom, left.
            margin: Some((EDGE, EDGE, EDGE, 0)),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(NAMESPACE.to_owned()),
        };
        let (id, open) = Message::layershell_open(settings);
        // Inside the rim; pointer events go through to the bar.
        let clock = NewLayerShellSettings {
            size: Some(bar.clock_surface),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Top,
            exclusive_zone: Some(0),
            margin: Some((EDGE + clock::SURFACE_TOP, EDGE + 1, 0, 0)),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option,
            events_transparent: true,
            namespace: Some(CLOCK_NAMESPACE.to_owned()),
        };
        let (clock_id, open_clock) = Message::layershell_open(clock);
        bar.clock = clock_id;
        self.bars.insert(id, bar);
        open.chain(open_clock)
    }

    /// `form` for bar `id`, switching from where it is since `start`; at
    /// once without motion.
    fn set_form(&mut self, id: window::Id, form: Form, start: Option<Instant>) -> Task<Message> {
        let now = self.now;
        let motion = self.look.motion;
        let Some(bar) = self.bars.get_mut(&id) else {
            return Task::none();
        };
        if bar.form == form {
            return Task::none();
        }
        tracing::debug!(output = bar.output, form = form.name(), "form");
        // A switch reversed midway starts from where the bar is then, as
        // the compositor's does.
        let from = start.map_or(0.0, |start| f64::from(bar.width(start)));
        bar.switch = match start {
            Some(start) if motion && now.saturating_duration_since(start) < hocus_link::SWITCH => {
                Some((from, start))
            }
            _ => None,
        };
        bar.form = form;
        self.fit(id)
    }

    /// Bar `id`'s surface and its clock's to the sizes they need now, if
    /// they changed.
    fn fit(&mut self, id: window::Id) -> Task<Message> {
        let Some(bar) = self.bars.get_mut(&id) else {
            return Task::none();
        };
        let mut tasks = Vec::new();
        let width = bar.surface_width();
        if bar.surface != width {
            bar.surface = width;
            tasks.push(Task::done(Message::SizeChange {
                id,
                size: (width, 0),
            }));
        }
        let size = bar.clock_size();
        if bar.clock_surface != size {
            bar.clock_surface = size;
            tasks.push(Task::done(Message::SizeChange {
                id: bar.clock,
                size,
            }));
        }
        Task::batch(tasks)
    }

    /// The clock of the bar whose clock's surface is `id`.
    fn clock_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.clock == id)?;
        let at = self.now + AHEAD;
        // The contents cross-fade over 120 ms, 60 ms into a switch (§3).
        let forms = match bar.switch {
            Some((_, since)) => {
                let t = at.saturating_duration_since(since).as_secs_f32();
                let new = ((t - 0.060) / 0.120).clamp(0.0, 1.0);
                vec![(bar.form.other(), 1.0 - new), (bar.form, new)]
            }
            None => vec![(bar.form, 1.0)],
        };
        let (width, height) = bar.clock_surface;
        let backdrop = (!self.look.glass).then(|| {
            // The bar's gradient where the clock is, which changes by less
            // than a level over the clock's height.
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = clock::SURFACE_TOP as f32 + height as f32 / 2.0;
            mix(
                OPAQUE_TOP,
                OPAQUE_BOTTOM,
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = clock::Clock {
            style: self.clock,
            motion: self.look.motion,
            wall: self.wall.clone(),
            lang: self.lang,
            forms: forms.into_iter().filter(|(_, a)| *a > 0.0).collect(),
            bar_width: bar.width(at),
            surface_width: width as f32,
            backdrop,
        };
        Some(
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(bar) = self.bars.get(&id) else {
            return self.clock_view(id).unwrap_or_else(|| Space::new().into());
        };
        let width = bar.width(self.now + AHEAD);
        let (top, bottom) = if self.look.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let column = container(Space::new())
            .width(width)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    gradient::Linear::new(iced::Degrees(180.0))
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom)
                        .into(),
                )),
                border: Border {
                    color: RIM,
                    width: 1.0,
                    radius: (RADIUS as f32).into(),
                },
                ..container::Style::default()
            });
        container(row![space::horizontal(), column])
            .width(Length::Fill)
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

/// The clock's style for `look`: its family when it is installed, else
/// the bundled Martian Mono.
fn clock_style(look: &ClockLook) -> clock::Style {
    let [r, g, b] = look.color;
    clock::Style {
        font: clock_font(&look.font),
        color: Color::from_rgb8(r, g, b),
        transition: look.transition,
        seconds: look.seconds,
    }
}

fn clock_font(family: &str) -> Font {
    let default = Font::with_name(ClockLook::DEFAULT_FONT);
    if family == ClockLook::DEFAULT_FONT {
        return default;
    }
    let found = iced::advanced::graphics::text::font_system()
        .write()
        .ok()
        .and_then(|mut system| {
            system.raw().db().faces().find_map(|face| {
                face.families
                    .iter()
                    .find(|(name, _)| name.eq_ignore_ascii_case(family))
                    .map(|(name, _)| name.clone())
            })
        });
    match found {
        // iced names families with `&'static str`: one leaked name per
        // family configured, which the compositor sends once a session.
        Some(name) => Font::with_name(Box::leak(name.into_boxed_str())),
        None => {
            tracing::warn!(family, "the clock's font is not installed: Martian Mono");
            default
        }
    }
}

/// The time zone, as it changes: a thread waits on the system's file.
fn zone_changes() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(4, async |output| {
        let mut out = output.clone();
        let started = std::thread::Builder::new()
            .name("time zone".to_owned())
            .spawn(move || {
                clock::time::watch_zone(|zone| {
                    !matches!(out.try_send(Message::Zone(zone)), Err(e) if e.is_disconnected())
                });
            });
        if let Err(err) = started {
            tracing::warn!(%err, "cannot watch the time zone");
        }
        drop(output);
        std::future::pending::<()>().await;
    })
}

/// The `Instant` of a time on `CLOCK_MONOTONIC` (microseconds), which is
/// the clock `Instant` reads on Linux, through the time between it and now.
fn instant_of(monotonic_us: u64) -> Instant {
    let now = Instant::now();
    let clock = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    let now_us = (clock.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(clock.tv_nsec as u64 / 1_000);
    let ago = Duration::from_micros(now_us.saturating_sub(monotonic_us));
    now.checked_sub(ago).unwrap_or(now)
}

/// The line to the compositor: what it says comes as messages; what the
/// shell says goes through the sender handed over first.
// `Subscription::run_with` hands its data back as given: a `&String`.
#[allow(clippy::ptr_arg)]
fn link(socket: &String) -> impl iced::futures::Stream<Item = Message> + use<> {
    let socket = socket.clone();
    iced::stream::channel(16, async move |mut output| {
        let stream = match tokio::net::UnixStream::connect(&socket).await {
            Ok(stream) => stream,
            Err(err) => {
                tracing::error!(socket, %err, "could not reach the compositor");
                let _ = output.send(Message::Unlinked).await;
                return;
            }
        };
        let (read, mut write) = stream.into_split();
        let (sender, mut outgoing) = mpsc::unbounded::<String>();
        let _ = output.send(Message::Linked(sender)).await;
        tokio::spawn(async move {
            while let Some(line) = outgoing.next().await {
                if let Err(err) = write.write_all(format!("{line}\n").as_bytes()).await {
                    tracing::warn!(%err, "could not write to the compositor");
                    return;
                }
            }
        });
        let mut lines = BufReader::new(read).lines();
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    let _ = output.send(Message::FromCompositor(line)).await;
                }
                Ok(None) => break,
                Err(err) => {
                    tracing::warn!(%err, "could not read from the compositor");
                    break;
                }
            }
        }
        let _ = output.send(Message::Unlinked).await;
    })
}
