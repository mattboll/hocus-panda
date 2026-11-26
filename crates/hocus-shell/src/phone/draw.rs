//! A paired phone's or tablet's row on the connections' surface, as the
//! #957 mockup draws it (bar.md §4.3, "The phone"): under Bluetooth's, a
//! 30 px row with its kind's icon (18 px), its name cut with an ellipsis,
//! and on the right its battery ("82 %", 13 px, `text.dim`, the digits in
//! fixed cells) after a 9 px green bolt while charging; a 7 px orange dot
//! on the icon at 15 % or less and not charging, the digits then peach.
//! Out of reach: icon and name faint, "hors de portée" on the right. While
//! a file is sent, a 2 px line along its bottom fills with the transfer.
//! A file dragged over it lights it, "Lâcher : Pixel 8".
//!
//! Mini form: a 44 × 38 button, its icon at 20 px (faint out of reach),
//! the same dot.

use iced::{
    Color, Pixels, Point, Rectangle, Size,
    widget::{
        canvas::{self, Frame, Path},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::{Device, Kind};
use crate::{
    clock::time::Lang,
    levels::draw::{
        DIM, FAINT, ITEM_OUT, LINE, MANROPE, MANROPE_BOLD, TEXT, cell, faded, polygon, rgb, stroke,
    },
    network::draw::dot,
    theme::Ink,
};

const LABEL: f32 = 14.5;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const MINI_ICON: f32 = 20.0;
const BATTERY: f32 = 13.0;
const AWAY: f32 = 12.0;
/// The bolt before the battery while charging, and the gap after it.
const BOLT: f32 = 9.0;
const BOLT_GAP: f32 = 3.0;
const BOLT_COLOUR: Ink = Ink::Dark(rgb(0x6CC287));
/// The battery's digits when low.
const LOW_DIGITS: Ink = Ink::Dark(rgb(0xF7B98A));
/// The transfer's line: 2 px, 6 px in, over its track.
const LINE_HEIGHT: f32 = 2.0;
const LINE_IN: f32 = 6.0;
const LINE_FILL: Ink = Ink::Dark(rgb(0x8FB3FF));
const LINE_TRACK: Ink = Ink::Dark(Color::from_rgba8(143, 179, 255, 0.18));
/// A file dragged over it.
const DROP_FILL: Ink = Ink::Dark(Color::from_rgba8(91, 141, 239, 0.18));
const DROP_RIM: Ink = Ink::Dark(rgb(0x8FB3FF));
const DROP_TEXT: Ink = Ink::Dark(rgb(0xB9CEFF));

/// What a row keeps between draws: its words, cut and measured to fit.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RowState {
    pub key: u32,
    pub kind: Option<Kind>,
    pub reachable: bool,
    pub low: bool,
    pub charging: bool,
    label: String,
    /// The battery's digits, its cell's width, and " %"'s.
    battery: Option<(String, f32, f32)>,
    away: String,
    /// "Lâcher : Pixel 8", as it fits.
    drop: String,
    /// How far its transfer is, from 0 to 1.
    pub progress: Option<f32>,
    made_for: Option<(Device, u32, Lang)>,
}

impl RowState {
    /// What the row of `device` (number `key`) shows, made to fit `width`.
    pub fn refresh(&mut self, lang: Lang, key: u32, device: &Device, width: f32) {
        self.key = key;
        self.progress = device
            .transfer
            .as_ref()
            .filter(|t| !t.failed)
            .map(super::Transfer::fraction);
        let mut shape = device.clone();
        shape.transfer = None;
        let made = (shape, width as u32, lang);
        if self.made_for.as_ref() == Some(&made) {
            return;
        }
        self.kind = Some(device.kind);
        self.reachable = device.reachable;
        self.low = device.low();
        self.charging = device.battery.is_some_and(|b| b.charging) && device.reachable;
        let mut system = iced::advanced::graphics::text::font_system().write().ok();
        let mut measure = |text: &str, size: f32, font| match system.as_mut() {
            Some(system) => crate::popover::kit::measure_in(system.raw(), text, size, font),
            None => text.chars().count() as f32 * size * 0.55,
        };
        let mut room = width - 2.0 * ITEM_OUT - ICON - ICON_GAP;
        self.battery = None;
        self.away = String::new();
        if !device.reachable {
            self.away = super::txt::OUT_OF_REACH.get(lang).to_owned();
            room -= measure(&self.away, AWAY, MANROPE) + ICON_GAP;
        } else if let Some(battery) = device.battery {
            let digits = battery.charge.to_string();
            let zero = measure("0", BATTERY, MANROPE);
            let unit = measure(" %", BATTERY, MANROPE);
            room -= zero * digits.len() as f32 + unit + ICON_GAP;
            if self.charging {
                room -= BOLT + BOLT_GAP;
            }
            self.battery = Some((digits, zero, unit));
        }
        self.label =
            crate::popover::kit::elide(&device.name, room.max(0.0), |t| measure(t, LABEL, MANROPE));
        let drop = super::txt::drop_on(lang, &device.name);
        let drop_room = width - 2.0 * ITEM_OUT;
        self.drop =
            crate::popover::kit::elide(&drop, drop_room, |t| measure(t, LABEL, MANROPE_BOLD));
        self.made_for = Some(made);
    }
}

/// The full form's row in `rect`; `dropping`: a file is dragged over it.
pub fn full(f: &mut Frame, rect: Rectangle, state: &RowState, dropping: bool, alpha: f32) {
    let Some(kind) = state.kind else {
        return;
    };
    let middle = rect.center_y();
    if dropping {
        let ring = Rectangle::new(
            Point::new(rect.x + 0.75, rect.y + 0.75),
            Size::new(rect.width - 1.5, rect.height - 1.5),
        );
        let shape = Path::rounded_rectangle(ring.position(), ring.size(), 8.0.into());
        f.fill(&shape, faded(DROP_FILL.get(), alpha));
        f.stroke(&shape, stroke(1.5, faded(DROP_RIM.get(), alpha)));
        f.fill_text(text(
            state.drop.clone(),
            Point::new(rect.x + ITEM_OUT, middle),
            LABEL,
            faded(DROP_TEXT.get(), alpha),
            Alignment::Left,
            MANROPE_BOLD,
        ));
        return;
    }
    let colour = if state.reachable { TEXT } else { FAINT };
    let icon = Point::new(rect.x + ITEM_OUT, middle - ICON / 2.0);
    kind_icon(f, icon, ICON, kind, faded(colour.get(), alpha));
    if state.low {
        dot(
            f,
            Point::new(icon.x + ICON + 3.0 - 3.5, icon.y - 2.0 + 3.5),
            alpha,
        );
    }
    f.fill_text(text(
        state.label.clone(),
        Point::new(icon.x + ICON + ICON_GAP, middle),
        LABEL,
        faded(colour.get(), alpha),
        Alignment::Left,
        MANROPE,
    ));
    let right = rect.x + rect.width - ITEM_OUT;
    if !state.away.is_empty() {
        f.fill_text(text(
            state.away.clone(),
            Point::new(right, middle),
            AWAY,
            faded(FAINT.get(), alpha),
            Alignment::Right,
            MANROPE,
        ));
    }
    if let Some((digits, zero, unit)) = &state.battery {
        let ink = if state.low { LOW_DIGITS } else { DIM };
        f.fill_text(text(
            " %".to_owned(),
            Point::new(right, middle),
            BATTERY,
            faded(ink.get(), alpha),
            Alignment::Right,
            MANROPE,
        ));
        let mut x = right - unit - zero * digits.len() as f32;
        let start = x;
        for ch in digits.chars() {
            cell(
                f,
                ch,
                x + zero / 2.0,
                middle,
                BATTERY,
                MANROPE,
                faded(ink.get(), alpha),
            );
            x += zero;
        }
        if state.charging {
            bolt(
                f,
                Point::new(start - BOLT_GAP - BOLT, middle - BOLT / 2.0),
                BOLT,
                faded(BOLT_COLOUR.get(), alpha),
            );
        }
    }
    if let Some(progress) = state.progress {
        let track = Rectangle::new(
            Point::new(rect.x + LINE_IN, rect.y + rect.height - LINE_HEIGHT),
            Size::new(rect.width - 2.0 * LINE_IN, LINE_HEIGHT),
        );
        f.fill(
            &Path::rounded_rectangle(track.position(), track.size(), 1.0.into()),
            faded(LINE_TRACK.get(), alpha),
        );
        let done = track.width * progress.clamp(0.0, 1.0);
        if done > 0.0 {
            f.fill(
                &Path::rounded_rectangle(
                    track.position(),
                    Size::new(done, LINE_HEIGHT),
                    1.0.into(),
                ),
                faded(LINE_FILL.get(), alpha),
            );
        }
    }
}

/// The mini form's button in `rect`: its icon, 20 px, the dot 10 px from
/// the button's right and 7 from its top.
pub fn mini(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    let Some(kind) = state.kind else {
        return;
    };
    let icon = Point::new(
        rect.center_x() - MINI_ICON / 2.0,
        rect.center_y() - MINI_ICON / 2.0,
    );
    let colour = if state.reachable { TEXT } else { FAINT };
    kind_icon(f, icon, MINI_ICON, kind, faded(colour.get(), alpha));
    if state.low {
        let centre = Point::new(rect.x + rect.width - 10.0 - 3.5, rect.y + 7.0 + 3.5);
        dot(f, centre, alpha);
    }
}

fn text(
    content: String,
    position: Point,
    size: f32,
    color: Color,
    align: Alignment,
    font: iced::Font,
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

// --- Icons, from the mockup's SVGs (a 24-unit box, 2 units of stroke) ----------

/// A phone (`rect x7 y2 w10 h20 rx2`, its button) or a tablet (`rect x4
/// y3 w16 h18 rx2`, its button), `size` px at `at`.
pub fn kind_icon(f: &mut Frame, at: Point, size: f32, kind: Kind, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    let (x, y, w, h, button) = match kind {
        Kind::Phone => (7.0, 2.0, 10.0, 20.0, 18.0),
        Kind::Tablet => (4.0, 3.0, 16.0, 18.0, 17.5),
    };
    f.stroke(
        &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (2.0 * s).into()),
        line,
    );
    f.stroke(&Path::line(p(11.0, button), p(13.0, button)), line);
}

/// A bolt, `size` px at `at`: `M13 2 4 14h7l-1 8 9-12h-7z`.
pub fn bolt(f: &mut Frame, at: Point, size: f32, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    f.fill(
        &polygon(&[
            p(13.0, 2.0),
            p(4.0, 14.0),
            p(11.0, 14.0),
            p(10.0, 22.0),
            p(19.0, 10.0),
            p(12.0, 10.0),
        ]),
        colour,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone::{Battery, Transfer};

    fn device(name: &str) -> Device {
        Device {
            id: "a1".to_owned(),
            name: name.to_owned(),
            kind: Kind::Phone,
            reachable: true,
            battery: Some(Battery {
                charge: 82,
                charging: false,
            }),
            ring: true,
            share: true,
            came: 1,
            transfer: None,
        }
    }

    #[test]
    fn a_long_name_gives_way_to_the_battery_and_out_of_reach() {
        let mut state = RowState::default();
        state.refresh(Lang::French, 1, &device("Pixel 8"), 184.0);
        assert_eq!(state.label, "Pixel 8");
        assert_eq!(state.battery.as_ref().map(|b| b.0.as_str()), Some("82"));
        let long = device("Un téléphone au nom vraiment beaucoup trop long");
        state.refresh(Lang::French, 1, &long, 184.0);
        assert!(state.label.ends_with('…'), "{}", state.label);
        let mut away = device("Pixel 8");
        away.reachable = false;
        state.refresh(Lang::French, 1, &away, 184.0);
        assert!(state.battery.is_none());
        assert_eq!(state.away, "hors de portée");
    }

    #[test]
    fn a_transfer_fills_its_line_and_redraws_nothing_else() {
        let mut state = RowState::default();
        let mut d = device("Pixel 8");
        state.refresh(Lang::French, 1, &d, 184.0);
        let made = state.made_for.clone();
        d.transfer = Some(Transfer {
            view: 1,
            file: "a.pdf".to_owned(),
            files: 1,
            processed: 42,
            total: 100,
            failed: false,
        });
        state.refresh(Lang::French, 1, &d, 184.0);
        assert_eq!(state.made_for, made);
        assert!(state.progress.is_some_and(|p| (p - 0.42).abs() < 1e-6));
    }
}
