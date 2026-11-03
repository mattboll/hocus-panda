//! The screens' names as the shell's Wayland connection knows them, so a
//! bar opens on the screen the compositor names and never on the one
//! under the pointer (#540).
//!
//! iced_layershell finds the screen of a surface by its xdg-output name.
//! When it does not know the name yet, as for a screen just plugged in, it
//! asks for the surface on no screen, and the compositor puts it under
//! the pointer. So the shell keeps a bar asked for until the name has had
//! time to reach iced_layershell: round trips (`wl_display.sync`) on an
//! event queue of its own on iced's connection, so in the same order as
//! iced's requests, which also tell it which names the compositor gave.
//!
//! Nothing here reads the connection: iced's loop does, and the queue's
//! events are handled when the shell looks ([`answered`]), which it does
//! only while a bar waits. At rest nothing here wakes.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_callback::{self, WlCallback},
        wl_output::{self, WlOutput},
        wl_registry::{self, WlRegistry},
    },
};
use wayland_protocols::xdg::xdg_output::zv1::client::{
    zxdg_output_manager_v1::ZxdgOutputManagerV1,
    zxdg_output_v1::{self, ZxdgOutputV1},
};

/// The round trips a bar waits for before it opens.
///
/// iced_layershell learns a new screen's name in three exchanges with the
/// compositor, each started on a turn of its loop once the answer to the
/// one before has come: its registry's global, which it binds; that
/// output's `done`, on which it asks for its xdg-output; and the name. A
/// round trip started once the previous one is answered goes out after an
/// exchange that iced started on the turn that read that answer, and comes
/// back after its answer: two cover each exchange, one the global already
/// sent with the `Bar` message, and one more is a margin.
pub const STEPS: u32 = 6;

/// The round trips after which a name still unknown is let go: the screen
/// went again, or the compositor named one it does not have.
pub const MOST: u32 = 24;

/// What the last round trip asked for says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// Not answered yet, or none asked for.
    Waiting,
    /// Answered: the names of the screens the compositor has given.
    Names(Vec<String>),
    /// The names cannot be known (no connection, no xdg-output):
    /// iced_layershell is left to find the screen as it can.
    Unknowable,
}

/// Starts a round trip, unless one is under way: whether it did.
pub fn ask() -> bool {
    let mut outputs = OUTPUTS.lock().unwrap_or_else(PoisonError::into_inner);
    outputs.as_mut().is_none_or(Outputs::ask)
}

/// The answer to the round trip under way, with what iced's loop has read
/// since the last time.
pub fn answered() -> Answer {
    let mut outputs = OUTPUTS.lock().unwrap_or_else(PoisonError::into_inner);
    match outputs.as_mut() {
        Some(outputs) => outputs.answered(),
        None => Answer::Unknowable,
    }
}

/// Binds the screens on `connection`, the one iced draws on.
pub fn start(connection: &Connection) {
    match Outputs::bind(connection) {
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
    screens: Screens,
    /// The round trip under way: whether it is answered.
    trip: Option<Arc<AtomicBool>>,
}

/// The screens by their global's name, and their xdg-output names once
/// given.
struct Screens {
    manager: ZxdgOutputManagerV1,
    by_global: HashMap<u32, Screen>,
}

struct Screen {
    output: WlOutput,
    xdg: ZxdgOutputV1,
    name: Option<String>,
}

impl Outputs {
    fn bind(connection: &Connection) -> Result<Self, String> {
        let (globals, mut queue) =
            registry_queue_init::<Screens>(connection).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let manager: ZxdgOutputManagerV1 = globals
            .bind(&qh, 1..=3, ())
            .map_err(|e| format!("zxdg_output_manager_v1: {e}"))?;
        let mut screens = Screens {
            manager,
            by_global: HashMap::new(),
        };
        let present: Vec<(u32, u32)> = globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == WlOutput::interface().name)
                .map(|g| (g.name, g.version))
                .collect()
        });
        for (name, version) in present {
            screens.add(globals.registry(), name, version, &qh);
        }
        // Before iced reads the connection: the names of the screens
        // there as the shell starts.
        queue
            .roundtrip(&mut screens)
            .map_err(|e| format!("the screens' names: {e}"))?;
        Ok(Self {
            connection: connection.clone(),
            queue,
            screens,
            trip: None,
        })
    }

    fn ask(&mut self) -> bool {
        if self.trip.is_some() {
            return false;
        }
        let answered = Arc::new(AtomicBool::new(false));
        self.connection
            .display()
            .sync(&self.queue.handle(), answered.clone());
        self.trip = Some(answered);
        // iced's loop flushes what is left when the socket is full.
        match self.connection.flush() {
            Ok(()) => {}
            Err(wayland_client::backend::WaylandError::Io(e))
                if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(err) => tracing::warn!(%err, "the screens' round trip"),
        }
        true
    }

    fn answered(&mut self) -> Answer {
        if let Err(err) = self.queue.dispatch_pending(&mut self.screens) {
            tracing::warn!(%err, "the screens' events");
            return Answer::Unknowable;
        }
        match &self.trip {
            Some(trip) if trip.load(Ordering::Relaxed) => {
                self.trip = None;
                Answer::Names(
                    self.screens
                        .by_global
                        .values()
                        .filter_map(|s| s.name.clone())
                        .collect(),
                )
            }
            _ => Answer::Waiting,
        }
    }
}

impl Screens {
    fn add(&mut self, registry: &WlRegistry, name: u32, version: u32, qh: &QueueHandle<Self>) {
        let output: WlOutput = registry.bind(name, version.min(4), qh, ());
        let xdg = self.manager.get_xdg_output(&output, qh, name);
        self.by_global.insert(
            name,
            Screen {
                output,
                xdg,
                name: None,
            },
        );
    }
}

impl Dispatch<WlRegistry, GlobalListContents> for Screens {
    fn event(
        screens: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } if interface == WlOutput::interface().name => {
                screens.add(registry, name, version, qh);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if let Some(gone) = screens.by_global.remove(&name) {
                    gone.xdg.destroy();
                    if gone.output.version() >= 3 {
                        gone.output.release();
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<WlCallback, Arc<AtomicBool>> for Screens {
    fn event(
        _: &mut Self,
        _: &WlCallback,
        event: wl_callback::Event,
        answered: &Arc<AtomicBool>,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            answered.store(true, Ordering::Relaxed);
        }
    }
}

impl Dispatch<WlOutput, ()> for Screens {
    fn event(
        _: &mut Self,
        _: &WlOutput,
        _: wl_output::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
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
        {
            screen.name = Some(name);
        }
    }
}
