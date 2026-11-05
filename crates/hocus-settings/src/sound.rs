//! The Son page's model (6.5, settings.md "Son"): what the search finds on
//! it, and how its rows read PipeWire's graph. Nothing of it is in the
//! file: the default sink and source, each device's volume, mute, balance
//! and profile, and each application's volume and output live in
//! PipeWire, which WirePlumber remembers per device and per application.
//! The window's side is `app::sound`.

use hocus_sound::{Card, Kind, Mixer, Sink};

use crate::{
    lang::{Lang, Words, w},
    model::{Outside, SectionId},
};

const OUTPUT: Words = w("SORTIE", "OUTPUT");
const INPUT: Words = w("ENTRÉE", "INPUT");
const APPS: Words = w("APPLICATIONS", "APPLICATIONS");

/// The page's settings, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const fn o(
        id: &'static str,
        group: Words,
        name: Words,
        fr: &'static [&'static str],
        en: &'static [&'static str],
    ) -> Outside {
        Outside {
            id,
            section: SectionId::Sound,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 9] = [
        o(
            "sound.output",
            OUTPUT,
            w("Sortie", "Output"),
            &["haut-parleurs", "casque", "enceinte", "hdmi", "displayport"],
            &["output", "speaker", "headphones"],
        ),
        o(
            "sound.output-volume",
            OUTPUT,
            w("Volume de la sortie", "Output volume"),
            &["fort", "muet"],
            &["volume", "mute", "loud"],
        ),
        o(
            "sound.profile",
            OUTPUT,
            w("Profil", "Profile"),
            &["a2dp", "hfp", "qualité", "mode appel"],
            &["profile", "codec"],
        ),
        o(
            "sound.balance",
            OUTPUT,
            w("Balance", "Balance"),
            &["gauche", "droite"],
            &["left", "right", "stereo"],
        ),
        o(
            "sound.test",
            OUTPUT,
            w("Tester les haut-parleurs", "Test the speakers"),
            &["essai"],
            &["test"],
        ),
        o(
            "sound.input",
            INPUT,
            w("Entrée", "Input"),
            &["micro", "microphone", "casque"],
            &["input", "mic"],
        ),
        o(
            "sound.input-volume",
            INPUT,
            w("Volume du micro", "Microphone volume"),
            &["gain", "sensibilité"],
            &["gain"],
        ),
        o(
            "sound.meter",
            INPUT,
            w("Niveau du micro", "Microphone level"),
            &["vumètre"],
            &["level", "meter"],
        ),
        o(
            "sound.apps",
            APPS,
            w("Volume des applications", "Applications' volume"),
            &["mélangeur"],
            &["mixer", "per application"],
        ),
    ];
    &ALL
}

/// The graph as the page reads it (icons are not read here).
pub type Graph = Mixer<()>;

/// Which side a channel is on, by its position.
fn side(position: &str) -> Option<bool> {
    const LEFT: [&str; 10] = [
        "FL", "RL", "SL", "FLC", "TFL", "TRL", "TSL", "FLW", "FLH", "RLC",
    ];
    const RIGHT: [&str; 10] = [
        "FR", "RR", "SR", "FRC", "TFR", "TRR", "TSR", "FRW", "FRH", "RRC",
    ];
    if LEFT.contains(&position) {
        Some(false)
    } else if RIGHT.contains(&position) {
        Some(true)
    } else {
        None
    }
}

/// The sides of `sink`'s channels: `Some(true)` the right, `None` neither
/// (the centre, the bass); two channels without positions are left and
/// right.
fn sides(sink: &Sink) -> Vec<Option<bool>> {
    if sink.positions.len() == sink.channels.len() {
        sink.positions.iter().map(|p| side(p)).collect()
    } else if sink.channels.len() == 2 {
        vec![Some(false), Some(true)]
    } else {
        vec![None; sink.channels.len()]
    }
}

/// A stereo output's balance, from -100 (all left) to 100 (all right),
/// by tens; 0 for a mono one.
pub fn balance(sink: &Sink) -> i32 {
    let sides = sides(sink);
    let loudest = |right: bool| {
        sink.channels
            .iter()
            .zip(&sides)
            .filter(|(_, s)| **s == Some(right))
            .map(|(c, _)| *c)
            .fold(0.0_f32, f32::max)
    };
    let (left, right) = (loudest(false), loudest(true));
    let top = left.max(right);
    if top <= 0.0 || sink.mono() {
        return 0;
    }
    let by = |quieter: f32| ((1.0 - quieter / top) * 10.0).round() as i32 * 10;
    if right < left { -by(right) } else { by(left) }
}

