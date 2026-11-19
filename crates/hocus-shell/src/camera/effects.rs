//! The camera's effects, one list ([`EFFECTS`]): each is an entry, its
//! tab, how it is chosen, its thumbnail (the mockup's CSS gradients), its
//! words, and whether this version makes it. A later lot adds an effect by
//! adding its entry; an add-on's (#442) will come the same way.
//!
//! Lot 2 makes Lumière and Bandeau (Image); lot 3 Fond (the room, Flou,
//! the desktop's living wallpapers), Présentateur (Image) and « Je
//! reviens » (Réactions); lot 5 Voix (the Hocuspanda microphone's voices,
//! written into `[voice] effect`). The others show, dimmed, where they
//! will be: their tab is the mockup's, their tile says "Bientôt".

use hocus_link::CameraShown;
use hocus_words::{Text, shell::camera as w};

/// The window's tabs, in its order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Image,
    Background,
    Face,
    Voice,
    Reactions,
}

impl Tab {
    pub const ALL: [Self; 5] = [
        Self::Image,
        Self::Background,
        Self::Face,
        Self::Voice,
        Self::Reactions,
    ];

    pub const fn name(self) -> Text {
        match self {
            Self::Image => w::IMAGE,
            Self::Background => w::BACKGROUND,
            Self::Face => w::FACE,
            Self::Voice => w::VOICE,
            Self::Reactions => w::REACTIONS,
        }
    }

    /// Its effects, in its grid's order.
    pub fn effects(self) -> impl Iterator<Item = &'static Effect> {
        EFFECTS.iter().filter(move |e| e.tab == self)
    }

    /// Whether one of its effects is on (its violet dot).
    pub fn on(self, shown: &CameraShown) -> bool {
        self.effects().any(|e| e.on(shown) && !e.plain())
    }

    /// Whether this version makes none of its effects.
    pub fn soon(self) -> bool {
        !self.effects().any(|e| e.ready)
    }
}

/// How an effect is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// On or off, by itself: its `[camera]` key.
    Toggle(&'static str),
    /// One of its tab's, as a key's value (`voice`: `[voice] effect`);
    /// `plain` is the choice that is no effect ("Ta pièce", "Aucun",
    /// "Telle quelle").
    One {
        key: &'static str,
        value: &'static str,
        plain: bool,
    },
}

/// A thumbnail, as the mockup's CSS draws it, colours as `0xRRGGBBAA`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Thumb {
    Flat(u32),
    /// `linear-gradient(ANGLE, …)`: its angle (CSS's degrees), its stops
    /// (where, from 0 to 1, and their colour).
    Linear(f32, &'static [(f32, u32)]),
    /// `radial-gradient(circle, A, B)`: from the middle to the corners.
    Radial(u32, u32),
    /// `radial-gradient(circle at X Y, A R%, B …)`: a disc of `A` at
    /// (`x`, `y`), parts of the size, `r` of the way to the farthest
    /// corner, on `B`.
    Disc {
        at: (f32, f32),
        r: f32,
        inner: u32,
        outer: u32,
    },
    /// The ears: two discs on a grey.
    Ears,
}

/// An effect of the camera's window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Effect {
    pub id: &'static str,
    pub tab: Tab,
    pub pick: Pick,
    pub thumb: Thumb,
    pub name: Text,
    /// What it does, under the grid while it is chosen.
    pub hint: Option<Text>,
    /// This version makes it.
    pub ready: bool,
}

impl Effect {
    /// Whether it is chosen now.
    pub fn on(&self, shown: &CameraShown) -> bool {
        match self.pick {
            Pick::Toggle(key) => toggle(shown, key),
            Pick::One { key, value, .. } => one(shown, key) == value,
        }
    }

    /// Whether it is the choice that is no effect.
    pub fn plain(&self) -> bool {
        matches!(self.pick, Pick::One { plain: true, .. })
    }
}

/// An on-or-off key's value, as the compositor says it: the ones made;
/// the others off.
fn toggle(shown: &CameraShown, key: &str) -> bool {
    match key {
        "lighting" => shown.lighting,
        "banner" => shown.banner,
        "presenter" => shown.presenter,
        "away" => shown.away,
        _ => false,
    }
}

/// A one-of key's value, as the compositor says it; until their lots, the
/// plain choice.
fn one<'a>(shown: &'a CameraShown, key: &str) -> &'a str {
    match key {
        "background" if !shown.background.is_empty() => &shown.background,
        "background" => "room",
        "voice" if !shown.voice.is_empty() => &shown.voice,
        "voice" => "quiet",
        _ => "none",
    }
}

