//! The private line between the compositor and its shell (docs/design/bar.md
//! §2, §3): the bar's measures both draw by, and the few messages they
//! exchange.
//!
//! The compositor starts the shell with [`SOCKET_ENV`] naming a Unix socket
//! in the session's runtime directory, which only it listens on. Each
//! message is one line of text, words separated by spaces (the screens'
//! names by tabs). The compositor decides where bars are and in which
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

use std::{fmt, str::FromStr, time::Duration};

pub mod actions;
mod auth;
pub mod backdrop;
mod capture;
mod curtain;
pub mod input;
mod leave;
pub mod locale;
mod lock;
pub mod moving;
mod record;
mod share;
pub mod theme;
pub mod wallpaper;
mod window_menu;
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

/// The environment variable naming the compositor's socket for its shell.
pub const SOCKET_ENV: &str = "HOCUS_SHELL_SOCKET";

/// The bar's layer surface's namespace.
pub const NAMESPACE: &str = "hocus-bar";
/// The namespace of the clock's own layer surface, over the bar's top: the
/// clock draws every second, and a surface of its own is what keeps that
/// to its region (bar.md §12), since iced presents a surface whole.
pub const CLOCK_NAMESPACE: &str = "hocus-bar-clock";
/// The namespace of the levels' surface (sound, brightness, battery), near
/// the bar's bottom: for the same reason as the clock's, a change of level
/// draws only that region.
pub const LEVELS_NAMESPACE: &str = "hocus-bar-levels";
/// The namespace of the connections' surface (Wi-Fi, Bluetooth), above
/// the levels: a change of connection draws only that region, and a change
/// of level does not draw it.
pub const CONNECTIONS_NAMESPACE: &str = "hocus-bar-connections";
/// The namespace of the desktops' surface (their tiles, or their dots in
/// the mini form), above the connections: a desktop's change draws only
/// that region, and nothing else's draws it.
pub const DESKTOPS_NAMESPACE: &str = "hocus-bar-desktops";
/// The namespace of the agents' group's surface, under the to-do list
/// (bar.md §4 group 3, 4.0; #599): the agents' changes draw only that
/// region.
pub const AGENTS_NAMESPACE: &str = "hocus-bar-agents";
/// The namespace of the to-do list's surface (bar.md §4 group 5, Day.4):
/// the group under Today in the full form, its item under the meeting's
/// mark in the mini form, the agents' over them (#599). A change of the tasks draws only that region; its field
/// takes the keyboard while the user types a task.
pub const TASKS_NAMESPACE: &str = "hocus-bar-tasks";
/// The namespace of a popover's layer surface, beside the bar (bar.md §9):
/// one at a time. The compositor blurs behind it as behind the bar, draws
/// its shadow, and moves it in and out ([`POPOVER_OPEN`], [`POPOVER_CLOSE`]).
pub const POPOVER_NAMESPACE: &str = "hocus-bar-popover";
/// The namespace of the secretary's drawer (docs/design/secretary.md
/// §5.1, assistant mode): against the bar, its top and bottom on the
/// bar's, over the windows. The compositor blurs behind it, draws its
/// shadow, and moves it in and out ([`DRAWER_OPEN`], [`DRAWER_CLOSE`]).
pub const SECRETARY_NAMESPACE: &str = "hocus-secretary";
/// The drawer's widths: [`DRAWER_WIDTH`], [`DRAWER_NARROW`] on a screen
/// under [`DRAWER_NARROW_BELOW`] logical pixels wide, and
/// [`DRAWER_WIDE`] for "Ce qu'elle sait".
pub const DRAWER_WIDTH: u32 = 420;
pub const DRAWER_NARROW: u32 = 360;
pub const DRAWER_NARROW_BELOW: u32 = 1600;
pub const DRAWER_WIDE: u32 = 720;
/// It comes in over this, fading and sliding [`DRAWER_SHIFT`] from the
/// bar (`ease-out`), and goes over [`DRAWER_CLOSE`] (`ease-in`); with
/// reduced motion or a shell on the CPU, it only fades.
pub const DRAWER_OPEN: Duration = Duration::from_millis(180);
pub const DRAWER_CLOSE: Duration = Duration::from_millis(140);
pub const DRAWER_SHIFT: f64 = 16.0;

/// The drawer's width on a screen `screen` logical pixels wide.
pub const fn drawer_width(screen: u32) -> u32 {
    if screen < DRAWER_NARROW_BELOW {
        DRAWER_NARROW
    } else {
        DRAWER_WIDTH
    }
}

/// The namespace of the panda peeking out of the bar's edge (bar.md §7 C,
/// 4.5): one surface beside each bar while it is out, none at rest. The
/// shell draws it once; the compositor moves it out and back.
pub const PEEK_NAMESPACE: &str = "hocus-bar-peek";
/// The namespace of the launcher's layer surface (5.1): over everything of
/// one screen, the bar too, while it is open. The shell draws the field
/// and the grid; the compositor darkens and blurs what is behind, and
/// fades and scales it in and out ([`LAUNCHER_OPEN`], [`LAUNCHER_CLOSE`]).
pub const LAUNCHER_NAMESPACE: &str = "hocus-launcher";
/// The namespace of the launcher's toast (5.2), "Copié : 6,214" at the
/// bottom of its screen once a result is copied. The shell draws it once
/// and removes it after [`LAUNCHER_TOAST`]; the compositor fades it in
/// rising and fades it out, from its first frame.
pub const LAUNCHER_TOAST_NAMESPACE: &str = "hocus-launcher-toast";
/// How long the launcher's toast shows, its fades included.
pub const LAUNCHER_TOAST: Duration = Duration::from_millis(1800);
/// The namespace of the emoji picker's layer surface (5.7): under the
/// focused application's caret, else centred on its window or the screen,
/// holding the keyboard while it is open. The shell draws it; the
/// compositor blurs behind it, draws its shadow and moves it in and out, as
/// a popover's ([`EMOJI_RISE`]).
pub const EMOJI_NAMESPACE: &str = "hocus-emoji";
/// The emoji picker is drawn this many times its mockup's sizes, the
/// launcher's (5.1b).
pub const EMOJI_ZOOM: f64 = LAUNCHER_ZOOM;
/// Its corners, logical pixels: the mockup's 14 px, zoomed.
pub const EMOJI_RADIUS: f64 = 14.0 * EMOJI_ZOOM;
/// It comes down this far (logical pixels) as it fades in, over
/// [`POPOVER_OPEN`], and goes back up as it fades out: the mockup's
/// `translateY(-6px)`, zoomed.
pub const EMOJI_RISE: f64 = 6.0 * EMOJI_ZOOM;
/// The most bytes an emoji inserted holds (a family with its tones is 35).
pub const EMOJI_MAX: usize = 64;

/// Whether `text` can be an emoji inserted: on one word, without spaces
/// nor control characters, [`EMOJI_MAX`] bytes at most.
pub fn emoji_text(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= EMOJI_MAX
        && !text.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// The namespaces of a notification's bubble (5.3), one surface each, its
/// number after the prefix ([`bubble_namespace`]): the compositor moves
/// each in and out on its own ([`BUBBLE_IN`], [`BUBBLE_OUT`]), and the
/// shell names one in its `wake` and `closing` lines.
pub const BUBBLE_PREFIX: &str = "hocus-bubble-";
/// The namespace of the "+N waiting" chip under the bubbles.
pub const BUBBLES_WAITING_NAMESPACE: &str = "hocus-bubbles-waiting";
/// The namespace of the bar's footer (bar.md §4 group 11, 5.5): its
/// buttons along the bar's bottom, the clipboard, the notifications' bell,
/// the menu and the user in the full form, the bell over the user in the
/// mini form. The bell's badge counts what the user has not seen, its
/// slash says they are not to be disturbed.
pub const FOOTER_NAMESPACE: &str = "hocus-bar-footer";
/// The bell's (5.4), on the footer's surface since 5.5.
pub const BELL_NAMESPACE: &str = FOOTER_NAMESPACE;
/// The namespace of the tray (bar.md §4 group 11, 5.14): the
/// applications' icons (StatusNotifierItem) just above the footer, a row
/// in the full form, a column in the mini. Opened while an icon shows,
/// none without: an icon's change draws that region alone.
pub const TRAY_NAMESPACE: &str = "hocus-bar-tray";
/// The namespace of a tray icon's tooltip (5.14), beside the bar: it
/// takes no pointer and no keyboard, and stays over the windows as the
/// popovers do.
pub const TIP_NAMESPACE: &str = "hocus-bar-tip";
/// The namespace of the next meeting (bar.md §4 group 2, §5; Day.2): its
/// card under the clock in the full form, its mark in the mini. Opened
/// while a meeting is left today, none without.
pub const MEETING_NAMESPACE: &str = "hocus-bar-meeting";
/// The namespace of Today (bar.md §4 group 4; Day.3): today's events after
/// the next meeting's, under its card, over the to-do list and the
/// agents' group, in the full form. Opened
/// while it lists something, none without.
pub const TODAY_NAMESPACE: &str = "hocus-bar-today";
/// The namespace of the windows in the bar (#604): the windows of the
/// screen's current desktop over its desktops' tiles, or the mini form's
/// item over its dots. Opened while it lists a window, none without.
pub const WINDOWS_NAMESPACE: &str = "hocus-bar-windows";
/// The namespace of the music player's tile (s12): what an MPRIS player
/// plays, over the desktops in the full form. Kept with its bar, drawing
/// nothing while no player is there.
pub const MUSIC_NAMESPACE: &str = "hocus-bar-music";
/// The height of the windows' and the music player's surfaces while they
/// show nothing, logical pixels: they are kept with their bar (one made
/// later would take the keyboard's keys, #617), and are not a pixel (at a
/// fractional scale tiny-skia refuses a buffer that small). A shell that
/// draws on the CPU has no alpha in its buffers, so the compositor leaves
/// out a surface this short ([`shows_nothing`]): it would show black.
pub const IDLE_HEIGHT: i32 = 10;

/// Whether a surface of `namespace`, `height` tall, is one kept while it
/// shows nothing ([`IDLE_HEIGHT`]).
pub fn shows_nothing(namespace: &str, height: i32) -> bool {
    matches!(namespace, WINDOWS_NAMESPACE | MUSIC_NAMESPACE) && height <= IDLE_HEIGHT
}
/// The tooltip's corners, logical pixels.
pub const TIP_RADIUS: f64 = 9.0;

/// A bubble slides in from the bar's side over [`BUBBLE_IN`] (the 5.3
/// mockup's `cubic-bezier(.2, .8, .2, 1)`), fading in, from this far
/// (logical pixels); out over [`BUBBLE_OUT`], `ease-in`. With reduced
/// motion, or a shell on the CPU, it fades only.
pub const BUBBLE_SHIFT: f64 = 26.0;
pub const BUBBLE_IN: Duration = Duration::from_millis(220);
pub const BUBBLE_OUT: Duration = Duration::from_millis(170);

/// Bubble `n`'s namespace.
pub fn bubble_namespace(n: u64) -> String {
    format!("{BUBBLE_PREFIX}{n}")
}

/// Whether `namespace` is a bubble's.
pub fn is_bubble(namespace: &str) -> bool {
    namespace
        .strip_prefix(BUBBLE_PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The namespaces of a meeting's reminder (Day.6): its tab beside a bar,
/// one surface for each tab on each bar, its number after the prefix
/// ([`reminder_namespace`]). Above the windows, a fullscreen one too; the
/// compositor moves each in and out as a bubble ([`slides_in`]).
pub const REMINDER_PREFIX: &str = "hocus-reminder-";
/// A reminder's corners, logical pixels.
pub const REMINDER_RADIUS: f64 = 12.0;

/// Reminder tab `n`'s namespace.
pub fn reminder_namespace(n: u64) -> String {
    format!("{REMINDER_PREFIX}{n}")
}

/// Whether `namespace` is a reminder's.
pub fn is_reminder(namespace: &str) -> bool {
    namespace
        .strip_prefix(REMINDER_PREFIX)
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether the compositor moves a surface of `namespace` in and out as a
/// bubble ([`BUBBLE_IN`], [`BUBBLE_OUT`]): a bubble's, or a reminder's.
pub fn slides_in(namespace: &str) -> bool {
    is_bubble(namespace) || is_reminder(namespace)
}

/// The environment variable naming the session bus the shell serves the
/// notifications on (`org.freedesktop.Notifications`), set by the
/// compositor: the bus given with `--session-bus`, else as the session,
/// the session's own (`session`). Without it, the shell serves none: a
/// nested or headless compositor never takes the live session's.
pub const NOTIFICATIONS_BUS_ENV: &str = "HOCUS_NOTIFICATIONS_BUS";

/// The environment variable naming the configuration file the compositor
/// read, for the shell's own sections (`[day]`, docs/design/day.md); empty:
/// none. Unset (the shell alone): `$XDG_CONFIG_HOME/hocuspanda/config.toml`.
pub const CONFIG_ENV: &str = "HOCUS_CONFIG";

/// A popover's width, its distance from the bar, and how far it slides from
/// the bar as it opens and closes (logical pixels). Its corners are the
/// bar's ([`RADIUS`]).
pub const POPOVER_WIDTH: i32 = 280;
pub const POPOVER_SHIFT: f64 = 8.0;
/// A popover fades and slides in over this, CSS's `ease-out` (`cubic-bezier(0,
/// 0, 0.58, 1)`), and out over [`POPOVER_CLOSE`], `ease-in` (`cubic-bezier(0.42,
/// 0, 1, 1)`); neither with reduced motion, nor for a shell on the CPU.
pub const POPOVER_OPEN: Duration = Duration::from_millis(160);
pub const POPOVER_CLOSE: Duration = Duration::from_millis(120);

/// The launcher fades in over this, CSS's `ease-out`, its panel growing
/// from [`LAUNCHER_SCALE`] to its size on the mockup's
/// `cubic-bezier(0.2, 0.8, 0.2, 1)`; it fades out over [`LAUNCHER_CLOSE`],
/// `ease-in`, shrinking back. With reduced motion it only fades.
pub const LAUNCHER_OPEN: Duration = Duration::from_millis(160);
pub const LAUNCHER_CLOSE: Duration = Duration::from_millis(120);
pub const LAUNCHER_SCALE: f64 = 0.97;
/// What tints the desktop behind the launcher (#518): the bar's dark blue
/// (bar.md §13, `rgba(22,27,42,…)` to `rgba(14,17,28,…)`, top to bottom),
/// about 85 % opaque over the blur, so that its grid reads over a white
/// page and the desktop still shows faintly.
pub const LAUNCHER_TINT: [([u8; 3], f32); 2] = [([22, 27, 42], 0.82), ([14, 17, 28], 0.88)];
/// The launcher is drawn this many times the 5.1 and 5.2 mockups' sizes
/// (#518): every size of its field, its grid, its cards and its toast.
pub const LAUNCHER_ZOOM: f64 = 1.5;
/// The launcher's panel (its field and its grid), logical pixels: centred
/// across its screen, [`LAUNCHER_TOP`] from its top, reaching
/// [`LAUNCHER_BOTTOM`] from its bottom, the 5.1 mockup's ×
/// [`LAUNCHER_ZOOM`].
pub const LAUNCHER_TOP: f64 = 54.0 * LAUNCHER_ZOOM;
pub const LAUNCHER_BOTTOM: f64 = 98.0 * LAUNCHER_ZOOM;

/// The middle of the launcher's panel on a screen `width` × `height`
/// logical pixels: what it grows from and shrinks to.
pub fn launcher_middle(width: f64, height: f64) -> (f64, f64) {
    let bottom = (height - LAUNCHER_BOTTOM).max(LAUNCHER_TOP);
    (width / 2.0, (LAUNCHER_TOP + bottom) / 2.0)
}

/// The bar's distance from the output's top, right and bottom edges, and
/// the windows' from the bar (logical pixels).
pub const EDGE: i32 = 8;
/// The bar's corner radius.
pub const RADIUS: f64 = 14.0;
/// An output at least this wide without the bar has the full form.
pub const FULL_FROM: i32 = 1600;
/// The form switch's duration; its curve is [`ease`].
pub const SWITCH: Duration = Duration::from_millis(240);

/// The bar's two forms (§3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    Full,
    Mini,
}

impl Form {
    /// The bar's width in this form, logical pixels.
    pub const fn width(self) -> i32 {
        match self {
            Self::Full => 196,
            Self::Mini => 56,
        }
    }

    pub const fn other(self) -> Self {
        match self {
            Self::Full => Self::Mini,
            Self::Mini => Self::Full,
        }
    }

    /// The default form on an output `width` logical pixels wide: full when
    /// what is left beside the full bar is at least [`FULL_FROM`].
    pub const fn default_for(width: i32) -> Self {
        if width - Self::Full.width() >= FULL_FROM {
            Self::Full
        } else {
            Self::Mini
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Mini => "mini",
        }
    }
}

impl FromStr for Form {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "full" => Ok(Self::Full),
            "mini" => Ok(Self::Mini),
            _ => Err(BadLine(format!("no form {s:?}"))),
        }
    }
}

/// The side of the screens the bar stands on (`[panel] side`, 6.4): its
/// edge zone, the room kept for it and what opens from it go with it, the
/// popovers on its inner side.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Side {
    #[default]
    Right,
    Left,
}

impl Side {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }

    /// Which way is away from the screen's edge the bar stands on, along
    /// x: toward the windows. `-1` with the bar on the right.
    pub const fn inward(self) -> f64 {
        match self {
            Self::Right => -1.0,
            Self::Left => 1.0,
        }
    }
}

impl FromStr for Side {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "right" => Ok(Self::Right),
            "left" => Ok(Self::Left),
            _ => Err(BadLine(format!("no side {s:?}"))),
        }
    }
}

