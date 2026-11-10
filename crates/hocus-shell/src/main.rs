//! The Hocuspanda shell: the bar (docs/design/bar.md), drawn with iced as a
//! wlr-layer-shell client of the compositor, which starts it and restarts
//! it when it ends.
//!
//! So far the bar is its column (§2, §3, §13), its clock (§4 group 1, §5),
//! the day's groups and its agents' group under it in their order (§4
//! groups 2 to 5, [`column`], [`agents`]), its desktops (§4 group 8, §5), its connections (§4 group 9, §5) and its
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
//! desktops theirs, the footer along its bottom ([`footer`]), and so has a
//! popover beside the bar ([`popover`]), opened from an item, and the panda
//! peeking out of the bar's edge ([`agents::peek`]).

mod a11y;
mod actions;
mod agents;
mod auth;
mod backdrop;
mod bluetooth;
mod capture;
mod clipboard;
mod clock;
mod column;
mod curtain;
mod day;
mod desktops;
mod drawer;
mod emoji;
mod ending;
mod field;
mod footer;
mod foreign;
mod in_use;
mod keys;
mod launcher;
mod layouts;
mod levels;
mod locale;
mod lock;
mod meeting;
mod network;
mod notifications;
mod outputs;
mod pandas;
mod paste;
mod pointer;
mod popover;
mod reminder;
mod renderer;
mod secretary;
mod setup;
mod share;
mod side;
mod speak;
mod tasks;
mod theme;
mod today;
mod tray;
mod user;
mod wayland;
mod windows;

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
    widget::{Space, canvas, container, pin, row, space, stack},
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

// --- Theme tokens (§13, theme.rs) ------------------------------------------

/// `bar.bg` on glass: the tint over the compositor's blur.
const GLASS_TOP: theme::Ink = theme::Ink::Token(|t| t.bar_top);
const GLASS_BOTTOM: theme::Ink = theme::Ink::Token(|t| t.bar_bottom);
/// `bar.bg` without glass (reduce transparency, no blur, CPU rendering).
const OPAQUE_TOP: theme::Ink = theme::Ink::Token(|t| t.bar_opaque_top);
const OPAQUE_BOTTOM: theme::Ink = theme::Ink::Token(|t| t.bar_opaque_bottom);
/// `bar.border`.
const RIM: theme::Ink = theme::Ink::Token(|t| t.bar_border);

