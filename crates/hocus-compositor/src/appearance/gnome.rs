//! GNOME's text size, for the GTK applications that read their settings
//! in dconf rather than through the portal ([`super::portal`], #942):
//! `[appearance] text-scale` written as `org.gnome.desktop.interface`
//! `text-scaling-factor` in the session's own settings, through
//! `gsettings`, as the session starts and as the file changes it. It is
//! GNOME's own setting: a GNOME session of the same account takes it too
//! (decided on #942).
//!
//! Only the session on a virtual terminal writes it
//! ([`Config::gnome_settings`](crate::Config::gnome_settings)): a nested or
//! headless compositor never touches the settings of the session it runs
//! in, and tests give a fake program of their own. The program runs on a
//! thread of its own, the values in the order asked, only the last of
//! those waiting: the loop never waits for it.

use std::{
    path::Path,
    sync::mpsc::{self, Receiver, Sender},
};

use hocus_link::TextScale;

use crate::{
    State,
    programs::{Env, on_a_thread},
};

/// GNOME's interface settings, and the text size's key in them.
pub const SCHEMA: &str = "org.gnome.desktop.interface";
pub const KEY: &str = "text-scaling-factor";

/// What was written of GNOME's text size, and the thread that writes it.
#[derive(Debug, Default)]
pub struct GnomeText {
    /// The writing thread's line, once one was asked.
    sender: Option<Sender<f64>>,
    /// The text size last asked to be written.
    written: Option<TextScale>,
}

impl State {
    /// GNOME's text size, written if it is not the one last written: as
    /// the session starts, and when the file changes it.
    pub fn gnome_text_scale(&mut self) {
        let Some(program) = self.config.gnome_settings.clone() else {
            return;
        };
        let text = self.text_scale();
        if self.gnome_text.written == Some(text) {
            return;
        }
        self.gnome_text.written = Some(text);
        let sender = match &self.gnome_text.sender {
            Some(sender) => sender.clone(),
            None => {
                let (sender, receiver) = mpsc::channel();
                let env = Env::of(self);
                on_a_thread("gnome's text size".to_owned(), move || {
                    write(&program, &env, &receiver);
                });
                self.gnome_text.sender = Some(sender.clone());
                sender
            }
        };
        if sender.send(text.factor()).is_err() {
            tracing::warn!("GNOME's text size not written: its thread is gone");
        }
    }
}

/// `factor` as `gsettings` takes a double: "1.30".
pub fn value(factor: f64) -> String {
    format!("{factor:.2}")
}

/// Writes each factor `receiver` gives with `program`, the last of those
/// waiting, until the compositor goes.
fn write(program: &Path, env: &Env, receiver: &Receiver<f64>) {
    while let Ok(mut factor) = receiver.recv() {
        while let Ok(later) = receiver.try_recv() {
            factor = later;
        }
        let value = value(factor);
        let ran = env
            .command(program)
            .args(["set", SCHEMA, KEY, &value])
            .output();
        match ran {
            Ok(out) if out.status.success() => {
                tracing::info!(value, "GNOME's text size written");
            }
            Ok(out) => tracing::warn!(
                value,
                status = %out.status,
                stderr = %String::from_utf8_lossy(&out.stderr).trim(),
                "GNOME's text size not written"
            ),
            Err(err) => tracing::warn!(
                value,
                program = %program.display(),
                %err,
                "GNOME's text size not written"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factor_is_written_as_gsettings_takes_a_double() {
        assert_eq!(value(TextScale(130).factor()), "1.30");
        assert_eq!(value(TextScale::NORMAL.factor()), "1.00");
        assert_eq!(value(TextScale(80).factor()), "0.80");
    }

    /// Nothing is written unless a program is given: only the session on a
    /// virtual terminal gives one (`main.rs`), never the file.
    #[test]
    fn nothing_is_written_without_the_sessions_program() {
        assert_eq!(crate::Config::default().gnome_settings, None);
        let read: crate::Config = toml::from_str("").expect("an empty file");
        assert_eq!(read.gnome_settings, None);
    }
}