/// What the compositor tells the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToShell {
    /// Says what the shell shows, for screen readers (`hocus msg a11y`,
    /// docs/design/screen-reader.md): answered by [`ToCompositor::A11yLine`]s
    /// and an [`ToCompositor::A11yEnd`].
    A11y,
    /// How bars look: on glass the compositor blurs behind (else opaque),
    /// whether they move (else reduced motion), and whether the pandas
    /// play their moods (#649: not with reduced motion nor in power saver).
    /// Written `look glass|opaque motion|still lively|calm`.
    Look {
        glass: bool,
        motion: bool,
        lively: bool,
    },
    /// The theme and the accent the shell draws in (`[appearance] theme`
    /// and `accent`, #402): sent as the shell connects and when the file
    /// changes them, or the primary screen's wallpaper while the accent is
    /// the wallpaper's (6.3). Written `theme SCHEME ACCENT WALLPAPER`.
    Theme(theme::Choice),
    /// The desktop's language and formats (`[locale]`, #402): sent as the
    /// shell connects and when the file changes them. Written `locale
    /// LANGUAGE DATE TIME`.
    Locale(locale::Locale),
    /// Each screen's wallpaper and the lock's (`[wallpaper]`, 6.3): sent
    /// as the shell connects and when the file or the screens change them.
    /// Written `wallpaper`, a space, then [`wallpaper::Wallpapers`]' tabs.
    Wallpaper(wallpaper::Wallpapers),
    /// A bar on `output`, in `form`; `since`: when it switched to it, in
    /// microseconds of `CLOCK_MONOTONIC` (0: it shows in that form at once);
    /// `scale`: its screen's, in 120ths as its surfaces' fractional scale
    /// says it (iced hands the wheel's notches multiplied by it).
    Bar {
        output: String,
        form: Form,
        since: u64,
        scale: u32,
    },
    /// No bar on `output` any more.
    NoBar { output: String },
    /// How the clock looks. The font's name comes last: it may hold
    /// spaces.
    Clock(ClockLook),
    /// A level key was pressed (or repeats, held): the shell, which keeps
    /// the sound and the backlight, changes them.
    Level(LevelKey),
    /// A press on none of the shell's surfaces while a popover shows, or
    /// while one of the shell's surfaces holds the keyboard: the shell
    /// closes the popover, or lets the keyboard go. A popover that holds
    /// the keyboard is a menu: the compositor keeps that press from what
    /// is under it. The shell sees the presses on its own surfaces itself.
    Outside,
    /// The screens other than the one the backlight lights (the built-in
    /// panel; without one, the first screen), by the names people know
    /// them by: their EDID's model, else their connector's. Sent as the
    /// shell connects and when they change: the brightness's popover says
    /// that they are set with their own buttons. Written `screens`, then
    /// each name after a tab (a name may hold spaces, never a tab).
    Screens(Vec<String>),
    /// The keyboard's layouts, in the keymap's order (XKB's groups): sent
    /// as the shell connects. Written `layouts`, then for each a tab, its
    /// code, a tab, its name.
    Layouts(Vec<KeyLayout>),
    /// The active layout, its index in [`ToShell::Layouts`]' list: sent as
    /// the shell connects and whenever it changes (the bar's row, an XKB
    /// group-switch option). Written `layout N`.
    Layout(u32),
    /// A bar's screen's desktops: sent as the shell connects, and when they
    /// change, at most once a frame (bar.md, 3.9).
    Desktops(Desktops),
    /// The titles of a bar's screen's windows, for the list a desktop's
    /// tile opens on hover (3.9b): apart from [`ToShell::Desktops`], so
    /// that a title that changes often (a terminal's) draws only an open
    /// list, never the tiles. Sent as the shell connects, and when one
    /// changes, at most once a frame. Written `titles`, the screen's name,
    /// then for each window a tab, its id, a tab, its title (on one line,
    /// without tabs, [`TITLE_MAX`] characters at most).
    Titles(Titles),
    /// A bar's screen's windows, every desktop's, minimised ones too, the
    /// most recently focused first, by their ids: the order of the
    /// windows in the bar (#604). Sent as the shell connects, and when it
    /// changes, at most once a frame. Written `recent`, the screen's name,
    /// then the ids: `recent eDP-1 12 3 13`.
    Recent(Recent),
    /// The screens whose desktops bar `bar` shows, one row of tiles each
    /// (8.4): its own first, then the others left to right, then top to
    /// bottom. Sent as the shell connects and when they change; each one's
    /// desktops come on its own `desktops` line. Written `desk-screens`,
    /// the bar's screen, then each screen as [`DeskScreen`] says.
    DeskScreens {
        bar: String,
        screens: Vec<DeskScreen>,
    },
    /// A window being moved is over desktop `desktop` (from 0) of screen
    /// `output` in bar `bar`'s tiles, or over none of them any more (8.5
    /// B): that tile lights, as where the window would go. Written
    /// `drop-over BAR OUTPUT N`, or `drop-over BAR -`.
    DropOver {
        bar: String,
        over: Option<(String, u8)>,
    },
    /// Whether the bar shows its screen's windows (`[panel] windows`,
    /// #604): sent as the shell connects and when the configuration
    /// changes it. Written `bar-windows shown` or `bar-windows hidden`.
    BarWindows(bool),
    /// The desktops' names (`[desktops] names`, 6.14), by position, the
    /// same on every screen; an empty one or one missing is "Bureau N":
    /// sent as the shell connects and when the configuration changes
    /// them. Written `desktop-names`, then for each a tab and its name (on
    /// one line, without tabs).
    DesktopNames(Vec<String>),
    /// Which of the bar's column's groups show, and the order of those
    /// that move (`[panel] meeting`, `today`, `tasks`, `agents`, `order`,
    /// 6.4): sent as the shell connects and when the configuration changes
    /// them. Written `bar-groups`, the order, then the hidden groups (`-`
    /// for none): `bar-groups tasks,today,agents meeting,today`.
    BarGroups(BarGroups),
    /// The side the bars stand on (`[panel] side`, 6.4): sent as the shell
    /// connects, before the bars, and when the configuration changes it,
    /// each bar then made again on that side (`nobar`, then `bar`).
    /// Written `bar-side left` or `bar-side right`.
    BarSide(Side),
    /// The agents' tools on this computer and what each is to the desktop
    /// (4.0): sent as the shell connects and when they change. Written
    /// `agent-tools`, then for each a tab, its name, a space, its state, a
    /// space and the file plugging it in changes (as the user knows it,
    /// `~/.claude/settings.json`; none for herdr).
    AgentTools(Vec<AgentTool>),
    /// The agents followed, and what each is doing: sent as the shell
    /// connects and when they change, the changes of a turn of the
    /// compositor's loop together (4.0). Written `agents`, then for each a
    /// tab and its [`AgentShown`] fields.
    Agents(Vec<AgentShown>),
    /// An agent's panda peeks out of the bar's edge (4.5): it started
    /// waiting, finished, or waits still at its reminder. Sent after the
    /// [`ToShell::Agents`] line that says so; the latest replaces the one
    /// out. Written `peek ID waiting|done SINCE NAME`.
    Peek(Peek),
    /// The panda is back in the bar: its surfaces go. Written `no-peek`.
    NoPeek,
    /// A screen went, or came back, and windows moved with it (windows.md
    /// §12): the winking panda peeks out with a blue bubble that says so,
    /// in place of a peek out unless that is an agent's, which it never
    /// replaces; it goes back as a peek does (`no-peek`). Written
    /// `screens-moved gone|back built-in|external INCHES DESKTOP`, then
    /// for each window a tab and its application's id (`-` for none);
    /// `INCHES` 0 unknown, `DESKTOP` from 1, 0 when they are on several.
    ScreensMoved(ScreensMoved),
    /// Whether the agents' rows show each agent's panda, or a dot of its
    /// state's colour (`[agents] pandas`, 6.16): sent as the shell
    /// connects and when the configuration changes it. Written
    /// `agent-rows pandas` or `agent-rows dots`.
    AgentPandas(bool),
    /// The screen of the focused window, else the pointer's: where the
    /// notifications' bubbles show (5.3). Sent as the shell connects and
    /// when it changes. Written `focus OUTPUT`.
    Focus(String),
    /// `Super+Space` (5.1): the launcher opens on `output`, the screen
    /// under the pointer, whose scale is `scale` (120ths, as
    /// [`ToShell::Bar`]'s); open, it closes. Written `launcher OUTPUT
    /// SCALE`.
    Launcher { output: String, scale: u32 },
    /// A client took the clipboard (5.6): what it offered, read off the
    /// compositor's loop, and the application of the window focused as it
    /// copied (its id; empty: none), for the history. Never a selection
    /// marked secret. Written `copied APP KIND DATA`, the application `-`
    /// for none, the data in base64.
    Copied { app: String, clip: Clip },
    /// A capture (5.8b) for the clipboard's history: its PNG, the
    /// compositor's `capture`th of the session. The history lists it as
    /// copied from [`CAPTURE_APP`]; the same capture told again (its
    /// annotated image, 5.9) replaces the item it made. Written
    /// `capture-copied CAPTURE KIND DATA`, the data in base64.
    CaptureCopied { capture: u64, clip: Clip },
    /// `Super+V` (5.6): the clipboard's history opens on `output`'s bar,
    /// the screen of the focused window, else the pointer's, its field
    /// holding the keyboard; open, it closes. Written `clipboard OUTPUT`.
    Clipboard { output: String },
    /// What the clipboard's history keeps (`[clipboard] history` and
    /// `keep`, 6.17): sent as the shell connects and when the
    /// configuration changes them. Written `clipboard-rules on 30d`.
    ClipboardRules(ClipRules),
    /// The Settings' "Effacer l'historique" (6.17): all but the pinned
    /// items go, as the history's own "Tout effacer". Written
    /// `clipboard-clear`.
    ClipboardClear,
    /// The screen capture is open (5.8), or what its toolbar shows changed
    /// ([`CaptureBar`]).
    Capture(CaptureBar),
    /// The capture's toolbar goes: the capture is taken, counts down, or
    /// was cancelled. Written `capture-end`.
    CaptureEnd,
    /// A capture was taken, copied and saved: its thumbnail, or its toast
    /// when it was not saved ([`Captured`]).
    Captured(Captured),
    /// The emoji picker's shortcut (5.7, `Super+.`): it opens on `output`
    /// (its scale in 120ths, its size `width` × `height` logical pixels)
    /// at `at`, or closes when it is open. Written `emoji OUTPUT SCALE
    /// WIDTH HEIGHT caret X Y W H`, `… window X Y W H` or `… screen`.
    Emoji {
        output: String,
        scale: u32,
        width: i32,
        height: i32,
        at: EmojiAt,
    },
    /// An emoji inserted with the picker kept open (`Shift+Enter`) went
    /// into its window, or could not: the picker takes the keyboard back.
    /// Written `emoji-inserted`.
    EmojiInserted,
    /// A request to share the screen (5.11): the chooser opens on its
    /// screen, or shows its sources anew ([`ShareChoose`]).
    ShareChoose(ShareChoose),
    /// The chooser of that request goes. Written `share-choose-end
    /// REQUEST`.
    ShareChooseEnd(u64),
    /// Where the system's authentication dialog goes (7.5, [`AuthAt`]).
    AuthAt(AuthAt),
    /// Caps Lock is on, or off (7.5): sent as it changes and as the shell
    /// connects. Written `caps on` or `caps off`.
    CapsLock(bool),
    /// What is shared now (5.11): sent as the shell connects and when it
    /// changes; empty, nothing is ([`ShareShown`]). Written `shares`, then
    /// each after a tab.
    Shares(Vec<ShareShown>),
    /// A recording under way (5.10): the bar's item, its time and whether
    /// it is paused ([`RecordingShown`]); sent as it starts, pauses and
    /// goes on, and as the shell connects.
    Recording(RecordingShown),
    /// No recording any more: the bar's item goes. Written
    /// `recording-end`.
    RecordingEnd,
    /// A recording saved: its thumbnail ([`Recorded`]).
    Recorded(Recorded),
    /// The activation token for the tray's click `ask`
    /// ([`ToCompositor::TrayClick`], 5.14b): the shell gives it to the
    /// item before activating it. Written `tray-token ASK TOKEN`.
    TrayToken { ask: u64, token: String },
    /// The xdg-foreign handle made for the shell's surface asked for in
    /// `ask` ([`ToCompositor::Export`], #595), `None` without such a
    /// surface: the shell names it to the desktop portal as its dialog's
    /// parent (`wayland:HANDLE`). Written `exported ASK HANDLE`, `-` for
    /// none.
    Exported { ask: u64, handle: Option<String> },
    /// A change asked of the day's tasks, or a read of every account
    /// (docs/design/day.md): what the bar's own views ask, for tests and
    /// tools that have no view to click. Written `day add TITLE`, `day tick
    /// ID`, `day untick ID`, `day refresh`, `day slept SECONDS`, `day calendar
    /// DATE [EVENT]`.
    Day(DayAsk),
    /// `Super+J` (`[shortcuts] join`, Day.6): the meeting whose reminder
    /// shows is joined, else the one starting next within 10 min; nothing
    /// without one. Written `join`.
    Join,
    /// The secretary's process (docs/design/secretary.md §16): sent as the
    /// shell connects and whenever it changes. The shell talks to her on
    /// her own socket, on a thread of its own. Written `secretary off`,
    /// `secretary unavailable`, or `secretary ready` and her socket's path.
    Secretary(SecretaryShown),
    /// A sentence put to her through the shell, as the drawer's field
    /// does: for tests and tools. Written `secretary-ask TEXT`.
    SecretaryAsk(String),
    /// A program given for this session in place of an installed one
    /// (`hocus msg shell restart --program`, docs/workflow.md) did not
    /// start or kept ending, and the installed one runs again: the shell
    /// says so in a notification. Written `fell-back shell|secretary
    /// PROGRAM`.
    FellBack {
        which: Supervised,
        program: std::path::PathBuf,
    },
    /// `Super+S` (`[shortcuts] secretary`, assistant mode only): her
    /// drawer opens beside the bar of `output` (the focused window's
    /// screen), `width` logical pixels wide, or closes. Written
    /// `secretary-drawer OUTPUT WIDTH`.
    SecretaryDrawer { output: String, width: u32 },
    /// Her peek of request `ID` comes out (secretary.md §6.2), as the
    /// shell asked ([`ToCompositor::SecretaryPeek`]), in place of a panda
    /// out: the shell draws it beside each bar on [`PEEK_NAMESPACE`]
    /// surfaces, which the compositor moves as an agent's; `no-peek` when
    /// she is back. Written `secretary-peek ID`.
    SecretaryPeek(u64),
    /// The screens whose front window is full screen, which covers their
    /// bar (Day.6: a meeting's reminder then shows at the screen's edge):
    /// sent as the shell connects and when they change. Written
    /// `fullscreen`, then each screen's name after a tab.
    Fullscreen(Vec<String>),
    /// The screens shared now (5.11), by their connectors' names: a
    /// meeting's reminder hides its title on them (Day.6). Sent with
    /// [`ToShell::Shares`]. Written `shared-screens`, then each name after
    /// a tab.
    SharedScreens(Vec<String>),
    /// The session is locked (7.1, [`LockShown`]): the lock's surfaces on
    /// every screen, as told. Sent as it locks, on each change, and as the
    /// shell connects while it is locked.
    Lock(LockShown),
    /// The lock opened and its motion ended: its surfaces go. Written
    /// `unlocked`.
    Unlocked,
    /// The curtain before the lock is up (7.2, [`CurtainShown`]): the
    /// pill's surface on the screen told. Sent as it comes, and as the
    /// shell connects while it is up.
    Curtain(CurtainShown),
    /// The curtain was lifted, or ended: the pill's surface goes. Written
    /// `curtain-end`.
    CurtainEnd,
    /// What logind allows of the session menu's power actions (7.4,
    /// [`LeaveCan`]): sent as the shell connects and when it changed.
    LeaveCan(LeaveCan),
    /// A confirmation of the session's end under way (7.4,
    /// [`LeaveShown`]): the user's popover shows it on its bar, opening if
    /// it was not (`Super+Shift+E`). Sent as it starts and on each change.
    Leave(LeaveShown),
    /// No confirmation any more: the menu's rows again. Written
    /// `leave-end`.
    LeaveEnd,
    /// The power button pressed, `[power] power-button = "ask"` (6.10):
    /// the session menu (7.4) opens on `output`'s bar, as a click on the
    /// footer's user opens it. Written `session-menu OUTPUT`.
    SessionMenu { output: String },
    /// What the bar's menu of a window shows (#723), the answer to
    /// [`ToCompositor::WindowMenu`] ([`WindowMenu`]).
    WindowMenu(WindowMenu),
    /// The windows kept above the others and those on all desktops, for
    /// the marks after their titles in the bar ([`WindowStates`]).
    WindowStates(WindowStates),
    /// `Super+B` (`[shortcuts] focus-bar`, bar.md §11): the keyboard goes
    /// into `output`'s bar, on its waiting agent, else its stuck one, else
    /// its clock; `None`, the bar holding it lets it go, back to the
    /// window that had it. Written `bar-keyboard OUTPUT` or `bar-keyboard
    /// -`.
    BarKeyboard(Option<String>),
    /// `Super+P` (#736, [`ActionsShown`]): the actions palette opens on
    /// the target's screen; open, it closes.
    Actions(ActionsShown),
    /// The palette's target window went, or the session locks: it closes.
    /// Written `actions-end`.
    ActionsEnd,
}

/// A program the compositor starts, supervises and starts again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Supervised {
    /// `hocuspanda-shell`.
    Shell,
    /// `hocus-secretary`, in assistant mode only.
    Secretary,
}

impl Supervised {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Secretary => "secretary",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        [Self::Shell, Self::Secretary]
            .into_iter()
            .find(|s| s.word() == word)
    }
}

/// The secretary's process as the compositor knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretaryShown {
    /// `[secretary] mode = "classic"`: no process; the 4.6 secretary.
    Off,
    /// Ended, and not started again yet: her header says "indisponible".
    Unavailable,
    /// Started, listening (or about to) on this socket.
    Ready(std::path::PathBuf),
}

