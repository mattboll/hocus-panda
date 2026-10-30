//! The private line between the compositor and its shell (docs/design/bar.md
//! §2, §3): the bar's measures both draw by, and the few messages they
//! exchange.
//!
//! The compositor starts the shell with [`SOCKET_ENV`] naming a Unix socket
//! in the session's runtime directory, which only it listens on. Each
//! message is one line of text, words separated by spaces (the screens'
//! names by tabs). The compositor decides where bars are and in which
//! form, reserves their space and takes the clicks on their edge, passes on
//! the clock's settings, the level keys (volume, brightness) pressed, the
//! presses outside the shell's surfaces while a popover shows, the
//! names of the screens besides the built-in one, the keyboard's layouts
//! and which one is active, the bars' screens' desktops with a sketch of
//! their windows, their applications and their titles, and the agents'
//! tools and the agents; the shell draws the bars and their popovers,
//! keeps the levels, and says when it draws on the CPU, when a popover
//! closes, and when the user asks for the next layout, another desktop or
//! a window, plugs a tool in or out, declines them, or copies a text.
//! Neither
//! side waits for the other: a message is a fact or a request, never a
//! question.
//!
//! The form switch is one motion, which both sides compute from the same
//! start on the same clock (`CLOCK_MONOTONIC`, in microseconds): the
//! compositor moves the windows and the glass behind the bar, the shell
//! draws the bar, each in its own frames, with no message per frame.

use std::{fmt, str::FromStr, time::Duration};

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
/// The namespace of the agents' group's surface, under the clock (bar.md
/// §4 group 3, 4.0): the agents' changes draw only that region.
pub const AGENTS_NAMESPACE: &str = "hocus-bar-agents";
/// The namespace of a popover's layer surface, beside the bar (bar.md §9):
/// one at a time. The compositor blurs behind it as behind the bar, draws
/// its shadow, and moves it in and out ([`POPOVER_OPEN`], [`POPOVER_CLOSE`]).
pub const POPOVER_NAMESPACE: &str = "hocus-bar-popover";

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

/// What the compositor tells the shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToShell {
    /// How bars look: on glass the compositor blurs behind (else opaque),
    /// and whether they move (else reduced motion).
    Look { glass: bool, motion: bool },
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
/// its name, last (it may hold spaces, never a tab).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentShown {
    pub id: u64,
    pub state: String,
    pub since: u64,
    pub source: String,
    pub window: u64,
    pub name: String,
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

/// A screen's windows' titles, by their ids, in [`Desktops::windows`]'
/// order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Titles {
    pub output: String,
    pub titles: Vec<(u64, String)>,
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

/// What the shell tells the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToCompositor {
    /// The shell draws on the CPU: its bars are opaque, nothing to blur.
    Software,
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
    /// The user goes to agent `id` (its row in the agents' group, 4.1):
    /// the compositor brings its window, if it has one, and a finished
    /// agent is seen. From a row (4.3), the streak runs from it to the
    /// window. Written `go-to-agent ID`, or `go-to-agent ID OUTPUT X Y W
    /// H` with the row.
    GoToAgent { id: u64, row: Option<Row> },
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
    /// The clipboard takes `text` (the command the Sources popover shows),
    /// which the compositor offers: the shell's surfaces take no serial to
    /// set it themselves. Written `copy TEXT`, the text on one line.
    Copy(String),
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
            Self::Look { glass, motion } => write!(
                f,
                "look {} {}",
                if *glass { "glass" } else { "opaque" },
                if *motion { "motion" } else { "still" }
            ),
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
                        "\t{} {} {} {} {} {}",
                        a.id,
                        a.state,
                        a.since,
                        a.source,
                        a.window,
                        screen_name(&a.name)
                    )?;
                }
                Ok(())
            }
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
            return fields
                .split('\t')
                .skip(1)
                .map(|field| {
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
                    })
                })
                .collect::<Result<_, BadLine>>()
                .map(Self::Agents);
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
        let mut words = line.split_whitespace();
        match words.next() {
            Some("look") => Ok(Self::Look {
                glass: flag(words.next(), "glass", "opaque")?,
                motion: flag(words.next(), "motion", "still")?,
            }),
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
            Self::Wake(namespace) => write!(f, "wake {namespace}"),
            Self::Closing(namespace) => write!(f, "closing {namespace}"),
            Self::NextLayout => f.write_str("next-layout"),
            Self::Desktop { output, to } => match to {
                DesktopTo::At(index) => write!(f, "desktop {output} {index}"),
                DesktopTo::Next => write!(f, "desktop {output} next"),
                DesktopTo::Previous => write!(f, "desktop {output} previous"),
            },
            Self::Bring(id) => write!(f, "bring {id}"),
            Self::GoToAgent { id, row: None } => write!(f, "go-to-agent {id}"),
            Self::GoToAgent { id, row: Some(r) } => write!(
                f,
                "go-to-agent {id} {} {} {} {} {}",
                r.output, r.x, r.y, r.width, r.height
            ),
            Self::Plug(tool) => write!(f, "plug {tool}"),
            Self::Unplug(tool) => write!(f, "unplug {tool}"),
            Self::Decline => f.write_str("decline"),
            Self::Follow(tool) => write!(f, "follow {tool}"),
            Self::Copy(text) => write!(f, "copy {}", screen_name(text)),
        }
    }
}

impl FromStr for ToCompositor {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        if let Some(text) = line.strip_prefix("copy ") {
            return Ok(Self::Copy(text.to_owned()));
        }
        let mut words = line.split_whitespace();
        match words.next() {
            Some("software") => Ok(Self::Software),
            Some("plug") => Ok(Self::Plug(word(&mut words, "tool")?.to_owned())),
            Some("unplug") => Ok(Self::Unplug(word(&mut words, "tool")?.to_owned())),
            Some("decline") => Ok(Self::Decline),
            Some("follow") => Ok(Self::Follow(word(&mut words, "tool")?.to_owned())),
            Some("wake") => Ok(Self::Wake(word(&mut words, "namespace")?.to_owned())),
            Some("closing") => Ok(Self::Closing(word(&mut words, "namespace")?.to_owned())),
            Some("next-layout") => Ok(Self::NextLayout),
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
            Some("bring") => Ok(Self::Bring(
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

    #[test]
    fn lines_read_back_as_written() {
        let to_shell = [
            ToShell::Look {
                glass: true,
                motion: false,
            },
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
        ];
        for message in to_shell
            .into_iter()
            .chain(keys)
            .chain([ToShell::Outside])
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
            ToCompositor::NextLayout,
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
            ToCompositor::GoToAgent { id: 7, row: None },
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
            ToCompositor::Follow("herdr".into()),
            ToCompositor::Copy("hocus agent my-agent waiting".into()),
        ] {
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
        assert!("wake".parse::<ToCompositor>().is_err());
        assert!("closing".parse::<ToCompositor>().is_err());
        assert!("desktop eDP-1".parse::<ToCompositor>().is_err());
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
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad:?}");
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
            }]),
        ]
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
}
