//! What the shell tells the compositor, and what those messages carry
//! that is theirs alone.

use serde::{Deserialize, Serialize};

use crate::{
    AuthAsk, BadLine, CaptureAsk, Clip, DeskTile, DesktopTo, LeaveAsk, LockAsk, RecordAsk,
    RunAction, ShareAsk, WindowAction, emoji_text, text::tidy, wire,
};

/// An application the launcher starts (5.1), as its desktop entry says: its id
/// (`org.mozilla.firefox`, for the log), whether it runs in a terminal, its
/// working directory (empty: the compositor's), and its command, field codes
/// taken out, one argument each.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub id: String,
    pub terminal: bool,
    pub dir: String,
    pub argv: Vec<String>,
}

impl Launch {
    /// Whether it can be run: something to run, and no field holds a
    /// control character.
    pub fn fits(&self) -> bool {
        let clean = |s: &str| !s.chars().any(char::is_control);
        self.argv.first().is_some_and(|program| !program.is_empty())
            && clean(&self.id)
            && clean(&self.dir)
            && self.argv.iter().all(|a| clean(a))
    }
}

/// How near the next meeting is, for the halo (bar.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MeetingNear {
    /// In 10 min or less (priority 3).
    Ten,
    /// In 2 min or less (priority 1).
    Two,
}

