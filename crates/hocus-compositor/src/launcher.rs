//! The launcher in the compositor: shown over everything on the screen
//! being worked on, it takes the keyboard and the pointer while open.
//! Applications are read from desktop entries on a thread when it opens,
//! and started in a clean environment; results are copied to the
//! clipboard, for Wayland and X11 clients alike.

use std::{os::fd::OwnedFd, path::PathBuf, sync::Arc};

use hocus_api::UiNode;
use hocus_platform::apps::{self, Entry};
use hocus_shell::{
    App as ShellApp, Choice, Lang, Launcher, LauncherEvent, Showcase,
    launcher::{ROOM, WIDTH},
    providers::Enabled,
    showcase::Showcased,
};
use hocus_ui::{Input, Key, Point as UiPoint, Size as UiSize, Theme};
use smithay::{
    backend::renderer::{
        ImportMem, Renderer,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
        },
    },
    input::keyboard::{Keysym, ModifiersState},
    output::Output,
    reexports::calloop::{LoopHandle, channel},
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Size},
    wayland::selection::data_device::set_data_device_selection,
};

use crate::{App, State};

/// What the clipboard the compositor offers holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// An X11 client's selection, read through XWayland.
    X11,
    /// Text the shell put there (a calculator result).
    Text(Arc<str>),
    /// A picture the shell put there (a screenshot), as PNG.
    Png(Arc<[u8]>),
}

/// The text formats the shell offers.
const TEXT: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];

#[derive(Debug)]
pub struct LauncherPanel {
    pub(crate) launcher: Launcher,
    on: Option<(String, Rectangle<i32, Logical>, f64)>,
    buffer: Option<MemoryRenderBuffer>,
    /// The buffer's size in pixels: the card grows with an answer.
    buffer_size: (u32, u32),
    /// Where desktop entries are read from.
    dirs: Vec<PathBuf>,
    apps: Option<channel::Sender<Vec<Entry>>>,
    pressed: bool,
    lang: Lang,
    /// Large type or a QR code over a screen.
    showcase: Option<Shown>,
    /// The configuration generation the providers follow.
    providers_from: u64,
    /// Actions run from the palette, newest first.
    action_recents: Vec<hocus_core::Action>,
    /// Where its parts were last drawn while open: the Matrix theme rains
    /// them away once it closes (#155).
    drawn_parts: Option<hocus_shell::launcher::Parts>,
}

/// Something shown over a whole screen.
#[derive(Debug)]
struct Shown {
    showcased: Showcased,
    on: (String, Rectangle<i32, Logical>, f64),
    buffer: Option<MemoryRenderBuffer>,
}

impl LauncherPanel {
    pub fn set_theme(&mut self, theme: Theme) {
        self.launcher.set_theme(theme);
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if lang != self.lang {
            self.lang = lang;
            self.launcher.set_lang(lang);
        }
    }
}

