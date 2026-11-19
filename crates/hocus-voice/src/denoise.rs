//! Sans bruit: the noise around the voice taken out by DeepFilterNet3
//! (Schröter et al., MIT or Apache-2.0), its three models run with tract
//! on the CPU, frame by frame, their state kept between frames
//! (`tract-pulse`).
//!
//! - **The models** (`models/`, `LICENSE-DEEPFILTERNET`): DeepFilterNet3's
//!   ONNX export, unchanged, 8.3 MB in the program (no download, as the
//!   camera's models).
//! - **The signal path**, as DeepFilterNet's own library runs it
//!   (`libDF`, read and written again here for tract 0.23 and this
//!   program's buffers): a 960-sample Vorbis window every 480 samples
//!   (10 ms); 32 ERB bands' energies and the first 96 bins, normalised
//!   with running means, into the encoder; the ERB decoder's gains on the
//!   spectrum of two frames before (the models look two frames ahead);
//!   the deep filter's 5-frame complex filter on its first 96 bins; then
//!   overlap-add. Frames with a local SNR under -10 dB are silence, over
//!   30 dB are left as they are, over 20 dB get the gains alone.
//! - **Its delay**: 480 samples of window, 960 of look-ahead, and up to
//!   480 to meet the stream's quanta (it is primed with them): 40 ms.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;

use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use tract_onnx::prelude::*;
use tract_onnx::tract_core::internal::SimpleState;
use tract_pulse::model::{PulsedModel, PulsedModelExt};

use crate::effect::Effect;

static ENCODER: &[u8] = include_bytes!("../models/enc.onnx");
static ERB_DECODER: &[u8] = include_bytes!("../models/erb_dec.onnx");
static DF_DECODER: &[u8] = include_bytes!("../models/df_dec.onnx");

/// The model's parameters (its `config.ini`).
const WINDOW: usize = 960;
const HOP: usize = 480;
const FREQS: usize = WINDOW / 2 + 1;
const ERB: usize = 32;
const MIN_ERB_FREQS: usize = 2;
const DF: usize = 96;
const ORDER: usize = 5;
const LOOKAHEAD: usize = 2;
const CONV_CH: usize = 64;
const HIDDEN: usize = CONV_CH * ERB / 4;
/// The running means' weight of the past: `exp(-hop / sr / tau)` with
/// `tau` = 1 s, rounded as DeepFilterNet rounds it.
const ALPHA: f32 = 0.99;
/// Local SNR thresholds, dB.
const MIN_DB: f32 = -10.0;
const MAX_ERB_DB: f32 = 30.0;
const MAX_DF_DB: f32 = 20.0;

/// The voice's delay through it, samples: the window's, the look-ahead's,
/// and the quanta's priming.
pub const LATENCY: usize = (WINDOW - HOP) + LOOKAHEAD * HOP + HOP;

type Plan = TypedSimpleState;

/// DeepFilterNet3, ready to run.
pub struct Denoiser {
    encoder: Plan,
    erb_decoder: Plan,
    df_decoder: Plan,
    stft: Stft,
    erb: [usize; ERB],
    mean_norm: [f32; ERB],
    unit_norm: [f32; DF],
    /// Spectra of the last frames: the gains' (`ORDER + LOOKAHEAD`) and
    /// the noisy ones the deep filter reads (`ORDER`), the newest last.
    gained: VecDeque<Vec<Complex32>>,
    noisy: VecDeque<Vec<Complex32>>,
    /// Samples not yet a hop, and the voice made, waiting to go out.
    pending: Vec<f32>,
    ready: VecDeque<f32>,
    /// Frames of silence in a row.
    silent: usize,
    spectrum: Vec<Complex32>,
    frame: Vec<f32>,
    /// The last frame's local SNR, dB (tests).
    pub lsnr: f32,
}

impl std::fmt::Debug for Denoiser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Denoiser").finish_non_exhaustive()
    }
}

/// What the models say of a frame: its local SNR, the ERB gains if they
/// apply, the deep filter's taps if it applies.
type Said = (f32, Option<Vec<f32>>, Option<Vec<Complex32>>);

