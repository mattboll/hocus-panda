//! The voices on fixed samples: the tests' reading (`data/speech.wav`),
//! a noise of fixed seed, and a voiced vowel made here; each voice's time
//! on the processor.

use std::time::Duration;

use hocus_link::Voice;

use crate::{RATE, effect, wav::test_speech};

/// A white noise of fixed seed, `rms` strong.
pub fn noise(len: usize, rms: f32) -> Vec<f32> {
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            // Uniform from -1 to 1 has an RMS of 1/sqrt(3).
            ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * rms * 3f32.sqrt()
        })
        .collect()
}

pub fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// A vowel ("a") sung at `f0`: a pulse a period, through three formants.
pub fn vowel(f0: f32, len: usize) -> Vec<f32> {
    let period = RATE as f32 / f0;
    let pulses: Vec<f32> = (0..len)
        .map(|i| {
            let phase = (i as f32 / period).fract();
            // A glottal pulse: a short rise and fall in each period.
            if phase < 0.4 {
                (std::f32::consts::PI * phase / 0.4).sin().powi(2)
            } else {
                0.0
            }
        })
        .collect();
    let mut out = vec![0.0; len];
    for (hz, bw, g) in [
        (730.0, 90.0, 1.0),
        (1090.0, 110.0, 0.5),
        (2440.0, 170.0, 0.25),
    ] {
        let r = (-std::f32::consts::PI * bw / RATE as f32).exp();
        let c = 2.0 * r * (2.0 * std::f32::consts::PI * hz / RATE as f32).cos();
        let (mut y1, mut y2) = (0.0f32, 0.0f32);
        for (o, x) in out.iter_mut().zip(&pulses) {
            let y = (1.0 - r) * x + c * y1 - r * r * y2;
            y2 = y1;
            y1 = y;
            *o += g * y;
        }
    }
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs())).max(1e-9);
    for o in &mut out {
        *o *= 0.3 / peak;
    }
    out
}

/// The fundamental of `x`, hertz, by YIN (de Cheveigné and Kawahara):
/// the first lag whose normalised difference dips under 0.15.
pub fn pitch(x: &[f32]) -> f32 {
    let (min, max) = (RATE / 500, RATE / 40);
    let w = 2048.min(x.len().saturating_sub(max));
    let d: Vec<f32> = (0..=max)
        .map(|tau| (0..w).map(|i| (x[i] - x[i + tau]).powi(2)).sum())
        .collect();
    let mut sum = 0.0;
    let cmnd: Vec<f32> = d
        .iter()
        .enumerate()
        .map(|(tau, v)| {
            if tau == 0 {
                return 1.0;
            }
            sum += v;
            v * tau as f32 / sum.max(1e-12)
        })
        .collect();
    let mut tau = min;
    while tau < max {
        if cmnd[tau] < 0.15 {
            while tau + 1 < max && cmnd[tau + 1] < cmnd[tau] {
                tau += 1;
            }
            break;
        }
        tau += 1;
    }
    // Between samples.
    let (a, b, c) = (cmnd[tau - 1], cmnd[tau], cmnd[tau + 1]);
    let shift = 0.5 * (a - c) / (a - 2.0 * b + c).max(1e-9);
    RATE as f32 / (tau as f32 + shift.clamp(-0.5, 0.5))
}

/// How much later `y` is than `x`, samples, up to `most`: the lag of their
/// largest correlation.
pub fn delay(x: &[f32], y: &[f32], most: usize) -> usize {
    (0..most)
        .max_by(|&a, &b| {
            let c =
                |lag: usize| -> f32 { x.iter().zip(y.iter().skip(lag)).map(|(p, q)| p * q).sum() };
            c(a).total_cmp(&c(b))
        })
        .unwrap_or(0)
}

/// The scale-invariant signal to noise ratio of `estimate` against
/// `clean`, dB.
pub fn si_snr(clean: &[f32], estimate: &[f32]) -> f32 {
    let dot: f32 = clean.iter().zip(estimate).map(|(c, e)| c * e).sum();
    let energy: f32 = clean.iter().map(|c| c * c).sum();
    let scale = dot / energy.max(1e-12);
    let (mut target, mut error) = (0.0f32, 0.0f32);
    for (c, e) in clean.iter().zip(estimate) {
        let t = scale * c;
        target += t * t;
        error += (e - t) * (e - t);
    }
    10.0 * (target / error.max(1e-12)).log10()
}

