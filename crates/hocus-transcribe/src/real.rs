//! The real engine: the call's voices told apart by sherpa-onnx
//! (pyannote's segmentation, 3D-Speaker's embeddings, clustered), then
//! each track's speech heard by whisper.cpp (large-v3-turbo, its language
//! found by itself), piece by piece.

use std::path::Path;

use hocus_meetings::models;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{
    engine::{Engine, Heard},
    speech::{self, Span},
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
}

impl std::fmt::Debug for Real {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Real")
    }
}

impl Real {
    /// The models in `dir`, loaded. The voices' are optional: without
    /// them, the call is "Les autres".
    pub fn load(dir: &Path) -> anyhow::Result<Self> {
        whisper_rs::install_logging_hooks();
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get())
            .clamp(1, 8);
        let threads = i32::try_from(threads).unwrap_or(4);
        let path = dir.join(models::WORDS.file);
        let words = WhisperContext::new_with_params(
            path.to_str()
                .ok_or_else(|| anyhow::anyhow!("{}: not UTF-8", path.display()))?,
            WhisperContextParameters::default(),
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
        })
    }

    /// The call's turns: its voices told apart (by their first turn, 1,
    /// 2…), else its speech as one speaker.
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
            return speech::speech(call)
                .into_iter()
                .map(|s| (s, Speaker::Others))
                .collect();
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

    /// What `samples` say, as lines from `offset` (seconds).
    fn hear_piece(
        &self,
        samples: &[f32],
        offset: f64,
        speaker: Speaker,
    ) -> anyhow::Result<Vec<Line>> {
        let mut state = self.words.create_state()?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(Some("auto"));
        params.set_n_threads(self.threads);
        params.set_no_context(true);
        params.set_suppress_blank(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        state.full(params, samples)?;
        let mut lines = Vec::new();
        for segment in state.as_iter() {
            if segment.no_speech_probability() > NO_SPEECH {
                continue;
            }
            let text = segment.to_str_lossy()?.trim().to_owned();
            // Its marks for what is not speech: "[BLANK_AUDIO]", "(rires)".
            let marked = |open: char, close: char| text.starts_with(open) && text.ends_with(close);
            if text.is_empty() || marked('[', ']') || marked('(', ')') {
                continue;
            }
            lines.push(Line {
                start: offset + segment.start_timestamp() as f64 / 100.0,
                end: offset + segment.end_timestamp() as f64 / 100.0,
                speaker,
                text,
            });
        }
        Ok(lines)
    }
}

impl Engine for Real {
    fn hear(
        &mut self,
        me: &[f32],
        call: &[f32],
        progress: &mut dyn FnMut(f32),
    ) -> anyhow::Result<Heard> {
        // The voices first: a tenth of the work, about.
        let call_turns = self.turns(call);
        progress(0.1);
        let mut pieces: Vec<(Span, Speaker, bool)> = speech::pieces(&speech::speech(me))
            .into_iter()
            .map(|s| (s, Speaker::Me, true))
            .collect();
        // The call's turns, a voice's neighbouring ones together.
        let mut by_voice: Vec<(Span, Speaker)> = Vec::new();
        for (span, speaker) in call_turns {
            match by_voice.last_mut() {
                Some((last, who))
                    if *who == speaker
                        && span.end - last.start <= speech::LONGEST
                        && span.start - last.end < 1.0 =>
                {
                    last.end = span.end;
                }
                _ => by_voice.push((span, speaker)),
            }
        }
        for (span, speaker) in by_voice {
            for piece in speech::pieces(&[span]) {
                pieces.push((piece, speaker, false));
            }
        }
        // Its progress said piece by piece: 28 s of sound at most each.
        let total: f64 = pieces
            .iter()
            .map(|(s, _, _)| s.length())
            .sum::<f64>()
            .max(1.0);
        let mut gone = 0.0;
        let mut heard = Heard::default();
        for (span, speaker, mine) in pieces {
            let samples = span.of(if mine { me } else { call });
            let lines = self.hear_piece(samples, span.start, speaker)?;
            gone += span.length();
            progress((0.1 + 0.9 * (gone / total)) as f32);
            if mine {
                heard.me.extend(lines);
            } else {
                heard.call.extend(lines);
            }
        }
        Ok(heard)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use hocus_meetings::{models::Model, wav};

    use super::*;
    use crate::fetch;

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
        fetch::missing(&models, &mut |_, _| {})?;
        if !models.join(SAMPLE.file).exists() {
            fetch::one(&fetch::agent(), &models, SAMPLE, &mut |_| {})?;
        }
        let voice = wav::read(&models.join(SAMPLE.file))?;
        let mut real = Real::load(&models)?;
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
