//! hocus-ui widgets in a plain Wayland window, for working on the toolkit
//! without the compositor: `cargo run -p hocus-ui --example gallery`.
//!
//! Tab and Shift+Tab move the focus, Enter or a click presses a button.
//! Frames are drawn only while something moves or changes.

use std::{
    f32::consts::TAU,
    time::{Duration, Instant},
};

use anyhow::Context;
use hocus_ui::{
    Align, ButtonKind, Canvas, Direction, Draw, Event, Input, Insets, Key, Kind, Length, Look,
    NodeId, Paint, Point, Rect, Size, Style, Surface, Theme, Tree,
    motion::{Curve, Spring},
    tree::Property,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_compositor, delegate_output, delegate_registry, delegate_shm, delegate_xdg_shell,
    delegate_xdg_window,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        xdg::{
            XdgShell,
            window::{Window, WindowConfigure, WindowDecorations, WindowHandler},
        },
    },
    shm::{Shm, ShmHandler, slot::SlotPool},
};
use wayland_client::{
    Connection, Dispatch, QueueHandle, WEnum,
    globals::registry_queue_init,
    protocol::{wl_keyboard, wl_output, wl_pointer, wl_seat, wl_shm, wl_surface},
};

const SIZE: Size = Size::new(360.0, 420.0);

/// The agent's spinner: an arc that turns, one turn a second, while the
/// agent works.
struct Spinner;

impl Draw for Spinner {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, now: Duration) {
        let center = Point::new(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
        let angle = now.as_secs_f32().fract() * TAU;
        canvas.arc(
            center,
            rect.w / 2.0 - 3.0,
            angle,
            4.2,
            4.0,
            theme.colors.agent,
        );
    }

    fn size(&self) -> Size {
        Size::new(36.0, 36.0)
    }

    fn is_moving(&self, _now: Duration) -> bool {
        true
    }
}

struct Gallery {
    tree: Tree,
    surface: Surface,
    started: Instant,
    progress: NodeId,
    chip: NodeId,
    toggle: NodeId,
    reset: NodeId,
    right: bool,
}

impl Gallery {
    fn new(scale: f32) -> anyhow::Result<Self> {
        let theme = Theme::default();
        let mut tree = Tree::new(theme.clone(), SIZE, scale);
        let root = tree.root();
        let none = || anyhow::anyhow!("not added");
        tree.set_style(
            root,
            Style {
                padding: Insets::all(theme.spacing.l),
                gap: theme.spacing.m,
                ..Style::default()
            },
        );
        tree.add(
            root,
            Style::default(),
            Kind::text("14:32", theme.text.display),
        )
        .ok_or_else(none)?;
        let card = |tree: &mut Tree| {
            tree.add(
                root,
                Style {
                    padding: Insets::all(theme.spacing.l),
                    gap: theme.spacing.s,
                    direction: Direction::Row,
                    align: Align::Center,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: theme.radii.large,
                    border: Some(Paint::Border),
                    shadow: 12.0,
                }),
            )
        };
        let agent = card(&mut tree).ok_or_else(none)?;
        tree.add(
            agent,
            Style::default(),
            Kind::Custom {
                draw: Box::new(Spinner),
                label: Some("Agent au travail".into()),
            },
        )
        .ok_or_else(none)?;
        let texts = tree
            .add(
                agent,
                Style {
                    width: Length::Fill,
                    gap: theme.spacing.xs,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .ok_or_else(none)?;
        tree.add(
            texts,
            Style::default(),
            Kind::text("Agent\u{202f}: compilation…", theme.text.title),
        )
        .ok_or_else(none)?;
        let progress = tree
            .add(
                texts,
                Style::default(),
                Kind::text(
                    "0\u{202f}% · étape 3 sur 7",
                    theme.text.body.with_color(Paint::Muted),
                ),
            )
            .ok_or_else(none)?;
        let track = tree
            .add(
                root,
                Style {
                    height: Length::Px(64.0),
                    padding: Insets::all(8.0),
                    align: Align::Start,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Surface),
                    radius: theme.radii.large,
                    border: None,
                    shadow: 0.0,
                }),
            )
            .ok_or_else(none)?;
        let chip = tree
            .add(
                track,
                Style {
                    width: Length::Px(48.0),
                    height: Length::Px(48.0),
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::Accent),
                    radius: theme.radii.medium,
                    border: None,
                    shadow: 0.0,
                }),
            )
            .ok_or_else(none)?;
        let buttons = tree
            .add(
                root,
                Style {
                    direction: Direction::Row,
                    gap: theme.spacing.s,
                    justify: hocus_ui::Justify::End,
                    ..Style::default()
                },
                Kind::Box(Look::default()),
            )
            .ok_or_else(none)?;
        let reset = tree
            .add(
                buttons,
                Style::default(),
                Kind::button("Remettre", ButtonKind::Plain, &theme),
            )
            .ok_or_else(none)?;
        let toggle = tree
            .add(
                buttons,
                Style::default(),
                Kind::button("Glisser", ButtonKind::Primary, &theme),
            )
            .ok_or_else(none)?;
        println!("{}", tree.access().dump());
        Ok(Self {
            surface: Surface::new(SIZE, scale),
            tree,
            started: Instant::now(),
            progress,
            chip,
            toggle,
            reset,
            right: false,
        })
    }

    fn now(&self) -> Duration {
        self.started.elapsed()
    }

    fn input(&mut self, input: Input) {
        for event in self.tree.input(input) {
            let Event::Activated(id) = event else {
                continue;
            };
            let target = if id == self.toggle {
                self.right = !self.right;
                if self.right { 264.0 } else { 0.0 }
            } else if id == self.reset {
                self.right = false;
                0.0
            } else {
                continue;
            };
            self.tree.animate(
                self.chip,
                Property::OffsetX,
                target,
                Curve::Spring(Spring::SETTLE),
            );
        }
    }

    /// The agent's progress, from the time.
    fn tick(&mut self, now: Duration) {
        let percent = (now.as_secs_f32() * 12.0) as u32 % 101;
        self.tree.set_text(
            self.progress,
            &format!("{percent}\u{202f}% · étape 3 sur 7"),
        );
    }
}

