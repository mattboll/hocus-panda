//! Bluetooth's row on the connections' surface, as the 3.6 mockup draws it
//! (bar.md §4.3, §5), and the icons it and the popover share.
//!
//! Full form: a 30 px row under the Wi-Fi's: the Bluetooth sign (18 px),
//! then the device connected last, a "+N" chip when others are too, and
//! its battery on the right ("82 %", the digits in fixed cells); else
//! "Connexion…", "Aucun appareil", or "Bluetooth coupé" (the sign
//! slashed). A 7 px orange dot on the sign while a connected device's
//! battery is at 15 % or less. Mini form: a 44 × 38 button, the sign at
//! 20 px, the same dot.

use iced::{
    Color, Pixels, Point, Rectangle, Size,
    widget::{
        canvas::{self, Frame, Path},
        text::{Alignment, LineHeight, Shaping},
    },
};

use super::model::{Bluetooth, Kind, Row};
use crate::{
    levels::{
        draw::{DIM, FAINT, ITEM_OUT, LINE, MANROPE, TEXT, cell, faded, stroke},
        model::Words,
    },
    network::draw::dot,
};

const LABEL: f32 = 14.5;
const ICON: f32 = 18.0;
const ICON_GAP: f32 = 8.0;
const MINI_ICON: f32 = 20.0;
const BATTERY: f32 = 13.0;
const CHIP: f32 = 12.0;
const CHIP_PAD: f32 = 5.0;
const CHIP_FILL: Color = Color::from_rgba8(255, 255, 255, 0.08);

/// What the row keeps between draws: what it shows, and its words cut and
/// measured to fit.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RowState {
    pub row: Option<Row>,
    /// The name or the words, as they fit.
    label: String,
    made_for: Option<(Row, u32)>,
    /// "+N", and its width.
    chip: Option<(String, f32)>,
    /// The battery's digits, its cell's width, and " %"'s.
    battery: Option<(String, f32, f32)>,
}

impl RowState {
    /// What the row shows of `bluetooth`, made to fit `width` (the row's).
    pub fn refresh(&mut self, words: Words, bluetooth: &Bluetooth, width: f32) {
        let row = bluetooth.row();
        let key = (row.clone(), width as u32);
        if self.made_for.as_ref() != Some(&key) {
            self.made_for = Some(key);
            self.fit(words, &row, width);
        }
        if self.row.as_ref() != Some(&row) {
            tracing::debug!(?row, "Bluetooth's row shows it");
        }
        self.row = Some(row);
    }

    fn fit(&mut self, words: Words, row: &Row, width: f32) {
        let mut system = iced::advanced::graphics::text::font_system().write().ok();
        let mut measure = |text: &str, size: f32| match system.as_mut() {
            Some(system) => crate::popover::sound::measure(system.raw(), text, MANROPE, size),
            None => text.chars().count() as f32 * size * 0.55,
        };
        let mut room = width - 2.0 * ITEM_OUT - ICON - ICON_GAP;
        self.chip = None;
        self.battery = None;
        let label = match row {
            Row::Hidden => String::new(),
            Row::Off => words.bluetooth_off().to_owned(),
            Row::Nothing => words.no_device().to_owned(),
            Row::Connecting => words.connecting().to_owned(),
            Row::Connected {
                name,
                battery,
                more,
                ..
            } => {
                if let Some(percent) = battery {
                    let digits = percent.to_string();
                    let zero = measure("0", BATTERY);
                    let unit = measure(" %", BATTERY);
                    room -= zero * digits.len() as f32 + unit + ICON_GAP;
                    self.battery = Some((digits, zero, unit));
                }
                if *more > 0 {
                    let text = format!("+{more}");
                    let chip = measure(&text, CHIP) + 2.0 * CHIP_PAD;
                    room -= chip + ICON_GAP;
                    self.chip = Some((text, chip));
                }
                name.clone()
            }
        };
        self.label = crate::popover::sound::elide(&label, room.max(0.0), |t| measure(t, LABEL));
    }
}

