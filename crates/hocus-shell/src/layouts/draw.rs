//! The keyboard layout's row and button, as the 3.8 mockup draws them
//! (bar.md §4.3, §5).
//!
//! Full form: a 30 px row under Bluetooth's, on the connections' surface:
//! the keyboard icon (18 px), the layout's full name in `text` cut with an
//! ellipsis, and its code on the right in a chip (12 px bold, `text.dim` on
//! `rgba(255,255,255,0.08)`, radius 6, 6 px each side). Mini form: the code
//! alone (13 px bold, `text`) in a 44 × 30 button under the battery, on the
//! levels' surface.

use iced::{
    Color, Pixels, Point, Rectangle, Size,
    widget::{
        canvas::{self, Frame, Path},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::{Layout, Layouts};
use crate::{
    bluetooth::{self, model::Kind},
    levels::draw::{DIM, ITEM_OUT, LINE, MANROPE, MANROPE_BOLD, TEXT, faded},
};

const LABEL: f32 = 14.5;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const CHIP: f32 = 12.0;
const CHIP_PAD: f32 = 6.0;
const CHIP_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);
/// The mini form's button, and its code's size.
pub const MINI_BUTTON: Size = Size::new(44.0, 30.0);
const MINI_CODE: f32 = 13.0;
/// The button's corners, the mockup's (the other mini items' are 10).
pub const MINI_RADIUS: f32 = 8.0;

/// What the row (or the button) shows, made to fit.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RowState {
    pub layout: Option<Layout>,
    /// The name as it fits, and the chip's width.
    label: String,
    chip: f32,
    /// The code as it fits in the button.
    mini: String,
    made_for: Option<(Layout, u32)>,
}

impl RowState {
    /// What `layouts` shows now, made to fit a row `width` wide.
    pub fn refresh(&mut self, layouts: &Layouts, width: f32) {
        let layout = layouts.shown().cloned();
        if let Some(shown) = &layout {
            let key = (shown.clone(), width as u32);
            if self.made_for.as_ref() != Some(&key) {
                self.fit(shown, width);
                self.made_for = Some(key);
            }
        }
        if self.layout != layout {
            tracing::debug!(?layout, "the layout's row shows it");
        }
        self.layout = layout;
    }

    fn fit(&mut self, layout: &Layout, width: f32) {
        let mut system = iced::advanced::graphics::text::font_system().write().ok();
        let mut measure = |text: &str, font, size: f32| match system.as_mut() {
            Some(system) => crate::popover::kit::measure_in(system.raw(), text, size, font),
            None => text.chars().count() as f32 * size * 0.6,
        };
        self.chip = measure(&layout.code, MANROPE_BOLD, CHIP) + 2.0 * CHIP_PAD;
        let room = width - 2.0 * ITEM_OUT - ICON - ICON_GAP - self.chip - ICON_GAP;
        self.label =
            crate::popover::kit::elide(&layout.name, room.max(0.0), |t| measure(t, MANROPE, LABEL));
        let button = MINI_BUTTON.width - 4.0;
        self.mini = crate::popover::kit::elide(&layout.code, button, |t| {
            measure(t, MANROPE_BOLD, MINI_CODE)
        });
    }
}

/// The full form's row in `rect`.
pub fn full(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    if state.layout.is_none() {
        return;
    }
    let icon = Point::new(rect.x + ITEM_OUT, rect.center_y() - ICON / 2.0);
    bluetooth::draw::kind(f, icon, ICON, Kind::Keyboard, faded(TEXT, alpha));
    let middle = rect.center_y();
    f.fill_text(text(
        state.label.clone(),
        Point::new(icon.x + ICON + ICON_GAP, middle),
        (MANROPE, LABEL),
        faded(TEXT, alpha),
        Alignment::Left,
    ));
    let height = CHIP * LINE + 2.0;
    let right = rect.x + rect.width - ITEM_OUT;
    let chip = Rectangle::new(
        Point::new(right - state.chip, middle - height / 2.0),
        Size::new(state.chip, height),
    );
    f.fill(
        &Path::rounded_rectangle(chip.position(), chip.size(), 6.0.into()),
        faded(CHIP_FILL, alpha),
    );
    if let Some(layout) = &state.layout {
        f.fill_text(text(
            layout.code.clone(),
            chip.center(),
            (MANROPE_BOLD, CHIP),
            faded(DIM, alpha),
            Alignment::Center,
        ));
    }
}

/// The mini form's button in `rect`: the code alone.
pub fn mini(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    if state.layout.is_none() {
        return;
    }
    f.fill_text(text(
        state.mini.clone(),
        rect.center(),
        (MANROPE_BOLD, MINI_CODE),
        faded(TEXT, alpha),
        Alignment::Center,
    ));
}

fn text(
    content: String,
    position: Point,
    (font, size): (iced::Font, f32),
    color: Color,
    align: Alignment,
) -> canvas::Text {
    canvas::Text {
        content,
        position,
        max_width: f32::INFINITY,
        color,
        size: Pixels(size),
        line_height: LineHeight::Relative(LINE),
        font,
        align_x: align,
        align_y: iced::alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_name_gives_way_to_its_code() {
        let mut layouts = Layouts {
            list: vec![
                Layout {
                    name: "Anglais (US, international avec touches mortes)".into(),
                    code: "US".into(),
                },
                Layout {
                    name: "Russe".into(),
                    code: "RU".into(),
                },
            ],
            active: 0,
        };
        let mut state = RowState::default();
        state.refresh(&layouts, 184.0);
        assert!(state.label.ends_with('…'), "{}", state.label);
        assert!(state.chip > 2.0 * CHIP_PAD);
        assert_eq!(state.mini, "US");
        layouts.active = 1;
        state.refresh(&layouts, 184.0);
        assert_eq!(state.label, "Russe");
        // One layout: nothing to show.
        layouts.list.pop();
        state.refresh(&layouts, 184.0);
        assert_eq!(state.layout, None);
    }
}
