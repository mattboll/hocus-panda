//! The secretary in her drawer (docs/design/secretary.md §5.6): her
//! validated drawing (`assets/secretary-calm.svg`) split into parts, so
//! that her head, eyes, ear, right paw and clipboard can move, and the
//! marks her moods add over it: the ear's rings, a "?", sparkles, closed
//! lids and two "z", a drop.
//!
//! Each mood is a still drawing (her ear, and the dot on her clipboard, in
//! the mood's colour; its marks shown) and a motion played over it, the
//! approved mockup's keyframes on the pandas' own machinery: whole cycles
//! within the burst's window, then every part settles in
//! [`SETTLE`](super::SETTLE). Her
//! eyes blink in every mood but sleep, and her ear twitches while she
//! listens; the blink and the twitch come at the start of their cycles, so
//! that a burst shorter than one still shows them.
//!
//! The parts are groups (`<g id="sx-…">`) that change nothing drawn: a
//! mood without marks (listen, type) is `secretary-calm.svg` exactly.

use std::{sync::OnceLock, time::Duration};

use super::{Anim, Ease, Part, part};

/// Her moods in the drawer (secretary.md §5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretaryMood {
    /// The drawer open, idle: a blink, an ear twitch.
    Listen,
    /// Composing: her head nods, her right paw scribbles, the clipboard's
    /// line grows.
    Type,
    /// A question to the user: her head tilts, a "?" bobs.
    Ask,
    /// The peek, the briefing ready: two hops, her ear rings, the
    /// clipboard's dot pulses.
    Nudge,
    /// Something learned, the briefing: a hop, sparkles.
    Happy,
    /// A source's error, a masked secret, a suspicious mail: a drop.
    Worry,
    /// Offline: eyes closed, two "z", breathing.
    Sleep,
}

/// Her ear's colours: green when calm, amber for a question or something
/// due, orange for a problem, grey offline.
const GREEN: &str = "#7BC47F";
const AMBER: &str = "#F5B44A";
const ORANGE: &str = "#F2945B";
const GREY: &str = "#8C96A8";
/// The clipboard's dot when calm: blue, as in `secretary-calm.svg`.
const BLUE: &str = "#8FB3FF";

impl SecretaryMood {
    pub const ALL: [Self; 7] = [
        Self::Listen,
        Self::Type,
        Self::Ask,
        Self::Nudge,
        Self::Happy,
        Self::Worry,
        Self::Sleep,
    ];