/// The full form's row in `rect`.
pub fn full(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    let Some(row) = &state.row else {
        return;
    };
    let icon = Point::new(rect.x + ITEM_OUT, rect.center_y() - ICON / 2.0);
    let (colour, slash) = colours(row);
    sign(f, icon, ICON, faded(colour, alpha), slash, alpha);
    if matches!(row, Row::Connected { low: true, .. }) {
        dot(
            f,
            Point::new(icon.x + ICON + 3.0 - 3.5, icon.y - 2.0 + 3.5),
            alpha,
        );
    }
    let middle = rect.center_y();
    f.fill_text(text(
        state.label.clone(),
        Point::new(icon.x + ICON + ICON_GAP, middle),
        LABEL,
        faded(colour, alpha),
        Alignment::Left,
    ));
    let mut right = rect.x + rect.width - ITEM_OUT;
    if let Some((digits, zero, unit)) = &state.battery {
        f.fill_text(text(
            " %".to_owned(),
            Point::new(right, middle),
            BATTERY,
            faded(DIM, alpha),
            Alignment::Right,
        ));
        right -= unit;
        let mut x = right - zero * digits.len() as f32;
        for ch in digits.chars() {
            cell(
                f,
                ch,
                x + zero / 2.0,
                middle,
                BATTERY,
                MANROPE,
                faded(DIM, alpha),
            );
            x += zero;
        }
        right -= zero * digits.len() as f32 + ICON_GAP;
    }
    if let Some((more, width)) = &state.chip {
        let height = CHIP * LINE + 2.0;
        let chip = Rectangle::new(
            Point::new(right - width, middle - height / 2.0),
            Size::new(*width, height),
        );
        f.fill(
            &Path::rounded_rectangle(chip.position(), chip.size(), 6.0.into()),
            faded(CHIP_FILL, alpha),
        );
        f.fill_text(text(
            more.clone(),
            chip.center(),
            CHIP,
            faded(DIM, alpha),
            Alignment::Center,
        ));
    }
}

/// The mini form's button in `rect`: the sign, 20 px, and the dot 10 px
/// from the button's right and 7 from its top, as the mockup has it.
pub fn mini(f: &mut Frame, rect: Rectangle, state: &RowState, alpha: f32) {
    let Some(row) = &state.row else {
        return;
    };
    let icon = Point::new(
        rect.center_x() - MINI_ICON / 2.0,
        rect.center_y() - MINI_ICON / 2.0,
    );
    let (colour, slash) = colours(row);
    sign(f, icon, MINI_ICON, faded(colour, alpha), slash, alpha);
    if matches!(row, Row::Connected { low: true, .. }) {
        let centre = Point::new(rect.x + rect.width - 10.0 - 3.5, rect.y + 7.0 + 3.5);
        dot(f, centre, alpha);
    }
}

