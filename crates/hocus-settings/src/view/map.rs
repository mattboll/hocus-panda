//! Language and region's world map (#67): the land, the time zones'
//! cities as dots, the system's own ringed in the accent; a click near a
//! city picks its zone. An equirectangular map from 84° N to 60° S.
//!
//! The land is Natural Earth's 1:110m land outline (public domain,
//! <https://www.naturalearthdata.com>), simplified to half degrees by
//! `scripts/land-outline.py` into `data/land.txt`. The cities are where
//! the system's time zone database (`zone1970.tab`, public domain) places
//! them, read when the settings start.

use std::{sync::OnceLock, time::Duration};

use hocus_platform::timedate::Place;
use hocus_ui::{Canvas, Draw, Point, Rect, Shape, Size, Theme};

/// One ring per line: longitude and latitude pairs, in half degrees.
const LAND: &str = include_str!("../../data/land.txt");
const NORTH: f32 = 84.0;
const SOUTH: f32 = -60.0;
/// The box the land is drawn in, stretched to the map.
const VIEW: f32 = 720.0;
/// Width over height.
pub(super) const RATIO: f32 = 360.0 / (NORTH - SOUTH);
/// How near a city a click has to be to pick it, logical pixels.
const NEAR: f32 = 18.0;

/// The land, once.
fn land() -> Option<&'static Shape> {
    static SHAPE: OnceLock<Option<Shape>> = OnceLock::new();
    SHAPE
        .get_or_init(|| {
            let mut d = String::new();
            for ring in LAND.lines() {
                let numbers: Vec<f32> = ring
                    .split_whitespace()
                    .filter_map(|n| n.parse::<i16>().ok().map(|n| f32::from(n) / 2.0))
                    .collect();
                for (i, [longitude, latitude]) in numbers.as_chunks::<2>().0.iter().enumerate() {
                    let x = (longitude + 180.0) / 360.0 * VIEW;
                    let y = (NORTH - latitude) / (NORTH - SOUTH) * VIEW;
                    d.push_str(&format!("{}{x:.2} {y:.2}", if i == 0 { 'M' } else { 'L' }));
                }
                d.push('Z');
            }
            Shape::svg(&d)
        })
        .as_ref()
}

/// Where a place shows on a map drawn in `rect`.
pub(super) fn project(rect: Rect, latitude: f32, longitude: f32) -> Point {
    let latitude = latitude.clamp(SOUTH, NORTH);
    Point::new(
        rect.x + (longitude + 180.0) / 360.0 * rect.w,
        rect.y + (NORTH - latitude) / (NORTH - SOUTH) * rect.h,
    )
}

/// The city nearest to `at` on a map drawn in `rect`, when near enough.
pub(super) fn nearest(places: &[Place], rect: Rect, at: Point) -> Option<&Place> {
    places
        .iter()
        .map(|p| {
            let q = project(rect, p.latitude, p.longitude);
            (p, (q.x - at.x).hypot(q.y - at.y))
        })
        .filter(|(_, d)| *d <= NEAR)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(p, _)| p)
}

/// The map: land, cities, the zone in effect.
pub(super) struct WorldMap {
    pub(super) places: std::sync::Arc<Vec<Place>>,
    /// The system's zone, ringed when it is one of the places.
    pub(super) current: String,
}

impl Draw for WorldMap {
    fn draw(&self, canvas: &mut Canvas, rect: Rect, theme: &Theme, _now: Duration) {
        let c = &theme.colors;
        if let Some(land) = land() {
            canvas.fill_shape(land, VIEW, rect, c.muted.with_alpha(52));
        }
        let dot = c.muted.with_alpha(170);
        for place in self.places.iter() {
            let p = project(rect, place.latitude, place.longitude);
            canvas.fill_rect(Rect::new(p.x - 1.5, p.y - 1.5, 3.0, 3.0), 1.5, dot);
        }
        if let Some(place) = self.places.iter().find(|p| p.zone == self.current) {
            let p = project(rect, place.latitude, place.longitude);
            canvas.fill_rect(
                Rect::new(p.x - 8.0, p.y - 8.0, 16.0, 16.0),
                8.0,
                c.accent.with_alpha(70),
            );
            canvas.fill_rect(Rect::new(p.x - 4.5, p.y - 4.5, 9.0, 9.0), 4.5, c.accent);
        }
    }

    fn size(&self) -> Size {
        Size::new(0.0, 0.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn place(zone: &str, latitude: f32, longitude: f32) -> Place {
        Place {
            zone: zone.into(),
            latitude,
            longitude,
        }
    }

    #[test]
    fn the_land_reads() {
        assert!(land().is_some());
        assert!(LAND.lines().count() > 20);
    }

    #[test]
    fn a_click_picks_the_nearest_city_when_near() {
        let rect = Rect::new(0.0, 0.0, 720.0, 288.0);
        let places = [
            place("Europe/Paris", 48.9, 2.3),
            place("Europe/Brussels", 50.8, 4.3),
        ];
        let paris = project(rect, 48.9, 2.3);
        assert_eq!((paris.x.round(), paris.y.round()), (365.0, 70.0));
        let picked = nearest(&places, rect, Point::new(paris.x + 2.0, paris.y + 3.0));
        assert_eq!(picked.map(|p| p.zone.as_str()), Some("Europe/Paris"));
        assert!(nearest(&places, rect, Point::new(10.0, 10.0)).is_none());
    }
}
