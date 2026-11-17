//! The tracks' files: WAV, 16 kHz, mono, 16-bit, written as they are
//! recorded (their header's sizes set as they end) and read whole.
//!
//! A track whose recording did not end (the session went) keeps sizes of
//! 0 in its header: it is read to its file's end all the same.

use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
};

use crate::RATE;

const HEADER: usize = 44;

/// A track being written.
#[derive(Debug)]
pub struct Writer {
    file: io::BufWriter<File>,
    samples: u64,
}

impl Writer {
    /// A new track at `path`, its header's sizes to be set.
    pub fn create(path: &Path) -> io::Result<Self> {
        let mut file = io::BufWriter::new(File::create(path)?);
        file.write_all(&header(0))?;
        Ok(Self { file, samples: 0 })
    }

    /// `samples` (-1 to 1) added, as 16-bit.
    pub fn write(&mut self, samples: &[f32]) -> io::Result<()> {
        let mut bytes = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        self.file.write_all(&bytes)?;
        self.samples += samples.len() as u64;
        Ok(())
    }

    /// How many samples it holds.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// The track ends: its header says its sizes.
    pub fn finish(self) -> io::Result<()> {
        let mut file = self
            .file
            .into_inner()
            .map_err(io::IntoInnerError::into_error)?;
        let data = u32::try_from(self.samples * 2).unwrap_or(u32::MAX);
        file.seek(SeekFrom::Start(0))?;
        file.write_all(&header(data))?;
        file.sync_all()
    }
}

/// The 44 bytes of a 16 kHz, mono, 16-bit WAV of `data` bytes.
fn header(data: u32) -> [u8; HEADER] {
    let mut h = [0u8; HEADER];
    let mut at = 0;
    let mut put = |bytes: &[u8]| {
        for b in bytes {
            if let Some(slot) = h.get_mut(at) {
                *slot = *b;
            }
            at += 1;
        }
    };
    put(b"RIFF");
    put(&data.saturating_add(36).to_le_bytes());
    put(b"WAVE");
    put(b"fmt ");
    put(&16u32.to_le_bytes());
    put(&1u16.to_le_bytes());
    put(&1u16.to_le_bytes());
    put(&RATE.to_le_bytes());
    put(&(RATE * 2).to_le_bytes());
    put(&2u16.to_le_bytes());
    put(&16u16.to_le_bytes());
    put(b"data");
    put(&data.to_le_bytes());
    h
}

/// A track's samples (-1 to 1): its `data` chunk's (another program's
/// WAV may have others before it, a `LIST`), to its file's end when its
/// size was not set.
pub fn read(path: &Path) -> io::Result<Vec<f32>> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.get(..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a WAV file"));
    }
    let mut at = 12;
    let data = loop {
        let (Some(id), Some(size)) = (bytes.get(at..at + 4), bytes.get(at + 4..at + 8)) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "no data in the WAV file",
            ));
        };
        let size = u32::from_le_bytes(size.try_into().unwrap_or([0; 4])) as usize;
        if id == b"data" {
            let end = match size {
                0 => bytes.len(),
                size => (at + 8 + size).min(bytes.len()),
            };
            break bytes.get(at + 8..end).unwrap_or_default();
        }
        at += 8 + size + size % 2;
    };
    Ok(data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| f32::from(i16::from_le_bytes(*b)) / f32::from(i16::MAX))
        .collect())
}

/// A track's length, in seconds, from its file's size.
pub fn seconds(path: &Path) -> io::Result<f64> {
    let len = std::fs::metadata(path)?.len().saturating_sub(HEADER as u64);
    Ok(len as f64 / 2.0 / f64::from(RATE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_track_written_reads_back_ended_or_not() -> io::Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("me.wav");
        let mut writer = Writer::create(&path)?;
        writer.write(&[0.0, 0.5, -0.5, 1.0])?;
        writer.write(&[2.0])?;
        assert_eq!(writer.samples(), 5);
        writer.finish()?;
        let read = read(&path)?;
        assert_eq!(read.len(), 5);
        assert!((read[1] - 0.5).abs() < 1e-3 && (read[4] - 1.0).abs() < 1e-6);
        let bytes = std::fs::read(&path)?;
        assert_eq!(&bytes[40..44], &10u32.to_le_bytes());
        // Not ended: sizes of 0, read all the same.
        let open = dir.path().join("call.wav");
        let mut writer = Writer::create(&open)?;
        writer.write(&[0.25; 16_000])?;
        drop(writer);
        assert_eq!(super::read(&open)?.len(), 16_000);
        assert!((seconds(&open)? - 1.0).abs() < 1e-9);
        Ok(())
    }
}
