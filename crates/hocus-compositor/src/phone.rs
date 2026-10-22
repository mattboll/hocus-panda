//! The phone (#85): KDE Connect is followed on a worker thread
//! (`hocus_platform::phone`) on the session bus, once the configuration
//! allows it. The column shows the first phone within reach with Ring and
//! a popover; its notifications become toasts, a reply typed into one goes
//! back to the phone. Without KDE Connect nothing shows.

use std::{collections::VecDeque, path::PathBuf, sync::Mutex};

use hocus_api::{Event, PhoneDevice, PhoneInfo};
use hocus_platform::{
    notifications::Notification as DesktopNotification,
    phone::{Device, Notification, Outcome, Phone, PhoneEvent, Presence, Request},
};
use hocus_shell::{column::PhoneLine, toasts::REPLY};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// Phone notifications remembered for their toasts, the latest.
const KEPT: usize = 64;

/// A phone notification shown as a toast.
#[derive(Debug)]
struct Shown {
    toast: u32,
    device: String,
    id: String,
    reply_id: Option<String>,
}

#[derive(Debug, Default)]
pub struct PhoneState {
    /// Where KDE Connect is looked for: the session bus.
    bus: Option<String>,
    news: Option<channel::Sender<PhoneEvent>>,
    phone: Option<Phone>,
    /// Started, or tried: it is not tried again.
    started: bool,
    /// `None` until the worker looked.
    presence: Option<Presence>,
    devices: Vec<Device>,
    shown: VecDeque<Shown>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, session_bus: Option<&str>) {
    let Some(bus) = session_bus else {
        return;
    };
    let (news, received) = channel::channel::<PhoneEvent>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(event) = event {
            app.timed("phone", |app| app.state.phone_event(event));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no phone");
        return;
    }
    state.phone.bus = Some(bus.to_owned());
    state.phone.news = Some(news);
}

impl State {
    fn phone_on(&self) -> bool {
        self.config.phone.enabled
    }

    /// After each dispatch: KDE Connect is followed once the configuration
    /// is read and allows it.
    pub fn refresh_phone(&mut self) {
        if self.phone.started || !self.phone_on() {
            return;
        }
        if self.config_path.is_some() && !self.config_read {
            return;
        }
        let (Some(bus), Some(news)) = (self.phone.bus.clone(), self.phone.news.clone()) else {
            return;
        };
        self.phone.started = true;
        let news = Mutex::new(news);
        let sink = move |event| {
            if let Ok(news) = news.lock() {
                let _ = news.send(event);
            }
        };
        match Phone::start(bus, sink) {
            Ok(phone) => self.phone.phone = Some(phone),
            Err(err) => tracing::warn!(%err, "the phone will not be followed"),
        }
        self.phone_changed();
    }

    fn phone_event(&mut self, event: PhoneEvent) {
        match event {
            PhoneEvent::Presence(presence) => {
                self.phone.presence = Some(presence);
                if presence != Presence::Running {
                    self.phone.devices.clear();
                }
                self.phone_changed();
            }
            PhoneEvent::Devices(devices) => {
                self.phone.devices = devices;
                self.phone_changed();
            }
            PhoneEvent::Posted(n) => self.phone_notification(n, false),
            PhoneEvent::Updated(n) => self.phone_notification(n, true),
            PhoneEvent::Removed { device, id } => {
                self.phone_notifications_gone(|s| s.device == device && s.id == id);
            }
            PhoneEvent::AllRemoved { device } => {
                self.phone_notifications_gone(|s| s.device == device);
            }
            PhoneEvent::Answered {
                device,
                request,
                outcome,
            } => self.phone_answered(&device, &request, outcome),
        }
    }

    fn phone_name(&self, device: &str) -> String {
        self.phone
            .devices
            .iter()
            .find(|d| d.id == device)
            .map_or_else(|| self.shell_lang().phone_line(None), |d| d.name.clone())
    }

    /// A notification as a toast, or a change to the one shown (never
    /// shown again once gone).
    fn phone_notification(&mut self, n: Notification, update: bool) {
        if !self.phone_on() || !self.config.phone.notifications {
            return;
        }
        let known = self
            .phone
            .shown
            .iter()
            .position(|s| s.device == n.device && s.id == n.id);
        let again = match known {
            Some(i) if update => {
                let toast = self.phone.shown[i].toast;
                if !self.toast_shown(toast) {
                    return;
                }
                Some(toast)
            }
            _ if update => return,
            _ => None,
        };
        let lang = self.shell_lang();
        let app = if n.app.is_empty() {
            self.phone_name(&n.device)
        } else {
            format!("{} · {}", n.app, self.phone_name(&n.device))
        };
        let actions = if n.reply_id.is_some() {
            vec![(REPLY.to_owned(), lang.reply().to_owned())]
        } else {
            Vec::new()
        };
        let Some(toast) = self.shell_toast_as(again, &app, &n.title, &n.text, actions) else {
            return;
        };
        if let Some(i) = known {
            self.phone.shown.remove(i);
        }
        self.phone.shown.push_front(Shown {
            toast,
            device: n.device,
            id: n.id,
            reply_id: n.reply_id,
        });
        self.phone.shown.truncate(KEPT);
    }

