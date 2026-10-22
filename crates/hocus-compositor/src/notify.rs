//! Notifications: the freedesktop server runs on its own threads
//! (hocus-platform); what it receives comes through a channel and becomes a
//! toast at the top right of the screen being worked on, left of the
//! column. Closing, expiring and actions are signalled back to the client.

use std::time::Duration;

use hocus_api::UiNode;
use hocus_platform::notifications::{CloseReason, Expiry, Incoming, Notifications, Urgency};
use hocus_shell::{
    Lang, Toast, ToastEvent, Toasts,
    toasts::{ROOM, WIDTH},
};
use hocus_ui::{Input, Point as UiPoint, Theme};
use smithay::{
    backend::renderer::{
        ImportMem, Renderer,
        element::{
            Kind,
            memory::{MemoryRenderBuffer, MemoryRenderBufferRenderElement},
        },
    },
    output::Output,
    reexports::calloop::{LoopHandle, channel},
    utils::{Logical, Physical, Point, Rectangle, Size},
};

use crate::{App, State};

/// How long a toast stays when its client lets us choose.
const NORMAL: Duration = Duration::from_secs(6);
const LOW: Duration = Duration::from_secs(4);
/// Between the toasts and the work area's edges.
const INSET: i32 = 8;

#[derive(Debug)]
pub struct ToastPanel {
    toasts: Toasts,
    server: Option<Notifications>,
    /// The output they are on, its work area and scale.
    on: Option<(String, Rectangle<i32, Logical>, f64)>,
    buffer: Option<MemoryRenderBuffer>,
    pointer_inside: bool,
    pressed: bool,
}

impl ToastPanel {
    pub fn set_theme(&mut self, theme: Theme) {
        self.toasts.set_theme(theme);
    }
}