/// A model read, its time axis `S` pulsed one frame at a time.
fn plan(
    bytes: &[u8],
    inputs: &[(&str, Vec<Option<usize>>)],
    outputs: Option<&[&str]>,
) -> TractResult<Plan> {
    let mut model = tract_onnx::onnx()
        .with_ignore_output_shapes(true)
        .model_for_read(&mut Cursor::new(bytes))?;
    let s = model.symbols.sym("S");
    for (i, (_, shape)) in inputs.iter().enumerate() {
        let dims: TVec<TDim> = shape
            .iter()
            .map(|d| d.map_or_else(|| s.to_dim(), |n| n.to_dim()))
            .collect();
        let fact = InferenceFact::dt_shape(f32::datum_type(), dims);
        model = model.with_input_fact(i, fact)?;
    }
    model = model.with_input_names(inputs.iter().map(|(name, _)| *name))?;
    if let Some(outputs) = outputs {
        model = model.with_outputs_by_name(outputs.iter().copied())?;
    }
    model.analyse(true)?;
    let mut typed = model.into_typed()?;
    typed.declutter()?;
    let pulsed = PulsedModel::new(&typed, s, &1.to_dim())?;
    let plan = pulsed.into_typed()?.into_optimized()?.into_runnable()?;
    SimpleState::new(&plan)
}

impl Denoiser {
    /// The three models read and planned: some hundreds of milliseconds,
    /// never on the stream's thread.
    pub fn new() -> TractResult<Self> {
        let encoder = plan(
            ENCODER,
            &[
                ("feat_erb", vec![Some(1), Some(1), None, Some(ERB)]),
                ("feat_spec", vec![Some(1), Some(2), None, Some(DF)]),
            ],
            Some(&["e0", "e1", "e2", "e3", "emb", "c0", "lsnr"]),
        )?;
        let erb_decoder = plan(
            ERB_DECODER,
            &[
                ("emb", vec![Some(1), None, Some(HIDDEN)]),
                ("e3", vec![Some(1), Some(CONV_CH), None, Some(ERB / 4)]),
                ("e2", vec![Some(1), Some(CONV_CH), None, Some(ERB / 4)]),
                ("e1", vec![Some(1), Some(CONV_CH), None, Some(ERB / 2)]),
                ("e0", vec![Some(1), Some(CONV_CH), None, Some(ERB)]),
            ],
            None,
        )?;
        let df_decoder = plan(
            DF_DECODER,
            &[
                ("emb", vec![Some(1), None, Some(HIDDEN)]),
                ("c0", vec![Some(1), Some(CONV_CH), None, Some(DF)]),
            ],
            Some(&["coefs"]),
        )?;
        let mut denoiser = Self {
            encoder,
            erb_decoder,
            df_decoder,
            stft: Stft::new(),
            erb: erb_bands(),
            mean_norm: [0.0; ERB],
            unit_norm: [0.0; DF],
            gained: VecDeque::new(),
            noisy: VecDeque::new(),
            pending: Vec::with_capacity(HOP),
            ready: VecDeque::with_capacity(4 * HOP),
            silent: 0,
            spectrum: vec![Complex32::default(); FREQS],
            frame: vec![0.0; HOP],
            lsnr: 0.0,
        };
        denoiser.clear();
        Ok(denoiser)
    }

    fn clear(&mut self) {
        self.stft.clear();
        // DeepFilterNet's starting means: -60 to -90 dB across the bands,
        // 0.001 to 0.0001 across the bins.
        for (i, m) in self.mean_norm.iter_mut().enumerate() {
            *m = -60.0 + (-90.0 + 60.0) * i as f32 / (ERB - 1) as f32;
        }
        for (i, u) in self.unit_norm.iter_mut().enumerate() {
            *u = 0.001 + (0.0001 - 0.001) * i as f32 / (DF - 1) as f32;
        }
        let zero = || vec![Complex32::default(); FREQS];
        self.gained = (0..ORDER + LOOKAHEAD).map(|_| zero()).collect();
        self.noisy = (0..ORDER.max(LOOKAHEAD)).map(|_| zero()).collect();
        self.pending.clear();
        self.ready.clear();
        self.ready.extend(std::iter::repeat_n(0.0, HOP));
        self.silent = 0;
    }