/// What [`ToShell::Day`] asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DayAsk {
    /// A task titled so, at the top of the tasks' add column.
    Add(String),
    /// The task of that id ticked, or unticked.
    Tick(String),
    Untick(String),
    /// Every account read again now.
    Refresh,
    /// Tests only, and only when the shell's clock reads a time of its own
    /// (`HOCUS_CLOCK_AT`): the machine slept that many seconds, its wall
    /// clock moved on while its monotonic clock did not, and it resumed,
    /// as logind would say it (`PrepareForSleep(false)`).
    Slept(u64),
    /// The calendar opens (Day.5) on `day` (`2026-11-04`), the event of
    /// that id chosen, as the next meeting's card opens it.
    Calendar {
        day: String,
        event: Option<String>,
    },
}

/// Where the emoji picker opens on its screen (logical pixels, from the
/// screen's top left corner): under the focused application's caret, as
/// its text input says it; centred on the focused window; or centred on
/// the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmojiAt {
    Caret(Rect),
    Window(Rect),
    Screen,
}

/// A rectangle, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// What the clipboard's history keeps of a selection (5.6): its text (the
/// `text/plain;charset=utf-8` offer), or a PNG image, as offered.
#[derive(Clone, PartialEq, Eq)]
pub struct Clip {
    pub kind: ClipKind,
    pub data: Vec<u8>,
}

impl fmt::Debug for Clip {
    // What was copied may be private: only its kind and its size.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Clip({}, {} bytes)", self.kind.word(), self.data.len())
    }
}

/// A [`Clip`]'s kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipKind {
    Text,
    Png,
}

impl ClipKind {
    pub fn word(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Png => "png",
        }
    }

    fn of(word: &str) -> Result<Self, BadLine> {
        match word {
            "text" => Ok(Self::Text),
            "png" => Ok(Self::Png),
            other => Err(BadLine(format!("no clip kind {other:?}"))),
        }
    }

    /// The types a selection of this kind is offered as.
    pub fn types(self) -> Vec<String> {
        match self {
            Self::Text => vec![
                "text/plain;charset=utf-8",
                "text/plain",
                "UTF8_STRING",
                "TEXT",
                "STRING",
            ],
            Self::Png => vec!["image/png"],
        }
        .into_iter()
        .map(str::to_owned)
        .collect()
    }
}

/// The most a clip holds: a larger offer is not kept (5.6).
pub const CLIP_MAX: usize = 8 * 1024 * 1024;

/// How long the clipboard's history keeps what is not pinned
/// (`[clipboard] keep`, 6.17): until the session ends (never on disk), a
/// day, a week or 30 days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipKeep {
    Session,
    Day,
    Week,
    #[default]
    Month,
}

impl ClipKeep {
    pub const ALL: [Self; 4] = [Self::Session, Self::Day, Self::Week, Self::Month];

    /// As the file and the line say it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Day => "1d",
            Self::Week => "7d",
            Self::Month => "30d",
        }
    }

    pub fn of(word: &str) -> Result<Self, BadLine> {
        Self::ALL
            .into_iter()
            .find(|k| k.word() == word)
            .ok_or_else(|| BadLine(format!("keep {word:?}: session, 1d, 7d or 30d")))
    }

    /// How many days an item is kept; none: as long as the session.
    pub const fn days(self) -> Option<i64> {
        match self {
            Self::Session => None,
            Self::Day => Some(1),
            Self::Week => Some(7),
            Self::Month => Some(30),
        }
    }
}

/// What the clipboard's history keeps (`[clipboard]`, 6.17): whether it
/// keeps anything new (off, only the pinned items show, the others erased
/// a moment later), and for how long.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipRules {
    pub history: bool,
    pub keep: ClipKeep,
}

impl Default for ClipRules {
    fn default() -> Self {
        Self {
            history: true,
            keep: ClipKeep::Month,
        }
    }
}

/// The application the clipboard's history says a capture was copied from
/// (5.8b): the history names it "Capture d'écran" / "Screenshot".
pub const CAPTURE_APP: &str = "hocuspanda.capture";

impl Clip {
    fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use base64::Engine;
        write!(
            f,
            "{} {}",
            self.kind.word(),
            base64::engine::general_purpose::STANDARD.encode(&self.data)
        )
    }

    fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        use base64::Engine;
        let kind = ClipKind::of(word(words, "clip kind")?)?;
        // An empty clip has no word for its data.
        let data = match words.next() {
            Some(data) => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| BadLine(format!("a clip's data: {e}")))?,
            None => Vec::new(),
        };
        if data.len() > CLIP_MAX {
            return Err(BadLine("a clip too large".to_owned()));
        }
        Ok(Self { kind, data })
    }
}

/// An application the launcher starts (5.1), as its desktop entry says:
/// its id (`org.mozilla.firefox`, for the log), whether it runs in a
/// terminal, its working directory (empty: the compositor's), and its
/// command, field codes taken out, one argument each. Written `launch`,
/// then each after a tab: the id, `terminal` or `window`, the directory,
/// then the arguments; none of them holds a tab or a line break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub id: String,
    pub terminal: bool,
    pub dir: String,
    pub argv: Vec<String>,
}

impl Launch {
    /// Whether it can go on a line: something to run, and no field holds
    /// a tab or a control character.
    pub fn fits(&self) -> bool {
        let clean = |s: &str| !s.chars().any(char::is_control);
        !self.argv.is_empty()
            && !self.argv[0].is_empty()
            && clean(&self.id)
            && clean(&self.dir)
            && self.argv.iter().all(|a| clean(a))
    }
}

/// The panda peeking out (4.5): the agent, why, since when (microseconds
/// of `CLOCK_MONOTONIC`, as [`AgentShown::since`]) and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peek {
    pub id: u64,
    pub kind: PeekKind,
    pub since: u64,
    pub name: String,
}

/// Windows that moved with a screen ([`ToShell::ScreensMoved`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreensMoved {
    /// The screen went (else it came back).
    pub gone: bool,
    /// It is the laptop's own.
    pub built_in: bool,
    /// Its diagonal, 0 unknown.
    pub inches: u32,
    /// The desktop they are on, from 1; 0 when they are on several.
    pub desktop: usize,
    /// Their applications' ids, the focused window's first; empty when
    /// one says none.
    pub apps: Vec<String>,
}

/// Why the panda peeks out: its agent waits for the user (it holds out its
/// hat), or finished (its arms up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeekKind {
    Waiting,
    Done,
}

impl PeekKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Waiting => "waiting",
            Self::Done => "done",
        }
    }
}

impl FromStr for PeekKind {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "waiting" => Ok(Self::Waiting),
            "done" => Ok(Self::Done),
            _ => Err(BadLine(format!("no peek {s:?}"))),
        }
    }
}

/// An agents' tool, as the bar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTool {
    /// `claude-code`, `codex` or `herdr`.
    pub tool: String,
    pub state: ToolState,
    /// The file plugging it in changes, as the user knows it.
    pub file: String,
}

/// What an agents' tool is to the desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState {
    /// Not on this computer.
    Absent,
    /// There, not plugged in: the card proposes it.
    Found,
    /// There, not plugged in, and the user said to plug nothing in.
    Declined,
    /// Plugged in: its agents are followed.
    Plugged,
    /// herdr runs, and is followed.
    Running,
    /// herdr runs, and the user said to plug nothing in.
    Ignored,
    /// herdr does not run.
    Stopped,
}

impl ToolState {
    pub const ALL: [Self; 7] = [
        Self::Absent,
        Self::Found,
        Self::Declined,
        Self::Plugged,
        Self::Running,
        Self::Ignored,
        Self::Stopped,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Found => "found",
            Self::Declined => "declined",
            Self::Plugged => "plugged",
            Self::Running => "running",
            Self::Ignored => "ignored",
            Self::Stopped => "stopped",
        }
    }

    /// Whether its agents are followed.
    pub const fn followed(self) -> bool {
        matches!(self, Self::Plugged | Self::Running)
    }
}

impl FromStr for ToolState {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.name() == s)
            .ok_or_else(|| BadLine(format!("no tool state {s:?}")))
    }
}

/// An agent as the bar shows it. Written `ID STATE SINCE SOURCE WINDOW
/// NAME`: the compositor's id for it; `working`, `waiting`, `done`
/// (finished, not seen yet), `seen` (finished and seen since: its window
/// got the keyboard, or its row was clicked; 4.1), `failed` or `idle`;
/// since when, in microseconds of `CLOCK_MONOTONIC`;
/// where its state comes from (`claude-code`, `codex`, `herdr`,
/// `command`; several joined with `+`); its window's id (0: none found);
/// its name, last (it may hold spaces, never a tab). Then a tab and its
/// [`Note`], empty when it has none.
#[derive(Clone, PartialEq, Eq)]
pub struct AgentShown {
    pub id: u64,
    pub state: String,
    pub since: u64,
    pub source: String,
    pub window: u64,
    pub name: String,
    /// What it asks for, or why it failed, as its tool said it (4.6).
    pub note: Option<Note>,
}

// The note may hold code or a secret (a command, a file's path): its
// length only, as everywhere it is logged.
impl fmt::Debug for AgentShown {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentShown")
            .field("id", &self.id)
            .field("state", &self.state)
            .field("since", &self.since)
            .field("source", &self.source)
            .field("window", &self.window)
            .field("name", &self.name)
            .field("note", &self.note)
            .finish()
    }
}

/// What an agent waits for, or why it failed, as its tool says it: what
/// the secretary's briefing shows (bar.md §7 B, 4.6). One short line, cut
/// to [`NOTE_MAX`] characters. Written `allow TOOL WHAT` or `says TEXT`.
///
/// It may hold code or a secret (a command, a file's path): it is never
/// logged, only its length ([`fmt::Debug`]).
#[derive(Clone, PartialEq, Eq)]
pub enum Note {
    /// It asks to use `tool` (one word) on `what` (its input in one line,
    /// maybe empty): the shell words it ("Autoriser Bash : npm install ?").
    Allow { tool: String, what: String },
    /// What its tool says: a notification, a question, an error.
    Says(String),
}

/// The characters a [`Note`]'s text keeps at most: the briefing's line is
/// cut to its width long before that.
pub const NOTE_MAX: usize = 120;

/// A note's text as it is kept: on one line, its spaces runs made one,
/// trimmed, [`NOTE_MAX`] characters at most (the last an ellipsis when it
/// was longer).
pub fn note_text(text: &str) -> String {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|w| !w.is_empty())
        .collect();
    let line = words.join(" ");
    if line.chars().count() <= NOTE_MAX {
        return line;
    }
    let mut cut: String = line.chars().take(NOTE_MAX - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    cut
}

impl Note {
    /// A tool's request, its words kept as a note's.
    pub fn allow(tool: &str, what: &str) -> Self {
        let tool: String = tool
            .chars()
            .map(|c| {
                if c.is_whitespace() || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .take(64)
            .collect();
        Self::Allow {
            tool,
            what: note_text(what),
        }
    }

    /// A tool's text, kept as a note's; none when it says nothing.
    pub fn says(text: &str) -> Option<Self> {
        Some(Self::Says(note_text(text))).filter(|n| *n != Self::Says(String::new()))
    }

    /// Its length, in characters: what is logged of it.
    pub fn len(&self) -> usize {
        match self {
            Self::Allow { tool, what } => tool.chars().count() + what.chars().count(),
            Self::Says(text) => text.chars().count(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl fmt::Debug for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self {
            Self::Allow { .. } => "allow",
            Self::Says(_) => "says",
        };
        write!(f, "{kind} ({} characters)", self.len())
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Allow { tool, what } if tool.is_empty() => {
                write!(f, "allow _ {}", note_text(what))
            }
            Self::Allow { tool, what } => write!(f, "allow {tool} {}", note_text(what)),
            Self::Says(text) => write!(f, "says {}", note_text(text)),
        }
    }
}

impl FromStr for Note {
    type Err = BadLine;

    // The error never holds the note's text.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, rest) = s.split_once(' ').unwrap_or((s, ""));
        match kind {
            "allow" => {
                let (tool, what) = rest.split_once(' ').unwrap_or((rest, ""));
                if tool.is_empty() {
                    return Err(BadLine("a note's request without its tool".to_owned()));
                }
                Ok(Self::allow(tool, what))
            }
            "says" => {
                Self::says(rest).ok_or_else(|| BadLine("a note that says nothing".to_owned()))
            }
            _ => Err(BadLine("a note of no kind".to_owned())),
        }
    }
}

/// A screen's desktops as its bar's tiles show them. Written `desktops`,
/// the screen's name, the count, the current one (from 0), then for each
/// window, bottom first, its desktop (from 0), its place and size in
/// [`SHARE`]s of the screen, its [`Look`] (0, 1 or 2), its id and its
/// application's id (its icon's; empty when it has none, its spaces made
/// `_`), commas between: `desktops eDP-1 4 0
/// 0,20,30,600,940,1,7,org.gnome.Nautilus`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktops {
    pub output: String,
    pub count: u8,
    pub current: u8,
    pub windows: Vec<DeskWindow>,
}

/// A window as its desktop's tile sketches it and its list lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskWindow {
    pub desktop: u8,
    /// Left, top, width, height, in [`SHARE`]s of the screen, inside it.
    pub rect: [u16; 4],
    pub look: Look,
    /// The compositor's id for it: what [`ToCompositor::Bring`] names.
    pub id: u64,
    /// Its application's id, for its icon (empty: none).
    pub app: String,
}

/// How a window shows in its desktop's tile and list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Look {
    /// Written 0.
    #[default]
    Plain,
    /// The focused window, on the current desktop: drawn brighter. Written
    /// 1.
    Focused,
    /// Minimised: not sketched, listed after the others, greyed (3.9c).
    /// Written 2.
    Minimized,
}

impl Look {
    fn digit(self) -> u8 {
        match self {
            Self::Plain => 0,
            Self::Focused => 1,
            Self::Minimized => 2,
        }
    }

    fn of(digit: &str) -> Result<Self, BadLine> {
        match digit {
            "0" => Ok(Self::Plain),
            "1" => Ok(Self::Focused),
            "2" => Ok(Self::Minimized),
            other => Err(BadLine(format!("no look {other:?}"))),
        }
    }
}

/// A screen whose desktops a bar shows (8.4): its name, its logical
/// size (its tiles take its shape), and what names it as the Settings'
/// Écrans page does: whether it is the built-in panel, its diagonal in
/// inches (0: not known) and its number there. Written with commas:
/// `DP-1,3440,1440,0,34,2`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskScreen {
    pub output: String,
    pub width: u32,
    pub height: u32,
    pub built_in: bool,
    pub inches: u32,
    pub number: u32,
}

impl DeskScreen {
    fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.output,
            self.width,
            self.height,
            u8::from(self.built_in),
            self.inches,
            self.number
        )
    }

    fn read(word: &str) -> Result<Self, BadLine> {
        let fields: Vec<&str> = word.split(',').collect();
        let [output, width, height, built_in, inches, number] = fields[..] else {
            return Err(BadLine(format!("a screen {word:?}")));
        };
        let n = |v: &str| {
            v.parse::<u32>()
                .map_err(|e| BadLine(format!("a screen's {v:?}: {e}")))
        };
        Ok(Self {
            output: output.to_owned(),
            width: n(width)?,
            height: n(height)?,
            built_in: flag(Some(built_in), "1", "0")?,
            inches: n(inches)?,
            number: n(number)?,
        })
    }
}

/// A desktop's tile in a bar (8.5 B): its screen, its desktop (from 0),
/// and where it is on the bar's desktops' surface, logical pixels from its
/// top-left corner. Written with commas: `DP-1,2,11,51,39,20`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskTile {
    pub output: String,
    pub desktop: u8,
    pub rect: [i32; 4],
}

/// A screen's windows' titles, by their ids, in [`Desktops::windows`]'
/// order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Titles {
    pub output: String,
    pub titles: Vec<(u64, String)>,
}

/// A screen's windows, the most recently focused first ([`ToShell::Recent`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Recent {
    pub output: String,
    pub ids: Vec<u64>,
}

/// The characters of a title a [`ToShell::Titles`] line carries at most:
/// the list cuts titles to its 190 px long before that.
pub const TITLE_MAX: usize = 200;

/// A title as a [`ToShell::Titles`] line carries it: on one line, without
/// tabs, trimmed, [`TITLE_MAX`] characters at most.
pub fn title_text(title: &str) -> String {
    let title: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(TITLE_MAX)
        .collect();
    title.trim().to_owned()
}

/// A text on one line: its line breaks and other control characters made
/// spaces, trimmed.
fn one_line(text: &str) -> String {
    let text: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    text.trim().to_owned()
}

/// An application's id as a [`ToShell::Desktops`] line carries it: its
/// spaces and control characters made `_`, which an id has none of.
fn app_word(app: &str) -> String {
    app.chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// A whole screen, in the units of [`DeskWindow::rect`]: a thousandth of it
/// is a fraction of a pixel in a tile, and moves smaller than that send
/// nothing.
pub const SHARE: u16 = 1000;

/// The desktop the shell asks for: one by its index (from 0), or the next
/// or the previous one, which the compositor takes no further than the
/// last or the first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopTo {
    At(u8),
    Next,
    Previous,
}

/// A keyboard layout as the compositor's keymap has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyLayout {
    /// Its XKB layout and variant as `[keyboard] layouts` writes them
    /// (`fr`, `fr(bepo)`); empty when unknown.
    pub code: String,
    /// The name the keymap gives it: XKB's description, in English
    /// (`French (BEPO)`).
    pub name: String,
}

/// The keyboard's level keys, as `[shortcuts]` names their actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LevelKey {
    VolumeUp,
    VolumeDown,
    Mute,
    BrightnessUp,
    BrightnessDown,
}

impl LevelKey {
    pub const ALL: [Self; 5] = [
        Self::VolumeUp,
        Self::VolumeDown,
        Self::Mute,
        Self::BrightnessUp,
        Self::BrightnessDown,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::VolumeUp => "volume-up",
            Self::VolumeDown => "volume-down",
            Self::Mute => "mute",
            Self::BrightnessUp => "brightness-up",
            Self::BrightnessDown => "brightness-down",
        }
    }

    /// Whether it steps on while held, as a key repeats (not mute).
    pub const fn repeats(self) -> bool {
        !matches!(self, Self::Mute)
    }
}

