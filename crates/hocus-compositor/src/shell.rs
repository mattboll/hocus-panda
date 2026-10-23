//! The shell in the compositor: the widget column on the primary output,
//! fed with the desktop's state after each dispatch, drawn into memory
//! buffers when something it shows changed, given the pointer when over it,
//! and its actions carried out here.

use hocus_api::{Rect as ApiRect, UiNode};
use hocus_shell::{
    Action as ShellAction, ActiveWindow, Column, ColumnState, Lang, ScreenDesktops, WallTime,
    column::{MARGIN, View, WIDTH},
};
use hocus_ui::{
    AccessNode, Input, Point as UiPoint, Theme,
    motion::{Animated, Curve, Easing, LARGE},
};
use smithay::{
    backend::{
        allocator::Fourcc,
        renderer::{
            ImportAll, ImportMem, Renderer,
            element::{
                Kind,
                memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            },
        },
    },
    output::Output,
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Size},
};

use crate::{
    State,
    blur::{Spot, Surface, spots},
    desktops::title_and_app_id,
    scene::SceneElement,
    window_mode::WindowMode,
};

/// The wall-clock time a virtual clock starts at: a Wednesday afternoon,
/// the same in every test.
const VIRTUAL_START: (i16, i8, i8, i8, i8) = (2026, 10, 21, 14, 37);

/// Where the user's time comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WallSource {
    /// The system's clock.
    System,
    /// [`VIRTUAL_START`], in UTC, plus the compositor's virtual time.
    Virtual,
}

/// The user's clock: where its time comes from and the system's time zone
/// it is read in. A new zone is read off the loop (see [`crate::timezone`]),
/// so that the loop never reads the database's files.
#[derive(Debug)]
pub struct WallClock {
    pub source: WallSource,
    /// UTC on the virtual clock, until a stand-in for timedated says
    /// another.
    zone: jiff::tz::TimeZone,
}

impl WallClock {
    pub fn new(source: WallSource) -> Self {
        Self {
            source,
            zone: match source {
                WallSource::System => jiff::tz::TimeZone::system(),
                WallSource::Virtual => jiff::tz::TimeZone::UTC,
            },
        }
    }

    /// The time zone the user reads the time in.
    pub fn zone(&self) -> jiff::tz::TimeZone {
        self.zone.clone()
    }

    /// The system's zone changed.
    pub fn set_zone(&mut self, zone: jiff::tz::TimeZone) {
        self.zone = zone;
    }
}

#[derive(Debug)]
pub struct Shell {
    pub(crate) column: Column,
    /// The output it is on, and that output's logical rectangle.
    on: Option<(String, Rectangle<i32, Logical>, f64)>,
    buffer: Option<MemoryRenderBuffer>,
    popover: Option<(MemoryRenderBuffer, UiPoint)>,
    pointer_inside: bool,
    /// A press began in the shell: its release goes there too.
    pressed: bool,
    /// The column as it was before its theme changed, fading over the new
    /// one (#49): its picture, size, and opacity.
    fading: Option<(MemoryRenderBuffer, (u32, u32), Animated)>,
    /// The desktop shown where it is, when its theme was last given.
    pub(crate) themed_desktop: Option<usize>,
}

/// One of what the column draws: its popover, its old picture fading, or
/// itself.
type ColumnPart<R> = Option<MemoryRenderBufferRenderElement<R>>;

impl Shell {
    /// Whether it has an output to be on.
    pub(crate) fn is_placed(&self) -> bool {
        self.on.is_some()
    }

    pub fn set_theme(&mut self, theme: Theme) {
        self.column.set_theme(theme);
    }

    /// The new theme crossfades from the old one from `now`, as the
    /// desktop it belongs to slides in.
    pub fn crossfade_theme(&mut self, theme: Theme, now: std::time::Duration) {
        if self.buffer.is_some() {
            let view = self.column.view();
            let mut fade = Animated::new(1.0);
            fade.set(0.0, Curve::Ease(Easing::InOutCubic, LARGE), now);
            self.fading = Some((buffer_for(&view), (view.width, view.height), fade));
        }
        self.column.set_theme(theme);
    }