pub fn start(
    handle: &LoopHandle<'static, App>,
    state: &mut State,
    lang: Lang,
    dirs: Option<Vec<PathBuf>>,
) {
    let (sender, receiver) = channel::channel::<Vec<Entry>>();
    let inserted = handle.insert_source(receiver, |event, _, app| {
        if let channel::Event::Msg(entries) = event {
            app.timed("applications", |app| app.state.apps_found(entries));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "the launcher will find no applications");
    }
    // Read once at start, for the dock; again each time the launcher
    // opens.
    let first_dirs = dirs.clone().unwrap_or_else(apps::data_dirs);
    let first_lang = std::env::var("LANG").ok();
    let scan = std::thread::Builder::new()
        .name("applications".into())
        .spawn({
            let sender = sender.clone();
            move || {
                let _ = sender.send(apps::scan(&first_dirs, first_lang.as_deref()));
            }
        });
    if let Err(err) = scan {
        tracing::warn!(%err, "could not read the applications");
    }
    state.launcher = Some(LauncherPanel {
        launcher: Launcher::new(Theme::default(), lang, 1.0),
        on: None,
        buffer: None,
        buffer_size: (0, 0),
        dirs: dirs.unwrap_or_else(apps::data_dirs),
        apps: Some(sender),
        pressed: false,
        lang,
        showcase: None,
        providers_from: 0,
        action_recents: Vec::new(),
        drawn_parts: None,
    });
}

fn shell_app(e: Entry) -> ShellApp {
    ShellApp {
        id: e.id,
        name: e.name,
        generic: e.generic,
        keywords: e.keywords,
        exec: e.exec,
    }
}

/// A key while the launcher is open: what the toolkit understands.
pub(crate) fn key_of(sym: Keysym) -> Option<Key> {
    Some(match sym {
        Keysym::Escape => Key::Escape,
        Keysym::Return | Keysym::KP_Enter => Key::Enter,
        Keysym::BackSpace => Key::Backspace,
        Keysym::Delete | Keysym::KP_Delete => Key::Delete,
        Keysym::Left | Keysym::KP_Left => Key::Left,
        Keysym::Right | Keysym::KP_Right => Key::Right,
        Keysym::Up | Keysym::KP_Up => Key::Up,
        Keysym::Down | Keysym::KP_Down => Key::Down,
        Keysym::Home | Keysym::KP_Home => Key::Home,
        Keysym::End | Keysym::KP_End => Key::End,
        Keysym::Tab => Key::Tab,
        Keysym::ISO_Left_Tab => Key::BackTab,
        _ => return None,
    })
}

impl LauncherPanel {
    pub fn app_for(&self, app_id: &str) -> Option<&hocus_shell::App> {
        self.launcher.app_for(app_id)
    }
}

impl State {
    pub fn launcher_open(&self) -> bool {
        self.launcher.as_ref().is_some_and(|l| l.launcher.is_open())
    }

    pub fn palette_open(&self) -> bool {
        self.launcher.as_ref().is_some_and(|l| {
            l.launcher.is_open() && l.launcher.mode() == hocus_shell::launcher::Mode::Actions
        })
    }

    /// The action palette, with the bindings in effect.
    pub fn toggle_palette(&mut self) {
        let shortcuts: Vec<_> = self
            .shortcuts
            .bindings
            .iter()
            .map(|(c, a)| (c.clone(), a.clone()))
            .collect();
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_shortcuts(shortcuts);
            panel
                .launcher
                .set_action_recents(panel.action_recents.clone());
        }
        self.toggle_launcher_mode(hocus_shell::launcher::Mode::Actions);
    }

    pub fn palette_keys(&mut self, combo: &hocus_core::shortcuts::Combo) {
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.press_keys(combo);
        }
        self.needs_redraw = true;
    }

    pub fn toggle_launcher(&mut self) {
        self.toggle_launcher_mode(hocus_shell::launcher::Mode::Commands);
    }

    /// Opens the card on commands, the clipboard history or emoji; closes
    /// it when it already shows that.
    pub fn toggle_launcher_mode(&mut self, mode: hocus_shell::launcher::Mode) {
        let showing = self
            .launcher
            .as_ref()
            .is_some_and(|l| l.launcher.is_open() && l.launcher.mode() == mode);
        if showing {
            self.close_launcher();
        } else {
            self.open_launcher();
            if let Some(panel) = self.launcher.as_mut() {
                panel.launcher.open_mode(mode);
            }
        }
    }

    /// Opens on the screen under the pointer; reads the applications again
    /// meanwhile, off the loop.
    pub fn open_launcher(&mut self) {
        if self.quick_open() {
            self.close_quick();
        }
        let output = self.output_under_pointer();
        let place = output.as_ref().and_then(|o| {
            // Centred in the work area: beside the column, not over it.
            self.space.output_geometry(o)?;
            Some((
                o.name(),
                self.work_area(o),
                o.current_scale().fractional_scale(),
            ))
        });
        let Some(panel) = self.launcher.as_mut() else {
            return;
        };
        if let Some((_, _, scale)) = &place {
            panel.launcher.set_scale(*scale as f32);
        }
        if panel.on != place {
            panel.buffer = None;
        }
        panel.on = place;
        let was_open = panel.launcher.is_open();
        panel.launcher.open();
        if let Some(sender) = panel.apps.clone() {
            let dirs = panel.dirs.clone();
            let lang = std::env::var("LANG").ok();
            let scan = std::thread::Builder::new()
                .name("applications".into())
                .spawn(move || {
                    let _ = sender.send(apps::scan(&dirs, lang.as_deref()));
                });
            if let Err(err) = scan {
                tracing::warn!(%err, "could not read the applications");
            }
        }
        // Clients let go of the keyboard while it is open: no key stays
        // down for them.
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        // The Matrix theme jacks in (#155).
        if !was_open {
            self.matrix_launcher(crate::matrix_shell::Moment::Open, None);
        }
        self.needs_redraw = true;
    }

    pub fn close_launcher(&mut self) {
        let mut closed = None;
        if let Some(panel) = self.launcher.as_mut() {
            // It may have closed itself (Escape) before saying so.
            closed = panel.drawn_parts.take();
            panel.launcher.close();
        }
        // Its text rains away in the Matrix theme (#155).
        if let Some(parts) = closed {
            self.matrix_launcher(crate::matrix_shell::Moment::Close, Some(parts));
        }
        self.apply_focus(SERIAL_COUNTER.next_serial());
        self.needs_redraw = true;
    }

    fn apps_found(&mut self, entries: Vec<Entry>) {
        // The dock names and launches the same apps.
        self.dock.entries = entries.clone();
        self.dock.entries_generation += 1;
        if let Some(panel) = self.launcher.as_mut() {
            panel
                .launcher
                .set_apps(entries.into_iter().map(shell_app).collect());
        }
        self.needs_redraw = true;
    }

    /// A key pressed while open; whether the launcher took it (always).
    pub fn launcher_key(&mut self, sym: Keysym, modifiers: &ModifiersState) -> bool {
        let Some(panel) = self.launcher.as_mut() else {
            return false;
        };
        if !panel.launcher.is_open() {
            return false;
        }
        // A long answer scrolls a page at a time.
        if matches!(
            sym,
            Keysym::Page_Up | Keysym::Page_Down | Keysym::KP_Page_Up | Keysym::KP_Page_Down
        ) {
            let down = matches!(sym, Keysym::Page_Down | Keysym::KP_Page_Down);
            panel.launcher.page(down);
            self.needs_redraw = true;
            return true;
        }
        let events = match key_of(sym) {
            Some(key) => panel.launcher.key(key),
            None => {
                if !modifiers.ctrl
                    && !modifiers.alt
                    && !modifiers.logo
                    && let Some(c) = sym.key_char().filter(|c| !c.is_control())
                {
                    panel.launcher.text(c.encode_utf8(&mut [0; 4]));
                }
                Vec::new()
            }
        };
        self.launcher_events(events);
        self.needs_redraw = true;
        true
    }

    fn launcher_events(&mut self, events: Vec<LauncherEvent>) {
        for event in events {
            match event {
                LauncherEvent::Closed => self.close_launcher(),
                LauncherEvent::Chosen(Choice::Launch(app)) => {
                    self.close_launcher();
                    tracing::info!(app = app.id, "launching");
                    if let Some((program, args)) = app.exec.split_first() {
                        self.launch(program, args);
                    }
                }
                LauncherEvent::Chosen(Choice::Copy(text)) => {
                    self.close_launcher();
                    self.copy_text(&text);
                }
                LauncherEvent::Chosen(Choice::Open(url)) => {
                    self.close_launcher();
                    tracing::info!(url, "opening");
                    self.launch("xdg-open", &[url]);
                }
                LauncherEvent::Chosen(Choice::OpenFile(path)) => {
                    self.close_launcher();
                    // Not the path at info: a document's name says enough.
                    tracing::debug!(path, "opening a document");
                    self.launch("xdg-open", &[path]);
                }
                LauncherEvent::Chosen(Choice::Settings) => {
                    self.close_launcher();
                    self.open_settings(None);
                }
                LauncherEvent::Chosen(Choice::AddNote(text)) => self.add_note(&text),
                LauncherEvent::Chosen(Choice::ToggleNote(index)) => self.tick_note(index),
                LauncherEvent::RemoveNote(index) => self.remove_note(index),
                LauncherEvent::Chosen(Choice::OpenNotes) => {
                    self.close_launcher();
                    self.toggle_notes();
                }
                LauncherEvent::Chosen(Choice::SetShortcut) => {
                    self.close_launcher();
                    self.open_settings(Some("keyboard".into()));
                }
                LauncherEvent::Chosen(Choice::Run(action)) => {
                    self.close_launcher();
                    if let Some(panel) = self.launcher.as_mut() {
                        let recents = &mut panel.action_recents;
                        recents.retain(|a| *a != action);
                        recents.insert(0, action.clone());
                        recents.truncate(5);
                    }
                    tracing::info!(%action, "from the palette");
                    // On what had the focus before the palette.
                    self.run_action(&action, smithay::utils::SERIAL_COUNTER.next_serial());
                }
                LauncherEvent::Pin(text) => self.history_pin(&text),
                LauncherEvent::Forget(text) => self.history_forget(&text),
                LauncherEvent::Chosen(Choice::Paste(text)) => {
                    self.close_launcher();
                    self.insert_text(&text, true);
                }
                LauncherEvent::Chosen(Choice::Insert(glyph)) => {
                    self.close_launcher();
                    self.emoji_chosen(&glyph);
                }
                LauncherEvent::Chosen(Choice::ClearHistory) => {
                    self.close_launcher();
                    self.history_clear();
                }
                LauncherEvent::Chosen(Choice::Session(what)) => {
                    use hocus_core::Action as A;
                    use hocus_shell::launcher::SessionAction as S;
                    self.close_launcher();
                    let action = match what {
                        S::Lock => A::Lock,
                        S::Suspend => A::Suspend,
                        S::LogOut => A::LogOut,
                        S::Restart => A::Restart,
                        S::ShutDown => A::ShutDown,
                    };
                    self.run_action(&action, smithay::utils::SERIAL_COUNTER.next_serial());
                }
                LauncherEvent::Chosen(Choice::Show(what)) => {
                    self.close_launcher();
                    self.show(what);
                }
                LauncherEvent::Chosen(Choice::Intent(intent)) => {
                    self.close_launcher();
                    self.run_intent(intent);
                }
                // Asked and answered in the field itself.
                LauncherEvent::Chosen(Choice::Ask(_) | Choice::Answer) => {}
                LauncherEvent::Ask { id, question } => self.ask_ai(id, question),
                LauncherEvent::Act(run) => {
                    self.close_launcher();
                    self.run_proposal(run);
                }
                LauncherEvent::NoteAnswer(text) => {
                    self.close_launcher();
                    // A line of the notes.
                    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    self.add_note(&line);
                    let lang = self.shell_lang();
                    self.shell_toast(lang.added_to_notes(), "");
                }
            }
        }
    }

    /// Runs a request the user confirmed (#44).
    pub(crate) fn run_intent(&mut self, intent: hocus_shell::intents::Intent) {
        use hocus_shell::intents::Intent;
        tracing::info!(?intent, "confirmed");
        match intent {
            Intent::Run(action) => {
                self.run_action(&action, SERIAL_COUNTER.next_serial());
            }
            Intent::CaptureApp {
                matches,
                name,
                delay,
            } => self.screenshot_app(matches, name, delay),
            Intent::Launch(app) => {
                if let Some((program, args)) = app.exec.split_first() {
                    self.launch(program, args);
                }
            }
            Intent::Settings(page) => self.open_settings(Some(page)),
            Intent::Note(text) => {
                self.add_note(&text);
                let lang = self.shell_lang();
                self.shell_toast(lang.added_to_notes(), &text);
            }
        }
    }

    /// Puts a PNG on the clipboard, for Wayland and X11 clients.
    pub fn copy_png(&mut self, png: Arc<[u8]>) {
        let mimes = vec!["image/png".to_owned()];
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            mimes.clone(),
            Offer::Png(png),
        );
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(
                smithay::wayland::selection::SelectionTarget::Clipboard,
                Some(mimes),
            )
        {
            tracing::warn!(?err, "could not offer the picture to X11");
        }
    }

    /// Puts text on the clipboard, for Wayland and X11 clients.
    pub fn copy_text(&mut self, text: &str) {
        self.shell_copied(text);
        let mimes: Vec<String> = TEXT.iter().map(|m| (*m).to_owned()).collect();
        set_data_device_selection(
            &self.display_handle,
            &self.seat,
            mimes.clone(),
            Offer::Text(Arc::from(text)),
        );
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(
                smithay::wayland::selection::SelectionTarget::Clipboard,
                Some(mimes),
            )
        {
            tracing::debug!(?err, "X11 clients will not see the copied text");
        }
    }

    fn launcher_origin(&self) -> Option<Point<f64, Logical>> {
        let panel = self.launcher.as_ref()?;
        let (_, area, _) = panel.on.as_ref()?;
        let x =
            f64::from(area.loc.x) + (f64::from(area.size.w) - f64::from(WIDTH + 2.0 * ROOM)) / 2.0;
        let y = f64::from(area.loc.y) + f64::from(area.size.h) * 0.12 - f64::from(ROOM);
        Some(Point::from((x, y)))
    }

    /// The launcher's card on glass, output-local (#37); the showcase has
    /// none.
    pub(crate) fn launcher_glass(&self, output: &Output) -> Vec<crate::blur::Spot> {
        let (Some(panel), Some(origin)) = (&self.launcher, self.launcher_origin()) else {
            return Vec::new();
        };
        let (Some(area), None) = (self.space.output_geometry(output), &panel.showcase) else {
            return Vec::new();
        };
        let origin = origin - area.loc.to_f64();
        crate::blur::spots(
            crate::blur::Surface::Launcher,
            origin,
            &panel.launcher.glass(),
        )
    }

    fn launcher_local(&self, location: Point<f64, Logical>) -> Option<UiPoint> {
        let origin = self.launcher_origin()?;
        Some(UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        ))
    }

    /// The wheel over the open launcher's card scrolls a long answer;
    /// whether it did.
    pub fn launcher_wheel(&mut self, dy: f64) -> bool {
        if !self.launcher_open() || dy == 0.0 {
            return false;
        }
        let Some(location) = self.seat.get_pointer().map(|p| p.current_location()) else {
            return false;
        };
        let Some(at) = self.launcher_local(location) else {
            return false;
        };
        let scrolled = self
            .launcher
            .as_mut()
            .is_some_and(|panel| panel.launcher.wheel(at, dy as f32));
        if scrolled {
            self.needs_redraw = true;
        }
        scrolled
    }

    /// While open, the launcher takes the pointer everywhere.
    pub fn launcher_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        if !self.launcher_open() {
            return false;
        }
        let Some(at) = self.launcher_local(location) else {
            return false;
        };
        if let Some(panel) = self.launcher.as_mut() {
            let events = panel.launcher.input(at, Input::PointerMove(at));
            self.launcher_events(events);
        }
        true
    }

    pub fn launcher_pointer_button(
        &mut self,
        location: Point<f64, Logical>,
        pressed: bool,
    ) -> bool {
        let open = self.launcher_open();
        let Some(panel) = self.launcher.as_mut() else {
            return false;
        };
        // The release of a press that closed it goes nowhere else either.
        if !open && !(panel.pressed && !pressed) {
            return false;
        }
        panel.pressed = pressed;
        let Some(at) = self.launcher_local(location) else {
            return true;
        };
        if let Some(panel) = self.launcher.as_mut() {
            let events = panel.launcher.input(at, Input::PointerButton { pressed });
            self.launcher_events(events);
        }
        true
    }

    /// Shows large type or a QR code over the screen under the pointer.
    fn show(&mut self, what: Showcase) {
        let Some(output) = self.output_under_pointer() else {
            return;
        };
        let Some(area) = self.space.output_geometry(&output) else {
            return;
        };
        let scale = output.current_scale().fractional_scale();
        let theme = self.theme();
        let Some(panel) = self.launcher.as_mut() else {
            return;
        };
        let size = UiSize::new(area.size.w as f32, area.size.h as f32);
        panel.showcase = Some(Shown {
            showcased: Showcased::new(what, theme, panel.lang, size, scale as f32),
            on: (output.name(), area, scale),
            buffer: None,
        });
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        self.needs_redraw = true;
    }

    pub fn showcase_open(&self) -> bool {
        self.launcher.as_ref().is_some_and(|l| l.showcase.is_some())
    }

    pub fn close_showcase(&mut self) {
        if let Some(panel) = self.launcher.as_mut()
            && panel.showcase.take().is_some()
        {
            self.apply_focus(SERIAL_COUNTER.next_serial());
            self.needs_redraw = true;
        }
    }

    /// While something is shown, a key or a click closes it.
    pub fn showcase_key(&mut self, sym: Keysym) -> bool {
        if !self.showcase_open() {
            return false;
        }
        if matches!(
            sym,
            Keysym::Escape | Keysym::Return | Keysym::KP_Enter | Keysym::space
        ) {
            self.close_showcase();
        }
        true
    }

    pub fn showcase_pointer(&mut self, pressed: Option<bool>) -> bool {
        if !self.showcase_open() {
            return false;
        }
        if pressed == Some(true) {
            self.close_showcase();
            if let Some(panel) = self.launcher.as_mut() {
                // Its release goes nowhere else.
                panel.pressed = true;
            }
        }
        true
    }

    /// Draws what changed; called after each dispatch.
    pub fn refresh_launcher(&mut self) {
        let now = self.clock.now();
        let generation = self.config_generation;
        let enabled = Enabled {
            calculator: self.config.launcher.calculator,
            units: self.config.launcher.units,
            urls: self.config.launcher.urls,
            web_search: self.config.launcher.web_search,
            search_engine: self.config.launcher.search_engine.clone(),
            large_type: self.config.launcher.large_type,
            qr: self.config.launcher.qr,
            intents: self.config.launcher.intents,
            ask: self.ai_source(),
            documents: self.config.search.documents && self.search_running(),
        };
        let Some(panel) = self.launcher.as_mut() else {
            return;
        };
        if panel.providers_from != generation {
            panel.launcher.set_providers(&enabled);
            panel.providers_from = generation;
        }
        if let Some(shown) = panel.showcase.as_mut() {
            let damage = shown.showcased.draw(now);
            let view = shown.showcased.view();
            match &mut shown.buffer {
                Some(buffer) => crate::shell::upload(buffer, &view, &damage),
                None => shown.buffer = Some(crate::shell::buffer_for(&view)),
            }
            if !damage.is_empty() {
                self.needs_redraw = true;
            }
            return;
        }
        if !panel.launcher.is_open() {
            panel.drawn_parts = None;
            return;
        }
        let damage = panel.launcher.draw(now);
        panel.drawn_parts = panel.launcher.parts();
        let view = panel.launcher.view();
        let mut changed = !damage.is_empty();
        let size = (view.width, view.height);
        match &mut panel.buffer {
            Some(buffer) if panel.buffer_size == size => {
                crate::shell::upload(buffer, &view, &damage)
            }
            _ => {
                panel.buffer = Some(crate::shell::buffer_for(&view));
                panel.buffer_size = size;
                changed = true;
            }
        }
        if changed {
            self.needs_redraw = true;
        }
    }

    /// The launcher on `output`, above everything; what it shows over a
    /// screen, above that.
    pub fn launcher_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(panel) = &self.launcher else {
            return Vec::new();
        };
        if let Some(shown) = &panel.showcase {
            let (name, _, scale) = &shown.on;
            let (Some(buffer), true) = (&shown.buffer, *name == output.name()) else {
                return Vec::new();
            };
            let view = shown.showcased.view();
            let all =
                Rectangle::from_size(Size::from((f64::from(view.width), f64::from(view.height))));
            let size = Size::<f64, Logical>::from((
                f64::from(view.width) / scale,
                f64::from(view.height) / scale,
            ))
            .to_i32_round();
            return MemoryRenderBufferRenderElement::from_buffer(
                renderer,
                Point::<f64, Physical>::from((0.0, 0.0)),
                buffer,
                None,
                Some(all),
                Some(size),
                Kind::Unspecified,
            )
            .inspect_err(|err| tracing::warn!(?err, "could not draw the showcase"))
            .map(|e| vec![e])
            .unwrap_or_default();
        }
        let (Some((name, _, scale)), Some(buffer), Some(origin)) =
            (&panel.on, &panel.buffer, self.launcher_origin())
        else {
            return Vec::new();
        };
        if !panel.launcher.is_open() || *name != output.name() || self.matrix_launcher_playing() {
            return Vec::new();
        }
        let view = panel.launcher.view();
        let all = Rectangle::from_size(Size::from((f64::from(view.width), f64::from(view.height))));
        let size = Size::<f64, Logical>::from((
            f64::from(view.width) / scale,
            f64::from(view.height) / scale,
        ))
        .to_i32_round();
        let Some(output_area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let location: Point<f64, Physical> =
            (origin - output_area.loc.to_f64()).to_physical(*scale);
        match MemoryRenderBufferRenderElement::from_buffer(
            renderer,
            location,
            buffer,
            None,
            Some(all),
            Some(size),
            Kind::Unspecified,
        ) {
            Ok(element) => vec![element],
            Err(err) => {
                tracing::warn!(?err, "could not draw the launcher");
                Vec::new()
            }
        }
    }

    /// The launcher in the Matrix theme on `output` (#155): behind it, the
    /// dim and the rain; as it opens or closes, its parts coming or going.
    /// Front first.
    pub fn matrix_launcher_on<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<crate::scene::SceneElement<R>>
    where
        R: Renderer + smithay::backend::renderer::ImportAll + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        if !self.matrix_look() {
            return Vec::new();
        }
        let Some(panel) = &self.launcher else {
            return Vec::new();
        };
        let (Some((name, _, scale)), Some(buffer), Some(origin)) =
            (&panel.on, &panel.buffer, self.launcher_origin())
        else {
            return Vec::new();
        };
        if *name != output.name() || panel.showcase.is_some() {
            return Vec::new();
        }
        let Some(output_area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let open = panel.launcher.is_open();
        let parts = open.then(|| panel.launcher.parts()).flatten();
        let at = origin - output_area.loc.to_f64();
        self.matrix_launcher_elements(
            renderer,
            buffer,
            at,
            output_area.size.to_f64(),
            parts,
            open,
            *scale,
        )
    }

    pub fn launcher_ui(&self, out: &mut Vec<UiNode>) {
        if let Some(shown) = self.launcher.as_ref().and_then(|p| p.showcase.as_ref()) {
            let (_, area, _) = &shown.on;
            crate::shell::flatten(&shown.showcased.access(), area.loc, 0, out);
        }
        let (Some(panel), Some(origin)) = (&self.launcher, self.launcher_origin()) else {
            return;
        };
        if !panel.launcher.is_open() {
            return;
        }
        let origin = Point::from((origin.x.round() as i32, origin.y.round() as i32));
        crate::shell::flatten(&panel.launcher.access(), origin, 0, out);
    }
}

/// Writes a text offer into a client's pipe, off the loop: the reader may
/// be slow.
/// Writes a picture to a reader, from a thread.
pub fn send_bytes(bytes: Arc<[u8]>, fd: OwnedFd) {
    let written = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            use std::io::Write;
            let mut file = std::fs::File::from(fd);
            let _ = file.write_all(&bytes);
        });
    if let Err(err) = written {
        tracing::warn!(%err, "could not give the clipboard");
    }
}

pub fn send_text(text: Arc<str>, fd: OwnedFd) {
    let written = std::thread::Builder::new()
        .name("clipboard".into())
        .spawn(move || {
            use std::io::Write;
            let mut file = std::fs::File::from(fd);
            let _ = file.write_all(text.as_bytes());
        });
    if let Err(err) = written {
        tracing::warn!(%err, "could not give the clipboard");
    }
}
