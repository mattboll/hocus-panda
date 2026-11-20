//! A mounted drive's row on the connections' surface, as the #898 mockup
//! draws it (bar.md §4.3, "Removable drives"): under Bluetooth's, a 30 px
//! row with the drive's icon (18 px), its name cut with an ellipsis, its
//! free space on the right ("14 Go", 13 px, `text.dim`) and the ⏏ button
//! last. The full form only: the mini form shows no drive.

use iced::{
    Color, Pixels, Point, Rectangle, Size,
    widget::{
        canvas::{self, Frame, Path},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::Drive;
use crate::{
    clock::time::Lang,
    levels::draw::{DIM, ITEM_OUT, LINE, MANROPE, TEXT, faded, stroke},
};

const LABEL: f32 = 14.5;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const FREE: f32 = 13.0;
/// Between the free space and the ⏏.
const FREE_GAP: f32 = 4.0;
/// The ⏏ button: its square, how far it reaches into the row's padding,
/// and its sign.
pub const EJECT: f32 = 22.0;
const EJECT_OUT: f32 = 3.0;
const EJECT_SIGN: f32 = 12.0;

/// What a drive's row keeps between draws: its words, cut to fit.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RowState {
    pub serial: u32,
    label: String,
    free: String,
    made_for: Option<(String, String, u32)>,
}

impl RowState {
    /// What the row shows of `drive`, made to fit `width` (the row's).
    pub fn refresh(&mut self, lang: Lang, drive: &Drive, width: f32) {
        self.serial = drive.serial;
        let name = super::said_name(drive, lang);
        let free = drive
            .free
            .map(|f| super::row_free(lang, f))
            .unwrap_or_default();
        let key = (name.clone(), free.clone(), width as u32);
        if self.made_for.as_ref() == Some(&key) {
            return;
        }
        let mut system = iced::advanced::graphics::text::font_system().write().ok();
        let mut measure = |text: &str, size: f32| match system.as_mut() {
            Some(system) => crate::popover::kit::measure_in(system.raw(), text, size, MANROPE),
            None => text.chars().count() as f32 * size * 0.55,
        };
        let mut room = width - 2.0 * ITEM_OUT - ICON - ICON_GAP - (EJECT - EJECT_OUT) - ICON_GAP;
        if !free.is_empty() {
            room -= measure(&free, FREE) + FREE_GAP;
        }
        self.label = crate::popover::kit::elide(&name, room.max(0.0), |t| measure(t, LABEL));
        self.free = free;
        self.made_for = Some(key);
    }
}

/// The ⏏ button of the row in `rect`.
pub fn eject_rect(rect: Rectangle) -> Rectangle {
    Rectangle::new(
        Point::new(
            rect.x + rect.width - ITEM_OUT + EJECT_OUT - EJECT,
            rect.center_y() - EJECT / 2.0,
        ),
        Size::new(EJECT, EJECT),
    )
}

/// The full form's row in `rect`.
pub fn full(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    let icon = Point::new(rect.x + ITEM_OUT, rect.center_y() - ICON / 2.0);
    let colour = faded(TEXT.get(), alpha);
    drive_icon(f, icon, ICON, colour);
    let middle = rect.center_y();
    f.fill_text(text(
        state.label.clone(),
        Point::new(icon.x + ICON + ICON_GAP, middle),
        LABEL,
        colour,
        Alignment::Left,
    ));
    let eject = eject_rect(rect);
    let sign = Point::new(
        eject.center_x() - EJECT_SIGN / 2.0,
        eject.center_y() - EJECT_SIGN / 2.0,
    );
    eject_sign(f, sign, EJECT_SIGN, faded(DIM.get(), alpha));
    if !state.free.is_empty() {
        f.fill_text(text(
            state.free.clone(),
            Point::new(eject.x - FREE_GAP, middle),
            FREE,
            faded(DIM.get(), alpha),
            Alignment::Right,
        ));
    }
}

fn text(
    content: String,
    position: Point,
    size: f32,
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
        font: MANROPE,
        align_x: align,
        align_y: iced::alignment::Vertical::Center,
        shaping: Shaping::Advanced,
    }
}

/// The mockup's drive, `size` px at `at`: `rect x7 y3 w10 h9 rx1.5` over
/// `rect x5 y11 w14 h10 rx2`, 2 units of stroke.
pub fn drive_icon(f: &mut Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    for (x, y, w, h, r) in [(7.0, 3.0, 10.0, 9.0, 1.5), (5.0, 11.0, 14.0, 10.0, 2.0)] {
        f.stroke(
            &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (r * s).into()),
            line,
        );
    }
}

/// ⏏, `size` px at `at`: a triangle over a bar.
fn eject_sign(f: &mut Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let triangle = Path::new(|b| {
        b.move_to(p(12.0, 3.0));
        b.line_to(p(21.0, 14.0));
        b.line_to(p(3.0, 14.0));
        b.close();
    });
    f.fill(&triangle, colour);
    f.fill(
        &Path::rounded_rectangle(p(3.0, 17.0), Size::new(18.0 * s, 3.5 * s), (1.0 * s).into()),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::removable::Kind;

    #[test]
    fn a_long_name_gives_way_to_the_free_space_and_the_eject_button() {
        let mut drive = Drive {
            serial: 1,
            name: "KINGSTON".to_owned(),
            kind: Kind::Stick,
            size: 32_000_000_000,
            free: Some(14_200_000_000),
            mount: Some("/media/alex/KINGSTON".into()),
            encrypted: false,
        };
        drive.name = "KEY".to_owned();
        let mut state = RowState::default();
        state.refresh(Lang::French, &drive, 184.0);
        assert_eq!(
            (state.label.as_str(), state.free.as_str()),
            ("KEY", "14 Go")
        );
        drive.name = "Une clé au nom vraiment beaucoup trop long".to_owned();
        state.refresh(Lang::French, &drive, 184.0);
        assert!(state.label.ends_with('…'), "{}", state.label);
        // The button sits inside the row, on its right.
        let row = Rectangle::new(Point::new(0.0, 0.0), Size::new(184.0, 30.0));
        let eject = eject_rect(row);
        assert!(eject.x + eject.width <= row.width && eject.x > row.width / 2.0);
        assert_eq!(eject.center_y(), row.center_y());
    }
}
