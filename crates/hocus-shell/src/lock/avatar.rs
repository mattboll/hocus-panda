//! The lock's avatar (7.1, decided), the footer's and the session menu's
//! too (7.4): the account's photo, as
//! AccountsService has it (`org.freedesktop.Accounts` on the system bus,
//! the user's `IconFile`), when it has one; else the panda on the accent's
//! gradient.
//!
//! Asked once as the shell starts, on a thread of its own with a runtime
//! of its own, given up after a few seconds: no AccountsService, no system
//! bus, no file, or a picture that does not read (PNG or JPEG, the
//! codecs the shell has, or SVG, drawn by resvg; told by their content:
//! the file's name says nothing), and the panda stays. The photo is cut
//! square from its middle and made [`PIXELS`] wide once, off the drawing.

use std::{
    sync::{Mutex, PoisonError},
    time::Duration,
};

use iced::advanced::image::Handle;

/// The photo's side in pixels: the avatar's 88 px at scale 2.
const PIXELS: u32 = 176;
/// The largest file read.
const MOST: u64 = 8 << 20;
/// How long the system bus is given.
const WAIT: Duration = Duration::from_secs(5);
const ACCOUNTS: &str = "org.freedesktop.Accounts";

static PHOTO: Mutex<Option<Handle>> = Mutex::new(None);

/// The photo, once found.
pub fn photo() -> Option<Handle> {
    PHOTO.lock().unwrap_or_else(PoisonError::into_inner).clone()
}

/// Looks the account's photo up, off the shell's thread.
pub fn start() {
    let looked = std::thread::Builder::new()
        .name("lock: avatar".to_owned())
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let file = match runtime {
                Ok(runtime) => runtime.block_on(async {
                    tokio::time::timeout(WAIT, icon_file())
                        .await
                        .unwrap_or_else(|_| Err("AccountsService did not answer".into()))
                }),
                Err(err) => Err(err.to_string()),
            };
            let file = match file {
                Ok(file) => file,
                Err(why) => {
                    tracing::debug!(why, "no account photo: the panda");
                    return;
                }
            };
            match read(&file) {
                Ok(handle) => {
                    tracing::info!("the lock shows the account's photo");
                    *PHOTO.lock().unwrap_or_else(PoisonError::into_inner) = Some(handle);
                    crate::popover::tell(hocus_link::ToCompositor::Wake(
                        hocus_link::LOCK_NAMESPACE.to_owned(),
                    ));
                    // The footer's and the session menu's avatars (7.4).
                    crate::popover::tell(hocus_link::ToCompositor::Wake(
                        hocus_link::FOOTER_NAMESPACE.to_owned(),
                    ));
                    crate::popover::changed(crate::popover::Item::User);
                }
                Err(why) => tracing::info!(why, "the account's photo does not read: the panda"),
            }
        });
    if let Err(err) = looked {
        tracing::warn!(%err, "cannot look the account's photo up");
    }
}

/// The user's `IconFile`, as AccountsService says it.
async fn icon_file() -> Result<std::path::PathBuf, String> {
    let connection = zbus::Connection::system()
        .await
        .map_err(|e| e.to_string())?;
    let uid = i64::from(rustix::process::getuid().as_raw());
    let reply = connection
        .call_method(
            Some(ACCOUNTS),
            "/org/freedesktop/Accounts",
            Some(ACCOUNTS),
            "FindUserById",
            &(uid,),
        )
        .await
        .map_err(|e| e.to_string())?;
    let user: zbus::OwnedObjectPath = reply.body().deserialize().map_err(|e| e.to_string())?;
    let reply = connection
        .call_method(
            Some(ACCOUNTS),
            user.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.Accounts.User", "IconFile"),
        )
        .await
        .map_err(|e| e.to_string())?;
    let value: zbus::OwnedVariant = reply.body().deserialize().map_err(|e| e.to_string())?;
    let file = String::try_from(&*value).map_err(|e| e.to_string())?;
    if file.is_empty() {
        return Err("no IconFile".to_owned());
    }
    Ok(file.into())
}

