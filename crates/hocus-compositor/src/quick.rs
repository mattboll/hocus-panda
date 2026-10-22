//! Quick settings in the compositor: the sheet on the screen under the
//! pointer, the rest of that screen dimmed behind it, the keyboard and the
//! pointer for the sheet while open. Its values come from the platform's
//! quick settings service (a stub until M3), on its own thread.

use hocus_api::UiNode;
use hocus_platform::quick::{self, Change, Profile, Quick};
use hocus_shell::{
    Lang, QuickAction, QuickState, Sheet, Tile,
    quick::{MARGIN, ROOM, WIDTH, tiles_in_order},
};
use hocus_ui::{Input, Key, Point as UiPoint, Theme};
use smithay::{
    backend::renderer::{
        ImportMem, Renderer,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
            solid::{SolidColorBuffer, SolidColorRenderElement},
        },
    },
    input::keyboard::{Keysym, Layout},
    output::Output,
    reexports::calloop::{LoopHandle, channel},
    utils::{Logical, Physical, Point, Rectangle, SERIAL_COUNTER, Size},
};

use crate::{App, State};

/// How dark the rest of the screen gets behind the sheet.
const DIM: f32 = 0.5;

#[derive(Debug)]
pub struct QuickPanel {
    sheet: Sheet,
    lang: Lang,
    on: Option<(String, Rectangle<i32, Logical>, f64)>,
    buffer: Option<MemoryRenderBuffer>,
    backdrop: SolidColorBuffer,
    service: Option<Quick>,
    /// The latest state from the service.
    platform: Option<quick::State>,
    /// Do not disturb: toasts are held, the history keeps them.
    pub dnd: bool,
    pressed: bool,
}

impl QuickPanel {
    pub fn set_theme(&mut self, theme: Theme) {
        self.sheet.set_theme(theme);
    }

