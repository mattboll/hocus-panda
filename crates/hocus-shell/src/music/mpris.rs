//! The players, read over MPRIS on the session bus the compositor names
//! ([`hocus_link::NOTIFICATIONS_BUS_ENV`]), on a thread of its own: never
//! the live session's from a nested or headless desktop.
//!
//! A player is a name under `org.mpris.MediaPlayer2.`; its properties
//! (`org.mpris.MediaPlayer2` and `….Player` at `/org/mpris/MediaPlayer2`)
//! are read as it comes, and read again on its `PropertiesChanged` and
//! `Seeked`; it goes with its name's owner. A cover on a `file://` URL is
//! decoded here, its corners rounded for the bar's sizes; an `http` one
//! shows the placeholder (nothing fetched). What the bar asks is called
//! here too, so that a player that does not answer holds nothing up.

use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

use hocus_link::NOTIFICATIONS_BUS_ENV;
use iced::{advanced::image, futures::StreamExt};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use zbus::{
    Connection, MatchRule, MessageStream, ObjectPath, OwnedVariant, Variant, connection::Builder,
    fdo::DBusProxy,
};

use super::{Ask, Cover, Player, Status};

const PREFIX: &str = "org.mpris.MediaPlayer2.";
const PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT: &str = "org.mpris.MediaPlayer2";
const PLAYER: &str = "org.mpris.MediaPlayer2.Player";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";
/// A player that does not answer in this long is left as it was.
const PATIENCE: Duration = Duration::from_secs(2);

type Props = HashMap<String, OwnedVariant>;

enum Input {
    /// A name's owner changed: its new one, none when it went.
    Owner {
        name: String,
        owner: Option<String>,
    },
    /// A player's signal, by its unique name.
    Changed(String),
    Ask(Ask),
}

/// Follows the players on the bus the compositor names, if it does.
pub fn start(asks: UnboundedReceiver<Ask>) {
    let Some(bus) = std::env::var(NOTIFICATIONS_BUS_ENV)
        .ok()
        .filter(|b| !b.is_empty())
    else {
        tracing::info!("no session bus given: no music player");
        return;
    };
    let started = std::thread::Builder::new()
        .name("music".to_owned())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run(bus, asks)),
                Err(err) => tracing::warn!(%err, "no runtime for the music player"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot follow the music players");
    }
}

struct Players {
    connection: Connection,
    /// By their well-known names, in the order they came.
    players: Vec<Player>,
    /// Unique name → well-known name.
    owners: HashMap<String, String>,
    /// Counts the starts of playing: the last to start is shown.
    starts: u64,
}

async fn run(bus: String, mut asks: UnboundedReceiver<Ask>) {
    let builder = if bus == "session" {
        Builder::session()
    } else {
        Builder::address(bus.as_str())
    };
    let connection = match builder.build().await {
        Ok(connection) => connection,
        Err(err) => {
            tracing::warn!(%err, "cannot reach the session bus: no music player");
            return;
        }
    };
    let (inputs, mut incoming) = unbounded_channel();
    if let Err(err) = listen(&connection, inputs.clone()).await {
        tracing::warn!(%err, "cannot follow the music players");
        return;
    }
    tokio::spawn({
        let inputs = inputs.clone();
        async move {
            while let Some(ask) = asks.recv().await {
                if inputs.send(Input::Ask(ask)).is_err() {
                    return;
                }
            }
        }
    });
    let mut players = Players {
        connection,
        players: Vec::new(),
        owners: HashMap::new(),
        starts: 0,
    };
    let names = match DBusProxy::new(&players.connection).await {
        Ok(bus) => bus.list_names().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    };
    for name in names {
        let name = name.to_string();
        if name.starts_with(PREFIX) {
            players.came(&name).await;
        }
    }
    tracing::info!(
        players = players.players.len(),
        "the music players are followed"
    );
    players.publish();
    while let Some(input) = incoming.recv().await {
        players.input(input).await;
        while let Ok(input) = incoming.try_recv() {
            players.input(input).await;
        }
        players.publish();
    }
}