/// Serves notifications on the bus at `address`; toasts are shown even if
/// the bus is not there (nothing then sends any).
pub fn start(
    handle: &LoopHandle<'static, App>,
    state: &mut State,
    address: Option<&str>,
    lang: Lang,
) {
    let mut panel = ToastPanel {
        toasts: Toasts::new(Theme::default(), lang, 600.0, 1.0),
        server: None,
        on: None,
        buffer: None,
        pointer_inside: false,
        pressed: false,
    };
    if let Some(address) = address {
        let (sender, receiver) = channel::channel::<Incoming>();
        let sender = std::sync::Mutex::new(sender);
        let inserted = handle.insert_source(receiver, |event, _, app| {
            if let channel::Event::Msg(incoming) = event {
                app.timed("notification", |app| app.state.on_notification(incoming));
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "notifications will not be shown");
        } else {
            let sink = move |incoming| {
                if let Ok(sender) = sender.lock() {
                    let _ = sender.send(incoming);
                }
            };
            match Notifications::start(address, sink, Duration::from_secs(2)) {
                Ok(server) => {
                    tracing::info!("serving notifications");
                    panel.server = Some(server);
                }
                Err(err) => tracing::warn!(%err, "not serving notifications"),
            }
        }
    }
    state.toasts = Some(panel);
}

fn toast_of(n: hocus_platform::notifications::Notification) -> Toast {
    let timeout = match (n.expiry, n.urgency) {
        (_, Urgency::Critical) | (Expiry::Never, _) => None,
        (Expiry::After(d), _) => Some(d),
        (Expiry::Default, Urgency::Low) => Some(LOW),
        (Expiry::Default, Urgency::Normal) => Some(NORMAL),
    };
    Toast {
        id: n.id,
        app: n.app,
        summary: n.summary,
        body: n.body,
        actions: n.actions,
        critical: n.urgency == Urgency::Critical,
        timeout,
    }
}

impl State {
    fn on_notification(&mut self, incoming: Incoming) {
        let now = self.clock.now();
        let dnd = self.dnd();
        // A first toast goes where the user works; the others join it.
        let place = self
            .toasts
            .as_ref()
            .filter(|p| p.toasts.is_empty())
            .and_then(|_| self.output_under_pointer())
            .map(|o| self.toast_place(&o));
        let Some(panel) = self.toasts.as_mut() else {
            return;
        };
        let events = match incoming {
            Incoming::Notify(n) => {
                let shown = hocus_api::Event::Notification {
                    id: n.id,
                    app: n.app.clone(),
                    summary: n.summary.clone(),
                };
                if let Some(place) = place {
                    let (_, area, scale) = &place;
                    panel
                        .toasts
                        .resize((area.size.h - 2 * INSET).max(1) as f32, *scale as f32);
                    if panel.on.as_ref() != Some(&place) {
                        panel.buffer = None;
                    }
                    panel.on = Some(place);
                }
                let toast = toast_of(n);
                // Do not disturb holds all but critical ones: the column's
                // history keeps them.
                if dnd && !toast.critical {
                    panel.toasts.remember(&toast);
                    Vec::new()
                } else {
                    let events = panel.toasts.show(toast, now);
                    self.emit(|_| shown);
                    events
                }
            }
            Incoming::Close(id) => {
                if panel.toasts.close(id)
                    && let Some(server) = &panel.server
                {
                    server.closed(id, CloseReason::Closed);
                }
                Vec::new()
            }
        };
        self.toast_events(events);
        self.needs_redraw = true;
    }

    fn toast_place(&self, output: &Output) -> (String, Rectangle<i32, Logical>, f64) {
        (
            output.name(),
            self.work_area(output),
            output.current_scale().fractional_scale(),
        )
    }

    fn toast_events(&mut self, events: Vec<ToastEvent>) {
        let Some(server) = self.toasts.as_ref().and_then(|p| p.server.as_ref()) else {
            return;
        };
        for event in events {
            match event {
                ToastEvent::Expired(id) => server.closed(id, CloseReason::Expired),
                ToastEvent::Dismissed(id) => server.closed(id, CloseReason::Dismissed),
                ToastEvent::Action(id, key) => {
                    server.action_invoked(id, &key);
                    server.closed(id, CloseReason::Dismissed);
                }
            }
        }
    }

    /// How many notifications the history holds.
    pub fn notification_count(&self) -> usize {
        self.toasts
            .as_ref()
            .map_or(0, |p| p.toasts.history().count())
    }

    pub fn clear_notifications(&mut self) {
        if let Some(panel) = self.toasts.as_mut() {
            panel.toasts.clear_history();
        }
        self.needs_redraw = true;
    }

    /// The newest notifications, for the column.
    pub fn notification_history(&self) -> Vec<(String, String)> {
        self.toasts
            .as_ref()
            .map(|p| {
                p.toasts
                    .history()
                    .take(3)
                    .map(|n| (n.app.clone(), n.summary.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Expires, follows the work area, and draws what changed; called after
    /// each dispatch.
    pub fn refresh_toasts(&mut self) {
        let now = self.clock.now();
        let place = self
            .toasts
            .as_ref()
            .and_then(|p| p.on.as_ref())
            .and_then(|(name, _, _)| self.find_output(name))
            .map(|o| self.toast_place(&o));
        let Some(panel) = self.toasts.as_mut() else {
            return;
        };
        let events = panel.toasts.update(now);
        if let Some(place) = place
            && panel.on.as_ref() != Some(&place)
        {
            let (_, area, scale) = &place;
            panel
                .toasts
                .resize((area.size.h - 2 * INSET).max(1) as f32, *scale as f32);
            panel.on = Some(place);
            panel.buffer = None;
        }
        let damage = panel.toasts.draw(now);
        let mut changed = !damage.is_empty();
        let view = panel.toasts.view();
        match &mut panel.buffer {
            Some(buffer) => crate::shell::upload(buffer, &view, &damage),
            None => {
                panel.buffer = Some(crate::shell::buffer_for(&view));
                changed = true;
            }
        }
        // The countdowns move: frames keep coming while they do.
        if changed || panel.toasts.is_animating(now) {
            self.needs_redraw = true;
        }
        self.toast_events(events);
    }

    /// The toasts' surface corner, in the global logical space.
    fn toasts_origin(&self) -> Option<Point<f64, Logical>> {
        let (_, area, _) = self.toasts.as_ref()?.on.as_ref()?;
        let x = area.loc.x + area.size.w - INSET - WIDTH as i32 - ROOM as i32;
        let y = area.loc.y + INSET - ROOM as i32;
        Some(Point::from((f64::from(x), f64::from(y))))
    }

    fn toasts_local(&self, location: Point<f64, Logical>) -> Option<UiPoint> {
        let origin = self.toasts_origin()?;
        Some(UiPoint::new(
            (location.x - origin.x) as f32,
            (location.y - origin.y) as f32,
        ))
    }

    /// The pointer moved: whether the toasts took it.
    pub fn toasts_pointer_motion(&mut self, location: Point<f64, Logical>) -> bool {
        let Some(at) = self.toasts_local(location) else {
            return false;
        };
        let Some(panel) = self.toasts.as_mut() else {
            return false;
        };
        if panel.toasts.hit(at) || panel.pressed {
            panel.pointer_inside = true;
            let _ = panel.toasts.input(at, Input::PointerMove(at));
            true
        } else {
            if panel.pointer_inside {
                panel.pointer_inside = false;
                let _ = panel.toasts.input(at, Input::PointerLeave);
            }
            false
        }
    }

    /// The primary button: whether the toasts took it.
    pub fn toasts_pointer_button(&mut self, location: Point<f64, Logical>, pressed: bool) -> bool {
        let Some(at) = self.toasts_local(location) else {
            return false;
        };
        let Some(panel) = self.toasts.as_mut() else {
            return false;
        };
        let mine = if pressed {
            panel.toasts.hit(at)
        } else {
            std::mem::take(&mut panel.pressed)
        };
        if !mine {
            return false;
        }
        if pressed {
            panel.pressed = true;
        }
        let events = panel.toasts.input(at, Input::PointerButton { pressed });
        self.toast_events(events);
        self.needs_redraw = true;
        true
    }

    /// The toasts on `output`, above windows.
    pub fn toast_elements<R>(
        &self,
        renderer: &mut R,
        output: &Output,
    ) -> Vec<MemoryRenderBufferRenderElement<R>>
    where
        R: Renderer + ImportMem,
        R::TextureId: Send + Clone + 'static,
    {
        let Some(panel) = &self.toasts else {
            return Vec::new();
        };
        let (Some((name, _, scale)), Some(buffer), Some(origin)) =
            (&panel.on, &panel.buffer, self.toasts_origin())
        else {
            return Vec::new();
        };
        if *name != output.name() || panel.toasts.is_empty() {
            return Vec::new();
        }
        let Some(area) = self.space.output_geometry(output) else {
            return Vec::new();
        };
        let view = panel.toasts.view();
        let all = Rectangle::from_size(Size::from((f64::from(view.width), f64::from(view.height))));
        let size = Size::<f64, Logical>::from((
            f64::from(view.width) / scale,
            f64::from(view.height) / scale,
        ))
        .to_i32_round();
        let local = origin - area.loc.to_f64();
        let location: Point<f64, Physical> = local.to_physical(*scale);
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
                tracing::warn!(?err, "could not draw the toasts");
                Vec::new()
            }
        }
    }

    /// The toasts for the control API, in global coordinates.
    pub fn toasts_ui(&self, out: &mut Vec<UiNode>) {
        let (Some(panel), Some(origin)) = (&self.toasts, self.toasts_origin()) else {
            return;
        };
        if panel.toasts.is_empty() {
            return;
        }
        let origin = Point::from((origin.x.round() as i32, origin.y.round() as i32));
        crate::shell::flatten(&panel.toasts.access(), origin, 0, out);
    }
}
