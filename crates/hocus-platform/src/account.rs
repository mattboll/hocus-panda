//! The user's account as the system keeps it: the name and picture
//! through AccountsService (`org.freedesktop.Accounts`), the fingerprints
//! enrolled through fprintd (`net.reactivated.Fprint`), both on the system
//! bus. Changing the name or the picture of one's own account is allowed
//! by polkit to the user at the screen. Where a service is missing, it
//! says so and nothing is changed.
//!
//! Everything here blocks: for a worker thread, never a loop.

use std::path::{Path, PathBuf};

use zbus::{Connection, Proxy};

const ACCOUNTS: &str = "org.freedesktop.Accounts";
const ACCOUNTS_PATH: &str = "/org/freedesktop/Accounts";
const USER: &str = "org.freedesktop.Accounts.User";
const FPRINT: &str = "net.reactivated.Fprint";
const FPRINT_MANAGER: &str = "/net/reactivated/Fprint/Manager";
/// The largest side of the picture kept, pixels.
const PICTURE: u32 = 128;

/// What the system says of the account.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccountInfo {
    pub user: UserState,
    pub fingerprint: Fingerprint,
}

/// The account as AccountsService knows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum UserState {
    /// Not asked yet.
    #[default]
    Unknown,
    /// No AccountsService to ask, and why: nothing is changed here.
    Absent(String),
    Known(User),
}

/// The account's own data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// "Alex Panda"; may be empty.
    pub real_name: String,
    /// The picture's file, when set.
    pub icon_file: Option<PathBuf>,
    /// The picture as premultiplied RGBA, width and height, when it reads
    /// (PNG only).
    pub picture: Option<(u16, u16, Vec<u8>)>,
}

/// The fingerprint reader and what it holds for the user.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Fingerprint {
    /// Not asked yet.
    #[default]
    Unknown,
    /// No fprintd on the bus.
    NoService,
    /// fprintd, without a reader.
    NoReader,
    /// A reader, and the fingers enrolled (`right-index-finger`), maybe
    /// none.
    Reader { name: String, fingers: Vec<String> },
}

async fn connect(bus: &Option<String>) -> zbus::Result<Connection> {
    match bus {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())
                .build()
                .await
        }
        None => Connection::system().await,
    }
}

async fn proxy(
    conn: &Connection,
    service: &'static str,
    path: zbus::OwnedObjectPath,
    interface: &'static str,
) -> zbus::Result<Proxy<'static>> {
    zbus::proxy::Builder::new(conn)
        .destination(service)
        .path(path)
        .interface(interface)
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
}

fn path(p: &str) -> zbus::OwnedObjectPath {
    zbus::OwnedObjectPath::try_from(p).unwrap_or_else(|_| unreachable!("{p}"))
}

async fn user_proxy(conn: &Connection, name: &str) -> zbus::Result<Proxy<'static>> {
    let accounts = proxy(conn, ACCOUNTS, path(ACCOUNTS_PATH), ACCOUNTS).await?;
    let user: zbus::OwnedObjectPath = accounts.call("FindUserByName", &(name,)).await?;
    proxy(conn, ACCOUNTS, user, USER).await
}

/// Reads what the services say of `name`'s account, on `bus` (the
/// machine's system bus when `None`).
pub fn read(bus: &Option<String>, name: &str) -> AccountInfo {
    let result = zbus::block_on(async {
        let conn = connect(bus).await?;
        let user = match read_user(&conn, name).await {
            Ok(user) => UserState::Known(user),
            Err(err) => UserState::Absent(reason(&err)),
        };
        let fingerprint = read_fingerprint(&conn, name).await;
        zbus::Result::Ok(AccountInfo { user, fingerprint })
    });
    result.unwrap_or_else(|err| AccountInfo {
        user: UserState::Absent(reason(&err)),
        fingerprint: Fingerprint::NoService,
    })
}

async fn read_user(conn: &Connection, name: &str) -> zbus::Result<User> {
    let user = user_proxy(conn, name).await?;
    let real_name: String = user.get_property("RealName").await?;
    let icon: String = user.get_property("IconFile").await.unwrap_or_default();
    let icon_file = (!icon.is_empty()).then(|| PathBuf::from(icon));
    let picture = icon_file.as_deref().and_then(picture);
    Ok(User {
        real_name,
        icon_file,
        picture,
    })
}

