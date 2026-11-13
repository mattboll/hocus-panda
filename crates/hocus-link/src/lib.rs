//! The private line between the compositor and its shell (docs/design/bar.md
//! §2, §3, docs/architecture.md): every message either side sends the
//! other ([`ToShell`], [`ToCompositor`]), and what both draw by: the
//! shell's surfaces' namespaces, their measures and durations, the
//! theme's tokens ([`theme`]).
//!
//! The compositor starts the shell with [`SOCKET_ENV`] naming a Unix socket
//! beside its Wayland socket (`<socket>.shell`), which only it listens on.
//! Each message is one JSON object on a line, `{"kind":…,"data":…}`,
//! written and read by serde, after a hello each way that names the
//! protocol's [`VERSION`] ([`wire`]): two sides of different versions do
//! not read each other. The compositor says what it decides (where bars
//! are and in which form, the theme, the screens, desktops and windows,
//! the agents, the keys meant for the shell, the lock, captures, sharing);
//! the shell says what the user did on its surfaces and what it asks of
//! the session.
//!
//! Neither side waits for the other: a message is a fact or a request,
//! never a question. Two requests have an answer: a click on a tray icon
//! asks for an activation token, and a dialog of the desktop portal's over
//! a shell surface asks for that surface's xdg-foreign handle; the
//! compositor sends either at once, and only the thread that asked waits
//! for it, briefly, never the loop nor the shell's surfaces.
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
pub mod mcp;
pub mod moving;
pub mod projects;
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
pub use mcp::{
    MCP_CARD_NAMESPACE, MCP_CARD_RADIUS, MCP_CARD_WAIT, MCP_JOURNAL_SHOWN, MCP_TAG_NAMESPACE,
    MCP_TAG_RADIUS, MCP_TOAST, MCP_TOAST_NAMESPACE, MCP_TOAST_RADIUS, McpAsk, McpAt, McpCard,
    McpDoing, McpKind, McpLine, McpPointer, McpRefusal, McpReply, McpRow, McpSaid, McpSet, McpShow,
    McpShown, McpSummary, McpToast, McpWhy,
};
pub use projects::{
    DeskProject, NAME_MOST, NameFault, PROJECT_CARD_NAMESPACE, PROJECT_CARD_RADIUS,
    PROJECT_CARD_WIDTH, PROJECT_FORM_NAMESPACE, PROJECT_FORM_RADIUS, PROJECT_FORM_WIDTH,
    PROJECT_RISE, ProjectCard, ProjectForm, ProjectRefusal, WindowPlace, is_project_surface,
};
pub use record::{RECORD_COUNTDOWN, RECORD_FPS, RecordAsk, RecordFormat, Recorded, RecordingShown};
pub use share::{
    IN_USE_NAMESPACE, SHARE_CHOOSER_FROM, SHARE_CHOOSER_IN, SHARE_CHOOSER_NAMESPACE,
    SHARE_CHOOSER_RADIUS, SHARE_MONITOR, SHARE_THUMB_RADIUS, SHARE_THUMBS_EVERY, SHARE_WINDOW,
    ShareAsk, ShareChoose, ShareScreen, ShareShown, ShareSource, ShareWindow, SharedThing,
};
pub use window_menu::{MenuScreen, WindowAction, WindowMenu, WindowStates};
