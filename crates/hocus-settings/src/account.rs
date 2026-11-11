//! The Compte page's model (6.11, settings.md "Compte"): what the search
//! finds on it, the eight pandas it offers as avatars, and pictures made
//! into avatars. Nothing of it is in the file: the name and the avatar are
//! AccountsService's ([`crate::accounts`]), the password the system's
//! (`passwd`, [`crate::passwd`]). The window's side is `app::account`.

use hocus_words::settings::account as words;
use std::path::{Path, PathBuf};

use crate::{
    lang::Words,
    model::{Outside, SectionId},
};

const PROFILE: Words = words::PROFILE;
const SECURITY: Words = words::SECURITY;

/// The rows, by their search ids.
pub const AVATAR: &str = "account.avatar";
pub const NAME: &str = "account.name";
pub const LOGIN: &str = "account.login";
pub const KIND: &str = "account.type";
pub const PASSWORD: &str = "account.password";
pub const LOCK: &str = "account.lock";

/// The page's settings, for the search: each lights its row.
pub fn outside() -> &'static [Outside] {
    const fn o(
        id: &'static str,
        group: Words,
        name: Words,
        fr: &'static [&'static str],
        en: &'static [&'static str],
    ) -> Outside {
        Outside {
            id,
            section: SectionId::Account,
            group,
            name,
            keywords: (fr, en),
        }
    }
    const ALL: [Outside; 5] = [
        o(
            AVATAR,
            PROFILE,
            words::AVATAR,
            &["photo", "image", "panda", "profil"],
            &["picture", "profile", "panda"],
        ),
        o(
            NAME,
            PROFILE,
            words::DISPLAY_NAME,
            &["nom", "prénom"],
            &["name", "full name"],
        ),
        o(
            LOGIN,
            PROFILE,
            words::LOGIN_NAME,
            &["login", "utilisateur"],
            &["user name", "login"],
        ),
        o(
            PASSWORD,
            SECURITY,
            words::PASSWORD,
            &["changer", "sécurité", "passwd"],
            &["password", "change", "passwd"],
        ),
        o(
            KIND,
            PROFILE,
            words::ACCOUNT_TYPE,
            &["administrateur", "sudo"],
            &["admin", "administrator"],
        ),
    ];
    &ALL
}

/// The pandas, 512 × 512 PNG, as `scripts/avatars/generate.py` drew them:
/// green-blue, blue, amber, pink, teal, grey, orange, lime.
pub const PANDAS: [&[u8]; 8] = [
    include_bytes!("../data/avatars/panda-1.png"),
    include_bytes!("../data/avatars/panda-2.png"),
    include_bytes!("../data/avatars/panda-3.png"),
    include_bytes!("../data/avatars/panda-4.png"),
    include_bytes!("../data/avatars/panda-5.png"),
    include_bytes!("../data/avatars/panda-6.png"),
    include_bytes!("../data/avatars/panda-7.png"),
    include_bytes!("../data/avatars/panda-8.png"),
];

/// A picture's side once made an avatar, at most.
pub const MOST: u32 = 512;
/// The side the window draws avatars from: 88 logical pixels at scale 2.
pub const SHOWN: u32 = 176;
/// The largest picture read.
pub const LARGEST: u64 = 16 << 20;

/// Where the packages install the pandas (`$XDG_DATA_DIRS/hocuspanda/
/// avatars`), or `HOCUS_AVATARS_DIR`.
pub fn installed(index: usize) -> Option<PathBuf> {
    let name = format!("panda-{}.png", index + 1);
    if let Some(dir) = std::env::var_os("HOCUS_AVATARS_DIR").filter(|d| !d.is_empty()) {
        return Some(Path::new(&dir).join(name)).filter(|p| p.is_file());
    }
    let dirs = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|d| !d.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    dirs.split(':')
        .map(|d| Path::new(d).join("hocuspanda/avatars").join(&name))
        .find(|p| p.is_file())
}

/// Which panda `bytes` are, if one.
pub fn which_panda(bytes: &[u8]) -> Option<usize> {
    PANDAS.iter().position(|p| *p == bytes)
}

/// `picture` cut square from its middle, made [`MOST`] wide at most.
pub fn square(picture: &image::DynamicImage) -> image::RgbaImage {
    let (width, height) = (picture.width(), picture.height());
    let side = width.min(height);
    let cut = picture.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let cut = if side > MOST {
        cut.resize_exact(MOST, MOST, image::imageops::FilterType::Lanczos3)
    } else {
        cut
    };
    cut.to_rgba8()
}

/// A picture as PNG.
pub fn png(picture: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    picture
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}

/// An avatar's file's bytes as the window draws it: square, [`SHOWN`]
/// wide, cut round (its edge smoothed); RGBA.
pub fn round(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let picture = image::load_from_memory(bytes).map_err(|e| e.to_string())?;
    let squared = square(&picture);
    let mut small = image::imageops::resize(
        &squared,
        SHOWN,
        SHOWN,
        image::imageops::FilterType::Triangle,
    );
    let middle = SHOWN as f32 / 2.0;
    for (x, y, pixel) in small.enumerate_pixels_mut() {
        let (dx, dy) = (x as f32 + 0.5 - middle, y as f32 + 0.5 - middle);
        let inside = (middle - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        pixel.0[3] = (f32::from(pixel.0[3]) * inside).round() as u8;
    }
    Ok(small.into_raw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_is_cut_square_from_its_middle_and_made_512_at_most() {
        let mut wide = image::RgbaImage::from_pixel(1200, 800, image::Rgba([200, 30, 30, 255]));
        // Its middle green, its sides red.
        for y in 0..800 {
            for x in 200..1000 {
                wide.put_pixel(x, y, image::Rgba([30, 200, 30, 255]));
            }
        }
        let cut = square(&image::DynamicImage::ImageRgba8(wide));
        assert_eq!((cut.width(), cut.height()), (512, 512));
        assert!(cut.pixels().all(|p| p.0[1] > 150), "only the middle");
        let small = image::RgbaImage::from_pixel(300, 100, image::Rgba([1, 2, 3, 255]));
        let cut = square(&image::DynamicImage::ImageRgba8(small));
        assert_eq!((cut.width(), cut.height()), (100, 100));
    }

    #[test]
    fn the_pandas_read_and_are_known_by_their_bytes() {
        for (i, panda) in PANDAS.iter().enumerate() {
            let shown = round(panda).expect("a panda reads");
            assert_eq!(shown.len(), (SHOWN * SHOWN * 4) as usize);
            // Its corner cut away, its middle there.
            assert_eq!(shown[3], 0);
            let middle = ((SHOWN / 2 * SHOWN + SHOWN / 2) * 4 + 3) as usize;
            assert_eq!(shown[middle], 255);
            assert_eq!(which_panda(panda), Some(i));
        }
        assert_eq!(which_panda(b"not a panda"), None);
    }

    #[test]
    fn every_setting_of_the_page_is_found_by_its_words() {
        let found = |q: &str, lang| {
            crate::search::find(q, lang)
                .iter()
                .map(|f| f.target.id())
                .collect::<Vec<_>>()
        };
        use crate::lang::Lang;
        assert!(found("panda", Lang::French).contains(&AVATAR.to_owned()));
        assert!(found("mot de passe", Lang::French).contains(&PASSWORD.to_owned()));
        assert!(found("passwd", Lang::English).contains(&PASSWORD.to_owned()));
        assert!(found("sudo", Lang::French).contains(&KIND.to_owned()));
        assert!(found("prénom", Lang::French).contains(&NAME.to_owned()));
    }
}