    pub fn set_lang(&mut self, lang: Lang) {
        self.column.set_lang(lang);
    }

    pub fn new(theme: Theme, lang: Lang) -> Self {
        Self {
            column: Column::new(theme, lang, 600.0, 1.0),
            on: None,
            buffer: None,
            popover: None,
            pointer_inside: false,
            pressed: false,
            fading: None,
            themed_desktop: None,
        }
    }
}

/// Creates the shell: the user's language, time zone and name with the
/// system's clock; English, a fixed wall clock and name with a virtual one,
/// for tests.
pub fn start(
    handle: &smithay::reexports::calloop::LoopHandle<'static, crate::App>,
    state: &mut State,
    session_bus: Option<&str>,
    data_dirs: Option<Vec<std::path::PathBuf>>,
) {
    use smithay::reexports::calloop::timer::{TimeoutAction, Timer};
    let lang = state.shell_lang();
    let system = state.wall.source == WallSource::System;
    state.shell = Some(Shell::new(Theme::default(), lang));
    crate::notify::start(handle, state, session_bus, lang);
    crate::tray::start(handle, state, session_bus);
    crate::phone::start(handle, state, session_bus);
    crate::launcher::start(handle, state, lang, data_dirs);
    crate::quick::start(handle, state, lang);
    crate::wallpaper::start(handle, state);
    crate::agents::start(handle, state);
    if system {
        // Wakes the loop when the minute turns; the column reads the time
        // after each dispatch.
        let until_next_minute = || {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            std::time::Duration::from_secs(60 - now.as_secs() % 60)
        };
        let timer = Timer::from_duration(until_next_minute());
        if let Err(err) = handle.insert_source(timer, move |_, _, _| {
            TimeoutAction::ToDuration(until_next_minute())
        }) {
            tracing::warn!(%err.error, "the column's clock will not tick");
        }
    }
}

/// Premultiplied RGBA is ABGR8888 in fourcc terms (little endian).
pub fn buffer_for(view: &View<'_>) -> MemoryRenderBuffer {
    MemoryRenderBuffer::from_slice(
        view.pixels,
        Fourcc::Abgr8888,
        (view.width as i32, view.height as i32),
        1,
        smithay::utils::Transform::Normal,
        None,
    )
}

/// Copies the damaged rectangles of `view` into `buffer`.
pub fn upload(buffer: &mut MemoryRenderBuffer, view: &View<'_>, damage: &[hocus_ui::PixelRect]) {
    let stride = view.width as usize * 4;
    let mut ctx = buffer.render();
    let _ = ctx.draw(|mem| {
        for r in damage {
            for y in r.y as usize..(r.y + r.h) as usize {
                let start = y * stride + r.x as usize * 4;
                let end = start + r.w as usize * 4;
                if let (Some(dst), Some(src)) =
                    (mem.get_mut(start..end), view.pixels.get(start..end))
                {
                    dst.copy_from_slice(src);
                }
            }
        }
        Ok::<_, ()>(
            damage
                .iter()
                .map(|r| {
                    Rectangle::new(
                        (r.x as i32, r.y as i32).into(),
                        (r.w as i32, r.h as i32).into(),
                    )
                })
                .collect(),
        )
    });
}

impl State {
    /// The output the column is on: the primary one.
    pub fn column_output(&self) -> Option<Output> {
        self.shell.as_ref()?;
        let primary = self.model.primary()?;
        self.find_output(&primary.name.0)
    }

    /// Where the column is, in the global logical space.
    pub fn column_rect(&self, output: &Output) -> Option<Rectangle<i32, Logical>> {
        let (name, area, _) = self.shell.as_ref()?.on.as_ref()?;
        if *name != output.name() {
            return None;
        }
        Some(column_rect(*area))
    }

    /// The user's time, as the column shows it.
    pub(crate) fn wall_time(&self) -> WallTime {
        let civil = self.wall_civil();
        WallTime {
            hour: civil.hour() as u8,
            minute: civil.minute() as u8,
            weekday: civil.weekday().to_monday_zero_offset() as u8,
            day: civil.day() as u8,
            month: civil.month() as u8,
            format: self.config.clock.format,
        }
    }

