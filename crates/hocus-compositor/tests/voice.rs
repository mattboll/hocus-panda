//! The Hocuspanda microphone (#619, lot 5, docs/design/voice.md): the
//! compositor starts `hocus-voice` (built beside the test) against a
//! PipeWire of the test's own, in a fresh short runtime directory, with a
//! fake microphone of the test's: an `Audio/Source` node playing a tape,
//! never the machine's microphones nor the session's PipeWire. The call
//! is `hocus_voice::listen`, linked to the source by itself as an
//! application's stream would be by a session manager.
//!
//! One test, the source's path from end to end: the microphone it would
//! read said (a meeting's transcription records that one), and read only
//! while the source is; Telle quelle gives the samples as they are, within
//! 20 ms; then, changed live, Sans bruit takes the noise out of a noisy
//! reading, Panda and Grave move a vowel's pitch by their ratios. A
//! quantum the graph loses on the way, on a machine too busy to run it in
//! time, moves what the call hears after it, and is followed (#880). The
//! voices' arithmetic and their time on the processor: `hocus-voice`'s
//! unit tests. Another: the ports set up again, as a session manager does
//! (#911), the microphone is linked again.

use crate::common;
use crate::sharing::Pipewire;

use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use common::wait;
use hocus_compositor::{Config, Headless, HeadlessConfig, RendererChoice};
use hocus_link::Voice;

const RATE: usize = 48_000;
const MIC: &str = "hocus-test-mic";

/// What the fake microphone plays: a tape from its start, or silence with
/// a click when asked.
#[derive(Default)]
struct Deck {
    tape: Vec<f32>,
    at: usize,
    /// A click asked: the next quantum starts with one, and when it went.
    click: bool,
    clicked: Option<Instant>,
}

/// The fake microphone: a source node on `remote`, as PipeWire's would be,
/// playing its deck while something reads it.
pub(crate) struct FakeMic {
    deck: Arc<Mutex<Deck>>,
    streaming: Arc<AtomicBool>,
    quit: pipewire::channel::Sender<()>,
}

impl Drop for FakeMic {
    fn drop(&mut self) {
        let _ = self.quit.send(());
    }
}

impl FakeMic {
    pub(crate) fn play(&self, tape: Vec<f32>) {
        let mut deck = self.deck.lock().unwrap_or_else(PoisonError::into_inner);
        deck.tape = tape;
        deck.at = 0;
    }

    fn click(&self) {
        let mut deck = self.deck.lock().unwrap_or_else(PoisonError::into_inner);
        deck.click = true;
        deck.clicked = None;
    }

    fn clicked(&self) -> Option<Instant> {
        self.deck
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clicked
    }

    fn streams(&self) -> bool {
        self.streaming.load(Ordering::SeqCst)
    }
}