    /// Its name, for logs, the protocol and the control socket.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Listen => "listen",
            Self::Type => "type",
            Self::Ask => "ask",
            Self::Nudge => "nudge",
            Self::Happy => "happy",
            Self::Worry => "worry",
            Self::Sleep => "sleep",
        }
    }

    pub fn of(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.name() == name)
    }

    /// Whether it keeps moving while the state lasts (she composes);
    /// the others play one burst after a change, then lie still.
    pub const fn lasting(self) -> bool {
        matches!(self, Self::Type)
    }

    /// Her ear's colour, and her clipboard's dot's.
    const fn colours(self) -> (&'static str, &'static str) {
        match self {
            Self::Listen | Self::Type | Self::Happy => (GREEN, BLUE),
            Self::Ask | Self::Nudge => (AMBER, AMBER),
            Self::Worry => (ORANGE, ORANGE),
            Self::Sleep => (GREY, GREY),
        }
    }

    /// Her still drawing in this mood (view box `0 0 120 120`).
    pub fn still(self) -> &'static str {
        static STILLS: OnceLock<Vec<String>> = OnceLock::new();
        let stills = STILLS.get_or_init(|| Self::ALL.into_iter().map(drawing).collect());
        Self::ALL
            .iter()
            .position(|m| *m == self)
            .and_then(|i| stills.get(i))
            .map_or("", String::as_str)
    }

    /// Its moving parts, the mockup's keyframes.
    fn parts(self) -> &'static [Part] {
        use Anim::*;
        use Ease::*;
        // Her eyes blink a second in, her ear twitches at once.
        const BLINK: Part = part("sx-eyes", Shut, 4.6, Plain, 1.1, (60.0, 52.0));
        const LISTEN: &[Part] = &[BLINK, part("sx-ear", Twitch, 3.4, Plain, 0.3, (85.0, 33.0))];
        const TYPE: &[Part] = &[
            BLINK,
            part("sx-head", Nod, 1.5, InOut, 0.0, (60.0, 82.0)),
            part("sx-face", Nod, 1.5, InOut, 0.0, (60.0, 82.0)),
            part("sx-paw", Scribble, 0.55, Linear, 0.0, (81.5, 92.0)),
            part("sx-line", Grow, 1.6, Out, 0.0, (46.0, 102.0)),
        ];
        // The tilt eases in rather than snapping to its first pose.
        const ASK: &[Part] = &[
            BLINK,
            part("sx-head", Tilt, 2.6, InOut, 0.2, (60.0, 82.0)),
            part("sx-face", Tilt, 2.6, InOut, 0.2, (60.0, 82.0)),
            part("sx-q", Float, 1.3, InOut, 0.0, (100.5, 8.0)),
        ];
        const NUDGE: &[Part] = &[
            BLINK,
            part("sx-all", Hop, 2.6, InOut, 0.0, (60.0, 116.0)),
            part("sx-ring1", Ripple, 1.3, Out, 0.0, (85.0, 33.0)),
            part("sx-ring2", Ripple, 1.3, Out, 0.65, (85.0, 33.0)),
            part("sx-dot", Pulse, 1.3, InOut, 0.0, (49.0, 96.0)),
        ];
        const HAPPY: &[Part] = &[
            BLINK,
            part("sx-all", Hop, 3.0, InOut, 0.0, (60.0, 116.0)),
            part("sx-s1", Glint, 1.1, InOut, 0.0, (18.0, 14.0)),
            part("sx-s2", Glint, 1.1, InOut, 0.55, (106.0, 59.5)),
            part("sx-s3", Glint, 1.1, InOut, 0.0, (100.0, 10.0)),
            part("sx-s4", Glint, 1.1, InOut, 0.55, (8.0, 70.0)),
        ];
        const WORRY: &[Part] = &[BLINK, part("sx-drop", Drip, 2.2, In, 0.0, (93.0, 45.0))];
        const SLEEP: &[Part] = &[
            part("sx-all", Puff, 4.0, InOut, 0.0, (60.0, 116.0)),
            part("sx-z1", Z, 2.8, Out, 0.0, (98.0, 18.0)),
            part("sx-z2", Z, 2.8, Out, 1.4, (108.5, 2.5)),
        ];
        match self {
            Self::Listen => LISTEN,
            Self::Type => TYPE,
            Self::Ask => ASK,
            Self::Nudge => NUDGE,
            Self::Happy => HAPPY,
            Self::Worry => WORRY,
            Self::Sleep => SLEEP,
        }
    }
}

/// Her drawing `t` into her mood's motion, moving for `window` (a burst
/// after a change, [`super::BURST`]; longer while the state lasts, as she
/// composes): her parts where they are then; none once it is over, her
/// still drawing ([`SecretaryMood::still`]) from then on.
pub fn secretary_frame(mood: SecretaryMood, window: Duration, t: Duration) -> Option<String> {
    super::moved(mood.still(), mood.parts(), window, t)
}

// --- Her drawing, by parts ---------------------------------------------------------

/// The pencil behind her ear.
const PENCIL: &str = r##"<g transform="translate(10 42.5) rotate(-41)"><path d="M0 0 L7.5 -3.4 L7.5 3.4 Z" fill="#F3D9A8" stroke="#23262E" stroke-width="1.6" stroke-linejoin="round" /><path d="M0 0 L2.6 -1.2 L2.6 1.2 Z" fill="#23262E" /><rect x="7.5" y="-3.4" width="31" height="6.8" fill="#F5C84B" stroke="#23262E" stroke-width="1.8" /><path d="M8.5 0 L37.5 0" stroke="#E0A92E" stroke-width="1.4" /><rect x="38.5" y="-3.4" width="4.6" height="6.8" fill="#C9CDD4" stroke="#23262E" stroke-width="1.6" /><path d="M43.1 -3.4 L46.5 -3.4 Q48.7 0 46.5 3.4 L43.1 3.4 Z" fill="#F08FA0" stroke="#23262E" stroke-width="1.6" stroke-linejoin="round" /></g>"##;

