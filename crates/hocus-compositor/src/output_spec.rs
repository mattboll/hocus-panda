//! Outputs described by hand, for the headless backend:
//! `WxH[@SCALE][+X+Y][:WMMxHMMmm]`; and the scale a screen takes from its
//! density when nothing sets one.

use std::{fmt, str::FromStr};

/// An output's pixel size, scale and logical position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSpec {
    pub width: i32,
    pub height: i32,
    /// As given, else the one its density gives (as a screen's, see
    /// [`default_scale`]), else 1.
    pub scale: f64,
    /// Millimetres, as a screen's EDID gives them; `None` as a VM's.
    pub physical_size: Option<(i32, i32)>,
    /// Logical position; `None` puts it to the right of the previous output.
    pub position: Option<(i32, i32)>,
}

impl OutputSpec {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            physical_size: None,
            position: None,
        }
    }

    pub fn scale(self, scale: f64) -> Self {
        Self { scale, ..self }
    }

    /// A size in millimetres, and the scale its density gives; a scale set
    /// after it is kept.
    pub fn physical_size(self, width_mm: i32, height_mm: i32) -> Self {
        Self {
            physical_size: Some((width_mm, height_mm)),
            scale: default_scale((self.width, self.height), (width_mm, height_mm)),
            ..self
        }
    }

    pub fn at(self, x: i32, y: i32) -> Self {
        Self {
            position: Some((x, y)),
            ..self
        }
    }

    /// Width in logical pixels, which is what the next output is placed
    /// after.
    pub fn logical_width(&self) -> i32 {
        (f64::from(self.width) / self.scale).round() as i32
    }
}

impl Default for OutputSpec {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

/// Logical positions of `specs`, each one without a position placed to the
/// right of the previous one.
pub fn layout(specs: &[OutputSpec]) -> Vec<(i32, i32)> {
    let mut next = (0, 0);
    specs
        .iter()
        .map(|spec| {
            let pos = spec.position.unwrap_or(next);
            next = (pos.0 + spec.logical_width(), pos.1);
            pos
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOutputSpecError(String);

impl fmt::Display for ParseOutputSpecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: expected WIDTHxHEIGHT[@SCALE][+X+Y][:WIDTHxHEIGHTmm], e.g. 2560x1600@2, \
             1920x1080+0+0 or 2560x1600:340x210mm",
            self.0
        )
    }
}

impl std::error::Error for ParseOutputSpecError {}

impl FromStr for OutputSpec {
    type Err = ParseOutputSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseOutputSpecError(s.to_string());
        let (s, physical_size) = match s.split_once(':') {
            Some((rest, mm)) => {
                let (w, h) = mm
                    .strip_suffix("mm")
                    .ok_or_else(err)?
                    .split_once('x')
                    .ok_or_else(err)?;
                let w: i32 = w.parse().map_err(|_| err())?;
                let h: i32 = h.parse().map_err(|_| err())?;
                if w <= 0 || h <= 0 {
                    return Err(err());
                }
                (rest, Some((w, h)))
            }
            None => (s, None),
        };
        let (rest, position) = match s.split_once('+') {
            Some((rest, pos)) => {
                let (x, y) = pos.split_once('+').ok_or_else(err)?;
                let x = x.parse().map_err(|_| err())?;
                let y = y.parse().map_err(|_| err())?;
                (rest, Some((x, y)))
            }
            None => (s, None),
        };
        let (size, scale) = match rest.split_once('@') {
            Some((size, scale)) => (size, Some(scale.parse().map_err(|_| err())?)),
            None => (rest, None),
        };
        let (width, height) = size.split_once('x').ok_or_else(err)?;
        let width: i32 = width.parse().map_err(|_| err())?;
        let height: i32 = height.parse().map_err(|_| err())?;
        let scale = scale
            .unwrap_or_else(|| physical_size.map_or(1.0, |mm| default_scale((width, height), mm)));
        if width <= 0 || height <= 0 || !(0.25..=8.0).contains(&scale) {
            return Err(err());
        }
        Ok(Self {
            width,
            height,
            scale,
            physical_size,
            position,
        })
    }
}

/// Pixels per inch for each unit of a screen's default scale. Scale 1 is
/// drawn for about 96 dpi, but the screens of 92 to 110 dpi most desks have
/// (a 24-inch 1920x1080, a 27-inch 2560x1440) read well at 1; a scale is
/// taken once a screen is clearly denser than those.
const DPI_PER_SCALE: f64 = 112.0;
/// The smallest diagonal believed, in millimetres (about 8 inches): below
/// it, a TV or projector gave its aspect ratio or a placeholder, not its
/// size.
const SMALLEST_DIAGONAL_MM: f64 = 200.0;
/// The densest screen believed, in dpi.
const DENSEST: f64 = 600.0;
/// How far a screen's shape in millimetres may differ from its shape in
/// pixels (EDID rounds sizes to centimetres) before its size is not
/// believed.
const SHAPE_TOLERANCE: f64 = 1.15;

