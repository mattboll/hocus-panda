//! A screen corner or a touchpad swipe that runs an action, written in
//! `[shortcuts]` beside the action's keys (issue #893):
//! `overview = ["Super+W", "corner:top-left", "swipe:4:down"]`.
//!
//! The compositor fires them (`hot_corner.rs`, `gestures.rs`); the
//! Settings show and write them (Keyboard and shortcuts › Shortcuts). The
//! older switches that turned the default gestures and corner off
//! (`[touchpad] swipe-desktops`, `swipe-launcher`, `[overview]
//! hot-corner`) are still read: set to false, each takes its defaults
//! away from an action whose line the file does not write ([`RETIRED`]).

/// One of a screen's four corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

impl Corner {
    pub const ALL: [Self; 4] = [
        Self::TopLeft,
        Self::TopRight,
        Self::BottomLeft,
        Self::BottomRight,
    ];

    /// As the file writes it, after `corner:`.
    pub fn word(self) -> &'static str {
        match self {
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
        }
    }

    pub fn top(self) -> bool {
        matches!(self, Self::TopLeft | Self::TopRight)
    }

    pub fn left(self) -> bool {
        matches!(self, Self::TopLeft | Self::BottomLeft)
    }

    /// The bar's own corner, with the bar on that side (`[panel] side`):
    /// the pointer pushed there brings the bar in front, so it takes no
    /// action.
    pub fn bars(self, bar_on_the_left: bool) -> bool {
        self == if bar_on_the_left {
            Self::TopLeft
        } else {
            Self::TopRight
        }
    }
}

/// Which way the fingers go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Toward {
    Left,
    Right,
    Up,
    Down,
}

impl Toward {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Up, Self::Down];

    pub fn word(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Up => "up",
            Self::Down => "down",
        }
    }

    pub fn arrow(self) -> &'static str {
        match self {
            Self::Left => "←",
            Self::Right => "→",
            Self::Up => "↑",
            Self::Down => "↓",
        }
    }

    /// The unit vector of that way, y down.
    pub fn unit(self) -> (f64, f64) {
        match self {
            Self::Left => (-1.0, 0.0),
            Self::Right => (1.0, 0.0),
            Self::Up => (0.0, -1.0),
            Self::Down => (0.0, 1.0),
        }
    }

    pub fn opposite(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }

    /// The way a travel of (`dx`, `dy`) mostly goes.
    pub fn of(dx: f64, dy: f64) -> Self {
        if dx.abs() >= dy.abs() {
            if dx < 0.0 { Self::Left } else { Self::Right }
        } else if dy < 0.0 {
            Self::Up
        } else {
            Self::Down
        }
    }
}

/// The finger counts a swipe of the desktop's takes.
pub const FINGERS: [u8; 2] = [3, 4];

/// A corner or a swipe that runs an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Trigger {
    Corner(Corner),
    /// Three or four fingers that way.
    Swipe {
        fingers: u8,
        toward: Toward,
    },
}

impl Trigger {
    /// The four corners, then the eight swipes, in the Settings' order.
    pub fn all() -> impl Iterator<Item = Self> {
        Corner::ALL
            .into_iter()
            .map(Self::Corner)
            .chain(FINGERS.into_iter().flat_map(|fingers| {
                Toward::ALL
                    .into_iter()
                    .map(move |toward| Self::Swipe { fingers, toward })
            }))
    }

    /// `corner:top-left`, `swipe:3:left`; `None` for anything else (a
    /// shortcut's keys, a word misspelt).
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if let Some(corner) = text.strip_prefix("corner:") {
            return Corner::ALL
                .into_iter()
                .find(|c| c.word() == corner)
                .map(Self::Corner);
        }
        let swipe = text.strip_prefix("swipe:")?;
        let (fingers, toward) = swipe.split_once(':')?;
        let fingers: u8 = fingers.parse().ok().filter(|n| FINGERS.contains(n))?;
        let toward = Toward::ALL.into_iter().find(|t| t.word() == toward)?;
        Some(Self::Swipe { fingers, toward })
    }

    /// Whether `text` is meant as one (it names a corner or a swipe), read
    /// or not: such a text is never a shortcut's keys.
    pub fn meant(text: &str) -> bool {
        let text = text.trim();
        text.starts_with("corner:") || text.starts_with("swipe:")
    }

    /// As the file writes it.
    pub fn written(self) -> String {
        match self {
            Self::Corner(corner) => format!("corner:{}", corner.word()),
            Self::Swipe { fingers, toward } => format!("swipe:{fingers}:{}", toward.word()),
        }
    }
}

