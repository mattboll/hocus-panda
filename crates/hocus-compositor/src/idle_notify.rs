//! ext-idle-notify for third-party idle tools: a notification says
//! "idled" once nobody used the session for its timeout, and "resumed" at
//! the next input. Version 2's input notifications ignore what holds idle
//! off (inhibitors, fullscreen windows); the others honour it. Deadlines
//! are checked after each dispatch, with the lock's timer to wake the loop.

use std::{sync::Mutex, time::Duration};

use smithay::reexports::{
    wayland_protocols::ext::idle_notify::v1::server::{
        ext_idle_notification_v1::{self, ExtIdleNotificationV1},
        ext_idle_notifier_v1::{self, ExtIdleNotifierV1},
    },
    wayland_server::{Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource},
};

use crate::State;

#[derive(Debug)]
pub struct Notification {
    timeout: Duration,
    /// Counts input alone, not what holds idle off.
    input_only: bool,
    idled: Mutex<bool>,
}

pub fn init(dh: &DisplayHandle) {
    dh.create_global::<State, ExtIdleNotifierV1, _>(2, ());
}

impl GlobalDispatch<ExtIdleNotifierV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ExtIdleNotifierV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
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
        data: &mut DataInit<'_, Self>,
    ) {
        let (id, timeout, input_only) = match request {
            ext_idle_notifier_v1::Request::GetIdleNotification { id, timeout, .. } => {
                (id, timeout, false)
            }
            ext_idle_notifier_v1::Request::GetInputIdleNotification { id, timeout, .. } => {
                (id, timeout, true)
            }
            _ => return,
        };
        let notification = data.init(
            id,
            Notification {
                timeout: Duration::from_millis(u64::from(timeout)),
                input_only,
                idled: Mutex::new(false),
            },
        );
        state.idle_notifications.push(notification);
    }
}

impl Dispatch<ExtIdleNotificationV1, Notification> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ExtIdleNotificationV1,
        _: ext_idle_notification_v1::Request,
        _: &Notification,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
    }

    fn destroyed(
        state: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        resource: &ExtIdleNotificationV1,
        _: &Notification,
    ) {
        state.idle_notifications.retain(|n| n != resource);
    }
}

impl State {
    /// Input: every notification that said idled says resumed.
    pub fn idle_resumed(&mut self) {
        for n in &self.idle_notifications {
            if let Some(data) = n.data::<Notification>()
                && let Ok(mut idled) = data.idled.lock()
                && *idled
            {
                *idled = false;
                n.resumed();
            }
        }
    }

    /// After each dispatch: the notifications whose timeout passed say so.
    pub fn refresh_idle_notifications(&mut self) {
        let now = self.clock.now();
        let (activity, input) = self.idle_since();
        for n in &self.idle_notifications {
            let Some(data) = n.data::<Notification>() else {
                continue;
            };
            let since = if data.input_only { input } else { activity };
            if now.saturating_sub(since) >= data.timeout
                && let Ok(mut idled) = data.idled.lock()
                && !*idled
            {
                *idled = true;
                n.idled();
            }
        }
    }

    /// When the next notification will idle, if any is waiting.
    pub fn next_idle_notification(&self) -> Option<Duration> {
        let (activity, input) = self.idle_since();
        self.idle_notifications
            .iter()
            .filter_map(|n| n.data::<Notification>())
            .filter(|d| d.idled.lock().is_ok_and(|i| !*i))
            .map(|d| if d.input_only { input } else { activity } + d.timeout)
            .min()
    }
}
