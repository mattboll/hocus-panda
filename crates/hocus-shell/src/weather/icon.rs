//! The weather's icons (#814), as the approved mockup draws them: SVG in
//! a 24 px box (a cloud, a sun and its rays, drops, flakes, fog, a moon,
//! a bolt), drawn at any size by iced's SVG renderer, on a canvas
//! ([`draw`]) or as a widget ([`widget`]). Each is made once a theme and
//! kept.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use hocus_weather::Icon;
use iced::{Color, Element, Rectangle, advanced::svg, widget::canvas::Frame};

use crate::theme::Ink;

/// The cloud, and a smaller one behind it.
const CLOUD: &str = "M7 18h10.5a3.5 3.5 0 0 0 .4-6.98A5 5 0 0 0 8.2 9.6 4.2 4.2 0 0 0 7 18z";
const CLOUD_BACK: &str = "M9 17h8.5a3 3 0 0 0 .3-5.98A4.4 4.4 0 0 0 9.4 9.8 3.6 3.6 0 0 0 9 17z";

/// The mockup's colours, read in the theme of the moment.
const CLOUD_FILL: Ink = Ink::Dark(Color::from_rgba8(220, 228, 242, 0.16));
const CLOUD_LINE: Ink = Ink::Dark(Color::from_rgb8(0xC9, 0xD2, 0xE2));
const BACK_FILL: Ink = Ink::Dark(Color::from_rgba8(160, 170, 190, 0.22));
const BACK_LINE: Ink = Ink::Dark(Color::from_rgb8(0x9A, 0xA5, 0xB8));
/// The sun, its rays, the bolt.
pub const SUN: Color = Color::from_rgb8(0xFF, 0xE0, 0x8A);
/// The rain's colour, the bar's rain line's too.
pub const RAIN: Ink = Ink::Dark(Color::from_rgb8(0x8F, 0xB3, 0xFF));
const FLAKE: Ink = Ink::Dark(Color::from_rgb8(0xE8, 0xF0, 0xFF));
const FOG: Ink = Ink::Dark(Color::from_rgb8(0xAF, 0xB8, 0xC8));
const MOON: Ink = Ink::Dark(Color::from_rgb8(0xE9, 0xEE, 0xF8));

fn hex(c: Color) -> String {
    let [r, g, b, _] = c.into_rgba8();
    format!("#{r:02X}{g:02X}{b:02X}")
}

/// `fill="#…" fill-opacity="…"` for `c`.
fn fill(c: Color) -> String {
    format!("fill=\"{}\" fill-opacity=\"{:.2}\"", hex(c), c.a)
}

fn stroke(c: Color, width: f32) -> String {
    format!(
        "stroke=\"{}\" stroke-opacity=\"{:.2}\" stroke-width=\"{width}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"",
        hex(c),
        c.a
    )
}

/// Eight rays round (`cx`, `cy`), from `r1` to `r2`.
fn rays(cx: f32, cy: f32, r1: f32, r2: f32) -> String {
    let ray = stroke(SUN, 1.6);
    (0..8)
        .map(|i| {
            let a = i as f32 * std::f32::consts::FRAC_PI_4;
            format!(
                "<line {ray} x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\"/>",
                cx + a.cos() * r1,
                cy + a.sin() * r1,
                cx + a.cos() * r2,
                cy + a.sin() * r2
            )
        })
        .collect()
}

fn sun(cx: f32, cy: f32, r: f32) -> String {
    format!("<circle {} cx=\"{cx}\" cy=\"{cy}\" r=\"{r}\"/>", fill(SUN))
}

fn cloud(dx: f32, dy: f32) -> String {
    format!(
        "<path {} {} d=\"{CLOUD}\" transform=\"translate({dx} {dy})\"/>",
        fill(CLOUD_FILL.get()),
        stroke(CLOUD_LINE.get(), 1.5)
    )
}

fn drop(x1: f32, y1: f32) -> String {
    format!(
        "<line {} x1=\"{x1}\" y1=\"{y1}\" x2=\"{}\" y2=\"{}\"/>",
        stroke(RAIN.get(), 1.7),
        x1 - 1.0,
        y1 + 3.0
    )
}

