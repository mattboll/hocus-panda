//! The screens' names as the shell's Wayland connection knows them, so a
//! bar opens on the screen the compositor names and never on the one
//! under the pointer (#540, #552).
//!
//! iced_layershell finds the screen of a surface by its xdg-output name,
//! in a cache it does not show. When it does not know the name yet, as
//! for a screen just plugged in, it asks for the surface on no screen, and
//! the compositor puts it under the pointer. So the shell opens a bar only
//! once it knows the name itself, having learnt it the way iced_layershell
//! does, one step behind it at each step:
//!
//! 1. the screen's global: iced_layershell binds its `wl_output` as it
//!    reads it, with an xdg-output of its toolkit's (sctk);
//! 2. that output's `done`, after the xdg-output's events: iced_layershell
//!    asks for the xdg-output of its cache;
//! 3. that xdg-output's name, which fills its cache.
//!
//! The shell's queue, on iced's connection, takes the same steps, each
//! request made only from the shell's update ([`step`]). iced_layershell
//! runs the update once it has handled every event read so far and made
//! the requests they called for (its `NormalDispatch` comes after them in
//! its loop). So each of the shell's requests goes out after
//! iced_layershell's for the same step, the compositor answers them in
//! that order, and a name reaches the shell's queue after it reached
//! iced_layershell's: when the shell sees a name, iced_layershell has it.
//! No round trip, no count: the order is the protocol's.
//!
//! The one update iced_layershell runs elsewhere is a held key's repeat,
//! from a timer of its loop: an update then can see an event that
//! iced_layershell reads in the same turn but handles just after. The
//! compositor's own defence (one of the bar's surfaces asked for on no
//! screen goes to the screen lacking it) covers that.
//!
//! Nothing here reads the connection: iced's loop does. The queue's
//! events wake [`changes`], whose handlers only note what came; at rest
//! nothing here wakes.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    task::{Context, Poll},
};

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

/// The screens' names the shell knows, which iced_layershell knows too.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Known {
    /// The names of the screens the compositor has given.
    Names(Vec<String>),
    /// The names cannot be known (no connection, no xdg-output):
    /// iced_layershell is left to find the screen as it can.
    Unknowable,
}

/// Takes the steps that are due, from the shell's update only (see the
/// module's comment): what the shell now knows.
pub fn step() -> Known {
    let mut outputs = OUTPUTS.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(present) = outputs.as_mut() else {
        return Known::Unknowable;
    };
    match present.step() {
        Ok(names) => Known::Names(names),
        Err(err) => {
            tracing::warn!(%err, "the screens' events");
            *outputs = None;
            Known::Unknowable
        }
    }
}

/// Wakes when the screens' queue has events that move a screen on a
/// step: the shell then takes the next one ([`step`]).
pub fn changes() -> impl iced::futures::Stream<Item = ()> {
    iced::futures::stream::poll_fn(|cx| {
        let mut outputs = OUTPUTS.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(present) = outputs.as_mut() else {
            return Poll::Ready(None);
        };
        match present.poll(cx) {
            Ok(true) => Poll::Ready(Some(())),
            Ok(false) => Poll::Pending,
            Err(err) => {
                tracing::warn!(%err, "the screens' events");
                *outputs = None;
                Poll::Ready(None)
            }
        }
    })
}

/// Makes the screens' queue on `connection`, the one iced draws on. The
/// screens there are bound later, by the shell's first update, after
/// iced_layershell's own.
pub fn start(connection: &Connection) {
    match Outputs::new(connection) {
        Ok(outputs) => {
            *OUTPUTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(outputs);
        }
        Err(err) => tracing::warn!(%err, "the screens' names cannot be known"),
    }
}

static OUTPUTS: Mutex<Option<Outputs>> = Mutex::new(None);

struct Outputs {
    connection: Connection,
    queue: EventQueue<Screens>,
    registry: WlRegistry,
    screens: Screens,
}

/// The screens by their global's name.
struct Screens {
    manager: ZxdgOutputManagerV1,
    by_global: HashMap<u32, Screen>,
    /// A screen moved on a step since the shell last looked.
    changed: bool,
}

/// Where a screen is in giving its name. Its events move it on; the
/// shell's update makes the requests ([`Outputs::step`]).
enum Screen {
    /// Its global, not bound yet.
    Seen { version: u32 },
    /// Bound; its first `done` has not come.
    Bound(WlOutput),
    /// Its first `done` came: its xdg-output is to be asked for.
    Described(WlOutput),
    /// Its xdg-output asked for; its name has not come.
    Asked(WlOutput, ZxdgOutputV1),
    /// Its name came: its objects are to be let go.
    Named(WlOutput, ZxdgOutputV1, String),
    /// Its name, and none of the shell's objects left on it. The
    /// compositor tells a surface its screens on every `wl_output` its
    /// client has; iced_layershell keeps the last one, and closes the
    /// surface when any screen goes if that one is not its own.
    Known(String),
}