/// The players' signals, and names changing owners, into `inputs`.
async fn listen(connection: &Connection, inputs: UnboundedSender<Input>) -> zbus::Result<()> {
    for (interface, member) in [(PROPERTIES, "PropertiesChanged"), (PLAYER, "Seeked")] {
        let rule = MatchRule::builder()
            .msg_type(zbus::message::Type::Signal)
            .interface(interface)
            .member(member)
            .path(PATH)
            .build()?;
        let mut signals = MessageStream::for_match_rule(rule, connection, None).await?;
        let to = inputs.clone();
        tokio::spawn(async move {
            while let Some(Ok(message)) = signals.next().await {
                let header = message.header();
                let Some(sender) = header.sender() else {
                    continue;
                };
                if to.send(Input::Changed(sender.to_string())).is_err() {
                    return;
                }
            }
        });
    }
    let bus = DBusProxy::new(connection).await?;
    let mut owners = bus.receive_name_owner_changed().await?;
    tokio::spawn(async move {
        while let Some(changed) = owners.next().await {
            let Ok(args) = changed.args() else {
                continue;
            };
            let name = args.name().to_string();
            if !name.starts_with(PREFIX) {
                continue;
            }
            let owner = args.new_owner().as_ref().map(ToString::to_string);
            if inputs.send(Input::Owner { name, owner }).is_err() {
                return;
            }
        }
    });
    Ok(())
}

impl Players {
    async fn input(&mut self, input: Input) {
        match input {
            Input::Owner { name, owner: None } => {
                self.players.retain(|p| p.bus != name);
                self.owners.retain(|_, n| *n != name);
                tracing::debug!(name, "a music player went");
            }
            Input::Owner {
                name,
                owner: Some(_),
            } => self.came(&name).await,
            Input::Changed(sender) => {
                if let Some(name) = self.owners.get(&sender).cloned() {
                    self.read(&name).await;
                }
            }
            Input::Ask(ask) => self.ask(ask).await,
        }
    }

    /// Player `name` came: its owner and properties.
    async fn came(&mut self, name: &str) {
        if let Ok(bus) = DBusProxy::new(&self.connection).await
            && let Ok(bus_name) = zbus::names::BusName::try_from(name)
            && let Ok(Ok(owner)) =
                tokio::time::timeout(PATIENCE, bus.get_name_owner(bus_name)).await
        {
            self.owners.insert(owner.to_string(), name.to_owned());
        }
        self.read(name).await;
    }

    async fn get_all(&self, name: &str, interface: &str) -> Option<Props> {
        let body = (interface,);
        let call = self
            .connection
            .call_method(Some(name), PATH, Some(PROPERTIES), "GetAll", &body);
        match tokio::time::timeout(PATIENCE, call).await {
            Ok(Ok(reply)) => reply.body().deserialize::<Props>().ok(),
            Ok(Err(err)) => {
                tracing::debug!(%err, name, interface, "a music player not read");
                None
            }
            Err(_) => {
                tracing::debug!(name, interface, "a music player that does not answer");
                None
            }
        }
    }

    /// Player `name`'s properties, read again.
    async fn read(&mut self, name: &str) {
        let Some(props) = self.get_all(name, PLAYER).await else {
            return;
        };
        let root = self.get_all(name, ROOT).await.unwrap_or_default();
        let now = Instant::now();
        let at = self.players.iter().position(|p| p.bus == name);
        let before = at.map(|i| self.players[i].clone());
        let mut player = player_of(name, &root, &props, now);
        // The cover decoded once a URL.
        player.cover = match &before {
            Some(b) if b.art_url == player.art_url => b.cover.clone(),
            _ => cover(&player.art_url),
        };
        let started = player.status == Status::Playing
            && before.as_ref().is_none_or(|b| b.status != Status::Playing);
        player.started = before.as_ref().map_or(0, |b| b.started);
        if started {
            self.starts += 1;
            player.started = self.starts;
        }
        match at {
            Some(i) => self.players[i] = player,
            None => {
                tracing::debug!(name, "a music player came");
                self.players.push(player);
            }
        }
    }