    /// The user's date and time.
    pub(crate) fn wall_civil(&self) -> jiff::civil::DateTime {
        let zone = self.wall.zone();
        match self.wall.source {
            WallSource::System => jiff::Timestamp::now().to_zoned(zone).datetime(),
            WallSource::Virtual => {
                let (y, mo, d, h, mi) = VIRTUAL_START;
                let start = jiff::civil::datetime(y, mo, d, h, mi, 0, 0);
                let elapsed = jiff::SignedDuration::try_from(self.clock.now()).unwrap_or_default();
                let civil = start.checked_add(elapsed).unwrap_or(start);
                civil
                    .to_zoned(jiff::tz::TimeZone::UTC)
                    .map_or(civil, |utc| utc.with_time_zone(zone).datetime())
            }
        }
    }

    /// What the column shows, from the desktop's state.
    fn column_state(&self, output: Option<&Output>) -> ColumnState {
        let focused = self.model.focused();
        let active = focused.and_then(|id| self.windows.get(&id)).map(|m| {
            let (title, app_id) = title_and_app_id(&m.window);
            ActiveWindow {
                title: title
                    .filter(|t| !t.is_empty())
                    .or(app_id)
                    .unwrap_or_default(),
                maximized: matches!(m.mode, WindowMode::Maximized { .. }),
            }
        });
        let first = output.map(Output::name);
        let mut screens: Vec<ScreenDesktops> = self
            .model
            .outputs()
            .iter()
            .map(|o| ScreenDesktops {
                output: o.name.0.clone(),
                count: o.desktops.len(),
                shown: o.desktops.iter().position(|d| *d == o.shown).unwrap_or(0) + 1,
                name: self.desktop_name(o.shown).unwrap_or_default(),
                names: self.desktop_names(&o.name),
                badges: self.desktop_badges(&o.name),
            })
            .collect();
        screens.sort_by_key(|s| Some(&s.output) != first.as_ref());
        let (layout, layout_name) = self.active_layout();
        ColumnState {
            time: self.wall_time(),
            active,
            screens,
            layout,
            layout_name,
            user: self.user.clone(),
            can_hibernate: self.leaver.caps.hibernate,
            notifications: self.notification_history(),
            status: self.status(),
            tray: self.tray.apps.clone(),
            sharing: self.sharing(),
            todos: self.open_todos(),
            updates: self.updates_line(),
            phone: self.phone_line(),
            meeting: self.meeting_card(),
            calendar_set: !self.config.calendars.is_empty(),
            agenda: self.agenda(),
            agents: self.agents_view(),
            journal: self.journal_view(),
        }
    }

    /// The active keyboard layout: its code in short (`FR`), and long.
    fn active_layout(&self) -> (String, String) {
        let index = self.layout_index;
        let layouts = &self.config.keyboard.layouts;
        let code = layouts
            .get(index)
            .or(layouts.first())
            .cloned()
            .unwrap_or_else(|| "us".into());
        let short: String = code.chars().take(4).flat_map(char::to_uppercase).collect();
        (short, code)
    }

