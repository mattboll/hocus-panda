//! The real engine: the call's voices told apart by sherpa-onnx
//! (pyannote's segmentation, 3D-Speaker's embeddings, clustered), then
//! each track's speech heard by whisper.cpp (large-v3-turbo, its language
//! found by itself), packed in full windows; each of the call's lines is
//! the voice's it overlaps most.

use std::path::Path;

use hocus_meetings::models;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{
    engine::{Engine, Heard},
    speech::{self, Pack, Span},
    text::{Line, Speaker},
};

/// A line whisper.cpp thinks is silence this surely is left out.
const NO_SPEECH: f32 = 0.6;
/// Two voices closer than this are one (sherpa-onnx's clustering).
const SAME_VOICE: f32 = 0.6;

pub struct Real {
    words: WhisperContext,
    voices: Option<sherpa_onnx::OfflineSpeakerDiarization>,
    threads: i32,
    /// The track's language, once its first window found it: its next
    /// windows are not heard twice to find it again.
    lang: Option<&'static str>,
}

impl std::fmt::Debug for Real {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Real")
    }
}

impl Real {
    /// The models in `dir`, loaded, the words' on the GPU whisper.cpp's
    /// Vulkan backend is shown (`gpu`), else on the CPU (its smaller
    /// model, every core). The voices' are optional: without them, the
    /// call is "Les autres".
    pub fn load(dir: &Path, gpu: bool) -> anyhow::Result<Self> {
        let [words, _, _] = models::for_device(gpu);
        Self::load_with(dir, &dir.join(words.file), gpu)
    }

    /// The same, the words' model at `path`.
    fn load_with(dir: &Path, path: &Path, gpu: bool) -> anyhow::Result<Self> {
        whisper_rs::install_logging_hooks();
        let threads = std::thread::available_parallelism().map_or(2, |n| n.get());
        let threads = i32::try_from(threads).unwrap_or(4);
        let mut params = WhisperContextParameters::default();
        params.use_gpu(gpu);
        let words = WhisperContext::new_with_params(
            path.to_str()
                .ok_or_else(|| anyhow::anyhow!("{}: not UTF-8", path.display()))?,
            params,
        )
        .map_err(|err| anyhow::anyhow!("{}: {err}", path.display()))?;
        let name = |m: models::Model| Some(dir.join(m.file).to_string_lossy().into_owned());
        let config = sherpa_onnx::OfflineSpeakerDiarizationConfig {
            segmentation: sherpa_onnx::OfflineSpeakerSegmentationModelConfig {
                pyannote: sherpa_onnx::OfflineSpeakerSegmentationPyannoteModelConfig {
                    model: name(models::SEGMENTS),
                    ..Default::default()
                },
                num_threads: threads,
                ..Default::default()
            },
            embedding: sherpa_onnx::SpeakerEmbeddingExtractorConfig {
                model: name(models::VOICES),
                num_threads: threads,
                ..Default::default()
            },
            clustering: sherpa_onnx::FastClusteringConfig {
                num_clusters: -1,
                threshold: SAME_VOICE,
                ..Default::default()
            },
            ..Default::default()
        };
        let voices = sherpa_onnx::OfflineSpeakerDiarization::create(&config);
        if voices.is_none() {
            eprintln!("hocus-transcribe: the voices' models do not load: the call is one");
        }
        Ok(Self {
            words,
            voices,
            threads,
            lang: None,
        })
    }

    /// The call's voices, told apart (by their first turn: 1, 2…): their
    /// turns; none when fewer than two are.
    fn turns(&self, call: &[f32]) -> Vec<(Span, Speaker)> {
        let told = self
            .voices
            .as_ref()
            .and_then(|v| v.process(call))
            .map(|result| result.sort_by_start_time())
            .unwrap_or_default();
        let mut order: Vec<i32> = Vec::new();
        for segment in &told {
            if !order.contains(&segment.speaker) {
                order.push(segment.speaker);
            }
        }
        if order.len() < 2 {
            return Vec::new();
        }
        told.iter()
            .map(|s| {
                let n = order.iter().position(|o| *o == s.speaker).unwrap_or(0) + 1;
                (
                    Span {
                        start: f64::from(s.start),
                        end: f64::from(s.end),
                    },
                    Speaker::Voice(u32::try_from(n).unwrap_or(u32::MAX)),
                )
            })
            .collect()
    }

    /// What `pack` of `track` says, as lines at the track's times.
    fn hear_pack(
        &mut self,
        pack: &Pack,
        track: &[f32],
        speaker: Speaker,
    ) -> anyhow::Result<Vec<Line>> {
        let samples = pack.samples(track);
        let mut state = self.words.create_state()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some(self.lang.unwrap_or("auto")));
        params.set_n_threads(self.threads);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, &samples)?;
        if self.lang.is_none() {
            self.lang = whisper_rs::get_lang_str(state.full_lang_id_from_state());
        }
        let mut lines = Vec::new();
        for segment in state.as_iter() {
            if segment.no_speech_probability() > NO_SPEECH {
                continue;
            }
            let text = segment.to_str_lossy()?.trim().to_owned();
            // Its marks for what is not speech: "[BLANK_AUDIO]", "(rires)".
            let marked = |open: char, close: char| text.starts_with(open) && text.ends_with(close);
            let said = text.chars().any(char::is_alphanumeric);
            if !said || marked('[', ']') || marked('(', ')') {
                continue;
            }
            lines.push(Line {
                start: pack.track_time(segment.start_timestamp() as f64 / 100.0),
                end: pack.track_time(segment.end_timestamp() as f64 / 100.0),
                speaker,
                text,
            });
        }
        Ok(lines)
    }
}

