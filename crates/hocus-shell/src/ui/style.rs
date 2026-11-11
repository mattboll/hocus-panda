//! The kit's styles: the theme's tokens as iced's widgets take them, and
//! text set as the mockups set it.

use iced::{
    Background, Border, Color, Font,
    widget::{
        button, container, scrollable, text,
        text::{LineHeight, Shaping, Wrapping},
    },
};

use crate::{
    levels::draw::{DIM, LINE, TEXT},
    theme::Ink,
};

/// A row under the pointer.
pub const ROW_HOVER: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.05));
/// A list's scrollbar's thumb.
pub const THUMB: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.16));

/// `content` in `font` at `size`, on the mockups' line, unwrapped.
pub fn said<'a>(
    content: impl text::IntoFragment<'a>,
    size: f32,
    font: Font,
    colour: Color,
) -> iced::widget::Text<'a> {
    text(content)
        .size(size)
        .font(font)
        .line_height(LineHeight::Relative(LINE))
        .shaping(Shaping::Advanced)
        .wrapping(Wrapping::None)
        .color(colour)
}

/// Whether a button is under the pointer, or pressed.
pub fn lit(status: button::Status) -> bool {
    matches!(status, button::Status::Hovered | button::Status::Pressed)
}

/// Corners of `radius`, no edge.
pub fn round(radius: f32) -> Border {
    Border {
        radius: radius.into(),
        ..Border::default()
    }
}

/// A container filled with `fill`.
pub fn boxed(fill: Color, radius: f32) -> container::Style {
    container::Style {
        background: Some(fill.into()),
        border: round(radius),
        ..container::Style::default()
    }
}

/// A button filled, its words in `ink`.
pub fn filled(fill: Color, radius: f32, ink: Color) -> button::Style {
    button::Style {
        background: Some(fill.into()),
        text_color: ink,
        border: round(radius),
        ..button::Style::default()
    }
}

/// A button that draws nothing of its own.
pub fn clear() -> button::Style {
    button::Style {
        background: None,
        text_color: TEXT.get(),
        ..button::Style::default()
    }
}

/// Lit under the pointer: its fill; under high contrast, which has no
/// fills, a 1 px border in `text.dim` (as [`crate::theme::hover`]).
pub fn hovered(fill: Color, radius: f32) -> button::Style {
    if crate::theme::contrast() {
        button::Style {
            background: None,
            text_color: TEXT.get(),
            border: Border {
                color: DIM.get(),
                width: 1.0,
                radius: radius.into(),
            },
            ..button::Style::default()
        }
    } else {
        filled(fill, radius, TEXT.get())
    }
}

/// A list's scrollbar: a thumb `width` wide, no rail.
pub fn scrollbar(width: f32) -> scrollable::Style {
    scrollbar_in(width, THUMB.get())
}

/// A list's scrollbar: a thumb `width` wide in `colour`, no rail.
pub fn scrollbar_in(width: f32, colour: Color) -> scrollable::Style {
    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: Background::Color(colour),
            border: round(width / 2.0),
        },
    };
    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: Color::TRANSPARENT.into(),
            border: Border::default(),
            shadow: iced::Shadow::default(),
            icon: Color::TRANSPARENT,
        },
    }
}
