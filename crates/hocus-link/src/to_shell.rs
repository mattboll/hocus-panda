//! What the compositor tells the shell, and what those messages carry
//! that is theirs alone.

use serde::{Deserialize, Serialize};

use crate::{
    ActionsShown, AgentShown, AgentTool, AuthAt, BadLine, BarGroups, CaptureBar, Captured, Clip,
    ClipRules, ClockLook, CurtainShown, DeskScreen, Desktops, Form, KeyLayout, LeaveCan,
    LeaveShown, LevelKey, LockShown, Recent, Recorded, RecordingShown, SHARE, ShareChoose,
    ShareShown, Side, Titles, WindowMenu, WindowStates, locale, text::tidy, theme, wallpaper, wire,
};

/// What the compositor tells the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum ToShell {
    /// Says what the shell shows, for screen readers (`hocus msg a11y`,
    /// docs/design/screen-reader.md): answered by [`ToCompositor::A11yLine`](crate::ToCompositor::A11yLine)s
    /// and an [`ToCompositor::A11yEnd`](crate::ToCompositor::A11yEnd).
    A11y,
    /// How bars look: on glass the compositor blurs behind (else opaque),
    /// whether they move (else reduced motion), and whether the pandas play
    /// their moods (#649: not with reduced motion nor in power saver).
    Look {
        glass: bool,
        motion: bool,
        lively: bool,
    },
    /// The theme and the accent the shell draws in (`[appearance] theme` and
    /// `accent`, #402): sent as the shell connects and when the file changes
    /// them, or the primary screen's wallpaper while the accent is the
    /// wallpaper's (6.3).
    Theme(theme::Choice),
    /// The desktop's language and formats (`[locale]`, #402): sent as the shell
    /// connects and when the file changes them.
    Locale(locale::Locale),
    /// Each screen's wallpaper and the lock's (`[wallpaper]`, 6.3): sent as the
    /// shell connects and when the file or the screens change them.
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
    /// The screens other than the one the backlight lights (the built-in panel;
    /// without one, the first screen), by the names people know them by: their
    /// EDID's model, else their connector's. Sent as the shell connects and
    /// when they change: the brightness's popover says that they are set with
    /// their own buttons.
    Screens(#[serde(serialize_with = "tidy::names")] Vec<String>),
    /// The keyboard's layouts, in the keymap's order (XKB's groups): sent as
    /// the shell connects.
    Layouts(Vec<KeyLayout>),
    /// The active layout, its index in [`ToShell::Layouts`](crate::ToShell::Layouts)' list: sent as the
    /// shell connects and whenever it changes (the bar's row, an XKB
    /// group-switch option).
    Layout(u32),
    /// A bar's screen's desktops: sent as the shell connects, and when they
    /// change, at most once a frame (bar.md, 3.9).
    Desktops(Desktops),
    /// The titles of a bar's screen's windows, for the list a desktop's tile
    /// opens on hover (3.9b): apart from [`ToShell::Desktops`](crate::ToShell::Desktops), so that a title
    /// that changes often (a terminal's) draws only an open list, never the
    /// tiles. Sent as the shell connects, and when one changes, at most once a
    /// frame.
    Titles(Titles),
    /// A bar's screen's windows, every desktop's, minimised ones too, the most
    /// recently focused first, by their ids: the order of the windows in the
    /// bar (#604). Sent as the shell connects, and when it changes, at most
    /// once a frame.
    Recent(Recent),
    /// The screens whose desktops bar `bar` shows, one row of tiles each (8.4):
    /// its own first, then the others left to right, then top to bottom. Sent
    /// as the shell connects and when they change; each one's desktops come on
    /// its own `desktops` line.
    DeskScreens {
        bar: String,
        screens: Vec<DeskScreen>,
    },
    /// A window being moved is over desktop `desktop` (from 0) of screen
    /// `output` in bar `bar`'s tiles, or over none of them any more (8.5 B):
    /// that tile lights, as where the window would go.
    DropOver {
        bar: String,
        over: Option<(String, u8)>,
    },
    /// Whether the bar shows its screen's windows (`[panel] windows`, #604):
    /// sent as the shell connects and when the configuration changes it.
    BarWindows(bool),
    /// The desktops' names (`[desktops] names`, 6.14), by position, the same on
    /// every screen; an empty one or one missing is "Bureau N": sent as the
    /// shell connects and when the configuration changes them.
    DesktopNames(#[serde(serialize_with = "tidy::titles")] Vec<String>),
    /// Which of the bar's column's groups show, and the order of those that
    /// move (`[panel] meeting`, `today`, `tasks`, `agents`, `order`, 6.4): sent
    /// as the shell connects and when the configuration changes them.
    BarGroups(BarGroups),
    /// The side the bars stand on (`[panel] side`, 6.4): sent as the shell
    /// connects, before the bars, and when the configuration changes it, each
    /// bar then made again on that side (`no-bar`, then `bar`).
    BarSide(Side),
    /// The agents' tools on this computer and what each is to the desktop
    /// (4.0): sent as the shell connects and when they change.
    AgentTools(Vec<AgentTool>),
    /// The agents followed, and what each is doing: sent as the shell connects
    /// and when they change, the changes of a turn of the compositor's loop
    /// together (4.0).
    Agents(Vec<AgentShown>),
    /// An agent's panda peeks out of the bar's edge (4.5): it started waiting,
    /// finished, or waits still at its reminder. Sent after the
    /// [`ToShell::Agents`](crate::ToShell::Agents) line that says so; the latest replaces the one out.
    Peek(Peek),
    /// The panda is back in the bar: its surfaces go.
    NoPeek,
    /// A screen went, or came back, and windows moved with it (windows.md §12):
    /// the winking panda peeks out with a blue bubble that says so, in place of
    /// a peek out unless that is an agent's, which it never replaces; it goes
    /// back as a peek does (`no-peek`).
    ScreensMoved(ScreensMoved),
    /// Whether the agents' rows show each agent's panda, or a dot of its
    /// state's colour (`[agents] pandas`, 6.16): sent as the shell connects and
    /// when the configuration changes it.
    AgentPandas(bool),
    /// The screen of the focused window, else the pointer's: where the
    /// notifications' bubbles show (5.3). Sent as the shell connects and when
    /// it changes.
    Focus(String),
    /// `Super+Space` (5.1): the launcher opens on `output`, the screen under
    /// the pointer, whose scale is `scale` (120ths, as [`ToShell::Bar`](crate::ToShell::Bar)'s);
    /// open, it closes.
    Launcher { output: String, scale: u32 },
    /// A client took the clipboard (5.6): what it offered, read off the
    /// compositor's loop, and the application of the window focused as it
    /// copied (its id; empty: none), for the history. Never a selection marked
    /// secret.
    Copied { app: String, clip: Clip },
    /// A capture (5.8b) for the clipboard's history: its PNG, the compositor's
    /// `capture`th of the session. The history lists it as copied from
    /// [`CAPTURE_APP`](crate::CAPTURE_APP); the same capture told again (its annotated image, 5.9)
    /// replaces the item it made.
    CaptureCopied { capture: u64, clip: Clip },
    /// `Super+V` (5.6): the clipboard's history opens on `output`'s bar, the
    /// screen of the focused window, else the pointer's, its field holding the
    /// keyboard; open, it closes.
    Clipboard { output: String },
    /// What the clipboard's history keeps (`[clipboard] history` and `keep`,
    /// 6.17): sent as the shell connects and when the configuration changes
    /// them.
    ClipboardRules(ClipRules),
    /// The Settings' "Effacer l'historique" (6.17): all but the pinned items
    /// go, as the history's own "Tout effacer".
    ClipboardClear,
    /// The screen capture is open (5.8), or what its toolbar shows changed
    /// ([`CaptureBar`]).
    Capture(CaptureBar),
    /// The capture's toolbar goes: the capture is taken, counts down, or was
    /// cancelled.
    CaptureEnd,
    /// A capture was taken, copied and saved: its thumbnail, or its toast
    /// when it was not saved ([`Captured`]).
    Captured(Captured),
    /// The emoji picker's shortcut (5.7, `Super+.`): it opens on `output` (its
    /// scale in 120ths, its size `width` × `height` logical pixels) at `at`, or
    /// closes when it is open.
    Emoji {
        output: String,
        scale: u32,
        width: i32,
        height: i32,
        at: EmojiAt,
    },
    /// An emoji inserted with the picker kept open (`Shift+Enter`) went into
    /// its window, or could not: the picker takes the keyboard back.
    EmojiInserted,
    /// A request to share the screen (5.11): the chooser opens on its
    /// screen, or shows its sources anew ([`ShareChoose`]).
    ShareChoose(ShareChoose),
    /// The chooser of that request goes.
    ShareChooseEnd(u64),
    /// Where the system's authentication dialog goes (7.5, [`AuthAt`]).
    AuthAt(AuthAt),
    /// Caps Lock is on, or off (7.5): sent as it changes and as the shell
    /// connects.
    CapsLock(bool),
    /// What is shared now (5.11): sent as the shell connects and when it
    /// changes; empty, nothing is ([`ShareShown`]).
    Shares(Vec<ShareShown>),
    /// A recording under way (5.10): the bar's item, its time and whether
    /// it is paused ([`RecordingShown`]); sent as it starts, pauses and
    /// goes on, and as the shell connects.
    Recording(RecordingShown),
    /// No recording any more: the bar's item goes.
    RecordingEnd,
    /// A recording saved: its thumbnail ([`Recorded`]).
    Recorded(Recorded),
    /// The activation token for the tray's click `ask`
    /// ([`ToCompositor::TrayClick`](crate::ToCompositor::TrayClick), 5.14b): the shell gives it to the item
    /// before activating it.
    TrayToken { ask: u64, token: String },
    /// The xdg-foreign handle made for the shell's surface asked for in `ask`
    /// ([`ToCompositor::Export`](crate::ToCompositor::Export), #595), `None` without such a surface: the
    /// shell names it to the desktop portal as its dialog's parent
    /// (`wayland:HANDLE`).
    Exported { ask: u64, handle: Option<String> },
    /// A change asked of the day's tasks, or a read of every account
    /// (docs/design/day.md): what the bar's own views ask, for tests and tools
    /// that have no view to click.
    Day(DayAsk),
    /// `Super+J` (`[shortcuts] join`, Day.6): the meeting whose reminder shows
    /// is joined, else the one starting next within 10 min; nothing without
    /// one.
    Join,
    /// A program given for this session in place of an installed one (`hocus
    /// msg shell restart --program`, docs/workflow.md) did not start or kept
    /// ending, and the installed one runs again: the shell says so in a
    /// notification.
    FellBack {
        which: Supervised,
        #[serde(serialize_with = "tidy::path")]
        program: std::path::PathBuf,
    },
    /// The screens whose front window is full screen, which covers their bar
    /// (Day.6: a meeting's reminder then shows at the screen's edge): sent as
    /// the shell connects and when they change.
    Fullscreen(#[serde(serialize_with = "tidy::names")] Vec<String>),
    /// The screens shared now (5.11), by their connectors' names: a meeting's
    /// reminder hides its title on them (Day.6). Sent with [`ToShell::Shares`](crate::ToShell::Shares).
    SharedScreens(#[serde(serialize_with = "tidy::names")] Vec<String>),
    /// The session is locked (7.1, [`LockShown`]): the lock's surfaces on
    /// every screen, as told. Sent as it locks, on each change, and as the
    /// shell connects while it is locked.
    Lock(LockShown),
    /// The lock opened and its motion ended: its surfaces go.
    Unlocked,
    /// The curtain before the lock is up (7.2, [`CurtainShown`]): the
    /// pill's surface on the screen told. Sent as it comes, and as the
    /// shell connects while it is up.
    Curtain(CurtainShown),
    /// The curtain was lifted, or ended: the pill's surface goes.
    CurtainEnd,
    /// What logind allows of the session menu's power actions (7.4,
    /// [`LeaveCan`]): sent as the shell connects and when it changed.
    LeaveCan(LeaveCan),
    /// A confirmation of the session's end under way (7.4,
    /// [`LeaveShown`]): the user's popover shows it on its bar, opening if
    /// it was not (`Super+Shift+E`). Sent as it starts and on each change.
    Leave(LeaveShown),
    /// No confirmation any more: the menu's rows again.
    LeaveEnd,
    /// The power button pressed, `[power] power-button = "ask"` (6.10): the
    /// session menu (7.4) opens on `output`'s bar, as a click on the footer's
    /// user opens it.
    SessionMenu { output: String },
    /// What the bar's menu of a window shows (#723), the answer to
    /// [`ToCompositor::WindowMenu`](crate::ToCompositor::WindowMenu) ([`WindowMenu`]).
    WindowMenu(WindowMenu),
    /// The windows kept above the others and those on all desktops, for
    /// the marks after their titles in the bar ([`WindowStates`]).
    WindowStates(WindowStates),
    /// `Super+B` (`[shortcuts] focus-bar`, bar.md §11): the keyboard goes into
    /// `output`'s bar, on its waiting agent, else its stuck one, else its
    /// clock; `None`, the bar holding it lets it go, back to the window that
    /// had it.
    BarKeyboard(Option<String>),
    /// `Super+P` (#736, [`ActionsShown`]): the actions palette opens on
    /// the target's screen; open, it closes.
    Actions(ActionsShown),
    /// The palette's target window went, or the session locks: it closes.
    ActionsEnd,
    /// What the agents acting through `hocus mcp` show (docs/design/mcp.md):
    /// sent as the shell connects and whenever it changes.
    Mcp(crate::McpShown),
}

impl ToShell {
    /// Its line, without the line break ([`wire`]).
    pub fn line(&self) -> String {
        wire::line(self)
    }

    /// The message `line` says. One that reads and cannot be is refused
    /// as one that does not: a window outside its screen, a group twice
    /// in the bar's order.
    pub fn read(line: &str) -> Result<Self, BadLine> {
        let message: Self = wire::read(line)?;
        match &message {
            Self::Desktops(desktops)
                if desktops
                    .windows
                    .iter()
                    .any(|w| w.rect.iter().any(|share| *share > SHARE)) =>
            {
                Err(BadLine("desktops: a window outside its screen".to_owned()))
            }
            Self::BarGroups(BarGroups {
                order: [a, b, c], ..
            }) if a == b || b == c || a == c => {
                Err(BadLine("bar-groups: a group twice".to_owned()))
            }
            _ => Ok(message),
        }
    }
}

/// A program the compositor starts, supervises and starts again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Supervised {
    /// `hocuspanda-shell`.
    Shell,
}

impl Supervised {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Shell => "shell",
        }
    }

    pub fn of(word: &str) -> Option<Self> {
        (word == Self::Shell.word()).then_some(Self::Shell)
    }
}

/// What [`ToShell::Day`](crate::ToShell::Day) asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DayAsk {
    /// A task titled so, at the top of the tasks' add column.
    Add(#[serde(serialize_with = "tidy::name")] String),
    /// The task of that id ticked, or unticked.
    Tick(#[serde(serialize_with = "tidy::name")] String),
    Untick(#[serde(serialize_with = "tidy::name")] String),
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EmojiAt {
    Caret(Rect),
    Window(Rect),
    Screen,
}

/// The panda peeking out (4.5): the agent, why, since when (microseconds
/// of `CLOCK_MONOTONIC`, as [`AgentShown::since`]) and its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peek {
    pub id: u64,
    pub kind: PeekKind,
    pub since: u64,
    #[serde(serialize_with = "tidy::name")]
    pub name: String,
}

/// Windows that moved with a screen ([`ToShell::ScreensMoved`](crate::ToShell::ScreensMoved)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
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

/// A rectangle, logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
