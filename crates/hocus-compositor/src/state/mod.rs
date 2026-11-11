//! Everything the compositor knows, owned by the event loop, and what it
//! does with windows: map them where the model says, focus them, unmap
//! them.

use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    time::Duration,
};

use anyhow::Context;
use smithay::{
    backend::session::{Session, libseat::LibSeatSession},
    desktop::{PopupManager, Space, Window, WindowSurfaceType, layer_map_for_output},
    input::{Seat, SeatState, keyboard::XkbConfig},
    output::Output,
    reexports::{
        calloop::{LoopHandle, LoopSignal},
        wayland_server::{
            DisplayHandle,
            backend::{ClientData, ClientId, DisconnectReason},
            protocol::wl_surface::WlSurface,
        },
    },
    utils::{Clock, Logical, Monotonic, Point, Rectangle},
    wayland::{
        compositor::CompositorClientState, seat::WaylandFocus,
        selection::primary_selection::PrimarySelectionState,
    },
    xwayland::X11Wm,
};

mod input;
mod menus;
mod protocols;
mod windows;

pub use input::{Gestures, Keyboard, Pointer};
pub use menus::{RadialMenu, Switching};
pub use protocols::Protocols;
pub use windows::{Arrivals, Departures, Snap};

use crate::{
    App, Config,
    cursor::CursorImages,
    decoration::Textures,
    desktops::Strip,
    model::{self, Model, Rect, WindowId},
    overlay::Overlay,
    programs::Programs,
    shortcuts::Ending,
    windows::{Managed, rect_of},
    wobble::Jelly,
};

/// Everything the compositor knows, owned by the event loop.
#[derive(Debug)]
pub struct State {
    pub display_handle: DisplayHandle,
    pub loop_handle: LoopHandle<'static, App>,
    pub loop_signal: LoopSignal,
    /// CLOCK_MONOTONIC, which frame callbacks and presentation times use.
    pub clock: Clock<Monotonic>,
    /// The motions' time while a test holds it (`Headless::hold_clock`):
    /// it moves only as the test steps it, so that a motion is seen at
    /// the same moments however slowly a loaded machine draws.
    pub held_clock: Option<Duration>,
    /// How far ahead of `clock` the motions' time is since a test let it
    /// go, having stepped it faster than it ran: it never goes back.
    pub clock_ahead: Duration,
    pub socket_path: PathBuf,
    pub runtime_dir: PathBuf,
    pub config: Config,
    /// In a window of another session: the desktop's shortcuts take
    /// `Alt+Super`, so that the host's own `Super` ones keep working
    /// (docs/design/windows.md §13).
    pub nested: bool,

    /// The Wayland protocols' states, which smithay's handlers ask for.
    pub protocols: Protocols,

