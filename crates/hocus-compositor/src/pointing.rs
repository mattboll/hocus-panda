//! Mice and touchpads on a session of its own: libinput sets them as the
//! configuration's `[mouse]` and `[touchpad]` say, when they come and
//! whenever the file changes. In a window of another session, that one's
//! settings apply; headless, there is no device.

use hocus_core::{Config, config::Pointing};
use smithay::reexports::input::{Device, DeviceCapability, DeviceConfigResult};

/// The pointing devices plugged, and what they were last set to.
#[derive(Debug, Default)]
pub struct PointingDevices {
    devices: Vec<Device>,
    /// A touchpad's settings and a mouse's, as applied last.
    applied: Option<(Pointing, Pointing)>,
}

impl PointingDevices {
    /// A device came: set at once if it points.
    pub fn added(&mut self, mut device: Device, config: &Config) {
        if !device.has_capability(DeviceCapability::Pointer) {
            return;
        }
        configure(&mut device, config);
        self.devices.push(device);
    }

    pub fn removed(&mut self, device: &Device) {
        self.devices.retain(|d| d != device);
    }

    /// After a turn of the loop: when the file changed what they are set
    /// to, every device follows.
    pub fn settle(&mut self, config: &Config) {
        let wanted = (config.pointing(true), config.pointing(false));
        if self.applied == Some(wanted) {
            return;
        }
        self.applied = Some(wanted);
        for device in &mut self.devices {
            configure(device, config);
        }
    }
}

/// A touchpad is what can tap.
fn configure(device: &mut Device, config: &Config) {
    let touchpad = device.config_tap_finger_count() > 0;
    let p = config.pointing(touchpad);
    let name = device.name().to_owned();
    let said = |what: &str, result: DeviceConfigResult| {
        if let Err(err) = result {
            tracing::warn!(device = name, ?err, "could not set {what}");
        }
    };
    if device.config_scroll_has_natural_scroll() {
        said(
            "natural scrolling",
            device.config_scroll_set_natural_scroll_enabled(p.natural_scroll),
        );
    }
    if device.config_left_handed_is_available() {
        said("the hand", device.config_left_handed_set(p.left_handed));
    }
    if device.config_accel_is_available() {
        said("the speed", device.config_accel_set_speed(p.speed));
    }
    if let Some(tap) = p.tap_to_click {
        said("tap to click", device.config_tap_set_enabled(tap));
    }
    if let Some(dwt) = p.disable_while_typing
        && device.config_dwt_is_available()
    {
        said("disable while typing", device.config_dwt_set_enabled(dwt));
    }
    tracing::info!(device = name, touchpad, "pointing device set");
}