/// Her left ear.
const EAR_LEFT: &str = r##"<circle cx="37" cy="31" r="10.5" fill="#23262E" stroke="#F7F4EC" stroke-width="4" paint-order="stroke" />"##;

/// Her feet, her body and her clipboard up to its dot.
const BODY_TOP: &str = r##"<ellipse cx="45" cy="108" rx="10" ry="7" fill="#23262E" stroke="#F7F4EC" stroke-width="4" paint-order="stroke" /><ellipse cx="75" cy="108" rx="10" ry="7" fill="#23262E" stroke="#F7F4EC" stroke-width="4" paint-order="stroke" /><ellipse cx="60" cy="91" rx="27" ry="22" fill="#F7F4EC" stroke="#23262E" stroke-width="2.5" /><rect x="38" y="76" width="44" height="36" rx="4" fill="#B98352" stroke="#23262E" stroke-width="2" /><rect x="42" y="81" width="36" height="27" rx="2" fill="#F7F4EC" /><rect x="52" y="72.5" width="16" height="7" rx="2" fill="#9AA0AA" stroke="#23262E" stroke-width="1.6" /><path d="M46 88 L48.5 90.5 L52.5 85.5" fill="none" stroke="#3FA66B" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /><rect x="55" y="86.5" width="18" height="2.4" rx="1.2" fill="#8A8E98" />"##;

/// The clipboard's second line, under its dot.
const BODY_LINE2: &str =
    r##"<rect x="55" y="94.8" width="15" height="2.4" rx="1.2" fill="#8A8E98" />"##;

/// The line she writes, which grows as she does.
const BODY_LINE3: &str = r##"<g id="sx-line"><rect x="46" y="101.5" width="24" height="2.4" rx="1.2" fill="#C2C6CE" /></g>"##;

/// Her paws on the clipboard, the right one scribbling.
const PAWS: &str = r##"<g fill="#23262E" stroke="#F7F4EC" stroke-width="4" paint-order="stroke"><ellipse cx="38.5" cy="92" rx="7" ry="8.5" transform="rotate(12 38.5 92)" /><g id="sx-paw"><ellipse cx="81.5" cy="92" rx="7" ry="8.5" transform="rotate(-12 81.5 92)" /></g></g>"##;

/// Her face and its patches.
const FACE: &str = r##"<ellipse cx="60" cy="54" rx="30" ry="27" fill="#F7F4EC" stroke="#23262E" stroke-width="2.5" /><path d="M39 47 C43 40 54 42 55.5 50 C56.5 57 50 62.5 44.5 61.5 C39.5 60.5 36.5 53 39 47 Z" fill="#23262E" /><path d="M81 47 C77 40 66 42 64.5 50 C63.5 57 70 62.5 75.5 61.5 C80.5 60.5 83.5 53 81 47 Z" fill="#23262E" />"##;

/// Her open eyes.
const EYES: &str = r##"<g id="sx-eyes"><circle cx="48" cy="52" r="3.6" fill="#FFFFFF" /><circle cx="48.4" cy="52.4" r="2.1" fill="#23262E" /><circle cx="47.6" cy="51.3" r="0.8" fill="#FFFFFF" /><circle cx="72" cy="52" r="3.6" fill="#FFFFFF" /><circle cx="72.4" cy="52.4" r="2.1" fill="#23262E" /><circle cx="71.6" cy="51.3" r="0.8" fill="#FFFFFF" /></g>"##;

/// Her closed eyes, asleep.
const LIDS: &str = r##"<g fill="none" stroke="#F7F4EC" stroke-width="1.8" stroke-linecap="round"><path d="M44.6 52.4 Q48 55.2 51.4 52.4" /><path d="M68.6 52.4 Q72 55.2 75.4 52.4" /></g>"##;

