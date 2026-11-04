//! The session menu's lines (7.4, docs/design/bar.md): the compositor
//! decides how the session ends and keeps the time (the countdown, the
//! windows given 5 s to close), reads logind and calls it, off its loop;
//! the shell draws what it is told in the user's popover and says what the
//! user chose.
//!
//! - `leave-can SUSPEND RESTART SHUT-DOWN` ([`LeaveCan`]): what logind says
//!   of each (`yes`, `challenge`: with a password, `no`), sent as the shell
//!   connects and when it changed, read again as the menu opens.
//! - `leave OUTPUT ACTION STEP…` ([`LeaveShown`]): a confirmation under way
//!   on `OUTPUT`'s bar, sent as it starts and on each change; `leave-end`:
//!   none any more, the menu's rows again.
//! - From the shell: `leave open` (the menu opened), `leave ask ACTION
//!   OUTPUT` (a row chosen), `leave suspend`, `leave now` ("… maintenant",
//!   "… quand même"), `leave cancel`, `leave force WINDOW` (a window that
//!   does not close: "Fermer quand même").

use std::{fmt, time::Duration};

use crate::{BadLine, one_line, word};

/// The confirmation counts down this long.
pub const LEAVE_COUNTDOWN: Duration = Duration::from_secs(10);
/// Windows asked to close are waited for this long at most.
pub const LEAVE_CLOSE_WAIT: Duration = Duration::from_secs(5);

/// How the session ends, once confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LeaveAction {
    LogOut,
    Restart,
    ShutDown,
}

impl LeaveAction {
    pub const ALL: [Self; 3] = [Self::LogOut, Self::Restart, Self::ShutDown];

    pub const fn name(self) -> &'static str {
        match self {
            Self::LogOut => "log-out",
            Self::Restart => "restart",
            Self::ShutDown => "shut-down",
        }
    }

    fn read(word: &str) -> Result<Self, BadLine> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == word)
            .ok_or_else(|| BadLine(format!("no leave action {word:?}")))
    }
}

/// What logind says of a power action (`CanSuspend`, `CanReboot`,
/// `CanPowerOff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Can {
    Yes,
    /// It needs a password: polkit asks (7.5's dialog).
    Challenge,
    /// Not allowed, not possible, or no logind to ask.
    #[default]
    No,
}

impl Can {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::Challenge => "challenge",
            Self::No => "no",
        }
    }

    /// logind's answer, as it says it.
    pub fn of_logind(answer: &str) -> Self {
        match answer {
            "yes" => Self::Yes,
            "challenge" => Self::Challenge,
            _ => Self::No,
        }
    }

    fn read(word: &str) -> Result<Self, BadLine> {
        match word {
            "yes" => Ok(Self::Yes),
            "challenge" => Ok(Self::Challenge),
            "no" => Ok(Self::No),
            other => Err(BadLine(format!("no can {other:?}"))),
        }
    }

    /// Whether the user can choose it (with a password or without).
    pub const fn allowed(self) -> bool {
        !matches!(self, Self::No)
    }
}

/// What logind allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeaveCan {
    pub suspend: Can,
    pub restart: Can,
    pub shut_down: Can,
}

impl LeaveCan {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "leave-can {} {} {}",
            self.suspend.name(),
            self.restart.name(),
            self.shut_down.name()
        )
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let can = Self {
            suspend: Can::read(word(words, "suspend")?)?,
            restart: Can::read(word(words, "restart")?)?,
            shut_down: Can::read(word(words, "shut down")?)?,
        };
        if words.next().is_some() {
            return Err(BadLine("a leave-can line too long".to_owned()));
        }
        Ok(can)
    }

    /// What `action` is allowed (logging out always is).
    pub const fn of(&self, action: LeaveAction) -> Can {
        match action {
            LeaveAction::LogOut => Can::Yes,
            LeaveAction::Restart => self.restart,
            LeaveAction::ShutDown => self.shut_down,
        }
    }
}

/// What holds the session's end back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderKind {
    /// A logind inhibitor in `block` mode, held by process `pid` (0: not
    /// known).
    Inhibitor { pid: u32 },
    /// A window still open 5 s after it was asked to close, by its id.
    Window { id: u64 },
}

/// One application holding it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub kind: HolderKind,
    /// The application: a window's app id, an inhibitor's `Who`.
    pub app: String,
    /// What it says: an inhibitor's `Why`, a window's title.
    pub text: String,
}