async fn read_fingerprint(conn: &Connection, name: &str) -> Fingerprint {
    let Ok(manager) = proxy(
        conn,
        FPRINT,
        path(FPRINT_MANAGER),
        "net.reactivated.Fprint.Manager",
    )
    .await
    else {
        return Fingerprint::NoService;
    };
    let device: zbus::OwnedObjectPath = match manager.call("GetDefaultDevice", &()).await {
        Ok(device) => device,
        Err(zbus::Error::MethodError(error, _, _)) if error.as_str().ends_with("NoSuchDevice") => {
            return Fingerprint::NoReader;
        }
        Err(_) => return Fingerprint::NoService,
    };
    let Ok(device) = proxy(conn, FPRINT, device, "net.reactivated.Fprint.Device").await else {
        return Fingerprint::NoService;
    };
    let reader: String = device.get_property("name").await.unwrap_or_default();
    // None enrolled is an error of its own to fprintd.
    let fingers: Vec<String> = device
        .call("ListEnrolledFingers", &(name,))
        .await
        .unwrap_or_default();
    Fingerprint::Reader {
        name: reader,
        fingers,
    }
}

/// Sets the account's full name; polkit may ask.
pub fn set_real_name(bus: &Option<String>, name: &str, real_name: &str) -> Result<(), String> {
    zbus::block_on(async {
        let conn = connect(bus).await?;
        user_proxy(&conn, name)
            .await?
            .call::<_, _, ()>("SetRealName", &(real_name,))
            .await
    })
    .map_err(|err| reason(&err))
}

/// Sets the account's picture from an image file, which the service
/// copies; polkit may ask.
pub fn set_icon_file(bus: &Option<String>, name: &str, file: &Path) -> Result<(), String> {
    if !file.is_absolute() || !file.is_file() {
        return Err(format!("{} is not a file", file.display()));
    }
    let file = file.to_string_lossy().into_owned();
    zbus::block_on(async {
        let conn = connect(bus).await?;
        user_proxy(&conn, name)
            .await?
            .call::<_, _, ()>("SetIconFile", &(file.as_str(),))
            .await
    })
    .map_err(|err| reason(&err))
}

/// What a service or polkit said, without the error's name.
fn reason(err: &zbus::Error) -> String {
    match err {
        zbus::Error::MethodError(name, message, _) => {
            message.clone().unwrap_or_else(|| name.as_str().to_owned())
        }
        zbus::Error::FDO(err) => {
            use zbus::DBusError;
            err.description()
                .map_or_else(|| err.to_string(), str::to_owned)
        }
        other => other.to_string(),
    }
}

/// A PNG file as premultiplied RGBA, its largest side brought down to
/// [`PICTURE`] pixels.
fn picture(file: &Path) -> Option<(u16, u16, Vec<u8>)> {
    let bytes = std::fs::read(file).ok()?;
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        png::ColorType::Indexed => return None,
    };
    let (w, h) = (info.width, info.height);
    if w == 0 || h == 0 {
        return None;
    }
    let side = w.max(h);
    let (tw, th) = if side > PICTURE {
        ((w * PICTURE / side).max(1), (h * PICTURE / side).max(1))
    } else {
        (w, h)
    };
    let mut out = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for x in 0..tw {
            let (sx, sy) = (x * w / tw, y * h / th);
            let at = ((sy * w + sx) * channels) as usize;
            let px = buf.get(at..at + channels as usize)?;
            let [r, g, b, a] = match px {
                [r, g, b, a] => [*r, *g, *b, *a],
                [r, g, b] => [*r, *g, *b, 255],
                [l, a] => [*l, *l, *l, *a],
                [l] => [*l, *l, *l, 255],
                _ => return None,
            };
            let pre = |c: u8| (u16::from(c) * u16::from(a) / 255) as u8;
            out.extend_from_slice(&[pre(r), pre(g), pre(b), a]);
        }
    }
    Some((u16::try_from(tw).ok()?, u16::try_from(th).ok()?, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_png_is_brought_down_and_premultiplied() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("face.png");
        let (w, h) = (256_u32, 128_u32);
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, w, h);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().map_err(std::io::Error::other)?;
            let pixels: Vec<u8> = (0..w * h).flat_map(|_| [200, 100, 0, 128]).collect();
            writer
                .write_image_data(&pixels)
                .map_err(std::io::Error::other)?;
        }
        std::fs::write(&file, data)?;
        let (pw, ph, px) = picture(&file).expect("it reads");
        assert_eq!((pw, ph), (128, 64));
        assert_eq!(&px[..4], &[100, 50, 0, 128]);
        assert_eq!(picture(&dir.path().join("none.png")), None);
        Ok(())
    }
}
