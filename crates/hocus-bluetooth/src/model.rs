//! What Bluetooth shows: the adapter and the devices as BlueZ last said
//! them ([`Bluetooth`]), and the bar's row for it ([`Row`]). The words are
//! each window's own.

/// What a device is, for its icon and its line, from BlueZ's `Icon`, made
/// finer by its `Class` or its `Appearance` (earbuds, a tablet).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Headphones,
    Earbuds,
    Speaker,
    Mouse,
    Keyboard,
    Gamepad,
    Phone,
    Tablet,
    Computer,
    /// Anything else: the Bluetooth sign.
    #[default]
    Other,
}

impl Kind {
    /// From BlueZ's `Icon`, a freedesktop icon name it derives from the
    /// device's class or appearance.
    pub fn from_icon(icon: &str) -> Option<Self> {
        Some(match icon {
            "audio-headphones" | "audio-headset" => Self::Headphones,
            "audio-card" | "audio-speakers" => Self::Speaker,
            "input-mouse" | "input-tablet" => Self::Mouse,
            "input-keyboard" => Self::Keyboard,
            "input-gaming" => Self::Gamepad,
            "phone" => Self::Phone,
            "computer" => Self::Computer,
            _ => return None,
        })
    }

    /// From a BR/EDR class of device: its major class (bits 8 to 12) and
    /// its minor one.
    pub fn from_class(class: u32) -> Option<Self> {
        let minor = (class >> 2) & 0x3F;
        Some(match (class >> 8) & 0x1F {
            0x01 if minor == 0x07 => Self::Tablet,
            0x01 => Self::Computer,
            0x02 => Self::Phone,
            0x04 => match minor {
                0x01 | 0x02 | 0x06 => Self::Headphones,
                _ => Self::Speaker,
            },
            0x05 => match ((minor >> 4) & 0x03, minor & 0x0F) {
                (0x01, _) => Self::Keyboard,
                (0x02 | 0x03, _) => Self::Mouse,
                (_, 0x01 | 0x02) => Self::Gamepad,
                _ => return None,
            },
            _ => return None,
        })
    }

    /// From a Bluetooth LE appearance: its category (the top ten bits) and
    /// its subcategory.
    pub fn from_appearance(appearance: u16) -> Option<Self> {
        Some(match (appearance >> 6, appearance & 0x3F) {
            (0x001, _) => Self::Phone,
            (0x002, 0x07) => Self::Tablet,
            (0x002, _) => Self::Computer,
            (0x00F, 0x01) => Self::Keyboard,
            (0x00F, 0x02) => Self::Mouse,
            (0x00F, 0x03 | 0x04) => Self::Gamepad,
            (0x021, _) => Self::Speaker,
            (0x025, 0x01) => Self::Earbuds,
            (0x025, _) => Self::Headphones,
            _ => return None,
        })
    }

    /// The kind of a device that says `icon` and, finer, `fine` (from its
    /// class or its appearance): the icon's, unless `fine` says more of it
    /// (earbuds for headphones, a tablet for a computer or a phone).
    pub fn of(icon: Option<Self>, fine: Option<Self>) -> Self {
        match (icon, fine) {
            (Some(Self::Headphones), Some(Self::Earbuds))
            | (Some(Self::Computer | Self::Phone), Some(Self::Tablet)) => fine.unwrap_or_default(),
            (Some(icon), _) => icon,
            (None, fine) => fine.unwrap_or_default(),
        }
    }

    /// Whether pairing it asks for a code (a phone, a tablet or a computer
    /// compares or types a number): the bar's popover leaves it to the
    /// Settings.
    pub fn asks_a_code(self) -> bool {
        matches!(self, Self::Phone | Self::Tablet | Self::Computer)
    }

    /// Whether it is a device one types or points with, which connects by
    /// itself when switched on once trusted.
    pub fn input(self) -> bool {
        matches!(self, Self::Mouse | Self::Keyboard | Self::Gamepad)
    }
}

/// A device as BlueZ says it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    /// Its object's path, which names it to BlueZ.
    pub path: String,
    /// Its `Address`, which names it to people and programs.
    pub address: String,
    /// Its `Alias`: its own name, or the one given to it.
    pub name: String,
    pub kind: Kind,
    pub paired: bool,
    pub connected: bool,
    /// It may connect by itself (`Trusted`).
    pub trusted: bool,
    /// Its battery, percent, when it reports one (BlueZ says it while it
    /// is connected).
    pub battery: Option<u8>,
    /// Pairing it needs a code: left to the Settings by the bar.
    pub code: bool,
}

