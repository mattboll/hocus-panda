//! Screen sharing's lines (5.11, docs/design/sharing.md): the compositor
//! serves the desktop portal's screen cast and streams to PipeWire; the
//! shell shows the chooser (what to share) and the bar's sign while
//! something is shared, and holds the notifications meanwhile.
//!
//! - `share-choose`: a request to share, with what can be shared: the
//!   chooser opens on that screen (or shows the sources anew).
//! - `share-choose-end`: it goes (chosen, cancelled, or the request went).
//! - `shares`: what is shared now, each with its application; none, the
//!   bar's sign goes and the notifications held are summed up.
//! - From the shell: where the chooser's thumbnails are (the compositor
//!   draws them over its surface), what was chosen, cancelled, and a
//!   share stopped from the bar.

use std::{fmt, str::FromStr, time::Duration};

use crate::{BadLine, LAUNCHER_ZOOM, Rect, flag, screen_name, title_text, word};

/// The chooser's surface: the panel alone, centred on its screen, in the
/// overlay layer, holding the keyboard. The compositor darkens the screen
/// under it, draws its shadow and its thumbnails, and pops it in from its
/// first frame over [`SHARE_CHOOSER_IN`].
pub const SHARE_CHOOSER_NAMESPACE: &str = "hocus-share-chooser";
/// The bar's sign that something is in use (the screen, so far: 5.12 adds
/// the microphone and the camera), under the clock.
pub const IN_USE_NAMESPACE: &str = "hocus-bar-in-use";
/// The mockup's `pop`: from 0.97 of its size and transparent, over this,
/// `cubic-bezier(.2, .8, .2, 1)`.
pub const SHARE_CHOOSER_IN: Duration = Duration::from_millis(180);
pub const SHARE_CHOOSER_FROM: f64 = 0.97;
/// The chooser's corners and its thumbnails', the mockup's × the
/// launcher's zoom (the chooser is drawn as large).
pub const SHARE_CHOOSER_RADIUS: f64 = 16.0 * LAUNCHER_ZOOM;
pub const SHARE_THUMB_RADIUS: f64 = 8.0 * LAUNCHER_ZOOM;
/// The thumbnails are drawn again at most this often while the chooser
/// shows.
pub const SHARE_THUMBS_EVERY: Duration = Duration::from_millis(500);
/// The portal's source types, as its `types` bitmask says them.
pub const SHARE_MONITOR: u32 = 1;
pub const SHARE_WINDOW: u32 = 2;

/// What can be shared: a screen, by its connector's name, or a window, by
/// the compositor's id. Written `screen:NAME` and `window:ID`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ShareSource {
    Screen(String),
    Window(u64),
}

impl fmt::Display for ShareSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Screen(name) => write!(f, "screen:{name}"),
            Self::Window(id) => write!(f, "window:{id}"),
        }
    }
}

impl FromStr for ShareSource {
    type Err = BadLine;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(name) = s.strip_prefix("screen:").filter(|n| !n.is_empty()) {
            return Ok(Self::Screen(name.to_owned()));
        }
        if let Some(id) = s.strip_prefix("window:") {
            return id
                .parse()
                .map(Self::Window)
                .map_err(|e| BadLine(format!("window: {e}")));
        }
        Err(BadLine(format!("not a source: {s:?}")))
    }
}

/// A screen the chooser offers: its connector's name, its size in pixels,
/// whether it is the built-in panel, and the name people know it by (its
/// EDID's model; empty: none).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareScreen {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub builtin: bool,
    pub model: String,
}

/// A window the chooser offers: its id, its desktop (from 1) on its
/// screen, whether that is the desktop its screen shows, whether it is
/// minimised, its application's id (empty: none) and its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareWindow {
    pub id: u64,
    pub desktop: u8,
    pub here: bool,
    pub minimized: bool,
    pub app: String,
    pub title: String,
}

