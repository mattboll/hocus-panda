//! The pandas as creatures on their agents' windows (#796,
//! docs/design/effects.md): the same drawings, moving on and on as the
//! board "Les agents en créatures" moves them, not in bursts. Working
//! types, its laptop's star pulsing, bobbing slightly; waiting sways its
//! held-out hat, its "?" hops and its rings go out; resting breathes as
//! two "z" rise; done hops, squashing as it lands, waving its hat among
//! its confetti. Stuck (not on the board) puffs its smoke as the pandas
//! canvas does. Every hat's star turns and pulses.
//!
//! The board's motion is a function of time, ported as it is: [`look`]
//! gives the pose of each part it moves (round the part's box's middle,
//! CSS's `transform-box: fill-box`; a held-out hat round its top, a
//! sleeping body round its bottom) and of the whole creature (round its
//! box's bottom middle, in logical pixels as the board's CSS pixels).

use crate::{Mood, Pose};

/// A creature at a moment.
#[derive(Debug, Clone, PartialEq)]
pub struct Look {
    /// The whole creature: its lengths in logical pixels, round its box's
    /// bottom middle.
    pub whole: Pose,
    /// Its moving parts, in its drawing's units.
    pub parts: Vec<Moved>,
}

/// A moving part's pose, and the point it turns and scales round, in its
/// group's own space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Moved {
    pub id: &'static str,
    pub pose: Pose,
    pub origin: (f32, f32),
}

/// The hat's star, in the hat's space: its box's middle.
const SPARK: (f32, f32) = (14.0, -12.6);
/// The held-out hat's box's top middle (waiting), the waved hat's (done).
const HELD_HAT: (f32, f32) = (60.0, 85.62);
const WAVED_HAT: (f32, f32) = (26.74, 1.71);
/// The sleeping body's box's bottom middle.
const BODY: (f32, f32) = (67.75, 115.0);
/// The confetti's boxes' middles.
const CONFETTI: [(&str, (f32, f32)); 5] = [
    ("c1", (42.0, 10.0)),
    ("c2", (108.0, 55.5)),
    ("c3", (72.0, 6.0)),
    ("c4", (112.0, 20.0)),
    ("c5", (10.0, 62.0)),
];

/// The groups of `mood`'s drawing a creature moves.
pub fn parts(mood: Mood) -> &'static [&'static str] {
    match mood {
        Mood::Working => &["spark", "lid", "paw-l", "paw-r"],
        Mood::Waiting => &["spark", "swing", "ask", "ring1", "ring2"],
        Mood::Resting => &["spark", "body", "z1", "z2"],
        Mood::Done => &[
            "spark", "swing", "c1", "c2", "c3", "c4", "c5", "ring1", "ring2",
        ],
        Mood::Stuck => &["spark", "p1", "p2", "p3"],
        Mood::Wink => &["spark"],
    }
}

