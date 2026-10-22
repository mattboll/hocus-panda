//! Master and stack: the first window on the left, the others sharing a
//! column on the right.

use crate::Rect;

/// The master's share of the width, and its limits.
pub const DEFAULT_RATIO: f64 = 0.55;
pub const MIN_RATIO: f64 = 0.2;
pub const MAX_RATIO: f64 = 0.8;

/// The rectangles of `count` windows in `area`, master first; `inner` is the
/// gap between them.
pub fn layout(area: Rect, count: usize, ratio: f64, inner: u32) -> Vec<Rect> {
    let gap = i32::try_from(inner).unwrap_or(0);
    match count {
        0 => Vec::new(),
        1 => vec![area],
        n => {
            let master_w =
                ((f64::from(area.width - gap)) * ratio.clamp(MIN_RATIO, MAX_RATIO)).round() as i32;
            let stack_x = area.x + master_w + gap;
            let stack_w = area.width - master_w - gap;
            let rows = (n - 1) as i32;
            let row_h = (area.height - gap * (rows - 1)) / rows;
            let mut rects = vec![Rect {
                x: area.x,
                y: area.y,
                width: master_w,
                height: area.height,
            }];
            for i in 0..rows {
                let y = area.y + i * (row_h + gap);
                // The last row takes what rounding left.
                let height = if i == rows - 1 {
                    area.y + area.height - y
                } else {
                    row_h
                };
                rects.push(Rect {
                    x: stack_x,
                    y,
                    width: stack_w,
                    height,
                });
            }
            rects
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect {
        x: 0,
        y: 0,
        width: 1280,
        height: 800,
    };

    #[test]
    fn one_window_fills_the_area() {
        assert_eq!(layout(AREA, 1, DEFAULT_RATIO, 8), vec![AREA]);
    }

    #[test]
    fn the_master_is_left_and_the_others_share_the_right_column() {
        let rects = layout(AREA, 3, 0.5, 10);
        assert_eq!(
            rects[0],
            Rect {
                x: 0,
                y: 0,
                width: 635,
                height: 800
            }
        );
        assert_eq!(
            rects[1],
            Rect {
                x: 645,
                y: 0,
                width: 635,
                height: 395
            }
        );
        assert_eq!(
            rects[2],
            Rect {
                x: 645,
                y: 405,
                width: 635,
                height: 395
            }
        );
    }

    #[test]
    fn the_ratio_stays_within_limits() {
        let rects = layout(AREA, 2, 5.0, 0);
        assert_eq!(rects[0].width, (1280.0 * MAX_RATIO) as i32);
    }
}