/// A request to share, as the chooser shows it: on `output` (its scale in
/// 120ths, its size `width` × `height` logical pixels), the portal's `types` asked ([`SHARE_MONITOR`],
/// [`SHARE_WINDOW`]), the pointer shown by default or not, the asking
/// application's id (empty: not known) and what it says of itself (its
/// window's title; empty: nothing), then the screens and the windows.
///
/// Written `share-choose REQUEST OUTPUT SCALE WIDTH HEIGHT TYPES
/// pointer|nopointer`,
/// then a tab and `app APP` (`-` for none), a tab and `detail TEXT`, and
/// for each screen a tab and `screen NAME WIDTH HEIGHT builtin|external
/// MODEL`, for each window a tab and `window ID DESKTOP here|there
/// shown|minimized APP TITLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareChoose {
    pub request: u64,
    pub output: String,
    pub scale: u32,
    pub width: i32,
    pub height: i32,
    pub types: u32,
    pub pointer: bool,
    pub app: String,
    pub detail: String,
    pub screens: Vec<ShareScreen>,
    pub windows: Vec<ShareWindow>,
}

fn dash(word: &str) -> String {
    let word: String = word
        .chars()
        .map(|c| {
            if c.is_whitespace() || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if word.is_empty() {
        "-".to_owned()
    } else {
        word
    }
}

fn undash(word: &str) -> String {
    if word == "-" {
        String::new()
    } else {
        word.to_owned()
    }
}

fn number<T: FromStr>(w: &str, what: &str) -> Result<T, BadLine>
where
    T::Err: fmt::Display,
{
    w.parse::<T>().map_err(|e| BadLine(format!("{what}: {e}")))
}

impl ShareChoose {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "share-choose {} {} {} {} {} {} {}\tapp {}\tdetail {}",
            self.request,
            self.output,
            self.scale,
            self.width,
            self.height,
            self.types,
            if self.pointer { "pointer" } else { "nopointer" },
            dash(&self.app),
            title_text(&self.detail),
        )?;
        for s in &self.screens {
            write!(
                f,
                "\tscreen {} {} {} {} {}",
                s.name,
                s.width,
                s.height,
                if s.builtin { "builtin" } else { "external" },
                screen_name(&s.model)
            )?;
        }
        for w in &self.windows {
            write!(
                f,
                "\twindow {} {} {} {} {} {}",
                w.id,
                w.desktop,
                if w.here { "here" } else { "there" },
                if w.minimized { "minimized" } else { "shown" },
                dash(&w.app),
                title_text(&w.title)
            )?;
        }
        Ok(())
    }

    pub(crate) fn read(fields: &str) -> Result<Self, BadLine> {
        let mut fields = fields.split('\t');
        let mut words = word(&mut fields, "request")?.split(' ');
        let request = number(word(&mut words, "request")?, "request")?;
        let output = word(&mut words, "output")?.to_owned();
        let scale = number(word(&mut words, "scale")?, "scale")?;
        let width = number(word(&mut words, "width")?, "width")?;
        let height = number(word(&mut words, "height")?, "height")?;
        let types = number(word(&mut words, "types")?, "types")?;
        let pointer = flag(words.next(), "pointer", "nopointer")?;
        let mut choose = Self {
            request,
            output,
            scale,
            width,
            height,
            types,
            pointer,
            app: String::new(),
            detail: String::new(),
            screens: Vec::new(),
            windows: Vec::new(),
        };
        for field in fields {
            let (kind, rest) = field.split_once(' ').unwrap_or((field, ""));
            match kind {
                "app" => choose.app = undash(rest),
                "detail" => choose.detail = rest.to_owned(),
                "screen" => {
                    let mut words = rest.splitn(5, ' ');
                    choose.screens.push(ShareScreen {
                        name: word(&mut words, "screen")?.to_owned(),
                        width: number(word(&mut words, "width")?, "width")?,
                        height: number(word(&mut words, "height")?, "height")?,
                        builtin: flag(words.next(), "builtin", "external")?,
                        model: words.next().unwrap_or_default().to_owned(),
                    });
                }
                "window" => {
                    let mut words = rest.splitn(6, ' ');
                    choose.windows.push(ShareWindow {
                        id: number(word(&mut words, "window")?, "window")?,
                        desktop: number(word(&mut words, "desktop")?, "desktop")?,
                        here: flag(words.next(), "here", "there")?,
                        minimized: flag(words.next(), "minimized", "shown")?,
                        app: undash(word(&mut words, "application")?),
                        title: words.next().unwrap_or_default().to_owned(),
                    });
                }
                other => return Err(BadLine(format!("not a chooser's field: {other:?}"))),
            }
        }
        Ok(choose)
    }
}

