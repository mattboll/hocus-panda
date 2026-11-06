//! A window's actions (#723, docs/design/windows.md §10b, bar.md): the
//! compositor's radial menu runs them itself; the bar's menu of a window
//! asks for them on the line.
//!
//! - From the shell: `window-action ID ACTION` ([`WindowAction`]) runs an
//!   action on window `ID`; `send-to ID OUTPUT N` sends it to desktop `N`
//!   (from 0) of screen `OUTPUT`; `window-menu ID` asks what its menu
//!   shows, which the compositor answers at once.
//! - To the shell: `window-menu ID FLAGS\tSCREEN…` ([`WindowMenu`]), that
//!   answer; `window-states ABOVE ALL` ([`WindowStates`]): the windows
//!   kept above the others and those on all desktops, for the marks after
//!   their titles, sent as the shell connects and when they change.

use std::fmt;

use crate::{BadLine, screen_name, word};

/// An action on a window, the same from the radial menu and the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowAction {
    /// Maximised, or back (`Super+↑` / `Super+↓`).
    Maximize,
    /// `Super+H`.
    Minimize,
    /// Sent to the desktop on that side of its screen's current one; the
    /// screen stays.
    PreviousDesktop,
    NextDesktop,
    /// Kept above the others, or no longer.
    Above,
    /// Shown on every desktop of its screen, or on the current one only.
    AllDesktops,
    /// Sent to the next screen by position, onto the desktop it shows.
    OtherScreen,
    /// Asked to close, as `Super+Q`: the application decides.
    Close,
}

impl WindowAction {
    pub const ALL: [Self; 8] = [
        Self::Maximize,
        Self::Minimize,
        Self::PreviousDesktop,
        Self::NextDesktop,
        Self::Above,
        Self::AllDesktops,
        Self::OtherScreen,
        Self::Close,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Maximize => "maximize",
            Self::Minimize => "minimize",
            Self::PreviousDesktop => "previous-desktop",
            Self::NextDesktop => "next-desktop",
            Self::Above => "above",
            Self::AllDesktops => "all-desktops",
            Self::OtherScreen => "other-screen",
            Self::Close => "close",
        }
    }

    pub fn read(word: &str) -> Result<Self, BadLine> {
        Self::ALL
            .into_iter()
            .find(|a| a.name() == word)
            .ok_or_else(|| BadLine(format!("no window action {word:?}")))
    }
}

/// A screen as the bar's menu of a window shows it under "Send to".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuScreen {
    /// Its connector's name, which [`crate::ToCompositor::SendTo`] names.
    pub output: String,
    /// How many desktops it has, and the one it shows (from 0).
    pub count: u8,
    pub current: u8,
    /// Its size, logical pixels: the tiles take its shape.
    pub width: u32,
    pub height: u32,
    /// The window's desktop, when the window is on this screen.
    pub here: Option<u8>,
    /// The built-in screen ("this laptop").
    pub internal: bool,
    /// What the user knows it by (its model), else its connector's name.
    pub name: String,
}

/// What the bar's menu of window `id` shows: its states and the screens
/// it can be sent to, left to right.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowMenu {
    pub id: u64,
    pub maximized: bool,
    pub above: bool,
    pub all_desktops: bool,
    pub screens: Vec<MenuScreen>,
}