    /// One hop of the voice in `self.frame`, made in place.
    fn hop(&mut self) -> TractResult<()> {
        let energy = self.frame.iter().map(|x| x * x).sum::<f32>() / HOP as f32;
        self.silent = if energy < 1e-7 { self.silent + 1 } else { 0 };
        if self.silent > 5 {
            // Silence in, silence out: the models rest.
            self.frame.fill(0.0);
            return Ok(());
        }
        self.stft.analysis(&self.frame, &mut self.spectrum);
        let mut gained = self.gained.pop_front().unwrap_or_default();
        gained.clone_from(&self.spectrum);
        self.gained.push_back(gained);
        let mut noisy = self.noisy.pop_front().unwrap_or_default();
        noisy.clone_from(&self.spectrum);
        self.noisy.push_back(noisy);

        let (lsnr, gains, coefs) = self.models()?;
        self.lsnr = lsnr;
        let Some(target) = self.gained.get_mut(ORDER - 1) else {
            return Ok(());
        };
        if let Some(gains) = gains {
            let mut bin = 0;
            for (&width, &gain) in self.erb.iter().zip(&gains) {
                for x in target.iter_mut().skip(bin).take(width) {
                    *x *= gain;
                }
                bin += width;
            }
        }
        self.spectrum.clone_from(target);
        if let Some(coefs) = coefs {
            deep_filter(&self.noisy, &coefs, &mut self.spectrum);
        }
        self.stft.synthesis(&mut self.spectrum, &mut self.frame);
        Ok(())
    }

    /// The models on the newest spectrum: the local SNR, the ERB gains if
    /// they apply, the deep filter's coefficients if it applies.
    fn models(&mut self) -> TractResult<Said> {
        let newest = self.noisy.back().map(Vec::as_slice).unwrap_or_default();
        let mut erb = [0f32; ERB];
        band_energies(newest, &self.erb, &mut erb);
        for (x, m) in erb.iter_mut().zip(self.mean_norm.iter_mut()) {
            let db = (*x + 1e-10).log10() * 10.0;
            *m = db * (1.0 - ALPHA) + *m * ALPHA;
            *x = (db - *m) / 40.0;
        }
        // The first bins, real parts then imaginary ones (the encoder's
        // [1, 2, 1, 96]).
        let mut spec = [0f32; 2 * DF];
        let (re, im) = spec.split_at_mut(DF);
        for (((x, u), r), i) in newest
            .iter()
            .zip(self.unit_norm.iter_mut())
            .zip(re.iter_mut())
            .zip(im.iter_mut())
        {
            *u = x.norm() * (1.0 - ALPHA) + *u * ALPHA;
            let n = u.sqrt();
            *r = x.re / n;
            *i = x.im / n;
        }
        let feat_erb = Tensor::from_shape(&[1, 1, 1, ERB], &erb)?;
        let feat_spec = Tensor::from_shape(&[1, 2, 1, DF], &spec)?;
        let mut out = self.encoder.run(tvec!(feat_erb.into(), feat_spec.into()))?;
        let lsnr = out.pop().and_then(|t| first(&t)).unwrap_or(0.0);
        let c0 = out.pop();
        let emb = out.pop();
        let (Some(c0), Some(emb)) = (c0, emb) else {
            anyhow::bail!("the encoder said too little");
        };
        let (gains, zeros, filter) = if lsnr < MIN_DB {
            (false, true, false)
        } else if lsnr > MAX_ERB_DB {
            (false, false, false)
        } else if lsnr > MAX_DF_DB {
            (true, false, false)
        } else {
            (true, false, true)
        };
        let gains = if gains {
            // e3, e2, e1, e0: the encoder's first four, last first.
            let mut inputs: TVec<TValue> = tvec!(emb.clone());
            for _ in 0..4 {
                inputs.push(out.pop().ok_or_else(|| anyhow::anyhow!("no skip"))?);
            }
            let m = self.erb_decoder.run(inputs)?;
            let m = m.first().ok_or_else(|| anyhow::anyhow!("no gains"))?;
            Some(m.to_plain_array_view::<f32>()?.iter().copied().collect())
        } else if zeros {
            Some(vec![0.0; ERB])
        } else {
            None
        };
        let coefs = if filter {
            let c = self.df_decoder.run(tvec!(emb, c0))?;
            let c = c
                .first()
                .ok_or_else(|| anyhow::anyhow!("no coefficients"))?;
            // [1, 1, 96, 10]: each bin's 5 complex taps, oldest first.
            let c: Vec<f32> = c.to_plain_array_view::<f32>()?.iter().copied().collect();
            Some(
                c.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|[re, im]| Complex32::new(*re, *im))
                    .collect(),
            )
        } else {
            None
        };
        Ok((lsnr, gains, coefs))
    }
}

