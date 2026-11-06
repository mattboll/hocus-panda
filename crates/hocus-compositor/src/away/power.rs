//! The desktop acting on the Énergie page (6.10, docs/design/settings.md,
//! `[power]`), on the loop, the system bus read and called from its
//! threads ([`super::system`], [`crate::leave::logind`]):
//!
//! - **The delays follow the power source** (UPower's `OnBattery`) and the
//!   file, live: a countdown running keeps the time already spent, only
//!   its deadline moves.
//! - **Sleeping after a while** (`suspend-after`): reached with nothing
//!   holding idleness off ([`crate::idle`]), logind is asked to suspend,
//!   unless its own `idle` inhibitors hold (then the countdown starts
//!   again). The lock before sleep is [`super`]'s, as for any sleep.
//! - **The lid** (`[power] lid`), once logind left it to the compositor
//!   (its `handle-lid-switch` inhibitor): `suspend`, `lock` (the laptop's
//!   panel off), `screen-off`, `nothing`; docked on AC with another screen
//!   on (`lid-ignored-when-docked`), only the panel goes off. Without the
//!   inhibitor logind decides, and the panel goes off as before.
//! - **The power button** (`[power] power-button`, its `handle-power-key`
//!   inhibitor): a press opens the session menu (`ask`), suspends, or
//!   powers off as the session menu does; without the inhibitor the key
//!   is logind's.
//! - **The automatic power saver** (`auto-saver`, `auto-saver-below`): on
//!   battery, at or under the level, power-profiles-daemon's profile goes
//!   to `power-saver` once per discharge, with a notification; back on AC
//!   the profile it replaced comes back, unless the user chose another
//!   meanwhile.

use std::os::fd::OwnedFd;

use hocus_link::{LeaveAsk, ToShell};

use crate::{
    State,
    config::{Lid, PowerButton},
    leave::logind::Ask,
    shell::Bus,
};

use super::{Felt, is_internal, system::ToSystem};

/// power-profiles-daemon's power saver.
const SAVER: &str = "power-saver";
/// The power button's key (evdev's `KEY_POWER`), as XKB numbers it.
const KEY_POWER: u32 = 116 + 8;

/// The automatic power saver, through one discharge.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Saver {
    /// It may come on.
    #[default]
    Ready,
    /// It came on, in place of this profile.
    On { replaced: String },
    /// It came on and the user chose another profile since, or the
    /// profile was the power saver already: not again until on AC.
    Spent,
}

/// What the loop keeps of the power's services.
#[derive(Debug, Default)]
pub struct Power {
    /// logind's `block` inhibitor of the lid and the power button: held,
    /// the compositor handles them.
    handles: Option<OwnedFd>,
    /// The battery's level in percent, if there is a battery.
    level: Option<f64>,
    /// power-profiles-daemon's profile, if it is there.
    profile: Option<String>,
    saver: Saver,
}

impl State {
    /// logind left the lid and the power button to the compositor.
    pub(crate) fn power_handles(&mut self, fd: OwnedFd) {
        tracing::info!("logind leaves the lid and the power button to the desktop");
        self.away.power.handles = Some(fd);
    }

    /// Whether the compositor handles the lid and the power button (else
    /// logind does).
    pub fn lid_handled(&self) -> bool {
        self.away.power.handles.is_some()
    }

    pub(crate) fn power_level(&mut self, level: Option<f64>) {
        self.away.power.level = level;
        self.power_saver_check();
    }

    pub(crate) fn power_profile(&mut self, profile: Option<String>) {
        tracing::debug!(?profile, "the power profile");
        if matches!(self.away.power.saver, Saver::On { .. })
            && profile.as_deref().is_some_and(|p| p != SAVER)
        {
            tracing::info!(
                ?profile,
                "another profile chosen: the power saver forgets its own"
            );
            self.away.power.saver = Saver::Spent;
        }
        self.away.power.profile = profile;
        self.power_saver_check();
    }

    /// power-profiles-daemon's profile now, if it is there.
    pub fn power_profile_now(&self) -> Option<&str> {
        self.away.power.profile.as_deref()
    }

