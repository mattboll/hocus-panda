//! The mice and touchpads (6.12, docs/design/settings.md "Souris et pavé
//! tactile"): `[pointer]` and `[touchpad]` given to each device as libinput
//! plugs it in, and again to every device when the file changes.
//!
//! Without them libinput's defaults rule (tap off). A device is a touchpad
//! when it taps, else a pointer; keyboards, tablets and touch screens are
//! left alone. What a device cannot take (edge scrolling on a pad without
//! it, a flat profile on a device without profiles) is skipped for that
//! device and logged. The devices, what they can and what they were given
//! are said on the control socket (`hocus msg input-devices`, followed by
//! the Settings); the winit and headless backends have no libinput, so
//! tests plug devices of their own (the harness), which keep what they
//! would be given.

use std::{io::Write as _, os::unix::net::UnixStream};

use hocus_link::input::{
    Acceleration, Applied, Bus, Can, DeviceEvent, DeviceKind, InputDevice, RightClick, ScrollWay,
};
use smithay::reexports::input::{
    self as libinput, AccelProfile, ClickMethod, DeviceCapability, ScrollMethod, SendEventsMode,
    TapButtonMap,
};

use crate::{
    State,
    config::{Pointer, Touchpad},
};

/// The devices plugged in, and the connections following them.
#[derive(Debug, Default)]
pub struct InputDevices {
    list: Vec<Plugged>,
    last_id: u64,
    followers: Vec<UnixStream>,
}

#[derive(Debug)]
struct Plugged {
    said: InputDevice,
    /// libinput's device; `None` for a test's.
    device: Option<libinput::Device>,
}

/// A device as a test plugs it in: what it is and what it can.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FakeDevice {
    pub kind: DeviceKind,
    pub bus: Bus,
    pub can: Can,
    pub click_default: Option<RightClick>,
}

/// What `[pointer]` or `[touchpad]` gives a device that can take `can`.
pub fn settle(
    kind: DeviceKind,
    can: Can,
    pointer: &Pointer,
    touchpad: &Touchpad,
) -> (Applied, Vec<&'static str>) {
    let mut skipped = Vec::new();
    let mut given = Applied::default();
    match kind {
        DeviceKind::Pointer => {
            given.speed = can.accel.then_some(pointer.speed);
            let profile_taken = match pointer.acceleration {
                Acceleration::Adaptive => can.adaptive,
                Acceleration::Flat => can.flat,
            };
            if profile_taken {
                given.acceleration = Some(pointer.acceleration);
            } else if pointer.acceleration == Acceleration::Flat {
                skipped.push("acceleration");
            }
            if can.natural {
                given.natural = Some(pointer.natural_scroll);
            } else if pointer.natural_scroll {
                skipped.push("natural-scroll");
            }
            if can.left_handed {
                given.left_handed = Some(pointer.left_handed);
            } else if pointer.left_handed {
                skipped.push("left-handed");
            }
        }
        DeviceKind::Touchpad => {
            given.speed = can.accel.then_some(touchpad.speed);
            if can.tap {
                given.tap = Some(touchpad.tap_to_click);
                given.drag = Some(touchpad.tap_and_drag);
            }
            match touchpad.right_click {
                None => {}
                Some(RightClick::TwoFingers) if can.clickfinger => {
                    given.click = Some(RightClick::TwoFingers);
                }
                Some(RightClick::Corner) if can.button_areas => {
                    given.click = Some(RightClick::Corner);
                }
                Some(_) => skipped.push("right-click"),
            }
            if can.dwt {
                given.dwt = Some(touchpad.disable_while_typing);
            }
            if can.natural {
                given.natural = Some(touchpad.natural_scroll);
            } else if touchpad.natural_scroll {
                skipped.push("natural-scroll");
            }
            match touchpad.scroll {
                ScrollWay::TwoFingers if can.two_fingers => {
                    given.scroll = Some(ScrollWay::TwoFingers)
                }
                ScrollWay::Edge if can.edge => given.scroll = Some(ScrollWay::Edge),
                ScrollWay::TwoFingers => {}
                ScrollWay::Edge => skipped.push("scroll"),
            }
            if can.off_with_mouse {
                given.off_with_mouse = Some(touchpad.off_with_mouse);
            } else if touchpad.off_with_mouse {
                skipped.push("off-with-mouse");
            }
        }
    }
    (given, skipped)
}