/// What a share shows of itself in the bar: a screen (by the name people
/// know it by, else its connector's) or a window (its title).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedThing {
    Screen(String),
    Window(String),
}

/// A share under way, as the bar shows it: its id, the application that
/// sees it (its id; empty: not known), and what it sees. Written, after
/// `shares`, a tab and `ID APP screen NAME` or `ID APP window TITLE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareShown {
    pub id: u64,
    pub app: String,
    pub thing: SharedThing,
}

impl ShareShown {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (kind, name) = match &self.thing {
            SharedThing::Screen(name) => ("screen", screen_name(name)),
            SharedThing::Window(title) => ("window", title_text(title)),
        };
        write!(f, "\t{} {} {kind} {name}", self.id, dash(&self.app))
    }

    pub(crate) fn read(field: &str) -> Result<Self, BadLine> {
        let mut words = field.splitn(4, ' ');
        let id = number(word(&mut words, "share")?, "share")?;
        let app = undash(word(&mut words, "application")?);
        let kind = word(&mut words, "what is shared")?;
        let name = words.next().unwrap_or_default().to_owned();
        let thing = match kind {
            "screen" => SharedThing::Screen(name),
            "window" => SharedThing::Window(name),
            other => return Err(BadLine(format!("not a shared thing: {other:?}"))),
        };
        Ok(Self { id, app, thing })
    }
}

/// What the shell says of sharing: where the chooser's thumbnails are on
/// its surface (logical pixels), what was chosen and whether the pointer
/// shows, the chooser cancelled, a share stopped from the bar.
///
/// Written `share thumbs REQUEST` then, for each, a tab and `SOURCE X Y W
/// H`; `share chosen REQUEST SOURCE pointer|nopointer`; `share cancel
/// REQUEST`; `share stop ID`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShareAsk {
    Thumbs {
        request: u64,
        rects: Vec<(ShareSource, Rect)>,
    },
    Chosen {
        request: u64,
        source: ShareSource,
        pointer: bool,
    },
    Cancel {
        request: u64,
    },
    Stop {
        id: u64,
    },
}

