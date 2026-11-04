//! ext-session-lock-v1: another locker (swaylock, gtklock) locks the
//! session with its own surfaces and its own password check. The rules
//! are this lock's: nothing of the session drawn nor given input, the
//! allowed shortcuts only, captures showing the lock.
//!
//! It cannot be gone round:
//!
//! - Locked already (by the desktop or by another locker), a new lock is
//!   refused (`finished`).
//! - Only the holder's `unlock_and_destroy` opens it. smithay calls
//!   [`SessionLockHandler::unlock`] for any lock object's, the refused
//!   ones' too (after a protocol error), without saying whose: so the
//!   unlock is taken after the turn, and only when the holder's lock
//!   object is gone while its client is still connected.
//! - The holder going without unlocking (a crash) leaves the session
//!   locked, under the desktop's own lock (its shell, PAM), as the
//!   protocol asks.
//! - `locked` is sent once every screen drew the lock.

use smithay::{
    delegate_session_lock,
    output::Output,
    reexports::{
        wayland_protocols::ext::session_lock::v1::server::ext_session_lock_v1::ExtSessionLockV1,
        wayland_server::{Resource, backend::ClientId, protocol::wl_output::WlOutput},
    },
    utils::Size,
    wayland::session_lock::{
        LockSurface, SessionLockHandler, SessionLockManagerState, SessionLocker,
    },
};

use super::Holder as LockHolder;
use crate::State;

/// An external locker holding the lock.
#[derive(Debug)]
pub struct Holder {
    lock: ExtSessionLockV1,
    client: Option<ClientId>,
    /// Its `locked`, sent once every screen drew the lock.
    confirm: Option<SessionLocker>,
    /// Its surfaces, by screen.
    surfaces: Vec<(String, LockSurface)>,
    /// An unlock was asked this turn, to judge after it.
    unlock_asked: bool,
}

impl SessionLockHandler for State {
    fn lock_state(&mut self) -> &mut SessionLockManagerState {
        &mut self.lock.manager
    }

    fn lock(&mut self, confirmation: SessionLocker) {
        if self.locked() {
            tracing::info!("an ext-session-lock client asks to lock, locked already: refused");
            // Dropped: `finished`.
            return;
        }
        let lock = confirmation.ext_session_lock().clone();
        let client = lock.client().map(|c| c.id());
        self.lock_with(LockHolder::External(Holder {
            lock,
            client,
            confirm: Some(confirmation),
            surfaces: Vec::new(),
            unlock_asked: false,
        }));
        // Already drawn locked (no screen): told at once.
        self.lock_after_turn();
    }

    fn unlock(&mut self) {
        // Whose, judged after the turn (see the module).
        if let Some(holder) = self.external_holder() {
            holder.unlock_asked = true;
        } else {
            tracing::warn!("an ext-session-lock unlock, no external locker holding: ignored");
        }
    }

    fn new_surface(&mut self, surface: LockSurface, output: WlOutput) {
        let Some(output) = Output::from_resource(&output) else {
            return;
        };
        let size = self.lock_surface_size(&output);
        let owner = surface.wl_surface().client().map(|c| c.id());
        let Some(holder) = self.external_holder() else {
            tracing::info!("a lock surface without its lock holding: not shown");
            return;
        };
        if owner != holder.client {
            tracing::warn!("a lock surface of another client than the locker's: not shown");
            return;
        }
        surface.with_pending_state(|state| state.size = size);
        holder.surfaces.retain(|(name, _)| *name != output.name());
        holder.surfaces.push((output.name(), surface));
        self.lock_keyboard();
        self.redraw = true;
    }
}
delegate_session_lock!(State);

impl State {
    fn external_holder(&mut self) -> Option<&mut Holder> {
        match &mut self.lock.locked_mut()?.holder {
            LockHolder::External(holder) => Some(holder),
            LockHolder::Own => None,
        }
    }

    fn external_holder_ref(&self) -> Option<&Holder> {
        match &self.lock.locked_state()?.holder {
            LockHolder::External(holder) => Some(holder),
            LockHolder::Own => None,
        }
    }

