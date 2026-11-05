//! Raising the window when the Settings are launched again: the second
//! launch hands its activation token (`XDG_ACTIVATION_TOKEN`, which the
//! compositor gives the programs it starts for the user) to the first,
//! which asks for the focus with it (xdg-activation). winit has no focus
//! request on Wayland, so this speaks the protocol on the connection winit
//! drew the window on, given by its raw handles: the FFI boundary of this
//! crate.
//!
//! The connection's registry is read on a thread of its own, on a queue of
//! its own: the window's thread never waits for the compositor.
//!
//! The same way, while a shortcut is recorded (6.8), the window asks the
//! compositor for every key, its own shortcuts with them
//! (keyboard-shortcuts-inhibit), and lets them go once it is done.

#![allow(unsafe_code)]

use iced::window::raw_window_handle::{RawDisplayHandle, RawWindowHandle};
use wayland_backend::client::{Backend, ObjectId};
use wayland_client::protocol::wl_seat::WlSeat;
use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{wl_registry, wl_surface::WlSurface},
};
use wayland_protocols::{
    wp::keyboard_shortcuts_inhibit::zv1::client::{
        zwp_keyboard_shortcuts_inhibit_manager_v1::ZwpKeyboardShortcutsInhibitManagerV1,
        zwp_keyboard_shortcuts_inhibitor_v1::ZwpKeyboardShortcutsInhibitorV1,
    },
    xdg::activation::v1::client::{
        xdg_activation_token_v1::XdgActivationTokenV1, xdg_activation_v1::XdgActivationV1,
    },
};

/// The window's connection and surface, taken from its handles.
#[derive(Debug)]
pub struct Surface {
    connection: Connection,
    surface: WlSurface,
}

impl Surface {
    /// From the window's raw handles, when it is a Wayland window.
    pub fn of(window: &dyn iced::window::Window) -> Option<Self> {
        let display = window.display_handle().ok()?.as_raw();
        let handle = window.window_handle().ok()?.as_raw();
        let (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(handle)) =
            (display, handle)
        else {
            return None;
        };
        // SAFETY: the display is winit's, alive as long as the event loop
        // runs, which outlives the window and this program's use of it: the
        // connection made from it is only used while the window is open,
        // and a foreign display is never closed by its `Backend`.
        let backend = unsafe { Backend::from_foreign_display(display.display.as_ptr().cast()) };
        let connection = Connection::from_backend(backend);
        // SAFETY: the pointer is the window's live `wl_surface` proxy, from
        // its handle; it stays valid while the window is open, and the
        // request below is sent right away.
        let id =
            unsafe { ObjectId::from_ptr(WlSurface::interface(), handle.surface.as_ptr().cast()) }
                .ok()?;
        let surface = WlSurface::from_id(&connection, id).ok()?;
        Some(Self {
            connection,
            surface,
        })
    }

    /// Asks the compositor for the focus with `token`, from a thread.
    pub fn activate(self, token: String) {
        let started = std::thread::Builder::new()
            .name("settings' activation".to_owned())
            .spawn(move || {
                if let Err(err) = self.send(&token) {
                    tracing::warn!(%err, "the window could not ask for the focus");
                }
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the window could not ask for the focus");
        }
    }

    /// A thread that takes the desktop's shortcuts while `asks` says
    /// `true`, and gives them back on `false`; it ends with the channel.
    pub fn inhibit_shortcuts(self, asks: std::sync::mpsc::Receiver<bool>) {
        let started = std::thread::Builder::new()
            .name("settings' shortcuts".to_owned())
            .spawn(move || {
                if let Err(err) = self.inhibit(&asks) {
                    tracing::warn!(%err, "the desktop's shortcuts are not taken while recording");
                }
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the desktop's shortcuts are not taken while recording");
        }
    }

    fn inhibit(&self, asks: &std::sync::mpsc::Receiver<bool>) -> Result<(), String> {
        let (globals, mut queue) =
            registry_queue_init::<Activation>(&self.connection).map_err(|e| e.to_string())?;
        let manager: ZwpKeyboardShortcutsInhibitManagerV1 = globals
            .bind(&queue.handle(), 1..=1, ())
            .map_err(|e| e.to_string())?;
        let seat: WlSeat = globals
            .bind(&queue.handle(), 1..=7, ())
            .map_err(|e| e.to_string())?;
        let mut held: Option<ZwpKeyboardShortcutsInhibitorV1> = None;
        for take in asks {
            match (take, held.take()) {
                (true, None) => {
                    held =
                        Some(manager.inhibit_shortcuts(&self.surface, &seat, &queue.handle(), ()));
                    tracing::info!("the desktop's shortcuts asked for, while recording");
                }
                (true, Some(inhibitor)) => held = Some(inhibitor),
                (false, Some(inhibitor)) => {
                    inhibitor.destroy();
                    tracing::info!("the desktop's shortcuts given back");
                }
                (false, None) => {}
            }
            queue
                .roundtrip(&mut Activation)
                .map_err(|e| e.to_string())?;
        }
        if let Some(inhibitor) = held {
            inhibitor.destroy();
            let _ = queue.roundtrip(&mut Activation);
        }
        Ok(())
    }

    fn send(&self, token: &str) -> Result<(), String> {
        let (globals, mut queue) =
            registry_queue_init::<Activation>(&self.connection).map_err(|e| e.to_string())?;
        let activation: XdgActivationV1 = globals
            .bind(&queue.handle(), 1..=1, ())
            .map_err(|e| e.to_string())?;
        activation.activate(token.to_owned(), &self.surface);
        activation.destroy();
        queue
            .roundtrip(&mut Activation)
            .map_err(|e| e.to_string())?;
        tracing::info!("the window asked for the focus with the second launch's token");
        Ok(())
    }
}

/// The queue's state: nothing to keep.
struct Activation;

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Activation {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationV1, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &XdgActivationV1,
        _: <XdgActivationV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitManagerV1, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitManagerV1,
        _: <ZwpKeyboardShortcutsInhibitManagerV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwpKeyboardShortcutsInhibitorV1, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &ZwpKeyboardShortcutsInhibitorV1,
        _: <ZwpKeyboardShortcutsInhibitorV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<XdgActivationTokenV1, ()> for Activation {
    fn event(
        _: &mut Self,
        _: &XdgActivationTokenV1,
        _: <XdgActivationTokenV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
