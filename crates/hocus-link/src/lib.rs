//! The private line between the compositor and its shell (docs/design/bar.md
//! §2, §3): the bar's measures both draw by, and the few messages they
//! exchange.
//!
//! The compositor starts the shell with [`SOCKET_ENV`] naming a Unix socket
//! in the session's runtime directory, which only it listens on. Each
//! message is one JSON object on a line, `{"kind":…,"data":…}`, after a
//! hello each way that names the protocol's [`VERSION`] ([`wire`]): two
//! sides of different versions do not read each other. The compositor
//! decides where bars are and in which
//! form, reserves their space and takes the clicks on their edge, passes on
//! the theme and the clock's settings, the level keys (volume, brightness) pressed, the
//! presses outside the shell's surfaces while a popover shows, the
//! names of the screens besides the built-in one, the keyboard's layouts
//! and which one is active, the bars' screens' desktops with a sketch of
//! their windows, their applications and their titles, the agents'
//! tools and the agents, the agent whose panda peeks out of the bar,
//! `Super+Space` for the launcher, what clients copy and `Super+V` for the
//! clipboard's history, `Super+.` for the emoji picker and where it opens,
//! what the screen capture's toolbar shows and what was captured, the
//! screen the notifications' bubbles show on, a request to share the screen
//! and what is shared, where the system's authentication dialog goes and
//! Caps Lock's state, the lock, the session menu's confirmation and what
//! logind allows, and a change asked of the day's tasks (for tests);
//! the shell draws the bars, their popovers, the peek, the launcher and the
//! bubbles, keeps the levels and the clipboard's history, and says when it
//! draws on the CPU, when a popover, the launcher or a bubble closes, and
//! when the user asks for the next layout, another desktop or a window,
//! plugs a tool in or out, declines them, copies a text, pastes or copies
//! an item of the clipboard's history, inserts an emoji, starts an
//! application, clicks a bubble (its application's window), the
//! capture's toolbar, chooses what to share, opens and closes the
//! authentication dialog, or chooses in the session menu.
//! Neither
//! side waits for the other: a message is a fact or a request, never a
//! question. Two requests have an answer: a click on a tray icon asks for
//! an activation token, and a dialog of the desktop portal's over a shell
//! surface asks for that surface's xdg-foreign handle; the compositor
//! sends either at once, and only the thread that asked waits for it,
//! briefly, never the loop nor the shell's surfaces.
//!
//! The form switch is one motion, which both sides compute from the same
//! start on the same clock (`CLOCK_MONOTONIC`, in microseconds): the
//! compositor moves the windows and the glass behind the bar, the shell
//! draws the bar, each in its own frames, with no message per frame.

pub mod actions;
mod agents;
mod auth;
pub mod backdrop;
mod bar;
mod capture;
mod clip;
mod curtain;
mod desktops;
pub mod input;
mod leave;
pub mod locale;
mod lock;
pub mod moving;
mod record;
mod share;
mod surfaces;
#[cfg(test)]
mod tests;
mod text;
pub mod theme;
mod to_compositor;
mod to_shell;
pub mod wallpaper;
mod window_menu;
pub mod wire;

pub use agents::*;
pub use bar::*;
pub use clip::*;
pub use desktops::*;
pub use surfaces::*;
pub use text::*;
pub use to_compositor::*;
pub use to_shell::*;
pub use wire::VERSION;

pub use actions::{
    ACTIONS_CLOSE, ACTIONS_DIM, ACTIONS_DROP, ACTIONS_GLOW_ALPHA, ACTIONS_GLOW_WIDTH,
    ACTIONS_NAMESPACE, ACTIONS_OPEN, ACTIONS_RADIUS, ACTIONS_RING, ACTIONS_RING_WIDTH, ACTIONS_TOP,
    ACTIONS_WIDTH, ActionsShown, RunAction,
};
pub use auth::{
    AUTH_AGENT_ENV, AUTH_DIM, AUTH_DIM_ALPHA, AUTH_DIM_IN, AUTH_FROM, AUTH_IN, AUTH_NAMESPACE,
    AUTH_NEXT, AUTH_OUT, AUTH_RADIUS, AUTH_TOP, AUTH_WIDTH, AuthAsk, AuthAt,
};
pub use capture::{
    CAPTURE_DELAYS, CAPTURE_EDITOR_IN, CAPTURE_EDITOR_NAMESPACE, CAPTURE_THUMB_IN,
    CAPTURE_THUMB_NAMESPACE, CAPTURE_THUMB_RADIUS, CAPTURE_THUMB_RISE, CAPTURE_TOOLBAR_BOTTOM,
    CAPTURE_TOOLBAR_IN, CAPTURE_TOOLBAR_NAMESPACE, CAPTURE_TOOLBAR_RADIUS, CaptureAsk, CaptureBar,
    CaptureMode, Captured,
};
pub use curtain::{
    CURTAIN, CURTAIN_DIM, CURTAIN_DIM_ALPHA, CURTAIN_DIM_POWER, CURTAIN_FRAME, CURTAIN_LIFT,
    CURTAIN_NAMESPACE, CurtainShown, PILL_HEIGHT, PILL_IN, PILL_RADIUS, PILL_RISE, PILL_TOP,
    PILL_WIDTH,
};
pub use leave::{
    Can, Holder, HolderKind, LEAVE_CLOSE_WAIT, LEAVE_COUNTDOWN, LeaveAction, LeaveAsk, LeaveCan,
    LeaveShown, LeaveStep,
};
pub use lock::{
    LOCK_FADE, LOCK_FADE_REDUCED, LOCK_NAMESPACE, LOCK_PART, LOCK_SHAKE, LOCK_WAIT, LockAsk,
    LockFocus, LockPhase, LockShown,
};
pub use record::{RECORD_COUNTDOWN, RECORD_FPS, RecordAsk, RecordFormat, Recorded, RecordingShown};
pub use share::{
    IN_USE_NAMESPACE, SHARE_CHOOSER_FROM, SHARE_CHOOSER_IN, SHARE_CHOOSER_NAMESPACE,
    SHARE_CHOOSER_RADIUS, SHARE_MONITOR, SHARE_THUMB_RADIUS, SHARE_THUMBS_EVERY, SHARE_WINDOW,
    ShareAsk, ShareChoose, ShareScreen, ShareShown, ShareSource, ShareWindow, SharedThing,
};
pub use window_menu::{MenuScreen, WindowAction, WindowMenu, WindowStates};
