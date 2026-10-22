//! Snapping: halves, quarters and the top for maximize, with gaps; zones
//! found at the edges of the work area; magnetic edges.

use std::str::FromStr;

use crate::{Rect, config::Gaps};

/// Where a window can snap in the work area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Zone {
    Left,
    Right,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// The whole work area: maximize.
    Top,
}

impl FromStr for Zone {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "left" => Self::Left,
            "right" => Self::Right,
            "top-left" => Self::TopLeft,
            "top-right" => Self::TopRight,
            "bottom-left" => Self::BottomLeft,
            "bottom-right" => Self::BottomRight,
            "top" => Self::Top,
            other => return Err(format!("unknown zone {other:?}")),
        })
    }
}

impl Zone {
    pub fn name(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::TopLeft => "top-left",
            Self::TopRight => "top-right",
            Self::BottomLeft => "bottom-left",
            Self::BottomRight => "bottom-right",
            Self::Top => "top",
        }
    }

    /// The window's rectangle in `area` (the work area, outer gap already
    /// out), with `inner` between the windows of neighbouring zones.
    pub fn rect(self, area: Rect, inner: u32) -> Rect {
        let gap = i32::try_from(inner).unwrap_or(0);
        // Each half gives up half the gap; an odd gap rounds towards the left.
        let left_w = (area.width - gap) / 2;
        let right_w = area.width - gap - left_w;
        let top_h = (area.height - gap) / 2;
        let bottom_h = area.height - gap - top_h;
        let (x_left, x_right) = (area.x, area.x + left_w + gap);
        let (y_top, y_bottom) = (area.y, area.y + top_h + gap);
        let r = |x, y, width, height| Rect {
            x,
            y,
            width,
            height,
        };
        match self {
            Self::Top => area,
            Self::Left => r(x_left, area.y, left_w, area.height),
            Self::Right => r(x_right, area.y, right_w, area.height),
            Self::TopLeft => r(x_left, y_top, left_w, top_h),
            Self::TopRight => r(x_right, y_top, right_w, top_h),
            Self::BottomLeft => r(x_left, y_bottom, left_w, bottom_h),
            Self::BottomRight => r(x_right, y_bottom, right_w, bottom_h),
        }
    }

    /// The zone of a pointer dragging a window into the edge of `output`
    /// (the whole output, so that its very edge counts): within `edge`
    /// pixels of a side; corners make quarters.
    pub fn at(output: Rect, x: f64, y: f64, edge: f64) -> Option<Self> {
        let left = x < f64::from(output.x) + edge;
        let right = x >= f64::from(output.x + output.width) - edge;
        let top = y < f64::from(output.y) + edge;
        let bottom = y >= f64::from(output.y + output.height) - edge;
        // Corners take a wider band, so that they are easy to reach.
        let near_top = y < f64::from(output.y) + 4.0 * edge;
        let near_bottom = y >= f64::from(output.y + output.height) - 4.0 * edge;
        Some(match (left, right, top, bottom) {
            (true, _, _, _) if near_top => Self::TopLeft,
            (true, _, _, _) if near_bottom => Self::BottomLeft,
            (true, _, _, _) => Self::Left,
            (_, true, _, _) if near_top => Self::TopRight,
            (_, true, _, _) if near_bottom => Self::BottomRight,
            (_, true, _, _) => Self::Right,
            (_, _, true, _) => Self::Top,
            _ => return None,
        })
    }
}

/// `window` moved so that its edges stick to the work area's edges and to
/// the facing edges of `others` (with `gaps`) when within `distance`.
pub fn magnet(window: Rect, area: Rect, others: &[Rect], gaps: Gaps, distance: i32) -> (i32, i32) {
    let inner = i32::try_from(gaps.inner).unwrap_or(0);
    // Candidate positions for the left edge, and for the top edge.
    let mut xs = vec![area.x, area.x + area.width - window.width];
    let mut ys = vec![area.y, area.y + area.height - window.height];
    for o in others {
        let overlaps_y = window.y < o.y + o.height && o.y < window.y + window.height;
        let overlaps_x = window.x < o.x + o.width && o.x < window.x + window.width;
        if overlaps_y {
            xs.push(o.x + o.width + inner);
            xs.push(o.x - inner - window.width);
        }
        if overlaps_x {
            ys.push(o.y + o.height + inner);
            ys.push(o.y - inner - window.height);
        }
    }
    let nearest = |at: i32, candidates: &[i32]| {
        candidates
            .iter()
            .copied()
            .filter(|c| (c - at).abs() <= distance)
            .min_by_key(|c| (c - at).abs())
            .unwrap_or(at)
    };
    (nearest(window.x, &xs), nearest(window.y, &ys))
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 1280,
        y: 0,
        width: 1920,
        height: 1080,
    };

    #[test]
    fn halves_and_quarters_share_the_inner_gap() {
        assert_eq!(
            Zone::Left.rect(AREA, 8),
            Rect {
                x: 1280,
                y: 0,
                width: 956,
                height: 1080
            }
        );
        assert_eq!(
            Zone::Right.rect(AREA, 8),
            Rect {
                x: 2244,
                y: 0,
                width: 956,
                height: 1080
            }
        );
        assert_eq!(
            Zone::BottomRight.rect(AREA, 8),
            Rect {
                x: 2244,
                y: 544,
                width: 956,
                height: 536
            }
        );
        assert_eq!(Zone::Top.rect(AREA, 8), AREA);
    }

    #[test]
    fn edges_make_zones_and_corners_quarters() {
        assert_eq!(Zone::at(AREA, 1282.0, 500.0, 8.0), Some(Zone::Left));
        assert_eq!(Zone::at(AREA, 1282.0, 10.0, 8.0), Some(Zone::TopLeft));
        assert_eq!(Zone::at(AREA, 3199.0, 1075.0, 8.0), Some(Zone::BottomRight));
        assert_eq!(Zone::at(AREA, 2000.0, 2.0, 8.0), Some(Zone::Top));
        assert_eq!(Zone::at(AREA, 2000.0, 500.0, 8.0), None);
    }

    #[test]
    fn windows_stick_to_edges_and_to_each_other() {
        let window = Rect {
            x: 1290,
            y: 300,
            width: 400,
            height: 300,
        };
        // Near the left edge: stuck to it.
        assert_eq!(magnet(window, AREA, &[], Gaps::default(), 12), (1280, 300));
        // Near another window's right edge, with the inner gap.
        let other = Rect {
            x: 1400,
            y: 250,
            width: 500,
            height: 400,
        };
        let near = Rect { x: 1915, ..window };
        let gaps = Gaps { inner: 8, outer: 0 };
        assert_eq!(magnet(near, AREA, &[other], gaps, 12), (1908, 300));
        // Too far: left where it is.
        let far = Rect { x: 2000, ..window };
        assert_eq!(magnet(far, AREA, &[other], gaps, 12), (2000, 300));
    }
}
