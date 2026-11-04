//! The lock's avatar (7.1, decided), the footer's and the session menu's
//! too (7.4): the account's photo, as
//! AccountsService has it (`org.freedesktop.Accounts` on the system bus,
//! the user's `IconFile`), when it has one; else the panda on the accent's
//! gradient.
//!
//! Asked once as the shell starts, on a thread of its own with a runtime
//! of its own, given up after a few seconds: no AccountsService, no system
//! bus, no file, or a picture that does not read (PNG only, the codec the
//! shell has), and the panda stays. The photo is cut square from its
//! middle and made [`PIXELS`] wide once, off the drawing.

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
    let picture = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    Ok(square(&picture))
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
}