/// The sign's and the label's colour, and whether the sign is slashed.
fn colours(row: &Row) -> (Color, bool) {
    match row {
        Row::Connected { .. } => (TEXT, false),
        Row::Connecting => (DIM, false),
        Row::Nothing | Row::Hidden => (FAINT, false),
        Row::Off => (FAINT, true),
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

// --- Icons, from the mockup's SVGs (a 24-unit box, 2 units of stroke) ----------

/// The Bluetooth sign, `size` px at `at`: `M7 7l10 10-5 5V2l5 5L7 17`,
/// slashed (`M4 3l16 18`) when off.
pub fn sign(f: &mut Frame, at: Point, size: f32, colour: Color, slash: bool, alpha: f32) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let rune = Path::new(|b| {
        b.move_to(p(7.0, 7.0));
        for (x, y) in [
            (17.0, 17.0),
            (12.0, 22.0),
            (12.0, 2.0),
            (17.0, 7.0),
            (7.0, 17.0),
        ] {
            b.line_to(p(x, y));
        }
    });
    f.stroke(&rune, stroke(2.0 * s, colour));
    if slash {
        f.stroke(
            &Path::line(p(4.0, 3.0), p(20.0, 21.0)),
            stroke(2.0 * s, faded(FAINT, alpha)),
        );
    }
}

/// A device's icon for its kind, `size` px at `at`; the Bluetooth sign for
/// anything else.
pub fn kind(f: &mut Frame, at: Point, size: f32, kind: Kind, colour: Color) {
    let s = size / 24.0;
    let p = |x: f32, y: f32| Point::new(at.x + x * s, at.y + y * s);
    let line = stroke(2.0 * s, colour);
    let rect = |f: &mut Frame, x: f32, y: f32, w: f32, h: f32, r: f32| {
        f.stroke(
            &Path::rounded_rectangle(p(x, y), Size::new(w * s, h * s), (r * s).into()),
            line,
        );
    };
    match kind {
        Kind::Headphones => {
            // `M4 15v-3a8 8 0 0 1 16 0v3`: up, over the top, down.
            let band = Path::new(|b| {
                b.move_to(p(4.0, 15.0));
                b.line_to(p(4.0, 12.0));
                b.arc(canvas::path::Arc {
                    center: p(12.0, 12.0),
                    radius: 8.0 * s,
                    start_angle: iced::Radians(std::f32::consts::PI),
                    end_angle: iced::Radians(std::f32::consts::TAU),
                });
                b.line_to(p(20.0, 15.0));
            });
            f.stroke(&band, line);
            rect(f, 3.0, 14.0, 4.0, 7.0, 1.5);
            rect(f, 17.0, 14.0, 4.0, 7.0, 1.5);
        }
        Kind::Mouse => {
            rect(f, 6.0, 3.0, 12.0, 18.0, 6.0);
            f.stroke(&Path::line(p(12.0, 7.0), p(12.0, 11.0)), line);
        }
        Kind::Keyboard => {
            rect(f, 2.0, 6.0, 20.0, 12.0, 2.0);
            // `M6 10h.01`…: round caps make them dots.
            for x in [6.0, 10.0, 14.0, 18.0] {
                f.fill(&Path::circle(p(x, 10.0), s), colour);
            }
            f.stroke(&Path::line(p(7.0, 14.0), p(17.0, 14.0)), line);
        }
        Kind::Speaker => {
            rect(f, 5.0, 2.0, 14.0, 20.0, 3.0);
            f.stroke(&Path::circle(p(12.0, 14.0), 3.5 * s), line);
            f.fill(&Path::circle(p(12.0, 7.0), 2.0 * s), colour);
        }
        Kind::Phone => {
            rect(f, 7.0, 2.0, 10.0, 20.0, 2.0);
            f.stroke(&Path::line(p(11.0, 18.0), p(13.0, 18.0)), line);
        }
        Kind::Computer | Kind::Other => sign(f, at, size, colour, false, 1.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bluetooth::model::Device;

    fn connected(name: &str, battery: Option<u8>) -> Device {
        Device {
            path: format!("/org/bluez/hci0/dev_{name}"),
            name: name.to_owned(),
            kind: Kind::Headphones,
            paired: true,
            connected: true,
            battery,
            code: false,
        }
    }

    #[test]
    fn a_long_name_gives_way_to_the_chip_and_the_battery() {
        let words = Words::new(crate::clock::time::Lang::French);
        let mut bt = Bluetooth {
            adapter: true,
            powered: true,
            connected: vec![connected("Un casque au nom vraiment très long", Some(82))],
            ..Bluetooth::default()
        };
        let mut state = RowState::default();
        state.refresh(words, &bt, 184.0);
        let alone = state.label.clone();
        assert!(alone.ends_with('…'), "{alone}");
        assert!(state.battery.is_some() && state.chip.is_none());
        bt.connected.push(connected("MX", Some(12)));
        state.refresh(words, &bt, 184.0);
        assert!(state.label.chars().count() < alone.chars().count());
        assert_eq!(state.chip.as_ref().map(|c| c.0.as_str()), Some("+1"));
        // Off: the words.
        bt.powered = false;
        state.refresh(words, &bt, 184.0);
        assert_eq!(state.label, "Bluetooth coupé");
        assert!(state.battery.is_none() && state.chip.is_none());
    }
}