    /// After each dispatch: follows the primary output, shows the current
    /// state and draws what changed. Nothing is drawn when nothing changed.
    pub fn refresh_shell(&mut self) {
        let now = self.clock.now();
        if let Some(shell) = self.shell.as_mut()
            && let Some((_, _, fade)) = &shell.fading
        {
            if !fade.is_running(now) {
                shell.fading = None;
            }
            self.needs_redraw = true;
        }
        self.refresh_lock();
        self.refresh_sessions();
        self.refresh_leaving();
        self.refresh_authorize();
        self.refresh_screenshots();
        self.refresh_recording();
        self.refresh_share();
        self.refresh_night();
        self.refresh_updates();
        self.refresh_phone();
        self.refresh_calendar();
        self.refresh_motion();
        self.refresh_slides();
        self.refresh_alt_tab();
        self.refresh_lang();
        self.refresh_first_run();
        self.refresh_identify();
        self.refresh_dock();
        self.refresh_corners();
        self.refresh_ink();
        self.refresh_circle();
        self.refresh_wobbles();
        self.refresh_pandas();
        self.refresh_agents();
        self.refresh_activity();
        self.refresh_clipboard();
        self.refresh_dropdown();
        self.refresh_appearance();
        self.refresh_bars();
        self.refresh_toasts();
        self.refresh_search();
        self.refresh_launcher();
        self.refresh_ai();
        self.refresh_quick();
        if self.shell.is_none() {
            return;
        }
        let output = self.column_output();
        let place = output.as_ref().and_then(|o| {
            let area = self.space.output_geometry(o)?;
            Some((o.name(), area, o.current_scale().fractional_scale()))
        });
        let state = self.column_state(output.as_ref());
        let now = self.clock.now();
        let Some(shell) = self.shell.as_mut() else {
            return;
        };
        let moved = shell.on != place;
        if moved {
            shell.on = place.clone();
            if let Some((_, area, scale)) = &place {
                let rect = column_rect(*area);
                shell.column.resize(rect.size.h as f32, *scale as f32);
            }
            shell.buffer = None;
        }
        shell.column.set_state(&state);
        let damage = shell.column.draw(now);
        let mut changed =
            !damage.column.is_empty() || !damage.popover.is_empty() || damage.popover_changed;
        match &mut shell.buffer {
            Some(buffer) => upload(buffer, &shell.column.view(), &damage.column),
            None if shell.on.is_some() => {
                shell.buffer = Some(buffer_for(&shell.column.view()));
                changed = true;
            }
            None => {}
        }
        match (shell.column.popover_view(), &mut shell.popover) {
            (Some(view), Some((buffer, at))) if !damage.popover_changed => {
                upload(buffer, &view, &damage.popover);
                *at = view.at;
            }
            (Some(view), _) => shell.popover = Some((buffer_for(&view), view.at)),
            (None, _) => shell.popover = None,
        }
        if damage.popover_changed && !shell.column.phone_popover_open() {
            self.phone_popover_closed();
        }
        if moved {
            // The work area of the column's output changed.
            self.relayout();
        }
        if changed || moved {
            self.needs_redraw = true;
        }
    }

