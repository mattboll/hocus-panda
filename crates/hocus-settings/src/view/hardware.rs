//! The Hardware pages (#67): Sound, Network, Bluetooth, Power and Lock. No
//! board draws them: they keep board R2's template, captioned sections of
//! choices and switch cards. What the system's services hold (the volume,
//! the radios, the power profile) is changed through the compositor, as
//! quick settings change it; the delays are the configuration's.

use hocus_api::HardwareChange;
use hocus_core::{
    Action,
    config::{PowerSource, Sleep},
};

use super::*;

/// Dimming delays offered, seconds; 0 is never.
const DIM: [u32; 5] = [60, 120, 300, 600, 0];
/// Sleeping delays offered.
const SLEEP: [u32; 5] = [900, 1800, 3600, 7200, 0];
/// How long the curtain shows before the lock.
const CURTAIN: [u32; 4] = [10, 30, 60, 120];

/// The Hardware pages between builds: the volume slider, and the volume it
/// last asked for.
#[derive(Debug, Default)]
pub(super) struct HardwarePage {
    pub(super) volume: Option<(NodeId, u8)>,
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

    /// Network: Wi-Fi and airplane mode.
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
        self.note(theme, section, words::NETWORK_NOTE);
    }

    /// Bluetooth: on or off, and the device connected.
    pub(super) fn bluetooth(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        self.tree.set_style(body, fill_width(column(26.0)));
        let unknown = self.unknown(theme, body);
        let h = self.ctx.hardware.clone();

        let section = self.section(theme, body, words::BLUETOOTH);
        let sub = match (&h.bluetooth_device, h.bluetooth) {
            (_, false) => words::BLUETOOTH_SUB_OFF.get(lang).to_owned(),
            (Some(device), true) => words::connected(device, lang),
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
        self.note(theme, section, words::BLUETOOTH_NOTE);
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