    pub seat: Seat<State>,
    pub space: Space<Window>,
    pub popups: PopupManager,
    /// The windows, apart from smithay.
    pub model: Model,
    /// The mapped windows (and the minimised ones), by their id in the
    /// model, with their motions.
    pub windows: BTreeMap<WindowId, Managed>,
    /// Commits of the windows' surfaces so far, for tests: none at rest.
    pub window_commits: u64,
    /// The day's performance: what the loop counts, the recorder (#655).
    pub perf: crate::perf::Perf,
    /// xdg-foreign's handles and the dialogs made through them (#595).
    pub foreign: crate::foreign::Foreign,
    /// What draws windows' frames, and their pictures as they leave.
    pub textures: Textures,
    /// The window moving, and the one wobbling.
    pub moving: Option<WindowId>,
    pub wobble: Option<(WindowId, Jelly)>,
    /// A frame is asked for at this time, after a pause.
    pub wake: Option<Duration>,
    /// When the last frame was drawn: motions are judged as of then (see
    /// `tick`). Set while drawing, which borrows the state shared.
    pub drawn_at: std::cell::Cell<Option<Duration>>,
    /// Something changed on screen since the last frame.
    pub redraw: bool,
    /// Nothing moved as of the last frame drawn, as `tick` judged it then:
    /// that frame shows every motion at its end (the tests' `at_rest`).
    pub still: bool,
    /// Each screen's desktops, by output name (§12).
    pub strips: HashMap<String, Strip>,
    /// The bar's tiles, and a window moved over them (8.5 B).
    pub tile_drop: crate::tile_drop::TileDrop,
    /// Going to an agent, its streak under way (bar.md §8, 4.3).
    pub going: Option<crate::going::Going>,
    /// The halo around and over the bars (bar.md §6, 4.4).
    pub halo: crate::halo::Halo,
    /// The panda peeking out of the bar's edge (bar.md §7 C, 4.5).
    pub peek: crate::peek::Peeks,
    /// The notifications' bubbles, and their screen (5.3).
    pub bubbles: crate::bubbles::Bubbles,
    /// What the meetings' reminders were told (Day.6).
    pub reminders: crate::reminders::Reminders,
    /// The clipboard's history's copies and pastes (5.6).
    pub clipboard: crate::clipboard::Clipboard,
    /// The emoji picker's insertions, and the text inputs (5.7).
    pub emoji: crate::emoji::Emoji,
    /// The screen capture (5.8).
    pub capture: crate::capture::Capture,
    /// Screen sharing (5.11).
    pub share: crate::share::Sharing,
    /// The system's authentication dialog (7.5).
    pub auth: crate::auth::Auth,
    /// Screen recording (5.10).
    pub record: crate::record::Recording,
    /// The strokes, the arrows and the laser drawn on the screens
    /// (docs/design/ink.md).
    pub ink: crate::ink::Ink,
    /// The actions palette's target and what draws it (#736).
    pub actions: crate::actions::Palette,
    /// The magnifier (#424).
    pub magnifier: crate::magnifier::Magnifier,
    /// The windows' opacity's wheel and its pill (#735).
    pub opacity: crate::opacity::Opacity,
    /// The night light (#436).
    pub night: crate::night_light::Night,
    /// The screen reader started with the session (#734).
    pub screen_reader: crate::screen_reader::ScreenReader,
    /// The overview while it shows (8.5 A), and what draws it.
    pub overview: Option<crate::overview::Overview>,
    pub overview_paint: crate::overview_draw::OverviewPaint,
    /// The windows' states last told the shell (#723).
    pub told_window_states: Option<hocus_link::WindowStates>,
    /// A level key held down (volume, brightness), and the presses kept
    /// for the shell while it is away.
    pub levels: crate::levels::Levels,
    /// What draws the switcher, the desktops' dots and their text.
    pub overlay: Overlay,

