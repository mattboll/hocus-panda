//! `ext_idle_notifier_v1`, version 2: the clients' idle notifications
//! (screen lockers, presence in chat applications), each with its own
//! timeout, on the session's single idle timer.
//!
//! smithay has its own, which removes and puts back a timer per
//! notification at every input event; here input only notes its time (see
//! the module above), so that a mouse moving costs nothing more with
//! notifications than without. The protocol's rules kept: a notification
//! counts from its creation or the last activity, whichever is later;
//! `idled` and `resumed` alternate, `resumed` only after `idled`; one made
//! with `get_idle_notification` is not made idle while something holds
//! idleness off, one made with `get_input_idle_notification` counts input
//! alone.

use std::time::Duration;

use smithay::reexports::{
    wayland_protocols::ext::idle_notify::v1::server::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
    },
    wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, backend::ClientId,
    },
};

use crate::State;

/// A client's notification.
#[derive(Debug)]
pub struct Notification {
    resource: ExtIdleNotificationV1,
    timeout: Duration,
    /// Made with `get_input_idle_notification`: input alone counts, holds
    /// do not apply.
    pub input: bool,
    /// When it was made.
    since: Duration,
    /// `idled` was sent, and no `resumed` since.
    pub idled: bool,
}

impl Notification {
    /// When it becomes idle, given the last input and where the session's
    /// countdown runs from.
    pub fn deadline(&self, input_at: Duration, counting_from: Duration) -> Duration {
        let from = if self.input { input_at } else { counting_from };
        from.max(self.since) + self.timeout
    }

    pub fn idle(&mut self) {
        self.resource.idled();
        self.idled = true;
    }

    pub fn resume(&mut self) {
        self.resource.resumed();
        self.idled = false;
    }
}

/// The notifier's global.
pub fn global(dh: &DisplayHandle) {
    dh.create_global::<State, ExtIdleNotifierV1, ()>(2, ());
}

impl GlobalDispatch<ExtIdleNotifierV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtIdleNotifierV1>,
        _: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ExtIdleNotifierV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ExtIdleNotifierV1,
        request: ext_idle_notifier_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        // The seat is the only one.
        let (id, timeout, input) = match request {
            ext_idle_notifier_v1::Request::GetIdleNotification { id, timeout, .. } => {
                (id, timeout, false)
            }
            ext_idle_notifier_v1::Request::GetInputIdleNotification { id, timeout, .. } => {
                (id, timeout, true)
            }
            _ => return,
        };
        let resource = data_init.init(id, ());
        let since = state.now();
        state.idle.notifications.push(Notification {
            resource,
            timeout: Duration::from_millis(u64::from(timeout)),
            input,
            since,
            idled: false,
        });
        tracing::debug!(timeout, input, "an idle notification");
        state.idle_check();
    }
}

impl Dispatch<ExtIdleNotificationV1, ()> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtIdleNotificationV1,
        _: ext_idle_notification_v1::Request,
        _: &(),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // `destroy` alone: `destroyed` below forgets it.
    }

    fn destroyed(state: &mut Self, _: ClientId, resource: &ExtIdleNotificationV1, _: &()) {
        state.idle.notifications.retain(|n| n.resource != *resource);
    }
}