/// How dense a screen of `pixels` and `millimetres` (from EDID) is, in dots
/// per inch along its diagonal; `None` when it gave no size, or one that
/// cannot be its own: under 8 inches across, denser than any screen, or of
/// another shape than its pixels (projectors, some TVs).
fn density(pixels: (i32, i32), millimetres: (i32, i32)) -> Option<f64> {
    let (w, h) = (f64::from(pixels.0), f64::from(pixels.1));
    let (w_mm, h_mm) = (f64::from(millimetres.0), f64::from(millimetres.1));
    if w <= 0.0 || h <= 0.0 || w_mm <= 0.0 || h_mm <= 0.0 {
        return None;
    }
    let diagonal_mm = w_mm.hypot(h_mm);
    if diagonal_mm < SMALLEST_DIAGONAL_MM {
        return None;
    }
    // Some panels give their size turned a quarter from their mode: only
    // the shape counts, not which way it stands.
    let shape = |a: f64, b: f64| a.max(b) / a.min(b);
    let mismatch = shape(w, h) / shape(w_mm, h_mm);
    if !(1.0 / SHAPE_TOLERANCE..=SHAPE_TOLERANCE).contains(&mismatch) {
        return None;
    }
    let dpi = w.hypot(h) / (diagonal_mm / 25.4);
    (dpi <= DENSEST).then_some(dpi)
}

/// The scale a screen takes when nothing sets one: a unit per 112 dpi of
/// its density, to the nearest quarter, from 1 to 3; 1 when its size is
/// unknown or not believed.
pub fn default_scale(pixels: (i32, i32), millimetres: (i32, i32)) -> f64 {
    density(pixels, millimetres).map_or(1.0, |dpi| {
        ((dpi / DPI_PER_SCALE * 4.0).round() / 4.0).clamp(1.0, 3.0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_size_scale_and_position() {
        assert_eq!("1920x1080".parse(), Ok(OutputSpec::new(1920, 1080)));
        assert_eq!(
            "2560x1600@2".parse(),
            Ok(OutputSpec::new(2560, 1600).scale(2.0))
        );
        assert_eq!(
            "3440x1440@1.25+1280+0".parse(),
            Ok(OutputSpec::new(3440, 1440).scale(1.25).at(1280, 0))
        );
    }

    #[test]
    fn a_physical_size_gives_the_scale_unless_one_is_given() {
        assert_eq!(
            "2560x1600:340x210mm".parse(),
            Ok(OutputSpec::new(2560, 1600).physical_size(340, 210))
        );
        assert_eq!(
            "2560x1600@1:340x210mm"
                .parse::<OutputSpec>()
                .map(|s| s.scale),
            Ok(1.0)
        );
        for bad in ["10x10:", "10x10:5x5", "10x10:0x5mm", "10x10:axbmm"] {
            assert!(bad.parse::<OutputSpec>().is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn rejects_malformed_specs() {
        for bad in [
            "", "1920", "1920x", "x1080", "0x10", "10x10@0", "10x10+5", "axb",
        ] {
            assert!(bad.parse::<OutputSpec>().is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn outputs_without_position_follow_the_previous_one() {
        let specs = [
            OutputSpec::new(2560, 1600).scale(2.0),
            OutputSpec::new(3440, 1440),
            OutputSpec::new(800, 600).at(0, 2000),
            OutputSpec::new(800, 600),
        ];
        assert_eq!(
            layout(&specs),
            vec![(0, 0), (1280, 0), (0, 2000), (800, 2000)]
        );
    }

    #[test]
    fn a_screens_default_scale_follows_its_density() {
        let cases = [
            // A 16-inch laptop, about 192 dpi.
            ((2560, 1600), (340, 210), 1.75),
            // A 24-inch 1920x1080, about 92 dpi.
            ((1920, 1080), (530, 300), 1.0),
            // 27-inch screens: 2560x1440 (109 dpi), 4K (163), 5K (218).
            ((2560, 1440), (597, 336), 1.0),
            ((3840, 2160), (597, 336), 1.5),
            ((5120, 2880), (597, 336), 2.0),
            // A 14-inch 2880x1800, about 243 dpi.
            ((2880, 1800), (301, 188), 2.25),
            // A 9-inch panel of about 500 dpi: no more than 3.
            ((3840, 2400), (196, 123), 3.0),
            // No size, or a projector's aspect ratio for one.
            ((1920, 1080), (0, 0), 1.0),
            ((1920, 1080), (16, 9), 1.0),
        ];
        for (pixels, mm, want) in cases {
            assert_eq!(default_scale(pixels, mm), want, "{pixels:?} on {mm:?} mm");
        }
    }
}