/// What libinput's `device` is and can, if it is a mouse or a touchpad.
fn describe(device: &libinput::Device) -> Option<(DeviceKind, Bus, bool, Can, Option<RightClick>)> {
    if !device.has_capability(DeviceCapability::Pointer)
        || device.has_capability(DeviceCapability::TabletTool)
        || device.has_capability(DeviceCapability::Touch)
    {
        return None;
    }
    let kind = if device.config_tap_finger_count() > 0 {
        DeviceKind::Touchpad
    } else {
        DeviceKind::Pointer
    };
    let (bus, integration) = udev_words(device);
    let bus = match bus.as_deref() {
        Some("usb") => Bus::Usb,
        Some("bluetooth") => Bus::Bluetooth,
        _ => Bus::Internal,
    };
    let built_in = bus == Bus::Internal || integration.as_deref() == Some("internal");
    let clicks = device.config_click_methods();
    let scrolls = device.config_scroll_methods();
    let profiles = device.config_accel_profiles();
    let can = Can {
        tap: device.config_tap_finger_count() > 0,
        clickfinger: clicks.contains(&ClickMethod::Clickfinger),
        button_areas: clicks.contains(&ClickMethod::ButtonAreas),
        edge: scrolls.contains(&ScrollMethod::Edge),
        two_fingers: scrolls.contains(&ScrollMethod::TwoFinger),
        dwt: device.config_dwt_is_available(),
        natural: device.config_scroll_has_natural_scroll(),
        accel: device.config_accel_is_available(),
        flat: profiles.contains(&AccelProfile::Flat),
        adaptive: profiles.contains(&AccelProfile::Adaptive),
        left_handed: device.config_left_handed_is_available(),
        off_with_mouse: device
            .config_send_events_modes()
            .contains(SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE),
    };
    let click_default = match device.config_click_default_method() {
        Some(ClickMethod::Clickfinger) => Some(RightClick::TwoFingers),
        Some(ClickMethod::ButtonAreas) => Some(RightClick::Corner),
        _ => None,
    };
    Some((kind, bus, built_in, can, click_default))
}

/// udev's `ID_BUS` and `ID_INPUT_TOUCHPAD_INTEGRATION` for `device`.
#[allow(unsafe_code)]
fn udev_words(device: &libinput::Device) -> (Option<String>, Option<String>) {
    // SAFETY: the libinput context was made from udev
    // (`Libinput::new_with_udev`, the tty backend's): the handle is that
    // context's, which the call requires. Its properties are udev's
    // database for the device, on tmpfs, read once as it is plugged.
    let Some(udev) = (unsafe { device.udev_device() }) else {
        return (None, None);
    };
    let word = |name: &str| {
        udev.property_value(name)
            .map(|v| v.to_string_lossy().into_owned())
    };
    (word("ID_BUS"), word("ID_INPUT_TOUCHPAD_INTEGRATION"))
}

