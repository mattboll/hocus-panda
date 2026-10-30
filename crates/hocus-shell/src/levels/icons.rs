//! Applications' icons, for the sound's popover and the desktops' tiles
//! and lists (3.9b, by their windows' application ids): found by name as
//! the freedesktop specifications place them, simplified.
//!
//! A name is an icon's own (`application.icon-name`) or a desktop file's
//! (`<name>.desktop`, whose `Icon` names the icon). Icons are looked for in
//! the `hicolor` theme, where applications install theirs (the icon theme
//! specification asks them to), in the sizes the popover draws best first,
//! then in `pixmaps`; the data directories are `XDG_DATA_HOME` and
//! `XDG_DATA_DIRS`. Read on the sound's thread (the desktops' icons' for
//! windows), never on the one that draws, once per application.

use std::path::{Path, PathBuf};

use iced::advanced::{image, svg};

/// An icon read, for the canvas.
#[derive(Debug, Clone, PartialEq)]
pub enum Icon {
    Raster(image::Handle),
    Vector(svg::Handle),
}

/// `hicolor`'s directories, the best for a 32 px tile (64 at ×2) first.
const SIZES: [&str; 9] = [
    "scalable", "64x64", "48x48", "128x128", "32x32", "256x256", "96x96", "512x512", "24x24",
];

/// Icons larger than this are not read.
const LARGEST: u64 = 2 << 20;

/// The icon of the first of `names` that has one.
pub fn find(names: &[String]) -> Option<Icon> {
    let dirs = data_dirs();
    names.iter().find_map(|name| {
        let file = icon_file(&dirs, name).or_else(|| {
            let icon = desktop_icon(&dirs, name)?;
            icon_file(&dirs, &icon)
        })?;
        read(&file)
    })
}

/// `XDG_DATA_HOME` (or `~/.local/share`), then `XDG_DATA_DIRS` (or
/// `/usr/local/share:/usr/share`).
fn data_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(std::env::split_paths(&system))
        .filter(|d| d.is_absolute())
        .collect()
}

/// The file of icon `name`: a path itself, or in `hicolor` or `pixmaps`.
fn icon_file(dirs: &[PathBuf], name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_absolute() {
        return path.is_file().then(|| path.to_owned());
    }
    if name.contains('/') || name.is_empty() {
        return None;
    }
    let themed = dirs.iter().flat_map(|dir| {
        SIZES.iter().flat_map(move |size| {
            let apps = dir.join("icons/hicolor").join(size).join("apps");
            ["svg", "png"].map(|ext| apps.join(format!("{name}.{ext}")))
        })
    });
    let pixmaps = dirs
        .iter()
        .flat_map(|dir| ["svg", "png"].map(|ext| dir.join(format!("pixmaps/{name}.{ext}"))));
    themed.chain(pixmaps).find(|p| p.is_file())
}

/// The `Icon` of desktop file `<id>.desktop`, if there is one.
fn desktop_icon(dirs: &[PathBuf], id: &str) -> Option<String> {
    if id.contains('/') {
        return None;
    }
    dirs.iter().find_map(|dir| {
        let file = dir.join("applications").join(format!("{id}.desktop"));
        let text = std::fs::read_to_string(file).ok()?;
        entry_icon(&text)
    })
}

/// The `Icon` key of a desktop file's `[Desktop Entry]` group.
fn entry_icon(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
        } else if in_entry
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "Icon"
        {
            return Some(value.trim().to_owned()).filter(|v| !v.is_empty());
        }
    }
    None
}

fn read(file: &Path) -> Option<Icon> {
    if std::fs::metadata(file).ok()?.len() > LARGEST {
        return None;
    }
    let bytes = std::fs::read(file).ok()?;
    match file.extension().and_then(|e| e.to_str()) {
        Some("svg") => Some(Icon::Vector(svg::Handle::from_memory(bytes))),
        Some("png") => {
            // Decoded here: the renderer then shows it in its first frame
            // (it decodes encoded images in the background, and a canvas
            // is not drawn again when that is done).
            let decoded = ::image::load_from_memory_with_format(&bytes, ::image::ImageFormat::Png)
                .inspect_err(|err| tracing::debug!(?file, %err, "an icon that does not read"))
                .ok()?
                .into_rgba8();
            let (width, height) = decoded.dimensions();
            Some(Icon::Raster(image::Handle::from_rgba(
                width,
                height,
                decoded.into_raw(),
            )))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1×1 PNG.
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F,
        0x15, 0xC4, 0x89, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0xF8,
        0xCF, 0xC0, 0xF0, 0x1F, 0x00, 0x05, 0x00, 0x01, 0xFF, 0x89, 0x99, 0x3D, 0x1D, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];

    #[test]
    fn an_icon_by_its_name_or_its_desktop_file() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let data = dir.path().to_owned();
        let scalable = data.join("icons/hicolor/scalable/apps");
        let sized = data.join("icons/hicolor/48x48/apps");
        for d in [&scalable, &sized, &data.join("applications")] {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(sized.join("music.png"), PNG)?;
        std::fs::write(scalable.join("music.svg"), b"<svg/>")?;
        std::fs::write(sized.join("web-browser.png"), b"png")?;
        std::fs::write(
            data.join("applications/browser.desktop"),
            "[Desktop Entry]\nName=Browser\nIcon=web-browser\n[Desktop Action new]\nIcon=other\n",
        )?;
        let dirs = vec![data.clone()];
        // The scalable one first.
        assert_eq!(icon_file(&dirs, "music"), Some(scalable.join("music.svg")));
        // Through the desktop file.
        assert_eq!(
            desktop_icon(&dirs, "browser").as_deref(),
            Some("web-browser")
        );
        assert_eq!(
            icon_file(&dirs, "web-browser"),
            Some(sized.join("web-browser.png"))
        );
        assert_eq!(icon_file(&dirs, "nothing"), None);
        assert_eq!(icon_file(&dirs, "../music"), None);
        assert!(matches!(
            read(&scalable.join("music.svg")),
            Some(Icon::Vector(_))
        ));
        // A PNG, decoded; one that is not, none.
        assert!(matches!(
            read(&sized.join("music.png")),
            Some(Icon::Raster(_))
        ));
        assert_eq!(read(&sized.join("web-browser.png")), None);
        Ok(())
    }

    #[test]
    fn a_desktop_files_icon_is_its_entrys() {
        assert_eq!(
            entry_icon("[Desktop Action x]\nIcon=no\n[Desktop Entry]\nIcon = firefox\n").as_deref(),
            Some("firefox")
        );
        assert_eq!(entry_icon("[Desktop Entry]\nName=x\n"), None);
    }
}
