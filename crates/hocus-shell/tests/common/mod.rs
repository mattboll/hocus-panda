//! Comparing surfaces with reference images; `HOCUS_BLESS=1` records them.

use std::path::{Path, PathBuf};

use hocus_shell::column::View;

fn references() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/references")
}

/// Premultiplied to straight alpha, for PNG.
fn straight(premultiplied: &[u8]) -> Vec<u8> {
    let mut out = premultiplied.to_vec();
    for px in out.as_chunks_mut::<4>().0 {
        let a = u32::from(px[3]);
        if a > 0 && a < 255 {
            for c in &mut px[..3] {
                *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
    out
}

fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> anyhow::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = png::Encoder::new(file, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(rgba)?;
    Ok(())
}

fn read_png(path: &Path) -> anyhow::Result<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(path)?));
    let mut reader = decoder.read_info()?;
    let mut buf = vec![0; reader.output_buffer_size().unwrap_or_default()];
    let info = reader.next_frame(&mut buf)?;
    buf.truncate(info.buffer_size());
    anyhow::ensure!(
        info.color_type == png::ColorType::Rgba,
        "{} is not RGBA",
        path.display()
    );
    Ok((info.width, info.height, buf))
}

/// The surface matches `name` within a small tolerance (renderers may round
/// differently on other CPUs).
pub fn expect_reference(view: &View<'_>, name: &str) -> anyhow::Result<()> {
    let path = references().join(name);
    let actual = straight(view.pixels);
    let (w, h) = (view.width, view.height);
    if std::env::var_os("HOCUS_BLESS").is_some_and(|v| v == "1") {
        return write_png(&path, w, h, &actual);
    }
    let (rw, rh, expected) = read_png(&path).map_err(|e| {
        anyhow::anyhow!(
            "{}: {e}; run with HOCUS_BLESS=1 to record it",
            path.display()
        )
    })?;
    anyhow::ensure!(
        (rw, rh) == (w, h),
        "{name}: {rw}x{rh} expected, got {w}x{h}"
    );
    let differ = actual
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .filter(|(a, e)| a.iter().zip(*e).any(|(a, e)| a.abs_diff(*e) > 8))
        .count();
    if differ > 0 {
        let out = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-artifacts/hocus-shell")
            .join(name);
        write_png(&out, w, h, &actual)?;
        anyhow::bail!(
            "{name}: {differ} pixels differ; the actual image is in {}",
            out.display()
        );
    }
    Ok(())
}