    /// The pointer moved to `location`: whether the shell took it.
    pub fn shell_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some((_, area, _)) = &shell.on else {
            return false;
        };
        let origin = column_rect(*area).loc.to_f64();
        let at = UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        );
        if shell.column.hit(at) || shell.pressed {
            shell.pointer_inside = true;
            let _ = shell.column.input(at, Input::PointerMove(at));
            true
        } else {
            if shell.pointer_inside {
                shell.pointer_inside = false;
                shell.column.pointer_left();
            }
            false
        }
    }

    /// The primary button: whether the shell took it.
    pub fn shell_pointer_button(&mut self, location: Point<f64, Logical>, pressed: bool) -> bool {
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some((_, area, _)) = &shell.on else {
            return false;
        };
        let origin = column_rect(*area).loc.to_f64();
        let at = UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        );
        let mine = if pressed {
            shell.column.hit(at)
        } else {
            std::mem::take(&mut shell.pressed)
        };
        if !mine {
            if pressed {
                shell.column.clicked_elsewhere();
            }
            return false;
        }
        if pressed {
            shell.pressed = true;
        }
        let actions = shell.column.input(at, Input::PointerButton { pressed });
        for action in actions {
            self.shell_action(action);
        }
        true
    }

    /// The secondary button pressed: over a tray application, its menu.
    pub fn shell_secondary_click(&mut self, location: Point<f64, Logical>) -> bool {
        let Some(shell) = self.shell.as_mut() else {
            return false;
        };
        let Some((_, area, _)) = &shell.on else {
            return false;
        };
        let origin = column_rect(*area).loc.to_f64();
        let at = UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        );
        if !shell.column.hit(at) {
            return false;
        }
        for action in shell.column.secondary_click(at) {
            self.shell_action(action);
        }
        true
    }

    fn shell_action(&mut self, action: ShellAction) {
        use hocus_core::shortcuts::Action as A;
        let serial = SERIAL_COUNTER.next_serial();
        match action {
            ShellAction::Minimize => self.run_action(&A::Minimize, serial),
            ShellAction::ToggleMaximize => self.run_action(&A::ToggleMaximized, serial),
            ShellAction::Close => self.run_action(&A::Close, serial),
            ShellAction::SwitchLayout => self.run_action(&A::SwitchLayout, serial),
            ShellAction::Lock => self.run_action(&A::Lock, serial),
            ShellAction::Settings => self.run_action(&A::Settings, serial),
            ShellAction::Suspend => self.run_action(&A::Suspend, serial),
            ShellAction::Hibernate => self.run_action(&A::Hibernate, serial),
            ShellAction::LogOut => self.run_action(&A::LogOut, serial),
            ShellAction::Restart => self.run_action(&A::Restart, serial),
            ShellAction::ShutDown => self.run_action(&A::ShutDown, serial),
            ShellAction::OpenLauncher => self.open_launcher(),
            ShellAction::StopSharing => self.stop_sharing(),
            ShellAction::TickTodo(index) => self.tick_note(index),
            ShellAction::InstallUpdates => self.install_updates(),
            ShellAction::RingPhone => self.ring_phone(),
            ShellAction::SendClipboardToPhone => self.send_clipboard_to_phone(),
            ShellAction::BrowsePhone => self.browse_phone(),
            ShellAction::PhoneOpened => self.phone_opened(),
            ShellAction::PairPhone => self.pair_phone(),
            ShellAction::CancelPairing => self.cancel_pairing(),
            ShellAction::MirrorPhone => self.mirror_phone(),
            ShellAction::JoinMeeting => self.join_meeting(),
            ShellAction::TrayActivate(key) => self.tray_activate(key),
            ShellAction::TrayMenu(key) => self.tray_menu(key),
            ShellAction::TrayChosen { key, id } => self.tray_chosen(key, id),
            ShellAction::Agent { key, ask } => self.agent_ask(&key, ask),
            ShellAction::StopAgents => self.set_agents_stopped(true),
            ShellAction::AllowAgents => self.set_agents_stopped(false),
            ShellAction::ShowDesktop { output, desktop } => {
                if let Some(output) = self.find_output(&output)
                    && let Err(err) = self.show_desktop(&output, desktop.saturating_sub(1), serial)
                {
                    tracing::warn!(%err, "could not show a desktop from the column");
                }
            }
        }
    }

    /// The column's popover and the column on `output`. None when a
    /// fullscreen window covers that output.
    pub fn shell_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> (ColumnPart<R>, ColumnPart<R>, ColumnPart<R>)
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(shell) = &self.shell else {
            return (None, None, None);
        };
        let Some((name, area, scale)) = &shell.on else {
            return (None, None, None);
        };
        if *name != output.name() || self.fullscreen_on(output) {
            return (None, None, None);
        }
        let rect = column_rect(*area);
        let local = Point::<f64, Logical>::from((
            f64::from(rect.loc.x - area.loc.x),
            f64::from(rect.loc.y - area.loc.y),
        ));
        let now = self.clock.now();
        let mut element = |buffer: &MemoryRenderBuffer,
                           at: Point<f64, Logical>,
                           view_size: (u32, u32),
                           alpha: Option<f32>| {
            let (w, h) = view_size;
            let all = Rectangle::from_size(Size::from((f64::from(w), f64::from(h))));
            let size = Size::<f64, Logical>::from((f64::from(w) / scale, f64::from(h) / scale))
                .to_i32_round();
            let location: Point<f64, Physical> = at.to_physical(*scale);
            MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                location,
                buffer,
                alpha,
                Some(all),
                Some(size),
                Kind::Unspecified,
            )
            .inspect_err(|err| tracing::warn!(?err, "could not draw the column"))
            .ok()
        };
        let popover = match (&shell.popover, shell.column.popover_view()) {
            (Some((buffer, at)), Some(view)) => {
                let at = local + Point::from((f64::from(at.x), f64::from(at.y)));
                element(buffer, at, (view.width, view.height), None)
            }
            _ => None,
        };
        // The old theme's column over the new one, fading (#49).
        let fading = shell.fading.as_ref().and_then(|(buffer, size, fade)| {
            element(buffer, local, *size, Some(fade.value(now).clamp(0.0, 1.0)))
        });
        // Coming or going in the Matrix theme, it is drawn in rain.
        let column = shell
            .buffer
            .as_ref()
            .filter(|_| self.matrix_column_moment().is_none())
            .and_then(|buffer| {
                let view = shell.column.view();
                element(buffer, local, (view.width, view.height), None)
            });
        (popover, fading, column)
    }

    /// The column coming or going in the Matrix theme's rain on `output`
    /// (#155), even under a fullscreen window it goes for. Front first.
    pub fn matrix_column_on<R>(&self, renderer: &mut R, output: &Output) -> Vec<SceneElement<R>>
    where
        R: Renderer + ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(shell) = &self.shell else {
            return Vec::new();
        };
        let (Some((name, area, scale)), Some(buffer)) = (&shell.on, &shell.buffer) else {
            return Vec::new();
        };
        if *name != output.name() || self.matrix_column_moment().is_none() {
            return Vec::new();
        }
        let rect = column_rect(*area);
        let view = shell.column.view();
        let column = crate::matrix_shell::rect(
            f64::from(rect.loc.x - area.loc.x),
            f64::from(rect.loc.y - area.loc.y),
            f64::from(view.width) / scale,
            f64::from(view.height) / scale,
        );
        self.matrix_column_elements(renderer, buffer, column, f64::from(area.size.w), *scale)
    }

    /// The column's panels on glass, and its popover's, on `output`.
    pub(crate) fn shell_glass(&self, output: &Output) -> (Vec<Spot>, Vec<Spot>) {
        let Some((shell, Some((name, area, _)))) = self.shell.as_ref().map(|s| (s, &s.on)) else {
            return (Vec::new(), Vec::new());
        };
        if *name != output.name() {
            return (Vec::new(), Vec::new());
        }
        let rect = column_rect(*area);
        let local = Point::<f64, Logical>::from((
            f64::from(rect.loc.x - area.loc.x),
            f64::from(rect.loc.y - area.loc.y),
        ));
        (
            spots(Surface::Popover, local, &shell.column.popover_glass()),
            spots(Surface::Column, local, &shell.column.glass()),
        )
    }

    /// Whether a fullscreen window is shown on `output`.
    pub(crate) fn fullscreen_on(&self, output: &Output) -> bool {
        let Some(shown) = self
            .model
            .output(&hocus_core::OutputName(output.name()))
            .map(|o| o.shown)
        else {
            return false;
        };
        self.windows.iter().any(|(id, m)| {
            matches!(m.mode, WindowMode::Fullscreen { .. })
                && self.model.is_visible(*id)
                && self.model.desktop_of(*id) == Some(shown)
        })
    }

    /// The shell's interface for the control API, in global coordinates.
    pub fn shell_ui(&self) -> Vec<UiNode> {
        let mut out = Vec::new();
        self.lock_ui(&mut out);
        // Locked, nothing else is there.
        if self.locked() {
            return out;
        }
        self.boot_ui(&mut out);
        self.card_ui(&mut out);
        self.identify_ui(&mut out);
        self.leave_ui(&mut out);
        self.auth_ui(&mut out);
        self.circle_ui(&mut out);
        self.picker_ui(&mut out);
        self.alt_tab_ui(&mut out);
        self.switcher_ui(&mut out);
        self.dock_ui(&mut out);
        self.pandas_ui(&mut out);
        self.recording_ui(&mut out);
        self.share_ui(&mut out);
        self.bars_ui(&mut out);
        let Some(shell) = &self.shell else {
            return out;
        };
        let Some((_, area, _)) = &shell.on else {
            return out;
        };
        let origin = column_rect(*area).loc;
        flatten(&shell.column.access(), origin, 0, &mut out);
        self.toasts_ui(&mut out);
        self.launcher_ui(&mut out);
        self.quick_ui(&mut out);
        out
    }
}