impl FromStr for LevelKey {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|k| k.name() == s)
            .ok_or_else(|| BadLine(format!("no level key {s:?}")))
    }
}

/// What the clock does when the minute changes (`[panel.clock]
/// transition`), from the 3.2 mockup's keyframes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Transition {
    /// Each changed digit leaves upward with a little blur; the new one
    /// comes from below with a slight overshoot.
    #[default]
    Odometer,
    /// The new digits rise into place and fade in.
    Roll,
    /// The old digits fold away, the new ones fold down like a split-flap.
    Flip,
    /// The new digits flicker on like a neon tube.
    Neon,
    /// They change at once.
    None,
}

impl Transition {
    pub const ALL: [Self; 5] = [
        Self::Odometer,
        Self::Roll,
        Self::Flip,
        Self::Neon,
        Self::None,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Odometer => "odometer",
            Self::Roll => "roll",
            Self::Flip => "flip",
            Self::Neon => "neon",
            Self::None => "none",
        }
    }
}

impl FromStr for Transition {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|t| t.name() == s)
            .ok_or_else(|| {
                BadLine(format!(
                    "no transition {s:?}: odometer, roll, flip, neon or none"
                ))
            })
    }
}

/// A group of the bar's column that the user moves (`[panel] order`,
/// 6.4): the meeting's card and the sign of what is in use stay first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BarGroup {
    Today,
    Tasks,
    Agents,
}

impl BarGroup {
    /// Their order when none is said (#599).
    pub const DEFAULT: [Self; 3] = [Self::Today, Self::Tasks, Self::Agents];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Tasks => "tasks",
            Self::Agents => "agents",
        }
    }

    /// The order `names` say: the names known, each once, in their order,
    /// then those missing in their default order; an unknown name is
    /// skipped.
    pub fn order_of<S: AsRef<str>>(names: &[S]) -> [Self; 3] {
        let mut order: Vec<Self> = Vec::with_capacity(3);
        let known = names.iter().filter_map(|n| n.as_ref().trim().parse().ok());
        for group in known.chain(Self::DEFAULT) {
            if !order.contains(&group) {
                order.push(group);
            }
        }
        [order[0], order[1], order[2]]
    }
}

impl FromStr for BarGroup {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::DEFAULT
            .into_iter()
            .find(|g| g.name() == s)
            .ok_or_else(|| BadLine(format!("no bar group {s:?}: today, tasks or agents")))
    }
}

/// What the bar's column shows (`[panel]`, 6.4): each group that can be
/// hidden, and the order of those that move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarGroups {
    pub order: [BarGroup; 3],
    pub meeting: bool,
    pub today: bool,
    pub tasks: bool,
    pub agents: bool,
}

impl Default for BarGroups {
    fn default() -> Self {
        Self {
            order: BarGroup::DEFAULT,
            meeting: true,
            today: true,
            tasks: true,
            agents: true,
        }
    }
}

impl BarGroups {
    /// Whether `group` shows.
    pub const fn shows(&self, group: BarGroup) -> bool {
        match group {
            BarGroup::Today => self.today,
            BarGroup::Tasks => self.tasks,
            BarGroup::Agents => self.agents,
        }
    }
}

/// How the clock looks (`[panel.clock]`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockLook {
    /// The font family of the hours and minutes (and of the seconds, in
    /// its light weight).
    pub font: String,
    /// The colour of the hours and minutes, as red, green, blue.
    pub color: [u8; 3],
    pub transition: Transition,
    /// The seconds show beside the minutes, in the full form.
    pub seconds: bool,
}

impl ClockLook {
    /// The bundled font, the mockup's.
    pub const DEFAULT_FONT: &str = "Martian Mono";
    /// `text` (bar.md §13).
    pub const DEFAULT_COLOR: [u8; 3] = [0xEE, 0xF1, 0xF6];
}

impl Default for ClockLook {
    fn default() -> Self {
        Self {
            font: Self::DEFAULT_FONT.to_owned(),
            color: Self::DEFAULT_COLOR,
            transition: Transition::default(),
            seconds: true,
        }
    }
}

/// A colour written `#RRGGBB` (or `#RGB`), as red, green, blue.
pub fn parse_color(text: &str) -> Result<[u8; 3], BadLine> {
    let bad = || BadLine(format!("colour {text:?}: #RRGGBB or #RGB"));
    let hex = text.strip_prefix('#').ok_or_else(bad)?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(bad());
    }
    let digit = |i: usize, len: usize| u8::from_str_radix(&hex[i * len..(i + 1) * len], 16);
    match hex.len() {
        6 => Ok([digit(0, 2), digit(1, 2), digit(2, 2)].map(|d| d.unwrap_or(0))),
        3 => Ok([digit(0, 1), digit(1, 1), digit(2, 1)].map(|d| d.unwrap_or(0) * 17)),
        _ => Err(bad()),
    }
}

/// How near the next meeting is, for the halo (bar.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingNear {
    /// In 10 min or less (priority 3).
    Ten,
    /// In 2 min or less (priority 1).
    Two,
}

/// What the shell tells the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToCompositor {
    /// The shell draws on the CPU: its bars are opaque, nothing to blur.
    Software,
    /// A line of what the shell shows, for screen readers ([`ToShell::A11y`]),
    /// its control characters made spaces.
    A11yLine(String),
    /// The last of them.
    A11yEnd,
    /// The shell's surfaces of this namespace have something new to show:
    /// the compositor sends them a configure, as they are. iced draws
    /// every surface after any message of the shell's own, and the whole
    /// bar would be drawn again; a configure makes it draw that surface
    /// alone (bar.md, as built, 3.3).
    Wake(String),
    /// The shell's surfaces of this namespace (a popover) are closing: the
    /// compositor fades them out over [`POPOVER_CLOSE`] (at once without
    /// motion), and they are no longer a menu; the shell removes them
    /// after that.
    Closing(String),
    /// A click on the secretary's head (assistant mode): her drawer is
    /// asked for on `output`'s bar; the compositor answers
    /// [`ToShell::SecretaryDrawer`] with the screen's width, as `Super+S`
    /// does. Written `secretary-drawer OUTPUT`.
    SecretaryDrawer(String),
    /// The user asks for the next keyboard layout (the bar's row): the
    /// compositor switches the seat's keyboard to it, then says which is
    /// active ([`ToShell::Layout`]).
    NextLayout,
    /// The user asks for a desktop of `output` (a tile clicked, the wheel
    /// over them): the compositor switches as its shortcuts do. Written
    /// `desktop OUTPUT N`, `desktop OUTPUT next` or `desktop OUTPUT
    /// previous`.
    Desktop { output: String, to: DesktopTo },
    /// The user asks for window `id` (a row of a desktop's list, 3.9b):
    /// the compositor goes to its desktop, sliding, and focuses it.
    /// Written `bring ID`.
    Bring(u64),
    /// Where bar `bar`'s desktops' tiles are on its desktops' surface
    /// (8.5 B): a window moved over one goes to that desktop when it is
    /// let go there. Sent when they move or change. Written `desk-tiles
    /// BAR`, then each [`DeskTile`].
    DeskTiles { bar: String, tiles: Vec<DeskTile> },
    /// The user asks window `id` to close (its ✕ in the bar, #604): the
    /// compositor asks its application, as `Super+Q` does; the
    /// application decides. Written `close ID`.
    Close(u64),
    /// The bar's "+ N autres" over its windows (#604): the overview opens
    /// on every screen, or closes when open (8.5 A). Written `overview`.
    Overview,
    /// The user goes to agent `id` (its row in the agents' group, 4.1):
    /// the compositor brings its window, if it has one, and a finished
    /// agent is seen. From a row (4.3), the streak runs from it to the
    /// window. Written `go-to-agent ID`, or `go-to-agent ID OUTPUT X Y W
    /// H` with the row.
    GoToAgent { id: u64, row: Option<Row> },
    /// The keyboard's ring moved in a bar (`Super+B`, bar.md §11): where
    /// it is, so that the magnifier's view follows it. Written `ring
    /// OUTPUT X Y W H`.
    Ring(Row),
    /// The user plugs an agents' tool in (`claude-code`, `codex`), having
    /// read what it changes: the compositor writes its hooks, off its
    /// loop, then says [`ToShell::AgentTools`]. Written `plug TOOL`.
    Plug(String),
    /// The user unplugs it. Written `unplug TOOL`.
    Unplug(String),
    /// "Plug nothing in": every tool there and not plugged in is declined,
    /// herdr's following too. Written `decline`.
    Decline,
    /// The user follows herdr again, declined before (Sources' "Suivre").
    /// Written `follow herdr`.
    Follow(String),
    /// The secretary's briefing opened (`open`) or closed (4.7b): while it
    /// is open no panda peeks out, and one out goes back at once, so that
    /// it never shows through it. Written `briefing open` or `briefing
    /// closed`.
    Briefing { open: bool },
    /// The user clicked the panda that says screens moved
    /// ([`ToShell::ScreensMoved`]), or answered her peek: it goes back.
    /// Written `peek-close`.
    PeekClose,
    /// The shell's speak gate lets her peek of request `id` out
    /// (secretary.md §6.2, §6.3) for `stays` seconds: it comes out as an
    /// agent's panda does, in place of one out, and none comes out while
    /// it is; asked again while it is out, it stays `stays` more. Written
    /// `secretary-peek ID STAYS`.
    SecretaryPeek { id: u64, stays: u32 },
    /// How near the next meeting is (bar.md §6, Day.2), for the halo: in
    /// 10 min or less, in 2 min or less, or neither (none, further, in
    /// progress, or opened by the user). Written `meeting ten`, `meeting
    /// two` or `meeting none`.
    Meeting(Option<MeetingNear>),
    /// The clipboard takes `text` (the command the Sources popover shows),
    /// which the compositor offers: the shell's surfaces take no serial to
    /// set it themselves. Written `copy TEXT`, the text on one line.
    Copy(String),
    /// A notification's bubble was clicked (5.3): the compositor brings the
    /// window of its application, `app` (the notification's desktop entry,
    /// else its application's name; `-` for none), else of the process that
    /// sent it, `pid`, or one of its parents (0: not known). Written `raise
    /// PID APP`.
    Raise { pid: u32, app: String },
    /// The user starts an application from the launcher (5.1): the
    /// compositor runs it off its loop, as its own child, its first window
    /// taking the focus on the current desktop.
    Launch(Launch),
    /// The user logs out (the footer's user, 5.5): the session ends, as
    /// its shortcut ends it. Written `log-out`.
    LogOut,
    /// An item of the clipboard's history is chosen (5.6): the clipboard
    /// takes it, which the compositor offers; with `paste`, the window
    /// that had the keyboard gets it back once the history lets it go, and
    /// `Ctrl+V` (`Ctrl+Shift+V` in a terminal). Written `clip paste KIND
    /// DATA` or `clip copy KIND DATA`, the data in base64.
    Clip { paste: bool, clip: Clip },
    /// How many items the clipboard's history holds, and how many of them
    /// are pinned (6.17): sent as the shell connects and when they change,
    /// for the Settings' page. Written `clipboard-count ITEMS PINNED`.
    ClipboardCount { items: u32, pinned: u32 },
    /// An emoji chosen in the picker or the launcher (5.7): it goes into
    /// the window that had the keyboard, through its text input when it
    /// has one, else pasted, the clipboard given back after. With `keep`
    /// the picker stays open, and is told when it is in
    /// ([`ToShell::EmojiInserted`]). Written `emoji insert TEXT` or `emoji
    /// keep TEXT`.
    Emoji { keep: bool, text: String },
    /// The user acts on the capture's toolbar (5.8, [`CaptureAsk`]).
    Capture(CaptureAsk),
    /// The chooser's thumbnails' places, what it chose, its cancel, a
    /// share stopped from the bar (5.11, [`ShareAsk`]).
    Share(ShareAsk),
    /// The bar's item pauses, goes on with or stops the recording; a
    /// recording's thumbnail deletes or drags it (5.10, [`RecordAsk`]).
    Record(RecordAsk),
    /// The user clicked a tray icon (5.14b): the shell is about to call
    /// the item's `Activate` or `SecondaryActivate`. `pid` is the process
    /// of the item's D-Bus connection (0: not known). The compositor
    /// answers at once with an activation token made for that click
    /// ([`ToShell::TrayToken`]), and lets that process's application
    /// focus one window shortly after, with a token of its own or none.
    /// Written `tray-click ASK PID`.
    TrayClick { ask: u64, pid: u32 },
    /// The lock's arrow or one of its buttons clicked, or the session
    /// menu's "Verrouiller" (7.1, 7.4, [`LockAsk`]).
    Lock(LockAsk),
    /// The shell is about to ask the desktop portal for a dialog over its
    /// surface of `namespace` (the capture's editor's "Enregistrer
    /// sous…", #595). The compositor exports that surface through
    /// xdg-foreign and answers at once with its handle
    /// ([`ToShell::Exported`]); a window that the portal's backend maps as
    /// that handle's child comes in front, with the keyboard. Written
    /// `export ASK NAMESPACE`.
    Export { ask: u64, namespace: String },
    /// The system's authentication dialog opens or fades out (7.5,
    /// [`AuthAsk`]).
    Auth(AuthAsk),
    /// The session menu (7.4, [`LeaveAsk`]): it opened, a row chosen, the
    /// confirmation acted on or cancelled.
    Leave(LeaveAsk),
    /// The bar's menu of window `id` (#723) runs `action` on it, as its
    /// radial menu does. Written `window-action ID ACTION`.
    WindowAction { id: u64, action: WindowAction },
    /// The bar's menu sends window `id` to desktop `desktop` (from 0) of
    /// screen `output`. Written `send-to ID OUTPUT N`.
    SendTo {
        id: u64,
        output: String,
        desktop: u8,
    },
    /// The bar's menu of window `id` is about to open: the compositor
    /// answers at once with what it shows ([`ToShell::WindowMenu`]).
    /// Written `window-menu ID`.
    WindowMenu(u64),
    /// The palette (#736) runs an action ([`RunAction`]). Written `action
    /// ID ARG WINDOW`.
    Action(RunAction),
}

/// Where something the user clicked is on a bar: its screen's name and
/// its rectangle in the bar's frame, logical pixels, `x` from the bar's
/// right edge (negative, inside it) and `y` from its top.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub output: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// A screen's name as a [`ToShell::Screens`] line carries it: on one
/// line, without tabs, its spaces trimmed.
pub fn screen_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    name.trim().to_owned()
}

/// A line of the screen readers' tree as a [`ToCompositor::A11yLine`]
/// carries it: its control characters made spaces, its indent kept.
pub fn a11y_text(line: &str) -> String {
    line.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// A line that does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLine(pub String);

impl fmt::Display for BadLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadLine {}

fn flag(word: Option<&str>, on: &str, off: &str) -> Result<bool, BadLine> {
    match word {
        Some(w) if w == on => Ok(true),
        Some(w) if w == off => Ok(false),
        other => Err(BadLine(format!("{other:?}: {on} or {off}"))),
    }
}

fn word<'a>(words: &mut impl Iterator<Item = &'a str>, what: &str) -> Result<&'a str, BadLine> {
    words.next().ok_or_else(|| BadLine(format!("no {what}")))
}