    async fn ask(&mut self, ask: Ask) {
        let (name, member, body): (String, &str, Option<(ObjectPath<'static>, i64)>) = match ask {
            Ask::PlayPause(name) => (name, "PlayPause", None),
            Ask::Next(name) => (name, "Next", None),
            Ask::Previous(name) => (name, "Previous", None),
            Ask::Raise(name) => {
                let call = self.connection.call_method(
                    Some(name.as_str()),
                    PATH,
                    Some(ROOT),
                    "Raise",
                    &(),
                );
                if let Ok(Err(err)) = tokio::time::timeout(PATIENCE, call).await {
                    tracing::debug!(%err, name, "a music player not raised");
                }
                return;
            }
            Ask::Seek { name, to } => {
                let Some(track) = self
                    .players
                    .iter()
                    .find(|p| p.bus == name)
                    .and_then(|p| p.track.clone())
                    .and_then(|t| ObjectPath::try_from(t).ok())
                else {
                    return;
                };
                let micros = i64::try_from(to.as_micros()).unwrap_or(i64::MAX);
                (name, "SetPosition", Some((track, micros)))
            }
        };
        tracing::debug!(name, member, "the music player is asked");
        let reply = match &body {
            Some(body) => {
                let call = self.connection.call_method(
                    Some(name.as_str()),
                    PATH,
                    Some(PLAYER),
                    member,
                    body,
                );
                tokio::time::timeout(PATIENCE, call).await
            }
            None => {
                let call = self.connection.call_method(
                    Some(name.as_str()),
                    PATH,
                    Some(PLAYER),
                    member,
                    &(),
                );
                tokio::time::timeout(PATIENCE, call).await
            }
        };
        if let Ok(Err(err)) = reply {
            tracing::debug!(%err, name, member, "a music player refused");
        }
        // A seek is not always said back: its position read again.
        self.read(&name).await;
    }

    fn publish(&self) {
        super::publish(self.players.clone());
    }
}

/// What a player's properties say.
fn player_of(name: &str, root: &Props, props: &Props, now: Instant) -> Player {
    let text = |props: &Props, key: &str| match props.get(key).map(|v| inner(v)) {
        Some(Variant::Str(s)) => s.as_str().to_owned(),
        _ => String::new(),
    };
    let flag = |key: &str| matches!(props.get(key).map(|v| inner(v)), Some(Variant::Bool(true)));
    let meta: HashMap<String, &Variant<'_>> = match props.get("Metadata").map(|v| inner(v)) {
        Some(Variant::Dict(dict)) => dict
            .iter()
            .filter_map(|(k, v)| match k {
                Variant::Str(k) => Some((k.as_str().to_owned(), inner(v))),
                _ => None,
            })
            .collect(),
        _ => HashMap::new(),
    };
    let meta_text = |key: &str| match meta.get(key) {
        Some(Variant::Str(s)) => s.as_str().to_owned(),
        Some(Variant::ObjectPath(p)) => p.as_str().to_owned(),
        Some(Variant::Array(list)) => list
            .inner()
            .iter()
            .filter_map(|v| match inner(v) {
                Variant::Str(s) => Some(s.as_str().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(", "),
        _ => String::new(),
    };
    let micros = |v: Option<&Variant<'_>>| match v {
        Some(Variant::I64(n)) => u64::try_from(*n).ok(),
        Some(Variant::U64(n)) => Some(*n),
        Some(Variant::I32(n)) => u64::try_from(*n).ok(),
        _ => None,
    };
    let length = micros(meta.get("mpris:length").copied())
        .filter(|n| *n > 0)
        .map(Duration::from_micros);
    let position = micros(props.get("Position").map(|v| inner(v)))
        .map(Duration::from_micros)
        .unwrap_or_default();
    let status = match text(props, "PlaybackStatus").as_str() {
        "Playing" => Status::Playing,
        "Paused" => Status::Paused,
        _ => Status::Stopped,
    };
    let mut album = meta_text("xesam:album");
    let year: String = meta_text("xesam:contentCreated").chars().take(4).collect();
    if year.len() == 4 && year.chars().all(|c| c.is_ascii_digit()) {
        album = if album.is_empty() {
            year
        } else {
            format!("{album} · {year}")
        };
    }
    let identity = text(root, "Identity");
    Player {
        bus: name.to_owned(),
        app: if identity.is_empty() {
            name.trim_start_matches(PREFIX)
                .split('.')
                .next()
                .unwrap_or_default()
                .to_owned()
        } else {
            identity
        },
        title: meta_text("xesam:title"),
        artist: meta_text("xesam:artist"),
        album,
        art_url: meta_text("mpris:artUrl"),
        cover: None,
        track: Some(meta_text("mpris:trackid")).filter(|t| !t.is_empty()),
        length,
        position,
        read_at: now,
        status,
        can_next: flag("CanGoNext"),
        can_previous: flag("CanGoPrevious"),
        can_seek: flag("CanSeek"),
        can_control: !matches!(
            props.get("CanControl").map(|v| inner(v)),
            Some(Variant::Bool(false))
        ),
        can_raise: matches!(
            root.get("CanRaise").map(|v| inner(v)),
            Some(Variant::Bool(true))
        ),
        started: 0,
    }
}

fn inner<'a, 'b>(value: &'b Variant<'a>) -> &'b Variant<'a> {
    match value {
        Variant::Variant(inner) => inner.as_ref(),
        other => other,
    }
}

/// The cover at `url`, decoded and its corners rounded for the popover
/// (72 px, radius 10) and the bar (34 px, radius 6), at twice their size:
/// a `file://` one only.
fn cover(url: &str) -> Option<Cover> {
    let path = url.strip_prefix("file://")?;
    let path = percent_decoded(path);
    let decoded = match image_reader(&path) {
        Ok(decoded) => decoded,
        Err(err) => {
            tracing::debug!(%err, path, "a cover not read");
            return None;
        }
    };
    let rounded = |side: u32, radius: f32| {
        let mut square = image_crate::imageops::resize(
            &decoded,
            side,
            side,
            image_crate::imageops::FilterType::Triangle,
        );
        round_corners(&mut square, radius);
        image::Handle::from_rgba(side, side, square.into_raw())
    };
    Some(Cover {
        big: rounded(144, 20.0),
        small: rounded(68, 12.0),
    })
}

use ::image as image_crate;

fn image_reader(path: &str) -> Result<image_crate::RgbaImage, image_crate::ImageError> {
    Ok(image_crate::ImageReader::open(path)?
        .with_guessed_format()?
        .decode()?
        .to_rgba8())
}

/// `%20` and the like, as a file URL writes them.
fn percent_decoded(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = path.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The image's corners cut round, `radius` pixels, antialiased by a pixel.
fn round_corners(image: &mut image_crate::RgbaImage, radius: f32) {
    let (w, h) = (image.width() as f32, image.height() as f32);
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let cx = px.clamp(radius, w - radius);
        let cy = py.clamp(radius, h - radius);
        let distance = ((px - cx).powi(2) + (py - cy).powi(2)).sqrt();
        let cover = (radius + 0.5 - distance).clamp(0.0, 1.0);
        if cover < 1.0 {
            pixel.0[3] = (f32::from(pixel.0[3]) * cover).round() as u8;
        }
    }
}