pub fn flatten(node: &AccessNode, origin: Point<i32, Logical>, depth: u32, out: &mut Vec<UiNode>) {
    let r = node.rect;
    out.push(UiNode {
        role: node.role.name().into(),
        label: node.label.clone(),
        rect: ApiRect {
            x: origin.x + r.x.round() as i32,
            y: origin.y + r.y.round() as i32,
            width: r.w.round() as i32,
            height: r.h.round() as i32,
        },
        depth,
        focused: node.focused,
        disabled: node.disabled,
        selected: node.selected,
    });
    for child in &node.children {
        flatten(child, origin, depth + 1, out);
    }
}

/// The column on an output of logical rectangle `area`.
pub fn column_rect(area: Rectangle<i32, Logical>) -> Rectangle<i32, Logical> {
    let (w, m) = (WIDTH as i32, MARGIN as i32);
    Rectangle::new(
        (area.loc.x + area.size.w - m - w, area.loc.y + m).into(),
        (w, (area.size.h - 2 * m).max(1)).into(),
    )
}

/// What a panel shows: a view of hocus-shell with its pixels.
pub trait Pixels {
    fn pixels(&self) -> View<'_>;
}

impl Pixels for hocus_shell::lock::Curtain {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::picker::Picker {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::authorize::Authorize {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::leave::Leave {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::readout::Readout {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::identify::Identify {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::boot::Boot {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::welcome::Welcome {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

impl Pixels for hocus_shell::lock::LockScreen {
    fn pixels(&self) -> View<'_> {
        self.view()
    }
}

/// A view of the shell drawn over a whole output, and its texture.
#[derive(Debug)]
pub struct Panel<V> {
    pub view: V,
    buffer: Option<MemoryRenderBuffer>,
    size: (u32, u32),
}

impl<V: Pixels> Panel<V> {
    pub fn new(view: V) -> Self {
        Self {
            view,
            buffer: None,
            size: (0, 0),
        }
    }

