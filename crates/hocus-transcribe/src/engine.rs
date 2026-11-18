//! What makes a meeting's text from its two tracks, behind a trait: the
//! real one (whisper.cpp and sherpa-onnx, [`crate::real`]), and a fake one
//! for tests, which says fixed lines (`HOCUS_TRANSCRIBE_FAKE`).

use std::{path::Path, time::Duration};

use serde::Deserialize;

use crate::text::{Line, Speaker};

/// What the engine heard: the user's lines and the call's, each at its
/// time from the tracks' start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Heard {
    pub me: Vec<Line>,
    pub call: Vec<Line>,
}

/// Makes the lines of two tracks (16 kHz, mono, -1 to 1), saying how far
/// it is (0 to 1) as it goes.
pub trait Engine {
    fn hear(
        &mut self,
        me: &[f32],
        call: &[f32],
        progress: &mut dyn FnMut(f32),
    ) -> anyhow::Result<Heard>;
}

/// The fake engine's file: the lines it says, or that it fails.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Fake {
    /// It fails, saying this.
    pub fail: Option<String>,
    /// The user's lines: start, end (seconds), text.
    pub me: Vec<(f64, f64, String)>,
    /// The call's: start, end, voice (0: "Les autres"), text.
    pub call: Vec<(f64, f64, u32, String)>,
    /// How long each of its steps takes, in milliseconds (the progress is
    /// said at each).
    pub step_ms: u64,
}

impl Fake {
    pub fn read(path: &Path) -> anyhow::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }
}

impl Engine for Fake {
    fn hear(
        &mut self,
        _me: &[f32],
        _call: &[f32],
        progress: &mut dyn FnMut(f32),
    ) -> anyhow::Result<Heard> {
        for step in 1..=4u8 {
            std::thread::sleep(Duration::from_millis(self.step_ms));
            progress(f32::from(step) / 4.0);
            if step == 2
                && let Some(why) = &self.fail
            {
                anyhow::bail!("{why}");
            }
        }
        let line = |start: f64, end: f64, speaker, text: &str| Line {
            start,
            end,
            speaker,
            text: text.to_owned(),
        };
        Ok(Heard {
            me: self
                .me
                .iter()
                .map(|(s, e, t)| line(*s, *e, Speaker::Me, t))
                .collect(),
            call: self
                .call
                .iter()
                .map(|(s, e, v, t)| {
                    let who = match *v {
                        0 => Speaker::Others,
                        n => Speaker::Voice(n),
                    };
                    line(*s, *e, who, t)
                })
                .collect(),
        })
    }
}