    fn phone_notifications_gone(&mut self, gone: impl Fn(&Shown) -> bool) {
        let toasts: Vec<u32> = self
            .phone
            .shown
            .iter()
            .filter(|s| gone(s))
            .map(|s| s.toast)
            .collect();
        self.phone.shown.retain(|s| !gone(s));
        for toast in toasts {
            self.close_shell_toast(toast);
        }
    }

    /// A toast of the shell's went: if it was a phone's, it is forgotten.
    pub fn phone_toast_gone(&mut self, toast: u32) {
        self.phone.shown.retain(|s| s.toast != toast);
    }

    /// A reply typed into the toast `toast` goes to the phone.
    pub fn phone_reply(&mut self, toast: u32, text: &str) {
        let Some(i) = self.phone.shown.iter().position(|s| s.toast == toast) else {
            return;
        };
        let Some(shown) = self.phone.shown.remove(i) else {
            return;
        };
        let Some(reply_id) = shown.reply_id else {
            return;
        };
        self.phone_ask(
            Some(shown.device),
            Request::Reply {
                reply_id,
                text: text.to_owned(),
            },
        );
    }

    /// Asks `device`, or the column's phone, for `request`.
    fn phone_ask(&mut self, device: Option<String>, request: Request) -> bool {
        let device = device.or_else(|| self.phone.devices.first().map(|d| d.id.clone()));
        match (&self.phone.phone, device) {
            (Some(phone), Some(device)) if self.phone_on() => {
                tracing::info!(?request, "asking the phone");
                phone.ask(device, request);
                true
            }
            _ => false,
        }
    }

    pub fn ring_phone(&mut self) {
        self.phone_ask(None, Request::Ring);
    }

    pub fn browse_phone(&mut self) {
        self.phone_ask(None, Request::Browse);
    }

    /// What was copied last goes to the phone.
    pub fn send_clipboard_to_phone(&mut self) {
        match self.last_copied() {
            Some(text) => {
                self.phone_ask(None, Request::Clipboard(text));
            }
            None => {
                let lang = self.shell_lang();
                self.shell_toast(lang.nothing_copied(), "");
            }
        }
    }

    /// Files to the column's phone (`hocus msg send-to-phone`); whether
    /// there is one to send them to.
    pub fn send_to_phone(&mut self, paths: Vec<PathBuf>) -> bool {
        self.phone_ask(None, Request::Share(paths))
    }

    fn phone_answered(
        &mut self,
        device: &str,
        request: &Request,
        outcome: Result<Outcome, String>,
    ) {
        let lang = self.shell_lang();
        let name = self.phone_name(device);
        match outcome {
            Ok(Outcome::Mounted(path)) => {
                self.launch("xdg-open", &[path.to_string_lossy().into_owned()]);
            }
            Ok(Outcome::Sent(files)) => self.shell_toast(&lang.sent_to(&name, files), ""),
            Ok(Outcome::Done) if matches!(request, Request::Clipboard(_)) => {
                self.shell_toast(&lang.sent_to(&name, 0), "");
            }
            Ok(Outcome::Done) => {}
            // A phone out of reach is no fault of the session's.
            Err(err) => {
                tracing::info!(%err, ?request, "the phone did not answer");
                self.shell_toast(&lang.phone_failed(&name), &err);
            }
        }
    }

    /// KDE Connect's own copy of a phone notification, held while the
    /// phone's toasts show it: KDE's notifications name the application
    /// and the event in their hints.
    pub fn phone_holds(&self, n: &DesktopNotification) -> bool {
        self.phone_on()
            && self.config.phone.notifications
            && self.phone.presence == Some(Presence::Running)
            && n.kde_event
                .as_ref()
                .is_some_and(|(app, event)| app == "kdeconnect" && event == "notification")
    }

    fn phone_state(&self) -> &'static str {
        if !self.phone_on() {
            return "off";
        }
        self.phone.presence.map_or("looking", Presence::as_str)
    }

    fn phone_changed(&mut self) {
        let (state, phones) = (self.phone_state(), self.phone.devices.len());
        tracing::debug!(state, phones, "phone");
        self.emit(|_| Event::Phone {
            state: state.into(),
            phones,
        });
        self.needs_redraw = true;
    }

    pub fn phone_info(&self) -> PhoneInfo {
        PhoneInfo {
            state: self.phone_state().into(),
            phones: if self.phone_on() {
                self.phone
                    .devices
                    .iter()
                    .map(|d| PhoneDevice {
                        id: d.id.clone(),
                        name: d.name.clone(),
                        battery: d.battery,
                        charging: d.charging,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }

    /// The column's line: the first phone within reach.
    pub fn phone_line(&self) -> Option<PhoneLine> {
        if !self.phone_on() {
            return None;
        }
        self.phone.devices.first().map(|d| PhoneLine {
            name: d.name.clone(),
            battery: d.battery,
            charging: d.charging,
        })
    }
}
