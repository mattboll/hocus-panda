//! The Hocuspanda shell: the bar (docs/design/bar.md), drawn with iced as a
//! wlr-layer-shell client of the compositor, which starts it and restarts
//! it when it ends.
//!
//! So far the bar is its column (§2, §3, §13), its clock (§4 group 1, §5),
//! its agents' group under it (§4 group 3, [`agents`]), its desktops (§4 group 8, §5), its connections (§4 group 9, §5) and its
//! levels (§4 group 10, §5): a glass tint and a rim against the right edge
//! of each output the compositor names, in its full or its mini form, the
//! time at its top, the screen's desktops ([`desktops`]) over the Wi-Fi
//! ([`network`]), Bluetooth ([`bluetooth`]) and the keyboard's layout
//! ([`layouts`]), above the sound, the brightness and the battery near its
//! bottom ([`levels`]).
//! The compositor blurs what is behind it; the shell draws the tint over
//! that blur, or an opaque background when there is no blur. The line
//! between the two is `hocus-link`.
//!
//! Each bar's surface is the bar itself, 8 px from the screen's edges: as
//! wide as its form, and as the full form while a switch is under way, so
//! that the bar has room to move. The compositor takes the clicks on the
//! bar's edge, which switch the form. The clock has a small surface of its
//! own over the bar's top ([`clock`]): it draws every second, and iced
//! presents a surface whole, so that only its region is drawn again. The
//! levels have theirs, for the same reason, the connections and the
//! desktops theirs, and so has a popover beside the bar ([`popover`]),
//! opened from an item.

mod agents;
mod bluetooth;
mod clock;
mod desktops;
mod layouts;
mod levels;
mod network;
mod paste;
mod popover;
mod renderer;

use std::{
    borrow::Cow,
    collections::HashMap,
    process::ExitCode,
    time::{Duration, Instant},
};

use hocus_link::{
    AGENTS_NAMESPACE, CLOCK_NAMESPACE, CONNECTIONS_NAMESPACE, ClockLook, DESKTOPS_NAMESPACE, EDGE,
    Form, LEVELS_NAMESPACE, NAMESPACE, RADIUS, ToCompositor, ToShell,
};
use iced::{
    Background, Border, Color, Element, Font, Length, Size, Subscription, Task, Theme,
    futures::{SinkExt, StreamExt, channel::mpsc},
    gradient,
    widget::{Space, canvas, container, row, space, stack},
    window,
};
use iced_layershell::{
    reexport::{Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption},
    settings::{LayerShellSettings, Settings, StartMode},
    to_layer_message,
};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing_subscriber::EnvFilter;