/// Where the confirmation is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaveStep {
    /// The password asked first (logind said `challenge`): 7.5's dialog
    /// shows.
    Authorizing,
    /// Counting down: this many milliseconds left.
    Counting { left_ms: u32 },
    /// Applications hold it back: nothing counts.
    Held(Vec<Holder>),
    /// Every window was asked to close: waiting for them, 5 s at most.
    Closing,
    /// It happens: logind called, or the session ending.
    Acting,
}

/// A confirmation under way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaveShown {
    /// The bar whose user's popover shows it.
    pub output: String,
    pub action: LeaveAction,
    pub step: LeaveStep,
}

impl LeaveShown {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "leave {} {} ",
            crate::screen_name(&self.output).replace(' ', "_"),
            self.action.name()
        )?;
        match &self.step {
            LeaveStep::Authorizing => f.write_str("authorizing"),
            LeaveStep::Counting { left_ms } => write!(f, "counting {left_ms}"),
            LeaveStep::Closing => f.write_str("closing"),
            LeaveStep::Acting => f.write_str("acting"),
            LeaveStep::Held(holders) => {
                f.write_str("held")?;
                for holder in holders {
                    match holder.kind {
                        HolderKind::Inhibitor { pid } => write!(f, "\tinhibitor {pid}")?,
                        HolderKind::Window { id } => write!(f, "\twindow {id}")?,
                    }
                    write!(
                        f,
                        "\t{}\t{}",
                        one_line(&holder.app).replace('\t', " "),
                        one_line(&holder.text).replace('\t', " ")
                    )?;
                }
                Ok(())
            }
        }
    }

    /// `rest`: what follows `leave `.
    pub(crate) fn read(rest: &str) -> Result<Self, BadLine> {
        let (head, holders) = rest.split_once('\t').unwrap_or((rest, ""));
        let mut words = head.split(' ');
        let output = word(&mut words, "output")?.to_owned();
        let action = LeaveAction::read(word(&mut words, "action")?)?;
        let step = match word(&mut words, "step")? {
            "authorizing" => LeaveStep::Authorizing,
            "closing" => LeaveStep::Closing,
            "acting" => LeaveStep::Acting,
            "counting" => LeaveStep::Counting {
                left_ms: word(&mut words, "milliseconds")?
                    .parse()
                    .map_err(|e| BadLine(format!("milliseconds: {e}")))?,
            },
            "held" => {
                let fields: Vec<&str> = if holders.is_empty() {
                    Vec::new()
                } else {
                    holders.split('\t').collect()
                };
                let (triples, rest) = fields.as_chunks::<3>();
                if !rest.is_empty() {
                    return Err(BadLine("a holder without its words".to_owned()));
                }
                let holders = triples
                    .iter()
                    .map(|[kind, app, text]| {
                        let mut words = kind.split(' ');
                        let what = word(&mut words, "holder")?;
                        let number = word(&mut words, "holder's number")?;
                        let kind = match what {
                            "inhibitor" => HolderKind::Inhibitor {
                                pid: number.parse().map_err(|e| BadLine(format!("pid: {e}")))?,
                            },
                            "window" => HolderKind::Window {
                                id: number
                                    .parse()
                                    .map_err(|e| BadLine(format!("window: {e}")))?,
                            },
                            other => return Err(BadLine(format!("no holder {other:?}"))),
                        };
                        Ok(Holder {
                            kind,
                            app: (*app).to_owned(),
                            text: (*text).to_owned(),
                        })
                    })
                    .collect::<Result<_, BadLine>>()?;
                LeaveStep::Held(holders)
            }
            other => return Err(BadLine(format!("no leave step {other:?}"))),
        };
        if words.next().is_some() {
            return Err(BadLine("a leave line too long".to_owned()));
        }
        Ok(Self {
            output,
            action,
            step,
        })
    }
}

/// What the shell asks of the session's end.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaveAsk {
    /// The menu opened: what logind allows is read again.
    Open,
    /// A row chosen: its confirmation, on `output`'s bar.
    Ask { action: LeaveAction, output: String },
    /// "Mettre en veille": the session locks, then the machine sleeps.
    Suspend,
    /// "… maintenant", or "… quand même": at once, past what holds it.
    Now,
    /// "Annuler", `Esc`, a click outside, the menu closed: nothing ends.
    Cancel,
    /// "Fermer quand même" on a window that does not close: its client is
    /// cut off.
    Force(u64),
}