    /// A lock surface's size on `output`: the screen's, logical.
    fn lock_surface_size(&self, output: &Output) -> Option<Size<u32, smithay::utils::Logical>> {
        let geometry = self.space.output_geometry(output)?;
        Some(Size::from((
            geometry.size.w.max(1) as u32,
            geometry.size.h.max(1) as u32,
        )))
    }

    /// The external locker's surface on `output`, if it holds the lock.
    pub(crate) fn external_surface_on(
        &self,
        output: &Output,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        let holder = self.external_holder_ref()?;
        holder
            .surfaces
            .iter()
            .find(|(name, s)| *name == output.name() && s.alive())
            .map(|(_, s)| s.wl_surface().clone())
    }

    /// Whether `surface` is the external locker's.
    pub(crate) fn external_owns(
        &self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) -> bool {
        self.external_holder_ref()
            .is_some_and(|h| h.surfaces.iter().any(|(_, s)| s.wl_surface() == surface))
    }

    /// The surface that gets the keyboard with an external locker: its
    /// surface under the pointer, else its first.
    pub(crate) fn external_keyboard_surface(
        &self,
    ) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
        let holder = self.external_holder_ref()?;
        let under = self.seat.get_pointer().and_then(|p| {
            self.space
                .output_under(p.current_location())
                .next()
                .cloned()
        });
        under
            .and_then(|o| self.external_surface_on(&o))
            .or_else(|| {
                holder
                    .surfaces
                    .iter()
                    .find(|(_, s)| s.alive())
                    .map(|(_, s)| s.wl_surface().clone())
            })
    }

    /// Every screen drew the lock: the external locker is told `locked`.
    pub(crate) fn external_drawn(&mut self) {
        if let Some(holder) = self.external_holder()
            && let Some(confirm) = holder.confirm.take()
        {
            tracing::info!("the ext-session-lock client is told the session is locked");
            confirm.lock();
        }
    }

    /// After each turn: an unlock asked is the holder's only when its lock
    /// object went with its client still there; the holder gone without
    /// unlocking leaves the desktop's own lock holding.
    pub(crate) fn external_after_turn(&mut self) {
        let display = self.display_handle.clone();
        let Some(holder) = self.external_holder() else {
            return;
        };
        let lock_alive = holder.lock.is_alive();
        let client_alive = holder
            .client
            .as_ref()
            .is_some_and(|c| display.backend_handle().get_client_data(c.clone()).is_ok());
        let asked = std::mem::take(&mut holder.unlock_asked);
        holder.surfaces.retain(|(_, s)| s.alive());
        match (asked, lock_alive, client_alive) {
            (true, false, true) => {
                tracing::info!("the ext-session-lock client unlocks");
                self.open_lock();
            }
            (true, true, _) => {
                tracing::warn!("an unlock not from the lock's holder: refused");
            }
            (_, false, _) => {
                tracing::warn!(
                    "the ext-session-lock client went without unlocking: the desktop's own lock holds"
                );
                if let Some(locked) = self.lock.locked_mut() {
                    locked.holder = LockHolder::Own;
                }
                self.lock_keyboard();
                self.tell_lock();
                self.redraw = true;
            }
            (false, true, _) => {}
        }
    }

    /// The screens changed while an external locker holds: its surfaces
    /// get their screens' sizes again.
    pub(crate) fn external_screens_changed(&mut self) {
        let sizes: Vec<(String, Option<Size<u32, smithay::utils::Logical>>)> = self
            .space
            .outputs()
            .map(|o| (o.name(), self.lock_surface_size(o)))
            .collect();
        let Some(holder) = self.external_holder() else {
            return;
        };
        for (name, surface) in &holder.surfaces {
            if let Some((_, size)) = sizes.iter().find(|(n, _)| n == name) {
                surface.with_pending_state(|state| state.size = *size);
                surface.send_configure();
            }
        }
    }
}
