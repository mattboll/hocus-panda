//! The mice and touchpads plugged in, as the compositor tells them on its
//! control socket (`hocus msg input-devices`, 6.12): each device's kind,
//! where it is plugged, what it can take and what the compositor gave it
//! of `[pointer]` or `[touchpad]` (docs/design/settings.md, "Souris et pavé
//! tactile").
//!
//! One device a line, words separated by spaces:
//!
//! ```text
//! device 3 touchpad internal built-in can=tap,clickfinger,button-areas,edge,two-fingers,dwt,natural,accel,off-with-mouse fingers=4 click-default=corner speed=0.0 tap=on drag=on click=two-fingers dwt=on natural=off scroll=two-fingers off-with-mouse=off
//! device 4 pointer usb external can=accel,flat,adaptive,natural,left-handed speed=-0.5 acceleration=flat natural=off left-handed=off
//! ```
//!
//! Followed (`msg input-devices follow`), the same lines come as events:
//! `added DEVICE`, then `listed` once the devices there are said, then
//! `added DEVICE`, `changed DEVICE` (its settings given again) and
//! `removed ID` as they happen. The device's own name (a vendor's string)
//! is never said: it goes to the compositor's log.

use std::{fmt, str::FromStr};

/// What a device is, for its settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeviceKind {
    /// It can tap (libinput says tap fingers): `[touchpad]`.
    Touchpad,
    /// Any other pointer (a mouse, a trackball, a trackpoint): `[pointer]`.
    Pointer,
}

/// Where it is plugged, as udev says (`ID_BUS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Bus {
    Usb,
    Bluetooth,
    /// Anything else: i2c, serial, the laptop's own.
    Internal,
}

/// How the touchpad clicks right: two fingers anywhere (clickfinger), or
/// the bottom right corner (button areas).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RightClick {
    TwoFingers,
    Corner,
}

/// How the touchpad scrolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollWay {
    TwoFingers,
    Edge,
}

/// The pointer's acceleration profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Acceleration {
    Adaptive,
    Flat,
}

/// What a device can take: libinput's configuration it offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Can {
    pub tap: bool,
    pub clickfinger: bool,
    pub button_areas: bool,
    pub edge: bool,
    pub two_fingers: bool,
    pub dwt: bool,
    pub natural: bool,
    pub accel: bool,
    pub flat: bool,
    pub adaptive: bool,
    pub left_handed: bool,
    /// Turned off while an external mouse is plugged
    /// (`DISABLED_ON_EXTERNAL_MOUSE`).
    pub off_with_mouse: bool,
    /// The most fingers it senses at once (its `BTN_TOOL_*TAP` keys,
    /// raised to the most fingers a gesture was seen with; 0 when not
    /// known, which greys nothing): the gestures it can make (6.13).
    /// Written `fingers=N`.
    pub fingers: u32,
}

/// What the compositor gave a device: `None` where it gave nothing (the
/// device cannot take it, or the setting is the device's own default).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Applied {
    pub speed: Option<f64>,
    pub acceleration: Option<Acceleration>,
    pub natural: Option<bool>,
    pub left_handed: Option<bool>,
    pub tap: Option<bool>,
    pub drag: Option<bool>,
    pub click: Option<RightClick>,
    pub dwt: Option<bool>,
    pub scroll: Option<ScrollWay>,
    pub off_with_mouse: Option<bool>,
}

/// A mouse or a touchpad plugged in.
#[derive(Debug, Clone, PartialEq)]
pub struct InputDevice {
    /// The compositor's number for it, for as long as it is plugged.
    pub id: u64,
    pub kind: DeviceKind,
    pub bus: Bus,
    /// Part of the computer (a laptop's touchpad or trackpoint).
    pub built_in: bool,
    pub can: Can,
    /// How a touchpad clicks right when nothing is set (libinput's
    /// default for it), if it can at all.
    pub click_default: Option<RightClick>,
    pub applied: Applied,
}

/// What a followed connection hears.
#[derive(Debug, Clone, PartialEq)]
pub enum DeviceEvent {
    Added(InputDevice),
    /// The devices there when it started following are all said.
    Listed,
    /// Its settings given again (the file changed).
    Changed(InputDevice),
    Removed(u64),
}

impl RightClick {
    pub fn word(self) -> &'static str {
        match self {
            Self::TwoFingers => "two-fingers",
            Self::Corner => "corner",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        match word {
            "two-fingers" => Some(Self::TwoFingers),
            "corner" => Some(Self::Corner),
            _ => None,
        }
    }
}