/// The name the shell alone gives its one bar, on the active output.
const ALONE: &str = "active";

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
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,hocuspanda_shell=info".into());
    // cosmic-text traces the text it shapes, a password shown with the eye
    // too: never, whatever RUST_LOG asks.
    let filter = match "cosmic_text::shape=off".parse() {
        Ok(quiet) => filter.add_directive(quiet),
        Err(_) => filter,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
    // On a CPU, the CPU renderer: this may start the program again.
    let software = renderer::choose();
    levels::start();
    network::start();
    bluetooth::start();
    let link = std::env::var(hocus_link::SOCKET_ENV).ok();
    if link.is_none() {
        tracing::warn!(
            "{} is not set: a full bar on the active output, for trying the shell alone",
            hocus_link::SOCKET_ENV
        );
    }
    // The connection iced draws on, made here: the shell reads the
    // clipboard on it (paste), and iced's own clipboard is not needed.
    let connection = match wayland_client::Connection::connect_to_env() {
        Ok(connection) => {
            paste::start(&connection);
            Some(connection)
        }
        Err(err) => {
            tracing::error!(%err, "could not reach the compositor's Wayland socket");
            None
        }
    };
    iced_layershell::disable_clipboard();
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
        with_connection: connection.map(Into::into),
        ..Settings::default()
    })
    .run();
    levels::sound::stop();
    bluetooth::stop();
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
    /// Its levels' surface, and its size as last asked for.
    levels: window::Id,
    levels_surface: (u32, u32),
    /// Its connections' surface (placed by [`network::draw`]).
    connections: window::Id,
    /// Its desktops' surface (placed by [`desktops::draw`]).
    desktops: window::Id,
    /// Its agents' group's surface (placed by [`agents::draw`]).
    agents: window::Id,
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

    /// The levels' surface's size, as the clock's; the mini form's taller
    /// with the keyboard layout's button.
    fn levels_size(&self) -> (u32, u32) {
        match (self.switch, self.form) {
            (Some(_), _) | (None, Form::Full) => levels::draw::FULL_SURFACE,
            (None, Form::Mini) => levels::draw::mini_surface(layouts::snapshot().shown().is_some()),
        }
    }

    fn moving(&self, now: Instant) -> bool {
        self.switch.is_some_and(|(_, since)| {
            now.saturating_duration_since(since) < hocus_link::SWITCH + AHEAD
        })
    }

    /// The forms its contents show at `at`, with their opacity: they
    /// cross-fade over 120 ms, 60 ms into a switch (§3).
    fn forms(&self, at: Instant) -> Vec<(Form, f32)> {
        let forms = match self.switch {
            Some((_, since)) => {
                let t = at.saturating_duration_since(since).as_secs_f32();
                let new = ((t - 0.060) / 0.120).clamp(0.0, 1.0);
                vec![(self.form.other(), 1.0 - new), (self.form, new)]
            }
            None => vec![(self.form, 1.0)],
        };
        forms.into_iter().filter(|(_, a)| *a > 0.0).collect()
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
    words: levels::model::Words,
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
    /// The levels changed, for a shell alone (with the compositor, it
    /// wakes their surface instead).
    Levels,
    /// The keyboard's layouts, as the compositor says them as the shell
    /// connects: the row comes (or goes), the surfaces to their sizes.
    Layouts(Vec<layouts::Layout>),
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
            words: levels::model::Words::new(clock::time::Lang::of_messages()),
        };
        popover::with(|c| c.motion = shell.look.motion);
        // Alone: one full bar where the compositor puts it.
        let task = if socket.is_none() {
            shell.open_bar(OutputOption::Active, ALONE.to_owned(), Form::Full)
        } else {
            Task::none()
        };
        (shell, task)
    }

    fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = Vec::new();
        match &self.socket {
            Some(socket) => subscriptions.push(Subscription::run_with(socket.clone(), link)),
            None => subscriptions.push(Subscription::run(levels_alone)),
        }
        // Frames only while a bar moves, and one past its switch's end,
        // which narrows its surface: at rest, nothing is drawn.
        if self.bars.values().any(|b| b.switch.is_some()) {
            subscriptions.push(window::frames().map(|_| Message::Frame));
        }
        subscriptions.push(Subscription::run(zone_changes));
        subscriptions.push(Subscription::run(popover::effects));
        subscriptions.push(bar_sizes());
        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        match message {
            Message::Linked(sender) => {
                if self.software {
                    let _ = sender.unbounded_send(ToCompositor::Software.to_string());
                }
                popover::linked(sender.clone());
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
            Message::Levels => Task::none(),
            Message::Layouts(list) => {
                layouts::set(list);
                let ids: Vec<window::Id> = self.bars.keys().copied().collect();
                Task::batch(ids.into_iter().map(|id| self.fit(id)))
            }
            Message::Resized(id, size) => {
                if let Some(bar) = self.bars.get_mut(&id) {
                    bar.height = size.height;
                    self.place_popovers(id);
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
                popover::with(|c| c.motion = self.look.motion);
                Task::none()
            }
            ToShell::Bar {
                output,
                form,
                since,
                scale,
            } => {
                popover::kit::set_scale(&output, scale as f32 / 120.0);
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
                    BARS.lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .retain(|b| *b != id);
                    popover::event(popover::Event::BarChanged {
                        bar: bar.output.clone(),
                    });
                    popover::with(|c| c.forget(&bar.output));
                    popover::kit::forget_scale(&bar.output);
                    network::draw::forget(&bar.output);
                    desktops::draw::forget(&bar.output);
                    agents::draw::forget(&bar.output);
                    Some(Task::batch([
                        Task::done(Message::RemoveWindow(bar.agents)),
                        Task::done(Message::RemoveWindow(bar.desktops)),
                        Task::done(Message::RemoveWindow(bar.connections)),
                        Task::done(Message::RemoveWindow(bar.levels)),
                        Task::done(Message::RemoveWindow(bar.clock)),
                        Task::done(Message::RemoveWindow(id)),
                    ]))
                }))
            }
            ToShell::Clock(look) => {
                self.clock = clock_style(&look);
                Task::none()
            }
            // Taken by the link itself, without a message: here only if
            // the link changes.
            ToShell::Level(key) => {
                levels::key(key);
                Task::none()
            }
            ToShell::Outside => {
                popover::event(popover::Event::Outside);
                Task::none()
            }
            ToShell::Screens(names) => {
                screens(names);
                Task::none()
            }
            // Made into what the bar shows by the link first.
            ToShell::Layouts(_) => Task::none(),
            ToShell::Layout(index) => {
                layouts::activate(index);
                Task::none()
            }
            ToShell::Desktops(screen) => {
                desktops::set(screen);
                Task::none()
            }
            ToShell::Titles(titles) => {
                desktops::set_titles(titles);
                Task::none()
            }
            ToShell::AgentTools(tools) => {
                agents::set_tools(tools);
                Task::none()
            }
            ToShell::Agents(list) => {
                agents::set_agents(list);
                Task::none()
            }
        }
    }

    /// A bar's surface on `output`: against its right edge, its height
    /// less the margins; and its clock's, over its top, and its levels',
    /// near its bottom, opened after it so that they are in front.
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
            levels: window::Id::unique(),
            levels_surface: (0, 0),
            connections: window::Id::unique(),
            desktops: window::Id::unique(),
            agents: window::Id::unique(),
        };
        bar.surface = bar.surface_width();
        bar.clock_surface = bar.clock_size();
        bar.levels_surface = bar.levels_size();
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
            output_option: option.clone(),
            events_transparent: true,
            namespace: Some(CLOCK_NAMESPACE.to_owned()),
        };
        let (clock_id, open_clock) = Message::layershell_open(clock);
        bar.clock = clock_id;
        // Its items take the pointer: the sound's opens its popover.
        let levels = NewLayerShellSettings {
            size: Some(bar.levels_surface),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Bottom,
            exclusive_zone: Some(0),
            margin: Some((0, EDGE + 1, EDGE + levels::draw::BOTTOM, 0)),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(LEVELS_NAMESPACE.to_owned()),
        };
        let (levels_id, open_levels) = Message::layershell_open(levels);
        bar.levels = levels_id;
        // Above the levels, following their group's top; its row opens
        // the Wi-Fi's popover.
        let connections_id = window::Id::unique();
        let (size, margin) = network::draw::opened(&bar.output, connections_id, form);
        let connections = NewLayerShellSettings {
            size: Some(size),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Bottom,
            exclusive_zone: Some(0),
            margin: Some(margin),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(CONNECTIONS_NAMESPACE.to_owned()),
        };
        let open_connections = Task::done(Message::NewLayerShell {
            settings: connections,
            id: connections_id,
        });
        bar.connections = connections_id;
        // Above the connections, following their group's top; a tile goes
        // to its desktop.
        let desktops_id = window::Id::unique();
        let (size, margin) = desktops::draw::opened(&bar.output, desktops_id, form);
        let desktops = NewLayerShellSettings {
            size: Some(size),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Bottom,
            exclusive_zone: Some(0),
            margin: Some(margin),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(DESKTOPS_NAMESPACE.to_owned()),
        };
        let open_desktops = Task::done(Message::NewLayerShell {
            settings: desktops,
            id: desktops_id,
        });
        bar.desktops = desktops_id;
        // Under the clock: the agents' group.
        let agents_id = window::Id::unique();
        let (size, margin) = agents::draw::opened(&bar.output, agents_id, form);
        let agents = NewLayerShellSettings {
            size: Some(size),
            layer: Layer::Top,
            anchor: Anchor::Right | Anchor::Top,
            exclusive_zone: Some(0),
            margin: Some(margin),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(AGENTS_NAMESPACE.to_owned()),
        };
        let open_agents = Task::done(Message::NewLayerShell {
            settings: agents,
            id: agents_id,
        });
        bar.agents = agents_id;
        self.bars.insert(id, bar);
        BARS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(id);
        self.place_popovers(id);
        open.chain(open_clock)
            .chain(open_levels)
            .chain(open_connections)
            .chain(open_desktops)
            .chain(open_agents)
    }

    /// Where bar `id` is, for the popovers.
    fn place_popovers(&self, id: window::Id) {
        let Some(bar) = self.bars.get(&id) else {
            return;
        };
        popover::with(|c| {
            c.place(
                &bar.output,
                bar.form,
                id,
                (bar.levels, bar.connections, bar.desktops, bar.agents),
            );
        });
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
        popover::event(popover::Event::BarChanged {
            bar: bar.output.clone(),
        });
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
        self.place_popovers(id);
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
        let size = bar.levels_size();
        if bar.levels_surface != size {
            bar.levels_surface = size;
            tasks.push(Task::done(Message::SizeChange {
                id: bar.levels,
                size,
            }));
        }
        let switching = bar.switch.is_some();
        for (id, size, margin) in [
            network::draw::form(&bar.output, bar.form, switching),
            desktops::draw::form(&bar.output, bar.form, switching),
            agents::draw::form(&bar.output, bar.form, switching),
        ]
        .into_iter()
        .flatten()
        {
            tasks.push(Task::done(Message::SizeChange { id, size }));
            tasks.push(Task::done(Message::MarginChange { id, margin }));
        }
        Task::batch(tasks)
    }

    /// The clock of the bar whose clock's surface is `id`.
    fn clock_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.clock == id)?;
        let at = self.now + AHEAD;
        let forms = bar.forms(at);
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
            forms,
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

    /// The levels of the bar whose levels' surface is `id`.
    fn levels_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.levels == id)?;
        let at = self.now + AHEAD;
        let (_, height) = bar.levels_surface;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - levels::draw::BOTTOM as f32 - height as f32 / 2.0;
            mix(
                OPAQUE_TOP,
                OPAQUE_BOTTOM,
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = levels::draw::Levels {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    /// The connections of the bar whose connections' surface is `id`.
    fn connections_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.connections == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - levels::draw::BOTTOM as f32 - 160.0;
            mix(
                OPAQUE_TOP,
                OPAQUE_BOTTOM,
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = network::draw::Connections {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    /// The desktops of the bar whose desktops' surface is `id`.
    fn desktops_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.desktops == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle =
                bar_height - levels::draw::BOTTOM as f32 - desktops::draw::middle(&bar.output);
            mix(
                OPAQUE_TOP,
                OPAQUE_BOTTOM,
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = desktops::draw::Tiles {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            words: self.words,
            backdrop,
            bar: bar.output.clone(),
        };
        Some(
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    /// The agents' group of the bar whose agents' surface is `id`.
    fn agents_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.agents == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = agents::draw::TOP + 40.0;
            mix(
                OPAQUE_TOP,
                OPAQUE_BOTTOM,
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = agents::draw::Group {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: agents::Words::new(clock::time::Lang::of_messages()),
            backdrop,
            bar: bar.output.clone(),
            motion: self.look.motion,
        };
        Some(
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    /// The popover whose surface is `id`.
    fn popover_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let item = popover::with(|c| c.item_of(id))?;
        let panel: Element<'_, Message> = match item {
            popover::Item::Sound => canvas(popover::sound::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Wifi => canvas(popover::wifi::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
                motion: self.look.motion,
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Bluetooth => canvas(popover::bluetooth::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
                motion: self.look.motion,
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Brightness => canvas(popover::brightness::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Battery => canvas(popover::battery::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Desktop(_) => canvas(popover::desktop::Panel {
                id,
                words: self.words,
                glass: self.look.glass,
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Sources | popover::Item::AnotherTool | popover::Item::Plug(_) => {
                canvas(popover::agents::Panel {
                    id,
                    words: agents::Words::new(clock::time::Lang::of_messages()),
                    glass: self.look.glass,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
            popover::Item::Secretary => canvas(popover::briefing::Panel {
                id,
                words: agents::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
                wall: self.wall.clone(),
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            // They open none.
            popover::Item::Keyboard | popover::Item::Decline | popover::Item::Agent(_) => {
                return None;
            }
        };
        Some(panel)
    }

    fn view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(bar) = self.bars.get(&id) else {
            return self
                .clock_view(id)
                .or_else(|| self.levels_view(id))
                .or_else(|| self.connections_view(id))
                .or_else(|| self.desktops_view(id))
                .or_else(|| self.agents_view(id))
                .or_else(|| self.popover_view(id))
                .unwrap_or_else(|| Space::new().into());
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
        let bar = container(row![space::horizontal(), column])
            .width(Length::Fill)
            .height(Length::Fill);
        let pointer = canvas(popover::BarPointer)
            .width(Length::Fill)
            .height(Length::Fill);
        stack![bar, pointer].into()
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

/// The bars' surfaces, by id: their sizes are messages, not the other
/// surfaces', which would draw every surface again.
static BARS: std::sync::Mutex<Vec<window::Id>> = std::sync::Mutex::new(Vec::new());

fn is_bar(id: window::Id) -> bool {
    BARS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(&id)
}

/// The bars' sizes, as they open and change: their height places their
/// items, for the popovers, which take it at once; a change of size is a
/// message too (the opaque backdrops follow the bar's height), as a bar
/// opening is not: it would draw every surface again.
fn bar_sizes() -> Subscription<Message> {
    iced::event::listen_with(|event, _, id| match event {
        iced::Event::Window(window::Event::Opened { size, .. }) if is_bar(id) => {
            popover::with(|c| c.height(id, size.height));
            None
        }
        iced::Event::Window(window::Event::Resized(size)) if is_bar(id) => {
            popover::with(|c| c.height(id, size.height));
            Some(Message::Resized(id, size))
        }
        _ => None,
    })
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

/// The other screens are `names`: the brightness's popover says so if it
/// is open (drawn again, resized).
fn screens(names: Vec<String>) {
    if levels::set_screens(names) {
        popover::changed(popover::Item::Brightness);
    }
}

/// A shell alone: a change of level or of network is a message (every
/// surface is drawn).
fn levels_alone() -> impl iced::futures::Stream<Item = Message> {
    iced::stream::channel(4, async |output| {
        let mut out = output.clone();
        levels::on_change(
            move || !matches!(out.try_send(Message::Levels), Err(e) if e.is_disconnected()),
        );
        let mut out = output.clone();
        network::on_change(move |change| {
            if change.shown {
                network::draw::rows_changed();
            }
            !matches!(out.try_send(Message::Levels), Err(e) if e.is_disconnected())
        });
        let mut out = output.clone();
        bluetooth::on_change(move |change| {
            if change.shown {
                network::draw::rows_changed();
            }
            !matches!(out.try_send(Message::Levels), Err(e) if e.is_disconnected())
        });
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
        // A change of level wakes the levels' surfaces through the
        // compositor, without a message that would draw every surface.
        // The brightness's and the battery's popovers show them too.
        let waker = sender.clone();
        levels::on_change(move || {
            popover::changed(popover::Item::Brightness);
            popover::changed(popover::Item::Battery);
            let wake = ToCompositor::Wake(LEVELS_NAMESPACE.to_owned());
            waker.unbounded_send(wake.to_string()).is_ok()
        });
        // A change of network wakes the connections' surface when the row
        // changes (resized when it comes or goes), and the Wi-Fi's popover
        // when it shows; Bluetooth's the same.
        let waker = sender.clone();
        network::on_change(move |change| {
            popover::changed(popover::Item::Wifi);
            if change.shown {
                network::draw::rows_changed();
            }
            if !change.row {
                return true;
            }
            let wake = ToCompositor::Wake(CONNECTIONS_NAMESPACE.to_owned());
            waker.unbounded_send(wake.to_string()).is_ok()
        });
        let waker = sender.clone();
        bluetooth::on_change(move |change| {
            popover::changed(popover::Item::Bluetooth);
            if change.shown {
                network::draw::rows_changed();
            }
            if !change.row {
                return true;
            }
            let wake = ToCompositor::Wake(CONNECTIONS_NAMESPACE.to_owned());
            waker.unbounded_send(wake.to_string()).is_ok()
        });
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
                    // A level key goes to its service: no message, no
                    // drawing; the gauge shows what the service then reads.
                    match line.parse() {
                        Ok(ToShell::Level(key)) => {
                            levels::key(key);
                            continue;
                        }
                        // A press outside closes the popover: no message
                        // either, the controller draws what changes.
                        Ok(ToShell::Outside) => {
                            popover::event(popover::Event::Outside);
                            continue;
                        }
                        // The other screens: only an open brightness's
                        // popover draws for them.
                        Ok(ToShell::Screens(names)) => {
                            screens(names);
                            continue;
                        }
                        // The layouts' names in the bar's language, read
                        // from XKB's catalog off the loop; then a message,
                        // once a connection: the surfaces take their sizes.
                        Ok(ToShell::Layouts(list)) => {
                            let lang = clock::time::Lang::of_messages();
                            let made =
                                tokio::task::spawn_blocking(move || layouts::made(&list, lang))
                                    .await
                                    .unwrap_or_default();
                            let _ = output.send(Message::Layouts(made)).await;
                            continue;
                        }
                        // The active layout: only the row draws.
                        Ok(ToShell::Layout(index)) => {
                            layouts::activate(index);
                            continue;
                        }
                        // A screen's desktops: only their surface draws
                        // (resized when their rows change).
                        Ok(ToShell::Desktops(screen)) => {
                            desktops::set(screen);
                            continue;
                        }
                        // Its windows' titles: only an open list draws.
                        Ok(ToShell::Titles(titles)) => {
                            desktops::set_titles(titles);
                            continue;
                        }
                        // The agents' tools and the agents: only their
                        // group draws (resized when its height changes),
                        // and an open Sources or consent popover.
                        Ok(ToShell::AgentTools(tools)) => {
                            agents::set_tools(tools);
                            continue;
                        }
                        Ok(ToShell::Agents(list)) => {
                            agents::set_agents(list);
                            continue;
                        }
                        _ => {}
                    }
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
