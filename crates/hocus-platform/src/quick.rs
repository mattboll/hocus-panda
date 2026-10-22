//! What quick settings show and change: sound, brightness per screen,
//! network, Bluetooth, power profile. A service runs on its own thread;
//! changes go in, the new state comes out.
//!
//! Until the session work (M3) brings PipeWire, the backlight, the network
//! and Bluetooth daemons and power profiles, a stub keeps the state in
//! memory and changes nothing on the system.

use std::sync::mpsc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    Saver,
    #[default]
    Balanced,
    Performance,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// Minutes left while discharging.
    pub minutes_left: Option<u32>,
}

/// The state quick settings show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub wifi: bool,
    /// The network joined, when connected.
    pub network: Option<String>,
    pub bluetooth: bool,
    /// The device connected, when there is one.
    pub bluetooth_device: Option<String>,
    pub airplane: bool,
    pub profile: Profile,
    /// 0 to 100.
    pub volume: u8,
    pub muted: bool,
    pub output_device: String,
    pub input_device: String,
    /// Each screen by its connector name, 0 to 100; screens without a
    /// backlight are left out.
    pub brightness: Vec<(String, u8)>,
    pub battery: Option<Battery>,
}

/// What the user changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Wifi(bool),
    Bluetooth(bool),
    Airplane(bool),
    Profile(Profile),
    Volume(u8),
    Muted(bool),
    Brightness(String, u8),
}

impl State {
    /// What a change does to what is shown, before the system confirms.
    pub fn apply_change(&mut self, change: Change) {
        match change {
            Change::Wifi(on) => self.wifi = on,
            Change::Bluetooth(on) => self.bluetooth = on,
            Change::Airplane(on) => {
                self.airplane = on;
                if on {
                    self.wifi = false;
                    self.bluetooth = false;
                }
            }
            Change::Profile(p) => self.profile = p,
            Change::Volume(v) => self.volume = v.min(100),
            Change::Muted(m) => self.muted = m,
            Change::Brightness(screen, v) => {
                let v = v.min(100);
                match self.brightness.iter_mut().find(|(s, _)| *s == screen) {
                    Some((_, b)) => *b = v,
                    // A screen plugged after the start.
                    None => self.brightness.push((screen, v)),
                }
            }
        }
    }
}

/// A plausible state for the stub: what a laptop with one external screen
/// would show.
pub fn stub_state(screens: &[String]) -> State {
    State {
        wifi: true,
        network: None,
        bluetooth: false,
        bluetooth_device: None,
        airplane: false,
        profile: Profile::Balanced,
        volume: 40,
        muted: false,
        output_device: String::new(),
        input_device: String::new(),
        brightness: screens.iter().map(|s| (s.clone(), 80)).collect(),
        battery: None,
    }
}

/// The service: changes are sent here, states come back through `sink`.
#[derive(Debug, Clone)]
pub struct Quick {
    inner: Inner,
}

#[derive(Debug, Clone)]
enum Inner {
    Stub(mpsc::Sender<Change>),
    System(crate::system::System),
}

impl Quick {
    /// Starts the stub service from `state`.
    pub fn stub(mut state: State, sink: impl Fn(State) + Send + 'static) -> std::io::Result<Self> {
        let (changes, received) = mpsc::channel();
        std::thread::Builder::new()
            .name("quick settings".into())
            .spawn(move || {
                sink(state.clone());
                for change in received {
                    state.apply_change(change);
                    sink(state.clone());
                }
            })?;
        Ok(Self {
            inner: Inner::Stub(changes),
        })
    }

    /// The system's services (UPower, NetworkManager, PipeWire…).
    pub fn system(
        buses: crate::system::Buses,
        screens: &[String],
        sink: impl Fn(State) + Send + 'static,
    ) -> std::io::Result<Self> {
        Ok(Self {
            inner: Inner::System(crate::system::System::start(buses, screens, sink)?),
        })
    }

    pub fn change(&self, change: Change) {
        match &self.inner {
            Inner::Stub(changes) => {
                let _ = changes.send(change);
            }
            Inner::System(system) => system.change(change),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airplane_mode_turns_radios_off() {
        let mut s = stub_state(&["eDP-1".into()]);
        s.bluetooth = true;
        s.apply_change(Change::Airplane(true));
        assert!(!s.wifi && !s.bluetooth);
        s.apply_change(Change::Brightness("eDP-1".into(), 120));
        assert_eq!(s.brightness, vec![("eDP-1".into(), 100)]);
    }

    #[test]
    fn the_stub_answers_each_change_with_the_new_state() -> std::io::Result<()> {
        let (tx, rx) = mpsc::channel();
        let quick = Quick::stub(stub_state(&[]), move |s| {
            let _ = tx.send(s);
        })?;
        let first = rx.recv_timeout(std::time::Duration::from_secs(2));
        assert_eq!(first.map(|s| s.volume), Ok(40));
        quick.change(Change::Volume(70));
        let next = rx.recv_timeout(std::time::Duration::from_secs(2));
        assert_eq!(next.map(|s| s.volume), Ok(70));
        Ok(())
    }
}
