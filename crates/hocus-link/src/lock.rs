//! The lock screen's lines (7.1, docs/design/lock.md): the compositor
//! locks, keeps the keys, the password and its check through PAM; the
//! shell draws what it is told, one surface a screen, and says what the
//! pointer did on it.
//!
//! - `lock PHASE CHARS caps|nocaps SHAKE FOCUS AGENTS wallpaper|plain
//!   motion|still OUTPUT` ([`LockShown`]): the lock as it is, sent as it
//!   locks, as the shell connects while it is locked, and on each change.
//!   The password itself never crosses: only how many characters it has.
//! - `unlocked`: the lock has opened and its motion ended; the shell's
//!   lock surfaces go.
//! - From the shell: `lock submit` (the arrow clicked), `lock focus
//!   field|button|layout` (one of them clicked), `lock now` (the session
//!   menu's "Verrouiller", 7.4). The layout's button sends `next-layout`.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// The namespace of the lock's layer surfaces, one on each screen, over
/// everything. The compositor draws them, from the shell's own connection
/// only, and nothing else while the session is locked.
pub const LOCK_NAMESPACE: &str = "hocus-lock";
/// The lock fades in over the desktop over this, out-cubic (motion's
/// `STANDARD`); [`LOCK_FADE_REDUCED`] with reduced motion or a shell on
/// the CPU.
pub const LOCK_FADE: Duration = Duration::from_millis(250);
pub const LOCK_FADE_REDUCED: Duration = Duration::from_millis(120);
/// Unlocked, its two halves slide apart over this, in-out (motion's
/// `LARGE`); at once with reduced motion or a shell on the CPU.
pub const LOCK_PART: Duration = Duration::from_millis(400);
/// A wrong password shakes the field over this.
pub const LOCK_SHAKE: Duration = Duration::from_millis(360);
/// How long the field waits after PAM said there were too many attempts.
pub const LOCK_WAIT: Duration = Duration::from_secs(30);

/// Where the lock's form is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockPhase {
    /// Nothing typed.
    Idle,
    /// Something typed: a dot a character.
    Typing,
    /// The password is being checked: the field takes nothing.
    Checking,
    /// The last password was wrong; the field is empty again.
    Wrong,
    /// PAM said too many attempts: the field waits this many seconds more.
    Waiting(u32),
    /// The right password: the lock opens.
    Opening,
}

impl LockPhase {
    /// Whether the field takes what is typed.
    pub const fn takes_keys(self) -> bool {
        matches!(self, Self::Idle | Self::Typing | Self::Wrong)
    }
}

/// What has the keyboard on the lock: `Tab` goes from the field to the
/// button, then to the layout's button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockFocus {
    #[default]
    Field,
    Button,
    Layout,
}

impl LockFocus {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Field => "field",
            Self::Button => "button",
            Self::Layout => "layout",
        }
    }

    /// The next one `Tab` goes to (`back`: `Shift+Tab`), the layout's
    /// button only when there is a choice of layouts.
    pub const fn next(self, back: bool, layouts: bool) -> Self {
        match (self, back, layouts) {
            (Self::Field, false, _) => Self::Button,
            (Self::Button, false, true) => Self::Layout,
            (Self::Button, false, false) | (Self::Layout, false, _) => Self::Field,
            (Self::Field, true, true) => Self::Layout,
            (Self::Field, true, false) | (Self::Layout, true, _) => Self::Button,
            (Self::Button, true, _) => Self::Field,
        }
    }
}

/// The lock as the shell draws it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockShown {
    pub phase: LockPhase,
    /// The characters typed, drawn as dots; never the characters.
    pub chars: u32,
    /// Caps Lock is on, as the keyboard's state says on every key.
    pub caps: bool,
    /// The wrong passwords so far: a new one shakes the field again.
    pub shake: u32,
    pub focus: LockFocus,
    /// How many agents wait for the user: a count, no name.
    pub agents: u32,
    /// The desktop's background is drawn (else its plain colour).
    pub wallpaper: bool,
    /// The compositor draws the wallpaper under the lock, with the
    /// effects playing on it (#807): the lock leaves its background clear,
    /// its veil over it.
    #[serde(default)]
    pub live: bool,
    /// Things move (no reduced motion): the field shakes.
    pub motion: bool,
    /// The screen with the form (the bar's, the primary); the others show
    /// the clock alone.
    pub output: String,
}

/// What the shell asks of the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LockAsk {
    /// The arrow was clicked: what was typed is checked.
    Submit,
    /// The field, the button or the layout's button was clicked.
    Focus(LockFocus),
    /// The session menu's "Verrouiller" (7.4): lock now.
    Now,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tab_goes_round_the_field_the_button_and_the_layout() {
        use LockFocus::*;
        assert_eq!(Field.next(false, true), Button);
        assert_eq!(Button.next(false, true), Layout);
        assert_eq!(Layout.next(false, true), Field);
        assert_eq!(Button.next(false, false), Field);
        assert_eq!(Field.next(true, true), Layout);
        assert_eq!(Field.next(true, false), Button);
        assert_eq!(Button.next(true, true), Field);
    }
}