impl ShareAsk {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Thumbs { request, rects } => {
                write!(f, "share thumbs {request}")?;
                for (source, r) in rects {
                    write!(f, "\t{source} {} {} {} {}", r.x, r.y, r.width, r.height)?;
                }
                Ok(())
            }
            Self::Chosen {
                request,
                source,
                pointer,
            } => write!(
                f,
                "share chosen {request} {source} {}",
                if *pointer { "pointer" } else { "nopointer" }
            ),
            Self::Cancel { request } => write!(f, "share cancel {request}"),
            Self::Stop { id } => write!(f, "share stop {id}"),
        }
    }

    pub(crate) fn read(rest: &str) -> Result<Self, BadLine> {
        if let Some(fields) = rest.strip_prefix("thumbs ") {
            let mut fields = fields.split('\t');
            let request = number(word(&mut fields, "request")?.trim(), "request")?;
            let rects = fields
                .map(|field| {
                    let mut words = field.split(' ');
                    let source = word(&mut words, "source")?.parse()?;
                    let mut n = |what: &str| number::<i32>(word(&mut words, what)?, what);
                    Ok((
                        source,
                        Rect {
                            x: n("x")?,
                            y: n("y")?,
                            width: n("width")?,
                            height: n("height")?,
                        },
                    ))
                })
                .collect::<Result<_, BadLine>>()?;
            return Ok(Self::Thumbs { request, rects });
        }
        let mut words = rest.split_whitespace();
        match word(&mut words, "what of sharing")? {
            "chosen" => Ok(Self::Chosen {
                request: number(word(&mut words, "request")?, "request")?,
                source: word(&mut words, "source")?.parse()?,
                pointer: flag(words.next(), "pointer", "nopointer")?,
            }),
            "cancel" => Ok(Self::Cancel {
                request: number(word(&mut words, "request")?, "request")?,
            }),
            "stop" => Ok(Self::Stop {
                id: number(word(&mut words, "share")?, "share")?,
            }),
            other => Err(BadLine(format!("not a share's ask: {other:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    fn round(line: ToShell) {
        let text = line.to_string();
        assert_eq!(text.parse::<ToShell>(), Ok(line), "{text}");
    }

    #[test]
    fn the_choosers_line_reads_back() {
        round(ToShell::ShareChoose(ShareChoose {
            request: 7,
            output: "eDP-1".into(),
            scale: 120,
            width: 2560,
            height: 1600,
            types: SHARE_MONITOR | SHARE_WINDOW,
            pointer: true,
            app: "org.mozilla.firefox".into(),
            detail: "Meet — Point produit".into(),
            screens: vec![
                ShareScreen {
                    name: "eDP-1".into(),
                    width: 2560,
                    height: 1600,
                    builtin: true,
                    model: String::new(),
                },
                ShareScreen {
                    name: "DP-2".into(),
                    width: 3440,
                    height: 1440,
                    builtin: false,
                    model: "DELL U3421WE".into(),
                },
            ],
            windows: vec![ShareWindow {
                id: 3,
                desktop: 2,
                here: false,
                minimized: true,
                app: String::new(),
                title: "kitty — cargo nextest".into(),
            }],
        }));
        round(ToShell::ShareChooseEnd(7));
        round(ToShell::Shares(Vec::new()));
        round(ToShell::Shares(vec![
            ShareShown {
                id: 1,
                app: "firefox".into(),
                thing: SharedThing::Window("hocus-panda — Visual Studio Code".into()),
            },
            ShareShown {
                id: 2,
                app: String::new(),
                thing: SharedThing::Screen("Écran du portable".into()),
            },
        ]));
    }

    #[test]
    fn a_title_cannot_break_the_line() {
        let line = ToShell::Shares(vec![ShareShown {
            id: 1,
            app: "a b".into(),
            thing: SharedThing::Window("one\ttwo\nthree".into()),
        }])
        .to_string();
        assert!(!line.contains('\n'));
        assert_eq!(line.matches('\t').count(), 1, "{line}");
    }

    #[test]
    fn what_the_shell_says_reads_back() {
        for ask in [
            ShareAsk::Thumbs {
                request: 3,
                rects: vec![
                    (
                        ShareSource::Screen("HEADLESS-1".into()),
                        Rect {
                            x: 31,
                            y: 140,
                            width: 236,
                            height: 129,
                        },
                    ),
                    (
                        ShareSource::Window(12),
                        Rect {
                            x: 290,
                            y: 140,
                            width: 236,
                            height: 129,
                        },
                    ),
                ],
            },
            ShareAsk::Thumbs {
                request: 3,
                rects: Vec::new(),
            },
            ShareAsk::Chosen {
                request: 3,
                source: ShareSource::Window(12),
                pointer: false,
            },
            ShareAsk::Cancel { request: 3 },
            ShareAsk::Stop { id: 9 },
        ] {
            let line = ToCompositor::Share(ask.clone()).to_string();
            assert_eq!(
                line.parse::<ToCompositor>(),
                Ok(ToCompositor::Share(ask)),
                "{line}"
            );
        }
        assert!(
            "share chosen 3 nothing pointer"
                .parse::<ToCompositor>()
                .is_err()
        );
    }
}
