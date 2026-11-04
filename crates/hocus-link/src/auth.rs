//! The system's authentication dialog's lines (7.5, docs/design/polkit.md):
//! the shell is the session's polkit agent and draws the dialog; the
//! compositor finds where it goes (the asking process's window), dims
//! every screen under it, moves it in and out, and keeps the pointer and
//! the keyboard from everything else while it shows.
//!
//! - From the shell, `auth open ID PID…`: a dialog is about to show for the
//!   request `ID`, asked by a process, followed by its parents, nearest
//!   first (none: not known); the shell reads them, so that the compositor
//!   reads nothing from `/proc` on its loop. The compositor answers with
//!   `auth-at`, and from then on takes a surface of [`AUTH_NAMESPACE`] for
//!   that dialog.
//! - `auth-at ID OUTPUT SCALE HEIGHT`, a tab and the window's application
//!   (`-`: no window found), a tab and its title: where the dialog goes.
//! - From the shell, `auth closing ID`: it fades out; the compositor lets
//!   the pointer and the keys go at once.
//! - `caps on` or `caps off`: Caps Lock's state, sent as it changes and as
//!   the shell connects, for the dialog's sign.

use std::{fmt, str::FromStr, time::Duration};

use crate::{BadLine, title_text, word};

/// Set (to `1`) for the shell when the compositor is the session, on a
/// virtual terminal: the shell is then the session's polkit agent. A
/// shell nested in another session leaves that session's agent alone.
pub const AUTH_AGENT_ENV: &str = "HOCUS_AUTH_AGENT";

/// The dialog's surface: the dialog alone, centred across its screen, its
/// top [`AUTH_TOP`] over the screen's middle, in the overlay layer, holding
/// the keyboard.
pub const AUTH_NAMESPACE: &str = "hocus-auth";
/// The dialog's width and corners, logical pixels.
pub const AUTH_WIDTH: f64 = 380.0;
pub const AUTH_RADIUS: f64 = 16.0;
/// Its top is this far over its screen's middle: it grows downward.
pub const AUTH_TOP: f64 = 220.0;
/// It fades and grows from [`AUTH_FROM`] of its size over [`AUTH_IN`]
/// (out-cubic), and fades out over [`AUTH_OUT`] (in-cubic); the dim under
/// it fades in over [`AUTH_DIM_IN`] (`ease-out`).
pub const AUTH_IN: Duration = Duration::from_millis(250);
pub const AUTH_FROM: f64 = 0.96;
pub const AUTH_OUT: Duration = Duration::from_millis(150);
pub const AUTH_DIM_IN: Duration = Duration::from_millis(160);
/// What dims every screen: `rgba(5, 7, 12, 0.58)`.
pub const AUTH_DIM: [u8; 3] = [5, 7, 12];
pub const AUTH_DIM_ALPHA: f32 = 0.58;
/// A request asked while one shows opens this long after it closed.
pub const AUTH_NEXT: Duration = Duration::from_millis(300);

/// What the shell says of its dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthAsk {
    /// A dialog for request `id`, asked by the first of `pids`, the others
    /// its parents, nearest first (empty: not known).
    Open { id: u64, pids: Vec<u32> },
    /// It fades out.
    Closing { id: u64 },
}

impl AuthAsk {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open { id, pids } => {
                write!(f, "auth open {id}")?;
                for pid in pids {
                    write!(f, " {pid}")?;
                }
                Ok(())
            }
            Self::Closing { id } => write!(f, "auth closing {id}"),
        }
    }

    pub(crate) fn read<'a>(words: &mut impl Iterator<Item = &'a str>) -> Result<Self, BadLine> {
        let what = word(words, "what of the dialog")?;
        let id = number(word(words, "request")?, "request")?;
        match what {
            "open" => Ok(Self::Open {
                id,
                pids: words
                    .map(|w| number(w, "process"))
                    .collect::<Result<_, _>>()?,
            }),
            "closing" => Ok(Self::Closing { id }),
            other => Err(BadLine(format!("not a dialog's ask: {other:?}"))),
        }
    }
}

/// Where request `id`'s dialog goes: on `output` (its scale in 120ths, its
/// height in logical pixels), and the window its process was matched to,
/// by its application's id and title (an empty `app`: none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthAt {
    pub id: u64,
    pub output: String,
    pub scale: u32,
    pub height: i32,
    pub app: String,
    pub title: String,
}

impl AuthAt {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let app: String = self
            .app
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .collect();
        write!(
            f,
            "auth-at {} {} {} {}\t{}\t{}",
            self.id,
            self.output,
            self.scale,
            self.height,
            if app.is_empty() { "-" } else { &app },
            title_text(&self.title)
        )
    }

    pub(crate) fn read(rest: &str) -> Result<Self, BadLine> {
        let mut fields = rest.split('\t');
        let mut words = word(&mut fields, "screen")?.split_whitespace();
        let id = number(word(&mut words, "request")?, "request")?;
        let output = word(&mut words, "output")?.to_owned();
        let scale = number(word(&mut words, "scale")?, "scale")?;
        let height = number(word(&mut words, "height")?, "height")?;
        let app = match word(&mut fields, "application")? {
            "-" => String::new(),
            app => app.to_owned(),
        };
        let title = fields.next().unwrap_or_default().to_owned();
        Ok(Self {
            id,
            output,
            scale,
            height,
            app,
            title,
        })
    }
}

fn number<T: FromStr>(w: &str, what: &str) -> Result<T, BadLine>
where
    T::Err: fmt::Display,
{
    w.parse::<T>().map_err(|e| BadLine(format!("{what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    #[test]
    fn the_dialogs_lines_read_back() {
        for line in [
            ToShell::AuthAt(AuthAt {
                id: 3,
                output: "eDP-1".into(),
                scale: 150,
                height: 1067,
                app: "org.gnome.Software".into(),
                title: "Mises à jour".into(),
            }),
            ToShell::AuthAt(AuthAt {
                id: 4,
                output: "HEADLESS-1".into(),
                scale: 120,
                height: 1080,
                app: String::new(),
                title: String::new(),
            }),
            ToShell::CapsLock(true),
            ToShell::CapsLock(false),
        ] {
            let text = line.to_string();
            assert_eq!(text.parse::<ToShell>(), Ok(line), "{text}");
        }
        for line in [
            ToCompositor::Auth(AuthAsk::Open {
                id: 3,
                pids: vec![4242, 4100, 1],
            }),
            ToCompositor::Auth(AuthAsk::Open {
                id: 4,
                pids: Vec::new(),
            }),
            ToCompositor::Auth(AuthAsk::Closing { id: 3 }),
        ] {
            let text = line.to_string();
            assert_eq!(text.parse::<ToCompositor>(), Ok(line), "{text}");
        }
        assert_eq!(
            ToShell::AuthAt(AuthAt {
                id: 1,
                output: "DP-2".into(),
                scale: 120,
                height: 1440,
                app: "my app".into(),
                title: "a\ttitle\n".into(),
            })
            .to_string(),
            "auth-at 1 DP-2 120 1440\tmyapp\ta title"
        );
        assert!("auth what 1".parse::<ToCompositor>().is_err());
    }
}