const fn toggle_of(
    id: &'static str,
    tab: Tab,
    thumb: Thumb,
    name: Text,
    hint: Option<Text>,
    ready: bool,
) -> Effect {
    Effect {
        id,
        tab,
        pick: Pick::Toggle(id),
        thumb,
        name,
        hint,
        ready,
    }
}

const fn one_of(
    key: &'static str,
    value: &'static str,
    tab: Tab,
    thumb: Thumb,
    name: Text,
    hint: Option<Text>,
) -> Effect {
    let plain = matches!(value.as_bytes(), b"room" | b"none");
    // Fond (lot 3) and Voix (lot 5) are made; the others wait for theirs.
    let ready = matches!(key.as_bytes(), b"background" | b"voice");
    Effect {
        id: value,
        tab,
        pick: Pick::One { key, value, plain },
        thumb,
        name,
        hint,
        ready,
    }
}

/// A voice (lot 5): one of Voix's, `[voice] effect`; its id apart from
/// its word, Visage's Panda being `panda` too.
const fn voice_of(
    id: &'static str,
    value: &'static str,
    thumb: Thumb,
    name: Text,
    hint: Option<Text>,
) -> Effect {
    let mut effect = one_of("voice", value, Tab::Voice, thumb, name, hint);
    effect.id = id;
    effect
}

const PANDA: Thumb = Thumb::Disc {
    at: (0.5, 0.5),
    r: 0.45,
    inner: 0xF4F1_EAFF,
    outer: 0x1A1C_22FF,
};