    /// The X11 window manager once XWayland is ready, and its display.
    pub xwm: Option<X11Wm>,
    pub xdisplay: Option<u32>,
    pub programs: Programs,
    /// The session's start-up applications, once the bar shows (6.15).
    pub autostart: crate::session::autostart::Autostart,
    /// The session saved, and the last one put back (windows.md §14).
    pub session_restore: crate::session::restore::Restore,
    /// The bars' places and forms (docs/design/bar.md §2, §3).
    pub panel: crate::panel::Panel,
    /// The shell drawing them.
    pub shell: crate::shell::Shell,
    /// The agents and their sources (4.0).
    pub agents: crate::agents::Agents,
    /// The screens' settings, their trials and the Settings' lines (6.9).
    pub(crate) displays: crate::displays::Displays,
    /// The screens' identities and their sets (windows.md §12).
    pub(crate) screen_sets: crate::screen_sets::Sets,
    /// The mice and touchpads, and what they were given (6.12).
    pub input_devices: crate::input_devices::InputDevices,
    /// When the session is idle, and what holds it off (7.3).
    pub idle: crate::idle::Idleness,
    /// The render node of the GPU the compositor draws with, which the
    /// shell draws with too; `None` without one.
    pub gpu: Option<crate::backend::DevId>,
    /// The renderer can blur what is behind the bars (GLES).
    pub blur: bool,
    /// The glass behind each bar, as last blurred.
    pub panes: std::cell::RefCell<crate::glass::Panes>,
    /// The screens' wallpapers (6.3), drawn on a thread of their own.
    pub backgrounds: std::cell::RefCell<crate::wallpaper::Backgrounds>,
    /// The screens' wallpapers that move (#433).
    pub walls: std::cell::RefCell<crate::moving::Walls>,
    /// The seat's session on a virtual terminal, which switches terminals.
    pub session: Option<LibSeatSession>,
    /// The lock screen (7.1, docs/design/lock.md).
    pub lock: crate::lock::Lock,
    /// Locking on idle and before sleep, the curtain and the screens off
    /// (7.2).
    pub away: crate::away::Away,
    /// How the session ends: the session menu's confirmation (7.4).
    pub leave: crate::leave::Leave,
    /// Set once the compositor is asked to end, and why.
    pub ending: Option<Ending>,
    /// Windows on their way in: before their first buffer, and whether
    /// they may take the focus as they come.
    pub arrivals: Arrivals,
    /// The windows going: their pictures, taken as their surfaces went.
    pub departures: Departures,
    /// Where a dragged window would snap, and the seam's handle.
    pub snap: Snap,
    /// The pointer: what it shows, its grabs, the buttons kept.
    pub pointer: Pointer,
    /// The keyboard: the keys kept from clients, the layouts' codes.
    pub keyboard: Keyboard,
    /// The touchpad's swipes.
    pub gestures: Gestures,
    /// Alt+Tab, while it is held, and its panel vanishing (§11).
    pub switching: Switching,
    /// A window's radial menu while it is open, and as it goes (#723).
    pub radial_menu: RadialMenu,
}