/// `sink`'s channels at `balance`: the louder side at its volume, the
/// other lowered by it; the centre and the bass at the volume.
pub fn balanced(sink: &Sink, balance: i32) -> Vec<f32> {
    let top = sink.channels.iter().copied().fold(0.0_f32, f32::max);
    let top = if top > 0.0 {
        top
    } else {
        sink.sound
            .map_or(0.0, |s| (s.volume.clamp(0.0, 100.0) / 100.0).powi(3))
    };
    let lowered = top * (1.0 - balance.unsigned_abs().min(100) as f32 / 100.0);
    sides(sink)
        .into_iter()
        .map(|side| match side {
            Some(false) if balance > 0 => lowered,
            Some(true) if balance < 0 => lowered,
            _ => top,
        })
        .collect()
}

/// "centre", "G 30", "D 30".
pub fn say_balance(balance: i32, lang: Lang) -> String {
    match (balance.signum(), lang) {
        (0, _) => w("centre", "centre").get(lang).to_owned(),
        (-1, Lang::French) => format!("G {}", -balance),
        (-1, Lang::English) => format!("L {}", -balance),
        (_, Lang::French) => format!("D {balance}"),
        (_, Lang::English) => format!("R {balance}"),
    }
}

/// Whether `card` is a Bluetooth headset.
fn bluetooth(sink: &Sink, card: Option<&Card>) -> bool {
    sink.bus.as_deref() == Some("bluetooth")
        || card.is_some_and(|c| c.api.as_deref() == Some("bluez5"))
}

/// Whether a profile is a headset's call mode (HFP, HSP).
pub fn call_mode(name: &str) -> bool {
    name.starts_with("headset-head-unit") || name.contains("hfp") || name.contains("hsp")
}

/// Whether a device's profile now is its call mode.
fn in_call_mode(card: Option<&Card>) -> bool {
    card.and_then(|c| {
        let index = c.profile?;
        c.profiles.iter().find(|p| p.index == index)
    })
    .is_some_and(|p| call_mode(&p.name))
}

/// The line under a device's name: what it is.
pub fn line(sink: &Sink, graph: &Graph, input: bool, lang: Lang) -> String {
    let card = sink.device.and_then(|d| graph.card(d));
    if bluetooth(sink, card) {
        let mut said = "Bluetooth".to_owned();
        if !input && in_call_mode(card) {
            said.push_str(w(" · mode appel", " · call mode").get(lang));
        }
        return said;
    }
    if sink.kind == Kind::Screen {
        return w("Écran externe", "External screen").get(lang).to_owned();
    }
    match (sink.form_factor.as_deref(), sink.bus.as_deref(), input) {
        (Some("internal"), _, false) | (None, Some("pci"), false) => {
            w("Intégrés au portable", "Built into the laptop")
        }
        (Some("internal"), _, true) | (None, Some("pci"), true) => {
            w("Intégré au portable", "Built into the laptop")
        }
        (_, Some("usb"), _) => w("USB", "USB"),
        _ => match card {
            Some(card) if !card.description.is_empty() => return card.description.clone(),
            _ => w("Sur ce PipeWire", "On this PipeWire"),
        },
    }
    .get(lang)
    .to_owned()
}

/// A device's own volume, on its row: "45 %", or "muet" ("coupé" for an
/// input).
pub fn say_volume(sink: &Sink, input: bool, lang: Lang) -> String {
    match sink.sound {
        Some(s) if s.muted => if input {
            w("coupé", "off")
        } else {
            w("muet", "muted")
        }
        .get(lang)
        .to_owned(),
        Some(s) => format!("{} %", s.volume.round() as i32),
        None => String::new(),
    }
}

/// The line under a profile: what it is for, for a headset's.
pub fn profile_line(name: &str, only: bool) -> Option<Words> {
    if only {
        return Some(w(
            "Le seul que propose cet appareil.",
            "The only one this device offers.",
        ));
    }
    if name.starts_with("a2dp") {
        Some(w(
            "Le meilleur son ; le micro du casque ne sert pas.",
            "The best sound; the headset's microphone is not used.",
        ))
    } else if call_mode(name) {
        Some(w(
            "Qualité téléphone, pour parler dans le micro du casque.",
            "Phone quality, to talk into the headset's microphone.",
        ))
    } else {
        None
    }
}