/// An older switch that takes default triggers away: its key, then the
/// action (its `[shortcuts]` key) and the trigger it takes away, while
/// that action's line is not in the file.
#[derive(Debug, Clone, Copy)]
pub struct Retired {
    pub switch: [&'static str; 2],
    pub action: &'static str,
    pub trigger: Trigger,
}

const fn swipe(fingers: u8, toward: Toward) -> Trigger {
    Trigger::Swipe { fingers, toward }
}

/// Every default trigger an older switch takes away when false.
pub const RETIRED: [Retired; 5] = [
    Retired {
        switch: ["touchpad", "swipe-desktops"],
        action: "desktop-left",
        trigger: swipe(3, Toward::Right),
    },
    Retired {
        switch: ["touchpad", "swipe-desktops"],
        action: "desktop-right",
        trigger: swipe(3, Toward::Left),
    },
    Retired {
        switch: ["touchpad", "swipe-launcher"],
        action: "launcher",
        trigger: swipe(4, Toward::Up),
    },
    Retired {
        switch: ["touchpad", "swipe-launcher"],
        action: "overview",
        trigger: swipe(4, Toward::Down),
    },
    Retired {
        switch: ["overview", "hot-corner"],
        action: "overview",
        trigger: Trigger::Corner(Corner::TopLeft),
    },
];

/// Whether `trigger`, a default of `action`, is taken away by an older
/// switch `off` says is false (`off(["touchpad", "swipe-desktops"])`).
pub fn retired(action: &str, trigger: Trigger, off: impl Fn([&str; 2]) -> bool) -> bool {
    RETIRED
        .iter()
        .any(|r| r.action == action && r.trigger == trigger && off(r.switch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_trigger_is_read_and_written_back() {
        for trigger in Trigger::all() {
            assert_eq!(Trigger::parse(&trigger.written()), Some(trigger));
        }
        assert_eq!(Trigger::all().count(), 12);
        assert_eq!(
            Trigger::parse(" corner:top-left "),
            Some(Trigger::Corner(Corner::TopLeft))
        );
        assert_eq!(
            Trigger::parse("swipe:4:down"),
            Some(Trigger::Swipe {
                fingers: 4,
                toward: Toward::Down
            })
        );
        for bad in [
            "swipe:5:left",
            "swipe:3:sideways",
            "swipe:3",
            "corner:middle",
            "Super+W",
            "",
        ] {
            assert_eq!(Trigger::parse(bad), None, "{bad}");
        }
        assert!(Trigger::meant("swipe:5:left") && !Trigger::meant("Super+W"));
        assert_eq!(Toward::of(-30.0, 4.0), Toward::Left);
        assert_eq!(Toward::of(2.0, 30.0), Toward::Down);
    }

    #[test]
    fn an_older_switch_takes_its_defaults_away() {
        let desktops_off = |s: [&str; 2]| s == ["touchpad", "swipe-desktops"];
        assert!(retired(
            "desktop-left",
            swipe(3, Toward::Right),
            desktops_off
        ));
        assert!(!retired(
            "desktop-left",
            swipe(3, Toward::Left),
            desktops_off
        ));
        assert!(!retired("overview", swipe(4, Toward::Down), desktops_off));
        let corner_off = |s: [&str; 2]| s == ["overview", "hot-corner"];
        assert!(retired(
            "overview",
            Trigger::Corner(Corner::TopLeft),
            corner_off
        ));
        assert!(Corner::TopRight.bars(false) && Corner::TopLeft.bars(true));
        assert!(!Corner::BottomRight.bars(false));
    }
}
