//! The Hardware pages (#67): Sound, Network, Bluetooth, Power and Lock. No
//! board draws them: they keep board R2's template, captioned sections of
//! choices and switch cards, and board R4's rows for the lists (the
//! networks in reach, the Bluetooth devices). What the system's services
//! hold (the volume, the radios, the power profile) is changed through the
//! compositor, as quick settings change it; the delays are the
//! configuration's. The networks and the devices are the settings' own to
//! ask, on workers, while their page shows.

use hocus_api::HardwareChange;
use hocus_core::{
    Action,
    config::{PowerSource, Sleep},
};
use hocus_platform::{
    bluetooth::{Bluetooth, Device},
    wifi::{JoinError, Network, Security, Wifi},
};

use super::*;

/// Dimming delays offered, seconds; 0 is never.
const DIM: [u32; 5] = [60, 120, 300, 600, 0];
/// Sleeping delays offered.
const SLEEP: [u32; 5] = [900, 1800, 3600, 7200, 0];
/// How long the curtain shows before the lock.
const CURTAIN: [u32; 4] = [10, 30, 60, 120];

/// What a service's list holds, as a worker last read it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Listed<T> {
    /// Not read yet.
    #[default]
    Unknown,
    /// The service is not there, and why.
    Absent(String),
    Known(T),
}

/// A password, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(pub String);

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// What the Network page asks NetworkManager.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WifiAction {
    /// Joins a network: through its saved connection without a password.
    Join {
        ssid: String,
        password: Option<Secret>,
    },
    /// Leaves the network joined, named.
    Disconnect(String),
    Forget(String),
}

impl WifiAction {
    fn ssid(&self) -> &str {
        match self {
            Self::Join { ssid, .. } | Self::Disconnect(ssid) | Self::Forget(ssid) => ssid,
        }
    }
}

/// What the Bluetooth page asks BlueZ, of a device by its object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothAction {
    Pair(String),
    Connect(String),
    Disconnect(String),
    Forget(String),
}

impl BluetoothAction {
    fn device(&self) -> &str {
        match self {
            Self::Pair(d) | Self::Connect(d) | Self::Disconnect(d) | Self::Forget(d) => d,
        }
    }
}

/// What a pairing shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pairing {
    /// A passkey the device shows too: confirmed or not.
    Confirm(u32),
    /// A code to type on the device.
    Show(String),
}

/// The Hardware pages between builds: the volume slider and the volume it
/// last asked for; the networks and devices as last read, and what was
/// asked of them.
#[derive(Debug, Default)]
pub(super) struct HardwarePage {
    pub(super) volume: Option<(NodeId, u8)>,
    pub(super) wifi: Listed<Wifi>,
    pub(super) bluetooth: Listed<Bluetooth>,
    /// The network whose password is asked, its field, and what is typed.
    pub(super) password: Option<(String, Option<NodeId>, String)>,
    /// The network asked something, and what it says meanwhile.
    pub(super) network_busy: Option<(String, words::Words)>,
    /// A network that was not joined, and why.
    pub(super) network_failed: Option<(String, String)>,
    /// The device asked something, and what it says meanwhile.
    pub(super) device_busy: Option<(String, words::Words)>,
    pub(super) device_failed: Option<(String, String)>,
    /// What the pairing under way shows.
    pub(super) pairing: Option<Pairing>,
}

impl HardwarePage {
    /// Leaving a page: what was being typed or asked goes.
    pub(super) fn leave(&mut self) {
        self.password = None;
        self.network_failed = None;
        self.device_failed = None;
        self.pairing = None;
    }

    pub(super) fn password_field(&self, id: Option<NodeId>) -> bool {
        id.is_some() && self.password.as_ref().is_some_and(|(_, f, _)| *f == id)
    }
}

/// A network's strength as four bars, those reached in the text's colour.
struct Bars(u8);

impl Draw for Bars {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let lit = match self.0 {
            0..25 => 1,
            25..50 => 2,
            50..75 => 3,
            _ => 4,
        };
        let (w, gap, tall) = (3.0, 2.0, 16.0);
        let x = rect.x + (rect.w - (4.0 * w + 3.0 * gap)) / 2.0;
        let bottom = rect.y + (rect.h + tall) / 2.0;
        for i in 0..4 {
            let h = 4.0 + 4.0 * i as f32;
            let color = if i < lit {
                theme.colors.text
            } else {
                theme.colors.border.over(theme.colors.surface_hover)
            };
            let bar = Rect::new(x + i as f32 * (w + gap), bottom - h, w, h);
            canvas.fill_rect(bar, 1.0, color);
        }
    }

    fn size(&self) -> Size {
        Size::new(20.0, 20.0)
    }
}

