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
//! and which one is active, and the bars' screens' desktops with a sketch
//! of their windows; the shell draws the bars and their popovers, keeps
//! the levels, and says when it draws on the CPU, when a popover closes,
//! and when the user asks for the next layout or another desktop. Neither
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
    /// microseconds of `CLOCK_MONOTONIC` (0: it shows in that form at once).
    Bar {
        output: String,
        form: Form,
        since: u64,
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
}

/// A screen's desktops as its bar's tiles show them. Written `desktops`,
/// the screen's name, the count, the current one (from 0), then for each
/// window, bottom first, its desktop (from 0), its place and size in
/// [`SHARE`]s of the screen, and 1 if it is the focused window of the
/// current desktop (else 0), commas between: `desktops eDP-1 4 0
/// 0,20,30,600,940,1`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Desktops {
    pub output: String,
    pub count: u8,
    pub current: u8,
    pub windows: Vec<DeskWindow>,
}

/// A window as its desktop's tile sketches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeskWindow {
    pub desktop: u8,
    /// Left, top, width, height, in [`SHARE`]s of the screen, inside it.
    pub rect: [u16; 4],
    /// The focused window, on the current desktop: drawn brighter.
    pub focused: bool,
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
            } => write!(f, "bar {output} {} {since}", form.name()),
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
                        " {},{x},{y},{width},{height},{}",
                        w.desktop,
                        u8::from(w.focused)
                    )?;
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
                        let fields: Vec<&str> = w.split(',').collect();
                        let [desktop, x, y, width, height, focused] = fields[..] else {
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
                            focused: flag(Some(focused), "1", "0")?,
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
        }
    }
}

impl FromStr for ToCompositor {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("software") => Ok(Self::Software),
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
                        focused: false,
                    },
                    DeskWindow {
                        desktop: 8,
                        rect: [20, 30, 600, 940],
                        focused: true,
                    },
                ],
            }),
        ];
        for message in to_shell
            .into_iter()
            .chain(keys)
            .chain([ToShell::Outside])
            .chain(screens)
            .chain(layouts)
            .chain(desktops)
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
        ] {
            assert_eq!(message.to_string().parse::<ToCompositor>(), Ok(message));
        }
        assert!("wake".parse::<ToCompositor>().is_err());
        assert!("closing".parse::<ToCompositor>().is_err());
        assert!("desktop eDP-1".parse::<ToCompositor>().is_err());
        assert!("desktop eDP-1 last".parse::<ToCompositor>().is_err());
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
        ] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad:?}");
        }
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