/// The voice of `turns` a line from `start` to `end` overlaps most; "Les
/// autres" without one.
fn voice_of(turns: &[(Span, Speaker)], start: f64, end: f64) -> Speaker {
    let mut best = (0.0, Speaker::Others);
    for (span, speaker) in turns {
        let overlap = end.min(span.end) - start.max(span.start);
        if overlap > best.0 {
            best = (overlap, *speaker);
        }
    }
    best.1
}

impl Engine for Real {
    fn hear(
        &mut self,
        me: &[f32],
        call: &[f32],
        progress: &mut dyn FnMut(f32),
    ) -> anyhow::Result<Heard> {
        // The voices first: a tenth of the work, about.
        let turns = self.turns(call);
        progress(0.1);
        let packs: Vec<(Pack, bool)> = speech::pack(&speech::speech(me))
            .into_iter()
            .map(|p| (p, true))
            .chain(
                speech::pack(&speech::speech(call))
                    .into_iter()
                    .map(|p| (p, false)),
            )
            .collect();
        // Its progress said pack by pack: 28 s of speech at most each.
        let total: f64 = packs.iter().map(|(p, _)| p.length()).sum::<f64>().max(1.0);
        let mut gone = 0.0;
        let mut heard = Heard::default();
        let mut track = None;
        for (pack, mine) in packs {
            // Each track's language found on its own first window: the
            // others may speak another.
            if track != Some(mine) {
                track = Some(mine);
                self.lang = None;
            }
            if mine {
                heard.me.extend(self.hear_pack(&pack, me, Speaker::Me)?);
            } else {
                let lines = self.hear_pack(&pack, call, Speaker::Others)?;
                heard.call.extend(lines.into_iter().map(|line| Line {
                    speaker: voice_of(&turns, line.start, line.end),
                    ..line
                }));
            }
            gone += pack.length();
            progress((0.1 + 0.9 * (gone / total)) as f32);
        }
        Ok(heard)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use hocus_meetings::{models::Model, wav};

    use super::*;
    use crate::fetch;

    #[test]
    fn a_calls_line_is_the_voice_it_overlaps_most() {
        let turns = [
            (
                Span {
                    start: 0.0,
                    end: 5.0,
                },
                Speaker::Voice(1),
            ),
            (
                Span {
                    start: 5.0,
                    end: 9.0,
                },
                Speaker::Voice(2),
            ),
        ];
        assert_eq!(voice_of(&turns, 1.0, 4.0), Speaker::Voice(1));
        assert_eq!(voice_of(&turns, 4.0, 8.0), Speaker::Voice(2));
        assert_eq!(voice_of(&turns, 10.0, 12.0), Speaker::Others);
        assert_eq!(voice_of(&[], 1.0, 2.0), Speaker::Others);
    }

    /// whisper.cpp's own sample (MIT): eleven seconds of a voice.
    const SAMPLE: Model = Model {
        file: "jfk.wav",
        url: "https://raw.githubusercontent.com/ggml-org/whisper.cpp/master/samples/jfk.wav",
        sha256: "59dfb9a4acb36fe2a2affc14bacbee2920ff435cb13cc314a08c13f66ba7860e",
        bytes: 352_078,
        used: models::Use::Words,
    };

    /// The real models on a real voice, on the call's track; the
    /// microphone's silent. Run on purpose, with a folder of its own for
    /// the models, which it downloads (610 MB) the first time:
    /// `HOCUS_TEST_MODELS=DIR cargo nextest run -p hocus-transcribe
    /// --run-ignored only real::`.
    #[test]
    #[ignore = "downloads the models into HOCUS_TEST_MODELS, then runs them on the CPU"]
    fn the_real_engine_writes_what_a_voice_says() -> anyhow::Result<()> {
        let models = std::env::var_os("HOCUS_TEST_MODELS")
            .map(PathBuf::from)
            .ok_or_else(|| anyhow::anyhow!("HOCUS_TEST_MODELS names the models' folder"))?;
        // On the CPU unless `HOCUS_TRANSCRIBE_TEST_GPU` is set: then on the
        // Vulkan device the caller shows whisper.cpp alone
        // (`GGML_VK_VISIBLE_DEVICES`), never a real GPU here but Mesa's
        // CPU Vulkan (docs/design/meetings.md, "Measured").
        let gpu = std::env::var_os("HOCUS_TRANSCRIBE_TEST_GPU").is_some();
        fetch::missing(&models, &models::for_device(gpu), &mut |_, _| {})?;
        if !models.join(SAMPLE.file).exists() {
            fetch::one(&fetch::agent(), &models, SAMPLE, &mut |_| {})?;
        }
        let voice = wav::read(&models.join(SAMPLE.file))?;
        // `HOCUS_TEST_WORDS`: another words' model (whisper.cpp's tiny one,
        // to try the Vulkan path on lavapipe, which is slow).
        let mut real = match std::env::var_os("HOCUS_TEST_WORDS") {
            Some(words) => Real::load_with(&models, Path::new(&words), gpu)?,
            None => Real::load(&models, gpu)?,
        };
        let mut last = 0.0;
        let heard = real.hear(&vec![0.0; voice.len()], &voice, &mut |done| last = done)?;
        let text = heard
            .call
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase();
        assert!(
            text.contains("ask not what your country can do for you"),
            "{text}"
        );
        assert!(heard.me.is_empty(), "{:?}", heard.me);
        assert!(heard.call.iter().all(|l| l.speaker == Speaker::Others));
        assert!((last - 1.0).abs() < 1e-6);
        Ok(())
    }
}
