//! Outputs described by hand, for the headless backend:
//! `WxH[@SCALE][+X+Y][/DESKTOPS][:WMMxHMMmm]`.

use std::{fmt, str::FromStr};

use hocus_core::outputs::default_scale;

/// An output's pixel size, scale and logical position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSpec {
    pub width: i32,
    pub height: i32,
    /// What it is lit at: as given, else the one its density gives (as a
    /// screen's, see [`hocus_core::outputs::default_scale`]), else 1.
    pub scale: f64,
    /// Millimetres, as a screen's EDID gives them; `None` as a VM's.
    pub physical_size: Option<(i32, i32)>,
    /// Logical position; `None` puts it to the right of the previous output.
    pub position: Option<(i32, i32)>,
    /// How many desktops it has when first plugged.
    pub desktops: usize,
}

/// Desktops of an output, until the configuration says otherwise.
pub const DEFAULT_DESKTOPS: usize = 4;
/// The most desktops an output can have.
pub const MAX_DESKTOPS: usize = 8;

impl OutputSpec {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            physical_size: None,
            position: None,
            desktops: DEFAULT_DESKTOPS,
        }
    }

    pub fn desktops(self, desktops: usize) -> Self {
        Self { desktops, ..self }
    }

    pub fn scale(self, scale: f64) -> Self {
        Self { scale, ..self }
    }

    /// A size in millimetres, and the scale its density gives; a scale
    /// set after it is kept.
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

    /// Width in logical pixels, which is what the next output is placed after.
    pub fn logical_width(&self) -> i32 {
        (f64::from(self.width) / self.scale).round() as i32
    }
}

impl Default for OutputSpec {
    fn default() -> Self {
        Self::new(1920, 1080)
    }
}

/// Logical positions of `specs`, each one without a position being placed
/// to the right of the previous one.
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
            "{}: expected WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS][:WIDTHxHEIGHTmm], e.g. \
             2560x1600@2, 1920x1080+0+0/1 or 2560x1600:340x210mm",
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
        let (s, desktops) = match s.split_once('/') {
            Some((rest, n)) => (rest, n.parse().map_err(|_| err())?),
            None => (s, DEFAULT_DESKTOPS),
        };
        if !(1..=MAX_DESKTOPS).contains(&desktops) {
            return Err(err());
        }
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
            desktops,
        })
    }
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
        assert_eq!(
            "1280x800/1".parse(),
            Ok(OutputSpec::new(1280, 800).desktops(1))
        );
    }

    #[test]
    fn a_physical_size_gives_the_scale_unless_one_is_given() {
        let laptop: OutputSpec = "2560x1600+0+0/1:340x210mm"
            .parse()
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(laptop.physical_size, Some((340, 210)));
        assert_eq!(laptop.scale, 1.75);
        assert_eq!(laptop.position, Some((0, 0)));
        assert_eq!(laptop.desktops, 1);
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
        assert_eq!(
            "1920x1080:530x300mm".parse::<OutputSpec>().map(|s| s.scale),
            Ok(1.0)
        );
        for bad in ["10x10:", "10x10:5x5", "10x10:0x5mm", "10x10:axbmm"] {
            assert!(bad.parse::<OutputSpec>().is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn rejects_malformed_specs() {
        for bad in [
            "", "1920", "1920x", "x1080", "0x10", "10x10@0", "10x10+5", "axb", "10x10/0", "10x10/9",
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
}
