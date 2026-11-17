//! The models a meeting's text is made with, downloaded on first use
//! into `$XDG_DATA_HOME/hocuspanda/models` (after the asking popover
//! says so), each checked against its SHA-256 before it is used.
//!
//! - whisper.cpp's large-v3-turbo, quantised (q5_0): OpenAI's Whisper
//!   weights (MIT), converted by whisper.cpp's authors (MIT);
//! - pyannote's segmentation 3.0 (MIT), as sherpa-onnx exported it: where
//!   the call's voices speak;
//! - 3D-Speaker's CAM++ (Apache-2.0), as sherpa-onnx exported it: who
//!   speaks, from the voice. Never NeMo's (CC-BY).

use std::path::Path;

/// What a model is used for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Use {
    /// Speech to text.
    Words,
    /// Where voices speak, in the call's track.
    Segments,
    /// Who speaks.
    Voices,
}

/// A model: its file's name, where it is downloaded from, its SHA-256 and
/// size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Model {
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    pub used: Use,
}

pub const WORDS: Model = Model {
    file: "ggml-large-v3-turbo-q5_0.bin",
    url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin",
    sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
    bytes: 574_041_195,
    used: Use::Words,
};

pub const SEGMENTS: Model = Model {
    file: "pyannote-segmentation-3.0.onnx",
    url: "https://huggingface.co/csukuangfj/sherpa-onnx-pyannote-segmentation-3-0/resolve/main/model.onnx",
    sha256: "220ad67ca923bef2fa91f2390c786097bf305bceb5e261d4af67b38e938e1079",
    bytes: 5_992_913,
    used: Use::Segments,
};

pub const VOICES: Model = Model {
    file: "3dspeaker-campplus-zh-en-advanced.onnx",
    url: "https://huggingface.co/csukuangfj/speaker-embedding-models/resolve/main/3dspeaker_speech_campplus_sv_zh_en_16k-common_advanced.onnx",
    sha256: "aa3cfc16963a10586a9393f5035d6d6b57e98d358b347f80c2a30bf4f00ceba2",
    bytes: 28_281_164,
    used: Use::Voices,
};

/// Every model, the largest first.
pub const ALL: [Model; 3] = [WORDS, SEGMENTS, VOICES];

/// Those not yet in `dir` (as a whole file: a download is written beside
/// it, then renamed).
pub fn missing(dir: &Path) -> Vec<Model> {
    ALL.into_iter()
        .filter(|m| std::fs::metadata(dir.join(m.file)).map_or(true, |meta| meta.len() != m.bytes))
        .collect()
}

/// What `models` weigh together, in megabytes rounded to the ten, as the
/// asking popover says it ("610 Mo").
pub fn megabytes(models: &[Model]) -> u64 {
    let bytes: u64 = models.iter().map(|m| m.bytes).sum();
    (bytes + 5_000_000) / 10_000_000 * 10
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_missing_and_how_much_it_weighs() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(missing(dir.path()).len(), 3);
        assert_eq!(megabytes(&missing(dir.path())), 610);
        std::fs::write(
            dir.path().join(SEGMENTS.file),
            vec![0u8; SEGMENTS.bytes as usize],
        )?;
        // A part only is not the model.
        std::fs::write(dir.path().join(VOICES.file), b"part")?;
        assert_eq!(missing(dir.path()), vec![WORDS, VOICES]);
        assert_eq!(megabytes(&[]), 0);
        Ok(())
    }
}