/// The photo at `file`, cut square from its middle, [`PIXELS`] wide.
fn read(file: &std::path::Path) -> Result<Handle, String> {
    let size = std::fs::metadata(file).map_err(|e| e.to_string())?.len();
    if size > MOST {
        return Err(format!("{size} bytes"));
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    Ok(square(&decode(&bytes)?))
}

/// A photo's pixels, its format told by its first bytes: AccountsService
/// keeps what it was given, a JPEG or an SVG as well as a PNG.
fn decode(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    if let Some(svg) = svg(bytes) {
        return draw_svg(svg);
    }
    image::load_from_memory(bytes)
        .map(image::DynamicImage::into_rgba8)
        .map_err(|e| e.to_string())
}

/// An SVG's XML from its first `<`, after a byte order mark or blanks
/// (which the XML parser refuses before a declaration): neither a PNG
/// nor a JPEG begins with `<`.
fn svg(bytes: &[u8]) -> Option<&[u8]> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace())?;
    (bytes[start] == b'<').then(|| &bytes[start..])
}

/// An SVG drawn so that its shorter side is [`PIXELS`]: sharp once cut
/// square, never drawn larger than shown.
fn draw_svg(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|e| e.to_string())?;
    let size = tree.size();
    let k = PIXELS as f32 / size.width().min(size.height()).max(1.0);
    // At most PIXELS × 4 on its longer side: a strip's photo stays small.
    let side = |s: f32| ((s * k).round() as u32).clamp(1, PIXELS * 4);
    let (w, h) = (side(size.width()), side(size.height()));
    let mut pixmap =
        resvg::tiny_skia::Pixmap::new(w, h).ok_or_else(|| format!("{w}x{h}: no pixmap"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(k, k),
        &mut pixmap.as_mut(),
    );
    // tiny-skia's pixels are premultiplied; iced takes them not.
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    image::RgbaImage::from_raw(w, h, pixels).ok_or_else(|| "pixels lost".to_owned())
}

fn square(picture: &image::RgbaImage) -> Handle {
    let (w, h) = picture.dimensions();
    let side = w.min(h).max(1);
    let cut =
        image::imageops::crop_imm(picture, (w - side) / 2, (h - side) / 2, side, side).to_image();
    let made = image::imageops::resize(&cut, PIXELS, PIXELS, image::imageops::FilterType::Triangle);
    Handle::from_rgba(PIXELS, PIXELS, made.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_photo_is_cut_square_from_its_middle() {
        let wide = image::RgbaImage::from_fn(300, 200, |x, _| {
            if (50..250).contains(&x) {
                image::Rgba([255, 0, 0, 255])
            } else {
                image::Rgba([0, 0, 255, 255])
            }
        });
        let Handle::Rgba {
            width,
            height,
            pixels,
            ..
        } = square(&wide)
        else {
            panic!("not pixels");
        };
        assert_eq!((width, height), (PIXELS, PIXELS));
        // Its middle's red, edge to edge.
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn a_photo_reads_as_a_png_a_jpeg_or_an_svg() {
        let photo = image::RgbImage::from_pixel(40, 30, image::Rgb([200, 30, 30]));
        for format in [image::ImageFormat::Png, image::ImageFormat::Jpeg] {
            let mut bytes = std::io::Cursor::new(Vec::new());
            photo.write_to(&mut bytes, format).expect("encoded");
            let read = decode(bytes.get_ref()).expect("decoded");
            assert_eq!(read.dimensions(), (40, 30), "{format:?}");
            let [r, g, _, a] = read.get_pixel(20, 15).0;
            assert!(r > 180 && g < 60 && a == 255, "{format:?}: {r} {g}");
        }
        // A BOM and a blank line before its XML, as editors leave them.
        let svg = "\u{feff}\n<?xml version=\"1.0\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" \
                   width=\"60\" height=\"30\"><rect width=\"60\" height=\"30\" \
                   fill=\"#c81e1e\"/></svg>";
        let read = decode(svg.as_bytes()).expect("drawn");
        // Its shorter side made the photo's.
        assert_eq!(read.dimensions(), (PIXELS * 2, PIXELS));
        let [r, g, _, a] = read.get_pixel(PIXELS / 2, PIXELS / 2).0;
        assert!(r > 180 && g < 60 && a == 255, "svg: {r} {g}");
        assert!(decode(b"not a picture").is_err());
        assert!(decode(b"<svg broken").is_err());
    }
}