impl Device {
    /// Its battery at 15 % or less: a dot on the bar's icon, the
    /// percentage in orange.
    pub fn low(&self) -> bool {
        self.battery.is_some_and(|b| b <= LOW)
    }
}

/// A battery at this or less is low.
pub const LOW: u8 = 15;

/// What was asked of a device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Connect,
    /// Pair, trust, then connect.
    Pair,
}

/// Why it did not come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// The device did not answer the connection (off, out of range,
    /// refused, timed out).
    NoAnswer,
    /// Pairing failed (not in pairing mode, refused, timed out).
    Pairing,
    /// Pairing asked for a code, which the agent does not give (the bar's:
    /// left to the Settings).
    Code,
    /// The device gave up the pairing (BlueZ's `Cancel`).
    Cancelled,
    /// The user cancelled it, or refused the code: said nowhere.
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Under way: pairing, then connecting (a paired device connects).
    Running(Action),
    Done,
    Failed(Failure),
}

/// What was asked last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Each asked has its own number: whoever shows it reacts once to
    /// each end.
    pub id: u64,
    pub path: String,
    pub step: Step,
}

/// Bluetooth as BlueZ last said it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Bluetooth {
    /// BlueZ is there with an adapter.
    pub adapter: bool,
    /// The adapter is on (off when blocked by rfkill too).
    pub powered: bool,
    /// It is off because rfkill blocks it (the wireless switch, airplane
    /// mode): turning it on fails.
    pub blocked: bool,
    /// The adapter's name, as other devices see it (`Alias`).
    pub alias: String,
    /// Other devices can find it now (`Discoverable`).
    pub discoverable: bool,
    /// The search runs its first pass.
    pub searching: bool,
    /// Connected, the last connected first.
    pub connected: Vec<Device>,
    /// Paired and not connected, by name (listed while the adapter is
    /// off too).
    pub paired: Vec<Device>,
    /// Around, not paired, named, the strongest first (while it is on).
    pub nearby: Vec<Device>,
    pub attempt: Option<Attempt>,
    /// What BlueZ asks the agent now, when it asks the user (the
    /// Settings').
    pub request: Option<Request>,
}

/// What BlueZ asks the user through the agent, for a pairing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Answered by it ([`crate::Ask::Answer`]).
    pub id: u64,
    pub path: String,
    pub name: String,
    pub kind: Kind,
    pub asked: Asked,
    /// When the pairing is given up, if it is.
    pub until: Option<std::time::Instant>,
    /// The pairing was asked here (else the device asked it).
    pub ours: bool,
}

/// What is asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// Both sides show this number: does it match? (`RequestConfirmation`)
    Confirm(u32),
    /// Type this code on the device (`DisplayPasskey`, with the digits
    /// typed so far; `DisplayPinCode`, without).
    Display { code: String, typed: Option<u16> },
    /// A PIN to give (`RequestPinCode`).
    Pin,
    /// A number to give (`RequestPasskey`).
    Passkey,
    /// The device pairs by itself: let it? (`RequestAuthorization`)
    Authorize,
}

/// The user's answer to a [`Request`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    Yes,
    No,
    Pin(String),
    Passkey(u32),
}

/// What the bar's row says (bar.md §4.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    /// No BlueZ or no adapter: no row.
    Hidden,
    Off,
    /// On, nothing connected.
    Nothing,
    /// On its way, nothing connected yet.
    Connecting,
    Connected {
        /// The last connected's name and battery.
        name: String,
        battery: Option<u8>,
        /// How many more are connected.
        more: usize,
        /// A connected device's battery is low.
        low: bool,
    },
}

impl Bluetooth {
    pub fn row(&self) -> Row {
        if !self.adapter {
            return Row::Hidden;
        }
        if !self.powered {
            return Row::Off;
        }
        match self.connected.first() {
            Some(first) => Row::Connected {
                name: first.name.clone(),
                battery: first.battery,
                more: self.connected.len() - 1,
                low: self.connected.iter().any(Device::low),
            },
            None if self.running().is_some() => Row::Connecting,
            None => Row::Nothing,
        }
    }