/// A headset whose microphone is offered while it plays in high quality:
/// its card, and its call mode's profile, which choosing the microphone
/// switches it to (WirePlumber's autoswitch, made by hand).
pub fn headsets_asleep(graph: &Graph) -> Vec<(u32, u32, String)> {
    graph
        .cards
        .iter()
        .filter(|card| card.api.as_deref() == Some("bluez5"))
        .filter(|card| !in_call_mode(Some(card)))
        .filter(|card| !graph.sources.iter().any(|s| s.device == Some(card.id)))
        .filter_map(|card| {
            let call = card.choices().find(|p| call_mode(&p.name))?;
            Some((card.id, call.index, card.description.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use hocus_sound::{Profile, Sound};

    use super::*;

    fn sink(channels: &[f32], positions: &[&str]) -> Sink {
        Sink {
            id: 30,
            serial: Some(130),
            name: "speakers".into(),
            description: "Haut-parleurs".into(),
            kind: Kind::Speaker,
            sound: Some(Sound {
                volume: 80.0,
                muted: false,
            }),
            channels: channels.to_vec(),
            positions: positions.iter().map(|p| (*p).to_owned()).collect(),
            default: true,
            device: None,
            bus: None,
            form_factor: None,
        }
    }

    #[test]
    fn the_balance_reads_and_writes_the_louder_side_at_the_volume() {
        let centred = sink(&[0.512, 0.512], &["FL", "FR"]);
        assert_eq!(balance(&centred), 0);
        assert_eq!(balanced(&centred, -30), vec![0.512, 0.512 * 0.7]);
        let left = sink(&[0.512, 0.512 * 0.7], &["FL", "FR"]);
        assert_eq!(balance(&left), -30);
        assert_eq!(say_balance(-30, Lang::French), "G 30");
        assert_eq!(say_balance(40, Lang::English), "R 40");
        assert_eq!(say_balance(0, Lang::French), "centre");
        let right = balanced(&left, 100);
        assert_eq!(right, vec![0.0, 0.512]);
        assert_eq!(balance(&sink(&right, &["FL", "FR"])), 100);
        // Five channels: the centre and the bass stay at the volume.
        let five = sink(&[0.5; 5], &["FL", "FR", "FC", "LFE", "RL"]);
        assert_eq!(balanced(&five, 50), vec![0.25, 0.5, 0.5, 0.5, 0.25]);
        // Mono: no balance.
        assert_eq!(balance(&sink(&[0.4], &["MONO"])), 0);
        // Without positions, two channels are left and right.
        assert_eq!(balance(&sink(&[0.2, 0.4], &[])), 50);
    }

    #[test]
    fn a_headset_asleep_offers_its_microphone_by_its_call_mode() {
        let card = Card {
            id: 64,
            name: "bluez_card.AA".into(),
            description: "Casque Bluetooth".into(),
            api: Some("bluez5".into()),
            profiles: vec![
                Profile {
                    index: 0,
                    name: "off".into(),
                    description: "Off".into(),
                    available: true,
                },
                Profile {
                    index: 1,
                    name: "a2dp-sink".into(),
                    description: "A2DP".into(),
                    available: true,
                },
                Profile {
                    index: 2,
                    name: "headset-head-unit".into(),
                    description: "HFP".into(),
                    available: true,
                },
            ],
            profile: Some(1),
        };
        let mut graph = Graph {
            cards: vec![card],
            ..Graph::new()
        };
        let mut headset = sink(&[0.3, 0.3], &["FL", "FR"]);
        headset.device = Some(64);
        headset.bus = Some("bluetooth".into());
        graph.sinks.push(headset.clone());
        assert_eq!(
            headsets_asleep(&graph),
            [(64, 2, "Casque Bluetooth".to_owned())]
        );
        assert_eq!(line(&headset, &graph, false, Lang::French), "Bluetooth");
        // In call mode: its microphone is there, nothing to offer.
        graph.cards[0].profile = Some(2);
        assert_eq!(
            line(&headset, &graph, false, Lang::French),
            "Bluetooth · mode appel"
        );
        let mut mic = sink(&[0.5], &["MONO"]);
        mic.device = Some(64);
        graph.sources.push(mic);
        assert!(headsets_asleep(&graph).is_empty());
        assert!(profile_line("a2dp-sink", false).is_some());
        assert_eq!(
            profile_line("headset-head-unit", false).map(|w| w.en),
            Some("Phone quality, to talk into the headset's microphone.")
        );
    }

    #[test]
    fn every_setting_of_the_page_is_found_by_its_words() {
        let ids: Vec<&str> = outside().iter().map(|o| o.id).collect();
        assert_eq!(ids.len(), 9);
        let casque = crate::search::find("casque", Lang::French);
        let found: Vec<&str> = casque.iter().map(|f| f.name).collect();
        assert_eq!(found, ["Sortie", "Entrée"], "{casque:?}");
        assert_eq!(casque[0].via.as_deref(), Some("casque"));
        let balance = crate::search::find("left", Lang::French);
        assert_eq!(balance[0].name, "Balance");
        assert_eq!(
            crate::search::find("vumetre", Lang::English)[0].name,
            "Microphone level"
        );
    }
}