impl ScrollWay {
    pub fn word(self) -> &'static str {
        match self {
            Self::TwoFingers => "two-fingers",
            Self::Edge => "edge",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        match word {
            "two-fingers" => Some(Self::TwoFingers),
            "edge" => Some(Self::Edge),
            _ => None,
        }
    }
}

impl Acceleration {
    pub fn word(self) -> &'static str {
        match self {
            Self::Adaptive => "adaptive",
            Self::Flat => "flat",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        match word {
            "adaptive" => Some(Self::Adaptive),
            "flat" => Some(Self::Flat),
            _ => None,
        }
    }
}

impl Can {
    /// Its words, in this order.
    fn words(self) -> Vec<&'static str> {
        [
            (self.tap, "tap"),
            (self.clickfinger, "clickfinger"),
            (self.button_areas, "button-areas"),
            (self.edge, "edge"),
            (self.two_fingers, "two-fingers"),
            (self.dwt, "dwt"),
            (self.natural, "natural"),
            (self.accel, "accel"),
            (self.flat, "flat"),
            (self.adaptive, "adaptive"),
            (self.left_handed, "left-handed"),
            (self.off_with_mouse, "off-with-mouse"),
        ]
        .into_iter()
        .filter_map(|(can, word)| can.then_some(word))
        .collect()
    }

    fn with_fingers(self, fingers: u32) -> Self {
        Self { fingers, ..self }
    }

    fn of(list: &str) -> Result<Self, String> {
        let mut can = Self::default();
        for word in list.split(',').filter(|w| !w.is_empty()) {
            let flag = match word {
                "tap" => &mut can.tap,
                "clickfinger" => &mut can.clickfinger,
                "button-areas" => &mut can.button_areas,
                "edge" => &mut can.edge,
                "two-fingers" => &mut can.two_fingers,
                "dwt" => &mut can.dwt,
                "natural" => &mut can.natural,
                "accel" => &mut can.accel,
                "flat" => &mut can.flat,
                "adaptive" => &mut can.adaptive,
                "left-handed" => &mut can.left_handed,
                "off-with-mouse" => &mut can.off_with_mouse,
                other => return Err(format!("a capability that does not read: {other}")),
            };
            *flag = true;
        }
        Ok(can)
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn bool_of(word: &str) -> Result<bool, String> {
    match word {
        "on" => Ok(true),
        "off" => Ok(false),
        other => Err(format!("on or off, not {other}")),
    }
}

impl fmt::Display for InputDevice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            DeviceKind::Touchpad => "touchpad",
            DeviceKind::Pointer => "pointer",
        };
        let bus = match self.bus {
            Bus::Usb => "usb",
            Bus::Bluetooth => "bluetooth",
            Bus::Internal => "internal",
        };
        let built_in = if self.built_in {
            "built-in"
        } else {
            "external"
        };
        write!(
            f,
            "device {} {kind} {bus} {built_in} can={}",
            self.id,
            self.can.words().join(",")
        )?;
        if self.can.fingers > 0 {
            write!(f, " fingers={}", self.can.fingers)?;
        }
        if let Some(click) = self.click_default {
            write!(f, " click-default={}", click.word())?;
        }
        let a = &self.applied;
        if let Some(speed) = a.speed {
            write!(f, " speed={speed:.1}")?;
        }
        if let Some(accel) = a.acceleration {
            write!(f, " acceleration={}", accel.word())?;
        }
        for (name, value) in [
            ("tap", a.tap),
            ("drag", a.drag),
            ("dwt", a.dwt),
            ("natural", a.natural),
            ("left-handed", a.left_handed),
        ] {
            if let Some(on) = value {
                write!(f, " {name}={}", on_off(on))?;
            }
        }
        if let Some(click) = a.click {
            write!(f, " click={}", click.word())?;
        }
        if let Some(scroll) = a.scroll {
            write!(f, " scroll={}", scroll.word())?;
        }
        if let Some(off) = a.off_with_mouse {
            write!(f, " off-with-mouse={}", on_off(off))?;
        }
        Ok(())
    }
}

impl FromStr for InputDevice {
    type Err = String;

