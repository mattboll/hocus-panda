//! Just enough of ZIP to read office files (OpenDocument and OOXML are
//! XML in a ZIP): the central directory, and an entry stored or deflated,
//! never more than a limit once inflated.

use std::io::Read;

/// An entry, as the central directory lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    method: u16,
    compressed: u64,
    /// Where its local header starts.
    offset: u64,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The entries of the archive `data`; `None` when it is not one.
pub fn entries(data: &[u8]) -> Option<Vec<Entry>> {
    // The end record is in the last 64 KiB (its comment) and 22 bytes.
    let from = data.len().saturating_sub(22 + 0xFFFF);
    let end = (from..data.len().saturating_sub(21))
        .rev()
        .find(|&i| u32_at(data, i) == Some(0x0605_4b50))?;
    let count = u16_at(data, end + 10)? as usize;
    let mut at = u32_at(data, end + 16)? as usize;
    let mut out = Vec::with_capacity(count.min(4096));
    for _ in 0..count {
        if u32_at(data, at)? != 0x0201_4b50 {
            return None;
        }
        let method = u16_at(data, at + 10)?;
        let compressed = u64::from(u32_at(data, at + 20)?);
        let name_len = u16_at(data, at + 28)? as usize;
        let extra_len = u16_at(data, at + 30)? as usize;
        let comment_len = u16_at(data, at + 32)? as usize;
        let offset = u64::from(u32_at(data, at + 42)?);
        let name = data.get(at + 46..at + 46 + name_len)?;
        out.push(Entry {
            name: String::from_utf8_lossy(name).into_owned(),
            method,
            compressed,
            offset,
        });
        at += 46 + name_len + extra_len + comment_len;
    }
    Some(out)
}

/// What `entry` holds, inflated, at most `limit` bytes of it; `None` when
/// it is compressed in a way not read here, or broken.
pub fn read(data: &[u8], entry: &Entry, limit: usize) -> Option<Vec<u8>> {
    let at = usize::try_from(entry.offset).ok()?;
    if u32_at(data, at)? != 0x0403_4b50 {
        return None;
    }
    let name_len = u16_at(data, at + 26)? as usize;
    let extra_len = u16_at(data, at + 28)? as usize;
    let start = at + 30 + name_len + extra_len;
    let length = usize::try_from(entry.compressed).ok()?;
    let raw = data.get(start..start.checked_add(length)?)?;
    match entry.method {
        0 => Some(raw[..raw.len().min(limit)].to_vec()),
        8 => {
            let mut out = Vec::new();
            flate2::read::DeflateDecoder::new(raw)
                .take(limit as u64)
                .read_to_end(&mut out)
                .ok()?;
            Some(out)
        }
        _ => None,
    }
}

/// Writes a ZIP of `files`, deflated or stored, for tests.
#[cfg(test)]
pub fn write(files: &[(&str, &[u8])], deflate: bool) -> Vec<u8> {
    use std::io::Write;
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, contents) in files {
        let body = if deflate {
            let mut e =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            let _ = e.write_all(contents);
            e.finish().unwrap_or_default()
        } else {
            contents.to_vec()
        };
        let method: u16 = if deflate { 8 } else { 0 };
        let offset = out.len() as u32;
        let crc = 0u32;
        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&[20, 0, 0, 0]);
        out.extend_from_slice(&method.to_le_bytes());
        out.extend_from_slice(&[0, 0, 0, 0]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        out.extend_from_slice(&(name.len() as u16).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&body);
        central.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        central.extend_from_slice(&[20, 0, 20, 0, 0, 0]);
        central.extend_from_slice(&method.to_le_bytes());
        central.extend_from_slice(&[0, 0, 0, 0]);
        central.extend_from_slice(&crc.to_le_bytes());
        central.extend_from_slice(&(body.len() as u32).to_le_bytes());
        central.extend_from_slice(&(contents.len() as u32).to_le_bytes());
        central.extend_from_slice(&(name.len() as u16).to_le_bytes());
        central.extend_from_slice(&[0; 12]);
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name.as_bytes());
    }
    let start = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&[0, 0, 0, 0]);
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(files.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&start.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stored_and_deflated_entries_read_back() {
        for deflate in [false, true] {
            let zip = write(
                &[
                    ("mimetype", b"text/plain"),
                    ("content.xml", b"<a>hello</a>"),
                ],
                deflate,
            );
            let all = entries(&zip).expect("a zip");
            assert_eq!(all.len(), 2);
            let content = all
                .iter()
                .find(|e| e.name == "content.xml")
                .expect("listed");
            assert_eq!(
                read(&zip, content, 1 << 20).as_deref(),
                Some(&b"<a>hello</a>"[..])
            );
            assert_eq!(read(&zip, content, 3).as_deref(), Some(&b"<a>"[..]));
        }
    }

    #[test]
    fn what_is_not_a_zip_reads_as_nothing() {
        assert_eq!(entries(b"%PDF-1.4 hello"), None);
        assert_eq!(entries(&[]), None);
        let mut zip = write(&[("a", b"b")], true);
        let len = zip.len();
        zip[len - 6] = 0xFF; // the directory's offset, out of the file
        assert_eq!(entries(&zip), None);
    }
}