    pub fn set_lang(&mut self, lang: Lang) {
        if lang != self.lang {
            self.lang = lang;
            self.sheet.set_lang(lang);
        }
    }
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, lang: Lang) {
    let (sender, receiver) = channel::channel::<quick::State>();
    let inserted = handle.insert_source(receiver, |event, _, app| {
        if let channel::Event::Msg(platform) = event {
            app.timed("quick settings", |app| {
                if let Some(panel) = app.state.quick.as_mut() {
                    panel.platform = Some(platform);
                }
                app.state.needs_redraw = true;
            });
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "quick settings will show nothing");
    }
    let screens: Vec<String> = state.space.outputs().map(Output::name).collect();
    let sender = std::sync::Mutex::new(sender);
    let sink = move |s| {
        if let Ok(sender) = sender.lock() {
            let _ = sender.send(s);
        }
    };
    // The machine's services only as the session itself: nested, quick
    // settings would change the developer's sound and screens. Tests give
    // a private bus, or get a stub.
    let service = if state.is_session || state.system_bus.is_some() {
        let buses = hocus_platform::system::Buses {
            system: state.system_bus.clone(),
        };
        Quick::system(buses, &screens, sink)
    } else {
        Quick::stub(quick::stub_state(&screens), sink)
    }
    .inspect_err(|err| tracing::warn!(%err, "no quick settings service"))
    .ok();
    state.quick = Some(QuickPanel {
        sheet: Sheet::new(Theme::default(), lang, 600.0, 1.0),
        lang,
        on: None,
        buffer: None,
        backdrop: SolidColorBuffer::default(),
        service,
        platform: None,
        dnd: false,
        pressed: false,
    });
}

impl State {
    pub fn quick_open(&self) -> bool {
        self.quick.as_ref().is_some_and(|q| q.sheet.is_open())
    }

    pub fn toggle_quick(&mut self) {
        if self.quick_open() {
            self.close_quick();
        } else {
            self.open_quick();
        }
    }

    /// Opens on the screen under the pointer, from its right edge.
    pub fn open_quick(&mut self) {
        if self.launcher_open() {
            self.close_launcher();
        }
        let place = self.output_under_pointer().and_then(|o| {
            Some((
                o.name(),
                self.space.output_geometry(&o)?,
                o.current_scale().fractional_scale(),
            ))
        });
        let Some(panel) = self.quick.as_mut() else {
            return;
        };
        let Some((_, area, scale)) = &place else {
            return;
        };
        panel
            .sheet
            .resize((area.size.h as f32 - 2.0 * MARGIN).max(1.0), *scale as f32);
        // Black, see-through, premultiplied.
        panel.backdrop.update(area.size, [0.0, 0.0, 0.0, DIM]);
        if panel.on != place {
            panel.buffer = None;
        }
        panel.on = place;
        panel.sheet.open();
        // Clients let go of the keyboard while it is open.
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, None, SERIAL_COUNTER.next_serial());
        }
        self.needs_redraw = true;
    }

    pub fn close_quick(&mut self) {
        if let Some(panel) = self.quick.as_mut() {
            panel.sheet.close();
        }
        self.apply_focus(SERIAL_COUNTER.next_serial());
        self.needs_redraw = true;
    }

    /// The power saver is on: background work waits.
    pub fn power_saver(&self) -> bool {
        self.quick
            .as_ref()
            .and_then(|q| q.platform.as_ref())
            .is_some_and(|p| p.profile == Profile::Saver)
    }

    /// What the sheet shows, from the service, the shell and the keyboard.
    fn quick_state(&self) -> Option<QuickState> {
        let panel = self.quick.as_ref()?;
        let lang = panel.lang;
        let p = panel.platform.as_ref()?;
        let works = |t: Tile| {
            matches!(
                t,
                Tile::Wifi
                    | Tile::Bluetooth
                    | Tile::DoNotDisturb
                    | Tile::Airplane
                    | Tile::NightLight
                    | Tile::Focus
                    | Tile::Record
            )
        };
        let tiles = tiles_in_order()
            .into_iter()
            .map(|t| {
                let on = match t {
                    Tile::Wifi => p.wifi,
                    Tile::Bluetooth => p.bluetooth,
                    Tile::DoNotDisturb => panel.dnd,
                    Tile::Airplane => p.airplane,
                    Tile::NightLight => self.night_on(),
                    Tile::Focus => self.comfort.focus_mode,
                    Tile::Record => self.recording(),
                    _ => false,
                };
                let detail = match t {
                    Tile::Wifi if p.wifi => p
                        .network
                        .clone()
                        .unwrap_or_else(|| lang.on_off(true).into()),
                    Tile::Bluetooth if p.bluetooth => p
                        .bluetooth_device
                        .clone()
                        .unwrap_or_else(|| lang.on_off(true).into()),
                    Tile::NightLight if on => lang.night_until(&self.config.night_light.to),
                    Tile::NightLight => lang.night_from(&self.config.night_light.from),
                    Tile::Focus => lang.focus_detail().into(),
                    Tile::Record => lang.record_detail().into(),
                    t if works(t) => lang.on_off(on).into(),
                    _ => lang.coming().into(),
                };
                (t, on, detail, works(t))
            })
            .collect();
        Some(QuickState {
            tiles,
            profile: match p.profile {
                Profile::Saver => 0,
                Profile::Balanced => 1,
                Profile::Performance => 2,
            },
            profiles: p.profiles,
            volume: p.volume,
            output_device: p.output_device.clone(),
            // Every screen plugged now; the service knows those it has
            // a value for.
            brightness: self
                .space
                .outputs()
                .map(|o| {
                    let name = o.name();
                    let value = p
                        .brightness
                        .iter()
                        .find(|(s, _)| *s == name)
                        .map_or(80, |(_, v)| *v);
                    (name, value)
                })
                .collect(),
            battery: p
                .battery
                .as_ref()
                .map(|b| lang.battery_left(b.percent, b.minutes_left)),
            layouts: self
                .config
                .keyboard
                .layouts
                .iter()
                .map(|l| l.chars().take(4).flat_map(char::to_uppercase).collect())
                .collect(),
            layout: self.layout_index,
            notifications: self.notification_count(),
        })
    }

    fn quick_actions(&mut self, actions: Vec<QuickAction>) {
        for action in actions {
            let change = match action {
                QuickAction::Closed => {
                    self.close_quick();
                    None
                }
                QuickAction::Toggle(Tile::Wifi, on) => Some(Change::Wifi(on)),
                QuickAction::Toggle(Tile::Bluetooth, on) => Some(Change::Bluetooth(on)),
                QuickAction::Toggle(Tile::Airplane, on) => Some(Change::Airplane(on)),
                QuickAction::Toggle(Tile::DoNotDisturb, on) => {
                    if let Some(panel) = self.quick.as_mut() {
                        panel.dnd = on;
                    }
                    None
                }
                QuickAction::Toggle(Tile::NightLight, _) => {
                    self.toggle_night_light();
                    None
                }
                QuickAction::Toggle(Tile::Focus, _) => {
                    self.toggle_focus_mode();
                    None
                }
                QuickAction::Toggle(Tile::Record, _) => {
                    self.close_quick();
                    self.toggle_recording();
                    None
                }
                QuickAction::Toggle(..) => None,
                QuickAction::Profile(i) => Some(Change::Profile(match i {
                    0 => Profile::Saver,
                    2 => Profile::Performance,
                    _ => Profile::Balanced,
                })),
                QuickAction::Volume(v) => Some(Change::Volume(v)),
                QuickAction::Brightness(screen, v) => Some(Change::Brightness(screen, v)),
                QuickAction::Layout(i) => {
                    self.set_layout(i);
                    None
                }
                QuickAction::ClearNotifications => {
                    self.clear_notifications();
                    None
                }
                QuickAction::OpenSettings => {
                    self.close_quick();
                    self.open_settings(None);
                    None
                }
            };
            if let (Some(change), Some(service)) =
                (change, self.quick.as_ref().and_then(|q| q.service.as_ref()))
            {
                service.change(change);
            }
        }
        self.needs_redraw = true;
    }

    /// Uses the `index`th layout of the configuration.
    fn set_layout(&mut self, index: usize) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let name = keyboard.with_xkb_state(self, |mut ctx| {
            ctx.set_layout(Layout(index as u32));
            ctx.xkb()
                .lock()
                .map(|x| x.layout_name(x.active_layout()).to_string())
                .unwrap_or_default()
        });
        self.layout_index = index;
        self.emit(|_| hocus_api::Event::KeyboardLayout { layout: name });
    }

    /// A key while the sheet is open.
    pub fn quick_key(&mut self, sym: Keysym) -> bool {
        if !self.quick_open() {
            return false;
        }
        let key = match sym {
            Keysym::Escape => Some(Key::Escape),
            Keysym::Tab => Some(Key::Tab),
            Keysym::ISO_Left_Tab => Some(Key::BackTab),
            Keysym::Left => Some(Key::Left),
            Keysym::Right => Some(Key::Right),
            Keysym::Home => Some(Key::Home),
            Keysym::End => Some(Key::End),
            Keysym::Return | Keysym::KP_Enter => Some(Key::Enter),
            Keysym::space => Some(Key::Space),
            _ => None,
        };
        if let Some(key) = key
            && let Some(panel) = self.quick.as_mut()
        {
            let actions = panel.sheet.key(key);
            self.quick_actions(actions);
        }
        true
    }

    fn quick_origin(&self) -> Option<Point<f64, Logical>> {
        let (_, area, _) = self.quick.as_ref()?.on.as_ref()?;
        Some(Point::from((
            f64::from(area.loc.x + area.size.w) - f64::from(MARGIN + WIDTH + ROOM),
            f64::from(area.loc.y) + f64::from(MARGIN),
        )))
    }

    fn quick_local(&self, location: Point<f64, Logical>) -> Option<UiPoint> {
        let origin = self.quick_origin()?;
        Some(UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        ))
    }

    /// While open, the sheet takes the pointer everywhere.
    pub fn quick_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        if !self.quick_open() {
            return false;
        }
        if let Some(at) = self.quick_local(location)
            && let Some(panel) = self.quick.as_mut()
        {
            let actions = panel.sheet.input(at, Input::PointerMove(at));
            self.quick_actions(actions);
        }
        true
    }

    pub fn quick_pointer_button(&mut self, location: Point<f64, Logical>, pressed: bool) -> bool {
        let open = self.quick_open();
        let Some(panel) = self.quick.as_mut() else {
            return false;
        };
        if !open && !(panel.pressed && !pressed) {
            return false;
        }
        panel.pressed = pressed;
        if let Some(at) = self.quick_local(location)
            && let Some(panel) = self.quick.as_mut()
        {
            let actions = panel.sheet.input(at, Input::PointerButton { pressed });
            self.quick_actions(actions);
        }
        true
    }

    /// Follows the state and draws what changed; after each dispatch.
    pub fn refresh_quick(&mut self) {
        let now = self.clock.now();
        let state = self.quick_state();
        let Some(panel) = self.quick.as_mut() else {
            return;
        };
        if !panel.sheet.is_open() {
            return;
        }
        if let Some(state) = state {
            panel.sheet.set_state(&state);
        }
        let damage = panel.sheet.draw(now);
        let view = panel.sheet.view();
        let mut changed = !damage.is_empty();
        match &mut panel.buffer {
            Some(buffer) => crate::shell::upload(buffer, &view, &damage),
            None => {
                panel.buffer = Some(crate::shell::buffer_for(&view));
                changed = true;
            }
        }
        if changed {
            self.needs_redraw = true;
        }
    }

    /// The sheet and the dimmed screen behind it, on `output`.
    pub fn quick_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> (
        Vec<MemoryRenderBufferRenderElement<R>>,
        Option<SolidColorRenderElement>,
    )
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let none = (Vec::new(), None);
        let Some(panel) = &self.quick else {
            return none;
        };
        let (Some((name, area, scale)), Some(buffer), Some(origin)) =
            (&panel.on, &panel.buffer, self.quick_origin())
        else {
            return none;
        };
        if !panel.sheet.is_open() || *name != output.name() {
            return none;
        }
        let view = panel.sheet.view();
        let all = Rectangle::from_size(Size::from((f64::from(view.width), f64::from(view.height))));
        let size = Size::<f64, Logical>::from((
            f64::from(view.width) / scale,
            f64::from(view.height) / scale,
        ))
        .to_i32_round();
        let location: Point<f64, Physical> = (origin - area.loc.to_f64()).to_physical(*scale);
        let sheet = match MemoryRenderBufferRenderElement::from_buffer(
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
                tracing::warn!(?err, "could not draw quick settings");
                Vec::new()
            }
        };
        let dim = SolidColorRenderElement::from_buffer(
            &panel.backdrop,
            Point::<i32, Physical>::from((0, 0)),
            *scale,
            1.0,
            Kind::Unspecified,
        );
        (sheet, Some(dim))
    }

    pub fn quick_ui(&self, out: &mut Vec<UiNode>) {
        let (Some(panel), Some(origin)) = (&self.quick, self.quick_origin()) else {
            return;
        };
        if !panel.sheet.is_open() {
            return;
        }
        let origin = Point::from((origin.x.round() as i32, origin.y.round() as i32));
        crate::shell::flatten(&panel.sheet.access(), origin, 0, out);
    }

    /// Do not disturb: toasts are held.
    pub fn dnd(&self) -> bool {
        self.quick.as_ref().is_some_and(|q| q.dnd)
    }
}

impl State {
    /// What the column's status row tells: what the system services said.
    pub fn status(&self) -> hocus_shell::column::Status {
        let Some(p) = self.quick.as_ref().and_then(|q| q.platform.as_ref()) else {
            return hocus_shell::column::Status::default();
        };
        hocus_shell::column::Status {
            profile: Some(match p.profile {
                Profile::Saver => 0,
                Profile::Balanced => 1,
                Profile::Performance => 2,
            }),
            network: Some((p.wifi, p.network.clone())),
            sound: Some((p.volume, p.muted)),
            battery: p.battery.as_ref().map(|b| b.percent),
        }
    }
}