/// A device's icon, by what it is.
fn device_icon(kind: hocus_platform::bluetooth::Kind) -> &'static str {
    use hocus_platform::bluetooth::Kind;
    match kind {
        Kind::Headphones => "M4 15v-3a8 8 0 0 1 16 0v3M4 15h3v5H4zM17 15h3v5h-3z",
        Kind::Speaker => "M6 3h12v18H6zM12 17a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM12 7h0",
        Kind::Keyboard => icons::KEYBOARD,
        Kind::Mouse => "M7 9a5 5 0 0 1 10 0v6a5 5 0 0 1-10 0zM12 4v5",
        Kind::Gamepad => {
            "M6 8h12a4 4 0 0 1 4 4v1a4 4 0 0 1-7 3H9a4 4 0 0 1-7-3v-1a4 4 0 0 1 4-4zM7 11v3M5.5 12.5h3M16 12h0M18 14h0"
        }
        Kind::Phone => "M7 3h10v18H7zM11 18h2",
        Kind::Computer => "M3 5h18v11H3zM8 20h8M12 16v4",
        Kind::Other => "M7 7l10 10-5 5V2l5 5L7 17",
    }
}

/// `offered`, with `current` among them when it is not (a value typed in
/// the file), in order; never last.
fn with_current(offered: &[u32], current: u32) -> Vec<u32> {
    let mut all = offered.to_vec();
    if !all.contains(&current) {
        all.push(current);
    }
    all.sort_by_key(|s| if *s == 0 { u32::MAX } else { *s });
    all
}

impl SettingsView {
    /// The volume dragged to `value` (0 to 1): the percent to ask for, when
    /// it is new.
    pub(super) fn volume_moved(&mut self, value: f32) -> Option<Outcome> {
        let percent = ((value * 100.0).round() as u8).min(100) / 5 * 5;
        let (_, last) = self.hardware.volume.as_mut()?;
        if *last == percent {
            return None;
        }
        *last = percent;
        Some(Outcome::Hardware(HardwareChange::Volume { percent }))
    }

    /// Until the services answered, a word on why nothing shows.
    fn unknown(&mut self, theme: &Theme, body: NodeId) -> bool {
        let known = self.ctx.hardware.known;
        if !known {
            let section = self.add(body, fill_width(column(8.0)), Kind::Box(Look::default()));
            self.note(theme, section, words::NO_SERVICE);
        }
        !known
    }

    /// A line naming a device: its name, and what it is.
    fn device_line(&mut self, theme: &Theme, parent: NodeId, what: words::Words, name: &str) {
        let lang = self.lang;
        let line = self.add(parent, fill_width(row(12.0)), Kind::Box(Look::default()));
        self.add(
            line,
            Style {
                width: Length::Px(170.0),
                ..Style::default()
            },
            Kind::text_line(what.get(lang), theme.text.body.with_color(Paint::Muted)),
        );
        let name = if name.is_empty() {
            words::NO_DEVICE.get(lang)
        } else {
            name
        };
        self.line(line, name, theme.text.body.with_weight(600));
    }

