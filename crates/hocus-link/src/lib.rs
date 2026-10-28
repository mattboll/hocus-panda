//! The private line between the compositor and its shell (docs/design/bar.md
//! §2, §3): the bar's measures both draw by, and the few messages they
//! exchange.
//!
//! The compositor starts the shell with [`SOCKET_ENV`] naming a Unix socket
//! in the session's runtime directory, which only it listens on. Each
//! message is one line of text, words separated by spaces. The compositor
//! decides where bars are and in which form, reserves their space and
//! takes the clicks on their edge; the shell draws them, and says when it
//! draws on the CPU. Neither side waits
//! for the other: a message is a fact, never a question.
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
}

/// What the shell tells the compositor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToCompositor {
    /// The shell draws on the CPU: its bars are opaque, nothing to blur.
    Software,
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
        }
    }
}

impl FromStr for ToShell {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
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
            _ => Err(BadLine(format!("unknown line {line:?}"))),
        }
    }
}

impl fmt::Display for ToCompositor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Software => f.write_str("software"),
        }
    }
}

impl FromStr for ToCompositor {
    type Err = BadLine;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("software") => Ok(Self::Software),
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
        ];
        for message in to_shell {
            assert_eq!(message.to_string().parse::<ToShell>(), Ok(message));
        }
        assert_eq!(
            ToCompositor::Software.to_string().parse::<ToCompositor>(),
            Ok(ToCompositor::Software)
        );
        for bad in ["", "bar eDP-1", "bar eDP-1 wide 0", "look glass", "hello"] {
            assert!(bad.parse::<ToShell>().is_err(), "{bad:?}");
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