fn main() -> ExitCode {
    // Before anything could take it for its own (docs/design/lock.md).
    let handed = wayland::take_socket();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "warn,hocuspanda_shell=info,hocus_day=info,hocus_bluetooth=info".into()
    });
    // cosmic-text traces the text it shapes, a password shown with the eye
    // too: never, whatever RUST_LOG asks.
    let filter = match "cosmic_text::shape=off".parse() {
        Ok(quiet) => filter.add_directive(quiet),
        Err(_) => filter,
    };
    // iced traces the text of every paragraph it lays out, a stock text
    // field's too (spike #770): never either.
    let filter = match "iced_graphics::text::paragraph=off".parse() {
        Ok(quiet) => filter.add_directive(quiet),
        Err(_) => filter,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
    // On a CPU, the CPU renderer: this may start the program again.
    let software = renderer::choose(handed);
    // From here, SIGTERM ends it once what it keeps is written.
    ending::start();
    load_fonts();
    levels::start();
    network::start();
    bluetooth::start();
    launcher::start();
    emoji::start();
    actions::start();
    // The session's polkit agent, when the shell is the session's (7.5).
    auth::start();
    notifications::start(software);
    // After the notifications' wall clock, which the day is read against.
    day::start();
    meeting::start();
    // After the day's start, which it follows.
    reminder::start(software);
    today::start();
    tasks::start();
    // After the day: whether the first-run card shows.
    setup::start();
    // The calendar drawn again as the day changes, while it is open.
    popover::calendar::start();
    // After the notifications' wall clock, which its times read.
    clipboard::start();
    user::start();
    // The lock's avatar: the account's photo, if AccountsService has one.
    lock::avatar::start();
    tray::start();
    // Screen readers: a thread that waits until one listens.
    a11y::start();
    let link = std::env::var(hocus_link::SOCKET_ENV).ok();
    if link.is_none() {
        tracing::warn!(
            "{} is not set: a full bar on the active output, for trying the shell alone",
            hocus_link::SOCKET_ENV
        );
    }
    // The connection iced draws on, made here: the shell reads the
    // clipboard on it (paste), and iced's own clipboard is not needed.
    let connection = match wayland::connect(handed) {
        Ok(connection) => {
            paste::start(&connection);
            outputs::start(&connection);
            pointer::start(&connection);
            keys::start(&connection);
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
    notifications::flush();
    day::flush();
    match started {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            tracing::error!(%err, "the shell stopped");
            ExitCode::FAILURE
        }
    }
}

/// The bundled fonts, in the font system that measures and draws text,
/// before anything is measured: iced loads them only as it makes the
/// renderer of the first surface it draws, and a text measured before (a
/// capture's toolbar opened as the shell starts) was measured in another
/// font, its surface too wide or too narrow for what it draws. iced loads
/// them again later, which the font system knows they are (by address).
fn load_fonts() {
    match iced::advanced::graphics::text::font_system().write() {
        Ok(mut system) => {
            for font in clock::FONTS {
                system.load_font(Cow::Borrowed(font));
            }
        }
        Err(err) => tracing::warn!(%err, "the fonts are not loaded before the first surface"),
    }
}

/// The surfaces are transparent where the bar does not draw.
fn style(_: &Shell, _: &Theme) -> iced::theme::Style {
    iced::theme::Style {
        background_color: Color::TRANSPARENT,
        text_color: theme::TEXT.get(),
    }
}

fn theme(_: &Shell, _: window::Id) -> Option<Theme> {
    Some(if theme::light() {
        Theme::Light
    } else {
        Theme::Dark
    })
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
    /// Its to-do list's (placed by [`tasks::draw`]).
    tasks: window::Id,
    /// Its footer's (placed by [`footer`]).
    footer: window::Id,
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

    /// The levels' surface's size, as the clock's (tall enough for both
    /// forms' groups while the form switches); the mini form's taller with
    /// the keyboard layout's button.
    fn levels_size(&self) -> (u32, u32) {
        let mini = levels::draw::mini_surface(layouts::snapshot().shown().is_some());
        // Over the tray (5.14): taller by what it takes in each form.
        let lift = |form| tray::draw::lift(&self.output, form).ceil() as u32;
        let full = levels::draw::FULL_SURFACE.1 + lift(Form::Full);
        let mini = (mini.0, mini.1 + lift(Form::Mini));
        match (self.switch, self.form) {
            (Some(_), _) => (levels::draw::FULL_SURFACE.0, full.max(mini.1)),
            (None, Form::Full) => (levels::draw::FULL_SURFACE.0, full),
            (None, Form::Mini) => mini,
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
    /// Bars asked for on screens whose names have not reached
    /// iced_layershell yet ([`outputs`]): their forms, by screen.
    waiting: HashMap<String, Form>,
    now: Instant,
    clock: clock::Style,
    /// What the compositor said of the clock: its colour follows the
    /// theme when it is the default one.
    clock_look: ClockLook,
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
    /// A tray icon came or went (5.14): the tray's surfaces, and the
    /// groups over them, to their places.
    Tray,
    Resized(window::Id, Size),
    /// A screen moved on in giving its name ([`outputs`]).
    Screens,
    /// A key went to another surface than the one holding the keyboard:
    /// every surface is woken, that one's widgets taking it ([`keys`]).
    Keys,
    /// The Wi-Fi's popover made of widgets (spike #770): what one asks.
    Wifi(popover::wifi::widgets::Msg),
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
            waiting: HashMap::new(),
            now: Instant::now(),
            clock: clock_style(&ClockLook::default()),
            clock_look: ClockLook::default(),
            wall: clock::time::Wall { zone, shift },
            lang: clock::time::Lang::from_env(),
            words: levels::model::Words::new(clock::time::Lang::of_messages()),
        };
        popover::with(|c| c.motion = shell.look.motion);
        pandas::set_lively(shell.look.motion);
        agents::peek::set_software(software);
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
        // Woken only by the screens' own events: a screen plugged in.
        subscriptions.push(Subscription::run(outputs::changes).map(|()| Message::Screens));
        subscriptions.push(Subscription::run(zone_changes));
        subscriptions.push(Subscription::run(popover::effects));
        subscriptions.push(Subscription::run(notifications::effects));
        subscriptions.push(Subscription::run(tray::effects));
        subscriptions.push(Subscription::run(reminder::effects));
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
                meeting::linked();
                clipboard::linked();
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
                notifications::set_zone(zone.clone());
                self.wall.zone = zone;
                reminder::zone_changed();
                Task::none()
            }
            Message::Levels => Task::none(),
            Message::Tray => {
                let ids: Vec<window::Id> = self.bars.keys().copied().collect();
                let fitted = Task::batch(ids.into_iter().map(|id| self.fit(id)));
                tray::placed();
                fitted
            }
            Message::Layouts(list) => {
                layouts::set(list);
                let ids: Vec<window::Id> = self.bars.keys().copied().collect();
                Task::batch(ids.into_iter().map(|id| self.fit(id)))
            }
            Message::Screens => self.screens_named(),
            // Its update is the wake: iced_layershell redraws every surface
            // after one.
            Message::Keys => Task::none(),
            Message::Wifi(message) => popover::wifi::widgets::update(message, self.look.motion),
            Message::Resized(id, size) => {
                if let Some(bar) = self.bars.get_mut(&id) {
                    bar.height = size.height;
                    self.place_popovers(id);
                    // The column's room is the bar's height less its
                    // groups'.
                    column::moved();
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }

    fn compositor_says(&mut self, message: ToShell) -> Task<Message> {
        match message {
            ToShell::Look {
                glass,
                motion,
                lively,
            } => {
                self.look = Look {
                    glass: glass && !self.software,
                    motion: motion && !self.software,
                };
                popover::with(|c| c.motion = self.look.motion);
                pandas::set_lively(lively && self.look.motion);
                Task::none()
            }
            // Every surface draws again, in it: this is a message.
            ToShell::Theme(choice) => {
                if theme::set(choice) {
                    self.clock = clock_style(&self.clock_look);
                }
                // The accent last: tests read the line's end.
                tracing::info!(
                    theme = %choice.scheme,
                    colours = %choice,
                    accent = %choice.accent,
                    "the theme"
                );
                Task::none()
            }
            // The lock draws the wallpapers (6.3).
            ToShell::Wallpaper(wallpapers) => {
                tracing::info!(%wallpapers, "the wallpapers");
                if lock::wallpaper::set(wallpapers) {
                    popover::tell(hocus_link::ToCompositor::Wake(
                        hocus_link::LOCK_NAMESPACE.to_owned(),
                    ));
                }
                Task::none()
            }
            // The words and the formats: every surface draws again in them.
            ToShell::Locale(locale) => {
                locale::set(locale);
                self.lang = clock::time::Lang::from_env();
                self.words = levels::model::Words::new(clock::time::Lang::of_messages());
                let lang = clock::time::Lang::of_messages();
                tracing::info!(%locale, ?lang, "the language and formats");
                Task::none()
            }
            ToShell::Bar {
                output,
                form,
                since,
                scale,
            } => {
                popover::kit::set_scale(&output, scale as f32 / 120.0);
                tray::scales_changed();
                let start = (since != 0).then(|| instant_of(since));
                match self.bars.iter().find(|(_, b)| b.output == output) {
                    Some((id, _)) => self.set_form(*id, form, start),
                    None => self.wait_for(output, form),
                }
            }
            ToShell::NoBar { output } => {
                self.waiting.remove(&output);
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
                    notifications::bars_changed();
                    popover::kit::forget_scale(&bar.output);
                    network::draw::forget(&bar.output);
                    desktops::draw::forget(&bar.output);
                    agents::draw::forget(&bar.output);
                    agents::peek::forget(&bar.output);
                    speak::peek::forget(&bar.output);
                    footer::forget(&bar.output);
                    tasks::draw::forget(&bar.output);
                    let sign = in_use::forget(&bar.output);
                    let card = meeting::forget(&bar.output);
                    reminder::forget(&bar.output);
                    let today = today::forget(&bar.output);
                    let windows = windows::forget(&bar.output);
                    let tray = tray::draw::forget(&bar.output);
                    Some(Task::batch([
                        sign.map_or_else(Task::none, |s| Task::done(Message::RemoveWindow(s))),
                        card.map_or_else(Task::none, |c| Task::done(Message::RemoveWindow(c))),
                        today.map_or_else(Task::none, |t| Task::done(Message::RemoveWindow(t))),
                        windows.map_or_else(Task::none, |w| Task::done(Message::RemoveWindow(w))),
                        Task::batch(tray.map(|id| Task::done(Message::RemoveWindow(id)))),
                        Task::done(Message::RemoveWindow(bar.footer)),
                        Task::done(Message::RemoveWindow(bar.tasks)),
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
                tracing::info!(
                    seconds = look.seconds,
                    transition = look.transition.name(),
                    font = look.font,
                    "the clock's look"
                );
                self.clock = clock_style(&look);
                self.clock_look = look;
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
                launcher::outside();
                emoji::outside();
                actions::close();
                drawer::outside();
                Task::none()
            }
            ToShell::Launcher { output, scale } => {
                launcher::toggle(output, scale);
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
            ToShell::Recent(recent) => {
                desktops::set_recent(recent);
                Task::none()
            }
            ToShell::DeskScreens { bar, screens } => {
                desktops::set_screens(bar, screens);
                Task::none()
            }
            ToShell::DropOver { bar, over } => {
                desktops::set_drop(bar, over);
                Task::none()
            }
            ToShell::BarWindows(shown) => {
                windows::set_shown(shown);
                Task::none()
            }
            ToShell::DesktopNames(names) => {
                desktops::set_names(names);
                Task::none()
            }
            ToShell::BarGroups(groups) => {
                column::set_groups(groups);
                Task::none()
            }
            // Before the bars, made again on that side after it.
            ToShell::BarSide(side) => {
                side::set(side);
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
            ToShell::Peek(peek) => {
                agents::peek::peek(peek);
                Task::none()
            }
            ToShell::NoPeek => {
                agents::peek::back();
                speak::peek::back();
                Task::none()
            }
            ToShell::ScreensMoved(moved) => {
                agents::peek::screens(moved);
                Task::none()
            }
            ToShell::AgentPandas(pandas) => {
                agents::set_pandas(pandas);
                Task::none()
            }
            ToShell::Focus(screen) => {
                notifications::focus(screen);
                Task::none()
            }
            ToShell::Copied { app, clip } => {
                clipboard::copied(app, clip);
                Task::none()
            }
            ToShell::CaptureCopied { capture, clip } => {
                clipboard::captured(capture, clip);
                Task::none()
            }
            ToShell::Clipboard { output } => {
                popover::toggle_clipboard(&output);
                Task::none()
            }
            ToShell::ClipboardRules(rules) => {
                clipboard::rules(rules);
                Task::none()
            }
            ToShell::ClipboardClear => {
                clipboard::clear();
                Task::none()
            }
            ToShell::Emoji {
                output,
                scale,
                width,
                height,
                at,
            } => {
                emoji::toggle(output, scale, (width, height), at);
                Task::none()
            }
            ToShell::EmojiInserted => {
                emoji::inserted();
                Task::none()
            }
            ToShell::Actions(shown) => {
                actions::toggle(shown);
                Task::none()
            }
            ToShell::ActionsEnd => {
                actions::close();
                Task::none()
            }
            ToShell::Capture(bar) => {
                capture::set(bar);
                Task::none()
            }
            ToShell::CaptureEnd => {
                capture::end();
                Task::none()
            }
            ToShell::Captured(captured) => {
                capture::captured(captured);
                Task::none()
            }
            ToShell::ShareChoose(choose) => {
                share::set(choose);
                Task::none()
            }
            ToShell::ShareChooseEnd(request) => {
                share::end(request);
                Task::none()
            }
            ToShell::AuthAt(at) => {
                auth::placed(at);
                Task::none()
            }
            ToShell::CapsLock(on) => {
                auth::caps(on);
                Task::none()
            }
            ToShell::TrayToken { ask, token } => {
                tray::token(ask, token);
                Task::none()
            }
            ToShell::Exported { ask, handle } => {
                foreign::exported(ask, handle);
                Task::none()
            }
            ToShell::Shares(shares) => {
                in_use::set_shares(shares);
                Task::none()
            }
            ToShell::Recording(shown) => {
                in_use::set_recording(Some(shown));
                Task::none()
            }
            ToShell::RecordingEnd => {
                in_use::set_recording(None);
                Task::none()
            }
            ToShell::Recorded(recorded) => {
                capture::recorded(recorded);
                Task::none()
            }
            ToShell::Day(ask) => {
                if let hocus_link::DayAsk::Slept(seconds) = ask {
                    self.slept(seconds);
                    secretary::resumed(Some(seconds));
                }
                day::ask(ask);
                Task::none()
            }
            ToShell::Join => {
                reminder::join_key();
                Task::none()
            }
            ToShell::Secretary(shown) => {
                secretary::told(shown);
                Task::none()
            }
            ToShell::SecretaryAsk(text) => {
                secretary::ask(text);
                Task::none()
            }
            ToShell::SecretaryDrawer { output, width } => {
                drawer::toggle(&output, width);
                Task::none()
            }
            ToShell::SecretaryPeek(id) => {
                speak::peek::show(id);
                Task::none()
            }
            ToShell::Fullscreen(screens) => {
                reminder::set_fullscreen(screens);
                Task::none()
            }
            ToShell::SharedScreens(screens) => {
                reminder::set_shared(screens);
                Task::none()
            }
            ToShell::Lock(shown) => {
                secretary::locked(true);
                speak::poke();
                let screens = outputs::step();
                let effects = lock::set(shown, &screens);
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::A11y => {
                a11y::dump();
                Task::none()
            }
            ToShell::Unlocked => {
                secretary::locked(false);
                let effects = lock::end();
                speak::poke();
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::Curtain(shown) => {
                let effects = curtain::set(shown);
                speak::poke();
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::CurtainEnd => {
                let effects = curtain::end();
                speak::poke();
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            // Taken by the link itself, without a message.
            ToShell::LeaveCan(can) => {
                popover::session::set_can(can);
                Task::none()
            }
            ToShell::Leave(shown) => {
                popover::session::set_shown(Some(shown));
                Task::none()
            }
            ToShell::LeaveEnd => {
                popover::session::set_shown(None);
                Task::none()
            }
            ToShell::SessionMenu { output } => {
                popover::session::open(&output);
                Task::none()
            }
            ToShell::WindowMenu(menu) => {
                popover::window_menu::answer(menu);
                Task::none()
            }
            ToShell::WindowStates(states) => {
                windows::set_states(states);
                Task::none()
            }
            ToShell::BarKeyboard(output) => {
                popover::bar_keyboard(output.as_deref());
                Task::none()
            }
        }
    }

    /// A bar for `output` opens once iced_layershell knows that screen by
    /// its name ([`outputs`]): asked for by a name it does not know, it
    /// would be on no screen, and the compositor would put it under the
    /// pointer (#540). Asked again while it waits, it takes the new form;
    /// it waits until the name comes, or until the compositor takes the
    /// bar back (`NoBar`).
    fn wait_for(&mut self, output: String, form: Form) -> Task<Message> {
        tracing::debug!(output, "a bar waits for its screen's name");
        self.waiting.insert(output, form);
        self.screens_named()
    }

    /// The screens' next steps, from the update as [`outputs`] needs:
    /// the bars waiting for a screen now named open on it.
    fn screens_named(&mut self) -> Task<Message> {
        let known = outputs::step();
        // Locked (7.1): a screen plugged in gets the clock.
        let lock = Task::batch(
            popover::messages(lock::screens_known(&known))
                .into_iter()
                .map(Task::done),
        );
        if self.waiting.is_empty() {
            return lock;
        }
        tracing::debug!(?known, "the screens' names");
        let ready: Vec<(String, Form)> = match known {
            outputs::Known::Names(names) => self
                .waiting
                .extract_if(|output, _| names.contains(output))
                .collect(),
            outputs::Known::Unknowable => self.waiting.drain().collect(),
        };
        let bars = Task::batch(ready.into_iter().map(|(output, form)| {
            self.open_bar(OutputOption::OutputName(output.clone()), output, form)
        }));
        lock.chain(bars)
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
            tasks: window::Id::unique(),
            footer: window::Id::unique(),
        };
        bar.surface = bar.surface_width();
        bar.clock_surface = bar.clock_size();
        bar.levels_surface = bar.levels_size();
        let settings = NewLayerShellSettings {
            size: Some((bar.surface, 0)),
            layer: Layer::Top,
            anchor: side::anchor(Anchor::Right | Anchor::Top | Anchor::Bottom),
            // The compositor keeps the bar's space itself, shell or not.
            exclusive_zone: Some(0),
            // Top, right, bottom, left.
            margin: Some(side::margin((EDGE, EDGE, EDGE, 0))),
            keyboard_interactivity: KeyboardInteractivity::None,
            output_option: option.clone(),
            events_transparent: false,
            namespace: Some(NAMESPACE.to_owned()),
        };
        let (id, open) = Message::layershell_open(settings);
        // Inside the rim; pointer events go through to the bar, which hears
        // them for the clock's group too (the calendar's item).
        let clock = NewLayerShellSettings {
            size: Some(bar.clock_surface),
            layer: Layer::Top,
            anchor: side::anchor(Anchor::Right | Anchor::Top),
            exclusive_zone: Some(0),
            margin: Some(side::margin((EDGE + clock::SURFACE_TOP, EDGE + 1, 0, 0))),
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
            anchor: side::anchor(Anchor::Right | Anchor::Bottom),
            exclusive_zone: Some(0),
            margin: Some(side::margin((0, EDGE + 1, EDGE + levels::draw::BOTTOM, 0))),
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
            anchor: side::anchor(Anchor::Right | Anchor::Bottom),
            exclusive_zone: Some(0),
            margin: Some(side::margin(margin)),
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
            anchor: side::anchor(Anchor::Right | Anchor::Bottom),
            exclusive_zone: Some(0),
            margin: Some(side::margin(margin)),
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
            anchor: side::anchor(Anchor::Right | Anchor::Top),
            exclusive_zone: Some(0),
            margin: Some(side::margin(margin)),
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
        for id in [
            id,
            clock_id,
            levels_id,
            connections_id,
            desktops_id,
            agents_id,
        ] {
            side::opened(id);
        }
        // Under the agents: the to-do list.
        let (tasks_id, open_tasks) = tasks::draw::open(&bar.output, form, option.clone());
        bar.tasks = tasks_id;
        // Last, above the others: the footer.
        let (footer_id, open_footer) = footer::open(&bar.output, form, option.clone());
        bar.footer = footer_id;
        self.bars.insert(id, bar);
        BARS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(id);
        self.place_popovers(id);
        // The sign of what is in use, if something is (5.11), and the next
        // meeting's card, if one is left today (Day.2): opened after the
        // bar's own, in front of it.
        let output = self.bars.get(&id).map(|b| (b.output.clone(), b.form));
        let mut after = Vec::new();
        if let Some((output, form)) = output {
            after.extend(in_use::bar_opened(&output, form));
            after.extend(meeting::bar_opened(&output, form));
            after.extend(today::bar_opened(&output, form));
            after.extend(windows::bar_opened(&output, form));
            reminder::bar_opened(&output, form);
        }
        open.chain(open_clock)
            .chain(open_levels)
            .chain(open_connections)
            .chain(open_desktops)
            .chain(open_agents)
            .chain(Task::batch(open_tasks.into_iter().map(Task::done)))
            .chain(Task::batch(open_footer.into_iter().map(Task::done)))
            .chain(Task::batch(
                popover::messages(after).into_iter().map(Task::done),
            ))
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
            c.set_footer(&bar.output, bar.footer);
            c.set_tasks(&bar.output, bar.tasks);
        });
        notifications::bars_changed();
    }

    /// `form` for bar `id`, switching from where it is since `start`; at
    /// once without motion.
    /// `day slept SECONDS` (tests): the machine slept that long, its wall
    /// clock moved on while the monotonic clock did not. Only when the
    /// clock reads a time of its own (`HOCUS_CLOCK_AT`): the system's clock
    /// is never touched.
    fn slept(&mut self, seconds: u64) {
        if std::env::var_os("HOCUS_CLOCK_AT").is_none() {
            tracing::warn!("day slept: only with HOCUS_CLOCK_AT");
            return;
        }
        let slept = jiff::SignedDuration::from_secs(i64::try_from(seconds).unwrap_or(i64::MAX));
        self.wall.shift = self.wall.shift.saturating_add(slept);
        notifications::shift_by(slept);
        tracing::info!(seconds, "slept: the clock moves on");
    }

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
        let output = bar.output.clone();
        self.place_popovers(id);
        agents::peek::form(&output, form);
        speak::peek::moved();
        self.fit(id)
    }

    /// Bar `id`'s surface and its clock's to the sizes they need now, if
    /// they changed.
    fn fit(&mut self, id: window::Id) -> Task<Message> {
        let Some(bar) = self.bars.get_mut(&id) else {
            return Task::none();
        };
        // The tray first: the groups over it go up by what it takes.
        let mut tasks: Vec<Task<Message>> =
            tray::draw::sync(&bar.output, bar.form, bar.switch.is_some())
                .into_iter()
                .map(Task::done)
                .collect();
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
            tasks.push(Task::done(Message::MarginChange {
                id,
                margin: side::moved(id, margin),
            }));
        }
        tasks.extend(
            footer::form(&bar.output, bar.form, switching)
                .into_iter()
                .map(Task::done),
        );
        tasks.extend(
            self::tasks::draw::form(&bar.output, bar.form, switching)
                .into_iter()
                .map(Task::done),
        );
        popover::send(in_use::form(&bar.output, bar.form, switching));
        popover::send(meeting::form(&bar.output, bar.form, switching));
        let output = bar.output.clone();
        let form = bar.form;
        today::form(&output, form, switching);
        windows::form(&output, form, switching);
        // The column's groups to their places in the form: Today's rows,
        // the room the agents' rows have on a bar in the full form.
        column::moved();
        reminder::form(&output, form);
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
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = clock::Clock {
            bar: bar.output.clone(),
            style: self.clock,
            motion: self.look.motion,
            wall: self.wall.clone(),
            lang: self.lang,
            forms,
            bar_width: bar.width(at),
            surface_width: width as f32,
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
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
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
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
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The connections of the bar whose connections' surface is `id`.
    fn connections_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.connections == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - levels::draw::BOTTOM as f32 - 160.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
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
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
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
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
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
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The agents' group of the bar whose agents' surface is `id`.
    fn agents_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.agents == id)?;
        let at = self.now + AHEAD;
        // The bar's colour where the column puts the group, read as it draws.
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = agents::draw::Group {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: agents::Words::new(clock::time::Lang::of_messages()),
            backdrop,
            bar: bar.output.clone(),
            motion: self.look.motion,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The to-do list of the bar whose to-do list's surface is `id`.
    fn tasks_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.tasks == id)?;
        let at = self.now + AHEAD;
        // The bar's colour where the column puts the list, read as it draws.
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = tasks::draw::Group {
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: tasks::Words::new(clock::time::Lang::of_messages()),
            backdrop,
            bar: bar.output.clone(),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The footer of the bar whose footer's surface is `id`.
    fn footer_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self.bars.values().find(|b| b.footer == id)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - 40.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = footer::Footer {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: footer::Words::new(clock::time::Lang::of_messages()),
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The sign of what is in use (5.12), if its surface is `id`.
    fn in_use_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = in_use::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = column::TOP;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = in_use::draw::Sign {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            backdrop,
            motion: self.look.motion,
            words: in_use::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The next meeting (Day.2), if its surface is `id`.
    fn meeting_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = meeting::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = column::TOP;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = meeting::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            backdrop,
            motion: self.look.motion,
            words: meeting::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The windows over the desktops (#604), if their surface is `id`.
    fn windows_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = windows::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then_some(agents::draw::Backdrop {
            top: OPAQUE_TOP.get(),
            bottom: OPAQUE_BOTTOM.get(),
        });
        let program = windows::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            bar_width: bar.width(at),
            words: windows::Words::new(clock::time::Lang::of_messages()),
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// Today (Day.3), if its surface is `id`.
    fn today_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = today::bar_of(id)?;
        let bar = self.bars.values().find(|b| b.output == output)?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = today::shown(&output).1 + 40.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = today::draw::View {
            bar: bar.output.clone(),
            forms: bar.forms(at),
            backdrop,
            motion: self.look.motion,
            words: today::Words::new(clock::time::Lang::of_messages()),
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The tray of the bar whose tray's surface is `id` (5.14).
    fn tray_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let bar = self
            .bars
            .values()
            .find(|b| tray::draw::surface(&b.output) == Some(id))?;
        let at = self.now + AHEAD;
        let backdrop = (!self.look.glass).then(|| {
            let bar_height = if bar.height > 0.0 { bar.height } else { 1064.0 };
            let middle = bar_height - 100.0;
            mix(
                OPAQUE_TOP.get(),
                OPAQUE_BOTTOM.get(),
                (middle / bar_height).clamp(0.0, 1.0),
            )
        });
        let program = tray::draw::Tray {
            id,
            forms: bar.forms(at),
            bar_width: bar.width(at),
            motion: self.look.motion,
            backdrop,
        };
        Some(in_bar(
            bar,
            at,
            canvas(program)
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        ))
    }

    /// The lock (7.1), if its surface is `id`.
    fn lock_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        let output = lock::output_of(id)?;
        let layer = |layer| {
            canvas(lock::draw::View {
                layer,
                id,
                output: output.clone(),
                wall: self.wall.clone(),
                lang: self.lang,
                words: lock::words(),
            })
            .width(Length::Fill)
            .height(Length::Fill)
        };
        use lock::draw::Layer;
        Some(stack![layer(Layer::Back), layer(Layer::Front)].into())
    }

    /// The curtain's pill (7.2), if its surface is `id`.
    fn curtain_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !curtain::is_surface(id) {
            return None;
        }
        Some(
            canvas(curtain::draw::View {
                id,
                words: curtain::words(),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The authentication dialog (7.5), if its surface is `id`.
    fn auth_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !auth::is_surface(id) {
            return None;
        }
        Some(
            canvas(auth::draw::View {
                id,
                words: auth::words(),
                glass: self.look.glass,
                motion: self.look.motion,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The share's chooser (5.11), if its surface is `id`.
    fn share_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !share::is_surface(id) {
            return None;
        }
        Some(
            canvas(share::draw::View {
                id,
                words: share::words(),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The panda peeking out (4.5), if its surface is `id`.
    fn peek_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if let Some(bar) = speak::peek::bar_of(id) {
            return Some(
                canvas(speak::peek::View { bar })
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into(),
            );
        }
        let bar = agents::peek::bar_of(id)?;
        Some(
            canvas(agents::peek::View { bar })
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
        )
    }

    /// The launcher, or its toast, if its surface is `id`.
    fn launcher_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if launcher::toast::is_surface(id) {
            return Some(
                canvas(launcher::toast::View {
                    id,
                    software: self.software,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            );
        }
        if !launcher::is_surface(id) {
            return None;
        }
        Some(
            canvas(launcher::draw::View {
                id,
                words: launcher::Words::new(clock::time::Lang::of_messages()),
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The actions palette, if its surface is `id`.
    fn actions_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !actions::is_surface(id) {
            return None;
        }
        Some(
            canvas(actions::draw::View {
                id,
                words: actions::words(),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The secretary's drawer, if its surface is `id`.
    fn drawer_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !drawer::is_surface(id) {
            return None;
        }
        let drawn = canvas(drawer::draw::View {
            id,
            words: drawer::words(),
            glass: self.look.glass,
        })
        .width(Length::Fill)
        .height(Length::Fill);
        // An input method for its fields (#763).
        Some(field::ime::Asking::new(drawn, Box::new(move || drawer::ime(id))).into())
    }

    /// The emoji picker, if its surface is `id`.
    fn emoji_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if !emoji::is_surface(id) {
            return None;
        }
        Some(
            canvas(emoji::draw::View {
                id,
                words: emoji::words(),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
        )
    }

    /// The capture's toolbar (5.8), its thumbnail or its editor (5.9), if
    /// its surface is `id`.
    fn capture_view(&self, id: window::Id) -> Option<Element<'_, Message>> {
        if capture::thumb::is_surface(id) {
            return Some(
                canvas(capture::thumb::View {
                    id,
                    words: capture::words(),
                    software: self.software,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into(),
            );
        }
        if capture::editor::is_surface(id) {
            let layer = |layer| {
                canvas(capture::editor::draw::View {
                    id,
                    words: capture::words(),
                    glass: self.look.glass,
                    layer,
                })
                .width(Length::Fill)
                .height(Length::Fill)
            };
            use capture::editor::draw::Layer;
            return Some(stack![layer(Layer::Panel), layer(Layer::Over)].into());
        }
        if !capture::is_surface(id) {
            return None;
        }
        Some(
            canvas(capture::draw::View {
                id,
                words: capture::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
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
            popover::Item::Wifi if popover::wifi::widgets::on() => {
                popover::wifi::widgets::view(&popover::wifi::Panel {
                    id,
                    words: self.words,
                    glass: self.look.glass,
                    motion: self.look.motion,
                    software: self.software,
                })
            }
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
            popover::Item::Windows => canvas(popover::windows::Panel {
                id,
                words: windows::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::WindowMenu(window) => canvas(popover::window_menu::Panel {
                id,
                window,
                words: popover::window_menu::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Desktop { .. } | popover::Item::WindowsMore => {
                canvas(popover::desktop::Panel {
                    id,
                    words: self.words,
                    glass: self.look.glass,
                    software: self.software,
                })
                .width(Length::Fill)
                .height(Length::Fill)
                .into()
            }
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
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Notifications => canvas(popover::history::Panel {
                id,
                glass: self.look.glass,
                motion: self.look.motion,
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Clipboard => canvas(popover::clipboard::Panel {
                id,
                words: popover::clipboard::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::InUse => canvas(popover::in_use::Panel {
                id,
                words: in_use::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Tasks | popover::Item::TasksMore => canvas(popover::tasks::Panel {
                id,
                words: tasks::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::DayChoose => canvas(popover::setup::Panel {
                id,
                words: setup::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
                motion: self.look.motion && !self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::MeetingMark => canvas(popover::meeting::Panel {
                id,
                words: meeting::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::User => canvas(popover::session::Panel {
                id,
                words: popover::session::Words::new(clock::time::Lang::of_messages()),
                glass: self.look.glass,
                motion: self.look.motion && !self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Calendar => canvas(popover::calendar::Panel {
                id,
                glass: self.look.glass,
                software: self.software,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            // They open none.
            popover::Item::Tray(key) => canvas(popover::tray::Panel {
                id,
                key,
                glass: self.look.glass,
            })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
            popover::Item::Keyboard
            | popover::Item::Decline
            | popover::Item::Agent(_)
            | popover::Item::Menu
            | popover::Item::Meeting
            | popover::Item::Join
            | popover::Item::TodayLine(_)
            | popover::Item::TodayMore
            | popover::Item::TaskAdd
            | popover::Item::Task(_)
            | popover::Item::DayLater
            | popover::Item::Window(_)
            | popover::Item::WindowClose(_) => {
                return None;
            }
        };
        Some(panel)
    }

    /// A surface's widgets, whose pointer's shape is set again each time
    /// the pointer enters it ([`pointer`]), and which get the keys the
    /// compositor gives their surface ([`keys`]).
    fn view(&self, id: window::Id) -> Element<'_, Message> {
        keys::Held::new(
            id,
            pointer::Entered::new(self.surface_view(id)),
            Message::Keys,
        )
        .into()
    }

    fn surface_view(&self, id: window::Id) -> Element<'_, Message> {
        let Some(bar) = self.bars.get(&id) else {
            return self
                .clock_view(id)
                .or_else(|| self.levels_view(id))
                .or_else(|| self.connections_view(id))
                .or_else(|| self.desktops_view(id))
                .or_else(|| self.agents_view(id))
                .or_else(|| self.tasks_view(id))
                .or_else(|| self.footer_view(id))
                .or_else(|| self.tray_view(id))
                .or_else(|| tray::tip::view(id))
                .or_else(|| self.peek_view(id))
                .or_else(|| self.launcher_view(id))
                .or_else(|| self.emoji_view(id))
                .or_else(|| self.actions_view(id))
                .or_else(|| self.drawer_view(id))
                .or_else(|| self.capture_view(id))
                .or_else(|| self.auth_view(id))
                .or_else(|| self.share_view(id))
                .or_else(|| self.lock_view(id))
                .or_else(|| self.curtain_view(id))
                .or_else(|| self.in_use_view(id))
                .or_else(|| self.meeting_view(id))
                .or_else(|| self.today_view(id))
                .or_else(|| self.windows_view(id))
                .or_else(|| self.popover_view(id))
                .or_else(|| notifications::view(id, self.look.glass))
                .or_else(|| reminder::view(id, self.look.glass))
                .unwrap_or_else(|| Space::new().into());
        };
        let width = bar.width(self.now + AHEAD);
        let output = bar.output.clone();
        let (top, bottom) = if self.look.glass {
            (GLASS_TOP, GLASS_BOTTOM)
        } else {
            (OPAQUE_TOP, OPAQUE_BOTTOM)
        };
        let marks = canvas(theme::BarMarks)
            .width(Length::Fill)
            .height(Length::Fill);
        let column = container(marks)
            .width(width)
            .height(Length::Fill)
            .style(move |_| container::Style {
                background: Some(Background::Gradient(
                    gradient::Linear::new(iced::Degrees(180.0))
                        .add_stop(0.0, top.get())
                        .add_stop(1.0, bottom.get())
                        .into(),
                )),
                border: Border {
                    color: RIM.get(),
                    width: theme::border(),
                    radius: (RADIUS as f32).into(),
                },
                ..container::Style::default()
            });
        let bar = container(if side::left() {
            row![column, space::horizontal()]
        } else {
            row![space::horizontal(), column]
        })
        .width(Length::Fill)
        .height(Length::Fill);
        let pointer = canvas(popover::BarPointer { bar: output })
            .width(Length::Fill)
            .height(Length::Fill);
        stack![bar, pointer].into()
    }
}

/// `content`, one of `bar`'s surfaces in its column. While the bar
/// switches form those are the full form's width and draw the bar's
/// inside against their right edge; on the left (`[panel] side`) their
/// left edge is against the bar's, so their drawing is moved left by what
/// the bar lacks of the full form at `at`.
fn in_bar<'a>(bar: &Bar, at: Instant, content: Element<'a, Message>) -> Element<'a, Message> {
    if !side::left() || bar.switch.is_none() {
        return content;
    }
    let full = (Form::Full.width() - 2) as f32;
    let shift = (bar.width(at) - 2.0 - full).min(0.0);
    pin(container(content).width(full).height(Length::Fill))
        .x(shift)
        .into()
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
    // `text` by default, in the theme; a colour of the user's own as it
    // is, unless it is a near-white that the light theme would lose.
    let color = if look.color == ClockLook::DEFAULT_COLOR {
        theme::TEXT.get()
    } else {
        theme::ink(Color::from_rgb8(r, g, b))
    };
    clock::Style {
        font: clock_font(&look.font),
        color,
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
                            launcher::outside();
                            emoji::outside();
                            actions::close();
                            drawer::outside();
                            continue;
                        }
                        // The launcher opens, or closes: its own surface.
                        Ok(ToShell::Launcher { output, scale }) => {
                            launcher::toggle(output, scale);
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
                        // Its windows' titles: only an open list draws,
                        // and the windows over the tiles if they list it.
                        Ok(ToShell::Titles(titles)) => {
                            desktops::set_titles(titles);
                            continue;
                        }
                        // Its windows' order, and whether the bar shows
                        // them (#604): only their surface draws.
                        Ok(ToShell::Recent(recent)) => {
                            desktops::set_recent(recent);
                            continue;
                        }
                        // The screens a bar shows the desktops of (8.4):
                        // only their surface draws, resized to their rows.
                        Ok(ToShell::DeskScreens { bar, screens }) => {
                            desktops::set_screens(bar, screens);
                            continue;
                        }
                        // A window moved over a tile (8.5 B): only the
                        // tiles draw, that one lit.
                        Ok(ToShell::DropOver { bar, over }) => {
                            desktops::set_drop(bar, over);
                            continue;
                        }
                        Ok(ToShell::BarWindows(shown)) => {
                            windows::set_shown(shown);
                            continue;
                        }
                        // The desktops' names (6.14): the tiles and an
                        // open list draw.
                        Ok(ToShell::DesktopNames(names)) => {
                            desktops::set_names(names);
                            continue;
                        }
                        // What the column shows (6.4): only its parts draw.
                        Ok(ToShell::BarGroups(groups)) => {
                            column::set_groups(groups);
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
                        // The panda peeking out: its own surfaces only.
                        Ok(ToShell::Peek(peek)) => {
                            agents::peek::peek(peek);
                            continue;
                        }
                        Ok(ToShell::NoPeek) => {
                            agents::peek::back();
                            speak::peek::back();
                            continue;
                        }
                        Ok(ToShell::SecretaryPeek(id)) => {
                            speak::peek::show(id);
                            continue;
                        }
                        Ok(ToShell::ScreensMoved(moved)) => {
                            agents::peek::screens(moved);
                            continue;
                        }
                        // The rows' pandas or dots (6.16): their group
                        // alone draws.
                        Ok(ToShell::AgentPandas(pandas)) => {
                            agents::set_pandas(pandas);
                            continue;
                        }
                        // The bubbles' screen: their stack alone.
                        Ok(ToShell::Focus(screen)) => {
                            notifications::focus(screen);
                            continue;
                        }
                        // A copy for the clipboard's history, kept off
                        // this task; `Super+V`: the history's surface.
                        Ok(ToShell::Copied { app, clip }) => {
                            clipboard::copied(app, clip);
                            continue;
                        }
                        Ok(ToShell::CaptureCopied { capture, clip }) => {
                            clipboard::captured(capture, clip);
                            continue;
                        }
                        Ok(ToShell::Clipboard { output }) => {
                            popover::toggle_clipboard(&output);
                            continue;
                        }
                        // What the history keeps (6.17), and the
                        // Settings' clear: on its own thread.
                        Ok(ToShell::ClipboardRules(rules)) => {
                            clipboard::rules(rules);
                            continue;
                        }
                        Ok(ToShell::ClipboardClear) => {
                            clipboard::clear();
                            continue;
                        }
                        // The emoji picker: its own surface.
                        Ok(ToShell::Emoji {
                            output,
                            scale,
                            width,
                            height,
                            at,
                        }) => {
                            emoji::toggle(output, scale, (width, height), at);
                            continue;
                        }
                        Ok(ToShell::EmojiInserted) => {
                            emoji::inserted();
                            continue;
                        }
                        // The secretary's drawer: its own surface
                        // (secretary.md §5).
                        Ok(ToShell::SecretaryDrawer { output, width }) => {
                            drawer::toggle(&output, width);
                            continue;
                        }
                        // The actions palette: its own surface (#736).
                        Ok(ToShell::Actions(shown)) => {
                            actions::toggle(shown);
                            continue;
                        }
                        Ok(ToShell::ActionsEnd) => {
                            actions::close();
                            continue;
                        }
                        // The capture's toolbar, and its toast: their own
                        // surfaces (5.8).
                        Ok(ToShell::Capture(bar)) => {
                            capture::set(bar);
                            continue;
                        }
                        Ok(ToShell::CaptureEnd) => {
                            capture::end();
                            continue;
                        }
                        Ok(ToShell::Captured(captured)) => {
                            capture::captured(captured);
                            continue;
                        }
                        // Sharing (5.11): the chooser's surface, the sign's.
                        Ok(ToShell::ShareChoose(choose)) => {
                            share::set(choose);
                            continue;
                        }
                        Ok(ToShell::ShareChooseEnd(request)) => {
                            share::end(request);
                            continue;
                        }
                        // The authentication dialog (7.5): its place, and
                        // Caps Lock.
                        Ok(ToShell::AuthAt(at)) => {
                            auth::placed(at);
                            continue;
                        }
                        Ok(ToShell::CapsLock(on)) => {
                            auth::caps(on);
                            continue;
                        }
                        Ok(ToShell::Shares(shares)) => {
                            in_use::set_shares(shares);
                            continue;
                        }
                        // Recording (5.10): the bar's item, the thumbnail.
                        Ok(ToShell::Recording(shown)) => {
                            in_use::set_recording(Some(shown));
                            continue;
                        }
                        Ok(ToShell::RecordingEnd) => {
                            in_use::set_recording(None);
                            continue;
                        }
                        Ok(ToShell::Recorded(recorded)) => {
                            capture::recorded(recorded);
                            continue;
                        }
                        // A tray click's token: to the tray's thread.
                        Ok(ToShell::TrayToken { ask, token }) => {
                            tray::token(ask, token);
                            continue;
                        }
                        // A portal dialog's parent: to the thread asking.
                        Ok(ToShell::Exported { ask, handle }) => {
                            foreign::exported(ask, handle);
                            continue;
                        }
                        // The session menu (7.4): its popover alone.
                        Ok(ToShell::LeaveCan(can)) => {
                            popover::session::set_can(can);
                            continue;
                        }
                        Ok(ToShell::Leave(shown)) => {
                            popover::session::set_shown(Some(shown));
                            continue;
                        }
                        Ok(ToShell::LeaveEnd) => {
                            popover::session::set_shown(None);
                            continue;
                        }
                        Ok(ToShell::SessionMenu { output }) => {
                            popover::session::open(&output);
                            continue;
                        }
                        // `Super+B` (bar.md §11): the item's surface
                        // takes the keyboard and draws its ring.
                        Ok(ToShell::BarKeyboard(output)) => {
                            popover::bar_keyboard(output.as_deref());
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