impl Outputs {
    fn new(connection: &Connection) -> Result<Self, String> {
        let (globals, queue) =
            registry_queue_init::<Screens>(connection).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        // Names came with version 2.
        let manager: ZxdgOutputManagerV1 = globals
            .bind(&qh, 2..=3, ())
            .map_err(|e| format!("zxdg_output_manager_v1: {e}"))?;
        let by_global = globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == WlOutput::interface().name)
                .map(|g| (g.name, Screen::Seen { version: g.version }))
                .collect()
        });
        Ok(Self {
            connection: connection.clone(),
            registry: globals.registry().clone(),
            queue,
            screens: Screens {
                manager,
                by_global,
                // The screens there as the shell starts wait for its
                // first update.
                changed: true,
            },
        })
    }

    /// Handles what iced's loop has read for the queue, and says whether
    /// a screen moved on since the shell last looked; `cx` is woken by
    /// the queue's next events.
    fn poll(&mut self, cx: &mut Context<'_>) -> Result<bool, String> {
        if let Poll::Ready(Err(err)) = self.queue.poll_dispatch_pending(cx, &mut self.screens) {
            return Err(err.to_string());
        }
        Ok(std::mem::take(&mut self.screens.changed))
    }

    fn step(&mut self) -> Result<Vec<String>, String> {
        self.queue
            .dispatch_pending(&mut self.screens)
            .map_err(|e| e.to_string())?;
        self.screens.changed = false;
        let qh = self.queue.handle();
        let mut asked = false;
        for (global, screen) in &mut self.screens.by_global {
            let next = match std::mem::replace(screen, Screen::Known(String::new())) {
                Screen::Seen { version } => {
                    asked = true;
                    Screen::Bound(self.registry.bind(*global, version.min(4), &qh, *global))
                }
                Screen::Described(output) => {
                    asked = true;
                    let xdg = self.screens.manager.get_xdg_output(&output, &qh, *global);
                    Screen::Asked(output, xdg)
                }
                Screen::Named(output, xdg, name) => {
                    asked = true;
                    let_go(&output, &xdg);
                    Screen::Known(name)
                }
                other => other,
            };
            *screen = next;
        }
        if asked {
            // iced's loop flushes what is left when the socket is full.
            match self.connection.flush() {
                Ok(()) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => tracing::warn!(%err, "the screens' requests"),
            }
        }
        Ok(self
            .screens
            .by_global
            .values()
            .filter_map(|s| match s {
                Screen::Known(name) => Some(name.clone()),
                _ => None,
            })
            .collect())
    }
}

fn let_go(output: &WlOutput, xdg: &ZxdgOutputV1) {
    xdg.destroy();
    if output.version() >= 3 {
        output.release();
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Screens {
    fn event(
        screens: &mut Self,
        _: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == WlOutput::interface().name => {
                screens.by_global.insert(name, Screen::Seen { version });
                screens.changed = true;
            }
            wl_registry::Event::GlobalRemove { name } => match screens.by_global.remove(&name) {
                Some(Screen::Bound(output) | Screen::Described(output)) => {
                    if output.version() >= 3 {
                        output.release();
                    }
                }
                Some(Screen::Asked(output, xdg) | Screen::Named(output, xdg, _)) => {
                    let_go(&output, &xdg);
                }
                Some(Screen::Seen { .. } | Screen::Known(_)) | None => {}
            },
            _ => {}
        }
    }
}

impl Dispatch<WlOutput, u32> for Screens {
    fn event(
        screens: &mut Self,
        _: &WlOutput,
        event: wl_output::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_output::Event::Done = event
            && let Some(screen) = screens.by_global.get_mut(global)
            && let Screen::Bound(output) = screen
        {
            *screen = Screen::Described(output.clone());
            screens.changed = true;
        }
    }
}

impl Dispatch<ZxdgOutputManagerV1, ()> for Screens {
    fn event(
        _: &mut Self,
        _: &ZxdgOutputManagerV1,
        _: <ZxdgOutputManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZxdgOutputV1, u32> for Screens {
    fn event(
        screens: &mut Self,
        _: &ZxdgOutputV1,
        event: zxdg_output_v1::Event,
        global: &u32,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_output_v1::Event::Name { name } = event
            && let Some(screen) = screens.by_global.get_mut(global)
            && let Screen::Asked(output, xdg) = screen
        {
            *screen = Screen::Named(output.clone(), xdg.clone(), name);
            screens.changed = true;
        }
    }
}