/// What the shell tells the compositor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
pub enum ToCompositor {
    /// The shell draws on the CPU: its bars are opaque, nothing to blur.
    Software,
    /// A line of what the shell shows, for screen readers ([`ToShell::A11y`](crate::ToShell::A11y)),
    /// its control characters made spaces.
    A11yLine(#[serde(serialize_with = "tidy::a11y")] String),
    /// The last of them.
    A11yEnd,
    /// The shell's surfaces of this namespace have something new to show:
    /// the compositor sends them a configure, as they are. iced draws
    /// every surface after any message of the shell's own, and the whole
    /// bar would be drawn again; a configure makes it draw that surface
    /// alone (bar.md, as built, 3.3).
    Wake(String),
    /// The shell's surfaces of this namespace (a popover) are closing: the
    /// compositor fades them out over [`POPOVER_CLOSE`](crate::POPOVER_CLOSE) (at once without
    /// motion), and they are no longer a menu; the shell removes them
    /// after that.
    Closing(String),
    /// The user asks for the next keyboard layout (the bar's row): the
    /// compositor switches the seat's keyboard to it, then says which is
    /// active ([`ToShell::Layout`](crate::ToShell::Layout)).
    NextLayout,
    /// The user asks for a desktop of `output` (a tile clicked, the wheel over
    /// them): the compositor switches as its shortcuts do.
    Desktop { output: String, to: DesktopTo },
    /// The user asks for window `id` (a row of a desktop's list, 3.9b): the
    /// compositor goes to its desktop, sliding, and focuses it.
    Bring(u64),
    /// Where bar `bar`'s desktops' tiles are on its desktops' surface (8.5 B):
    /// a window moved over one goes to that desktop when it is let go there.
    /// Sent when they move or change.
    DeskTiles { bar: String, tiles: Vec<DeskTile> },
    /// The user asks window `id` to close (its ✕ in the bar, #604): the
    /// compositor asks its application, as `Super+Q` does; the application
    /// decides.
    Close(u64),
    /// The bar's "+ N autres" over its windows (#604): the overview opens on
    /// every screen, or closes when open (8.5 A).
    Overview,
    /// The user goes to agent `id` (its row in the agents' group, 4.1): the
    /// compositor brings its window, if it has one, and a finished agent is
    /// seen. From a row (4.3), the streak runs from it to the window.
    GoToAgent { id: u64, row: Option<Row> },
    /// The keyboard's ring moved in a bar (`Super+B`, bar.md §11): where it is,
    /// so that the magnifier's view follows it.
    Ring(Row),
    /// The user plugs an agents' tool in (`claude-code`, `codex`), having read
    /// what it changes: the compositor writes its hooks, off its loop, then
    /// says [`ToShell::AgentTools`](crate::ToShell::AgentTools).
    Plug(String),
    /// The user unplugs it.
    Unplug(String),
    /// "Plug nothing in": every tool there and not plugged in is declined,
    /// herdr's following too.
    Decline,
    /// The user follows herdr again, declined before (Sources' "Suivre").
    Follow(String),
    /// The secretary's briefing opened (`open`) or closed (4.7b): while it is
    /// open no panda peeks out, and one out goes back at once, so that it never
    /// shows through it.
    Briefing { open: bool },
    /// The user clicked the panda that says screens moved
    /// ([`ToShell::ScreensMoved`](crate::ToShell::ScreensMoved)): it goes back.
    PeekClose,
    /// How near the next meeting is (bar.md §6, Day.2), for the halo: in 10 min
    /// or less, in 2 min or less, or neither (none, further, in progress, or
    /// opened by the user).
    Meeting(Option<MeetingNear>),
    /// Whether the player in the bar's music block plays (s12), sent as the
    /// shell connects and when it changes: the aurora on the wallpaper
    /// ripples while it does, and calms when it stops (#801). No level: the
    /// shell reads no sound.
    Music { playing: bool },
    /// The clipboard takes `text` (the command the Sources popover shows),
    /// which the compositor offers: the shell's surfaces take no serial to set
    /// it themselves.
    Copy(#[serde(serialize_with = "tidy::name")] String),
    /// A notification's bubble was clicked (5.3): the compositor brings the
    /// window of its application, `app` (the notification's desktop entry, else
    /// its application's name; `-` for none), else of the process that sent it,
    /// `pid`, or one of its parents (0: not known).
    Raise { pid: u32, app: String },
    /// The user starts an application from the launcher (5.1): the
    /// compositor runs it off its loop, as its own child, its first window
    /// taking the focus on the current desktop.
    Launch(Launch),
    /// The user logs out (the footer's user, 5.5): the session ends, as its
    /// shortcut ends it.
    LogOut,
    /// An item of the clipboard's history is chosen (5.6): the clipboard takes
    /// it, which the compositor offers; with `paste`, the window that had the
    /// keyboard gets it back once the history lets it go, and `Ctrl+V`
    /// (`Ctrl+Shift+V` in a terminal).
    Clip { paste: bool, clip: Clip },
    /// How many items the clipboard's history holds, and how many of them are
    /// pinned (6.17): sent as the shell connects and when they change, for the
    /// Settings' page.
    ClipboardCount { items: u32, pinned: u32 },
    /// An emoji chosen in the picker or the launcher (5.7): it goes into the
    /// window that had the keyboard, through its text input when it has one,
    /// else pasted, the clipboard given back after. With `keep` the picker
    /// stays open, and is told when it is in ([`ToShell::EmojiInserted`](crate::ToShell::EmojiInserted)).
    Emoji { keep: bool, text: String },
    /// The user acts on the capture's toolbar (5.8, [`CaptureAsk`]).
    Capture(CaptureAsk),
    /// The chooser's thumbnails' places, what it chose, its cancel, a
    /// share stopped from the bar (5.11, [`ShareAsk`]).
    Share(ShareAsk),
    /// The bar's item pauses, goes on with or stops the recording; a
    /// recording's thumbnail deletes or drags it (5.10, [`RecordAsk`]).
    Record(RecordAsk),
    /// The user clicked a tray icon (5.14b): the shell is about to call the
    /// item's `Activate` or `SecondaryActivate`. `pid` is the process of the
    /// item's D-Bus connection (0: not known). The compositor answers at once
    /// with an activation token made for that click ([`ToShell::TrayToken`](crate::ToShell::TrayToken)),
    /// and lets that process's application focus one window shortly after, with
    /// a token of its own or none.
    TrayClick { ask: u64, pid: u32 },
    /// The lock's arrow or one of its buttons clicked, or the session
    /// menu's "Verrouiller" (7.1, 7.4, [`LockAsk`]).
    Lock(LockAsk),
    /// The shell is about to ask the desktop portal for a dialog over its
    /// surface of `namespace` (the capture's editor's "Enregistrer sous…",
    /// #595). The compositor exports that surface through xdg-foreign and
    /// answers at once with its handle ([`ToShell::Exported`](crate::ToShell::Exported)); a window that
    /// the portal's backend maps as that handle's child comes in front, with
    /// the keyboard.
    Export { ask: u64, namespace: String },
    /// The system's authentication dialog opens or fades out (7.5,
    /// [`AuthAsk`]).
    Auth(AuthAsk),
    /// The session menu (7.4, [`LeaveAsk`]): it opened, a row chosen, the
    /// confirmation acted on or cancelled.
    Leave(LeaveAsk),
    /// The bar's menu of window `id` (#723) runs `action` on it, as its radial
    /// menu does.
    WindowAction { id: u64, action: WindowAction },
    /// The bar's menu sends window `id` to desktop `desktop` (from 0) of screen
    /// `output`.
    SendTo {
        id: u64,
        output: String,
        desktop: u8,
    },
    /// The bar's menu of window `id` is about to open: the compositor answers
    /// at once with what it shows ([`ToShell::WindowMenu`](crate::ToShell::WindowMenu)).
    WindowMenu(u64),
    /// The palette (#736) runs an action ([`RunAction`]).
    Action(RunAction),
    /// The toast's or the journal's "Annuler": request `0` of an MCP agent
    /// undone (docs/design/mcp.md).
    McpUndo(u64),
    /// "Tout arrêter" (`true`), or "Reprendre" (`false`): every MCP agent
    /// stopped, or let go on.
    McpStop(bool),
    /// The user answered card `card` (docs/design/mcp.md, scene 3).
    McpReply { card: u64, reply: crate::McpReply },
    /// "Rendre la main": agent `0`'s actions go on.
    McpHandBack(String),
    /// The form's "Créer le projet" (docs/design/projects.md §2.1): desktop
    /// `desktop` (from 0) of `output` becomes project `name`, of folder
    /// `folder`. Answered by [`ToShell::ProjectMade`](crate::ToShell::ProjectMade) or
    /// [`ToShell::ProjectRefused`](crate::ToShell::ProjectRefused).
    ProjectMake {
        output: String,
        desktop: u8,
        #[serde(serialize_with = "tidy::name")]
        name: String,
        #[serde(serialize_with = "tidy::name")]
        folder: String,
    },
    /// "Fermer le projet…" on project `name` (its list in the bar, its
    /// card): its confirmation, answered by
    /// [`ToShell::ProjectConfirm`](crate::ToShell::ProjectConfirm).
    ProjectCloseAsk(#[serde(serialize_with = "tidy::name")] String),
    /// The confirmation's "Fermer le projet": project `name` closes (§5).
    ProjectClose(#[serde(serialize_with = "tidy::name")] String),
    /// Closed project `name` reopens (§7) on `output`, the launcher's.
    ProjectOpen {
        output: String,
        #[serde(serialize_with = "tidy::name")]
        name: String,
    },
    /// Open project `name`: its desktop shows.
    ProjectGo(#[serde(serialize_with = "tidy::name")] String),
    /// The launcher's "Créer le projet « … »" (§2.2): folder `folder` is a
    /// project, on the desktop of `output` §7 chooses.
    ProjectNew {
        output: String,
        #[serde(serialize_with = "tidy::name")]
        folder: String,
    },
}

impl ToCompositor {
    /// Its line, without the line break ([`wire`]).
    pub fn line(&self) -> String {
        wire::line(self)
    }

    /// The message `line` says. One that reads and cannot be is refused
    /// as one that does not: a launch with nothing to run, an emoji that
    /// is none.
    pub fn read(line: &str) -> Result<Self, BadLine> {
        let message: Self = wire::read(line)?;
        match &message {
            Self::Launch(launch) if !launch.fits() => {
                Err(BadLine("launch: nothing to run".to_owned()))
            }
            Self::Emoji { text, .. } if !emoji_text(text) => {
                Err(BadLine("emoji: not an emoji".to_owned()))
            }
            _ => Ok(message),
        }
    }
}

/// Where something the user clicked is on a bar: its screen's name and
/// its rectangle in the bar's frame, logical pixels, `x` from the bar's
/// right edge (negative, inside it) and `y` from its top.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Row {
    pub output: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