/// Her glasses, nose and mouth.
const FACE_REST: &str = r##"<g fill="none" stroke="#E8B44E" stroke-width="2.2"><circle cx="48" cy="52" r="8.6" /><circle cx="72" cy="52" r="8.6" /><path d="M56.6 51 Q60 48.6 63.4 51" stroke-linecap="round" /><path d="M39.4 50 L33 47.5 M80.6 50 L87 47.5" stroke-linecap="round" /></g><path d="M56.5 62.4 Q60 60.4 63.5 62.4 Q62.3 65.5 60 66.1 Q57.7 65.5 56.5 62.4 Z" fill="#23262E" /><path d="M55.5 69.4 Q60 72.8 64.5 69.4" fill="none" stroke="#23262E" stroke-width="2" stroke-linecap="round" />"##;

/// Her bow.
const BOW: &str = r##"<g fill="#7BC47F" stroke="#23262E" stroke-width="1.8" stroke-linejoin="round"><path d="M60 80 L51 75.6 Q49.3 80 51 84.4 Z" /><path d="M60 80 L69 75.6 Q70.7 80 69 84.4 Z" /><rect x="57.6" y="77.6" width="4.8" height="4.8" rx="1.5" /></g>"##;

/// The question's "?", over her ear.
const QUESTION: &str = r##"<g id="sx-q"><path d="M95.5 4 Q95.5 -1 100.5 -1 Q105.5 -1 105.5 4 Q105.5 7.5 101 9 L101 12" fill="none" stroke="#F5B44A" stroke-width="3" stroke-linecap="round" stroke-linejoin="round" /><circle cx="101" cy="16.5" r="1.8" fill="#F5B44A" /></g>"##;

/// The sparkles round her when she is happy.
const SPARKLES: &str = r##"<g fill="#7EE0A0"><g id="sx-s1"><path d="M18 8 L19.6 12.6 L24 14 L19.6 15.4 L18 20 L16.4 15.4 L12 14 L16.4 12.6 Z" /></g><g id="sx-s2"><path d="M106 54 L107.3 58.2 L111.5 59.5 L107.3 60.8 L106 65 L104.7 60.8 L100.5 59.5 L104.7 58.2 Z" /></g><g id="sx-s3"><circle cx="100" cy="10" r="2.2" /></g><g id="sx-s4"><circle cx="8" cy="70" r="2" /></g></g>"##;

/// The two "z" of her sleep.
const ZS: &str = r##"<g fill="none" stroke="#A6A9B1" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><g id="sx-z1"><path d="M94 14 L102 14 L94 22 L102 22" /></g><g id="sx-z2"><path d="M106 0 L111 0 L106 5 L111 5" /></g></g>"##;

/// The worry's drop, by her ear.
const DROP: &str = r##"<g id="sx-drop"><path d="M93 40 Q97.5 46.5 93 50 Q88.5 46.5 93 40 Z" fill="#8FB3FF" stroke="#23262E" stroke-width="1.2" /></g>"##;

