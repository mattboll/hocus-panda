//! The voices, one at a time ([`Voicer`]), each an [`Effect`]: a stream
//! of samples in, as many out, its delay fixed.
//!
//! A change of voice fades the one playing out over 5 ms, then the next
//! one in from its start, so that no click is heard; the next one is made
//! before (Sans bruit's models take some hundreds of milliseconds to
//! read), off the stream's thread.

use hocus_link::Voice;

use crate::{denoise::Denoiser, shift::Shifter, studio::Studio};

/// The fades' length, samples: 5 ms.
const FADE: usize = 240;

/// A voice effect.
pub trait Effect: Send {
    /// `io` made, in place: as many samples out as in.
    fn run(&mut self, io: &mut [f32]);
    /// How many samples it delays the voice by.
    fn latency(&self) -> usize;
    /// Back to its start: what it kept of the past forgotten.
    fn reset(&mut self);
}

/// Telle quelle: the samples as they are.
#[derive(Debug, Default)]
pub struct AsItIs;

impl Effect for AsItIs {
    fn run(&mut self, _io: &mut [f32]) {}

    fn latency(&self) -> usize {
        0
    }

    fn reset(&mut self) {}
}

/// The effect of `voice`, made: Sans bruit reads its models (some
/// hundreds of milliseconds); `None` if they cannot be read.
pub fn make(voice: Voice) -> Option<Box<dyn Effect>> {
    Some(match voice {
        Voice::AsItIs => Box::new(AsItIs),
        Voice::Quiet => match Denoiser::new() {
            Ok(denoiser) => Box::new(denoiser),
            Err(err) => {
                tracing::warn!(%err, "the noise reduction's models cannot be read");
                return None;
            }
        },
        Voice::Studio => Box::new(Studio::new()),
        Voice::Panda => Box::new(Shifter::panda()),
        Voice::Robot => Box::new(Shifter::robot()),
        Voice::Deep => Box::new(Shifter::deep()),
    })
}

/// Where the fade between two voices is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fade {
    Steady,
    /// The voice playing goes, this many samples left.
    Out(usize),
    /// The new one comes, this many samples done.
    In(usize),
}

/// The voice made now, and the next one while the change fades.
pub struct Voicer {
    voice: Voice,
    effect: Box<dyn Effect>,
    next: Option<(Voice, Box<dyn Effect>)>,
    fade: Fade,
}

impl std::fmt::Debug for Voicer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voicer")
            .field("voice", &self.voice)
            .field("next", &self.next.as_ref().map(|(v, _)| v))
            .field("fade", &self.fade)
            .finish()
    }
}

impl Default for Voicer {
    fn default() -> Self {
        Self::new(Voice::AsItIs, Box::new(AsItIs))
    }
}

impl Voicer {
    pub fn new(voice: Voice, effect: Box<dyn Effect>) -> Self {
        Self {
            voice,
            effect,
            next: None,
            fade: Fade::Steady,
        }
    }

    /// The voice heard, or coming once the fade ends.
    pub fn voice(&self) -> Voice {
        self.next.as_ref().map_or(self.voice, |(v, _)| *v)
    }

    /// The voice playing now, the change's included.
    pub fn latency(&self) -> usize {
        self.effect.latency()
    }

    /// `voice` from now on, through `effect`: the one playing fades out
    /// first.
    pub fn change(&mut self, voice: Voice, effect: Box<dyn Effect>) {
        self.next = Some((voice, effect));
        if let Fade::Steady | Fade::In(_) = self.fade {
            self.fade = Fade::Out(FADE);
        }
    }

    /// Back to the start: nothing of the past kept, no fade.
    pub fn reset(&mut self) {
        if let Some((voice, effect)) = self.next.take() {
            self.voice = voice;
            self.effect = effect;
        }
        self.effect.reset();
        self.fade = Fade::Steady;
    }

    /// `io` made by the voice, or by the fade between two.
    pub fn run(&mut self, io: &mut [f32]) {
        let mut rest = io;
        while !rest.is_empty() {
            match self.fade {
                Fade::Steady => {
                    self.effect.run(rest);
                    return;
                }
                Fade::Out(left) => {
                    let n = left.min(rest.len());
                    let (now, after) = rest.split_at_mut(n);
                    self.effect.run(now);
                    for (i, x) in now.iter_mut().enumerate() {
                        *x *= (left - i) as f32 / FADE as f32;
                    }
                    rest = after;
                    if left > n {
                        self.fade = Fade::Out(left - n);
                    } else {
                        if let Some((voice, effect)) = self.next.take() {
                            tracing::info!(?voice, latency = effect.latency(), "the voice");
                            self.voice = voice;
                            self.effect = effect;
                            self.effect.reset();
                        }
                        self.fade = Fade::In(0);
                    }
                }
                Fade::In(done) => {
                    let n = (FADE - done).min(rest.len());
                    let (now, after) = rest.split_at_mut(n);
                    self.effect.run(now);
                    for (i, x) in now.iter_mut().enumerate() {
                        *x *= (done + i) as f32 / FADE as f32;
                    }
                    rest = after;
                    self.fade = if done + n >= FADE {
                        Fade::Steady
                    } else {
                        Fade::In(done + n)
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A change fades out, then in: no jump between two samples.
    #[test]
    fn a_change_fades_without_a_click() {
        let mut voicer = Voicer::default();
        let mut io = vec![0.5f32; 64];
        voicer.run(&mut io);
        assert!(io.iter().all(|&x| x == 0.5));
        voicer.change(Voice::AsItIs, Box::new(AsItIs));
        let mut all = Vec::new();
        for _ in 0..20 {
            let mut io = vec![0.5f32; 37];
            voicer.run(&mut io);
            all.extend(io);
        }
        let steps = all.windows(2).map(|w| (w[1] - w[0]).abs());
        assert!(steps.fold(0f32, f32::max) < 0.01);
        assert!(all.iter().any(|&x| x < 0.01), "it went down");
        assert_eq!(*all.last().unwrap(), 0.5);
    }
}