impl State {
    pub fn new(
        dh: &DisplayHandle,
        loop_handle: LoopHandle<'static, App>,
        loop_signal: LoopSignal,
        socket_path: PathBuf,
        runtime_dir: PathBuf,
        config: Config,
        nested: bool,
    ) -> anyhow::Result<Self> {
        let mut seat_state = SeatState::new();
        let mut seat = seat_state.new_wl_seat(dh, "seat0");
        let keyboard = &config.keyboard;
        let names = config.xkb_names();
        tracing::info!(
            model = names.model,
            layout = names.layout,
            variant = names.variant,
            options = names.options,
            from = names.from,
            "keyboard"
        );
        let xkb = XkbConfig {
            model: &names.model,
            layout: &names.layout,
            variant: &names.variant,
            options: (!names.options.is_empty()).then(|| names.options.clone()),
            ..XkbConfig::default()
        };
        let added = seat.add_keyboard(xkb, keyboard.repeat_delay, keyboard.repeat_rate);
        let mut layout_codes = crate::layouts::codes(&names);
        if let Err(err) = added {
            layout_codes.clear();
            // A layout XKB does not know: its defaults, rather than no
            // keyboard at all.
            tracing::warn!(
                ?err,
                layout = names.layout,
                "keyboard layouts refused; XKB's defaults"
            );
            seat.add_keyboard(
                XkbConfig::default(),
                keyboard.repeat_delay,
                keyboard.repeat_rate,
            )
            .context("could not set up the keyboard")?;
        }
        // Num Lock on as the session opens (6.8), locked in the keymap's
        // state: every client hears it as it gets the keyboard.
        if keyboard.numlock
            && let Some(handle) = seat.get_keyboard()
        {
            let mut mods = handle.modifier_state();
            mods.num_lock = true;
            handle.set_modifier_state(mods);
        }
        seat.add_pointer();
        crate::overlay::load_font(&loop_handle);
        let appearance = &config.appearance;
        let cursor_images = CursorImages::start(
            &loop_handle,
            crate::cursor::Choice::of(
                appearance.cursor_theme.as_deref(),
                appearance.cursor_size,
                &config.cursor_env,
            ),
            config.cursor_env.search.clone(),
        );
        let primary_selection_state = PrimarySelectionState::new::<Self>(dh);
        let panel = crate::panel::Panel::load(config.state_dir.as_deref());
        let screen_sets = crate::screen_sets::Sets::load(config.state_dir.as_deref());
        let capture = crate::capture::Capture::load(config.state_dir.as_deref());
        let idle = crate::idle::Idleness::new(dh, config.idle.delay);
        let halo = crate::halo::Halo::with(crate::halo::Rules::of(&config.agents));
        Self::foreign_globals(dh);
        let protocols = Protocols::new(dh, seat_state, primary_selection_state);

        Ok(Self {
            display_handle: dh.clone(),
            loop_handle,
            loop_signal,
            clock: Clock::new(),
            held_clock: None,
            clock_ahead: Duration::ZERO,
            socket_path,
            runtime_dir,
            config,
            nested,
            protocols,
            seat,
            space: Space::default(),
            popups: PopupManager::default(),
            model: Model::default(),
            windows: BTreeMap::new(),
            window_commits: 0,
            perf: crate::perf::Perf::default(),
            foreign: crate::foreign::Foreign::default(),
            textures: Textures::default(),
            levels: crate::levels::Levels::default(),
            moving: None,
            wobble: None,
            wake: None,
            drawn_at: std::cell::Cell::new(None),
            still: false,
            redraw: true,
            strips: HashMap::new(),
            tile_drop: crate::tile_drop::TileDrop::default(),
            going: None,
            halo,
            peek: crate::peek::Peeks::default(),
            bubbles: crate::bubbles::Bubbles::default(),
            reminders: crate::reminders::Reminders::default(),
            clipboard: crate::clipboard::Clipboard::default(),
            emoji: crate::emoji::Emoji::new(dh),
            capture,
            share: crate::share::Sharing::default(),
            auth: crate::auth::Auth::default(),
            record: crate::record::Recording::default(),
            ink: crate::ink::Ink::default(),
            actions: crate::actions::Palette::default(),
            overview: None,
            overview_paint: crate::overview_draw::OverviewPaint::default(),
            magnifier: crate::magnifier::Magnifier::default(),
            opacity: crate::opacity::Opacity::default(),
            night: crate::night_light::Night::default(),
            screen_reader: crate::screen_reader::ScreenReader::default(),
            told_window_states: None,
            overlay: Overlay::new(),
            xwm: None,
            xdisplay: None,
            programs: Programs::default(),
            autostart: crate::session::autostart::Autostart::default(),
            session_restore: crate::session::restore::Restore::default(),
            panel,
            shell: crate::shell::Shell::default(),
            agents: crate::agents::Agents::default(),
            displays: crate::displays::Displays::default(),
            screen_sets,
            input_devices: crate::input_devices::InputDevices::default(),
            idle,
            gpu: None,
            blur: false,
            panes: std::cell::RefCell::default(),
            backgrounds: std::cell::RefCell::default(),
            walls: std::cell::RefCell::default(),
            session: None,
            lock: crate::lock::Lock::new(dh),
            away: crate::away::Away::default(),
            leave: crate::leave::Leave::default(),
            ending: None,
            radial_menu: RadialMenu::default(),
            switching: Switching::default(),
            gestures: Gestures::default(),
            keyboard: Keyboard::new(layout_codes),
            pointer: Pointer::new(cursor_images),
            snap: Snap::default(),
            departures: Departures::default(),
            arrivals: Arrivals::default(),
        })
    }