fn thread_time() -> Duration {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// `voice` made of `input` in quanta of 256 samples (a stream's), and the
/// processor time it took.
pub fn through(voice: Voice, input: &[f32]) -> (Vec<f32>, Duration) {
    let mut effect = effect::make(voice).expect("the voice is made");
    effect.reset();
    let mut out = input.to_vec();
    let start = thread_time();
    for chunk in out.chunks_mut(256) {
        effect.run(chunk);
    }
    (out, thread_time() - start)
}

/// Telle quelle gives the samples back as they are.
#[test]
fn as_it_is_gives_the_voice_back() {
    let speech = test_speech();
    let (out, _) = through(Voice::AsItIs, &speech);
    assert_eq!(out, speech);
}

/// Sans bruit on the reading with a white noise 5 dB under it: the noise
/// goes, the voice stays; its delay is the one it says.
#[test]
fn sans_bruit_takes_the_noise_out() {
    let speech = test_speech();
    let hiss = noise(speech.len(), rms(&speech) * 10f32.powf(-5.0 / 20.0));
    let noisy: Vec<f32> = speech.iter().zip(&hiss).map(|(s, n)| s + n).collect();
    let (out, _) = through(Voice::Quiet, &noisy);
    let lag = delay(&speech, &out, 4000);
    assert_eq!(lag, crate::denoise::LATENCY, "its delay");
    // After the models' first second, against the clean reading.
    let skip = RATE;
    let clean = &speech[skip..speech.len() - lag];
    let made = &out[skip + lag..];
    let before = si_snr(clean, &noisy[skip..speech.len() - lag]);
    let after = si_snr(clean, made);
    eprintln!("SI-SNR {before:.1} dB noisy, {after:.1} dB without noise");
    assert!(after - before > 6.0, "{before:.1} dB to {after:.1} dB");
    // Where she is silent, the noise is 20 dB down at least.
}

/// Panda raises a vowel by a fifth, Grave lowers it by an octave; Robot
/// sings it on its one note.
#[test]
fn the_voices_move_the_pitch_by_their_ratio() {
    let f0 = 140.0;
    let sung = vowel(f0, RATE);
    let mid = |x: &[f32]| x[RATE / 2..].to_vec();
    assert!(
        (pitch(&mid(&sung)) - f0).abs() < 2.0,
        "{}",
        pitch(&mid(&sung))
    );
    for (voice, want) in [
        (Voice::Panda, f0 * crate::shift::PANDA),
        (Voice::Deep, f0 * crate::shift::DEEP),
        (Voice::Robot, RATE as f32 / crate::shift::ROBOT_HOP as f32),
    ] {
        let (out, _) = through(voice, &sung);
        let got = pitch(&mid(&out));
        eprintln!("{voice:?}: {got:.1} Hz, {want:.1} wanted");
        assert!(
            (got / want - 1.0).abs() < 0.03,
            "{voice:?}: {got} Hz, not {want}"
        );
        // As loud as it came.
        let level = rms(&mid(&out)) / rms(&mid(&sung));
        assert!((0.5..2.0).contains(&level), "{voice:?}: {level}");
    }
}

/// Each voice takes a part of one processor's time, on the reading under
/// a noise (Sans bruit's models then run on every frame): its thread's
/// time, and the time it took on the clock (no other thread helps it).
#[test]
fn each_voice_keeps_within_its_budget() {
    let speech = test_speech();
    let hiss = noise(speech.len(), rms(&speech) * 0.3);
    let noisy: Vec<f32> = speech.iter().zip(&hiss).map(|(s, n)| s + n).collect();
    let length = Duration::from_secs_f32(speech.len() as f32 / RATE as f32);
    for voice in Voice::ALL {
        let start = std::time::Instant::now();
        let (_, took) = through(voice, &noisy);
        let wall = start.elapsed();
        let part = took.as_secs_f32() / length.as_secs_f32();
        let clock = wall.as_secs_f32() / length.as_secs_f32();
        eprintln!(
            "{voice:?}: {:.1} % of one processor, {:.1} % of the time on the clock",
            part * 100.0,
            clock * 100.0
        );
        assert!(part < 0.5, "{voice:?}: {:.0} %", part * 100.0);
    }
}

/// The maintainer's listening samples: a reading (`HOCUS_VOICE_INPUT`, a
/// 48 kHz WAV; the tests' own else) through each voice, as WAV files in
/// `HOCUS_VOICE_SAMPLES`; Sans bruit also on the reading under a noise.
#[test]
#[ignore = "writes the listening samples where HOCUS_VOICE_SAMPLES says"]
fn listening_samples() {
    let Some(dir) = std::env::var_os("HOCUS_VOICE_SAMPLES") else {
        panic!("HOCUS_VOICE_SAMPLES names the folder they go in");
    };
    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).unwrap();
    let reading = match std::env::var_os("HOCUS_VOICE_INPUT") {
        Some(path) => {
            let (samples, rate) = crate::wav::read(&std::fs::read(path).unwrap()).unwrap();
            assert_eq!(rate as usize, RATE, "a 48 kHz file");
            samples
        }
        None => test_speech(),
    };
    let write = |name: &str, samples: &[f32]| {
        std::fs::write(dir.join(name), crate::wav::write(samples, RATE as u32)).unwrap();
    };
    write("0-input.wav", &reading);
    for voice in Voice::ALL {
        let (out, took) = through(voice, &reading);
        eprintln!(
            "{voice:?}: {:.1} % of one processor",
            100.0 * took.as_secs_f32() * RATE as f32 / reading.len() as f32
        );
        write(&format!("{}.wav", voice.word()), &out);
    }
    let hiss = noise(reading.len(), rms(&reading) * 10f32.powf(-10.0 / 20.0));
    let noisy: Vec<f32> = reading.iter().zip(&hiss).map(|(s, n)| s + n).collect();
    write("noisy-input.wav", &noisy);
    let (out, _) = through(Voice::Quiet, &noisy);
    write("noisy-quiet.wav", &out);
}
