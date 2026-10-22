//! The clock follows the system's time zone (#67): timedated says it at
//! start and whenever it changes (the settings set it there), and, as the
//! session itself, `/etc/localtime` changed by hand. Names heard are read
//! from the database on the follower's threads; the loop only takes the
//! zone ready.

use hocus_core::config::Clock;
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

pub fn start(handle: &LoopHandle<'static, App>, state: &State) {
    // Nested in a live session, the clock keeps the zone it started with:
    // that session's services are the developer's machine's.
    if !state.is_session && state.system_bus.is_none() {
        return;
    }
    let (tx, rx) = channel::channel::<(String, jiff::tz::TimeZone)>();
    if let Err(err) = handle.insert_source(rx, |event, _, app| {
        if let channel::Event::Msg((name, zone)) = event {
            app.timed("time zone", |app| app.state.zone_changed(&name, zone));
        }
    }) {
        tracing::warn!(err = %err.error, "the system's time zone will not be followed");
        return;
    }
    let tx = std::sync::Mutex::new(tx);
    let followed =
        hocus_platform::timedate::follow(state.system_bus.clone(), state.is_session, move |name| {
            // Only names of the database, never a way out of it.
            if !Clock::zone_name_is_valid(&name) {
                tracing::warn!(name, "not a time zone");
                return;
            }
            match jiff::tz::TimeZone::get(&name) {
                Ok(zone) => {
                    if let Ok(tx) = tx.lock() {
                        let _ = tx.send((name, zone));
                    }
                }
                Err(err) => tracing::warn!(name, %err, "unknown time zone"),
            }
        });
    if let Err(err) = followed {
        tracing::warn!(%err, "the system's time zone will not be followed");
    }
}

impl State {
    fn zone_changed(&mut self, name: &str, zone: jiff::tz::TimeZone) {
        if self.wall.zone().iana_name() == Some(name) {
            return;
        }
        tracing::info!(name, "time zone");
        self.wall.set_zone(zone);
        self.needs_redraw = true;
    }
}