/// Every effect, in the mockup's order.
pub const EFFECTS: &[Effect] = &[
    toggle_of(
        "lighting",
        Tab::Image,
        Thumb::Linear(135.0, &[(0.0, 0xF5D9_A8FF), (1.0, 0xF5B4_4AFF)]),
        w::LIGHTING,
        Some(w::LIGHTING_HINT),
        true,
    ),
    toggle_of(
        "framing",
        Tab::Image,
        Thumb::Disc {
            at: (0.5, 0.45),
            r: 0.31,
            inner: 0xE3BC_9DFF,
            outer: 0x2B34_48FF,
        },
        w::FRAMING,
        Some(w::FRAMING_HINT),
        false,
    ),
    toggle_of(
        "banner",
        Tab::Image,
        Thumb::Linear(
            0.0,
            &[
                (0.0, 0x0A0E_18FF),
                (0.4, 0x0A0E_18FF),
                (0.42, 0x4A55_70FF),
                (1.0, 0x4A55_70FF),
            ],
        ),
        w::BANNER,
        Some(w::BANNER_HINT),
        true,
    ),
    // Présentateur (lot 3): a bust over the board's shared screen.
    toggle_of(
        "presenter",
        Tab::Image,
        Thumb::Disc {
            at: (0.5, 1.0),
            r: 0.42,
            inner: 0x3B5B_8CFF,
            outer: 0xF6F4_EEFF,
        },
        w::PRESENTER,
        Some(w::PRESENTER_HINT),
        true,
    ),
    one_of(
        "background",
        "room",
        Tab::Background,
        Thumb::Linear(180.0, &[(0.0, 0xC9B8_A0FF), (1.0, 0x8E7C_66FF)]),
        w::ROOM,
        Some(w::ROOM_HINT),
    ),
    one_of(
        "background",
        "blur",
        Tab::Background,
        Thumb::Linear(180.0, &[(0.0, 0xB7A4_8BFF), (1.0, 0x8E7C_66FF)]),
        w::BLUR,
        Some(w::BLUR_HINT),
    ),
    one_of(
        "background",
        "aurora",
        Tab::Background,
        Thumb::Linear(
            135.0,
            &[(0.0, 0x0B1B_33FF), (0.5, 0x54E6_AAFF), (1.0, 0x8278_FFFF)],
        ),
        w::AURORA,
        Some(w::AURORA_HINT),
    ),
    one_of(
        "background",
        "koi",
        Tab::Background,
        Thumb::Radial(0x1F5C_66FF, 0x0D2E_38FF),
        w::KOI,
        Some(w::KOI_HINT),
    ),
    one_of(
        "face",
        "none",
        Tab::Face,
        Thumb::Flat(0xFFFF_FF14),
        w::NO_FACE,
        None,
    ),
    one_of(
        "face",
        "ears",
        Tab::Face,
        Thumb::Ears,
        w::EARS,
        Some(w::EARS_HINT),
    ),
    one_of(
        "face",
        "beanie",
        Tab::Face,
        Thumb::Linear(
            180.0,
            &[
                (0.0, 0xD953_4FFF),
                (0.6, 0xD953_4FFF),
                (0.62, 0xB33F_3BFF),
                (1.0, 0xB33F_3BFF),
            ],
        ),
        w::BEANIE,
        None,
    ),
    one_of(
        "face",
        "crown",
        Tab::Face,
        Thumb::Linear(180.0, &[(0.0, 0xF5C5_42FF), (1.0, 0xC997_1EFF)]),
        w::CROWN,
        None,
    ),
    one_of(
        "face",
        "glasses",
        Tab::Face,
        Thumb::Linear(
            90.0,
            &[
                (0.0, 0x1416_1CFF),
                (0.45, 0x1416_1CFF),
                (0.47, 0x8FB3_FFFF),
                (0.53, 0x8FB3_FFFF),
                (0.55, 0x1416_1CFF),
                (1.0, 0x1416_1CFF),
            ],
        ),
        w::GLASSES,
        None,
    ),
    one_of(
        "face",
        "panda",
        Tab::Face,
        PANDA,
        w::PANDA,
        Some(w::PANDA_HINT),
    ),
    voice_of(
        "voice-none",
        "none",
        Thumb::Flat(0xFFFF_FF14),
        w::AS_IT_IS,
        None,
    ),
    voice_of(
        "voice-quiet",
        "quiet",
        Thumb::Linear(90.0, &[(0.0, 0x6CC2_87FF), (1.0, 0x2B34_48FF)]),
        w::NO_NOISE,
        Some(w::NO_NOISE_HINT),
    ),
    voice_of(
        "voice-studio",
        "studio",
        Thumb::Linear(90.0, &[(0.0, 0xF5B4_4AFF), (1.0, 0xF294_5BFF)]),
        w::STUDIO,
        Some(w::STUDIO_HINT),
    ),
    voice_of(
        "voice-panda",
        "panda",
        PANDA,
        w::PANDA,
        Some(w::PANDA_VOICE_HINT),
    ),
    voice_of(
        "voice-robot",
        "robot",
        Thumb::Linear(90.0, &[(0.0, 0x8FB3_FFFF), (1.0, 0x5B6F_A8FF)]),
        w::ROBOT,
        Some(w::ROBOT_HINT),
    ),
    voice_of(
        "voice-deep",
        "deep",
        Thumb::Linear(90.0, &[(0.0, 0x5B3E_8CFF), (1.0, 0x2B1F_45FF)]),
        w::DEEP,
        Some(w::DEEP_HINT),
    ),
    toggle_of(
        "gestures",
        Tab::Reactions,
        Thumb::Linear(135.0, &[(0.0, 0xF5B4_4AFF), (1.0, 0xF294_5BFF)]),
        w::GESTURES,
        Some(w::GESTURES_HINT),
        false,
    ),
    toggle_of(
        "celebrations",
        Tab::Reactions,
        Thumb::Linear(
            135.0,
            &[(0.0, 0x7EE0_A0FF), (0.5, 0x8FB3_FFFF), (1.0, 0xF5B4_4AFF)],
        ),
        w::PARTIES,
        Some(w::PARTIES_HINT),
        false,
    ),
    // « Je reviens » (lot 3): the board's panda asleep.
    toggle_of(
        "away",
        Tab::Reactions,
        PANDA,
        w::AWAY,
        Some(w::AWAY_HINT),
        true,
    ),
];

