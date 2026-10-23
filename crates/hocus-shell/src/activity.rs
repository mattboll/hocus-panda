//! What goes on on a desktop Alex is not looking at (#192), as the desktop
//! switchers show it: the strip, the column's dots and its desktops
//! popover, Alt+Tab and the overview. One badge per desktop, the one that
//! asks the most: an agent waiting for an answer, then a window asking for
//! attention, then an agent done since the desktop was last shown.

use std::time::Duration;

use hocus_ui::{Canvas, Color, Draw, Paint, Rect, Size, Theme};

use crate::i18n::Lang;

/// A desktop's badge; the later ones ask more of Alex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, PartialOrd, Ord, Hash)]
pub enum Badge {
    #[default]
    None,
    /// An agent finished since the desktop was last shown.
    Done,
    /// A window asked to come forward and was not let to.
    Attention,
    /// An agent waits for an answer.
    Waits,
}

impl Badge {
    /// Its colour: the waiting colour for what waits for Alex, the accent
    /// for what is done.
    pub fn paint(self) -> Option<Paint> {
        match self {
            Self::None => None,
            Self::Done => Some(Paint::Accent),
            Self::Attention | Self::Waits => Some(Paint::Waiting),
        }
    }

    pub fn is_some(self) -> bool {
        self != Self::None
    }

    /// The badge as a dot of `size` in `rect`'s centre: filled for what
    /// waits, a ring for what is done (a filled accent dot is the desktop
    /// shown).
    pub fn draw_dot(self, canvas: &mut Canvas, rect: Rect, size: f32, theme: &Theme) {
        let Some(paint) = self.paint() else {
            return;
        };
        let color: Color = theme.colors.get(paint);
        let x = rect.x + (rect.w - size) / 2.0;
        let y = rect.y + (rect.h - size) / 2.0;
        match self {
            Self::Done => {
                let r = size / 2.0;
                let centre = hocus_ui::Point::new(x + r, y + r);
                canvas.arc(centre, r - 1.0, 0.0, std::f32::consts::TAU, 2.0, color);
            }
            _ => canvas.fill_rect(Rect::new(x, y, size, size), size / 2.0, color),
        }
    }
}

/// A badge alone, as a dot its box's size.
#[derive(Debug)]
pub struct BadgeDot(pub Badge);

impl Draw for BadgeDot {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        self.0.draw_dot(canvas, rect, rect.w.min(rect.h), theme);
    }

    fn size(&self) -> Size {
        Size::new(8.0, 8.0)
    }
}

impl Lang {
    /// What a badge says to screen readers.
    pub fn badge(self, badge: Badge) -> Option<&'static str> {
        Some(match badge {
            Badge::None => return None,
            Badge::Waits => self.pick(
                "an agent waits for an answer",
                "un agent attend une réponse",
            ),
            Badge::Attention => self.pick(
                "a window asks for attention",
                "une fenêtre demande ton attention",
            ),
            Badge::Done => self.pick("an agent is done", "un agent a fini"),
        })
    }

    /// A desktop with its badge: "Website: an agent waits for an answer".
    pub fn desktop_badge(self, name: &str, badge: Badge) -> Option<String> {
        let what = self.badge(badge)?;
        Some(match self {
            Self::En => format!("{name}: {what}"),
            Self::Fr => format!("{name}\u{a0}: {what}"),
        })
    }

    /// A desktop by its name, or its number.
    pub fn desktop_label(self, name: &str, n: usize) -> String {
        if name.is_empty() {
            self.desktop(n)
        } else {
            name.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_badge_that_asks_most_wins_and_says_so() {
        assert_eq!(
            [Badge::Done, Badge::Waits, Badge::Attention].iter().max(),
            Some(&Badge::Waits)
        );
        assert_eq!(Badge::Attention.paint(), Some(Paint::Waiting));
        assert_eq!(Badge::Done.paint(), Some(Paint::Accent));
        assert_eq!(
            Lang::En.desktop_badge("Website", Badge::Waits).as_deref(),
            Some("Website: an agent waits for an answer")
        );
        assert_eq!(
            Lang::Fr.desktop_badge("Chapitô", Badge::Done).as_deref(),
            Some("Chapitô\u{a0}: un agent a fini")
        );
        assert_eq!(Lang::En.desktop_badge("Me", Badge::None), None);
        assert_eq!(Lang::En.desktop_label("", 3), "Desktop 3");
    }
}
