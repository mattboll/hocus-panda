//! Outputs described by hand, for the headless backend: `WxH[@SCALE][+X+Y]`.

use std::{fmt, str::FromStr};

/// An output's pixel size, scale and logical position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputSpec {
    pub width: i32,
    pub height: i32,
    pub scale: f64,
    /// Logical position; `None` puts it to the right of the previous output.
    pub position: Option<(i32, i32)>,
}

impl OutputSpec {
    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width,
            height,
            scale: 1.0,
            position: None,
        }
    }

    pub fn scale(self, scale: f64) -> Self {
        Self { scale, ..self }
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
            "{}: expected WIDTHxHEIGHT[@SCALE][+X+Y], e.g. 2560x1600@2 or 1920x1080+0+0",
            self.0
        )
    }
}

impl std::error::Error for ParseOutputSpecError {}

impl FromStr for OutputSpec {
    type Err = ParseOutputSpecError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseOutputSpecError(s.to_string());
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
            Some((size, scale)) => (size, scale.parse().map_err(|_| err())?),
            None => (rest, 1.0),
        };
        let (width, height) = size.split_once('x').ok_or_else(err)?;
        let width: i32 = width.parse().map_err(|_| err())?;
        let height: i32 = height.parse().map_err(|_| err())?;
        if width <= 0 || height <= 0 || !(0.25..=8.0).contains(&scale) {
            return Err(err());
        }
        Ok(Self {
            width,
            height,
            scale,
            position,
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
}
