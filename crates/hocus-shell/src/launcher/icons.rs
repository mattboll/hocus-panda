//! The launcher's icons (5.1): found in the user's icon theme, then the
//! themes it inherits, then `hicolor`, then `pixmaps`, as freedesktop's
//! Icon Theme Specification looks them up, at the tiles' size on the
//! screen's scale; SVG drawn and PNG scaled to that size, on a thread of
//! their own, never on the one that draws.
//!
//! The user's theme is GTK's setting (`gtk-icon-theme-name` in
//! `gtk-4.0/settings.ini`, then `gtk-3.0`'s), else KDE's (`kdeglobals`'
//! `[Icons] Theme`), else GTK's own default, Adwaita, when it is there. A
//! theme without an `index.theme` (one copied by hand) is read from its
//! directories' names (`48x48`, `48x48@2`, `scalable`).
use std::{
    collections::HashMap,
    path::Path,
    sync::{
        Mutex, PoisonError,
        mpsc::{self, Receiver, Sender},
    },
};

pub use hocus_xdg::icons::Themes;
use iced::advanced::image;

/// Icon files larger than this are not read.
const LARGEST: u64 = 4 << 20;

/// Icon file `file` at `px` × `px` pixels: an SVG drawn at that size, a
/// PNG scaled to fit it.
pub fn load(file: &Path, px: u32) -> Option<image::Handle> {
    if std::fs::metadata(file).ok()?.len() > LARGEST {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    let px = px.clamp(8, 1024);
    let rgba = match file.extension().and_then(|e| e.to_str()) {
        Some("svg" | "svgz") => {
            let tree = resvg::usvg::Tree::from_data(&bytes, &resvg::usvg::Options::default())
                .inspect_err(|err| tracing::debug!(?file, %err, "an icon that does not read"))
                .ok()?;
            let size = tree.size();
            let k = px as f32 / size.width().max(size.height()).max(1.0);
            let mut pixmap = resvg::tiny_skia::Pixmap::new(px, px)?;
            let (dx, dy) = (
                (px as f32 - size.width() * k) / 2.0,
                (px as f32 - size.height() * k) / 2.0,
            );
            resvg::render(
                &tree,
                resvg::tiny_skia::Transform::from_row(k, 0.0, 0.0, k, dx, dy),
                &mut pixmap.as_mut(),
            );
            // tiny-skia's pixels are premultiplied; iced takes them not.
            let pixels: Vec<u8> = pixmap
                .pixels()
                .iter()
                .flat_map(|p| {
                    let c = p.demultiply();
                    [c.red(), c.green(), c.blue(), c.alpha()]
                })
                .collect();
            ::image::RgbaImage::from_raw(px, px, pixels)?
        }
        Some("png") => {
            let decoded = ::image::load_from_memory_with_format(&bytes, ::image::ImageFormat::Png)
                .inspect_err(|err| tracing::debug!(?file, %err, "an icon that does not read"))
                .ok()?
                .into_rgba8();
            let (w, h) = decoded.dimensions();
            if w.max(h) == px {
                decoded
            } else {
                let k = px as f32 / w.max(h).max(1) as f32;
                let (nw, nh) = (
                    ((w as f32 * k).round() as u32).max(1),
                    ((h as f32 * k).round() as u32).max(1),
                );
                // Catmull-Rom: a larger icon scaled down stays sharp.
                ::image::imageops::resize(
                    &decoded,
                    nw,
                    nh,
                    ::image::imageops::FilterType::CatmullRom,
                )
            }
        }
        _ => return None,
    };
    let (w, h) = rgba.dimensions();
    Some(image::Handle::from_rgba(w, h, rgba.into_raw()))
}

// --- The icons' thread ----------------------------------------------------------

/// An icon as looked for: at what size, and what was found.
#[derive(Debug, Clone)]
enum Looked {
    Asked,
    Found(Option<image::Handle>),
}

type Key = (String, u32);

static ICONS: Mutex<Option<HashMap<Key, Looked>>> = Mutex::new(None);
static ASK: Mutex<Option<Sender<Ask>>> = Mutex::new(None);

#[derive(Debug)]
enum Ask {
    Icon {
        name: String,
        size: u32,
        scale: f32,
    },
    /// The applications changed: the themes are read again.
    Again,
}

fn icons<T>(with: impl FnOnce(&mut HashMap<Key, Looked>) -> T) -> T {
    let mut icons = ICONS.lock().unwrap_or_else(PoisonError::into_inner);
    with(icons.get_or_insert_with(HashMap::new))
}

/// The pixels an icon `size` logical pixels wide takes at `scale`.
pub fn pixels(size: u32, scale: f32) -> u32 {
    (size as f32 * scale).round().max(1.0) as u32
}

/// Icon `name` at `px` pixels: `None` until it has been looked for, then
/// what was found.
pub fn of(name: &str, px: u32) -> Option<Option<image::Handle>> {
    icons(|icons| match icons.get(&(name.to_owned(), px)) {
        Some(Looked::Found(found)) => Some(found.clone()),
        _ => None,
    })
}

fn send(ask: Ask) {
    let mut sender = ASK.lock().unwrap_or_else(PoisonError::into_inner);
    if sender.is_none() {
        let (tx, rx) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("launcher's icons".to_owned())
            .spawn(move || look(&rx));
        match started {
            Ok(_) => *sender = Some(tx),
            Err(err) => {
                tracing::warn!(%err, "cannot look for the launcher's icons");
                return;
            }
        }
    }
    if let Some(tx) = sender.as_ref() {
        let _ = tx.send(ask);
    }
}

/// `names` are wanted `size` logical pixels wide at `scale`, in that
/// order: those not looked for yet are.
pub fn want<'a>(names: impl IntoIterator<Item = &'a str>, size: u32, scale: f32) {
    let px = pixels(size, scale);
    let new: Vec<String> = icons(|icons| {
        let mut new = Vec::new();
        for name in names {
            let key = (name.to_owned(), px);
            if !name.is_empty() && !icons.contains_key(&key) {
                icons.insert(key, Looked::Asked);
                new.push(name.to_owned());
            }
        }
        new
    });
    for name in new {
        send(Ask::Icon { name, size, scale });
    }
}

