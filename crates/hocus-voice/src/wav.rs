//! WAV files, as the tests and the listening samples need them: 16-bit
//! or 32-bit float PCM read (its first channel), 16-bit mono written.

use std::io;

/// The samples of a WAV file's first channel, from -1 to 1, and its rate.
pub fn read(bytes: &[u8]) -> io::Result<(Vec<f32>, u32)> {
    let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_owned());
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(bad("not a WAV file"));
    }
    let u16_at = |at: usize| {
        bytes.get(at..at + 2).map(|b| {
            u16::from_le_bytes([
                b.first().copied().unwrap_or(0),
                b.get(1).copied().unwrap_or(0),
            ])
        })
    };
    let u32_at = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_le_bytes)
    };
    let mut at = 12;
    let mut format = None;
    while let (Some(id), Some(len)) = (bytes.get(at..at + 4), u32_at(at + 4)) {
        let body = at + 8;
        let len = len as usize;
        match id {
            b"fmt " => {
                let tag = u16_at(body).ok_or_else(|| bad("short fmt"))?;
                let channels = u16_at(body + 2).ok_or_else(|| bad("short fmt"))?;
                let rate = u32_at(body + 4).ok_or_else(|| bad("short fmt"))?;
                let bits = u16_at(body + 14).ok_or_else(|| bad("short fmt"))?;
                format = Some((tag, channels.max(1) as usize, rate, bits));
            }
            b"data" => {
                let (tag, channels, rate, bits) = format.ok_or_else(|| bad("data before fmt"))?;
                let data = bytes
                    .get(body..(body + len).min(bytes.len()))
                    .unwrap_or_default();
                let samples = match (tag, bits) {
                    (1, 16) => data
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .step_by(channels)
                        .map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0)
                        .collect(),
                    (3, 32) => data
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .step_by(channels)
                        .map(|b| f32::from_le_bytes(*b))
                        .collect(),
                    _ => return Err(bad("neither 16-bit nor float samples")),
                };
                return Ok((samples, rate));
            }
            _ => {}
        }
        at = body + len + (len & 1);
    }
    Err(bad("no data"))
}

/// `samples` as a 16-bit mono WAV file at `rate`.
pub fn write(samples: &[f32], rate: u32) -> Vec<u8> {
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    out
}

/// The tests' voice (`data/speech.wav`: three seconds of a reading in
/// French, LibriVox's, public domain), 48 kHz.
#[cfg(any(test, feature = "test-speech"))]
pub fn test_speech() -> Vec<f32> {
    static WAV: &[u8] = include_bytes!("../data/speech.wav");
    read(WAV).map(|(s, _)| s).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_written_reads_back() {
        let samples: Vec<f32> = (0..100).map(|i| (i as f32 / 50.0) - 1.0).collect();
        let (back, rate) = read(&write(&samples, 48_000)).unwrap();
        assert_eq!(rate, 48_000);
        for (a, b) in back.iter().zip(&samples) {
            assert!((a - b).abs() < 1e-4);
        }
        assert_eq!(test_speech().len(), 3 * 48_000);
    }
}
