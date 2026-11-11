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

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::text::tidy;

/// The confirmation counts down this long.
pub const LEAVE_COUNTDOWN: Duration = Duration::from_secs(10);
/// Windows asked to close are waited for this long at most.
pub const LEAVE_CLOSE_WAIT: Duration = Duration::from_secs(5);

/// How the session ends, once confirmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
}

/// What logind says of a power action (`CanSuspend`, `CanReboot`,
/// `CanPowerOff`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

    /// Whether the user can choose it (with a password or without).
    pub const fn allowed(self) -> bool {
        !matches!(self, Self::No)
    }
}

/// What logind allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LeaveCan {
    pub suspend: Can,
    pub restart: Can,
    pub shut_down: Can,
}

impl LeaveCan {
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HolderKind {
    /// A logind inhibitor in `block` mode, held by process `pid` (0: not
    /// known).
    Inhibitor { pid: u32 },
    /// A window still open 5 s after it was asked to close, by its id.
    Window { id: u64 },
}

/// One application holding it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
    pub kind: HolderKind,
    /// The application: a window's app id, an inhibitor's `Who`.
    #[serde(serialize_with = "tidy::name")]
    pub app: String,
    /// What it says: an inhibitor's `Why`, a window's title.
    #[serde(serialize_with = "tidy::name")]
    pub text: String,
}

/// Where the confirmation is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaveShown {
    /// The bar whose user's popover shows it.
    pub output: String,
    pub action: LeaveAction,
    pub step: LeaveStep,
}

/// What the shell asks of the session's end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

#[cfg(test)]
mod tests {
    use super::*;

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
