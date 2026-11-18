//! Where a track speaks, from its loudness: whisper.cpp is given the
//! speech only, packed in windows of 28 s at most (it hears 30 s at once,
//! whatever they hold), so that long silences take no time and make up no
//! words, and each window it hears is full.

use hocus_meetings::RATE;

/// A frame: 30 ms.
const FRAME: usize = RATE as usize * 30 / 1000;
/// The quietest a voice is (about -40 dBFS), whatever the room's noise.
const FLOOR: f32 = 0.01;
/// How much louder than the room's noise speech is.
const OVER_NOISE: f32 = 3.0;
/// Kept around speech; gaps shorter than this joined.
const PAD: f64 = 0.2;
const JOIN: f64 = 0.6;
/// Shorter than this is a noise, not speech.
const SHORTEST: f64 = 0.3;
/// A piece whisper.cpp hears at once.
pub const LONGEST: f64 = 28.0;

/// A stretch of a track, in seconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start: f64,
    pub end: f64,
}

impl Span {
    pub fn length(self) -> f64 {
        self.end - self.start
    }

    /// Its samples in `samples`.
    pub fn of(self, samples: &[f32]) -> &[f32] {
        let at = |t: f64| ((t * f64::from(RATE)) as usize).min(samples.len());
        samples
            .get(at(self.start)..at(self.end))
            .unwrap_or_default()
    }
}

/// The stretches of `samples` that speak.
pub fn speech(samples: &[f32]) -> Vec<Span> {
    let levels: Vec<f32> = samples
        .chunks(FRAME)
        .map(|frame| (frame.iter().map(|s| s * s).sum::<f32>() / frame.len() as f32).sqrt())
        .collect();
    if levels.is_empty() {
        return Vec::new();
    }
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let noise = sorted.get(sorted.len() / 5).copied().unwrap_or(0.0);
    let threshold = FLOOR.max(noise * OVER_NOISE);
    let frame = FRAME as f64 / f64::from(RATE);
    let total = samples.len() as f64 / f64::from(RATE);
    let mut spans: Vec<Span> = Vec::new();
    for (i, level) in levels.iter().enumerate() {
        if *level < threshold {
            continue;
        }
        let start = (i as f64 * frame - PAD).max(0.0);
        let end = ((i + 1) as f64 * frame + PAD).min(total);
        match spans.last_mut() {
            Some(last) if start - last.end < JOIN => last.end = end,
            _ => spans.push(Span { start, end }),
        }
    }
    spans.retain(|s| s.length() >= SHORTEST + 2.0 * PAD || s.length() >= total);
    spans
}

/// Silence put between two stretches of speech packed together.
const BETWEEN: f64 = 0.4;

/// Stretches of speech heard at once: whisper.cpp hears a 30 s window
/// whatever it holds, so as much speech as fits in one goes in it, the
/// silences between left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Pack {
    pub parts: Vec<Span>,
}

impl Pack {
    /// Its speech, with a short silence between its parts.
    pub fn samples(&self, track: &[f32]) -> Vec<f32> {
        let gap = vec![0.0; (BETWEEN * f64::from(RATE)) as usize];
        let mut out = Vec::new();
        for (i, part) in self.parts.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(&gap);
            }
            out.extend_from_slice(part.of(track));
        }
        out
    }

    /// How long it is, as heard.
    pub fn length(&self) -> f64 {
        let parts: f64 = self.parts.iter().map(|p| p.length()).sum();
        parts + BETWEEN * self.parts.len().saturating_sub(1) as f64
    }

    /// The track's time of `t`, a time in its samples.
    pub fn track_time(&self, t: f64) -> f64 {
        let mut at = 0.0;
        for part in &self.parts {
            if t < at + part.length() + BETWEEN {
                return part.start + (t - at).clamp(0.0, part.length());
            }
            at += part.length() + BETWEEN;
        }
        self.parts.last().map_or(t, |p| p.end)
    }
}

/// `spans` packed in windows of [`LONGEST`] at most, in order; a longer
/// one cut.
pub fn pack(spans: &[Span]) -> Vec<Pack> {
    let mut cut = Vec::new();
    for span in spans {
        let mut start = span.start;
        while span.end - start > LONGEST {
            cut.push(Span {
                start,
                end: start + LONGEST,
            });
            start += LONGEST;
        }
        cut.push(Span {
            start,
            end: span.end,
        });
    }
    let mut packs: Vec<Pack> = Vec::new();
    for span in cut {
        match packs.last_mut() {
            Some(pack) if pack.length() + BETWEEN + span.length() <= LONGEST => {
                pack.parts.push(span);
            }
            _ => packs.push(Pack { parts: vec![span] }),
        }
    }
    packs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f64, loud: f32) -> Vec<f32> {
        (0..(seconds * f64::from(RATE)) as usize)
            .map(|i| loud * (i as f32 * 0.1).sin())
            .collect()
    }

    #[test]
    fn speech_is_found_between_silences() {
        let mut samples = tone(2.0, 0.0);
        samples.extend(tone(1.5, 0.3));
        samples.extend(tone(3.0, 0.0005));
        samples.extend(tone(1.0, 0.3));
        let spans = speech(&samples);
        assert_eq!(spans.len(), 2, "{spans:?}");
        assert!((spans[0].start - 1.8).abs() < 0.05, "{spans:?}");
        assert!((spans[0].end - 3.7).abs() < 0.05, "{spans:?}");
        assert!((spans[1].start - 6.3).abs() < 0.05, "{spans:?}");
        assert!(speech(&tone(5.0, 0.0)).is_empty());
    }

    #[test]
    fn speech_is_packed_in_windows_of_28_s_and_its_times_found_back() {
        let span = |start, end| Span { start, end };
        let packs = pack(&[span(0.0, 10.0), span(30.0, 39.0), span(50.0, 120.0)]);
        // 10 and 9 s, with a silence between, then 70 s cut in three.
        assert_eq!(packs[0].parts, [span(0.0, 10.0), span(30.0, 39.0)]);
        assert!((packs[0].length() - 19.4).abs() < 1e-9);
        assert!(packs.iter().all(|p| p.length() <= LONGEST + 1e-9));
        assert_eq!(packs.len(), 4);
        // A time in the pack's samples, back on the track.
        assert!((packs[0].track_time(2.0) - 2.0).abs() < 1e-9);
        assert!((packs[0].track_time(10.4 + 1.5) - 31.5).abs() < 1e-9);
        let track = vec![0.5f32; 40 * RATE as usize];
        assert_eq!(
            packs[0].samples(&track).len(),
            (19.4 * f64::from(RATE)) as usize
        );
    }
}
