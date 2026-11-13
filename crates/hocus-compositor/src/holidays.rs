//! The garlands proposed in December (#813, docs/design/effects.md): once
//! a year, in December (the wall clock's date, where the user is), the
//! desktop asks "Des guirlandes pour les fêtes ?" in a notification of its
//! own (5.3), Oui / Non. Oui turns both garlands on (`[effects] garland`
//! and `garland-top`, written as the Settings write them); Non does
//! nothing. Either way, not asked again that year: the year it was shown
//! in is kept in `$XDG_STATE_HOME/hocuspanda/garlands-asked`. Not asked
//! with either garland on already, nor with reduced motion.
//!
//! Looked at as the session starts, as the machine wakes and at each
//! midnight; never per frame.

use std::{path::PathBuf, time::Duration};

use jiff::{Zoned, civil::Date};
use smithay::reexports::calloop::{
    RegistrationToken,
    channel::{self, Event, Sender},
    timer::{TimeoutAction, Timer},
};

use crate::{State, shell::Asked};

/// The file keeping the year it was asked in, in the state's folder.
pub const ASKED_FILE: &str = "garlands-asked";
/// Its actions' keys.
const YES: &str = "yes";
const NO: &str = "no";
/// The longest it waits before looking at the date again: the clock may
/// be changed by hand.
const LONGEST: Duration = Duration::from_secs(6 * 3600);

/// What the notification came to, back on the loop.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Answer {
    /// Shown, the year it was asked in.
    Shown(i16),
    Yes,
    /// Non, closed unanswered, or not sent.
    Not,
}

/// The proposal's state on the loop.
#[derive(Debug, Default)]
pub struct Holidays {
    /// The year it was asked in last; `None` while its file was not read.
    asked: Option<Option<i16>>,
    /// A notification out, waiting for its answer.
    asking: bool,
    /// When it looks at the date again.
    timer: Option<RegistrationToken>,
    /// The answers, sent from the notification's thread.
    answers: Option<Sender<Answer>>,
    /// Tests: the wall clock's date, in place of the system's.
    pub(crate) clock: Option<Date>,
}

/// Whether to ask on `date`, the year it was asked in last being `asked`:
/// in December, not asked that year, both garlands off, without reduced
/// motion.
pub fn to_ask(date: Date, asked: Option<i16>, garlands_on: bool, reduced_motion: bool) -> bool {
    date.month() == 12 && asked != Some(date.year()) && !garlands_on && !reduced_motion
}

/// The year in the file's `text`.
fn year_of(text: &str) -> Option<i16> {
    text.trim().parse().ok()
}

/// How long until the next midnight (and a second), on the system's
/// clock, at most [`LONGEST`].
fn until_tomorrow() -> Duration {
    let now = Zoned::now();
    let tomorrow = now
        .date()
        .tomorrow()
        .ok()
        .and_then(|d| d.to_zoned(now.time_zone().clone()).ok());
    let wait = tomorrow
        .map(|t| t.timestamp().as_second() - now.timestamp().as_second() + 1)
        .unwrap_or(3600);
    Duration::from_secs(u64::try_from(wait).unwrap_or(60).max(1)).min(LONGEST)
}

impl State {
    /// Looks at the date: asks when it is time to (#813), and wakes again
    /// at the next midnight. As the session starts, as the machine wakes,
    /// and from its own timer.
    pub(crate) fn holidays_check(&mut self) {
        if let Some(token) = self.holidays.timer.take() {
            self.loop_handle.remove(token);
        }
        let date = self.holidays.clock.unwrap_or_else(|| Zoned::now().date());
        let asked = self.holidays_asked();
        let effects = &self.config.effects;
        if !self.holidays.asking
            && to_ask(
                date,
                asked,
                effects.garland || effects.garland_top,
                self.config.appearance.reduced_motion,
            )
        {
            self.holidays_ask(date.year());
        }
        // Tests hold the date: they move it and look again themselves.
        if self.holidays.clock.is_some() {
            return;
        }
        let inserted =
            self.loop_handle
                .insert_source(Timer::from_duration(until_tomorrow()), |_, _, app| {
                    app.state.holidays.timer = None;
                    app.state.holidays_check();
                    TimeoutAction::Drop
                });
        match inserted {
            Ok(token) => self.holidays.timer = Some(token),
            Err(err) => tracing::warn!(%err.error, "the garlands will not be proposed in December"),
        }
    }

    /// The year it was asked in last: its file read the first time, as
    /// the compositor starts (before its loop runs), then kept.
    fn holidays_asked(&mut self) -> Option<i16> {
        if let Some(asked) = self.holidays.asked {
            return asked;
        }
        let asked = self
            .holidays_file()
            .and_then(|file| match std::fs::read_to_string(&file) {
                Ok(text) => year_of(&text),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
                Err(err) => {
                    tracing::warn!(file = %file.display(), %err, "the garlands' year not read");
                    None
                }
            });
        self.holidays.asked = Some(asked);
        asked
    }