    /// Sound: the volume, muted or not, and the devices.
    pub(super) fn sound(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let unknown = self.unknown(theme, body);
        let h = self.ctx.hardware.clone();

        let section = self.section(theme, body, words::OUTPUT);
        if !unknown {
            let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
            self.add(
                line,
                Style {
                    width: Length::Px(170.0),
                    ..Style::default()
                },
                Kind::text_line(words::VOLUME.get(lang), theme.text.body),
            );
            let volume = h.volume.min(100);
            let slider = self.add(
                line,
                Style {
                    width: Length::Fill,
                    height: Length::Px(32.0),
                    ..Style::default()
                },
                Kind::Slider {
                    value: f32::from(volume) / 100.0,
                    step: 0.05,
                    name: words::VOLUME.get(lang).to_owned(),
                },
            );
            self.hardware.volume = Some((slider, volume));
            self.add(
                line,
                Style {
                    width: Length::Px(56.0),
                    ..Style::default()
                },
                Kind::text_line(
                    words::percent(u32::from(h.volume), lang),
                    theme.text.body.with_color(Paint::Muted),
                ),
            );
        }
        let mute = (!unknown).then(|| Target::Hardware(HardwareChange::Muted { on: !h.muted }));
        let sub = words::MUTE_SUB.get(lang);
        self.switch_card_to(theme, section, (words::MUTE, sub), h.muted, mute, None);
        self.device_line(theme, section, words::OUTPUT_DEVICE, &h.output_device);
        self.highlighted(section, &Setting::Volume, Look::default());

        let section = self.section(theme, body, words::INPUT);
        self.device_line(theme, section, words::INPUT_DEVICE, &h.input_device);
        self.note(theme, section, words::SOUND_NOTE);
        self.keys_line(theme, section, &[Action::QuickSettings]);
    }

    /// Network: Wi-Fi and airplane mode, and the networks in reach.
    pub(super) fn network(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let unknown = self.unknown(theme, body);
        let h = self.ctx.hardware.clone();

        let section = self.section(theme, body, words::WIRELESS);
        let switches = self.switches(section);
        let joined = match (&h.network, h.wifi) {
            (_, false) => words::WIFI_SUB_OFF.get(lang).to_owned(),
            (Some(name), true) => words::joined(name, lang),
            (None, true) => words::NOT_CONNECTED.get(lang).to_owned(),
        };
        let wifi = (!unknown).then(|| Target::Hardware(HardwareChange::Wifi { on: !h.wifi }));
        self.switch_card_to(
            theme,
            switches,
            (words::WIFI, &joined),
            h.wifi && !unknown,
            wifi,
            Some(&Setting::Wifi),
        );
        let airplane =
            (!unknown).then(|| Target::Hardware(HardwareChange::Airplane { on: !h.airplane }));
        let sub = words::AIRPLANE_SUB.get(lang);
        self.switch_card_to(
            theme,
            switches,
            (words::AIRPLANE, sub),
            h.airplane,
            airplane,
            Some(&Setting::Airplane),
        );

        let section = self.section(theme, body, words::NETWORKS);
        // The compositor hears the radio go off first: the list follows.
        let radio_off = h.known && !h.wifi;
        let networks = match &self.hardware.wifi {
            _ if radio_off => Err(words::WIFI_OFF_LIST),
            Listed::Unknown => Err(words::LOOKING_NETWORKS),
            Listed::Absent(_) => Err(words::NO_NETWORKMANAGER),
            Listed::Known(Wifi::Off) => Err(words::WIFI_OFF_LIST),
            Listed::Known(Wifi::NoDevice) => Err(words::NO_WIFI_DEVICE),
            Listed::Known(Wifi::Networks(list)) if list.is_empty() => Err(words::NO_NETWORK),
            Listed::Known(Wifi::Networks(list)) => Ok(list.clone()),
        };
        match networks {
            Ok(list) => {
                for network in &list {
                    self.network_row(theme, section, network);
                }
            }
            Err(said) => self.note(theme, section, said),
        }
        self.note(theme, section, words::NETWORK_NOTE);
    }

    /// A row of the list, after board R4's: the current one tinted, a name
    /// with a line under it, buttons at the end. Its line and its box.
    fn list_row(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        current: bool,
        icon: Kind,
        (name, sub, sub_paint): (&str, &str, Paint),
    ) -> (NodeId, NodeId) {
        let row_box = self.add(
            parent,
            Style {
                padding: Insets::symmetric(12.0, 14.0),
                ..fill_width(column(10.0))
            },
            Kind::Box(Look {
                background: Some(if current {
                    Paint::Rgba(theme.colors.accent.with_alpha(26))
                } else {
                    Paint::SurfaceHover
                }),
                radius: 14.0,
                ..Look::default()
            }),
        );
        let line = self.add(row_box, fill_width(row(12.0)), Kind::Box(Look::default()));
        // As tall as the buttons: rows with and without them alike.
        self.add(
            line,
            Style {
                width: Length::Px(22.0),
                height: Length::Px(40.0),
                ..Style::default()
            },
            icon,
        );
        let words_box = self.add(line, fill_width(column(1.0)), Kind::Box(Look::default()));
        self.line(words_box, name, theme.text.title.with_weight(700));
        // Wrapped rather than cut: the battery comes last.
        self.add(
            words_box,
            fill_width(Style::default()),
            Kind::text(
                sub,
                theme.text.caption.with_weight(500).with_color(sub_paint),
            ),
        );
        (line, row_box)
    }