    pub fn power_saver(&self) -> &Saver {
        &self.away.power.saver
    }

    /// The delays of the power source now, from the file: each countdown
    /// keeps where it runs from, only its end moves.
    pub(crate) fn power_delays_changed(&mut self) {
        let battery = self.away.battery;
        self.idle_set_delay(self.config.idle_delay(battery));
        self.idle_set_suspend(self.config.suspend_after(battery));
        self.away_check();
    }

    /// `[idle]` or `[power]` changed on disk: followed at once.
    pub(crate) fn power_config_changed(&mut self, new: &crate::Config) -> bool {
        if new.idle == self.config.idle && new.power == self.config.power {
            return false;
        }
        self.config.idle = new.idle;
        self.config.power = new.power;
        self.power_log_old_keys();
        self.power_delays_changed();
        self.power_saver_check();
        true
    }

    /// 6.1's keys the file still has, logged as it is read.
    pub(crate) fn power_log_old_keys(&self) {
        let old = self.config.power.old_keys();
        if !old.is_empty() {
            tracing::info!(?old, "6.1's [power] keys, read as 6.10's");
        }
    }

    /// No activity for `suspend-after`: logind is asked to suspend, unless
    /// its `idle` inhibitors hold.
    pub(crate) fn power_idle_suspend(&mut self) {
        tracing::info!("no activity for the delay before sleeping: the machine sleeps");
        self.leave_logind(Ask::Sleep {
            unless_idle_held: true,
        });
    }

    // --- The lid ------------------------------------------------------------------

    /// The lid closed or opened: what `[power] lid` says; opened, the
    /// panel on again and the user there.
    pub fn lid(&mut self, closed: bool) {
        if self.away.lid_closed == closed {
            return;
        }
        tracing::info!(closed, "the lid");
        self.away.lid_closed = closed;
        if !closed {
            self.set_lid_dark(false);
            self.felt(Felt::Other);
            return;
        }
        if !self.lid_handled() {
            // logind decides (it may sleep); the panel goes off.
            self.set_lid_dark(true);
            return;
        }
        let power = self.config.power;
        let docked = power.lid_ignored_when_docked
            && !self.away.battery
            && self
                .space
                .outputs()
                .any(|o| !is_internal(o) && !self.display_off(o));
        if docked {
            tracing::info!("docked on AC: only the laptop's panel goes off");
            self.set_lid_dark(true);
            return;
        }
        match power.lid {
            Lid::Suspend => {
                self.set_lid_dark(true);
                self.leave_logind(Ask::Sleep {
                    unless_idle_held: false,
                });
            }
            Lid::Lock => {
                self.set_lid_dark(true);
                if !self.locked() {
                    self.lock_now();
                }
            }
            Lid::ScreenOff => self.set_lid_dark(true),
            Lid::Nothing => {}
        }
    }

    // --- The power button -----------------------------------------------------------

    /// A key: whether it is the power button's, taken here (`[power]
    /// power-button`) rather than going on. A press acts; its release is
    /// taken too.
    pub(crate) fn power_key(&mut self, code: u32, pressed: bool) -> bool {
        if code != KEY_POWER || !self.lid_handled() {
            return false;
        }
        if !pressed {
            return true;
        }
        let button = self.config.power.power_button;
        tracing::info!(?button, "the power button");
        match button {
            PowerButton::Ask => {
                if self.locked() {
                    return true;
                }
                let output = self
                    .bubbles_screen()
                    .or_else(|| self.space.outputs().next().cloned())
                    .map(|o| o.name());
                if let Some(output) = output {
                    self.tell_shell(ToShell::SessionMenu { output });
                }
            }
            PowerButton::Suspend => self.leave_asked(LeaveAsk::Suspend),
            PowerButton::PowerOff => self.leave_power_off(),
            PowerButton::Nothing => {}
        }
        true
    }

    // --- The automatic power saver ----------------------------------------------------