impl Effect for Denoiser {
    fn run(&mut self, io: &mut [f32]) {
        for x in io.iter_mut() {
            self.pending.push(*x);
            if self.pending.len() == HOP {
                std::mem::swap(&mut self.pending, &mut self.frame);
                if let Err(err) = self.hop() {
                    tracing::warn!(%err, "the noise reduction failed on a frame");
                }
                self.ready.extend(self.frame.iter().copied());
                self.pending.clear();
            }
            *x = self.ready.pop_front().unwrap_or(0.0);
        }
    }

    fn latency(&self) -> usize {
        LATENCY
    }

    fn reset(&mut self) {
        self.clear();
    }
}

/// A tensor's first value.
fn first(t: &TValue) -> Option<f32> {
    t.to_plain_array_view::<f32>().ok()?.iter().next().copied()
}

/// The deep filter: each of the first bins, the sum of the last `ORDER`
/// noisy frames' times its taps, into `out`.
fn deep_filter(noisy: &VecDeque<Vec<Complex32>>, coefs: &[Complex32], out: &mut [Complex32]) {
    for (f, o) in out.iter_mut().take(DF).enumerate() {
        let taps = coefs.get(f * ORDER..(f + 1) * ORDER).unwrap_or_default();
        *o = noisy
            .iter()
            .zip(taps)
            .map(|(frame, c)| frame.get(f).copied().unwrap_or_default() * c)
            .sum();
    }
}

/// Each ERB band's mean power.
fn band_energies(spectrum: &[Complex32], widths: &[usize; ERB], out: &mut [f32; ERB]) {
    let mut bins = spectrum.iter();
    for (o, &width) in out.iter_mut().zip(widths) {
        let sum: f32 = bins.by_ref().take(width).map(Complex32::norm_sqr).sum();
        *o = sum / width.max(1) as f32;
    }
}

fn freq_to_erb(hz: f32) -> f32 {
    9.265 * (hz / (24.7 * 9.265)).ln_1p()
}

fn erb_to_freq(erb: f32) -> f32 {
    24.7 * 9.265 * ((erb / 9.265).exp() - 1.0)
}

/// How many bins each ERB band takes, at least [`MIN_ERB_FREQS`], all
/// of them in all (DeepFilterNet's `erb_fb`).
fn erb_bands() -> [usize; ERB] {
    let width = crate::RATE as f32 / WINDOW as f32;
    let high = freq_to_erb(crate::RATE as f32 / 2.0);
    let step = high / ERB as f32;
    let mut bands = [0usize; ERB];
    let (mut previous, mut over) = (0i32, 0i32);
    for (i, band) in bands.iter_mut().enumerate() {
        let f = erb_to_freq((i + 1) as f32 * step);
        let bin = (f / width).round() as i32;
        let mut n = bin - previous - over;
        if n < MIN_ERB_FREQS as i32 {
            over = MIN_ERB_FREQS as i32 - n;
            n = MIN_ERB_FREQS as i32;
        } else {
            over = 0;
        }
        *band = n.max(0) as usize;
        previous = bin;
    }
    let total: usize = bands.iter().sum::<usize>() + 1;
    if let Some(last) = bands.last_mut() {
        *last = (*last + 1).saturating_sub(total.saturating_sub(FREQS));
    }
    bands
}

/// DeepFilterNet's short-time Fourier transform: a Vorbis window over
/// [`WINDOW`] samples every [`HOP`], scaled in the analysis alone.
struct Stft {
    window: Vec<f32>,
    forward: Arc<dyn Fft<f32>>,
    inverse: Arc<dyn Fft<f32>>,
    analysis: Vec<f32>,
    synthesis: Vec<f32>,
    buffer: Vec<Complex32>,
    scratch: Vec<Complex32>,
}