impl LeaveAsk {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("leave open"),
            Self::Ask { action, output } => write!(
                f,
                "leave ask {} {}",
                action.name(),
                crate::screen_name(output).replace(' ', "_")
            ),
            Self::Suspend => f.write_str("leave suspend"),
            Self::Now => f.write_str("leave now"),
            Self::Cancel => f.write_str("leave cancel"),
            Self::Force(id) => write!(f, "leave force {id}"),
        }
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let ask = match word(words, "leave's ask")? {
            "open" => Self::Open,
            "ask" => Self::Ask {
                action: LeaveAction::read(word(words, "action")?)?,
                output: word(words, "output")?.to_owned(),
            },
            "suspend" => Self::Suspend,
            "now" => Self::Now,
            "cancel" => Self::Cancel,
            "force" => Self::Force(
                word(words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
            ),
            other => return Err(BadLine(format!("no leave ask {other:?}"))),
        };
        if words.next().is_some() {
            return Err(BadLine("a leave ask too long".to_owned()));
        }
        Ok(ask)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    #[test]
    fn the_session_menus_lines_read_back() {
        let shown = [
            ToShell::LeaveCan(LeaveCan {
                suspend: Can::Yes,
                restart: Can::Challenge,
                shut_down: Can::No,
            }),
            ToShell::Leave(LeaveShown {
                output: "eDP-1".into(),
                action: LeaveAction::ShutDown,
                step: LeaveStep::Counting { left_ms: 9400 },
            }),
            ToShell::Leave(LeaveShown {
                output: "eDP-1".into(),
                action: LeaveAction::LogOut,
                step: LeaveStep::Held(vec![
                    Holder {
                        kind: HolderKind::Inhibitor { pid: 4242 },
                        app: "Firefox".into(),
                        text: "Un téléchargement est en cours\tencore".into(),
                    },
                    Holder {
                        kind: HolderKind::Window { id: 7 },
                        app: "org.gnome.TextEditor".into(),
                        text: "Sans titre 1".into(),
                    },
                ]),
            }),
            ToShell::Leave(LeaveShown {
                output: "DP-2".into(),
                action: LeaveAction::Restart,
                step: LeaveStep::Held(Vec::new()),
            }),
            ToShell::Leave(LeaveShown {
                output: "DP-2".into(),
                action: LeaveAction::Restart,
                step: LeaveStep::Authorizing,
            }),
            ToShell::Leave(LeaveShown {
                output: "DP-2".into(),
                action: LeaveAction::Restart,
                step: LeaveStep::Closing,
            }),
            ToShell::Leave(LeaveShown {
                output: "DP-2".into(),
                action: LeaveAction::Restart,
                step: LeaveStep::Acting,
            }),
            ToShell::LeaveEnd,
        ];
        for message in shown {
            let line = message.to_string();
            let read = line.parse::<ToShell>();
            match (&message, &read) {
                // A tab in a reason is a space on the line.
                (
                    ToShell::Leave(LeaveShown {
                        step: LeaveStep::Held(h),
                        ..
                    }),
                    Ok(_),
                ) if h.iter().any(|h| h.text.contains('\t')) => {
                    assert!(line.contains("en cours encore"), "{line}");
                }
                _ => assert_eq!(read, Ok(message), "{line}"),
            }
        }
        for ask in [
            LeaveAsk::Open,
            LeaveAsk::Ask {
                action: LeaveAction::LogOut,
                output: "HEADLESS-1".into(),
            },
            LeaveAsk::Suspend,
            LeaveAsk::Now,
            LeaveAsk::Cancel,
            LeaveAsk::Force(12),
        ] {
            let message = ToCompositor::Leave(ask);
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
        assert!("leave eDP-1 log-out sleeping".parse::<ToShell>().is_err());
        assert!("leave ask reboot eDP-1".parse::<ToCompositor>().is_err());
    }

    #[test]
    fn logging_out_is_always_allowed() {
        let none = LeaveCan::default();
        assert_eq!(none.of(LeaveAction::LogOut), Can::Yes);
        assert_eq!(none.of(LeaveAction::ShutDown), Can::No);
        assert_eq!(Can::of_logind("challenge"), Can::Challenge);
        assert_eq!(Can::of_logind("na"), Can::No);
        assert!(Can::Challenge.allowed() && !Can::No.allowed());
    }
}