    /// On battery at or under the level: the power saver, once per
    /// discharge. On AC: the profile it replaced back.
    pub(crate) fn power_saver_check(&mut self) {
        let power = &mut self.away.power;
        if !self.away.battery {
            if let Saver::On { replaced } = &power.saver
                && power.profile.as_deref() == Some(SAVER)
            {
                tracing::info!(
                    replaced,
                    "on AC: the profile the power saver replaced comes back"
                );
                let back = ToSystem::Profile(replaced.clone());
                power.saver = Saver::Ready;
                self.to_system(back);
                return;
            }
            power.saver = Saver::Ready;
            return;
        }
        if !self.config.power.auto_saver || power.saver != Saver::Ready {
            return;
        }
        let (Some(level), Some(profile)) = (power.level, power.profile.clone()) else {
            return;
        };
        let below = self.config.power.auto_saver_below;
        if level > f64::from(below) {
            return;
        }
        if profile == SAVER {
            power.saver = Saver::Spent;
            return;
        }
        tracing::info!(
            level,
            below,
            replaced = profile,
            "the automatic power saver comes on"
        );
        power.saver = Saver::On { replaced: profile };
        self.to_system(ToSystem::Profile(SAVER.to_owned()));
        self.power_notify(level);
    }

    fn to_system(&self, ask: ToSystem) {
        if let Some(to) = &self.away.sleep.to_system {
            let _ = to.send(ask);
        }
    }

    /// "Économie d'énergie activée : batterie à 20 %.", through the
    /// session bus's notifications (5.3), off the loop.
    fn power_notify(&self, level: f64) {
        let Some(bus) = self.shell.bus.clone() else {
            return;
        };
        let percent = level.round() as u32;
        let summary = if self.config.french {
            format!("Économie d’énergie activée : batterie à {percent} %.")
        } else {
            format!("Power saver on: battery at {percent} %.")
        };
        let started = std::thread::Builder::new()
            .name("power saver: notification".to_owned())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                match runtime {
                    Ok(runtime) => runtime.block_on(notify(bus, summary)),
                    Err(err) => tracing::warn!(%err, "no runtime for the notification"),
                }
            });
        if let Err(err) = started {
            tracing::warn!(%err, "the power saver's notification is not sent");
        }
    }

    // --- What it says ------------------------------------------------------------------

    /// `hocus msg idle`'s lines of it.
    pub(crate) fn power_said(&self) -> String {
        let power = &self.away.power;
        let saver = match &power.saver {
            Saver::Ready => "ready".to_owned(),
            Saver::On { replaced } => format!("on {replaced}"),
            Saver::Spent => "spent".to_owned(),
        };
        format!(
            "lid {} {}\nlevel {}\nprofile {}\nsaver {saver}\n",
            if self.away.lid_closed {
                "closed"
            } else {
                "open"
            },
            if self.lid_handled() {
                "desktop"
            } else {
                "system"
            },
            power
                .level
                .map_or_else(|| "none".to_owned(), |l| format!("{l:.0}")),
            power.profile.as_deref().unwrap_or("none"),
        )
    }
}

/// One notification, `org.freedesktop.Notifications.Notify` on `bus`.
async fn notify(bus: Bus, summary: String) {
    let builder = match &bus {
        Bus::Given(address) => zbus::connection::Builder::address(address.as_str()),
        Bus::Session => zbus::connection::Builder::session(),
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "no session bus: the power saver's notification is not sent");
            return;
        }
    };
    let hints: std::collections::HashMap<&str, zbus::Variant<'_>> =
        std::collections::HashMap::new();
    let sent = connection
        .call_method(
            Some("org.freedesktop.Notifications"),
            "/org/freedesktop/Notifications",
            Some("org.freedesktop.Notifications"),
            "Notify",
            &(
                "Hocuspanda",
                0u32,
                "battery-caution",
                summary.as_str(),
                "",
                Vec::<&str>::new(),
                hints,
                -1i32,
            ),
        )
        .await;
    if let Err(err) = sent {
        tracing::warn!(%err, "the power saver's notification is not sent");
    }
}
