//! The Hocuspanda shell (docs/architecture.md): every surface the desktop
//! draws itself, as wlr-layer-shell surfaces of a client of the
//! compositor's, drawn with iced. The compositor starts it, hands it its
//! Wayland connection and its line ([`hocus_link`]), and starts it again
//! when it ends; the windows stay.
//!
//! The bar (docs/design/bar.md) is a column against each screen's edge in
//! its full or its mini form ([`column`](mod@column)), with surfaces of its own for
//! what draws often: the clock ([`clock`]), the levels ([`levels`]), the
//! connections ([`network`], [`bluetooth`], [`layouts`]), the desktops
//! ([`desktops`]), the footer ([`footer`]), and the popovers opened from
//! its items ([`popover`]); the day's and the agents' groups ([`day`],
//! [`agents`], the panda peeking out of the bar's edge). Beside it: the
//! launcher, the actions palette, the emoji picker, the clipboard's
//! history, the notifications' bubbles and their server, the tray, the
//! music, captures' and sharing's surfaces, the lock screen's and the
//! authentication dialog's interface, the session menu.
//!
//! Each surface's state is a value of its own, changed by messages and
//! viewed as a function of it ([`surfaces`]); surfaces are built from
//! iced's widgets and the shell's kit ([`ui`], decision 0005), canvases
//! only where the look draws. Talking to the system (D-Bus, PipeWire, the
//! day's accounts) runs on workers that send owned results back; the view
//! never waits. The compositor blurs behind the bar; the shell draws the
//! tint over that blur, or an opaque background when there is none.

// What is left of #778 here: 197 indexed places, most in drawing code
// (pixels, glyphs, fixed tables). A panic ends the bar, which the
// compositor starts again; the lint is on everywhere else.
#![allow(
    clippy::indexing_slicing,
    reason = "#778: the shell's 197 sites are still to be gone through"
)]

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
mod emoji;
mod ending;
mod field;
mod footer;
mod foreign;
mod in_use;
mod items;
mod keys;
mod launcher;
mod layouts;
mod levels;
mod locale;
mod lock;
mod mcp;
mod meeting;
mod music;
mod network;
mod notifications;
mod outputs;
mod pandas;
mod paste;
mod pointer;
mod popover;
mod project;
mod reminder;
mod renderer;
mod setup;
mod share;
mod side;
mod surfaces;
mod tasks;
mod theme;
mod today;
mod transcribe;
mod tray;
mod ui;
mod user;
mod views;
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
    let filter = match hocus_panic::LOG.parse() {
        Ok(panics) => filter.add_directive(panics),
        Err(_) => filter,
    };
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(filter)
        .init();
    // A panic is in the log the compositor keeps of the shell (#778).
    hocus_panic::install();
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
    music::start();
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

use surfaces::{Look, Surface};

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
    /// Its clock's surface's size as last asked for.
    clock_surface: (u32, u32),
    /// Its levels' surface's size as last asked for.
    levels_surface: (u32, u32),
    /// The surfaces of its own its items opened with it ([`items`]), in
    /// the list's order: the clock's, the levels', the connections'
    /// (placed by [`network::draw`]), the desktops' ([`desktops::draw`]),
    /// the agents' group's ([`agents::draw`]), the to-do list's
    /// ([`tasks::draw`]), the footer's ([`footer`]).
    surfaces: Vec<(items::Kind, window::Id)>,
}