/// `given` set on libinput's `device`; what it refuses is logged.
fn apply(device: &mut libinput::Device, given: &Applied) {
    let mut refused = Vec::new();
    let mut check = |what: &'static str, done: libinput::DeviceConfigResult| {
        if done.is_err() {
            refused.push(what);
        }
    };
    if let Some(speed) = given.speed {
        check("speed", device.config_accel_set_speed(speed));
    }
    if let Some(profile) = given.acceleration {
        check(
            "acceleration",
            device.config_accel_set_profile(match profile {
                Acceleration::Adaptive => AccelProfile::Adaptive,
                Acceleration::Flat => AccelProfile::Flat,
            }),
        );
    }
    if let Some(natural) = given.natural {
        check(
            "natural-scroll",
            device.config_scroll_set_natural_scroll_enabled(natural),
        );
    }
    if let Some(left) = given.left_handed {
        check("left-handed", device.config_left_handed_set(left));
    }
    if let Some(tap) = given.tap {
        check("tap-to-click", device.config_tap_set_enabled(tap));
        if tap {
            // One finger left, two right, three middle.
            check(
                "tap-to-click",
                device.config_tap_set_button_map(TapButtonMap::LeftRightMiddle),
            );
        }
    }
    if let Some(drag) = given.drag {
        check("tap-and-drag", device.config_tap_set_drag_enabled(drag));
    }
    if let Some(click) = given.click {
        check(
            "right-click",
            device.config_click_set_method(match click {
                RightClick::TwoFingers => ClickMethod::Clickfinger,
                RightClick::Corner => ClickMethod::ButtonAreas,
            }),
        );
    }
    if let Some(dwt) = given.dwt {
        check("disable-while-typing", device.config_dwt_set_enabled(dwt));
    }
    if let Some(scroll) = given.scroll {
        check(
            "scroll",
            device.config_scroll_set_method(match scroll {
                ScrollWay::TwoFingers => ScrollMethod::TwoFinger,
                ScrollWay::Edge => ScrollMethod::Edge,
            }),
        );
    }
    if let Some(off) = given.off_with_mouse {
        check(
            "off-with-mouse",
            device.config_send_events_set_mode(if off {
                SendEventsMode::DISABLED_ON_EXTERNAL_MOUSE
            } else {
                SendEventsMode::ENABLED
            }),
        );
    }
    if !refused.is_empty() {
        tracing::warn!(
            device = device.name(),
            ?refused,
            "settings the device refused"
        );
    }
}

impl InputDevices {
    /// The devices, as `hocus msg input-devices` says them.
    pub fn said(&self) -> String {
        self.list.iter().map(|p| format!("{}\n", p.said)).collect()
    }

    pub fn list(&self) -> Vec<InputDevice> {
        self.list.iter().map(|p| p.said.clone()).collect()
    }

    /// `event` told to every connection following; one that does not take
    /// it at once is let go (the loop never waits for a reader).
    fn tell(&mut self, event: &DeviceEvent) {
        let line = format!("{event}\n");
        self.followers
            .retain_mut(|stream| stream.write_all(line.as_bytes()).is_ok());
    }
}

impl State {
    /// A device libinput plugged in: given its settings, if it is a mouse
    /// or a touchpad.
    pub fn input_device_added(&mut self, mut device: libinput::Device) {
        let Some((kind, bus, built_in, can, click_default)) = describe(&device) else {
            return;
        };
        let id = self.plug_device(kind, bus, built_in, can, click_default, Some(&mut device));
        tracing::info!(
            id,
            device = device.name(),
            ?kind,
            ?bus,
            "a pointing device plugged in"
        );
        if let Some(plugged) = self.input_devices.list.last_mut() {
            plugged.device = Some(device);
        }
    }

    pub fn input_device_removed(&mut self, device: &libinput::Device) {
        let Some(at) = self
            .input_devices
            .list
            .iter()
            .position(|p| p.device.as_ref() == Some(device))
        else {
            return;
        };
        let gone = self.input_devices.list.remove(at);
        tracing::info!(
            id = gone.said.id,
            device = device.name(),
            "a pointing device unplugged"
        );
        self.input_devices.tell(&DeviceEvent::Removed(gone.said.id));
    }

    /// A test's device plugged in; its number.
    pub fn fake_device_added(&mut self, fake: FakeDevice) -> u64 {
        self.plug_device(
            fake.kind,
            fake.bus,
            fake.bus == Bus::Internal,
            fake.can,
            fake.click_default,
            None,
        )
    }

    pub fn fake_device_removed(&mut self, id: u64) {
        let before = self.input_devices.list.len();
        self.input_devices.list.retain(|p| p.said.id != id);
        if self.input_devices.list.len() < before {
            self.input_devices.tell(&DeviceEvent::Removed(id));
        }
    }