pub(crate) fn fake_mic(remote: &std::path::Path) -> FakeMic {
    use pipewire as pw;
    use pw::spa;
    let deck = Arc::new(Mutex::new(Deck::default()));
    let streaming = Arc::new(AtomicBool::new(false));
    let (quit, quitting) = pw::channel::channel::<()>();
    let remote = remote.to_string_lossy().into_owned();
    let (playing, flag) = (deck.clone(), streaming.clone());
    std::thread::spawn(move || -> Result<(), pw::Error> {
        pw::init();
        let mainloop = pw::main_loop::MainLoopRc::new(None)?;
        let context = pw::context::ContextRc::new(&mainloop, None)?;
        let core = context.connect_rc(Some(pw::properties::properties! {
            *pw::keys::REMOTE_NAME => remote,
        }))?;
        let stream = pw::stream::StreamRc::new(
            core,
            MIC,
            pw::properties::properties! {
                *pw::keys::MEDIA_CLASS => "Audio/Source",
                *pw::keys::NODE_NAME => MIC,
                *pw::keys::NODE_DESCRIPTION => "Micro de test",
                "adapter.auto-port-config" => "{ mode = dsp monitor = false position = preserve }",
            },
        )?;
        let on = flag.clone();
        let _listener = stream
            .add_local_listener::<()>()
            .state_changed(move |_, _, _, new| {
                on.store(new == pw::stream::StreamState::Streaming, Ordering::SeqCst);
            })
            .process(move |stream, _| {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    return;
                };
                let mut deck = playing.lock().unwrap_or_else(PoisonError::into_inner);
                let mut first = true;
                hocus_voice::serve::samples_out(&mut buffer, |out| {
                    for o in out.iter_mut() {
                        *o = if deck.tape.is_empty() {
                            0.0
                        } else {
                            let v = deck.tape[deck.at % deck.tape.len()];
                            deck.at += 1;
                            v
                        };
                    }
                    if first && deck.click {
                        out[0] = 0.9;
                        deck.click = false;
                        deck.clicked = Some(Instant::now());
                    }
                    first = false;
                });
            })
            .register()?;
        let pod = hocus_voice::serve::format_pod();
        let Some(pod) = spa::pod::Pod::from_bytes(&pod) else {
            return Err(pw::Error::CreationFailed);
        };
        stream.connect(
            spa::utils::Direction::Output,
            None,
            pw::stream::StreamFlags::MAP_BUFFERS,
            &mut [pod],
        )?;
        let weak = mainloop.downgrade();
        let _quit = quitting.attach(mainloop.loop_(), move |()| {
            if let Some(mainloop) = weak.upgrade() {
                mainloop.quit();
            }
        });
        mainloop.run();
        flag.store(false, Ordering::SeqCst);
        Ok(())
    });
    FakeMic {
        deck,
        streaming,
        quit,
    }
}

/// What the call heard, and when it heard a click.
#[derive(Default)]
struct Heard {
    samples: Vec<f32>,
    click: Option<Instant>,
}

/// The call: a reader of the Hocuspanda source, as an application's.
pub(crate) struct Call {
    _listener: hocus_voice::listen::Listener,
    heard: Arc<Mutex<Heard>>,
}

impl Call {
    pub(crate) fn start(remote: &std::path::Path) -> anyhow::Result<Self> {
        let heard = Arc::new(Mutex::new(Heard::default()));
        let into = heard.clone();
        let listener = hocus_voice::listen::start_as(
            Some(remote.to_owned()),
            ("hocus-test-call", "Visio"),
            move |samples| {
                let mut heard = into.lock().unwrap_or_else(PoisonError::into_inner);
                if heard.click.is_none()
                    && let Some(at) = samples.iter().position(|s| s.abs() > 0.5)
                {
                    // When its sample was: the quantum's start plus its place.
                    heard.click =
                        Some(Instant::now() + Duration::from_secs_f64(at as f64 / RATE as f64));
                }
                heard.samples.extend_from_slice(samples);
            },
        )
        .ok_or_else(|| anyhow::anyhow!("no call"))?;
        Ok(Self {
            _listener: listener,
            heard,
        })
    }

    fn take(&self) -> Vec<f32> {
        std::mem::take(
            &mut self
                .heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .samples,
        )
    }

    fn len(&self) -> usize {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .samples
            .len()
    }

    fn forget_click(&self) {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .click = None;
    }

    fn click(&self) -> Option<Instant> {
        self.heard
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .click
    }
}

struct Bed {
    hp: Headless,
    mic: FakeMic,
    pipewire: Pipewire,
    config: Config,
}