impl Stft {
    fn new() -> Self {
        let mut planner = FftPlanner::new();
        let forward = planner.plan_fft_forward(WINDOW);
        let inverse = planner.plan_fft_inverse(WINDOW);
        let half = (WINDOW / 2) as f64;
        let window = (0..WINDOW)
            .map(|i| {
                let s = (0.5 * std::f64::consts::PI * (i as f64 + 0.5) / half).sin();
                (0.5 * std::f64::consts::PI * s * s).sin() as f32
            })
            .collect();
        let scratch = vec![
            Complex32::default();
            forward
                .get_inplace_scratch_len()
                .max(inverse.get_inplace_scratch_len())
        ];
        Self {
            window,
            forward,
            inverse,
            analysis: vec![0.0; WINDOW - HOP],
            synthesis: vec![0.0; WINDOW - HOP],
            buffer: vec![Complex32::default(); WINDOW],
            scratch,
        }
    }

    fn clear(&mut self) {
        self.analysis.fill(0.0);
        self.synthesis.fill(0.0);
    }

    /// A hop of samples in, the spectrum of the window ending with it out.
    fn analysis(&mut self, hop: &[f32], spectrum: &mut [Complex32]) {
        let samples = self.analysis.iter().chain(hop);
        for ((b, x), w) in self.buffer.iter_mut().zip(samples).zip(&self.window) {
            *b = Complex32::new(x * w, 0.0);
        }
        self.analysis.clear();
        self.analysis.extend_from_slice(hop);
        self.forward
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let norm = (2 * HOP) as f32 / (WINDOW * WINDOW) as f32;
        for (s, b) in spectrum.iter_mut().zip(&self.buffer) {
            *s = b * norm;
        }
    }

    /// A spectrum in, a hop of samples out, overlapped with the last.
    fn synthesis(&mut self, spectrum: &mut [Complex32], hop: &mut [f32]) {
        // The full spectrum, its upper half the lower's mirror.
        for (i, b) in self.buffer.iter_mut().enumerate() {
            *b = if i < FREQS {
                spectrum.get(i).copied().unwrap_or_default()
            } else {
                spectrum.get(WINDOW - i).copied().unwrap_or_default().conj()
            };
        }
        self.inverse
            .process_with_scratch(&mut self.buffer, &mut self.scratch);
        let mut x = self.buffer.iter().zip(&self.window).map(|(b, w)| b.re * w);
        for (h, m) in hop.iter_mut().zip(&self.synthesis) {
            *h = x.next().unwrap_or(0.0) + m;
        }
        for m in self.synthesis.iter_mut() {
            *m = x.next().unwrap_or(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bands_take_every_bin() {
        let bands = erb_bands();
        assert_eq!(bands.iter().sum::<usize>(), FREQS);
        assert!(bands.iter().all(|&b| b >= MIN_ERB_FREQS));
        // DeepFilterNet's first bands, two bins each, the last the widest.
        assert_eq!(bands[0], 2);
        assert!(bands[ERB - 1] > bands[ERB / 2]);
    }

    /// The window's analysis and synthesis give the input back, a hop
    /// later.
    #[test]
    fn the_transform_gives_the_signal_back_a_hop_later() {
        let mut stft = Stft::new();
        let signal: Vec<f32> = (0..HOP * 8)
            .map(|i| (i as f32 * 0.031).sin() * 0.5 + (i as f32 * 0.17).cos() * 0.2)
            .collect();
        let mut out = Vec::new();
        let mut spectrum = vec![Complex32::default(); FREQS];
        let mut hop = vec![0.0; HOP];
        for chunk in signal.chunks(HOP) {
            stft.analysis(chunk, &mut spectrum);
            stft.synthesis(&mut spectrum, &mut hop);
            out.extend_from_slice(&hop);
        }
        for (i, (o, s)) in out.iter().skip(HOP).zip(&signal).enumerate().skip(HOP) {
            assert!((o - s).abs() < 1e-3, "{i}: {o} {s}");
        }
    }
}