    /// Ctrl+Alt+F`vt`: shows another virtual terminal, on a session of its
    /// own.
    pub fn switch_vt(&mut self, vt: i32) {
        if let Some(session) = &mut self.session
            && let Err(err) = session.change_vt(vt)
        {
            tracing::warn!(vt, %err, "could not switch terminal");
        }
    }

    /// The output under the pointer, or the first one.
    pub fn output_under_pointer(&self) -> Option<Output> {
        let at = self.seat.get_pointer()?.current_location();
        self.space
            .output_under(at)
            .next()
            .or_else(|| self.space.outputs().next())
            .cloned()
    }

    /// What layer surfaces leave of `output`, global coordinates.
    pub fn non_exclusive_zone(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let geometry = self.space.output_geometry(output)?;
        let zone = layer_map_for_output(output).non_exclusive_zone();
        Some(Rectangle::new(geometry.loc + zone.loc, zone.size))
    }

    /// Where the desktop places windows on `output`: what layer surfaces
    /// leave of it less the bar's space when it has a bar, 8 px in.
    pub fn work_area(&self, output: &Output) -> Option<Rect> {
        let zone = self.non_exclusive_zone(output)?;
        let geometry = self.space.output_geometry(output)?;
        let bar = self.panel.width(&output.name());
        Some(model::work_area(
            rect_of(zone),
            rect_of(geometry),
            bar,
            self.bar_side(),
        ))
    }

    /// The window of `surface`, mapped or waiting for its first buffer.
    pub fn window_for(&self, surface: &WlSurface) -> Option<Window> {
        self.windows
            .values()
            .map(|m| &m.window)
            .chain(self.arrivals.unmapped.iter())
            .find(|w| w.wl_surface().is_some_and(|s| *s == *surface))
            .cloned()
    }

    /// The model's id of a mapped window.
    pub fn id_of(&self, window: &Window) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, m)| m.window == *window)
            .map(|(id, _)| *id)
    }

    /// The surface under `pos`, and where its origin is: layer surfaces of
    /// the overlay and top layers, then windows, then the bar's own
    /// surfaces unless it is in front (#519), then the bottom and
    /// background layers.
    pub fn surface_under(
        &self,
        pos: Point<f64, Logical>,
    ) -> Option<(WlSurface, Point<f64, Logical>)> {
        use crate::handlers::layer_shell::Stand;
        // Locked (7.1): the lock's surface, nothing else.
        if self.locked() {
            return self.lock_surface_under(pos);
        }
        let layer = |stand| self.layer_surface_under(pos, stand).map(|(s, p, _)| (s, p));
        layer(Stand::Above)
            .or_else(|| {
                // Where its surface's origin is, its geometry's corner less
                // its client-side shadow.
                self.space.element_under(pos).and_then(|(window, origin)| {
                    window
                        .surface_under(pos - origin.to_f64(), WindowSurfaceType::ALL)
                        .map(|(s, p)| (s, (p + origin).to_f64()))
                })
            })
            .or_else(|| layer(Stand::Below))
    }

    /// The window under `pos`, if no layer surface above the windows is.
    pub fn window_under(&self, pos: Point<f64, Logical>) -> Option<WindowId> {
        use crate::handlers::layer_shell::Stand;
        if self.locked() {
            return None;
        }
        if self.layer_surface_under(pos, Stand::Above).is_some() {
            return None;
        }
        let (window, _) = self.space.element_under(pos)?;
        self.id_of(window)
    }
}

/// What the compositor keeps per client.
#[derive(Debug, Default)]
pub struct ClientState {
    pub compositor_state: CompositorClientState,
    /// The shell's own connection, which the compositor made and handed
    /// it as it started it (`WAYLAND_SOCKET`): its lock surfaces are the
    /// only ones shown (7.1).
    pub shell: bool,
}

impl ClientData for ClientState {
    fn initialized(&self, _client_id: ClientId) {}
    fn disconnected(&self, _client_id: ClientId, _reason: DisconnectReason) {}
}