fn start() -> anyhow::Result<Bed> {
    let program = common::voice_program();
    if !std::process::Command::new("sh")
        .args(["-c", "command -v pipewire"])
        .stdout(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
    {
        common::missing("pipewire");
    }
    let pipewire = Pipewire::start("unix:path=/nonexistent")?;
    let mic = fake_mic(&pipewire.socket);
    let mut config = Config {
        plain_background: true,
        pipewire_remote: Some(pipewire.socket.clone()),
        voice_program: Some(program),
        ..Config::default()
    };
    config.voice.effect = Voice::AsItIs;
    config.keyboard.layouts = vec!["us".to_owned()];
    let hp = Headless::start_with(
        HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config.clone(),
    )?;
    Ok(Bed {
        hp,
        mic,
        pipewire,
        config,
    })
}

impl Bed {
    /// Runs until `done`, or fails saying `what`.
    fn until(
        &mut self,
        within: Duration,
        what: &str,
        mut done: impl FnMut(&mut Self) -> bool,
    ) -> anyhow::Result<()> {
        let start = Instant::now();
        while start.elapsed() < within {
            if done(self) {
                return Ok(());
            }
            wait(&mut self.hp, Duration::from_millis(20))?;
        }
        anyhow::bail!("{what}: {:?}", self.hp.voice())
    }

    /// `voice` from now on, as the file would say it; what the call
    /// heard of it, `seconds` long, the change's fade over.
    fn hear(
        &mut self,
        call: &Call,
        voice: Voice,
        tape: Vec<f32>,
        seconds: f32,
    ) -> anyhow::Result<Vec<f32>> {
        self.config.voice.effect = voice;
        self.hp.reconfigure(self.config.clone());
        // Sans bruit's models are read when it is asked; then the fade.
        if voice != Voice::AsItIs {
            self.until(Duration::from_secs(30), "the voice not made", |bed| {
                bed.hp.voice_made() == Some(voice)
            })?;
        }
        wait(&mut self.hp, Duration::from_millis(500))?;
        self.mic.play(tape);
        call.take();
        let want = (seconds * RATE as f32) as usize;
        self.until(Duration::from_secs(30), "the call heard too little", |_| {
            call.len() >= want
        })?;
        Ok(call.take())
    }
}

/// The lag of `heard` after `tape`'s start, up to `most`: where their
/// correlation over a second is largest.
fn lag(tape: &[f32], heard: &[f32], most: usize) -> usize {
    let window = &tape[..RATE];
    let corr = |lag: usize| -> f32 {
        window
            .iter()
            .zip(&heard[lag..])
            .map(|(a, b)| a * b)
            .sum::<f32>()
    };
    (0..most)
        .max_by(|&a, &b| corr(a).total_cmp(&corr(b)))
        .unwrap_or(0)
}

/// A run of samples that places what was heard in the tape: 64 (1.3 ms).
const RUN: usize = 64;
/// The windows the SI-SNR follows the lag by: 50 ms.
const WINDOW: usize = RATE / 20;

/// Each sample of `heard` from `at` on against `tape`'s from its start:
/// the largest difference, how many samples were the tape's, and how many
/// times the call's place in the tape moved. A machine too busy to run
/// the graph in time (no real-time scheduling, every core taken: CI's)
/// loses quanta on the way, between the fake microphone, the voice and
/// the call, and the voice says silence for those it was not given
/// (#880): what follows comes earlier or later, which no voice does. So
/// the silence it says is skipped, and a sample that is not the tape's
/// where the one before was is looked for anywhere in it, by the run it
/// starts; one that is the tape's nowhere is the voice's doing.
fn as_it_was(tape: &[f32], heard: &[f32], at: usize) -> (f32, usize, usize) {
    let (mut offset, mut worst, mut same, mut moved) = (at as isize, 0f32, 0, 0);
    let same_at = |h: usize, t: usize| {
        (0..RUN).all(|i| {
            heard
                .get(h + i)
                .zip(tape.get(t + i))
                .is_some_and(|(a, b)| (a - b).abs() < 1e-5)
        })
    };
    for (h, x) in heard.iter().enumerate().skip(at) {
        if *x == 0.0 {
            continue;
        }
        let d = usize::try_from(h as isize - offset)
            .ok()
            .and_then(|t| tape.get(t))
            .map_or(f32::INFINITY, |t| (x - t).abs());
        if d < 1e-5 {
            same += 1;
            continue;
        }
        match (0..tape.len()).find(|t| same_at(h, *t)) {
            Some(t) => {
                offset = h as isize - t as isize;
                same += 1;
                moved += 1;
            }
            // The tape's end, or the voice's doing.
            None if h + RUN > heard.len() => break,
            None => worst = worst.max(d),
        }
    }
    (worst, same, moved)
}

/// `heard` along `tape[from..]`, 50 ms at a time, each window at the lag
/// the one before had; where the voice speaks and what is heard there is
/// not the tape's (scaled: 1 - ρ² over a tenth), at the lag within a
/// quarter of a second either way where it is the most, if that is half
/// as far or less (a voice's period may be near a quantum's). A window
/// of the silence the voice said for quanta it was not given (a run of
/// zeros where the tape speaks) is left out, and so are the two around a
/// move, which the voice made of a reading cut ([`as_it_was`], #880). Where
/// the windows kept start in the tape, what was heard of them in a row,
/// and how many times the lag moved.
fn follow(tape: &[f32], heard: &[f32], lag: usize, from: usize) -> (Vec<usize>, Vec<f32>, usize) {
    const NEAR: isize = RATE as isize / 4;
    let energy = |x: &[f32]| x.iter().map(|v| v * v).sum::<f32>();
    let rest = tape.get(from..).unwrap_or_default();
    let speaks = 0.1 * energy(rest) * WINDOW as f32 / rest.len().max(1) as f32;
    let (mut lag, mut moved) = (lag as isize, 0);
    let (mut kept_at, mut heard_out) = (Vec::new(), Vec::new());
    let mut t = from;
    while t + WINDOW <= tape.len() {
        let window = &tape[t..t + WINDOW];
        let at = |l: isize| {
            let h = usize::try_from(t as isize + l).ok()?;
            heard.get(h..h + WINDOW)
        };
        let apart = |l: isize| {
            at(l).map(|w| {
                let c: f32 = window.iter().zip(w).map(|(a, b)| a * b).sum();
                1.0 - c * c.abs() / (energy(window) * energy(w)).max(1e-12)
            })
        };
        let Some(kept) = apart(lag) else {
            break;
        };
        let speaking = energy(window) > speaks;
        if speaking && kept > 0.1 {
            let was = lag;
            let mut best = 0.5 * kept;
            for l in was - NEAR..=was + NEAR {
                if let Some(d) = apart(l)
                    && d < best
                {
                    (best, lag) = (d, l);
                }
            }
            if lag != was {
                moved += 1;
                // The quanta lost were between this window and the one
                // before: the voice made both of a reading cut there.
                if kept_at.last() == Some(&(t - WINDOW)) {
                    kept_at.pop();
                    heard_out.truncate(heard_out.len() - WINDOW);
                }
                t += WINDOW;
                continue;
            }
        }
        let w = at(lag).unwrap_or_default();
        let gap = w.windows(RUN).any(|r| r.iter().all(|v| *v == 0.0));
        if !(speaking && gap) {
            kept_at.push(t);
            heard_out.extend_from_slice(w);
        }
        t += WINDOW;
    }
    (kept_at, heard_out, moved)
}

/// The fundamental of `x` by YIN, hertz.
fn pitch(x: &[f32]) -> f32 {
    let (min, max) = (RATE / 500, RATE / 40);
    let w = 2048;
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
    while tau < max && cmnd[tau] >= 0.15 {
        tau += 1;
    }
    while tau + 1 < max && cmnd[tau + 1] < cmnd[tau] {
        tau += 1;
    }
    RATE as f32 / tau as f32
}

/// A vowel ("a") at `f0`: a pulse a period, through three formants.
fn vowel(f0: f32, len: usize) -> Vec<f32> {
    let period = RATE as f32 / f0;
    let mut out = vec![0f32; len];
    for (hz, bw, g) in [
        (730.0, 90.0, 1.0),
        (1090.0, 110.0, 0.5),
        (2440.0, 170.0, 0.25),
    ] {
        let r = (-std::f32::consts::PI * bw / RATE as f32).exp();
        let c = 2.0 * r * (2.0 * std::f32::consts::PI * hz / RATE as f32).cos();
        let (mut y1, mut y2) = (0f32, 0f32);
        for (i, o) in out.iter_mut().enumerate() {
            let phase = (i as f32 / period).fract();
            let x = if phase < 0.4 {
                (std::f32::consts::PI * phase / 0.4).sin().powi(2)
            } else {
                0.0
            };
            let y = (1.0 - r) * x + c * y1 - r * r * y2;
            y2 = y1;
            y1 = y;
            *o += g * y;
        }
    }
    let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
    out.iter().map(|v| v * 0.3 / peak).collect()
}

/// A white noise of fixed seed, `rms` strong.
fn noise(len: usize, rms: f32) -> Vec<f32> {
    let mut state = 0x9E37_79B9_7F4A_7C15u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ((state >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0) * rms * 3f32.sqrt()
        })
        .collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// The scale-invariant SNR of `estimate` against `clean`, dB.
fn si_snr(clean: &[f32], estimate: &[f32]) -> f32 {
    let dot: f32 = clean.iter().zip(estimate).map(|(c, e)| c * e).sum();
    let energy: f32 = clean.iter().map(|c| c * c).sum();
    let scale = dot / energy.max(1e-12);
    let (mut target, mut error) = (0f32, 0f32);
    for (c, e) in clean.iter().zip(estimate) {
        let t = scale * c;
        target += t * t;
        error += (e - t) * (e - t);
    }
    10.0 * (target / error.max(1e-12)).log10()
}

#[test]
fn the_call_hears_the_microphone_through_the_voice_and_it_is_read_only_meanwhile()
-> anyhow::Result<()> {
    let mut bed = start()?;
    let speech = hocus_voice::wav::test_speech();
    // On the graph, the microphone it would read said: what a meeting's
    // transcription records, never the Hocuspanda one.
    bed.until(Duration::from_secs(20), "not on the graph", |bed| {
        let (ready, _, mic) = bed.hp.voice();
        ready && mic.as_deref() == Some(MIC)
    })?;
    // Unread: the microphone is not read.
    let unread = Instant::now();
    while unread.elapsed() < Duration::from_secs(2) {
        wait(&mut bed.hp, Duration::from_millis(50))?;
        anyhow::ensure!(!bed.mic.streams(), "the microphone is read for nothing");
    }

    // A call reads it: the microphone too, and the call hears it as it is.
    let call = Call::start(&bed.pipewire.socket)?;
    bed.until(Duration::from_secs(10), "not read", |bed| {
        bed.hp.voice().1 && bed.mic.streams()
    })?;
    let heard = bed.hear(&call, Voice::AsItIs, speech.clone(), 2.5)?;
    let first = |x: &[f32]| x.iter().position(|s| s.abs() > 0.01).unwrap_or(0);
    let at = first(&heard) - first(&speech);
    let (worst, same, moved) = as_it_was(&speech, &heard, at);
    eprintln!("Telle quelle: {same} samples as they were, the place moved {moved} times");
    assert!(worst < 1e-5, "Telle quelle changed the samples by {worst}");
    assert!(same >= RATE, "only {same} samples heard as they were");

    // Its delay: a click from the microphone, heard by the call.
    bed.mic.play(Vec::new());
    let mut worst = Duration::ZERO;
    for _ in 0..5 {
        call.forget_click();
        bed.mic.click();
        bed.until(Duration::from_secs(5), "no click heard", |_| {
            call.click().is_some()
        })?;
        let (Some(went), Some(came)) = (bed.mic.clicked(), call.click()) else {
            anyhow::bail!("no click");
        };
        worst = worst.max(came.saturating_duration_since(went));
        wait(&mut bed.hp, Duration::from_millis(200))?;
    }
    eprintln!(
        "Telle quelle: {:.1} ms from the microphone to the call",
        worst.as_secs_f64() * 1e3
    );
    assert!(worst <= Duration::from_millis(20), "{worst:?}");

    // Sans bruit, chosen while the call reads: a reading under a white
    // noise 5 dB down comes out with the noise taken out.
    let hiss = noise(speech.len(), rms(&speech) * 10f32.powf(-5.0 / 20.0));
    let noisy: Vec<f32> = speech.iter().zip(&hiss).map(|(s, n)| s + n).collect();
    let heard = bed.hear(&call, Voice::Quiet, noisy.clone(), 3.0)?;
    let at = lag(&speech, &heard, RATE);
    let (kept, heard, moved) = follow(&speech, &heard, at, RATE);
    anyhow::ensure!(heard.len() >= RATE, "{} samples followed", heard.len());
    // The same windows of the tape, and of the noisy reading.
    let of = |x: &[f32]| -> Vec<f32> {
        kept.iter()
            .flat_map(|t| &x[*t..t + WINDOW])
            .copied()
            .collect()
    };
    let said = of(&speech);
    let before = si_snr(&said, &of(&noisy));
    let after = si_snr(&said, &heard);
    eprintln!(
        "Sans bruit: SI-SNR {before:.1} dB in, {after:.1} dB heard, the lag moved {moved} times"
    );
    assert!(after - before > 6.0, "{before:.1} dB to {after:.1} dB");

    // Panda and Grave move a vowel's pitch: a fifth up, an octave down.
    let sung = vowel(140.0, 3 * RATE);
    for (voice, want) in [(Voice::Panda, 210.0), (Voice::Deep, 70.0)] {
        let heard = bed.hear(&call, voice, sung.clone(), 1.5)?;
        let got = pitch(&heard[heard.len() - RATE / 2 - 2048..]);
        eprintln!("{voice:?}: {got:.1} Hz heard, {want} wanted");
        assert!((got / want - 1.0).abs() < 0.03, "{voice:?}: {got} Hz");
    }

    // The call goes: so does the microphone.
    drop(call);
    bed.until(
        Duration::from_secs(10),
        "the microphone is still read",
        |bed| !bed.mic.streams() && !bed.hp.voice().1,
    )?;
    Ok(())
}

/// A session manager sets each node's ports up again once it is on the
/// graph: WirePlumber does, for the voice's stream, a few milliseconds
/// after the voice linked it to the microphone (its ports made again, a
/// monitor port with them), and the link goes with the old ports. The
/// voice links the microphone again: the call hears it again (#911:
/// silent, never linked again).
#[test]
fn the_microphone_is_linked_again_when_its_ports_are_set_up_again() -> anyhow::Result<()> {
    let mut bed = start()?;
    bed.until(Duration::from_secs(20), "not on the graph", |bed| {
        let (ready, _, mic) = bed.hp.voice();
        ready && mic.as_deref() == Some(MIC)
    })?;
    let call = Call::start(&bed.pipewire.socket)?;
    bed.until(Duration::from_secs(10), "not read", |bed| {
        bed.hp.voice().1 && bed.mic.streams()
    })?;
    let tape = vowel(140.0, RATE);
    let heard = bed.hear(&call, Voice::AsItIs, tape.clone(), 0.5)?;
    anyhow::ensure!(rms(&heard) > 0.01, "the call hears nothing");

    set_up_again(&bed.pipewire.socket, hocus_link::voice::VOICE_INPUT_NODE)?;
    let heard = bed.hear(&call, Voice::AsItIs, tape, 1.0)?;
    let last = &heard[heard.len() / 2..];
    anyhow::ensure!(
        rms(last) > 0.01,
        "the call hears nothing once the stream's ports are set up again ({})",
        rms(last)
    );
    Ok(())
}

/// The input stream `node`'s ports set up again as WirePlumber's audio
/// adapter does: the node suspended, then in DSP mode, mono, with a
/// monitor port. A node that runs keeps its ports, and WirePlumber's
/// comes in the milliseconds before the link to it runs: here, the links
/// to it are undone first, in the same go.
fn set_up_again(remote: &std::path::Path, node: &'static str) -> anyhow::Result<()> {
    use pipewire as pw;
    use pw::spa::node::command::{NodeCommand, NodeCommandId};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    #[derive(Default)]
    struct Seen {
        node: Option<(u32, pw::node::Node)>,
        /// Each link, and the node it goes into.
        links: Vec<(u32, u32)>,
        listed: Option<i32>,
        done: Option<i32>,
    }

    let pod = port_config()?;
    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(Some(pw::properties::properties! {
        *pw::keys::REMOTE_NAME => remote.to_string_lossy().into_owned(),
    }))?;
    let registry = core.get_registry_rc()?;
    let seen = Rc::new(RefCell::new(Seen::default()));
    let (binder, seeing) = (registry.clone(), seen.clone());
    let _registry = registry
        .add_listener_local()
        .global(move |global| {
            let Some(props) = global.props else {
                return;
            };
            let mut seen = seeing.borrow_mut();
            match global.type_ {
                pw::types::ObjectType::Node if props.get("node.name") == Some(node) => {
                    if let Ok(proxy) = binder.bind::<pw::node::Node, _>(global) {
                        seen.node = Some((global.id, proxy));
                    }
                }
                pw::types::ObjectType::Link => {
                    if let Some(to) = props.get("link.input.node").and_then(|n| n.parse().ok()) {
                        seen.links.push((global.id, to));
                    }
                }
                _ => {}
            }
        })
        .register();
    seen.borrow_mut().listed = Some(core.sync(0)?.seq());
    let finished = Rc::new(Cell::new(false));
    let (quit, done, undoing, syncing, setting) = (
        mainloop.downgrade(),
        finished.clone(),
        registry.clone(),
        core.clone(),
        seen.clone(),
    );
    let _done = core
        .add_listener_local()
        .done(move |id, answered| {
            if id != pw::core::PW_ID_CORE {
                return;
            }
            let mut seen = setting.borrow_mut();
            if seen.listed == Some(answered.seq()) {
                let Some((id, proxy)) = &seen.node else {
                    return;
                };
                for (link, _) in seen.links.iter().filter(|(_, to)| to == id) {
                    let _ = undoing.destroy_global(*link);
                }
                proxy.send_command(&NodeCommand::new(NodeCommandId::SUSPEND));
                if let Some(param) = pw::spa::pod::Pod::from_bytes(&pod) {
                    proxy.set_param(pw::spa::param::ParamType::PortConfig, 0, param);
                }
                seen.done = syncing.sync(0).ok().map(|s| s.seq());
            } else if seen.done == Some(answered.seq()) {
                done.set(true);
                if let Some(mainloop) = quit.upgrade() {
                    mainloop.quit();
                }
            }
        })
        .register();
    let quit = mainloop.downgrade();
    let timer = mainloop.loop_().add_timer(move |_| {
        if let Some(mainloop) = quit.upgrade() {
            mainloop.quit();
        }
    });
    let _ = timer.update_timer(Some(Duration::from_secs(10)), None);
    mainloop.run();
    anyhow::ensure!(finished.get(), "{node}'s ports were not set up again");
    Ok(())
}

/// A `PortConfig` for an input in DSP mode, 48 kHz, mono, with a monitor
/// port: WirePlumber's.
fn port_config() -> anyhow::Result<Vec<u8>> {
    use pipewire::spa::{
        self,
        param::{
            ParamType,
            audio::{AudioFormat, AudioInfoRaw, MAX_CHANNELS},
        },
        pod::{Object, Property, PropertyFlags, Value},
        sys,
        utils::{Id, SpaTypes},
    };
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32P);
    info.set_rate(RATE as u32);
    info.set_channels(1);
    let mut position = [0; MAX_CHANNELS];
    position[0] = sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let format = Object {
        type_: SpaTypes::ObjectParamFormat.as_raw(),
        id: ParamType::Format.as_raw(),
        properties: info.into(),
    };
    let property = |key, value| Property {
        key,
        flags: PropertyFlags::empty(),
        value,
    };
    let config = Object {
        type_: SpaTypes::ObjectParamPortConfig.as_raw(),
        id: ParamType::PortConfig.as_raw(),
        properties: vec![
            property(
                sys::SPA_PARAM_PORT_CONFIG_direction,
                Value::Id(Id(sys::SPA_DIRECTION_INPUT)),
            ),
            property(
                sys::SPA_PARAM_PORT_CONFIG_mode,
                Value::Id(Id(sys::SPA_PARAM_PORT_CONFIG_MODE_dsp)),
            ),
            property(sys::SPA_PARAM_PORT_CONFIG_monitor, Value::Bool(true)),
            property(sys::SPA_PARAM_PORT_CONFIG_format, Value::Object(format)),
        ],
    };
    let (cursor, _) = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &Value::Object(config),
    )
    .map_err(|err| anyhow::anyhow!("a PortConfig: {err:?}"))?;
    Ok(cursor.into_inner())
}