/// The hint under `tab`'s grid: its first chosen effect's with a hint
/// ("Lumière : exposition, blancs et chaleur"); "Bientôt dans Hocuspanda"
/// for a tab this version makes none of; else nothing.
pub fn hint(tab: Tab, shown: &CameraShown, lang: hocus_words::Lang) -> String {
    if tab.soon() {
        return w::SOON_TAB.get(lang).to_owned();
    }
    tab.effects()
        .filter(|e| e.ready && e.on(shown))
        .find_map(|e| {
            e.hint.map(|hint| {
                w::hint(
                    lang,
                    w::Hint {
                        name: e.name.get(lang),
                        hint: hint.get(lang),
                    },
                )
            })
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shown(lighting: bool, banner: bool) -> CameraShown {
        CameraShown {
            lighting,
            banner,
            screen_light: true,
            strength: "auto".to_owned(),
            ready: true,
            read: true,
            source: None,
            background: "room".to_owned(),
            away: true,
            presenter: false,
            hush: false,
            voice: "none".to_owned(),
            voice_ready: false,
            voice_read: false,
            microphone: None,
        }
    }

    #[test]
    fn each_tab_has_its_effects_and_lots_2_3_and_5_make_theirs() {
        let ids = |tab: Tab| tab.effects().map(|e| e.id).collect::<Vec<_>>();
        assert_eq!(
            ids(Tab::Image),
            ["lighting", "framing", "banner", "presenter"]
        );
        assert_eq!(ids(Tab::Background), ["room", "blur", "aurora", "koi"]);
        assert_eq!(ids(Tab::Face).len(), 6);
        assert_eq!(ids(Tab::Voice).len(), 6);
        assert_eq!(ids(Tab::Reactions), ["gestures", "celebrations", "away"]);
        let ready: Vec<&str> = EFFECTS.iter().filter(|e| e.ready).map(|e| e.id).collect();
        assert_eq!(
            ready,
            [
                "lighting",
                "banner",
                "presenter",
                "room",
                "blur",
                "aurora",
                "koi",
                "voice-none",
                "voice-quiet",
                "voice-studio",
                "voice-panda",
                "voice-robot",
                "voice-deep",
                "away"
            ]
        );
        assert!(!Tab::Image.soon() && Tab::Face.soon() && !Tab::Background.soon());
        assert!(!Tab::Voice.soon());
    }

    #[test]
    fn fond_is_one_of_its_tiles_with_its_dot() {
        let lang = hocus_words::Lang::French;
        let mut s = shown(false, false);
        assert!(!Tab::Background.on(&s));
        assert_eq!(hint(Tab::Background, &s, lang), "Ta pièce : sans effet");
        s.background = "aurora".to_owned();
        assert!(Tab::Background.on(&s));
        assert_eq!(
            hint(Tab::Background, &s, lang),
            "Aurore : le fond vivant du bureau"
        );
        let on: Vec<&str> = Tab::Background
            .effects()
            .filter(|e| e.on(&s))
            .map(|e| e.id)
            .collect();
        assert_eq!(on, ["aurora"]);
    }

    /// Voix: one voice at a time, the dot while it is not the microphone
    /// as it is, its words under the grid.
    #[test]
    fn voix_is_one_of_its_tiles_with_its_dot() {
        let lang = hocus_words::Lang::French;
        let mut s = shown(false, false);
        s.voice = "none".to_owned();
        assert!(!Tab::Voice.on(&s));
        assert_eq!(hint(Tab::Voice, &s, lang), "");
        s.voice = "quiet".to_owned();
        assert!(Tab::Voice.on(&s));
        assert_eq!(
            hint(Tab::Voice, &s, lang),
            "Sans bruit : clavier, ventilateur, rue : retirés"
        );
        s.voice = "deep".to_owned();
        assert_eq!(hint(Tab::Voice, &s, lang), "Grave : une octave plus bas");
        let on: Vec<&str> = Tab::Voice
            .effects()
            .filter(|e| e.on(&s))
            .map(|e| e.id)
            .collect();
        assert_eq!(on, ["voice-deep"]);
        // The microphone's tile's dot: only while the Hocuspanda
        // microphone is read.
        assert!(!s.voice_dot());
        s.voice_read = true;
        assert!(s.voice_dot());
        s.voice = "none".to_owned();
        assert!(!s.voice_dot());
    }

    #[test]
    fn the_dot_and_the_hint_follow_what_is_on() {
        let lang = hocus_words::Lang::French;
        let off = shown(false, false);
        assert!(!Tab::Image.on(&off));
        assert_eq!(hint(Tab::Image, &off, lang), "");
        let on = shown(true, true);
        assert!(Tab::Image.on(&on));
        assert_eq!(
            hint(Tab::Image, &on, lang),
            "Lumière : exposition, blancs et chaleur"
        );
        assert_eq!(
            hint(Tab::Image, &shown(false, true), lang),
            "Bandeau : ton nom et ton projet"
        );
        // A tab of plain choices only has no dot.
        assert!(!Tab::Background.on(&on));
        assert!(Tab::Reactions.on(&on), "« Je reviens » is on");
        assert_eq!(hint(Tab::Face, &on, lang), "Bientôt dans Hocuspanda");
    }
}