/// `mood`'s creature `t` seconds into its motion.
pub fn look(mood: Mood, t: f32) -> Look {
    use std::f32::consts::PI;
    let s = t;
    let moved = |id, pose, origin| Moved { id, pose, origin };
    let opacity = |opacity| Pose {
        opacity,
        ..Pose::REST
    };
    let lift = |dy| Pose { dy, ..Pose::REST };
    let turn = |turn| Pose { turn, ..Pose::REST };
    let pulse = 1.0 + 0.25 * (s * 2.2).sin().max(0.0);
    let mut parts = vec![moved(
        "spark",
        Pose {
            turn: (s * 40.0).rem_euclid(360.0),
            sx: pulse,
            sy: pulse,
            ..Pose::REST
        },
        SPARK,
    )];
    let mut whole = Pose::REST;
    // The rings going out, one after the other.
    let rings = |parts: &mut Vec<Moved>| {
        let k = (s * 0.9).rem_euclid(1.0);
        parts.push(moved("ring1", opacity(1.0 - k), (0.0, 0.0)));
        parts.push(moved(
            "ring2",
            opacity((1.0 - (k + 0.5).rem_euclid(1.0)) * 0.7),
            (0.0, 0.0),
        ));
    };
    match mood {
        Mood::Working => {
            let l = (s * 15.0).sin().max(0.0) * 3.4;
            let r = (s * 15.0 + 2.2).sin().max(0.0) * 3.4;
            parts.push(moved("paw-l", lift(-l), (42.0, 100.0)));
            parts.push(moved("paw-r", lift(-r), (78.0, 100.0)));
            parts.push(moved(
                "lid",
                opacity(0.55 + 0.45 * (s * 3.0).sin().abs()),
                (60.0, 88.5),
            ));
            whole.dy = (s * 7.5).sin() * 0.8;
        }
        Mood::Waiting => {
            parts.push(moved("swing", turn((s * 3.2).sin() * 9.0), HELD_HAT));
            parts.push(moved(
                "ask",
                lift(-(s * 2.6).sin().abs() * 4.0),
                (97.5, 10.65),
            ));
            rings(&mut parts);
        }
        Mood::Resting => {
            parts.push(moved(
                "body",
                Pose {
                    sy: 1.0 + (s * 1.5).sin() * 0.025,
                    ..Pose::REST
                },
                BODY,
            ));
            for (id, from) in [("z1", 0.0), ("z2", 0.5)] {
                let z = (s * 0.35 + from).rem_euclid(1.0);
                parts.push(moved(
                    id,
                    Pose {
                        dx: z * 6.0,
                        dy: -z * 14.0,
                        opacity: (z * PI).sin(),
                        ..Pose::REST
                    },
                    (0.0, 0.0),
                ));
            }
        }
        Mood::Done => {
            let c = s.rem_euclid(1.3);
            let up = if c < 0.75 { (c / 0.75 * PI).sin() } else { 0.0 };
            let land = if c >= 0.75 {
                ((c - 0.75) / 0.55 * PI).sin()
            } else {
                0.0
            };
            whole = Pose {
                dy: -up * 34.0,
                sx: 1.0 + land * 0.12,
                sy: 1.0 - land * 0.14 + up * 0.05,
                ..Pose::REST
            };
            parts.push(moved("swing", turn(-up * 22.0), WAVED_HAT));
            for (i, (id, origin)) in CONFETTI.into_iter().enumerate() {
                let k = (s * 0.8 + (i + 1) as f32 * 0.2).rem_euclid(1.0);
                let scale = 0.6 + k * 0.7;
                parts.push(moved(
                    id,
                    Pose {
                        dy: -k * 10.0,
                        sx: scale,
                        sy: scale,
                        opacity: (k * PI).sin(),
                        ..Pose::REST
                    },
                    origin,
                ));
            }
            rings(&mut parts);
        }
        Mood::Stuck => {
            for part in Mood::Stuck.parts().iter().filter(|p| p.id != "spark") {
                parts.push(moved(part.id, part.looping(s), part.origin));
            }
        }
        Mood::Wink => {}
    }
    Look { whole, parts }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_mood_moves_the_parts_it_names() {
        for mood in Mood::ALL {
            let look = look(mood, 0.7);
            let mut ids: Vec<&str> = look.parts.iter().map(|p| p.id).collect();
            let mut named = parts(mood).to_vec();
            ids.sort_unstable();
            named.sort_unstable();
            assert_eq!(ids, named, "{mood:?}");
            for id in ids {
                assert!(
                    mood.still().contains(&format!("<g id=\"{id}\">")),
                    "{mood:?} has no {id}"
                );
            }
        }
    }

    #[test]
    fn done_hops_and_lands() {
        // Up at the top of its hop, squashed as it lands.
        let top = look(Mood::Done, 0.375).whole;
        assert!((top.dy + 34.0).abs() < 0.01, "{top:?}");
        let landing = look(Mood::Done, 0.75 + 0.275).whole;
        assert!(landing.dy.abs() < 1e-6 && landing.sx > 1.1 && landing.sy < 0.87);
    }

    /// The origins are the boxes' as the board's CSS takes them
    /// (`transform-box: fill-box`).
    #[test]
    fn the_origins_are_the_parts_boxes() {
        use resvg::usvg;
        let place = |mood: Mood, id: &str, (fx, fy): (f32, f32)| {
            let tree = usvg::Tree::from_str(mood.still(), &usvg::Options::default())
                .expect("the drawing reads");
            let Some(usvg::Node::Group(g)) = tree.node_by_id(id) else {
                panic!("{mood:?} has no {id}");
            };
            let b = g.bounding_box();
            (b.x() + b.width() * fx, b.y() + b.height() * fy)
        };
        let near =
            |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).abs() < 0.05 && (a.1 - b.1).abs() < 0.05;
        assert!(near(place(Mood::Working, "spark", (0.5, 0.5)), SPARK));
        assert!(near(place(Mood::Waiting, "swing", (0.5, 0.0)), HELD_HAT));
        assert!(near(place(Mood::Done, "swing", (0.5, 0.0)), WAVED_HAT));
        assert!(near(place(Mood::Resting, "body", (0.5, 1.0)), BODY));
        for (id, origin) in CONFETTI {
            assert!(near(place(Mood::Done, id, (0.5, 0.5)), origin), "{id}");
        }
    }
}