/// The applications changed (one installed may bring its icon): those not
/// found are looked for again when next wanted, in the themes read anew.
pub fn again() {
    icons(|icons| icons.retain(|_, looked| matches!(looked, Looked::Found(Some(_)))));
    if ASK.lock().unwrap_or_else(PoisonError::into_inner).is_some() {
        send(Ask::Again);
    }
}

/// The icons' thread: what is asked, and what was asked meanwhile, then one
/// wake for what was found.
fn look(asked: &Receiver<Ask>) {
    let mut themes = Themes::load(&super::entries::data_dirs());
    while let Ok(first) = asked.recv() {
        let mut found = 0;
        for ask in std::iter::once(first).chain(asked.try_iter()) {
            match ask {
                Ask::Again => themes = Themes::load(&super::entries::data_dirs()),
                Ask::Icon { name, size, scale } => {
                    let px = pixels(size, scale);
                    let icon = themes
                        .find(&name, size, scale.ceil().max(1.0) as u32)
                        .and_then(|file| load(&file, px));
                    tracing::debug!(icon = %name, px, found = icon.is_some(), "a launcher's icon");
                    found += usize::from(icon.is_some());
                    icons(|icons| icons.insert((name, px), Looked::Found(icon)));
                }
            }
        }
        if found > 0 {
            super::wake();
            crate::share::wake();
            crate::auth::wake();
            crate::popover::changed(crate::popover::Item::InUse);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_svg_is_drawn_at_the_size_asked() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let svg = dir.path().join("red.svg");
        std::fs::write(
            &svg,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\">\
             <rect width=\"16\" height=\"16\" fill=\"#ff0000\"/></svg>",
        )?;
        let handle = load(&svg, 96);
        assert!(
            matches!(
                &handle,
                Some(image::Handle::Rgba {
                    width: 96,
                    height: 96,
                    ..
                })
            ),
            "{handle:?}"
        );
        assert_eq!(load(&dir.path().join("none.svg"), 48), None);
        Ok(())
    }
}