impl fmt::Display for ToShell {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Look {
                glass,
                motion,
                lively,
            } => write!(
                f,
                "look {} {} {}",
                if *glass { "glass" } else { "opaque" },
                if *motion { "motion" } else { "still" },
                if *lively { "lively" } else { "calm" }
            ),
            Self::Theme(choice) => write!(f, "theme {choice}"),
            Self::Locale(locale) => write!(f, "locale {locale}"),
            Self::Wallpaper(wallpapers) => write!(f, "wallpaper {wallpapers}"),
            Self::Bar {
                output,
                form,
                since,
                scale,
            } => write!(f, "bar {output} {} {since} {scale}", form.name()),
            Self::NoBar { output } => write!(f, "nobar {output}"),
            Self::Level(key) => write!(f, "level {}", key.name()),
            Self::Outside => f.write_str("outside"),
            Self::Screens(names) => {
                f.write_str("screens")?;
                for name in names {
                    write!(f, "\t{}", screen_name(name))?;
                }
                Ok(())
            }
            Self::Layouts(layouts) => {
                f.write_str("layouts")?;
                for layout in layouts {
                    write!(
                        f,
                        "\t{}\t{}",
                        screen_name(&layout.code),
                        screen_name(&layout.name)
                    )?;
                }
                Ok(())
            }
            Self::Layout(index) => write!(f, "layout {index}"),
            Self::Recent(recent) => {
                write!(f, "recent {}", recent.output)?;
                for id in &recent.ids {
                    write!(f, " {id}")?;
                }
                Ok(())
            }
            Self::DeskScreens { bar, screens } => {
                write!(f, "desk-screens {bar}")?;
                for screen in screens {
                    f.write_str(" ")?;
                    screen.write(f)?;
                }
                Ok(())
            }
            Self::DropOver { bar, over: None } => write!(f, "drop-over {bar} -"),
            Self::DropOver {
                bar,
                over: Some((output, desktop)),
            } => write!(f, "drop-over {bar} {output} {desktop}"),
            Self::BarSide(side) => write!(f, "bar-side {}", side.name()),
            Self::DesktopNames(names) => {
                write!(f, "desktop-names")?;
                for name in names {
                    write!(f, "\t{}", title_text(name))?;
                }
                Ok(())
            }
            Self::BarWindows(shown) => {
                write!(f, "bar-windows {}", if *shown { "shown" } else { "hidden" })
            }
            Self::BarGroups(groups) => {
                let order: Vec<&str> = groups.order.iter().map(|g| g.name()).collect();
                let hidden: Vec<&str> = [
                    ("meeting", groups.meeting),
                    ("today", groups.today),
                    ("tasks", groups.tasks),
                    ("agents", groups.agents),
                ]
                .into_iter()
                .filter(|(_, shown)| !shown)
                .map(|(name, _)| name)
                .collect();
                let hidden = if hidden.is_empty() {
                    "-".to_owned()
                } else {
                    hidden.join(",")
                };
                write!(f, "bar-groups {} {hidden}", order.join(","))
            }
            Self::Desktops(desktops) => {
                write!(
                    f,
                    "desktops {} {} {}",
                    desktops.output, desktops.count, desktops.current
                )?;
                for w in &desktops.windows {
                    let [x, y, width, height] = w.rect;
                    write!(
                        f,
                        " {},{x},{y},{width},{height},{},{},{}",
                        w.desktop,
                        w.look.digit(),
                        w.id,
                        app_word(&w.app)
                    )?;
                }
                Ok(())
            }
            Self::AgentTools(tools) => {
                f.write_str("agent-tools")?;
                for tool in tools {
                    write!(
                        f,
                        "\t{} {} {}",
                        tool.tool,
                        tool.state.name(),
                        screen_name(&tool.file)
                    )?;
                }
                Ok(())
            }
            Self::Agents(agents) => {
                f.write_str("agents")?;
                for a in agents {
                    write!(
                        f,
                        "\t{} {} {} {} {} {}\t",
                        a.id,
                        a.state,
                        a.since,
                        a.source,
                        a.window,
                        screen_name(&a.name)
                    )?;
                    if let Some(note) = &a.note {
                        write!(f, "{note}")?;
                    }
                }
                Ok(())
            }
            Self::Peek(peek) => write!(
                f,
                "peek {} {} {} {}",
                peek.id,
                peek.kind.name(),
                peek.since,
                screen_name(&peek.name)
            ),
            Self::NoPeek => f.write_str("no-peek"),
            Self::ScreensMoved(moved) => {
                write!(
                    f,
                    "screens-moved {} {} {} {}",
                    if moved.gone { "gone" } else { "back" },
                    if moved.built_in {
                        "built-in"
                    } else {
                        "external"
                    },
                    moved.inches,
                    moved.desktop
                )?;
                for app in &moved.apps {
                    let app = screen_name(app).replace(' ', "");
                    write!(f, "\t{}", if app.is_empty() { "-" } else { app.as_str() })?;
                }
                Ok(())
            }
            Self::AgentPandas(pandas) => {
                write!(f, "agent-rows {}", if *pandas { "pandas" } else { "dots" })
            }
            Self::Focus(output) => write!(f, "focus {output}"),
            Self::BarKeyboard(Some(output)) => write!(f, "bar-keyboard {output}"),
            Self::BarKeyboard(None) => f.write_str("bar-keyboard -"),
            Self::Launcher { output, scale } => write!(f, "launcher {output} {scale}"),
            Self::Copied { app, clip } => {
                let app = app_word(app);
                write!(f, "copied {} ", if app.is_empty() { "-" } else { &app })?;
                clip.write(f)
            }
            Self::CaptureCopied { capture, clip } => {
                write!(f, "capture-copied {capture} ")?;
                clip.write(f)
            }
            Self::Clipboard { output } => write!(f, "clipboard {output}"),
            Self::ClipboardRules(rules) => write!(
                f,
                "clipboard-rules {} {}",
                if rules.history { "on" } else { "off" },
                rules.keep.word()
            ),
            Self::ClipboardClear => f.write_str("clipboard-clear"),
            Self::Emoji {
                output,
                scale,
                width,
                height,
                at,
            } => {
                write!(f, "emoji {output} {scale} {width} {height} ")?;
                match at {
                    EmojiAt::Caret(r) => {
                        write!(f, "caret {} {} {} {}", r.x, r.y, r.width, r.height)
                    }
                    EmojiAt::Window(r) => {
                        write!(f, "window {} {} {} {}", r.x, r.y, r.width, r.height)
                    }
                    EmojiAt::Screen => f.write_str("screen"),
                }
            }
            Self::EmojiInserted => f.write_str("emoji-inserted"),
            Self::Join => f.write_str("join"),
            Self::Secretary(SecretaryShown::Off) => f.write_str("secretary off"),
            Self::Secretary(SecretaryShown::Unavailable) => f.write_str("secretary unavailable"),
            Self::Secretary(SecretaryShown::Ready(socket)) => {
                write!(f, "secretary ready {}", one_line(&socket.to_string_lossy()))
            }
            Self::SecretaryAsk(text) => write!(f, "secretary-ask {}", one_line(text)),
            Self::FellBack { which, program } => write!(
                f,
                "fell-back {} {}",
                which.word(),
                one_line(&program.to_string_lossy())
            ),
            Self::SecretaryDrawer { output, width } => {
                write!(f, "secretary-drawer {output} {width}")
            }
            Self::SecretaryPeek(id) => write!(f, "secretary-peek {id}"),
            Self::SharedScreens(names) => {
                f.write_str("shared-screens")?;
                for name in names {
                    write!(f, "\t{}", screen_name(name))?;
                }
                Ok(())
            }
            Self::Fullscreen(names) => {
                f.write_str("fullscreen")?;
                for name in names {
                    write!(f, "\t{}", screen_name(name))?;
                }
                Ok(())
            }
            Self::ShareChoose(choose) => choose.write(f),
            Self::ShareChooseEnd(request) => write!(f, "share-choose-end {request}"),
            Self::AuthAt(at) => at.write(f),
            Self::CapsLock(on) => write!(f, "caps {}", if *on { "on" } else { "off" }),
            Self::TrayToken { ask, token } => write!(f, "tray-token {ask} {token}"),
            Self::Exported { ask, handle } => {
                write!(f, "exported {ask} {}", handle.as_deref().unwrap_or("-"))
            }
            Self::Shares(shares) => {
                f.write_str("shares")?;
                for share in shares {
                    share.write(f)?;
                }
                Ok(())
            }
            Self::Capture(bar) => bar.write(f),
            Self::CaptureEnd => f.write_str("capture-end"),
            Self::Captured(captured) => captured.write(f),
            Self::Recording(shown) => shown.write(f),
            Self::RecordingEnd => f.write_str("recording-end"),
            Self::Recorded(recorded) => recorded.write(f),
            Self::Lock(shown) => shown.write(f),
            Self::Unlocked => f.write_str("unlocked"),
            Self::A11y => f.write_str("a11y"),
            Self::Curtain(shown) => shown.write(f),
            Self::CurtainEnd => f.write_str("curtain-end"),
            Self::LeaveCan(can) => can.write(f),
            Self::Leave(shown) => shown.write(f),
            Self::LeaveEnd => f.write_str("leave-end"),
            Self::SessionMenu { output } => write!(f, "session-menu {output}"),
            Self::WindowMenu(menu) => menu.write(f),
            Self::WindowStates(states) => states.write(f),
            Self::Actions(shown) => shown.write(f),
            Self::ActionsEnd => f.write_str("actions-end"),
            Self::Day(ask) => match ask {
                DayAsk::Add(title) => write!(f, "day add {}", one_line(title)),
                DayAsk::Tick(id) => write!(f, "day tick {}", one_line(id)),
                DayAsk::Untick(id) => write!(f, "day untick {}", one_line(id)),
                DayAsk::Refresh => f.write_str("day refresh"),
                DayAsk::Slept(seconds) => write!(f, "day slept {seconds}"),
                DayAsk::Calendar { day, event } => {
                    write!(f, "day calendar {}", one_line(day))?;
                    match event {
                        Some(event) => write!(f, " {}", one_line(event)),
                        None => Ok(()),
                    }
                }
            },
            Self::Titles(titles) => {
                write!(f, "titles {}", titles.output)?;
                for (id, title) in &titles.titles {
                    write!(f, "\t{id}\t{}", title_text(title))?;
                }
                Ok(())
            }
            Self::Clock(look) => {
                let [r, g, b] = look.color;
                write!(
                    f,
                    "clock {} {} #{r:02X}{g:02X}{b:02X} {}",
                    if look.seconds { "seconds" } else { "noseconds" },
                    look.transition.name(),
                    look.font
                )
            }
        }
    }
}

