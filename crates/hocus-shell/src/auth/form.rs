//! NetworkManager's secrets asked in the dialog (#930,
//! docs/design/polkit.md, "NetworkManager's secrets"): a VPN's password or one-time code, a
//! Wi-Fi key the computer does not keep. The agent ([`super::nm`]) reads
//! what is asked and hands it here; the dialog shows the connection's
//! name, a field per secret asked (what was held filled in, masked), and
//! "Retenir le mot de passe" where the agent may keep one. Se connecter
//! answers with what was typed; Annuler, `Esc` or two minutes without an
//! answer cancel; NetworkManager may withdraw it.
//!
//! Its surface, its queue and its dim are the system's authentication
//! dialog's ([`super::Auth`]): one dialog at a time, whoever asks.

use std::time::{Duration, Instant};

use hocus_link::AuthAt;
use hocus_network::agent::Asked;
use iced::window;

use super::{Answer, Target};
use crate::{
    field::{self, Entry},
    network::secret::Secret,
};

/// How long a request waits for its answer, from when NetworkManager
/// asked: then the connection stops.
pub const PATIENCE: Duration = Duration::from_secs(120);
/// The characters a field keeps at most.
const LONGEST: usize = 256;

/// How the request ended, for the agent.
#[derive(Debug)]
pub enum Given {
    /// Se connecter: each secret asked by its key, and whether the agent
    /// keeps the ones it may.
    Secrets {
        values: Vec<(String, Secret)>,
        remember: bool,
    },
    /// Annuler, `Esc`, or no answer in time.
    Cancelled,
    /// NetworkManager withdrew it (`CancelGetSecrets`).
    Withdrawn,
}

/// A request from NetworkManager, as the agent read it.
#[derive(Debug, Clone)]
pub struct Ask {
    pub id: u64,
    /// The connection's object path, with the setting: what NetworkManager
    /// withdraws a request by.
    pub path: String,
    pub asked: Asked,
    /// When NetworkManager asked.
    pub since: Instant,
    pub answer: Answer<Given>,
}

impl Ask {
    pub fn deadline(&self) -> Instant {
        self.since + PATIENCE
    }
}

/// The dialog of a request.
#[derive(Debug)]
pub struct Form {
    pub ask: Ask,
    pub surface: Option<window::Id>,
    pub at: Option<AuthAt>,
    /// A field per secret asked, in the request's order.
    pub entries: Vec<Entry>,
    pub remember: bool,
    pub focus: Target,
    pub size: (u32, u32),
}

impl Form {
    pub fn new(ask: Ask) -> Self {
        let entries: Vec<Entry> = ask
            .asked
            .wanted
            .iter()
            .map(|wanted| {
                let kind = if wanted.what.masked() {
                    field::Kind::Secret
                } else {
                    field::Kind::Line
                };
                let mut entry = Entry::new(kind, LONGEST);
                if let Some(known) = wanted.known.as_deref().filter(|_| !ask.asked.new_ones()) {
                    entry.set(known);
                }
                entry
            })
            .collect();
        let focus = first_empty(&entries).map_or(Target::Connect, Target::Entry);
        Self {
            ask,
            surface: None,
            at: None,
            entries,
            remember: true,
            focus,
            size: (0, 0),
        }
    }

    /// Whether every field is filled: Se connecter answers.
    pub fn ready(&self) -> bool {
        first_empty(&self.entries).is_none()
    }

    /// Se connecter: what was typed, if every field is; else the ring goes
    /// to the first empty one.
    pub fn submit(&mut self) -> Option<Given> {
        if let Some(i) = first_empty(&self.entries) {
            self.focus = Target::Entry(i);
            return None;
        }
        let values = self
            .ask
            .asked
            .wanted
            .iter()
            .zip(&self.entries)
            .map(|(wanted, entry)| {
                // A copy for NetworkManager, made on purpose.
                let mut secret = Secret::longest(LONGEST);
                secret.push(entry.text());
                (wanted.key.clone(), secret)
            })
            .collect();
        Some(Given::Secrets {
            values,
            remember: self.remember && self.ask.asked.rememberable(),
        })
    }
}

fn first_empty(entries: &[Entry]) -> Option<usize> {
    entries.iter().position(Entry::is_empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hocus_network::agent::{Keep, Kind, Wanted, What};

    fn ask(wanted: Vec<Wanted>, flags: u32) -> Ask {
        let (answer, _) = tokio::sync::oneshot::channel();
        Ask {
            id: 1,
            path: "/org/freedesktop/NetworkManager/Settings/1".into(),
            asked: Asked {
                uuid: "u".into(),
                name: "VPN Bureau".into(),
                setting: "vpn".into(),
                kind: Kind::Vpn,
                message: None,
                wanted,
                flags,
            },
            since: Instant::now(),
            answer: Answer::new(answer),
        }
    }

    fn wanted(key: &str, what: What, keep: Keep, known: Option<&str>) -> Wanted {
        Wanted {
            key: key.into(),
            what,
            keep,
            known: known.map(str::to_owned),
        }
    }

    #[test]
    fn what_is_held_fills_its_field_and_the_ring_starts_on_the_first_empty_one() {
        let mut form = Form::new(ask(
            vec![
                wanted("password", What::Password, Keep::Agent, Some("panda")),
                wanted("challenge-response", What::Code, Keep::Never, None),
            ],
            1,
        ));
        assert_eq!(form.entries[0].text(), "panda");
        assert_eq!(form.focus, Target::Entry(1));
        assert!(!form.ready());
        assert!(form.submit().is_none());
        form.entries[1].set("123456");
        let Some(Given::Secrets { values, remember }) = form.submit() else {
            panic!("answered");
        };
        assert!(remember);
        let values: Vec<_> = values
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            values,
            [("password", "panda"), ("challenge-response", "123456")]
        );
    }

    #[test]
    fn a_refused_password_is_not_filled_in() {
        let form = Form::new(ask(
            vec![wanted(
                "password",
                What::Password,
                Keep::System,
                Some("old"),
            )],
            3,
        ));
        assert!(form.entries[0].is_empty());
        assert_eq!(form.focus, Target::Entry(0));
    }
}