    fn holidays_file(&self) -> Option<PathBuf> {
        Some(self.config.state_dir.as_ref()?.join(ASKED_FILE))
    }

    /// The notification sent, off the loop, on the bus the notifications
    /// are served on; none without one, nor without a configuration file
    /// to write the answer in.
    fn holidays_ask(&mut self, year: i16) {
        let (Some(bus), Some(_)) = (self.shell.bus.clone(), self.config.file.as_ref()) else {
            tracing::debug!(
                "December: no notifications' bus or no file, the garlands not proposed"
            );
            return;
        };
        let Some(answers) = self.holidays_answers() else {
            return;
        };
        tracing::info!(year, "December: the garlands proposed");
        self.holidays.asking = true;
        let lang = self.config.lang;
        use hocus_words::compositor::holidays as words;
        crate::shell::notify_asking(
            bus,
            words::ASK.get(lang).to_owned(),
            vec![
                (YES, words::YES.get(lang).to_owned()),
                (NO, words::NO.get(lang).to_owned()),
            ],
            move |asked| {
                let answer = match asked {
                    Asked::Shown => Answer::Shown(year),
                    Asked::Chose(key) if key == YES => Answer::Yes,
                    Asked::Chose(_) | Asked::Gone => Answer::Not,
                };
                let _ = answers.send(answer);
            },
        );
    }

    /// The channel the notification's thread answers on, made once.
    fn holidays_answers(&mut self) -> Option<Sender<Answer>> {
        if let Some(answers) = &self.holidays.answers {
            return Some(answers.clone());
        }
        let (answers, heard) = channel::channel::<Answer>();
        let inserted = self.loop_handle.insert_source(heard, |event, (), app| {
            if let Event::Msg(answer) = event {
                app.state.holidays_answered(answer);
            }
        });
        if let Err(err) = inserted {
            tracing::warn!(%err.error, "the garlands' proposal would hear no answer");
            return None;
        }
        self.holidays.answers = Some(answers.clone());
        Some(answers)
    }

    fn holidays_answered(&mut self, answer: Answer) {
        match answer {
            Answer::Shown(year) => {
                // Not asked again that year, whatever the answer.
                self.holidays.asked = Some(Some(year));
                if let Some(file) = self.holidays_file() {
                    write_year(file, year);
                }
            }
            Answer::Yes => {
                self.holidays.asking = false;
                tracing::info!("December: the garlands turned on");
                match self.config.file.clone() {
                    Some(file) => write_garlands(file),
                    None => tracing::info!("no configuration file: nothing written"),
                }
            }
            Answer::Not => {
                self.holidays.asking = false;
                tracing::debug!("December: the garlands not wanted, or not asked");
            }
        }
    }
}

/// `year` written into `file`, on a thread of its own.
fn write_year(file: PathBuf, year: i16) {
    let started = std::thread::Builder::new()
        .name("garlands-asked".to_owned())
        .spawn(move || {
            let written = file
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&file, format!("{year}\n")));
            if let Err(err) = written {
                tracing::warn!(file = %file.display(), %err, "the garlands' year not kept");
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to keep the garlands' year");
    }
}

/// `[effects] garland` and `garland-top` turned on in `file`, on a thread
/// of its own, as the Settings write it: the desktop follows as it reads
/// it again.
fn write_garlands(file: PathBuf) {
    let started = std::thread::Builder::new()
        .name("garlands-on".to_owned())
        .spawn(move || {
            let written = hocus_config::edit(
                &file,
                |doc| {
                    use hocus_config::toml_edit::{Item, Table, value};
                    // The user's file: `effects` may be anything there.
                    let effects = doc
                        .entry("effects")
                        .or_insert_with(|| Item::Table(Table::new()))
                        .as_table_like_mut()
                        .ok_or("effects is not a table")?;
                    effects.insert("garland", value(true));
                    effects.insert("garland-top", value(true));
                    Ok(())
                },
                |_| Ok(()),
            );
            if let Err(err) = written {
                tracing::warn!(%err, "could not write the configuration");
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to write the configuration");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asked_in_december_once_a_year_with_both_off_and_motion() {
        let day = |y: i16, m: i8, d: i8| Date::new(y, m, d).unwrap_or_default();
        assert!(to_ask(day(2026, 12, 1), None, false, false));
        assert!(to_ask(day(2026, 12, 31), Some(2025), false, false));
        assert!(!to_ask(day(2026, 11, 30), None, false, false), "November");
        assert!(!to_ask(day(2027, 1, 1), None, false, false), "January");
        assert!(!to_ask(day(2026, 12, 2), Some(2026), false, false), "asked");
        assert!(!to_ask(day(2026, 12, 1), None, true, false), "one on");
        assert!(
            !to_ask(day(2026, 12, 1), None, false, true),
            "reduced motion"
        );
        assert_eq!(year_of("2026\n"), Some(2026));
        assert_eq!(year_of("nonsense"), None);
    }
}