impl FromStr for ToShell {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        if let Some(rest) = line.strip_prefix("share-choose ") {
            return ShareChoose::read(rest).map(Self::ShareChoose);
        }
        if let Some(rest) = line.strip_prefix("window-menu ") {
            return WindowMenu::read(rest).map(Self::WindowMenu);
        }
        if let Some(rest) = line.strip_prefix("actions ") {
            return ActionsShown::read(rest).map(Self::Actions);
        }
        if line == "actions-end" {
            return Ok(Self::ActionsEnd);
        }
        if let Some(rest) = line.strip_prefix("window-states ") {
            return WindowStates::read(rest).map(Self::WindowStates);
        }
        if let Some(rest) = line.strip_prefix("auth-at ") {
            return AuthAt::read(rest).map(Self::AuthAt);
        }
        if let Some(rest) = line.strip_prefix("leave ") {
            return LeaveShown::read(rest).map(Self::Leave);
        }
        if let Some(rest) = line.strip_prefix("secretary-ask ") {
            return Ok(Self::SecretaryAsk(rest.to_owned()));
        }
        if let Some(rest) = line.strip_prefix("fell-back ") {
            return match rest.split_once(' ') {
                Some((which, program)) if !program.is_empty() => Supervised::of(which)
                    .map(|which| Self::FellBack {
                        which,
                        program: std::path::PathBuf::from(program),
                    })
                    .ok_or_else(|| BadLine(format!("fell-back {rest}"))),
                _ => Err(BadLine(format!("fell-back {rest}"))),
            };
        }
        if let Some(rest) = line.strip_prefix("secretary ") {
            return match rest.split_once(' ') {
                None if rest == "off" => Ok(Self::Secretary(SecretaryShown::Off)),
                None if rest == "unavailable" => Ok(Self::Secretary(SecretaryShown::Unavailable)),
                Some(("ready", socket)) if !socket.is_empty() => Ok(Self::Secretary(
                    SecretaryShown::Ready(std::path::PathBuf::from(socket)),
                )),
                _ => Err(BadLine(format!("secretary {rest}"))),
            };
        }
        if let Some(rest) = line.strip_prefix("day ") {
            let (what, text) = rest.split_once(' ').unwrap_or((rest, ""));
            let text = || {
                if text.trim().is_empty() {
                    Err(BadLine(format!("day {what}: nothing given")))
                } else {
                    Ok(text.to_owned())
                }
            };
            return match what {
                "add" => text().map(|t| Self::Day(DayAsk::Add(t))),
                "tick" => text().map(|t| Self::Day(DayAsk::Tick(t))),
                "untick" => text().map(|t| Self::Day(DayAsk::Untick(t))),
                "refresh" => Ok(Self::Day(DayAsk::Refresh)),
                "calendar" => text().map(|t| {
                    let (day, event) = match t.split_once(' ') {
                        Some((day, event)) => (day.to_owned(), Some(event.to_owned())),
                        None => (t, None),
                    };
                    Self::Day(DayAsk::Calendar { day, event })
                }),
                "slept" => text().and_then(|t| {
                    t.trim()
                        .parse()
                        .map(|s| Self::Day(DayAsk::Slept(s)))
                        .map_err(|e| BadLine(format!("day slept: {e}")))
                }),
                other => Err(BadLine(format!("day {other}"))),
            };
        }
        if let Some(fields) = line.strip_prefix("shares")
            && (fields.is_empty() || fields.starts_with('\t'))
        {
            return fields
                .split('\t')
                .skip(1)
                .map(ShareShown::read)
                .collect::<Result<_, BadLine>>()
                .map(Self::Shares);
        }
        if let Some(names) = line.strip_prefix("shared-screens")
            && (names.is_empty() || names.starts_with('\t'))
        {
            return Ok(Self::SharedScreens(
                names
                    .split('\t')
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ));
        }
        if let Some(names) = line.strip_prefix("fullscreen")
            && (names.is_empty() || names.starts_with('\t'))
        {
            return Ok(Self::Fullscreen(
                names
                    .split('\t')
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ));
        }
        if let Some(names) = line.strip_prefix("screens")
            && (names.is_empty() || names.starts_with('\t'))
        {
            return Ok(Self::Screens(
                names
                    .split('\t')
                    .filter(|n| !n.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ));
        }
        if let Some(fields) = line.strip_prefix("layouts")
            && (fields.is_empty() || fields.starts_with('\t'))
        {
            let fields: Vec<&str> = fields.split('\t').skip(1).collect();
            let (pairs, rest) = fields.as_chunks::<2>();
            if !rest.is_empty() {
                return Err(BadLine(format!("a layout without its name: {line:?}")));
            }
            return Ok(Self::Layouts(
                pairs
                    .iter()
                    .map(|[code, name]| KeyLayout {
                        code: (*code).to_owned(),
                        name: (*name).to_owned(),
                    })
                    .collect(),
            ));
        }
        if let Some(fields) = line.strip_prefix("agent-tools")
            && (fields.is_empty() || fields.starts_with('\t'))
        {
            return fields
                .split('\t')
                .skip(1)
                .map(|field| {
                    let mut words = field.splitn(3, ' ');
                    let tool = word(&mut words, "tool")?.to_owned();
                    let state = word(&mut words, "tool state")?.parse()?;
                    let file = words.next().unwrap_or_default().to_owned();
                    Ok(AgentTool { tool, state, file })
                })
                .collect::<Result<_, BadLine>>()
                .map(Self::AgentTools);
        }
        if let Some(fields) = line.strip_prefix("agents")
            && (fields.is_empty() || fields.starts_with('\t'))
        {
            let fields: Vec<&str> = fields.split('\t').skip(1).collect();
            let (pairs, rest) = fields.as_chunks::<2>();
            if !rest.is_empty() {
                return Err(BadLine("an agent without its note".to_owned()));
            }
            return pairs
                .iter()
                .map(|[field, note]| {
                    let mut words = field.splitn(6, ' ');
                    let number = |w: &str, what: &str| {
                        w.parse::<u64>()
                            .map_err(|e| BadLine(format!("{what}: {e}")))
                    };
                    Ok(AgentShown {
                        id: number(word(&mut words, "agent")?, "agent")?,
                        state: word(&mut words, "state")?.to_owned(),
                        since: number(word(&mut words, "since")?, "since")?,
                        source: word(&mut words, "source")?.to_owned(),
                        window: number(word(&mut words, "window")?, "window")?,
                        name: words.next().unwrap_or_default().to_owned(),
                        note: match *note {
                            "" => None,
                            note => Some(note.parse()?),
                        },
                    })
                })
                .collect::<Result<_, BadLine>>()
                .map(Self::Agents);
        }
        if line.starts_with("captured ") {
            return Captured::read(line).map(Self::Captured);
        }
        if line.starts_with("recorded ") {
            return Recorded::read(line).map(Self::Recorded);
        }
        if let Some(fields) = line.strip_prefix("desktop-names") {
            let names = match fields.strip_prefix('\t') {
                Some(fields) => fields.split('\t').map(ToOwned::to_owned).collect(),
                None if fields.is_empty() => Vec::new(),
                None => return Err(BadLine(format!("desktops' names: {line:?}"))),
            };
            return Ok(Self::DesktopNames(names));
        }
        if let Some(fields) = line.strip_prefix("titles ") {
            let mut fields = fields.split('\t');
            let output = fields.next().unwrap_or_default();
            if output.is_empty() || output.contains(' ') {
                return Err(BadLine(format!("titles of what screen: {line:?}")));
            }
            let fields: Vec<&str> = fields.collect();
            let (pairs, rest) = fields.as_chunks::<2>();
            if !rest.is_empty() {
                return Err(BadLine(format!("a window without its title: {line:?}")));
            }
            let titles = pairs
                .iter()
                .map(|[id, title]| {
                    let id = id.parse().map_err(|e| BadLine(format!("an id: {e}")))?;
                    Ok((id, (*title).to_owned()))
                })
                .collect::<Result<_, BadLine>>()?;
            return Ok(Self::Titles(Titles {
                output: output.to_owned(),
                titles,
            }));
        }
        if let Some(rest) = line.strip_prefix("locale ") {
            return rest.parse().map(Self::Locale);
        }
        if let Some(rest) = line.strip_prefix("theme ") {
            return rest.parse().map(Self::Theme);
        }
        if let Some(rest) = line.strip_prefix("wallpaper ") {
            return rest.parse().map(Self::Wallpaper);
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("look") => {
                let glass = flag(words.next(), "glass", "opaque")?;
                let motion = flag(words.next(), "motion", "still")?;
                // Before #649 the pandas moved as the bars did.
                let lively = match words.next() {
                    None => motion,
                    word => flag(word, "lively", "calm")?,
                };
                Ok(Self::Look {
                    glass,
                    motion,
                    lively,
                })
            }
            Some("bar") => Ok(Self::Bar {
                output: word(&mut words, "output")?.to_owned(),
                form: word(&mut words, "form")?.parse()?,
                since: word(&mut words, "start")?
                    .parse()
                    .map_err(|e| BadLine(format!("start: {e}")))?,
                scale: word(&mut words, "scale")?
                    .parse()
                    .map_err(|e| BadLine(format!("scale: {e}")))?,
            }),
            Some("nobar") => Ok(Self::NoBar {
                output: word(&mut words, "output")?.to_owned(),
            }),
            Some("level") => Ok(Self::Level(word(&mut words, "level key")?.parse()?)),
            Some("outside") => Ok(Self::Outside),
            Some("peek") => {
                // The name last: it may hold spaces.
                let mut fields = line.splitn(5, ' ').skip(1);
                let number = |w: &str, what: &str| {
                    w.parse::<u64>()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                Ok(Self::Peek(Peek {
                    id: number(word(&mut fields, "agent")?, "agent")?,
                    kind: word(&mut fields, "peek")?.parse()?,
                    since: number(word(&mut fields, "since")?, "since")?,
                    name: fields.next().unwrap_or_default().to_owned(),
                }))
            }
            Some("no-peek") => Ok(Self::NoPeek),
            Some("screens-moved") => {
                let mut parts = line.split('\t');
                let mut words = parts.next().unwrap_or_default().split_whitespace().skip(1);
                let gone = flag(words.next(), "gone", "back")?;
                let built_in = flag(words.next(), "built-in", "external")?;
                let number = |w: &str, what: &str| {
                    w.parse::<usize>()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                let inches = number(word(&mut words, "inches")?, "inches")?;
                let desktop = number(word(&mut words, "desktop")?, "desktop")?;
                Ok(Self::ScreensMoved(ScreensMoved {
                    gone,
                    built_in,
                    inches: u32::try_from(inches).unwrap_or(0),
                    desktop,
                    apps: parts
                        .map(|a| {
                            if a == "-" {
                                String::new()
                            } else {
                                a.to_owned()
                            }
                        })
                        .collect(),
                }))
            }
            Some("agent-rows") => Ok(Self::AgentPandas(flag(words.next(), "pandas", "dots")?)),
            Some("copied") => Ok(Self::Copied {
                app: match word(&mut words, "application")? {
                    "-" => String::new(),
                    app => app.to_owned(),
                },
                clip: Clip::read(&mut words)?,
            }),
            Some("capture-copied") => Ok(Self::CaptureCopied {
                capture: word(&mut words, "capture")?
                    .parse()
                    .map_err(|e| BadLine(format!("capture: {e}")))?,
                clip: Clip::read(&mut words)?,
            }),
            Some("clipboard") => Ok(Self::Clipboard {
                output: word(&mut words, "output")?.to_owned(),
            }),
            Some("secretary-drawer") => Ok(Self::SecretaryDrawer {
                output: word(&mut words, "output")?.to_owned(),
                width: word(&mut words, "width")?
                    .parse()
                    .map_err(|e| BadLine(format!("width: {e}")))?,
            }),
            Some("secretary-peek") => Ok(Self::SecretaryPeek(
                word(&mut words, "peek")?
                    .parse()
                    .map_err(|e| BadLine(format!("peek: {e}")))?,
            )),
            Some("clipboard-rules") => Ok(Self::ClipboardRules(ClipRules {
                history: flag(words.next(), "on", "off")?,
                keep: ClipKeep::of(word(&mut words, "keep")?)?,
            })),
            Some("clipboard-clear") => Ok(Self::ClipboardClear),
            Some("capture") => CaptureBar::read(&mut words).map(Self::Capture),
            Some("capture-end") => Ok(Self::CaptureEnd),
            Some("recording") => RecordingShown::read(&mut words).map(Self::Recording),
            Some("recording-end") => Ok(Self::RecordingEnd),
            Some("lock") => LockShown::read(&mut words).map(Self::Lock),
            Some("unlocked") => Ok(Self::Unlocked),
            Some("a11y") => Ok(Self::A11y),
            Some("curtain") => CurtainShown::read(&mut words).map(Self::Curtain),
            Some("curtain-end") => Ok(Self::CurtainEnd),
            Some("leave-can") => LeaveCan::read(&mut words).map(Self::LeaveCan),
            Some("leave-end") => Ok(Self::LeaveEnd),
            Some("session-menu") => Ok(Self::SessionMenu {
                output: word(&mut words, "output")?.to_owned(),
            }),
            Some("focus") => Ok(Self::Focus(word(&mut words, "output")?.to_owned())),
            Some("bar-keyboard") => Ok(Self::BarKeyboard(
                Some(word(&mut words, "output")?)
                    .filter(|o| *o != "-")
                    .map(str::to_owned),
            )),
            Some("emoji") => {
                let output = word(&mut words, "output")?.to_owned();
                let mut number = |what: &str| -> Result<i32, BadLine> {
                    word(&mut words, what)?
                        .parse()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                let scale = number("scale")?;
                let (width, height) = (number("width")?, number("height")?);
                let place = word(&mut words, "place")?;
                let mut rect = || -> Result<Rect, BadLine> {
                    let mut number = |what: &str| -> Result<i32, BadLine> {
                        word(&mut words, what)?
                            .parse()
                            .map_err(|e| BadLine(format!("{what}: {e}")))
                    };
                    Ok(Rect {
                        x: number("x")?,
                        y: number("y")?,
                        width: number("width")?,
                        height: number("height")?,
                    })
                };
                let at = match place {
                    "caret" => EmojiAt::Caret(rect()?),
                    "window" => EmojiAt::Window(rect()?),
                    "screen" => EmojiAt::Screen,
                    other => return Err(BadLine(format!("{other:?}: caret, window or screen"))),
                };
                Ok(Self::Emoji {
                    output,
                    scale: u32::try_from(scale).map_err(|e| BadLine(format!("scale: {e}")))?,
                    width,
                    height,
                    at,
                })
            }
            Some("emoji-inserted") => Ok(Self::EmojiInserted),
            Some("join") => Ok(Self::Join),
            Some("caps") => Ok(Self::CapsLock(flag(words.next(), "on", "off")?)),
            Some("tray-token") => {
                let ask = word(&mut words, "click")?
                    .parse()
                    .map_err(|e| BadLine(format!("click: {e}")))?;
                let token = word(&mut words, "token")?.to_owned();
                if words.next().is_some() {
                    return Err(BadLine(format!("a token on one word: {line:?}")));
                }
                Ok(Self::TrayToken { ask, token })
            }
            Some("exported") => {
                let ask = word(&mut words, "ask")?
                    .parse()
                    .map_err(|e| BadLine(format!("ask: {e}")))?;
                let handle = word(&mut words, "handle")?;
                if words.next().is_some() {
                    return Err(BadLine(format!("a handle on one word: {line:?}")));
                }
                let handle = (handle != "-").then(|| handle.to_owned());
                Ok(Self::Exported { ask, handle })
            }
            Some("share-choose-end") => Ok(Self::ShareChooseEnd(
                word(&mut words, "request")?
                    .parse()
                    .map_err(|e| BadLine(format!("request: {e}")))?,
            )),
            Some("launcher") => Ok(Self::Launcher {
                output: word(&mut words, "output")?.to_owned(),
                scale: word(&mut words, "scale")?
                    .parse()
                    .map_err(|e| BadLine(format!("scale: {e}")))?,
            }),
            Some("recent") => Ok(Self::Recent(Recent {
                output: word(&mut words, "output")?.to_owned(),
                ids: words
                    .map(|w| w.parse().map_err(|e| BadLine(format!("an id: {e}"))))
                    .collect::<Result<_, _>>()?,
            })),
            Some("bar-windows") => Ok(Self::BarWindows(flag(words.next(), "shown", "hidden")?)),
            Some("bar-side") => Ok(Self::BarSide(word(&mut words, "side")?.parse()?)),
            Some("drop-over") => {
                let bar = word(&mut words, "bar")?.to_owned();
                let over = match word(&mut words, "screen")? {
                    "-" => None,
                    output => Some((
                        output.to_owned(),
                        word(&mut words, "desktop")?
                            .parse()
                            .map_err(|e| BadLine(format!("desktop: {e}")))?,
                    )),
                };
                Ok(Self::DropOver { bar, over })
            }
            Some("desk-screens") => Ok(Self::DeskScreens {
                bar: word(&mut words, "bar")?.to_owned(),
                screens: words.map(DeskScreen::read).collect::<Result<_, _>>()?,
            }),
            Some("bar-groups") => {
                let order: Vec<BarGroup> = word(&mut words, "order")?
                    .split(',')
                    .map(str::parse)
                    .collect::<Result<_, _>>()?;
                let [a, b, c] = order[..] else {
                    return Err(BadLine(format!("an order of three groups: {order:?}")));
                };
                if a == b || b == c || a == c {
                    return Err(BadLine(format!("a group twice in {order:?}")));
                }
                let hidden = word(&mut words, "hidden groups")?;
                let hidden: Vec<&str> = if hidden == "-" {
                    Vec::new()
                } else {
                    hidden.split(',').collect()
                };
                if let Some(bad) = hidden
                    .iter()
                    .find(|h| !["meeting", "today", "tasks", "agents"].contains(h))
                {
                    return Err(BadLine(format!("no bar group {bad:?}")));
                }
                Ok(Self::BarGroups(BarGroups {
                    order: [a, b, c],
                    meeting: !hidden.contains(&"meeting"),
                    today: !hidden.contains(&"today"),
                    tasks: !hidden.contains(&"tasks"),
                    agents: !hidden.contains(&"agents"),
                }))
            }
            Some("layout") => Ok(Self::Layout(
                word(&mut words, "layout")?
                    .parse()
                    .map_err(|e| BadLine(format!("layout: {e}")))?,
            )),
            Some("desktops") => {
                let output = word(&mut words, "output")?.to_owned();
                let number = |w: &str, what: &str| {
                    w.parse::<u8>().map_err(|e| BadLine(format!("{what}: {e}")))
                };
                let count = number(word(&mut words, "count")?, "count")?;
                let current = number(word(&mut words, "current")?, "current")?;
                let windows = words
                    .map(|w| {
                        // The application's id last: it may hold commas.
                        let fields: Vec<&str> = w.splitn(8, ',').collect();
                        let [desktop, x, y, width, height, look, id, app] = fields[..] else {
                            return Err(BadLine(format!("a window {w:?}")));
                        };
                        let share = |v: &str| {
                            v.parse::<u16>()
                                .ok()
                                .filter(|v| *v <= SHARE)
                                .ok_or_else(|| BadLine(format!("a share {v:?}")))
                        };
                        Ok(DeskWindow {
                            desktop: number(desktop, "desktop")?,
                            rect: [share(x)?, share(y)?, share(width)?, share(height)?],
                            look: Look::of(look)?,
                            id: id.parse().map_err(|e| BadLine(format!("an id: {e}")))?,
                            app: app.to_owned(),
                        })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(Self::Desktops(Desktops {
                    output,
                    count,
                    current,
                    windows,
                }))
            }
            Some("clock") => {
                let seconds = flag(words.next(), "seconds", "noseconds")?;
                let transition = word(&mut words, "transition")?.parse()?;
                let color = parse_color(word(&mut words, "colour")?)?;
                let font = words.collect::<Vec<_>>().join(" ");
                if font.is_empty() {
                    return Err(BadLine("no font".to_owned()));
                }
                Ok(Self::Clock(ClockLook {
                    font,
                    color,
                    transition,
                    seconds,
                }))
            }
            _ => Err(BadLine(format!("unknown line {line:?}"))),
        }
    }
}

impl fmt::Display for ToCompositor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Software => f.write_str("software"),
            Self::A11yLine(line) => write!(f, "a11y-line {}", a11y_text(line)),
            Self::A11yEnd => f.write_str("a11y-end"),
            Self::Wake(namespace) => write!(f, "wake {namespace}"),
            Self::Closing(namespace) => write!(f, "closing {namespace}"),
            Self::SecretaryDrawer(output) => write!(f, "secretary-drawer {output}"),
            Self::NextLayout => f.write_str("next-layout"),
            Self::Desktop { output, to } => match to {
                DesktopTo::At(index) => write!(f, "desktop {output} {index}"),
                DesktopTo::Next => write!(f, "desktop {output} next"),
                DesktopTo::Previous => write!(f, "desktop {output} previous"),
            },
            Self::Bring(id) => write!(f, "bring {id}"),
            Self::DeskTiles { bar, tiles } => {
                write!(f, "desk-tiles {bar}")?;
                for t in tiles {
                    let [x, y, width, height] = t.rect;
                    write!(f, " {},{},{x},{y},{width},{height}", t.output, t.desktop)?;
                }
                Ok(())
            }
            Self::Close(id) => write!(f, "close {id}"),
            Self::Overview => f.write_str("overview"),
            Self::WindowAction { id, action } => write!(f, "window-action {id} {}", action.name()),
            Self::SendTo {
                id,
                output,
                desktop,
            } => write!(f, "send-to {id} {output} {desktop}"),
            Self::WindowMenu(id) => write!(f, "window-menu {id}"),
            Self::Action(run) => run.write(f),
            Self::GoToAgent { id, row: None } => write!(f, "go-to-agent {id}"),
            Self::GoToAgent { id, row: Some(r) } => write!(
                f,
                "go-to-agent {id} {} {} {} {} {}",
                r.output, r.x, r.y, r.width, r.height
            ),
            Self::Ring(r) => write!(
                f,
                "ring {} {} {} {} {}",
                r.output, r.x, r.y, r.width, r.height
            ),
            Self::Plug(tool) => write!(f, "plug {tool}"),
            Self::Unplug(tool) => write!(f, "unplug {tool}"),
            Self::Decline => f.write_str("decline"),
            Self::Follow(tool) => write!(f, "follow {tool}"),
            Self::Briefing { open } => {
                write!(f, "briefing {}", if *open { "open" } else { "closed" })
            }
            Self::PeekClose => f.write_str("peek-close"),
            Self::SecretaryPeek { id, stays } => write!(f, "secretary-peek {id} {stays}"),
            Self::Meeting(near) => f.write_str(match near {
                Some(MeetingNear::Ten) => "meeting ten",
                Some(MeetingNear::Two) => "meeting two",
                None => "meeting none",
            }),
            Self::Copy(text) => write!(f, "copy {}", screen_name(text)),
            Self::Raise { pid, app } => {
                let app = app_word(app);
                write!(f, "raise {pid} {}", if app.is_empty() { "-" } else { &app })
            }
            Self::LogOut => f.write_str("log-out"),
            Self::TrayClick { ask, pid } => write!(f, "tray-click {ask} {pid}"),
            Self::Export { ask, namespace } => write!(f, "export {ask} {namespace}"),
            Self::Capture(ask) => ask.write(f),
            Self::Share(ask) => ask.write(f),
            Self::Auth(ask) => ask.write(f),
            Self::Record(ask) => ask.write(f),
            Self::Lock(ask) => ask.write(f),
            Self::Leave(ask) => ask.write(f),
            Self::Clip { paste, clip } => {
                write!(f, "clip {} ", if *paste { "paste" } else { "copy" })?;
                clip.write(f)
            }
            Self::ClipboardCount { items, pinned } => {
                write!(f, "clipboard-count {items} {pinned}")
            }
            Self::Emoji { keep, text } => {
                write!(f, "emoji {} {text}", if *keep { "keep" } else { "insert" })
            }
            Self::Launch(launch) => {
                write!(
                    f,
                    "launch\t{}\t{}\t{}",
                    launch.id,
                    if launch.terminal {
                        "terminal"
                    } else {
                        "window"
                    },
                    launch.dir
                )?;
                for arg in &launch.argv {
                    write!(f, "\t{arg}")?;
                }
                Ok(())
            }
        }
    }
}

impl FromStr for ToCompositor {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        if let Some(text) = line.strip_prefix("copy ") {
            return Ok(Self::Copy(text.to_owned()));
        }
        if let Some(text) = line.strip_prefix("a11y-line ") {
            return Ok(Self::A11yLine(text.to_owned()));
        }
        if line == "a11y-line" {
            return Ok(Self::A11yLine(String::new()));
        }
        if let Some(rest) = line.strip_prefix("share ") {
            return ShareAsk::read(rest).map(Self::Share);
        }
        if let Some(rest) = line.strip_prefix("action ") {
            return RunAction::read(rest).map(Self::Action);
        }
        if let Some(fields) = line.strip_prefix("launch\t") {
            let mut fields = fields.split('\t');
            let id = word(&mut fields, "application")?.to_owned();
            let terminal = flag(fields.next(), "terminal", "window")?;
            let dir = word(&mut fields, "directory")?.to_owned();
            let argv: Vec<String> = fields.map(str::to_owned).collect();
            let launch = Launch {
                id,
                terminal,
                dir,
                argv,
            };
            if !launch.fits() {
                return Err(BadLine("a launch with nothing to run".to_owned()));
            }
            return Ok(Self::Launch(launch));
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("software") => Ok(Self::Software),
            Some("plug") => Ok(Self::Plug(word(&mut words, "tool")?.to_owned())),
            Some("unplug") => Ok(Self::Unplug(word(&mut words, "tool")?.to_owned())),
            Some("decline") => Ok(Self::Decline),
            Some("follow") => Ok(Self::Follow(word(&mut words, "tool")?.to_owned())),
            Some("briefing") => Ok(Self::Briefing {
                open: flag(words.next(), "open", "closed")?,
            }),
            Some("peek-close") => Ok(Self::PeekClose),
            Some("secretary-peek") => {
                let mut number = |what: &str| -> Result<u64, BadLine> {
                    word(&mut words, what)?
                        .parse()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                Ok(Self::SecretaryPeek {
                    id: number("peek")?,
                    stays: u32::try_from(number("stays")?)
                        .map_err(|e| BadLine(format!("stays: {e}")))?,
                })
            }
            Some("meeting") => match words.next() {
                Some("ten") => Ok(Self::Meeting(Some(MeetingNear::Ten))),
                Some("two") => Ok(Self::Meeting(Some(MeetingNear::Two))),
                Some("none") => Ok(Self::Meeting(None)),
                _ => Err(BadLine("meeting: ten, two or none".into())),
            },
            Some("wake") => Ok(Self::Wake(word(&mut words, "namespace")?.to_owned())),
            Some("closing") => Ok(Self::Closing(word(&mut words, "namespace")?.to_owned())),
            Some("secretary-drawer") => Ok(Self::SecretaryDrawer(
                word(&mut words, "output")?.to_owned(),
            )),
            Some("next-layout") => Ok(Self::NextLayout),
            Some("log-out") => Ok(Self::LogOut),
            Some("tray-click") => {
                let mut number = |what: &str| -> Result<u64, BadLine> {
                    word(&mut words, what)?
                        .parse()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                let ask = number("click")?;
                let pid = u32::try_from(number("process")?)
                    .map_err(|e| BadLine(format!("process: {e}")))?;
                Ok(Self::TrayClick { ask, pid })
            }
            Some("export") => {
                let ask = word(&mut words, "ask")?
                    .parse()
                    .map_err(|e| BadLine(format!("ask: {e}")))?;
                let namespace = word(&mut words, "namespace")?.to_owned();
                Ok(Self::Export { ask, namespace })
            }
            Some("capture") => CaptureAsk::read(&mut words).map(Self::Capture),
            Some("record") => RecordAsk::read(&mut words).map(Self::Record),
            Some("lock") => LockAsk::read(&mut words).map(Self::Lock),
            Some("auth") => AuthAsk::read(&mut words).map(Self::Auth),
            Some("leave") => LeaveAsk::read(&mut words).map(Self::Leave),
            Some("clip") => Ok(Self::Clip {
                paste: flag(words.next(), "paste", "copy")?,
                clip: Clip::read(&mut words)?,
            }),
            Some("clipboard-count") => {
                let mut count = |what| {
                    word(&mut words, what)?
                        .parse::<u32>()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                Ok(Self::ClipboardCount {
                    items: count("items")?,
                    pinned: count("pinned")?,
                })
            }
            Some("emoji") => {
                let keep = flag(words.next(), "keep", "insert")?;
                let text = word(&mut words, "emoji")?;
                if !emoji_text(text) || words.next().is_some() {
                    return Err(BadLine("not an emoji".to_owned()));
                }
                Ok(Self::Emoji {
                    keep,
                    text: text.to_owned(),
                })
            }
            Some("desktop") => {
                let output = word(&mut words, "output")?.to_owned();
                let to = match word(&mut words, "desktop")? {
                    "next" => DesktopTo::Next,
                    "previous" => DesktopTo::Previous,
                    index => DesktopTo::At(
                        index
                            .parse()
                            .map_err(|e| BadLine(format!("desktop: {e}")))?,
                    ),
                };
                Ok(Self::Desktop { output, to })
            }
            Some("raise") => Ok(Self::Raise {
                pid: word(&mut words, "process")?
                    .parse()
                    .map_err(|e| BadLine(format!("process: {e}")))?,
                app: match word(&mut words, "application")? {
                    "-" => String::new(),
                    app => app.to_owned(),
                },
            }),
            Some("desk-tiles") => Ok(Self::DeskTiles {
                bar: word(&mut words, "bar")?.to_owned(),
                tiles: words
                    .map(|w| {
                        let fields: Vec<&str> = w.split(',').collect();
                        let [output, desktop, x, y, width, height] = fields[..] else {
                            return Err(BadLine(format!("a tile {w:?}")));
                        };
                        let n = |v: &str| {
                            v.parse::<i32>()
                                .map_err(|e| BadLine(format!("a tile's {v:?}: {e}")))
                        };
                        Ok(DeskTile {
                            output: output.to_owned(),
                            desktop: desktop
                                .parse()
                                .map_err(|e| BadLine(format!("a tile's desktop: {e}")))?,
                            rect: [n(x)?, n(y)?, n(width)?, n(height)?],
                        })
                    })
                    .collect::<Result<_, _>>()?,
            }),
            Some("bring") => Ok(Self::Bring(
                word(&mut words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
            )),
            Some("window-action") => Ok(Self::WindowAction {
                id: word(&mut words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
                action: WindowAction::read(word(&mut words, "action")?)?,
            }),
            Some("send-to") => Ok(Self::SendTo {
                id: word(&mut words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
                output: word(&mut words, "output")?.to_owned(),
                desktop: word(&mut words, "desktop")?
                    .parse()
                    .map_err(|e| BadLine(format!("desktop: {e}")))?,
            }),
            Some("window-menu") => Ok(Self::WindowMenu(
                word(&mut words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
            )),
            Some("overview") => Ok(Self::Overview),
            Some("a11y-end") => Ok(Self::A11yEnd),
            Some("close") => Ok(Self::Close(
                word(&mut words, "window")?
                    .parse()
                    .map_err(|e| BadLine(format!("window: {e}")))?,
            )),
            Some("go-to-agent") => {
                let id = word(&mut words, "agent")?
                    .parse()
                    .map_err(|e| BadLine(format!("agent: {e}")))?;
                let row = match words.next() {
                    None => None,
                    Some(output) => {
                        let mut number = |what: &str| -> Result<i32, BadLine> {
                            word(&mut words, what)?
                                .parse()
                                .map_err(|e| BadLine(format!("{what}: {e}")))
                        };
                        Some(Row {
                            output: output.to_owned(),
                            x: number("x")?,
                            y: number("y")?,
                            width: number("width")?,
                            height: number("height")?,
                        })
                    }
                };
                Ok(Self::GoToAgent { id, row })
            }
            Some("ring") => {
                let output = word(&mut words, "output")?.to_owned();
                let mut number = |what: &str| -> Result<i32, BadLine> {
                    word(&mut words, what)?
                        .parse()
                        .map_err(|e| BadLine(format!("{what}: {e}")))
                };
                Ok(Self::Ring(Row {
                    output,
                    x: number("x")?,
                    y: number("y")?,
                    width: number("width")?,
                    height: number("height")?,
                }))
            }
            _ => Err(BadLine(format!("unknown line {line:?}"))),
        }
    }
}

/// The form switch's curve, CSS's `cubic-bezier(0.2, 0.9, 0.25, 1.08)`, at
/// linear progress `x` from 0 to 1: a quick start and a slight overshoot.
pub fn ease(x: f64) -> f64 {
    const X1: f64 = 0.2;
    const Y1: f64 = 0.9;
    const X2: f64 = 0.25;
    const Y2: f64 = 1.08;
    let axis = |p1: f64, p2: f64, t: f64| {
        let u = 1.0 - t;
        3.0 * u * u * t * p1 + 3.0 * u * t * t * p2 + t * t * t
    };
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    // x(t) grows with t on this curve: bisection finds it.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..40 {
        let t = (lo + hi) / 2.0;
        if axis(X1, X2, t) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    axis(Y1, Y2, (lo + hi) / 2.0)
}

/// The bar's width `elapsed` after a switch to `to` began from `from`
/// pixels wide (logical pixels, fractional while it moves). A switch
/// reversed midway starts from where the bar is.
pub fn width_at(from: f64, to: Form, elapsed: Duration) -> f64 {
    let p = ease(elapsed.as_secs_f64() / SWITCH.as_secs_f64());
    from + (f64::from(to.width()) - from) * p
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the clipboard's history is told: a text with its line breaks,
    /// an image, from no application; `Super+V`.
    fn clips() -> Vec<ToShell> {
        vec![
            ToShell::Copied {
                app: "kitty".into(),
                clip: Clip {
                    kind: ClipKind::Text,
                    data: "cargo nextest run\nla suite".as_bytes().to_vec(),
                },
            },
            ToShell::Copied {
                app: String::new(),
                clip: Clip {
                    kind: ClipKind::Png,
                    data: (0..=255).collect(),
                },
            },
            ToShell::Copied {
                app: "org.gnome.Nautilus".into(),
                clip: Clip {
                    kind: ClipKind::Text,
                    data: Vec::new(),
                },
            },
            ToShell::CaptureCopied {
                capture: 3,
                clip: Clip {
                    kind: ClipKind::Png,
                    data: (0..=255).rev().collect(),
                },
            },
            ToShell::Clipboard {
                output: "eDP-1".into(),
            },
            ToShell::ClipboardRules(ClipRules::default()),
            ToShell::ClipboardRules(ClipRules {
                history: false,
                keep: ClipKeep::Session,
            }),
            ToShell::ClipboardClear,
            ToShell::SessionMenu {
                output: "eDP-1".into(),
            },
        ]
    }

    #[test]
    fn the_screens_whose_desktops_a_bar_shows_read_back_as_written() {
        let screens = ToShell::DeskScreens {
            bar: "eDP-1".into(),
            screens: vec![
                DeskScreen {
                    output: "eDP-1".into(),
                    width: 1707,
                    height: 1067,
                    built_in: true,
                    inches: 14,
                    number: 1,
                },
                DeskScreen {
                    output: "DP-1".into(),
                    width: 3440,
                    height: 1440,
                    built_in: false,
                    inches: 0,
                    number: 2,
                },
            ],
        };
        assert_eq!(
            screens.to_string(),
            "desk-screens eDP-1 eDP-1,1707,1067,1,14,1 DP-1,3440,1440,0,0,2"
        );
        assert_eq!(screens.to_string().parse::<ToShell>(), Ok(screens));
        assert!("desk-screens eDP-1 eDP-1,1,2".parse::<ToShell>().is_err());
    }

    #[test]
    fn the_tiles_and_a_drop_over_them_read_back_as_written() {
        let tiles = ToCompositor::DeskTiles {
            bar: "eDP-1".into(),
            tiles: vec![DeskTile {
                output: "DP-1".into(),
                desktop: 2,
                rect: [11, 51, 39, 20],
            }],
        };
        assert_eq!(tiles.to_string(), "desk-tiles eDP-1 DP-1,2,11,51,39,20");
        assert_eq!(tiles.to_string().parse::<ToCompositor>(), Ok(tiles));
        for line in [
            ToShell::DropOver {
                bar: "eDP-1".into(),
                over: Some(("DP-1".into(), 3)),
            },
            ToShell::DropOver {
                bar: "eDP-1".into(),
                over: None,
            },
        ] {
            assert_eq!(line.to_string().parse::<ToShell>(), Ok(line));
        }
    }

    #[test]
    fn the_emoji_pickers_lines_read_back_as_written() {
        let rect = Rect {
            x: 120,
            y: -4,
            width: 2,
            height: 18,
        };
        for line in [
            ToShell::Emoji {
                output: "eDP-1".into(),
                scale: 180,
                width: 1707,
                height: 1067,
                at: EmojiAt::Caret(rect),
            },
            ToShell::Emoji {
                output: "HDMI-A-1".into(),
                scale: 120,
                width: 1920,
                height: 1080,
                at: EmojiAt::Window(rect),
            },
            ToShell::Emoji {
                output: "HDMI-A-1".into(),
                scale: 120,
                width: 1920,
                height: 1080,
                at: EmojiAt::Screen,
            },
            ToShell::EmojiInserted,
            ToShell::TrayToken {
                ask: 7,
                token: "Xq3vZk0aPAbtUJjJ2hF0cO1DzQ9m4wKe".into(),
            },
        ] {
            assert_eq!(line.to_string().parse::<ToShell>(), Ok(line));
        }
        assert!("tray-token 7".parse::<ToShell>().is_err());
        for line in [
            ToShell::Exported {
                ask: 2,
                handle: Some("Rk2v8QwZp0aPAbtUJjJ2hF0cO1DzQ9m4".into()),
            },
            ToShell::Exported {
                ask: 3,
                handle: None,
            },
        ] {
            assert_eq!(line.to_string().parse::<ToShell>(), Ok(line));
        }
        assert!("exported 2".parse::<ToShell>().is_err());
        assert!("exported 2 a b".parse::<ToShell>().is_err());
        assert!("tray-token 7 a b".parse::<ToShell>().is_err());
        for line in [
            ToCompositor::Emoji {
                keep: false,
                text: "🐱".into(),
            },
            ToCompositor::Emoji {
                keep: true,
                text: "🧑🏿‍🤝‍🧑🏿".into(),
            },
        ] {
            assert_eq!(line.to_string().parse::<ToCompositor>(), Ok(line));
        }
        assert_eq!(
            "emoji eDP-1 120 1920 1080 caret 1 2 3 4".parse::<ToShell>(),
            Ok(ToShell::Emoji {
                output: "eDP-1".into(),
                scale: 120,
                width: 1920,
                height: 1080,
                at: EmojiAt::Caret(Rect {
                    x: 1,
                    y: 2,
                    width: 3,
                    height: 4
                }),
            })
        );
        for bad in [
            "emoji eDP-1 120 1920 1080",
            "emoji eDP-1 120 1920 1080 caret 1 2",
            "emoji eDP-1 120 1920 1080 middle",
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad}");
        }
        for bad in ["emoji insert", "emoji maybe 🐱", "emoji insert 🐱 🐶"] {
            assert!(bad.parse::<ToCompositor>().is_err(), "{bad}");
        }
        let long = "😀".repeat(20);
        assert!(!emoji_text(&long));
        assert!(!emoji_text("a\u{7}"));
    }

    #[test]
    fn a_clip_too_large_or_of_no_kind_does_not_read() {
        assert!("copied kitty html PGI+".parse::<ToShell>().is_err());
        assert!("copied kitty text not*base64".parse::<ToShell>().is_err());
        assert!("clip maybe text YQ==".parse::<ToCompositor>().is_err());
        assert!("clipboard".parse::<ToShell>().is_err());
        let clip = Clip {
            kind: ClipKind::Text,
            data: "hunter2".as_bytes().to_vec(),
        };
        // Never what it holds.
        assert_eq!(format!("{clip:?}"), "Clip(text, 7 bytes)");
        let large = ToCompositor::Clip {
            paste: true,
            clip: Clip {
                kind: ClipKind::Png,
                data: vec![0; CLIP_MAX + 1],
            },
        };
        assert!(large.to_string().parse::<ToCompositor>().is_err());
    }

    #[test]
    fn lines_read_back_as_written() {
        let to_shell = [
            ToShell::Look {
                glass: true,
                motion: false,
                lively: false,
            },
            ToShell::Look {
                glass: false,
                motion: true,
                lively: false,
            },
            ToShell::Theme(theme::Choice {
                scheme: theme::Scheme::Light,
                accent: theme::Accent::Violet,
                wallpaper: theme::AccentColours::lifted([0x3A, 0x2A, 0x26]),
                contrast: true,
            }),
            ToShell::Wallpaper(wallpaper::Wallpapers {
                lock: wallpaper::LockLook::Picture,
                lock_picture: wallpaper::Picture::File("~/a b.jpg".into()),
                picture: wallpaper::Picture::Builtin(backdrop::Backdrop::Forest),
                fit: wallpaper::Fit::Center,
                color: wallpaper::DEFAULT_COLOR,
                outputs: vec![("eDP-1".into(), wallpaper::Picture::Plain)],
            }),
            ToShell::Locale(locale::Locale {
                language: locale::Language::English,
                date: locale::DateFormat::Short,
                time: Some(locale::TimeFormat::H12),
            }),
            ToShell::Bar {
                output: "eDP-1".into(),
                form: Form::Mini,
                since: 123_456_789,
                scale: 180,
            },
            ToShell::NoBar {
                output: "DP-2".into(),
            },
            ToShell::Clock(ClockLook::default()),
            ToShell::Clock(ClockLook {
                font: "JetBrains Mono".into(),
                color: [0x3D, 0xFF, 0xA2],
                transition: Transition::Flip,
                seconds: false,
            }),
        ];
        let day = [
            ToShell::Day(DayAsk::Add("Acheter du pain, et du café".into())),
            ToShell::Day(DayAsk::Tick("18a2f-3".into())),
            ToShell::Day(DayAsk::Untick("c 2".into())),
            ToShell::Day(DayAsk::Refresh),
            ToShell::Day(DayAsk::Slept(900)),
            ToShell::Join,
            ToShell::Secretary(SecretaryShown::Off),
            ToShell::Secretary(SecretaryShown::Unavailable),
            ToShell::Secretary(SecretaryShown::Ready(
                "/run/user/1000/hocus 0.secretary".into(),
            )),
            ToShell::SecretaryAsk("Qui relit les PR ?".into()),
            ToShell::FellBack {
                which: Supervised::Shell,
                program: "/home/alex/my builds/hocuspanda-shell".into(),
            },
            ToShell::FellBack {
                which: Supervised::Secretary,
                program: "/tmp/hocus-secretary".into(),
            },
            ToShell::SecretaryDrawer {
                output: "eDP-1".into(),
                width: 1366,
            },
            ToShell::SecretaryPeek(3),
            ToShell::Fullscreen(vec!["HDMI-A-1".to_owned(), "eDP-1".to_owned()]),
            ToShell::Fullscreen(Vec::new()),
            ToShell::SharedScreens(vec!["HEADLESS-1".to_owned()]),
            ToShell::SharedScreens(Vec::new()),
            ToShell::Day(DayAsk::Calendar {
                day: "2026-11-10".into(),
                event: Some("e 12".into()),
            }),
            ToShell::Day(DayAsk::Calendar {
                day: "2026-11-04".into(),
                event: None,
            }),
        ];
        for message in &day {
            assert_eq!(message.to_string().parse::<ToShell>().as_ref(), Ok(message));
        }
        assert!("day add ".parse::<ToShell>().is_err());
        assert!("day plant x".parse::<ToShell>().is_err());
        assert_eq!(
            ToShell::Day(DayAsk::Add("deux\nlignes".into())).to_string(),
            "day add deux lignes"
        );
        let keys = LevelKey::ALL.map(ToShell::Level);
        let screens = [
            ToShell::Screens(Vec::new()),
            ToShell::Screens(vec!["DELL U3419W".into(), "HDMI-A-1".into()]),
        ];
        let layouts = [
            ToShell::Layouts(Vec::new()),
            ToShell::Layouts(vec![
                KeyLayout {
                    code: "fr".into(),
                    name: "French".into(),
                },
                KeyLayout {
                    code: "fr(bepo)".into(),
                    name: "French (BEPO)".into(),
                },
                KeyLayout {
                    code: String::new(),
                    name: "English (US)".into(),
                },
            ]),
            ToShell::Layout(0),
            ToShell::Layout(2),
            ToShell::Lock(LockShown {
                phase: LockPhase::Waiting(27),
                chars: 0,
                caps: true,
                shake: 3,
                focus: LockFocus::Layout,
                agents: 2,
                wallpaper: true,
                motion: false,
                output: "eDP-1".into(),
            }),
            ToShell::Lock(LockShown {
                phase: LockPhase::Typing,
                chars: 7,
                caps: false,
                shake: 0,
                focus: LockFocus::Field,
                agents: 0,
                wallpaper: false,
                motion: true,
                output: "HDMI-A-1".into(),
            }),
            ToShell::Unlocked,
            ToShell::A11y,
            ToShell::Curtain(CurtainShown {
                elapsed: 0,
                length: 10_000,
                top: 1010,
                motion: true,
                output: "eDP-1".into(),
            }),
            ToShell::Curtain(CurtainShown {
                elapsed: 4_250,
                length: 10_000,
                top: 922,
                motion: false,
                output: "HDMI-A-1".into(),
            }),
            ToShell::CurtainEnd,
        ];
        let desktops = [
            ToShell::Desktops(Desktops {
                output: "eDP-1".into(),
                count: 4,
                current: 0,
                windows: Vec::new(),
            }),
            ToShell::Desktops(Desktops {
                output: "HDMI-A-1".into(),
                count: 9,
                current: 8,
                windows: vec![
                    DeskWindow {
                        desktop: 0,
                        rect: [0, 0, SHARE, SHARE],
                        look: Look::Plain,
                        id: 3,
                        app: String::new(),
                    },
                    DeskWindow {
                        desktop: 8,
                        rect: [20, 30, 600, 940],
                        look: Look::Focused,
                        id: 12,
                        app: "com.example,odd".into(),
                    },
                    DeskWindow {
                        desktop: 8,
                        rect: [0, 0, 0, 0],
                        look: Look::Minimized,
                        id: 13,
                        app: "foot".into(),
                    },
                ],
            }),
            ToShell::Titles(Titles {
                output: "eDP-1".into(),
                titles: Vec::new(),
            }),
            ToShell::Titles(Titles {
                output: "eDP-1".into(),
                titles: vec![(3, "~/hocus — fish".into()), (12, String::new())],
            }),
            ToShell::Recent(Recent {
                output: "eDP-1".into(),
                ids: vec![12, 3, 13],
            }),
            ToShell::Recent(Recent {
                output: "HDMI-A-1".into(),
                ids: Vec::new(),
            }),
            ToShell::BarWindows(true),
            ToShell::BarWindows(false),
            ToShell::DesktopNames(Vec::new()),
            ToShell::DesktopNames(vec!["Travail".into(), String::new(), "Mail · perso".into()]),
            ToShell::BarSide(Side::Left),
            ToShell::BarSide(Side::Right),
            ToShell::BarGroups(BarGroups::default()),
            ToShell::BarGroups(BarGroups {
                order: [BarGroup::Tasks, BarGroup::Today, BarGroup::Agents],
                meeting: false,
                today: true,
                tasks: false,
                agents: true,
            }),
        ];
        for message in to_shell
            .into_iter()
            .chain(keys)
            .chain([
                ToShell::Outside,
                ToShell::Focus("DP-2".into()),
                ToShell::BarKeyboard(Some("DP-2".into())),
                ToShell::BarKeyboard(None),
            ])
            .chain(clips())
            .chain(screens)
            .chain(layouts)
            .chain(desktops)
            .chain(agents())
        {
            assert_eq!(message.to_string().parse::<ToShell>(), Ok(message));
        }
        for message in [
            ToCompositor::Software,
            ToCompositor::Wake(LEVELS_NAMESPACE.to_owned()),
            ToCompositor::Wake(CONNECTIONS_NAMESPACE.to_owned()),
            ToCompositor::Closing(POPOVER_NAMESPACE.to_owned()),
            ToCompositor::SecretaryDrawer("eDP-1".to_owned()),
            ToCompositor::NextLayout,
            ToCompositor::Lock(LockAsk::Submit),
            ToCompositor::Lock(LockAsk::Focus(LockFocus::Button)),
            ToCompositor::Lock(LockAsk::Now),
            ToCompositor::Desktop {
                output: "eDP-1".into(),
                to: DesktopTo::At(3),
            },
            ToCompositor::Desktop {
                output: "eDP-1".into(),
                to: DesktopTo::Next,
            },
            ToCompositor::Desktop {
                output: "eDP-1".into(),
                to: DesktopTo::Previous,
            },
            ToCompositor::Bring(42),
            ToCompositor::Close(42),
            ToCompositor::Overview,
            ToCompositor::A11yLine("  button \"14 h 32\" <click>".into()),
            ToCompositor::A11yEnd,
            ToCompositor::GoToAgent { id: 7, row: None },
            ToCompositor::PeekClose,
            ToCompositor::SecretaryPeek { id: 3, stays: 12 },
            ToCompositor::Ring(Row {
                output: "eDP-1".into(),
                x: -190,
                y: 14,
                width: 172,
                height: 77,
            }),
            ToCompositor::GoToAgent {
                id: 7,
                row: Some(Row {
                    output: "eDP-1".into(),
                    x: -190,
                    y: 126,
                    width: 184,
                    height: 28,
                }),
            },
            ToCompositor::Plug("claude-code".into()),
            ToCompositor::Unplug("codex".into()),
            ToCompositor::Decline,
            ToCompositor::LogOut,
            ToCompositor::Follow("herdr".into()),
            ToCompositor::Briefing { open: true },
            ToCompositor::Briefing { open: false },
            ToCompositor::Meeting(Some(MeetingNear::Ten)),
            ToCompositor::Meeting(Some(MeetingNear::Two)),
            ToCompositor::Meeting(None),
            ToCompositor::Copy("hocus agent my-agent waiting".into()),
            ToCompositor::Clip {
                paste: true,
                clip: Clip {
                    kind: ClipKind::Text,
                    data: "git log\n\tétoile ✦".as_bytes().to_vec(),
                },
            },
            ToCompositor::Clip {
                paste: false,
                clip: Clip {
                    kind: ClipKind::Png,
                    data: vec![0x89, b'P', b'N', b'G', 0, 255],
                },
            },
            ToCompositor::ClipboardCount {
                items: 143,
                pinned: 3,
            },
            ToCompositor::Raise {
                pid: 4242,
                app: "org.mozilla.firefox".into(),
            },
            ToCompositor::Raise {
                pid: 0,
                app: String::new(),
            },
            ToCompositor::TrayClick { ask: 3, pid: 4242 },
            ToCompositor::TrayClick { ask: 4, pid: 0 },
            ToCompositor::Export {
                ask: 5,
                namespace: "hocus-capture-editor".into(),
            },
            ToCompositor::Launch(Launch {
                id: "org.mozilla.firefox".into(),
                terminal: false,
                dir: String::new(),
                argv: vec!["firefox".into(), "--new-window".into()],
            }),
            ToCompositor::Launch(Launch {
                id: "htop".into(),
                terminal: true,
                dir: "/home/alex/my projects".into(),
                argv: vec!["htop".into(), String::new(), "a b".into()],
            }),
        ] {
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
        assert!("wake".parse::<ToCompositor>().is_err());
        assert!("tray-click 3".parse::<ToCompositor>().is_err());
        assert!("export 5".parse::<ToCompositor>().is_err());
        assert!("tray-click 3 -1".parse::<ToCompositor>().is_err());
        assert!("closing".parse::<ToCompositor>().is_err());
        assert!("briefing".parse::<ToCompositor>().is_err());
        assert!("briefing shut".parse::<ToCompositor>().is_err());
        assert!("launch\tx\twindow\t".parse::<ToCompositor>().is_err());
        assert!("launch\tx\tmaybe\t\tfoot".parse::<ToCompositor>().is_err());
        assert!("launcher eDP-1".parse::<ToShell>().is_err());
        assert!("desktop eDP-1".parse::<ToCompositor>().is_err());
        assert!("close".parse::<ToCompositor>().is_err());
        assert!("close me".parse::<ToCompositor>().is_err());
        assert!("desktop eDP-1 last".parse::<ToCompositor>().is_err());
        assert!(
            "go-to-agent 7 eDP-1 -190 126 184"
                .parse::<ToCompositor>()
                .is_err()
        );
        for bad in [
            "",
            "bar eDP-1",
            "bar eDP-1 wide 0",
            "look glass",
            "hello",
            "clock seconds odometer #EEF1F6",
            "clock seconds spin #EEF1F6 Martian Mono",
            "clock seconds odometer white Martian Mono",
            "level",
            "level louder",
            "layout",
            "layout first",
            "layouts\tfr",
            "layoutsfr\tFrench",
            "desktops eDP-1 4",
            "desktops eDP-1 4 0 0,1,2,3,4",
            "desktops eDP-1 4 0 0,1,2,3,1001,0",
            "desktops eDP-1 4 0 0,1,2,3,4,yes",
            "desktops eDP-1 4 0 0,1,2,3,4,1",
            "desktops eDP-1 4 0 0,1,2,3,4,1,x,foot",
            "titles",
            "titles eDP-1\t3",
            "titles eDP-1\tthree\tfish",
            "recent",
            "recent eDP-1 12 x",
            "bar-windows",
            "bar-windows maybe",
            "agent-rows",
            "agent-rows panda",
            "bar-side",
            "bar-side up",
            "day slept",
            "day slept soon",
            "fullscreenHDMI-A-1",
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_bubbles_namespace_holds_its_number() {
        assert_eq!(bubble_namespace(7), "hocus-bubble-7");
        assert!(is_bubble(&bubble_namespace(12)));
        for not in [
            "hocus-bubble-",
            "hocus-bubble-x",
            BUBBLES_WAITING_NAMESPACE,
            NAMESPACE,
        ] {
            assert!(!is_bubble(not), "{not}");
        }
    }

    #[test]
    fn a_reminders_namespace_holds_its_number_and_slides_as_a_bubble() {
        assert_eq!(reminder_namespace(3), "hocus-reminder-3");
        assert!(is_reminder(&reminder_namespace(12)));
        assert!(slides_in(&reminder_namespace(1)) && slides_in(&bubble_namespace(1)));
        for not in ["hocus-reminder-", "hocus-reminder-x", MEETING_NAMESPACE] {
            assert!(!is_reminder(not) && !slides_in(not), "{not}");
        }
    }

    fn agents() -> Vec<ToShell> {
        vec![
            ToShell::AgentTools(Vec::new()),
            ToShell::AgentTools(vec![
                AgentTool {
                    tool: "claude-code".into(),
                    state: ToolState::Found,
                    file: "~/.claude/settings.json".into(),
                },
                AgentTool {
                    tool: "codex".into(),
                    state: ToolState::Plugged,
                    file: "/opt/my codex/config.toml".into(),
                },
                AgentTool {
                    tool: "herdr".into(),
                    state: ToolState::Ignored,
                    file: String::new(),
                },
            ]),
            ToShell::Agents(Vec::new()),
            ToShell::Agents(vec![AgentShown {
                id: 3,
                state: "waiting".into(),
                since: 123_456,
                source: "claude-code+herdr".into(),
                window: 0,
                name: "hocus panda".into(),
                note: None,
            }]),
            ToShell::Agents(vec![
                AgentShown {
                    id: 3,
                    state: "waiting".into(),
                    since: 1,
                    source: "claude-code".into(),
                    window: 7,
                    name: "site-web".into(),
                    note: Some(Note::allow("Bash", "npm install")),
                },
                AgentShown {
                    id: 4,
                    state: "failed".into(),
                    since: 2,
                    source: "claude-code".into(),
                    window: 0,
                    name: "migration".into(),
                    note: Note::says("API Error: Rate limit reached"),
                },
                AgentShown {
                    id: 5,
                    state: "waiting".into(),
                    since: 2,
                    source: "codex".into(),
                    window: 0,
                    name: "infra".into(),
                    note: Some(Note::allow("apply_patch", "")),
                },
            ]),
            ToShell::Peek(Peek {
                id: 3,
                kind: PeekKind::Waiting,
                since: 123_456,
                name: "hocus panda · refacto".into(),
            }),
            ToShell::Peek(Peek {
                id: 4,
                kind: PeekKind::Done,
                since: 0,
                name: "api-paiements".into(),
            }),
            ToShell::NoPeek,
            ToShell::ScreensMoved(ScreensMoved {
                gone: true,
                built_in: false,
                inches: 34,
                desktop: 1,
                apps: vec!["firefox".into(), "foot".into(), String::new()],
            }),
            ToShell::ScreensMoved(ScreensMoved {
                gone: false,
                built_in: true,
                inches: 0,
                desktop: 0,
                apps: vec![],
            }),
            ToShell::AgentPandas(true),
            ToShell::AgentPandas(false),
            ToShell::Launcher {
                output: "eDP-1".into(),
                scale: 240,
            },
        ]
    }

    #[test]
    fn a_note_is_one_short_line_and_never_logged() {
        // Its line breaks, tabs and runs of spaces made one space.
        assert_eq!(
            note_text("  npm\tinstall \n --save\r\n"),
            "npm install --save"
        );
        // Cut to its most, the last character an ellipsis.
        let long = note_text(&"é".repeat(NOTE_MAX + 30));
        assert_eq!(long.chars().count(), NOTE_MAX);
        assert!(long.ends_with('…'));
        assert_eq!(note_text(&"a".repeat(NOTE_MAX)), "a".repeat(NOTE_MAX));
        // Nothing said: no note.
        assert_eq!(Note::says(" \n "), None);
        // A request's tool is one word.
        assert_eq!(
            Note::allow("my tool", "x"),
            Note::Allow {
                tool: "my_tool".into(),
                what: "x".into()
            }
        );
        // Read back as written; a text that holds tabs stays on its field.
        let note = Note::says("a\tb");
        let line = ToShell::Agents(vec![AgentShown {
            id: 1,
            state: "failed".into(),
            since: 0,
            source: "command".into(),
            window: 0,
            name: "x".into(),
            note: note.clone(),
        }])
        .to_string();
        assert!(line.ends_with("\tsays a b"), "{line:?}");
        let Ok(ToShell::Agents(read)) = line.parse::<ToShell>() else {
            panic!("{line:?}");
        };
        assert_eq!(read[0].note, Some(Note::Says("a b".into())));
        // Its length only, in what is logged.
        let secret = Note::allow("Bash", "export TOKEN=hunter2");
        assert_eq!(format!("{secret:?}"), "allow (24 characters)");
        assert!(!format!("{:?}", read[0]).contains("a b"));
        assert!("agents\t1 idle 0 command 0 x".parse::<ToShell>().is_err());
        assert!(
            "agents\t1 idle 0 command 0 x\tnope"
                .parse::<ToShell>()
                .is_err()
        );
    }

    #[test]
    fn a_title_and_an_app_id_stay_on_their_line() {
        let line = ToShell::Titles(Titles {
            output: "eDP-1".into(),
            titles: vec![(1, " a\tb\nc ".into()), (2, "é".repeat(TITLE_MAX + 5))],
        })
        .to_string();
        let Ok(ToShell::Titles(read)) = line.parse::<ToShell>() else {
            panic!("{line:?}");
        };
        assert_eq!(read.titles[0], (1, "a b c".to_owned()));
        assert_eq!(read.titles[1].1.chars().count(), TITLE_MAX);
        let desktops = ToShell::Desktops(Desktops {
            output: "eDP-1".into(),
            count: 1,
            current: 0,
            windows: vec![DeskWindow {
                desktop: 0,
                rect: [0, 0, 1, 1],
                look: Look::Plain,
                id: 1,
                app: "my app".into(),
            }],
        });
        let Ok(ToShell::Desktops(read)) = desktops.to_string().parse::<ToShell>() else {
            panic!();
        };
        assert_eq!(read.windows[0].app, "my_app");
    }

    #[test]
    fn a_screens_name_stays_on_its_line() {
        assert_eq!(screen_name(" LG\tULTRA\nGEAR "), "LG ULTRA GEAR");
        let line = ToShell::Screens(vec!["LG\tULTRA".into()]).to_string();
        assert_eq!(line, "screens\tLG ULTRA");
        assert!("screensaver".parse::<ToShell>().is_err());
    }

    #[test]
    fn colours_read_in_both_lengths() {
        assert_eq!(parse_color("#EEF1F6"), Ok([0xEE, 0xF1, 0xF6]));
        assert_eq!(parse_color("#3dffa2"), Ok([0x3D, 0xFF, 0xA2]));
        assert_eq!(parse_color("#fff"), Ok([255, 255, 255]));
        for bad in ["EEF1F6", "#EEF1F", "#GGGGGG", "#", "#+1+2+3"] {
            assert!(parse_color(bad).is_err(), "{bad}");
        }
        for t in Transition::ALL {
            assert_eq!(t.name().parse::<Transition>(), Ok(t));
        }
    }

    #[test]
    fn the_default_form_follows_the_width_left_beside_the_bar() {
        // bar.md §3's table, in logical pixels.
        assert_eq!(Form::default_for(1366), Form::Mini);
        assert_eq!(Form::default_for(1920), Form::Full);
        assert_eq!(Form::default_for(1280), Form::Mini);
        assert_eq!(Form::default_for(2560), Form::Full);
        assert_eq!(Form::default_for(3440), Form::Full);
        assert_eq!(Form::default_for(1796), Form::Full);
        assert_eq!(Form::default_for(1795), Form::Mini);
    }

    #[test]
    fn the_switch_overshoots_a_little_and_lands() {
        assert_eq!(width_at(196.0, Form::Mini, Duration::ZERO), 196.0);
        assert_eq!(width_at(196.0, Form::Mini, SWITCH), 56.0);
        let narrowest = (1..240)
            .map(|ms| width_at(196.0, Form::Mini, Duration::from_millis(ms)))
            .fold(f64::MAX, f64::min);
        assert!((44.0..56.0).contains(&narrowest), "{narrowest}");
        assert!(ease(0.3) > 0.7, "quick to start");
    }

    #[test]
    fn the_bar_order_takes_known_names_once_then_the_missing_ones() {
        use BarGroup::{Agents, Tasks, Today};
        assert_eq!(BarGroup::order_of::<&str>(&[]), [Today, Tasks, Agents]);
        assert_eq!(
            BarGroup::order_of(&["tasks", "today", "agents"]),
            [Tasks, Today, Agents]
        );
        // Unknown and repeated names skipped, the missing appended.
        assert_eq!(
            BarGroup::order_of(&["agents", "windows", "agents"]),
            [Agents, Today, Tasks]
        );
        assert!("meeting".parse::<BarGroup>().is_err());
        // A line with a group twice, or two, does not read.
        assert!(
            "bar-groups today,today,agents -"
                .parse::<ToShell>()
                .is_err()
        );
        assert!("bar-groups today,tasks -".parse::<ToShell>().is_err());
        assert!(
            "bar-groups today,tasks,agents windows"
                .parse::<ToShell>()
                .is_err()
        );
    }
}