    fn plug_device(
        &mut self,
        kind: DeviceKind,
        bus: Bus,
        built_in: bool,
        can: Can,
        click_default: Option<RightClick>,
        device: Option<&mut libinput::Device>,
    ) -> u64 {
        let (applied, skipped) = settle(kind, can, &self.config.pointer, &self.config.touchpad);
        if !skipped.is_empty() {
            tracing::info!(?kind, ?skipped, "settings this device cannot take: skipped");
        }
        if let Some(device) = device {
            apply(device, &applied);
        }
        let devices = &mut self.input_devices;
        devices.last_id += 1;
        let said = InputDevice {
            id: devices.last_id,
            kind,
            bus,
            built_in,
            can,
            click_default,
            applied,
        };
        devices.tell(&DeviceEvent::Added(said.clone()));
        devices.list.push(Plugged { said, device: None });
        devices.last_id
    }

    /// `[pointer]` or `[touchpad]` changed: every device given them again.
    pub fn input_settings_changed(&mut self) {
        let (pointer, touchpad) = (self.config.pointer, self.config.touchpad);
        let mut changed = Vec::new();
        for plugged in &mut self.input_devices.list {
            let said = &mut plugged.said;
            let (applied, skipped) = settle(said.kind, said.can, &pointer, &touchpad);
            if applied == said.applied {
                continue;
            }
            if !skipped.is_empty() {
                tracing::info!(
                    id = said.id,
                    ?skipped,
                    "settings this device cannot take: skipped"
                );
            }
            if let Some(device) = &mut plugged.device {
                apply(device, &applied);
            }
            said.applied = applied;
            changed.push(DeviceEvent::Changed(said.clone()));
        }
        tracing::info!(
            devices = changed.len(),
            "the pointing devices' settings given again"
        );
        for event in &changed {
            self.input_devices.tell(event);
        }
    }

    /// A connection that follows the devices: told those there, then each
    /// change.
    pub fn follow_input_devices(&mut self, stream: UnixStream) {
        if let Err(err) = stream.set_nonblocking(true) {
            tracing::warn!(%err, "a connection following the devices is let go");
            return;
        }
        let mut lines: String = self
            .input_devices
            .list
            .iter()
            .map(|p| format!("{}\n", DeviceEvent::Added(p.said.clone())))
            .collect();
        lines.push_str(&format!("{}\n", DeviceEvent::Listed));
        let mut stream = stream;
        if stream.write_all(lines.as_bytes()).is_ok() {
            self.input_devices.followers.push(stream);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PAD: Can = Can {
        tap: true,
        clickfinger: true,
        button_areas: false,
        edge: false,
        two_fingers: true,
        dwt: true,
        natural: true,
        accel: true,
        flat: false,
        adaptive: true,
        left_handed: true,
        off_with_mouse: true,
    };

    #[test]
    fn a_touchpad_gets_tap_and_what_it_can_take() {
        let (given, skipped) = settle(
            DeviceKind::Touchpad,
            PAD,
            &Pointer::default(),
            &Touchpad::default(),
        );
        assert_eq!(given.tap, Some(true), "tap on, where libinput says off");
        assert_eq!(given.drag, Some(true));
        assert_eq!(given.click, None, "the pad's own right click");
        assert_eq!(given.scroll, Some(ScrollWay::TwoFingers));
        assert!(skipped.is_empty());
        // Edge scrolling and the corner on a pad without them: skipped.
        let touchpad = Touchpad {
            scroll: ScrollWay::Edge,
            right_click: Some(RightClick::Corner),
            ..Touchpad::default()
        };
        let (given, skipped) = settle(DeviceKind::Touchpad, PAD, &Pointer::default(), &touchpad);
        assert_eq!((given.scroll, given.click), (None, None));
        assert_eq!(skipped, ["right-click", "scroll"]);
    }

    #[test]
    fn a_mouse_gets_the_pointers_table() {
        let mouse = Can {
            accel: true,
            adaptive: true,
            natural: true,
            left_handed: true,
            ..Can::default()
        };
        let pointer = Pointer {
            speed: 0.5,
            acceleration: Acceleration::Flat,
            natural_scroll: true,
            left_handed: true,
        };
        let (given, skipped) = settle(DeviceKind::Pointer, mouse, &pointer, &Touchpad::default());
        assert_eq!(given.speed, Some(0.5));
        assert_eq!(given.acceleration, None, "no flat profile here");
        assert_eq!(skipped, ["acceleration"]);
        assert_eq!((given.natural, given.left_handed), (Some(true), Some(true)));
        assert_eq!(given.tap, None);
    }
}
