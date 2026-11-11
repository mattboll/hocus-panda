//! The names the two sides know each other's surfaces by (their layer
//! surfaces' namespaces), the environment variables the compositor hands
//! the shell, and the measures and durations both draw by.

use std::time::Duration;

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
/// bar's ([`RADIUS`](crate::RADIUS)).
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