    /// The device something runs for, and what.
    pub fn running(&self) -> Option<(&str, Action)> {
        let attempt = self.attempt.as_ref()?;
        match attempt.step {
            Step::Running(action) => Some((attempt.path.as_str(), action)),
            _ => None,
        }
    }

    /// The paired devices: those connected, the last connected first, then
    /// the others by name.
    pub fn mine(&self) -> impl Iterator<Item = &Device> {
        self.connected.iter().chain(&self.paired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(name: &str, battery: Option<u8>) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            name: name.to_owned(),
            kind: Kind::Headphones,
            paired: true,
            connected: true,
            battery,
            ..Device::default()
        }
    }

    #[test]
    fn kinds_from_icons_classes_and_appearances() {
        assert_eq!(Kind::from_icon("audio-headset"), Some(Kind::Headphones));
        assert_eq!(Kind::from_icon("audio-card"), Some(Kind::Speaker));
        assert_eq!(Kind::from_icon("input-mouse"), Some(Kind::Mouse));
        assert_eq!(Kind::from_icon("input-gaming"), Some(Kind::Gamepad));
        assert_eq!(Kind::from_icon("printer"), None);
        // Headphones (0x240418), a phone (0x5A020C), a keyboard
        // (0x002540), a mouse (0x002580), a speaker (0x240414), a gamepad
        // (0x002508), a tablet (0x10011C).
        assert_eq!(Kind::from_class(0x24_0418), Some(Kind::Headphones));
        assert_eq!(Kind::from_class(0x5A_020C), Some(Kind::Phone));
        assert_eq!(Kind::from_class(0x00_2540), Some(Kind::Keyboard));
        assert_eq!(Kind::from_class(0x00_2580), Some(Kind::Mouse));
        assert_eq!(Kind::from_class(0x24_0414), Some(Kind::Speaker));
        assert_eq!(Kind::from_class(0x00_2508), Some(Kind::Gamepad));
        assert_eq!(Kind::from_class(0x10_011C), Some(Kind::Tablet));
        assert_eq!(Kind::from_appearance(0x03C1), Some(Kind::Keyboard));
        assert_eq!(Kind::from_appearance(0x03C2), Some(Kind::Mouse));
        assert_eq!(Kind::from_appearance(0x03C4), Some(Kind::Gamepad));
        assert_eq!(Kind::from_appearance(0x0941), Some(Kind::Earbuds));
        assert_eq!(Kind::from_appearance(0x0943), Some(Kind::Headphones));
        assert_eq!(Kind::from_appearance(0x0087), Some(Kind::Tablet));
        // The icon, made finer by the class or the appearance.
        assert_eq!(
            Kind::of(Some(Kind::Headphones), Some(Kind::Earbuds)),
            Kind::Earbuds
        );
        assert_eq!(
            Kind::of(Some(Kind::Computer), Some(Kind::Tablet)),
            Kind::Tablet
        );
        assert_eq!(
            Kind::of(Some(Kind::Speaker), Some(Kind::Headphones)),
            Kind::Speaker
        );
        assert_eq!(Kind::of(None, None), Kind::Other);
        assert!(Kind::Phone.asks_a_code() && Kind::Tablet.asks_a_code());
        assert!(!Kind::Speaker.asks_a_code());
        assert!(Kind::Keyboard.input() && !Kind::Headphones.input());
    }

    #[test]
    fn the_row_says_off_nothing_connecting_or_the_last_connected() {
        let mut bt = Bluetooth::default();
        assert_eq!(bt.row(), Row::Hidden);
        bt.adapter = true;
        assert_eq!(bt.row(), Row::Off);
        bt.powered = true;
        assert_eq!(bt.row(), Row::Nothing);
        bt.attempt = Some(Attempt {
            id: 1,
            path: "/org/bluez/hci0/dev_x".to_owned(),
            step: Step::Running(Action::Connect),
        });
        assert_eq!(bt.row(), Row::Connecting);
        bt.connected = vec![device("WH-1000XM4", Some(82)), device("MX", Some(12))];
        assert_eq!(
            bt.row(),
            Row::Connected {
                name: "WH-1000XM4".to_owned(),
                battery: Some(82),
                more: 1,
                low: true,
            }
        );
        bt.paired = vec![device("K3", None)];
        let mine: Vec<&str> = bt.mine().map(|d| d.name.as_str()).collect();
        assert_eq!(mine, ["WH-1000XM4", "MX", "K3"]);
    }
}