/// Her drawing in `mood`: the calm drawing's shapes in its order, her ear
/// and dot in the mood's colours, its marks over it.
fn drawing(mood: SecretaryMood) -> String {
    use SecretaryMood::*;
    let (ear, dot) = mood.colours();
    let rings = if mood == Nudge {
        format!(
            "<g fill=\"none\" stroke=\"{ear}\" stroke-width=\"2.2\" stroke-linecap=\"round\">\
             <g id=\"sx-ring1\"><path d=\"M70.9 27.9 A15 15 0 1 1 96.5 42.6\" /></g>\
             <g id=\"sx-ring2\"><path d=\"M66.2 26.2 A20 20 0 1 1 100.3 45.9\" opacity=\"0.6\" /></g></g>"
        )
    } else {
        String::new()
    };
    let marks = match mood {
        Ask => QUESTION,
        Happy => SPARKLES,
        Sleep => ZS,
        Worry => DROP,
        Listen | Type | Nudge => "",
    };
    let (eyes, opacity) = if mood == Sleep {
        // Asleep she fades a little, as in the mockup.
        (LIDS, " opacity=\"0.8\"")
    } else {
        (EYES, "")
    };
    format!(
        "<!-- The secretary, {name}: Hocuspanda's own artwork (assets/README.md). -->\n\
         <svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 120 120\" width=\"120\" height=\"120\">\
         <g id=\"sx-all\"><g{opacity}>\
         <g id=\"sx-head\">{PENCIL}{EAR_LEFT}<g id=\"sx-ear\"><ellipse cx=\"85\" cy=\"33\" rx=\"11.5\" ry=\"7.5\" \
         transform=\"rotate(38 85 33)\" fill=\"{ear}\" stroke=\"#23262E\" stroke-width=\"2.5\" paint-order=\"stroke\" /></g></g>\
         {BODY_TOP}<g id=\"sx-dot\"><circle cx=\"49\" cy=\"96\" r=\"2.2\" fill=\"{dot}\" /></g>{BODY_LINE2}{BODY_LINE3}{PAWS}\
         <g id=\"sx-face\">{FACE}{eyes}{FACE_REST}</g>{BOW}{rings}{marks}</g></g></svg>\n",
        name = mood.name(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BURST, lasts};

    const MS: fn(u64) -> Duration = Duration::from_millis;

    /// Each mood's marks, by a group that only it has.
    const MARKS: [(SecretaryMood, &str); 5] = [
        (SecretaryMood::Nudge, "sx-ring1"),
        (SecretaryMood::Ask, "sx-q"),
        (SecretaryMood::Happy, "sx-s1"),
        (SecretaryMood::Sleep, "sx-z1"),
        (SecretaryMood::Worry, "sx-drop"),
    ];

    fn render(svg: &str) -> Vec<u8> {
        let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default()).expect("svg");
        let mut pixmap = resvg::tiny_skia::Pixmap::new(120, 120).expect("pixmap");
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        pixmap.take()
    }

    fn pixel(pixels: &[u8], (x, y): (usize, usize)) -> [u8; 3] {
        let i = (y * 120 + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    }

    #[test]
    fn names_read_back() {
        for mood in SecretaryMood::ALL {
            assert_eq!(SecretaryMood::of(mood.name()), Some(mood));
        }
        assert_eq!(SecretaryMood::of("grumpy"), None);
        assert!(SecretaryMood::Type.lasting());
        assert!(!SecretaryMood::Listen.lasting());
    }

    #[test]
    fn each_mood_shows_its_marks_and_no_others() {
        for mood in SecretaryMood::ALL {
            let still = mood.still();
            for (owner, group) in MARKS {
                let has = still.contains(&format!("<g id=\"{group}\">"));
                assert_eq!(has, owner == mood, "{group} in {}", mood.name());
            }
            // Asleep, her eyes are closed.
            assert_eq!(
                still.contains("<g id=\"sx-eyes\">"),
                mood != SecretaryMood::Sleep
            );
        }
    }

    #[test]
    fn every_moving_part_is_a_group_of_her_drawing() {
        for mood in SecretaryMood::ALL {
            for part in mood.parts() {
                let group = format!("<g id=\"{}\">", part.id);
                assert_eq!(mood.still().matches(&group).count(), 1, "{}", part.id);
            }
        }
    }

    #[test]
    fn without_marks_she_is_the_calm_drawing() {
        let calm = render(crate::Secretary::Calm.still());
        for mood in [SecretaryMood::Listen, SecretaryMood::Type] {
            assert_eq!(render(mood.still()), calm, "{}", mood.name());
        }
    }

    #[test]
    fn her_ear_is_in_the_moods_colour() {
        let rgb = |hex: &str| {
            let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).expect("hex");
            [(v >> 16) as u8, (v >> 8) as u8, v as u8]
        };
        for (mood, colour) in [
            (SecretaryMood::Listen, GREEN),
            (SecretaryMood::Type, GREEN),
            (SecretaryMood::Happy, GREEN),
            (SecretaryMood::Ask, AMBER),
            (SecretaryMood::Nudge, AMBER),
            (SecretaryMood::Worry, ORANGE),
            (SecretaryMood::Sleep, GREY),
        ] {
            let shown = render(mood.still());
            let [r, g, b] = pixel(&shown, (85, 33));
            let [er, eg, eb] = rgb(colour);
            // Asleep she is drawn at 80 %: near, not equal.
            let near = |a: u8, b: u8| a.abs_diff(b) < 60;
            assert!(
                near(r, er) && near(g, eg) && near(b, eb),
                "{}: {:?} for {colour}",
                mood.name(),
                [r, g, b]
            );
        }
    }

    #[test]
    fn a_burst_moves_then_draws_nothing_more() {
        for mood in SecretaryMood::ALL {
            assert!(secretary_frame(mood, BURST, Duration::ZERO).is_some());
            assert!(secretary_frame(mood, BURST, lasts(BURST) - MS(1)).is_some());
            assert_eq!(secretary_frame(mood, BURST, lasts(BURST)), None);
            // Somewhere in it, she moves.
            let moves = (0..26).any(|i| {
                secretary_frame(mood, BURST, MS(i * 100)).is_some_and(|f| f != mood.still())
            });
            assert!(moves, "{}", mood.name());
        }
        // Writing, mid-burst, she is not still.
        let mid = secretary_frame(SecretaryMood::Type, BURST, MS(700)).expect("a frame");
        assert_ne!(render(&mid), render(SecretaryMood::Type.still()));
        // Listening, she blinks within a burst.
        let blink = secretary_frame(SecretaryMood::Listen, BURST, MS(1200)).expect("a frame");
        assert_ne!(render(&blink), render(SecretaryMood::Listen.still()));
    }

    #[test]
    fn writing_lasts_while_she_composes() {
        let long = Duration::from_secs(60);
        assert!(secretary_frame(SecretaryMood::Type, long, Duration::from_secs(30)).is_some());
        assert_eq!(
            secretary_frame(SecretaryMood::Type, long, lasts(long)),
            None
        );
    }

    #[test]
    fn its_frames_are_drawings_and_settle() {
        let options = resvg::usvg::Options::default();
        for mood in SecretaryMood::ALL {
            let mut t = Duration::ZERO;
            while let Some(svg) = secretary_frame(mood, BURST, t) {
                assert!(
                    resvg::usvg::Tree::from_str(&svg, &options).is_ok(),
                    "{} at {t:?}",
                    mood.name()
                );
                t += MS(50);
            }
            let end = lasts(BURST).as_secs_f32();
            for part in mood.parts() {
                assert!(part.at(BURST.as_secs_f32(), end).is_rest(), "{}", part.id);
            }
        }
    }

    /// Her seven moods still and mid-motion, as one PNG file in the folder
    /// `HOCUS_SHOTS` names.
    #[test]
    #[ignore = "writes screenshots: run with HOCUS_SHOTS set"]
    fn screenshots_of_her_moods() {
        let Some(dir) = std::env::var_os("HOCUS_SHOTS") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("the folder");
        let scale = 2.0;
        let side = (120.0 * scale) as u32;
        let mut sheet = resvg::tiny_skia::Pixmap::new(side * 7, side * 2).expect("pixmap");
        sheet.fill(resvg::tiny_skia::Color::from_rgba8(16, 19, 30, 255));
        for (i, mood) in SecretaryMood::ALL.into_iter().enumerate() {
            let mid = secretary_frame(mood, BURST, MS(600)).unwrap_or_default();
            for (row, svg) in [mood.still(), mid.as_str()].into_iter().enumerate() {
                let tree = resvg::usvg::Tree::from_str(svg, &resvg::usvg::Options::default())
                    .expect("svg");
                let at = resvg::tiny_skia::Transform::from_row(
                    scale,
                    0.0,
                    0.0,
                    scale,
                    (i as u32 * side) as f32,
                    (row as u32 * side) as f32,
                );
                resvg::render(&tree, at, &mut sheet.as_mut());
            }
        }
        sheet
            .save_png(dir.join("secretary-moods.png"))
            .expect("the file");
    }
}