fn main() -> anyhow::Result<()> {
    let conn = Connection::connect_to_env().context("no Wayland compositor")?;
    let (globals, mut queue) = registry_queue_init(&conn)?;
    let qh = queue.handle();
    let compositor = CompositorState::bind(&globals, &qh).context("no wl_compositor")?;
    let xdg_shell = XdgShell::bind(&globals, &qh).context("no xdg_wm_base")?;
    let shm = Shm::bind(&globals, &qh).context("no wl_shm")?;
    let _seat: wl_seat::WlSeat = globals.bind(&qh, 1..=7, ()).context("no wl_seat")?;
    let surface = compositor.create_surface(&qh);
    let window = xdg_shell.create_window(surface, WindowDecorations::RequestServer, &qh);
    window.set_title("hocus-ui gallery");
    window.set_app_id("hocus-ui-gallery");
    window.set_min_size(Some((SIZE.w as u32, SIZE.h as u32)));
    window.commit();
    let pool = SlotPool::new((SIZE.w * SIZE.h * 4.0) as usize, &shm)?;
    let mut app = App {
        registry: RegistryState::new(&globals),
        outputs: OutputState::new(&globals, &qh),
        shm,
        pool,
        window,
        gallery: Gallery::new(1.0)?,
        configured: false,
        frame_pending: false,
        shift: false,
        closed: false,
    };
    while !app.closed {
        queue.blocking_dispatch(&mut app)?;
    }
    Ok(())
}

struct App {
    registry: RegistryState,
    outputs: OutputState,
    shm: Shm,
    pool: SlotPool,
    window: Window,
    gallery: Gallery,
    configured: bool,
    frame_pending: bool,
    shift: bool,
    closed: bool,
}

impl App {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let g = &mut self.gallery;
        let now = g.now();
        g.tick(now);
        let damage = g.tree.update(now);
        let drawn = g.surface.draw(&mut g.tree, &damage);
        let (w, h) = (g.surface.width(), g.surface.height());
        let Ok((buffer, canvas)) =
            self.pool
                .create_buffer(w as i32, h as i32, w as i32 * 4, wl_shm::Format::Argb8888)
        else {
            return;
        };
        // Premultiplied RGBA to the little-endian ARGB of wl_shm.
        for (dst, src) in canvas
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(g.surface.pixels().as_chunks::<4>().0)
        {
            dst.copy_from_slice(&[src[2], src[1], src[0], src[3]]);
        }
        let surface = self.window.wl_surface();
        for r in drawn {
            surface.damage_buffer(r.x as i32, r.y as i32, r.w as i32, r.h as i32);
        }
        if g.tree.is_animating(now) {
            surface.frame(qh, surface.clone());
            self.frame_pending = true;
        }
        let _ = buffer.attach_to(surface);
        self.window.commit();
    }

    fn wake(&mut self, qh: &QueueHandle<Self>) {
        if self.configured && !self.frame_pending {
            self.draw(qh);
        }
    }
}

impl WindowHandler for App {
    fn request_close(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &Window) {
        self.closed = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &Window,
        _: WindowConfigure,
        _: u32,
    ) {
        if !self.configured {
            self.configured = true;
            self.draw(qh);
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        self.frame_pending = false;
        self.draw(qh);
    }

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for App {
    fn event(
        _: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(caps),
        } = event
        {
            if caps.contains(wl_seat::Capability::Pointer) {
                seat.get_pointer(qh, ());
            }
            if caps.contains(wl_seat::Capability::Keyboard) {
                seat.get_keyboard(qh, ());
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let input = match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            }
            | wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => Input::PointerMove(Point::new(surface_x as f32, surface_y as f32)),
            wl_pointer::Event::Leave { .. } => Input::PointerLeave,
            // BTN_LEFT
            wl_pointer::Event::Button {
                button: 0x110,
                state,
                ..
            } => Input::PointerButton {
                pressed: state == WEnum::Value(wl_pointer::ButtonState::Pressed),
            },
            _ => return,
        };
        app.gallery.input(input);
        app.wake(qh);
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for App {
    fn event(
        app: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        let wl_keyboard::Event::Key { key, state, .. } = event else {
            return;
        };
        let pressed = state == WEnum::Value(wl_keyboard::KeyState::Pressed);
        // Evdev codes: the layout does not matter for these keys.
        let key = match key {
            42 | 54 => {
                app.shift = pressed;
                return;
            }
            15 if app.shift => Key::BackTab,
            15 => Key::Tab,
            28 => Key::Enter,
            57 => Key::Space,
            _ => return,
        };
        if pressed {
            app.gallery.input(Input::Key(key));
            app.wake(qh);
        }
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.outputs
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for App {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry
    }

    registry_handlers![OutputState];
}

delegate_compositor!(App);
delegate_output!(App);
delegate_shm!(App);
delegate_xdg_shell!(App);
delegate_xdg_window!(App);
delegate_registry!(App);