// --- The window's Voix tab, with the real shell ------------------------------

/// The microphone's tile opens the window on Voix, Sans bruit chosen (the
/// default); Robot's tile writes `[voice] effect = "robot"`, shown chosen
/// at once.
#[test]
fn the_microphone_s_tile_opens_voix_and_its_tiles_write_the_voice() -> anyhow::Result<()> {
    let mut bed = crate::camera::WindowBed::start_with_voice("", "1600x1000")?;
    let window = bed.open_on_microphone()?;
    bed.logged(
        "the camera's window shows it",
        &[
            "Utilisée par Visio",
            "| Voix (dots: Image, Voix, Réactions)",
            "[ ] Telle quelle, [x] Sans bruit, [ ] Studio, [ ] Panda, [ ] Robot, [ ] Grave",
            "Sans bruit : clavier, ventilateur, rue : retirés",
        ],
    )?;
    bed.tile(window, 4)?;
    bed.written(|t| t.contains("[voice]") && t.contains("effect = \"robot\""))?;
    bed.logged(
        "the camera's window shows it",
        &["[x] Robot", "Robot : métallique"],
    )?;
    Ok(())
}

/// The pull request's screenshots of the Voix tab (#619, lot 5), at
/// 2560 × 1600 at scale 2, the shell on the CPU: opened from the
/// microphone's tile, Sans bruit, then Panda; the sign's microphone tile
/// and its violet dot. Written to `$HOCUS_SHOTS` when it names a folder.
/// Run by hand (`--run-ignored only`).
#[test]
#[ignore = "writes the pull request's screenshots"]
fn screenshots_of_the_voix_tab() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(std::path::PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let mut bed = crate::camera::WindowBed::start_with_voice("", "2560x1600@2")?;
    let window = bed.open_on_microphone()?;
    bed.logged("the camera's preview shows a frame", &[])?;
    wait(&mut bed.hp, Duration::from_secs(2))?;
    let around =
        |r: hocus_compositor::model::Rect| (r.x - 16, r.y - 16, r.width + 32, r.height + 32);
    bed.shoot(&out, "voice-window-quiet.png", around(window), 2)?;
    bed.tile(window, 3)?;
    bed.logged("the camera's window shows it", &["[x] Panda"])?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    bed.shoot(&out, "voice-window-panda.png", around(window), 2)?;
    bed.tile(window, 0)?;
    bed.logged("the camera's window shows it", &["[x] Telle quelle"])?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    bed.shoot(&out, "voice-window-as-it-is.png", around(window), 2)?;
    bed.tile(window, 5)?;
    bed.logged("the camera's window shows it", &["[x] Grave"])?;
    wait(&mut bed.hp, Duration::from_secs(1))?;
    let sign = common::drawn(&mut bed.hp, hocus_link::IN_USE_NAMESPACE)?[0].1;
    bed.shoot(
        &out,
        "voice-microphone-tile-dot.png",
        (sign.x - 40, sign.y - 30, sign.width + 80, sign.height + 60),
        2,
    )?;
    Ok(())
}