    /// Uploads what the view drew; whether the texture changed.
    pub fn upload(&mut self, drawn: &[hocus_ui::PixelRect]) -> bool {
        let view = self.view.pixels();
        let size = (view.width, view.height);
        match &mut self.buffer {
            Some(buffer) if self.size == size => {
                if drawn.is_empty() {
                    return false;
                }
                upload(buffer, &view, drawn);
            }
            _ => {
                self.buffer = Some(buffer_for(&view));
                self.size = size;
            }
        }
        true
    }

    /// Its element at `at` on `output`, logical pixels.
    pub fn element<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        at: Point<i32, Logical>,
    ) -> Option<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        self.element_with(renderer, output, at, None)
    }

    /// The same, see-through as `alpha` says.
    pub fn element_with<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        at: Point<i32, Logical>,
        alpha: Option<f32>,
    ) -> Option<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let buffer = self.buffer.as_ref()?;
        let scale = output.current_scale().fractional_scale();
        let (w, h) = self.size;
        let all = Rectangle::from_size(Size::from((f64::from(w), f64::from(h))));
        let size =
            Size::<f64, Logical>::from((f64::from(w) / scale, f64::from(h) / scale)).to_i32_round();
        let location = at.to_f64().to_physical(scale);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            location,
            buffer,
            alpha,
            Some(all),
            Some(size),
            Kind::Unspecified,
        )
        .inspect_err(|err| tracing::warn!(?err, "could not draw a panel"))
        .ok()
    }

    /// Only `part` of it (logical, from its corner), drawn at `at`.
    pub fn element_part<R>(
        &self,
        renderer: &mut R,
        output: &Output,
        at: Point<i32, Logical>,
        part: Rectangle<i32, Logical>,
        alpha: Option<f32>,
    ) -> Option<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let buffer = self.buffer.as_ref()?;
        let scale = output.current_scale().fractional_scale();
        let src = Rectangle::new(
            (f64::from(part.loc.x) * scale, f64::from(part.loc.y) * scale).into(),
            (
                f64::from(part.size.w) * scale,
                f64::from(part.size.h) * scale,
            )
                .into(),
        );
        let location = at.to_f64().to_physical(scale);
        MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            location,
            buffer,
            alpha,
            Some(src),
            Some(part.size),
            Kind::Unspecified,
        )
        .inspect_err(|err| tracing::warn!(?err, "could not draw part of a panel"))
        .ok()
    }
}