impl WindowMenu {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flag = |on: bool, word: &'static str| if on { word } else { "-" };
        write!(
            f,
            "window-menu {} {} {} {}",
            self.id,
            flag(self.maximized, "maximized"),
            flag(self.above, "above"),
            flag(self.all_desktops, "all"),
        )?;
        for s in &self.screens {
            let here = s.here.map_or_else(|| "-".to_owned(), |d| d.to_string());
            write!(
                f,
                "\t{} {} {} {} {} {here} {} {}",
                screen_name(&s.output).replace(' ', "_"),
                s.count,
                s.current,
                s.width,
                s.height,
                if s.internal { "internal" } else { "external" },
                screen_name(&s.name),
            )?;
        }
        Ok(())
    }

    /// What follows `window-menu `.
    pub(crate) fn read(rest: &str) -> Result<Self, BadLine> {
        let mut fields = rest.split('\t');
        let mut words = word(&mut fields, "window")?.split(' ');
        let number = |w: &str, what: &str| -> Result<u64, BadLine> {
            w.parse().map_err(|e| BadLine(format!("{what}: {e}")))
        };
        let id = number(word(&mut words, "window")?, "window")?;
        let mut flag = |on: &str| -> Result<bool, BadLine> {
            match word(&mut words, on)? {
                w if w == on => Ok(true),
                "-" => Ok(false),
                other => Err(BadLine(format!("{other:?}: {on} or -"))),
            }
        };
        let maximized = flag("maximized")?;
        let above = flag("above")?;
        let all_desktops = flag("all")?;
        let screens = fields
            .map(|field| {
                let mut words = field.splitn(8, ' ');
                let output = word(&mut words, "output")?.to_owned();
                let small = |w: &str, what: &str| -> Result<u8, BadLine> {
                    w.parse().map_err(|e| BadLine(format!("{what}: {e}")))
                };
                let count = small(word(&mut words, "count")?, "count")?;
                let current = small(word(&mut words, "current")?, "current")?;
                let width = number(word(&mut words, "width")?, "width")? as u32;
                let height = number(word(&mut words, "height")?, "height")? as u32;
                let here = match word(&mut words, "desktop")? {
                    "-" => None,
                    d => Some(small(d, "desktop")?),
                };
                let internal = match word(&mut words, "kind")? {
                    "internal" => true,
                    "external" => false,
                    other => return Err(BadLine(format!("{other:?}: internal or external"))),
                };
                let name = words.next().unwrap_or_default().to_owned();
                Ok(MenuScreen {
                    output,
                    count,
                    current,
                    width,
                    height,
                    here,
                    internal,
                    name,
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            id,
            maximized,
            above,
            all_desktops,
            screens,
        })
    }
}

/// The windows in the two states the bar marks after their titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowStates {
    /// Kept above the others.
    pub above: Vec<u64>,
    /// On every desktop of their screen.
    pub all_desktops: Vec<u64>,
}

fn ids(f: &mut fmt::Formatter<'_>, ids: &[u64]) -> fmt::Result {
    if ids.is_empty() {
        return f.write_str("-");
    }
    for (i, id) in ids.iter().enumerate() {
        if i > 0 {
            f.write_str(",")?;
        }
        write!(f, "{id}")?;
    }
    Ok(())
}

fn read_ids(word: &str) -> Result<Vec<u64>, BadLine> {
    if word == "-" {
        return Ok(Vec::new());
    }
    word.split(',')
        .map(|w| w.parse().map_err(|e| BadLine(format!("a window: {e}"))))
        .collect()
}

impl WindowStates {
    pub(crate) fn write(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("window-states ")?;
        ids(f, &self.above)?;
        f.write_str(" ")?;
        ids(f, &self.all_desktops)
    }

    /// What follows `window-states `.
    pub(crate) fn read(rest: &str) -> Result<Self, BadLine> {
        let mut words = rest.split(' ');
        Ok(Self {
            above: read_ids(word(&mut words, "above")?)?,
            all_desktops: read_ids(word(&mut words, "all")?)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ToCompositor, ToShell};

    #[test]
    fn the_window_menus_lines_read_back_as_written() {
        let menu = WindowMenu {
            id: 12,
            maximized: true,
            above: false,
            all_desktops: true,
            screens: vec![
                MenuScreen {
                    output: "eDP-1".into(),
                    count: 4,
                    current: 1,
                    width: 1707,
                    height: 1067,
                    here: Some(0),
                    internal: true,
                    name: "eDP-1".into(),
                },
                MenuScreen {
                    output: "DP-2".into(),
                    count: 2,
                    current: 0,
                    width: 3440,
                    height: 1440,
                    here: None,
                    internal: false,
                    name: "Dell U3419W wide".into(),
                },
            ],
        };
        for line in [
            ToShell::WindowMenu(menu.clone()),
            ToShell::WindowMenu(WindowMenu {
                screens: Vec::new(),
                ..menu
            }),
            ToShell::WindowStates(WindowStates::default()),
            ToShell::WindowStates(WindowStates {
                above: vec![3, 9],
                all_desktops: vec![4],
            }),
        ] {
            assert_eq!(line.to_string().parse::<ToShell>(), Ok(line));
        }
        for action in WindowAction::ALL {
            let line = ToCompositor::WindowAction { id: 7, action };
            assert_eq!(line.to_string().parse::<ToCompositor>(), Ok(line));
        }
        for line in [
            ToCompositor::SendTo {
                id: 7,
                output: "HDMI-A-1".into(),
                desktop: 2,
            },
            ToCompositor::WindowMenu(7),
        ] {
            assert_eq!(line.to_string().parse::<ToCompositor>(), Ok(line));
        }
        assert!("window-action 7 fly".parse::<ToCompositor>().is_err());
        assert!("window-states 1,x -".parse::<ToShell>().is_err());
    }
}