    fn from_str(line: &str) -> Result<Self, String> {
        let mut words = line.split_whitespace();
        if words.next() != Some("device") {
            return Err(format!("not a device: {line}"));
        }
        let id = words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or("a device without its number")?;
        let kind = match words.next() {
            Some("touchpad") => DeviceKind::Touchpad,
            Some("pointer") => DeviceKind::Pointer,
            other => return Err(format!("a kind that does not read: {other:?}")),
        };
        let bus = match words.next() {
            Some("usb") => Bus::Usb,
            Some("bluetooth") => Bus::Bluetooth,
            Some("internal") => Bus::Internal,
            other => return Err(format!("a bus that does not read: {other:?}")),
        };
        let built_in = match words.next() {
            Some("built-in") => true,
            Some("external") => false,
            other => return Err(format!("built-in or external, not {other:?}")),
        };
        let mut device = Self {
            id,
            kind,
            bus,
            built_in,
            can: Can::default(),
            click_default: None,
            applied: Applied::default(),
        };
        for word in words {
            let (name, value) = word
                .split_once('=')
                .ok_or_else(|| format!("not NAME=VALUE: {word}"))?;
            let a = &mut device.applied;
            match name {
                "can" => device.can = Can::of(value)?.with_fingers(device.can.fingers),
                "fingers" => device.can.fingers = value.parse().map_err(|_| "a finger count")?,
                "click-default" => {
                    device.click_default = Some(RightClick::of(value).ok_or("a click method")?);
                }
                "speed" => a.speed = Some(value.parse().map_err(|_| "a speed")?),
                "acceleration" => {
                    a.acceleration = Some(Acceleration::of(value).ok_or("an acceleration")?);
                }
                "tap" => a.tap = Some(bool_of(value)?),
                "drag" => a.drag = Some(bool_of(value)?),
                "dwt" => a.dwt = Some(bool_of(value)?),
                "natural" => a.natural = Some(bool_of(value)?),
                "left-handed" => a.left_handed = Some(bool_of(value)?),
                "click" => a.click = Some(RightClick::of(value).ok_or("a click method")?),
                "scroll" => a.scroll = Some(ScrollWay::of(value).ok_or("a scroll method")?),
                "off-with-mouse" => a.off_with_mouse = Some(bool_of(value)?),
                // A newer compositor's: passed over.
                _ => {}
            }
        }
        Ok(device)
    }
}

impl fmt::Display for DeviceEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added(device) => write!(f, "added {device}"),
            Self::Listed => write!(f, "listed"),
            Self::Changed(device) => write!(f, "changed {device}"),
            Self::Removed(id) => write!(f, "removed {id}"),
        }
    }
}

impl FromStr for DeviceEvent {
    type Err = String;

    fn from_str(line: &str) -> Result<Self, String> {
        let line = line.trim_end_matches(['\n', '\r']);
        if line == "listed" {
            return Ok(Self::Listed);
        }
        if let Some(device) = line.strip_prefix("added ") {
            return device.parse().map(Self::Added);
        }
        if let Some(device) = line.strip_prefix("changed ") {
            return device.parse().map(Self::Changed);
        }
        if let Some(id) = line.strip_prefix("removed ") {
            return id
                .parse()
                .map(Self::Removed)
                .map_err(|_| format!("not a device's number: {id}"));
        }
        Err(format!("not a device's event: {line}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices_and_their_events_read_back() {
        let touchpad = InputDevice {
            id: 3,
            kind: DeviceKind::Touchpad,
            bus: Bus::Internal,
            built_in: true,
            can: Can {
                tap: true,
                clickfinger: true,
                button_areas: true,
                two_fingers: true,
                dwt: true,
                natural: true,
                accel: true,
                fingers: 4,
                ..Can::default()
            },
            click_default: Some(RightClick::Corner),
            applied: Applied {
                speed: Some(0.3),
                tap: Some(true),
                drag: Some(true),
                click: Some(RightClick::TwoFingers),
                dwt: Some(true),
                natural: Some(false),
                scroll: Some(ScrollWay::TwoFingers),
                ..Applied::default()
            },
        };
        let line = touchpad.to_string();
        assert!(line.starts_with("device 3 touchpad internal built-in can=tap,"));
        assert!(line.contains(" fingers=4 "), "{line}");
        assert_eq!(line.parse::<InputDevice>(), Ok(touchpad.clone()));
        let mouse = InputDevice {
            id: 4,
            kind: DeviceKind::Pointer,
            bus: Bus::Usb,
            built_in: false,
            can: Can {
                accel: true,
                flat: true,
                adaptive: true,
                left_handed: true,
                natural: true,
                ..Can::default()
            },
            click_default: None,
            applied: Applied {
                speed: Some(-0.5),
                acceleration: Some(Acceleration::Flat),
                natural: Some(true),
                left_handed: Some(false),
                ..Applied::default()
            },
        };
        for event in [
            DeviceEvent::Added(mouse.clone()),
            DeviceEvent::Listed,
            DeviceEvent::Changed(touchpad),
            DeviceEvent::Removed(4),
        ] {
            assert_eq!(event.to_string().parse::<DeviceEvent>(), Ok(event));
        }
        assert!("device x touchpad".parse::<InputDevice>().is_err());
        assert!("gone 3".parse::<DeviceEvent>().is_err());
    }
}