/// `icon`'s drawing, in the theme of the moment.
pub fn svg_of(icon: Icon) -> String {
    let body = match icon {
        Icon::Sun => rays(12.0, 12.0, 6.8, 9.6) + &sun(12.0, 12.0, 4.6),
        Icon::PartlyCloudy => rays(9.0, 8.5, 5.3, 7.6) + &sun(9.0, 8.5, 3.6) + &cloud(1.5, 1.0),
        Icon::Cloudy => {
            format!(
                "<path {} {} d=\"{CLOUD_BACK}\" transform=\"translate(-4 -3)\"/>",
                fill(BACK_FILL.get()),
                stroke(BACK_LINE.get(), 1.5)
            ) + &cloud(0.0, 1.0)
        }
        Icon::Rain => cloud(0.0, -3.0) + &drop(9.0, 18.0) + &drop(13.0, 18.0) + &drop(17.0, 18.0),
        Icon::Showers => {
            rays(8.0, 7.0, 4.6, 6.6)
                + &sun(8.0, 7.0, 3.0)
                + &cloud(1.0, -2.0)
                + &drop(11.0, 19.0)
                + &drop(15.0, 19.0)
        }
        Icon::Snow => {
            let flake = |x: f32, y: f32| {
                format!(
                    "<circle {} cx=\"{x}\" cy=\"{y}\" r=\"1.2\"/>",
                    fill(FLAKE.get())
                )
            };
            cloud(0.0, -3.0) + &flake(9.0, 19.5) + &flake(13.0, 21.0) + &flake(17.0, 19.5)
        }
        Icon::Fog => {
            let line = stroke(FOG.get(), 1.6);
            format!(
                "<path {} {} d=\"{CLOUD}\" transform=\"translate(0 -4)\"/>\
                 <line {line} x1=\"5\" y1=\"17\" x2=\"19\" y2=\"17\"/>\
                 <line {line} x1=\"7\" y1=\"20.5\" x2=\"17\" y2=\"20.5\"/>",
                fill(BACK_FILL.get()),
                stroke(BACK_LINE.get(), 1.5)
            )
        }
        Icon::Thunder => {
            cloud(0.0, -3.0)
                + &format!(
                    "<path {} d=\"M12.6 15.2 L9.8 19.6 H12.2 L11.2 23 L14.6 18.2 H12.2 Z\"/>",
                    fill(SUN)
                )
        }
        Icon::Moon => {
            let mut moon = MOON.get();
            moon.a = 0.9;
            format!(
                "<path {} d=\"M15.5 4.5a7.5 7.5 0 1 0 4 12.6A6 6 0 0 1 15.5 4.5z\"/>",
                fill(moon)
            )
        }
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" viewBox=\"0 0 24 24\" overflow=\"visible\">{body}</svg>"
    )
}

/// The icons made so far, by icon and theme.
static MADE: Mutex<Option<HashMap<(Icon, bool), svg::Handle>>> = Mutex::new(None);

/// `icon`'s drawing as a handle, made once a theme.
pub fn handle(icon: Icon) -> svg::Handle {
    let light = crate::theme::light();
    let mut made = MADE.lock().unwrap_or_else(PoisonError::into_inner);
    made.get_or_insert_with(HashMap::new)
        .entry((icon, light))
        .or_insert_with(|| svg::Handle::from_memory(svg_of(icon).into_bytes()))
        .clone()
}

/// `icon` drawn in `rect` on a canvas, at `alpha`.
pub fn draw(f: &mut Frame, icon: Icon, rect: Rectangle, alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    f.draw_svg(rect, svg::Svg::new(handle(icon)).opacity(alpha));
}

/// `icon` as a widget, `size` px square.
pub fn widget<'a, M: 'a>(icon: Icon, size: f32) -> Element<'a, M> {
    iced::widget::svg(handle(icon))
        .width(size)
        .height(size)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_icon_is_an_svg_of_the_mockups_box() {
        for icon in [
            Icon::Sun,
            Icon::PartlyCloudy,
            Icon::Cloudy,
            Icon::Rain,
            Icon::Showers,
            Icon::Snow,
            Icon::Fog,
            Icon::Thunder,
            Icon::Moon,
        ] {
            let svg = svg_of(icon);
            assert!(
                svg.starts_with("<svg") && svg.ends_with("</svg>"),
                "{icon:?}"
            );
            assert!(svg.contains("viewBox=\"0 0 24 24\""), "{icon:?}");
            assert!(svg.matches('<').count() > 2, "{icon:?}");
        }
        // Rain's drops in the rain's colour, the sun's rays eight.
        assert!(svg_of(Icon::Rain).contains("#8FB3FF"));
        assert_eq!(svg_of(Icon::Sun).matches("<line").count(), 8);
    }
}