    /// A button at the end of a row, named with what it acts on.
    fn row_button(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        (verb, on): (words::Words, &str),
        kind: ButtonKind,
        target: Option<Target>,
    ) -> NodeId {
        let lang = self.lang;
        let mut button = Button::new(verb.get(lang), kind, theme);
        button.enabled = target.is_some();
        let b = self.add(
            parent,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        self.tree.set_name(b, &words::acting(verb, on, lang));
        if let Some(target) = target {
            self.targets.insert(b, target);
        }
        b
    }

    /// A network in reach: its strength, name, what it is, and what can be
    /// done with it; its password asked under it.
    fn network_row(&mut self, theme: &Theme, parent: NodeId, n: &Network) {
        let lang = self.lang;
        let busy = self
            .hardware
            .network_busy
            .as_ref()
            .filter(|(s, _)| *s == n.ssid)
            .map(|(_, said)| *said);
        let failed = self
            .hardware
            .network_failed
            .as_ref()
            .filter(|(s, _)| *s == n.ssid)
            .map(|(_, why)| why.clone());
        let security = match n.security {
            Security::Open => words::OPEN_NETWORK,
            Security::Personal => words::SECURED,
            Security::Wpa3 => words::SECURED_WPA3,
            Security::Other => words::ACCOUNT_NETWORK,
        };
        let (sub, paint) = match (busy, failed) {
            (Some(said), _) => (said.get(lang).to_owned(), Paint::Muted),
            (None, Some(why)) => (why, Paint::Waiting),
            (None, None) => {
                let mut said = Vec::new();
                if n.connected {
                    said.push(words::CONNECTED.get(lang).to_owned());
                } else if n.saved {
                    said.push(words::SAVED_NETWORK.get(lang).to_owned());
                }
                said.push(security.get(lang).to_owned());
                said.push(words::percent(u32::from(n.strength), lang));
                (said.join(" · "), Paint::Muted)
            }
        };
        let icon = Kind::Custom {
            draw: Box::new(Bars(n.strength)),
            label: None,
        };
        let (line, row_box) =
            self.list_row(theme, parent, n.connected, icon, (&n.ssid, &sub, paint));
        let free = busy.is_none();
        let act = |action: WifiAction| free.then_some(Target::Wifi(action));
        let asked = self
            .hardware
            .password
            .as_ref()
            .filter(|(s, _, _)| *s == n.ssid)
            .map(|(_, _, typed)| typed.clone());
        if n.connected {
            let target = act(WifiAction::Disconnect(n.ssid.clone()));
            self.row_button(
                theme,
                line,
                (words::DISCONNECT, &n.ssid),
                ButtonKind::Plain,
                target,
            );
        } else if asked.is_none() && (n.saved || n.security != Security::Other) {
            let target = if n.saved || !n.security.needs_password() {
                act(WifiAction::Join {
                    ssid: n.ssid.clone(),
                    password: None,
                })
            } else {
                free.then(|| Target::AskPassword(n.ssid.clone()))
            };
            self.row_button(
                theme,
                line,
                (words::JOIN, &n.ssid),
                ButtonKind::Plain,
                target,
            );
        }
        if n.saved {
            let target = act(WifiAction::Forget(n.ssid.clone()));
            self.row_button(
                theme,
                line,
                (words::FORGET, &n.ssid),
                ButtonKind::Plain,
                target,
            );
        }
        // Its own Join is the form's while the password is asked.
        if let Some(typed) = asked {
            self.password_form(theme, row_box, &n.ssid, &typed);
        }
    }

    /// The password of a secured network: typed, then joined with.
    fn password_form(&mut self, theme: &Theme, parent: NodeId, ssid: &str, typed: &str) {
        let lang = self.lang;
        let line = self.add(parent, fill_width(row(10.0)), Kind::Box(Look::default()));
        let boxed = self.add(
            line,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 14.0),
                align: Align::Center,
                ..fill_width(row(0.0))
            },
            Kind::Box(Look {
                background: Some(Paint::Surface),
                border: Some(Paint::Border),
                radius: 12.0,
                ..Look::default()
            }),
        );
        let name = words::password_for(ssid, lang);
        let field = TextField::new(&name, words::PASSWORD.get(lang), theme.text.body).secret();
        let field = self.add(
            boxed,
            Style {
                width: Length::Fill,
                height: Length::Px(28.0),
                ..Style::default()
            },
            Kind::TextField(Box::new(field)),
        );
        self.tree.set_field_value(field, typed);
        if let Some((_, node, _)) = &mut self.hardware.password {
            *node = Some(field);
        }
        let join = (!typed.is_empty()).then_some(Target::JoinTyped);
        self.row_button(theme, line, (words::JOIN, ssid), ButtonKind::Primary, join);
        let cancel = Some(Target::CancelPassword);
        self.row_button(
            theme,
            line,
            (words::CANCEL, ssid),
            ButtonKind::Plain,
            cancel,
        );
        self.note(theme, parent, words::WIFI_PASSWORD_NOTE);
    }

    /// Bluetooth: on or off, the devices paired and those in reach.
    pub(super) fn bluetooth(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let unknown = self.unknown(theme, body);
        let h = self.ctx.hardware.clone();
        let devices = match &self.hardware.bluetooth {
            Listed::Known(Bluetooth::Devices(list)) => Some(list.clone()),
            _ => None,
        };

        let section = self.section(theme, body, words::BLUETOOTH);
        // The devices connected, as BlueZ says, once it has been asked.
        let connected = devices.as_ref().and_then(|list| {
            let names: Vec<&str> = list
                .iter()
                .filter(|d| d.connected)
                .map(|d| d.name.as_str())
                .collect();
            (!names.is_empty()).then(|| names.join(", "))
        });
        let sub = match (connected.or(h.bluetooth_device.clone()), h.bluetooth) {
            (_, false) => words::BLUETOOTH_SUB_OFF.get(lang).to_owned(),
            (Some(device), true) => words::connected(&device, lang),
            (None, true) => words::NO_BLUETOOTH_DEVICE.get(lang).to_owned(),
        };
        let target =
            (!unknown).then(|| Target::Hardware(HardwareChange::Bluetooth { on: !h.bluetooth }));
        self.switch_card_to(
            theme,
            section,
            (words::BLUETOOTH_RADIO, &sub),
            h.bluetooth && !unknown,
            target,
            Some(&Setting::Bluetooth),
        );

        let mine = self.section(theme, body, words::MY_DEVICES);
        let radio_off = h.known && !h.bluetooth;
        let said = match &self.hardware.bluetooth {
            _ if radio_off => Some(words::BLUETOOTH_OFF_LIST),
            Listed::Unknown => Some(words::LOOKING_BLUETOOTH),
            Listed::Absent(_) => Some(words::NO_BLUEZ),
            Listed::Known(Bluetooth::Off) => Some(words::BLUETOOTH_OFF_LIST),
            Listed::Known(Bluetooth::NoAdapter) => Some(words::NO_ADAPTER),
            Listed::Known(Bluetooth::Devices(_)) => None,
        };
        let last = match (said, devices) {
            (said, None) | (said @ Some(_), _) => {
                self.note(theme, mine, said.unwrap_or(words::LOOKING_BLUETOOTH));
                mine
            }
            (None, Some(list)) => {
                let (paired, near): (Vec<Device>, Vec<Device>) =
                    list.into_iter().partition(|d| d.paired);
                if paired.is_empty() {
                    self.note(theme, mine, words::NO_PAIRED);
                }
                for d in &paired {
                    self.device_row(theme, mine, d);
                }
                let section = self.section(theme, body, words::DEVICES_IN_REACH);
                for d in &near {
                    self.device_row(theme, section, d);
                }
                if near.is_empty() {
                    self.note(theme, section, words::NONE_IN_REACH);
                }
                self.note(theme, section, words::LOOKING_DEVICES);
                section
            }
        };
        self.note(theme, last, words::BLUETOOTH_NOTE);
    }

    /// A device: its kind, name, state, battery; what can be done with
    /// it; the pairing's question under it.
    fn device_row(&mut self, theme: &Theme, parent: NodeId, d: &Device) {
        let lang = self.lang;
        let busy = self
            .hardware
            .device_busy
            .as_ref()
            .filter(|(p, _)| *p == d.path)
            .map(|(_, said)| *said);
        let failed = self
            .hardware
            .device_failed
            .as_ref()
            .filter(|(p, _)| *p == d.path)
            .map(|(_, why)| why.clone());
        let (sub, paint) = match (busy, failed) {
            (Some(said), _) => (said.get(lang).to_owned(), Paint::Muted),
            (None, Some(why)) => (words::device_failed(&why, lang), Paint::Waiting),
            (None, None) => {
                let mut said = vec![words::device_kind(d.kind).get(lang).to_owned()];
                if d.paired {
                    let state = if d.connected {
                        words::CONNECTED
                    } else {
                        words::NOT_CONNECTED
                    };
                    said.push(state.get(lang).to_owned());
                }
                if let Some(percent) = d.battery {
                    said.push(words::device_battery(percent, lang));
                }
                (said.join(" · "), Paint::Muted)
            }
        };
        let icon = Kind::Icon {
            icon: stroked(device_icon(d.kind), 20.0),
            color: Paint::Text,
            label: None,
        };
        let (line, row_box) =
            self.list_row(theme, parent, d.connected, icon, (&d.name, &sub, paint));
        let free = busy.is_none();
        let act = |action: BluetoothAction| free.then_some(Target::Bluetooth(action));
        let path = d.path.clone();
        if d.paired {
            let (verb, action) = if d.connected {
                (words::DISCONNECT, BluetoothAction::Disconnect(path.clone()))
            } else {
                (words::CONNECT, BluetoothAction::Connect(path.clone()))
            };
            self.row_button(theme, line, (verb, &d.name), ButtonKind::Plain, act(action));
            let forget = act(BluetoothAction::Forget(path));
            self.row_button(
                theme,
                line,
                (words::FORGET, &d.name),
                ButtonKind::Plain,
                forget,
            );
        } else {
            let pair = act(BluetoothAction::Pair(path));
            self.row_button(theme, line, (words::PAIR, &d.name), ButtonKind::Plain, pair);
        }
        let pairing_here = self
            .hardware
            .device_busy
            .as_ref()
            .is_some_and(|(p, _)| *p == d.path);
        match self.hardware.pairing.clone().filter(|_| pairing_here) {
            Some(Pairing::Confirm(code)) => {
                let ask = self.add(row_box, fill_width(row(10.0)), Kind::Box(Look::default()));
                let text = self.add(
                    ask,
                    fill_width(Style::default()),
                    Kind::Box(Look::default()),
                );
                let said = words::compare_code(&d.name, code, lang);
                self.add(
                    text,
                    fill_width(Style::default()),
                    Kind::text(&said, theme.text.body.with_weight(600)),
                );
                let yes = Some(Target::PairingAnswer(true));
                self.row_button(
                    theme,
                    ask,
                    (words::SAME_CODE, &d.name),
                    ButtonKind::Primary,
                    yes,
                );
                let no = Some(Target::PairingAnswer(false));
                self.row_button(theme, ask, (words::CANCEL, &d.name), ButtonKind::Plain, no);
            }
            Some(Pairing::Show(code)) => {
                let said = words::type_code(&d.name, &code, lang);
                self.add(
                    row_box,
                    fill_width(Style::default()),
                    Kind::text(&said, theme.text.body.with_weight(600)),
                );
            }
            None => {}
        }
    }

    /// What NetworkManager says, as the page's worker read it.
    pub fn set_wifi(&mut self, wifi: Listed<Wifi>) {
        if wifi != self.hardware.wifi {
            self.hardware.wifi = wifi;
            self.dirty = true;
        }
    }

    /// The networks were not read yet.
    pub fn wifi_unknown(&self) -> bool {
        self.hardware.wifi == Listed::Unknown
    }

    /// The devices were not read yet.
    pub fn bluetooth_unknown(&self) -> bool {
        self.hardware.bluetooth == Listed::Unknown
    }

    /// What was asked of a network is done, or why not.
    pub fn wifi_done(&mut self, result: Result<(), JoinError>) {
        let Some((ssid, _)) = self.hardware.network_busy.take() else {
            return;
        };
        self.dirty = true;
        let lang = self.lang;
        match result {
            Ok(()) => self.hardware.network_failed = None,
            // Its saved connection had no password: it is asked.
            Err(JoinError::NeedsPassword) => {
                self.hardware.password = Some((ssid, None, String::new()));
            }
            Err(JoinError::Failed) => {
                self.hardware.network_failed = Some((ssid, words::not_joined(None, lang)));
            }
            Err(JoinError::Refused(why)) => {
                let said = words::not_joined(Some(&why), lang);
                self.hardware.network_failed = Some((ssid, said));
            }
        }
    }

    /// What BlueZ says, as the page's worker read it.
    pub fn set_bluetooth(&mut self, bluetooth: Listed<Bluetooth>) {
        if bluetooth != self.hardware.bluetooth {
            self.hardware.bluetooth = bluetooth;
            self.dirty = true;
        }
    }

    /// What was asked of a device is done, or why not.
    pub fn bluetooth_done(&mut self, result: Result<(), String>) {
        let Some((device, _)) = self.hardware.device_busy.take() else {
            return;
        };
        self.hardware.pairing = None;
        self.hardware.device_failed = result.err().map(|why| (device, why));
        self.dirty = true;
    }

    /// What the pairing under way shows now; `None` once it no longer
    /// asks.
    pub fn set_pairing(&mut self, pairing: Option<Pairing>) {
        if pairing != self.hardware.pairing {
            self.hardware.pairing = pairing;
            self.dirty = true;
        }
    }

    /// A network asked something: it says so until done.
    pub(super) fn ask_network(&mut self, action: WifiAction) -> Outcome {
        let said = match &action {
            WifiAction::Join { .. } => words::JOINING,
            _ => words::WORKING,
        };
        self.hardware.network_busy = Some((action.ssid().to_owned(), said));
        self.hardware.network_failed = None;
        self.hardware.password = None;
        self.dirty = true;
        Outcome::Wifi(action)
    }

    /// The password typed joins its network.
    pub(super) fn join_typed(&mut self) -> Option<Outcome> {
        let (ssid, _, typed) = self.hardware.password.clone()?;
        if typed.is_empty() {
            return None;
        }
        Some(self.ask_network(WifiAction::Join {
            ssid,
            password: Some(Secret(typed)),
        }))
    }

    /// A device asked something: it says so until done.
    pub(super) fn ask_device(&mut self, action: BluetoothAction) -> Outcome {
        let said = match &action {
            BluetoothAction::Pair(_) => words::PAIRING,
            _ => words::WORKING,
        };
        self.hardware.device_busy = Some((action.device().to_owned(), said));
        self.hardware.device_failed = None;
        self.hardware.pairing = None;
        self.dirty = true;
        Outcome::Bluetooth(action)
    }

    /// Power: the battery, the profile, the delays on the charger and on
    /// battery, how the machine sleeps, what keeps it awake.
    pub(super) fn power(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let unknown = self.unknown(theme, body);
        let h = self.ctx.hardware.clone();
        let idle = self.ctx.config.idle;

        if !unknown {
            let section = self.section(theme, body, words::BATTERY);
            match &h.battery {
                Some(battery) => {
                    let said = words::battery_state(battery, lang);
                    self.line(section, &said, theme.text.title.with_weight(700));
                }
                None => self.note(theme, section, words::NO_BATTERY),
            }

            let section = self.section(theme, body, words::POWER_PROFILE);
            let line = self.add(section, row(8.0), Kind::Box(Look::default()));
            for p in &h.profiles {
                let target = Target::Hardware(HardwareChange::Profile { profile: p.clone() });
                let name = words::profile(p).get(lang);
                let b = self.choice(theme, line, name, *p == h.profile, target);
                let said = words::named_value(words::POWER_PROFILE, name, lang);
                self.tree.set_name(b, &said);
            }
            self.note(theme, section, words::POWER_PROFILE_NOTE);
            self.highlighted(section, &Setting::PowerProfile, Look::default());
        }

        // The charger's delays, then the battery's, which the file may
        // leave as the charger's.
        for (source, words_, section_key) in [
            (PowerSource::Charger, words::ON_CHARGER, "idle"),
            (PowerSource::Battery, words::ON_BATTERY, "idle.battery"),
        ] {
            let section = self.section(theme, body, words_);
            let delays = idle.delays(source);
            for (name, key, offered, current) in [
                (
                    words::DIM_AFTER,
                    "curtain_after",
                    &DIM[..],
                    delays.curtain_after,
                ),
                (
                    words::SLEEP_AFTER,
                    "sleep_after",
                    &SLEEP[..],
                    delays.sleep_after,
                ),
            ] {
                let all = with_current(offered, current);
                let choices = all
                    .iter()
                    .map(|s| {
                        let target = Target::Set(Change::new(section_key, key, i64::from(*s)));
                        (words::duration(*s, lang), *s == current, target)
                    })
                    .collect();
                let buttons = self.named_choices(theme, section, name, choices);
                for (b, s) in buttons.into_iter().zip(&all) {
                    let said = words::on_source(words_, name, &words::duration(*s, lang), lang);
                    self.tree.set_name(b, &said);
                }
            }
            if source == PowerSource::Battery {
                self.note(theme, section, words::DELAYS_NOTE);
            }
            self.highlighted(section, &Setting::IdleDelays, Look::default());
        }

        let section = self.section(theme, body, words::SLEEP_HOW);
        let line = self.add(section, row(8.0), Kind::Box(Look::default()));
        for sleep in [Sleep::Suspend, Sleep::Hibernate] {
            let target = Target::Set(Change::new("idle", "sleep", sleep.as_str()));
            let name = words::sleep_name(sleep).get(lang);
            let b = self.choice(theme, line, name, sleep == idle.sleep, target);
            let said = words::named_value(words::SLEEP_HOW, name, lang);
            self.tree.set_name(b, &said);
        }
        // Known only once logind answered; unknown, nothing is said.
        if idle.sleep == Sleep::Hibernate && h.known && !h.can_hibernate {
            self.note(theme, section, words::CANNOT_HIBERNATE);
        }
        self.note(theme, section, words::SLEEP_NOTE);
        self.highlighted(section, &Setting::SleepAction, Look::default());

        let section = self.section(theme, body, words::KEEP_AWAKE);
        self.switch_card(
            theme,
            section,
            (words::FULLSCREEN_AWAKE, words::FULLSCREEN_AWAKE_SUB),
            idle.fullscreen_awake,
            Some(Change::new(
                "idle",
                "fullscreen_awake",
                !idle.fullscreen_awake,
            )),
            Some(&Setting::FullscreenAwake),
        );
        self.note(theme, section, words::KEEP_AWAKE_NOTE);
    }

    /// Lock: after the curtain or not, how long the curtain shows, the
    /// keys, the lock screen.
    pub(super) fn lock(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let idle = self.ctx.config.idle;

        let section = self.section(theme, body, words::LOCKING);
        self.switch_card(
            theme,
            section,
            (words::LOCK_ON_IDLE, words::LOCK_ON_IDLE_SUB),
            idle.lock,
            Some(Change::new("idle", "lock", !idle.lock)),
            Some(&Setting::LockOnIdle),
        );
        if idle.lock {
            let all = with_current(&CURTAIN, idle.lock_after);
            let choices = all
                .iter()
                .map(|s| {
                    let target = Target::Set(Change::new("idle", "lock_after", i64::from(*s)));
                    (words::duration(*s, lang), *s == idle.lock_after, target)
                })
                .collect();
            self.named_choices(theme, section, words::CURTAIN_FOR, choices);
        }
        let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        let text = self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        self.note(theme, text, words::LOCK_NOTE);
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                Page::Power.label().get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(b, Target::Page(Page::Power));
        self.keys_line(theme, section, &[Action::Lock]);
        self.note(theme, section, words::ALWAYS_LOCKED);

        let section = self.section(theme, body, words::LOCK_SCREEN);
        let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        let text = self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        self.note(theme, text, words::LOCK_SCREEN_NOTE);
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                Page::Pandas.label().get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(b, Target::Page(Page::Pandas));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_typed_in_the_file_is_offered_too() {
        assert_eq!(with_current(&DIM, 300), [60, 120, 300, 600, 0]);
        assert_eq!(
            with_current(&[60, 0, 300], 45),
            vec![45, 60, 300, 0],
            "never stays last"
        );
    }
}
