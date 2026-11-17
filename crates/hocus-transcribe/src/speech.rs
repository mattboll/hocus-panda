//! Where a track speaks, from its loudness: whisper.cpp is given the
//! speech only, in pieces of 28 s at most (its window is 30 s), so that
//! long silences take no time and make up no words.

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

/// `spans` in pieces whisper.cpp hears at once: neighbours together while
/// they fit in [`LONGEST`], a longer one cut.
pub fn pieces(spans: &[Span]) -> Vec<Span> {
    let mut out: Vec<Span> = Vec::new();
    for span in spans {
        let mut start = span.start;
        while span.end - start > LONGEST {
            out.push(Span {
                start,
                end: start + LONGEST,
            });
            start += LONGEST;
        }
        let rest = Span {
            start,
            end: span.end,
        };
        match out.last_mut() {
            Some(last) if rest.end - last.start <= LONGEST && rest.start - last.end < 2.0 => {
                last.end = rest.end;
            }
            _ => out.push(rest),
        }
    }
    out
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
    fn pieces_hold_28_s_at_most() {
        let spans = [
            Span {
                start: 0.0,
                end: 10.0,
            },
            Span {
                start: 11.0,
                end: 20.0,
            },
            Span {
                start: 40.0,
                end: 100.0,
            },
        ];
        let pieces = pieces(&spans);
        assert_eq!(
            pieces[0],
            Span {
                start: 0.0,
                end: 20.0
            }
        );
        assert!(pieces.iter().all(|p| p.length() <= LONGEST + 1e-9));
        assert_eq!(pieces.last().map(|p| p.end), Some(100.0));
        assert_eq!(pieces.len(), 4);
    }
}