impl Bar {
    /// Item `kind`'s surface of its own, if it opened one with the bar.
    fn surface(&self, kind: items::Kind) -> Option<window::Id> {
        self.surfaces
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, id)| *id)
    }

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
    /// The curtain's pill before the lock (7.2).
    curtain: curtain::Curtain,
    /// The share's chooser (5.11).
    share: surfaces::Lent<share::Share>,
    /// The lock (7.1).
    lock: surfaces::Lent<lock::Locked>,
    /// The system's authentication dialog (7.5).
    auth: surfaces::Lent<auth::Auth>,
    /// The actions palette (#736).
    actions: surfaces::Lent<actions::Palette>,
    /// The emoji picker (5.7).
    emoji: surfaces::Lent<emoji::Picker>,
    /// A project's form and card, over the overview (docs/design/projects.md).
    project: surfaces::Lent<project::Projects>,
}

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
enum Message {
    Linked(mpsc::UnboundedSender<String>),
    FromCompositor(Box<ToShell>),
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
    /// The music players changed (s12): their tiles and popover follow.
    Music,
    /// The music tile was pressed.
    MusicTile(music::tile::Press),
    /// Asked of a surface from another thread (a screen reader's click).
    Surface(surfaces::Asked),
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
            lang: clock::time::Lang::of_time(),
            words: levels::model::Words::new(clock::time::Lang::of_messages()),
            curtain: curtain::Curtain::default(),
            share: surfaces::Lent::default(),
            lock: surfaces::Lent::default(),
            auth: surfaces::Lent::default(),
            actions: surfaces::Lent::default(),
            emoji: surfaces::Lent::default(),
            project: surfaces::Lent::default(),
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
        subscriptions.push(Subscription::run(surfaces::inbox));
        subscriptions.push(Subscription::run(notifications::effects));
        subscriptions.push(Subscription::run(tray::effects));
        subscriptions.push(Subscription::run(music::effects));
        subscriptions.push(Subscription::run(reminder::effects));
        subscriptions.push(bar_sizes());
        Subscription::batch(subscriptions)
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        self.now = Instant::now();
        match message {
            Message::Linked(sender) => {
                if self.software {
                    let _ = sender.unbounded_send(ToCompositor::Software.line());
                }
                popover::linked(sender.clone());
                self.to_compositor = Some(sender);
                meeting::linked();
                music::linked();
                clipboard::linked();
                Task::none()
            }
            Message::FromCompositor(message) => self.compositor_says(*message),
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
            Message::Music => {
                music::changed();
                Task::none()
            }
            Message::MusicTile(press) => {
                music::tile::pressed(press);
                Task::none()
            }
            Message::Surface(asked) => match asked {
                surfaces::Asked::Share(act) => {
                    let effects = self
                        .share
                        .update(share::Said::Act { on: None, act }, self.now);
                    Task::batch(popover::messages(effects).into_iter().map(Task::done))
                }
                surfaces::Asked::Auth(come) => self.auth_told(auth::Told::Come(come)),
                surfaces::Asked::Actions(come) => self.actions_told(come),
                surfaces::Asked::Emoji(come) => self.emoji_told(come),
                surfaces::Asked::Project(come) => self.project_told(come),
            },
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

    // Over 200 lines: one arm per kind of message from the compositor.
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
                self.lang = clock::time::Lang::of_time();
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
                    reminder::forget(&bar.output);
                    // The items' surfaces that came and went, then the
                    // bar's own, then the bar.
                    let mut gone: Vec<window::Id> = items::ITEMS
                        .iter()
                        .filter_map(|item| forget_item(item.kind, &bar.output))
                        .collect();
                    gone.extend(bar.surfaces.iter().rev().map(|(_, surface)| *surface));
                    gone.push(id);
                    Some(Task::batch(
                        gone.into_iter()
                            .map(|surface| Task::done(Message::RemoveWindow(surface))),
                    ))
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
                self.project_told(project::Come::Close)
            }
            ToShell::ProjectForm(form) => self.project_told(project::Come::Form(form)),
            ToShell::ProjectCard(card) => self.project_told(project::Come::Card(card)),
            ToShell::ProjectMade {
                output,
                desktop,
                name,
            } => self.project_told(project::Come::Made {
                output,
                desktop,
                name,
            }),
            ToShell::ProjectRefused(why) => self.project_told(project::Come::Refused(why)),
            ToShell::ProjectEnd => self.project_told(project::Come::Close),
            ToShell::Projects(rows) => {
                project::set_rows(rows);
                Task::none()
            }
            ToShell::ProjectConfirm(shown) => self.project_told(project::Come::Confirm(shown)),
            ToShell::ProjectClosed {
                output,
                name,
                kept,
                stayed,
            } => self.project_told(project::Come::Closed {
                output,
                name,
                kept,
                stayed,
            }),
            ToShell::ProjectNoRoom { output, name } => {
                launcher::no_room(output, &name);
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
            ToShell::Mcp(shown) => Task::batch(
                popover::messages(mcp::set(*shown))
                    .into_iter()
                    .map(Task::done),
            ),
            ToShell::Peek(peek) => {
                agents::peek::peek(peek);
                Task::none()
            }
            ToShell::NoPeek => {
                agents::peek::back();
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
                scale: _,
                width,
                height,
                at,
            } => self.emoji_told(emoji::Come::Toggle {
                output,
                screen: (width, height),
                at,
            }),
            ToShell::EmojiInserted => self.emoji_told(emoji::Come::Inserted),
            ToShell::Actions(shown) => self.actions_told(actions::Come::Toggle(shown)),
            ToShell::ActionsEnd => self.actions_told(actions::Come::Close),
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
            // A message (every surface's view built again): the chooser
            // opens a surface, which is one.
            ToShell::ShareChoose(choose) => {
                let effects = self.share.update(share::Said::Choose(choose), self.now);
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::ShareChooseEnd(request) => {
                let effects = self.share.update(share::Said::End(request), self.now);
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::AuthAt(at) => self.auth_told(auth::Told::Placed(at)),
            ToShell::CapsLock(on) => self.auth_told(auth::Told::Caps(on)),
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
            ToShell::Transcribing(shown) => {
                transcribe::set(Some(shown));
                Task::none()
            }
            ToShell::TranscribingEnd => {
                transcribe::set(None);
                Task::none()
            }
            ToShell::Recorded(recorded) => {
                capture::recorded(recorded);
                Task::none()
            }
            ToShell::Day(ask) => {
                if let hocus_link::DayAsk::Slept(seconds) = ask {
                    self.slept(seconds);
                }
                day::ask(ask);
                Task::none()
            }
            ToShell::Join => {
                reminder::join_key();
                Task::none()
            }
            ToShell::FellBack { which, program } => {
                notifications::fell_back(which, &program);
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
                let screens = outputs::step();
                let effects = self
                    .lock
                    .update(lock::Told::Shown(shown, screens), self.now);
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::A11y => {
                a11y::dump();
                Task::none()
            }
            ToShell::Unlocked => {
                let effects = self.lock.update(lock::Told::End, self.now);
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::Curtain(shown) => {
                let effects = self
                    .curtain
                    .update(curtain::Said::Shown(shown), Instant::now());
                Task::batch(popover::messages(effects).into_iter().map(Task::done))
            }
            ToShell::CurtainEnd => {
                let effects = self.curtain.update(curtain::Said::End, Instant::now());
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
            ToShell::Journal { output } => {
                popover::open_journal(&output);
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
            popover::messages(
                self.lock
                    .update(lock::Told::Screens(known.clone()), self.now),
            )
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
            clock_surface: (0, 0),
            levels_surface: (0, 0),
            surfaces: Vec::new(),
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
        side::opened(id);
        // The items' surfaces of their own, in the list's order: each
        // opened after, so in front of, those before it.
        let mut opening = vec![open];
        for item in items::ITEMS {
            if let Some((surface, open)) = open_item(&bar, item.kind, &option) {
                bar.surfaces.push((item.kind, surface));
                opening.push(open);
            }
        }
        self.bars.insert(id, bar);
        BARS.lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(id);
        self.place_popovers(id);
        // The items that come and go (the sign of what is in use, the next
        // meeting's card, …), if they are to show: opened after the bar's
        // own, in front of it.
        let output = self.bars.get(&id).map(|b| (b.output.clone(), b.form));
        let mut after = Vec::new();
        if let Some((output, form)) = output {
            for item in items::ITEMS {
                after.extend(item_opened(item.kind, &output, form));
            }
            reminder::bar_opened(&output, form);
        }
        opening.push(Task::batch(
            popover::messages(after).into_iter().map(Task::done),
        ));
        opening
            .into_iter()
            .reduce(Task::chain)
            .unwrap_or_else(Task::none)
    }

    /// Where bar `id` is, for the popovers.
    fn place_popovers(&self, id: window::Id) {
        let Some(bar) = self.bars.get(&id) else {
            return;
        };
        let of = |kind| bar.surface(kind);
        popover::with(|c| {
            if let (Some(levels), Some(connections), Some(desktops), Some(agents)) = (
                of(items::Kind::Levels),
                of(items::Kind::Connections),
                of(items::Kind::Desktops),
                of(items::Kind::Agents),
            ) {
                c.place(
                    &bar.output,
                    bar.form,
                    id,
                    (levels, connections, desktops, agents),
                );
            }
            if let Some(footer) = of(items::Kind::Footer) {
                c.set_footer(&bar.output, footer);
            }
            if let Some(tasks) = of(items::Kind::Tasks) {
                c.set_tasks(&bar.output, tasks);
            }
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
        self.fit(id)
    }

    /// Bar `id`'s surface and its clock's to the sizes they need now, if
    /// they changed.
    fn fit(&mut self, id: window::Id) -> Task<Message> {
        let Some(bar) = self.bars.get_mut(&id) else {
            return Task::none();
        };
        let mut tasks: Vec<Task<Message>> = Vec::new();
        let width = bar.surface_width();
        if bar.surface != width {
            bar.surface = width;
            tasks.push(Task::done(Message::SizeChange {
                id,
                size: (width, 0),
            }));
        }
        // Each item to its form, in the list's order (the tray first: the
        // groups over it go up by what it takes).
        for item in items::ITEMS {
            tasks.extend(item_form(bar, item.kind));
        }
        let output = bar.output.clone();
        let form = bar.form;
        // The column's groups to their places in the form: Today's rows,
        // the room the agents' rows have on a bar in the full form.
        column::moved();
        reminder::form(&output, form);
        Task::batch(tasks)
    }
}

/// Item `kind`'s surface of its own, opened with bar `bar` (a layer
/// surface on `option`'s screen) and placed where its kind says: its id
/// and what opens it; none for an item that opens as it comes.
fn open_item(
    bar: &Bar,
    kind: items::Kind,
    option: &OutputOption,
) -> Option<(window::Id, Task<Message>)> {
    use items::Kind;
    // A surface of the bar's, against its edge: `anchor` and `margin`
    // said for a bar on the right.
    let layer = |size, anchor, margin, events_transparent, namespace: &str| NewLayerShellSettings {
        size: Some(size),
        layer: Layer::Top,
        anchor: side::anchor(anchor),
        exclusive_zone: Some(0),
        margin: Some(side::margin(margin)),
        keyboard_interactivity: KeyboardInteractivity::None,
        output_option: option.clone(),
        events_transparent,
        namespace: Some(namespace.to_owned()),
    };
    // One placed by its own module, which knows its id first.
    let placed = |id, settings| Task::done(Message::NewLayerShell { settings, id });
    let opened = match kind {
        Kind::Clock => {
            // Inside the rim; pointer events go through to the bar, which
            // hears them for the clock's group too (the calendar's item).
            Message::layershell_open(layer(
                bar.clock_surface,
                Anchor::Right | Anchor::Top,
                (EDGE + clock::SURFACE_TOP, EDGE + 1, 0, 0),
                true,
                CLOCK_NAMESPACE,
            ))
        }
        Kind::Levels => {
            // Its items take the pointer: the sound's opens its popover.
            Message::layershell_open(layer(
                bar.levels_surface,
                Anchor::Right | Anchor::Bottom,
                (0, EDGE + 1, EDGE + levels::draw::BOTTOM, 0),
                false,
                LEVELS_NAMESPACE,
            ))
        }
        Kind::Connections => {
            // Above the levels, following their group's top; its row
            // opens the Wi-Fi's popover.
            let id = window::Id::unique();
            let (size, margin) = network::draw::opened(&bar.output, id, bar.form);
            let settings = layer(
                size,
                Anchor::Right | Anchor::Bottom,
                margin,
                false,
                CONNECTIONS_NAMESPACE,
            );
            (id, placed(id, settings))
        }
        Kind::Desktops => {
            // Above the connections, following their group's top; a tile
            // goes to its desktop.
            let id = window::Id::unique();
            let (size, margin) = desktops::draw::opened(&bar.output, id, bar.form);
            let settings = layer(
                size,
                Anchor::Right | Anchor::Bottom,
                margin,
                false,
                DESKTOPS_NAMESPACE,
            );
            (id, placed(id, settings))
        }
        Kind::Agents => {
            // Under the clock, in the column.
            let id = window::Id::unique();
            let (size, margin) = agents::draw::opened(&bar.output, id, bar.form);
            let settings = layer(
                size,
                Anchor::Right | Anchor::Top,
                margin,
                false,
                AGENTS_NAMESPACE,
            );
            (id, placed(id, settings))
        }
        Kind::Tasks => {
            let (id, open) = tasks::draw::open(&bar.output, bar.form, option.clone());
            return Some((id, Task::batch(open.into_iter().map(Task::done))));
        }
        Kind::Footer => {
            // Last of the bar's own, above the others.
            let (id, open) = footer::open(&bar.output, bar.form, option.clone());
            return Some((id, Task::batch(open.into_iter().map(Task::done))));
        }
        Kind::Tray | Kind::InUse | Kind::Meeting | Kind::Today | Kind::Windows | Kind::Music => {
            return None;
        }
    };
    // Mirrored on the left as they move ([`side`]).
    side::opened(opened.0);
    Some(opened)
}

/// What item `kind` opens on bar `bar`, in `form`, once the bar is: the
/// items that come and go, if they are to show.
fn item_opened(kind: items::Kind, bar: &str, form: Form) -> Vec<popover::Effect> {
    use items::Kind;
    match kind {
        Kind::InUse => in_use::bar_opened(bar, form),
        Kind::Meeting => meeting::bar_opened(bar, form),
        Kind::Today => today::bar_opened(bar, form),
        Kind::Windows => windows::bar_opened(bar, form),
        Kind::Music => music::bar_opened(bar, form),
        Kind::Clock
        | Kind::Levels
        | Kind::Connections
        | Kind::Desktops
        | Kind::Agents
        | Kind::Tasks
        | Kind::Footer
        | Kind::Tray => Vec::new(),
    }
}

/// Item `kind` of `bar` to the bar's form (both forms' room while it
/// switches): what that asks of its surfaces.
fn item_form(bar: &mut Bar, kind: items::Kind) -> Vec<Task<Message>> {
    use items::Kind;
    let switching = bar.switch.is_some();
    let (output, form) = (bar.output.as_str(), bar.form);
    let messages = |messages: Vec<Message>| messages.into_iter().map(Task::done).collect();
    // A surface its module placed: its size and its margins.
    let placed = |placed: Option<network::draw::Placed>| -> Vec<Task<Message>> {
        placed.map_or_else(Vec::new, |(id, size, margin)| {
            vec![
                Task::done(Message::SizeChange { id, size }),
                Task::done(Message::MarginChange {
                    id,
                    margin: side::moved(id, margin),
                }),
            ]
        })
    };
    match kind {
        Kind::Tray => messages(tray::draw::sync(output, form, switching)),
        Kind::Clock => {
            let size = bar.clock_size();
            match bar.surface(kind).filter(|_| bar.clock_surface != size) {
                Some(id) => {
                    bar.clock_surface = size;
                    vec![Task::done(Message::SizeChange { id, size })]
                }
                None => Vec::new(),
            }
        }
        Kind::Levels => {
            let size = bar.levels_size();
            match bar.surface(kind).filter(|_| bar.levels_surface != size) {
                Some(id) => {
                    bar.levels_surface = size;
                    vec![Task::done(Message::SizeChange { id, size })]
                }
                None => Vec::new(),
            }
        }
        Kind::Connections => placed(network::draw::form(output, form, switching)),
        Kind::Desktops => placed(desktops::draw::form(output, form, switching)),
        Kind::Agents => placed(agents::draw::form(output, form, switching)),
        Kind::Tasks => messages(tasks::draw::form(output, form, switching)),
        Kind::Footer => messages(footer::form(output, form, switching)),
        Kind::InUse => {
            popover::send(in_use::form(output, form, switching));
            Vec::new()
        }
        Kind::Meeting => {
            popover::send(meeting::form(output, form, switching));
            Vec::new()
        }
        Kind::Today => {
            today::form(output, form, switching);
            Vec::new()
        }
        Kind::Windows => {
            windows::form(output, form, switching);
            Vec::new()
        }
        Kind::Music => {
            music::form(output, form, switching);
            Vec::new()
        }
    }
}

/// Item `kind` forgets bar `bar`, gone: the surface it opened as it came,
/// if any, to remove (the bar's own are its [`Bar::surfaces`]).
fn forget_item(kind: items::Kind, bar: &str) -> Option<window::Id> {
    use items::Kind;
    match kind {
        Kind::Clock | Kind::Levels => None,
        Kind::Connections => {
            network::draw::forget(bar);
            None
        }
        Kind::Desktops => {
            desktops::draw::forget(bar);
            None
        }
        Kind::Agents => {
            agents::draw::forget(bar);
            agents::peek::forget(bar);
            None
        }
        Kind::Tasks => {
            tasks::draw::forget(bar);
            None
        }
        Kind::Footer => {
            footer::forget(bar);
            None
        }
        Kind::Tray => tray::draw::forget(bar),
        Kind::InUse => in_use::forget(bar),
        Kind::Meeting => meeting::forget(bar),
        Kind::Today => today::forget(bar),
        Kind::Windows => windows::forget(bar),
        Kind::Music => music::forget(bar),
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
            // The column's room is the bar's height less its groups': what
            // was fit before the bar had its size is fit again. The day
            // read before the bar opened, Today kept the rows of a bar as
            // tall as the default, over the desktops on a short screen
            // (#769).
            column::moved();
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
// Over 200 lines: its loop dispatches each kind of line the compositor
// says outside of iced's messages.
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
        let mut lines = BufReader::new(read).lines();
        // The hello (`hocus_link::wire`): this shell's version, then the
        // compositor's. On another version (a package upgraded under the
        // session) nothing more is said nor read: its messages would be
        // misread. The compositor ends this shell, or the session does.
        let hello = format!("{}\n", hocus_link::wire::hello());
        if let Err(err) = write.write_all(hello.as_bytes()).await {
            tracing::warn!(%err, "could not write to the compositor");
            let _ = output.send(Message::Unlinked).await;
            return;
        }
        match lines.next_line().await {
            Ok(Some(first)) => {
                let theirs = hocus_link::wire::said_version(&first);
                if theirs != Some(hocus_link::VERSION) {
                    tracing::error!(
                        compositor = ?theirs,
                        shell = hocus_link::VERSION,
                        "the compositor speaks another version of the line: \
                         nothing is read from it nor said to it"
                    );
                    while let Ok(Some(_)) = lines.next_line().await {}
                    let _ = output.send(Message::Unlinked).await;
                    return;
                }
            }
            Ok(None) => {
                let _ = output.send(Message::Unlinked).await;
                return;
            }
            Err(err) => {
                tracing::warn!(%err, "could not read from the compositor");
                let _ = output.send(Message::Unlinked).await;
                return;
            }
        }
        let (sender, mut outgoing) = mpsc::unbounded::<String>();
        // A change of level wakes the levels' surfaces through the
        // compositor, without a message that would draw every surface.
        // The brightness's and the battery's popovers show them too.
        let waker = sender.clone();
        levels::on_change(move || {
            popover::changed(popover::Item::Brightness);
            popover::changed(popover::Item::Battery);
            let wake = ToCompositor::Wake(LEVELS_NAMESPACE.to_owned());
            waker.unbounded_send(wake.line()).is_ok()
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
            waker.unbounded_send(wake.line()).is_ok()
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
            waker.unbounded_send(wake.line()).is_ok()
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
        loop {
            match lines.next_line().await {
                Ok(Some(line)) => {
                    // A level key goes to its service: no message, no
                    // drawing; the gauge shows what the service then reads.
                    let message = match ToShell::read(&line) {
                        Ok(message) => message,
                        // A kind this shell does not know, or a line that
                        // does not read: said and skipped.
                        Err(err) => {
                            tracing::warn!(%err, "a line from the compositor is skipped");
                            continue;
                        }
                    };
                    match message {
                        ToShell::Level(key) => {
                            levels::key(key);
                            continue;
                        }
                        // A press outside closes the popover: no message
                        // either, the controller draws what changes.
                        ToShell::Outside => {
                            popover::event(popover::Event::Outside);
                            launcher::outside();
                            emoji::outside();
                            actions::close();
                            project::close();
                            continue;
                        }
                        // The launcher opens, or closes: its own surface.
                        ToShell::Launcher { output, scale } => {
                            launcher::toggle(output, scale);
                            continue;
                        }
                        // The other screens: only an open brightness's
                        // popover draws for them.
                        ToShell::Screens(names) => {
                            screens(names);
                            continue;
                        }
                        // The layouts' names in the bar's language, read
                        // from XKB's catalog off the loop; then a message,
                        // once a connection: the surfaces take their sizes.
                        ToShell::Layouts(list) => {
                            let lang = clock::time::Lang::of_messages();
                            let made =
                                tokio::task::spawn_blocking(move || layouts::made(&list, lang))
                                    .await
                                    .unwrap_or_default();
                            let _ = output.send(Message::Layouts(made)).await;
                            continue;
                        }
                        // The active layout: only the row draws.
                        ToShell::Layout(index) => {
                            layouts::activate(index);
                            continue;
                        }
                        // A screen's desktops: only their surface draws
                        // (resized when their rows change).
                        ToShell::Desktops(screen) => {
                            desktops::set(screen);
                            continue;
                        }
                        // Its windows' titles: only an open list draws,
                        // and the windows over the tiles if they list it.
                        ToShell::Titles(titles) => {
                            desktops::set_titles(titles);
                            continue;
                        }
                        // Its windows' order, and whether the bar shows
                        // them (#604): only their surface draws.
                        ToShell::Recent(recent) => {
                            desktops::set_recent(recent);
                            continue;
                        }
                        // The screens a bar shows the desktops of (8.4):
                        // only their surface draws, resized to their rows.
                        ToShell::DeskScreens { bar, screens } => {
                            desktops::set_screens(bar, screens);
                            continue;
                        }
                        // A window moved over a tile (8.5 B): only the
                        // tiles draw, that one lit.
                        ToShell::DropOver { bar, over } => {
                            desktops::set_drop(bar, over);
                            continue;
                        }
                        ToShell::BarWindows(shown) => {
                            windows::set_shown(shown);
                            continue;
                        }
                        // The desktops' names (6.14): the tiles and an
                        // open list draw.
                        ToShell::DesktopNames(names) => {
                            desktops::set_names(names);
                            continue;
                        }
                        // What the column shows (6.4): only its parts draw.
                        ToShell::BarGroups(groups) => {
                            column::set_groups(groups);
                            continue;
                        }
                        // The agents' tools and the agents: only their
                        // group draws (resized when its height changes),
                        // and an open Sources or consent popover.
                        ToShell::AgentTools(tools) => {
                            agents::set_tools(tools);
                            continue;
                        }
                        ToShell::Agents(list) => {
                            agents::set_agents(list);
                            continue;
                        }
                        // The panda peeking out: its own surfaces only.
                        ToShell::Peek(peek) => {
                            agents::peek::peek(peek);
                            continue;
                        }
                        ToShell::NoPeek => {
                            agents::peek::back();
                            continue;
                        }
                        // A developer's program let go of: a bubble, the
                        // notifications' own surface.
                        ToShell::FellBack { which, program } => {
                            notifications::fell_back(which, &program);
                            continue;
                        }
                        ToShell::ScreensMoved(moved) => {
                            agents::peek::screens(moved);
                            continue;
                        }
                        // The rows' pandas or dots (6.16): their group
                        // alone draws.
                        ToShell::AgentPandas(pandas) => {
                            agents::set_pandas(pandas);
                            continue;
                        }
                        // The bubbles' screen: their stack alone.
                        ToShell::Focus(screen) => {
                            notifications::focus(screen);
                            continue;
                        }
                        // A copy for the clipboard's history, kept off
                        // this task; `Super+V`: the history's surface.
                        ToShell::Copied { app, clip } => {
                            clipboard::copied(app, clip);
                            continue;
                        }
                        ToShell::CaptureCopied { capture, clip } => {
                            clipboard::captured(capture, clip);
                            continue;
                        }
                        ToShell::Clipboard { output } => {
                            popover::toggle_clipboard(&output);
                            continue;
                        }
                        // What the history keeps (6.17), and the
                        // Settings' clear: on its own thread.
                        ToShell::ClipboardRules(rules) => {
                            clipboard::rules(rules);
                            continue;
                        }
                        ToShell::ClipboardClear => {
                            clipboard::clear();
                            continue;
                        }
                        // The capture's toolbar, and its toast: their own
                        // surfaces (5.8).
                        ToShell::Capture(bar) => {
                            capture::set(bar);
                            continue;
                        }
                        ToShell::CaptureEnd => {
                            capture::end();
                            continue;
                        }
                        ToShell::Captured(captured) => {
                            capture::captured(captured);
                            continue;
                        }
                        ToShell::Shares(shares) => {
                            in_use::set_shares(shares);
                            continue;
                        }
                        // Recording (5.10): the bar's item, the thumbnail.
                        ToShell::Recording(shown) => {
                            in_use::set_recording(Some(shown));
                            continue;
                        }
                        ToShell::RecordingEnd => {
                            in_use::set_recording(None);
                            continue;
                        }
                        // A meeting's transcription (docs/design/meetings.md).
                        ToShell::Transcribing(shown) => {
                            transcribe::set(Some(shown));
                            continue;
                        }
                        ToShell::TranscribingEnd => {
                            transcribe::set(None);
                            continue;
                        }
                        ToShell::Recorded(recorded) => {
                            capture::recorded(recorded);
                            continue;
                        }
                        // A tray click's token: to the tray's thread.
                        ToShell::TrayToken { ask, token } => {
                            tray::token(ask, token);
                            continue;
                        }
                        // A portal dialog's parent: to the thread asking.
                        ToShell::Exported { ask, handle } => {
                            foreign::exported(ask, handle);
                            continue;
                        }
                        // The session menu (7.4): its popover alone.
                        ToShell::LeaveCan(can) => {
                            popover::session::set_can(can);
                            continue;
                        }
                        ToShell::Leave(shown) => {
                            popover::session::set_shown(Some(shown));
                            continue;
                        }
                        ToShell::LeaveEnd => {
                            popover::session::set_shown(None);
                            continue;
                        }
                        ToShell::SessionMenu { output } => {
                            popover::session::open(&output);
                            continue;
                        }
                        ToShell::Journal { output } => {
                            popover::open_journal(&output);
                            continue;
                        }
                        // `Super+B` (bar.md §11) is not taken here: it
                        // would overtake a bar's form still on its way as
                        // a message (`Super+Shift+B` just before), and the
                        // form, coming after, would take the keyboard back
                        // from the bar. It goes as a message, in its turn.
                        other => {
                            let _ = output.send(Message::FromCompositor(Box::new(other))).await;
                        }
                    }
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
