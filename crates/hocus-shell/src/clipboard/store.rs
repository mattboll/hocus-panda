//! The clipboard's history on disk (5.6), on a thread of its own.
//!
//! Each item is a file of `$XDG_STATE_HOME/hocuspanda/clipboard/`
//! (`<key>.clip`: its details and its text, or its image's thumbnail), an
//! image's PNG another (`<key>.png`), each sealed with XChaCha20-Poly1305
//! (a random nonce each time, the file's name authenticated with it so
//! that files cannot be swapped). Nothing in them reads as what was
//! copied. They are the user's alone (`0700`, `0600`), written whole then
//! renamed into place.
//!
//! The key is the session's keyring's (the Secret Service, [`keyring`])
//! when it answers; else a key file beside the history,
//! `clipboard.key` (`0600`), made once and kept. Once there, a key file is
//! used from then on. A keyring that holds the key but does not answer
//! leaves the history as it is: what is copied meanwhile is kept in
//! memory only, for this session.
//!
//! [`keyring`]: super::keyring

use std::{
    collections::HashMap,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, mpsc},
};

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Generate, Key, Payload},
};
use hocus_link::{CAPTURE_APP, Clip, ClipKind, ToCompositor};

use super::{Item, Kind, Picture, digest, keyring};

/// What the thread is asked.
#[derive(Debug)]
pub enum Ask {
    /// A client copied `clip` from `app` at `at` (milliseconds).
    Copied {
        app: String,
        clip: Clip,
        at: i64,
    },
    /// Capture `capture` of the compositor's session (5.8b), its PNG, at
    /// `at`.
    Captured {
        capture: u64,
        clip: Clip,
        at: i64,
    },
    /// An item's details changed (its time, its pin): its file again.
    Changed(Item),
    Remove(Vec<u64>),
    /// An image chosen: its PNG to the compositor.
    Choose {
        key: u64,
        paste: bool,
    },
}

static ASK: Mutex<Option<mpsc::Sender<Ask>>> = Mutex::new(None);

/// Asks the thread.
pub fn ask(ask: Ask) {
    let sender = ASK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    match sender {
        Some(sender) => {
            let _ = sender.send(ask);
        }
        None => tracing::debug!("the clipboard's history is not kept"),
    }
}

/// Starts the thread: the history in `dir` (none: kept in memory only),
/// its key in the keyring on `bus`, else beside it.
pub fn start(dir: Option<PathBuf>, bus: Option<String>) {
    let (sender, asked) = mpsc::channel();
    *ASK.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
    let started = std::thread::Builder::new()
        .name("clipboard's history".to_owned())
        .spawn(move || run(dir, bus.as_deref(), &asked));
    if let Err(err) = started {
        tracing::warn!(%err, "cannot keep the clipboard's history");
        *ASK.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }
}

fn run(dir: Option<PathBuf>, bus: Option<&str>, asked: &mpsc::Receiver<Ask>) {
    let mut store = Store {
        files: dir.and_then(|dir| Files::open(dir, bus)),
        images: HashMap::new(),
        captures: HashMap::new(),
    };
    if let Some(files) = &store.files {
        let items = files.load();
        tracing::info!(items = items.len(), "the clipboard's history is read back");
        let now = crate::notifications::wall_ms();
        let gone = super::with(|h| h.loaded(items, now));
        store.forget(&gone);
        super::changed();
    }
    while let Ok(ask) = asked.recv() {
        store.ask(ask);
    }
}

struct Store {
    files: Option<Files>,
    /// Images' PNGs when there are no files to keep them in.
    images: HashMap<u64, Arc<[u8]>>,
    /// The item each capture of this session made, by the capture's number
    /// (5.8b): the one its annotated image replaces (5.9).
    captures: HashMap<u64, u64>,
}

impl Store {
    fn ask(&mut self, ask: Ask) {
        match ask {
            Ask::Copied { app, clip, at } => self.copied(app, clip, at),
            Ask::Captured { capture, clip, at } => self.captured(capture, clip, at),
            Ask::Changed(item) => {
                if let Some(files) = &self.files {
                    files.write_item(&item);
                }
            }
            Ask::Remove(keys) => self.forget(&keys),
            Ask::Choose { key, paste } => {
                let png = self
                    .images
                    .get(&key)
                    .map(|png| png.to_vec())
                    .or_else(|| self.files.as_ref().and_then(|f| f.read_png(key)));
                match png {
                    Some(data) => {
                        crate::popover::tell(ToCompositor::Clip {
                            paste,
                            clip: Clip {
                                kind: ClipKind::Png,
                                data,
                            },
                        });
                    }
                    None => tracing::warn!(key, "a copied image is gone"),
                }
            }
        }
    }

    fn copied(&mut self, app: String, clip: Clip, at: i64) {
        let Some(item) = item(app, &clip, at) else {
            return;
        };
        tracing::debug!(
            kind = item.kind.word(),
            app = item.app,
            "a copy for the history"
        );
        let change = super::with(|h| h.add(item, at));
        self.keep(change, clip);
    }

    /// Capture `capture` (5.8b): an image copied from the capture, first;
    /// told again (its annotated image, 5.9), it replaces the item it made,
    /// its pin kept, unless that item went meanwhile.
    fn captured(&mut self, capture: u64, clip: Clip, at: i64) {
        let Some(item) = item(CAPTURE_APP.to_owned(), &clip, at) else {
            return;
        };
        tracing::debug!(capture, "a capture for the history");
        let made = self
            .captures
            .get(&capture)
            .copied()
            .filter(|key| super::with(|h| h.get(*key).is_some()));
        let change = match made {
            Some(old) => {
                tracing::debug!(capture, "a capture's item replaced");
                super::with(|h| h.replace(old, item, at))
            }
            None => super::with(|h| h.add(item, at)),
        };
        if let Some(key) = self.keep(change, clip) {
            self.captures.insert(capture, key);
        }
    }

    /// A change of the history kept on disk, `clip` the new item's: the
    /// item now holding the copy, if one does.
    fn keep(&mut self, change: super::Change, clip: Clip) -> Option<u64> {
        if let Some(new) = &change.new {
            match &self.files {
                Some(files) => {
                    if new.kind == Kind::Image {
                        files.write_png(new.key, &clip.data);
                    }
                    files.write_item(new);
                }
                None if new.kind == Kind::Image => {
                    self.images.insert(new.key, clip.data.into());
                }
                None => {}
            }
        }
        if let (Some(changed), Some(files)) = (&change.changed, &self.files) {
            files.write_item(changed);
        }
        self.forget(&change.gone);
        super::changed();
        change.new.or(change.changed).map(|item| item.key)
    }

    /// What went: its files.
    fn forget(&mut self, keys: &[u64]) {
        for key in keys {
            self.images.remove(key);
            if let Some(files) = &self.files {
                files.remove(*key);
            }
        }
    }
}

/// The item `clip` makes, copied from `app` at `at`, under a new key;
/// none for an image that does not read.
fn item(app: String, clip: &Clip, at: i64) -> Option<Item> {
    let key = new_key()?;
    let item = match clip.kind {
        ClipKind::Text => {
            let text = String::from_utf8_lossy(&clip.data).into_owned();
            Item {
                key,
                kind: Kind::of(&text),
                digest: digest(text.as_bytes()),
                text: text.into(),
                app,
                at,
                pinned: false,
                picture: None,
            }
        }
        ClipKind::Png => Item {
            key,
            kind: Kind::Image,
            digest: digest(&clip.data),
            text: "".into(),
            app,
            at,
            pinned: false,
            picture: Some(Picture::of_png(&clip.data)?),
        },
    };
    Some(item)
}

/// A new item's number, at random.
fn new_key() -> Option<u64> {
    let nonce = XNonce::try_generate()
        .inspect_err(|err| tracing::warn!(%err, "no randomness: a copy not kept"))
        .ok()?;
    let mut bytes = [0; 8];
    bytes.copy_from_slice(&nonce[..8]);
    Some(u64::from_le_bytes(bytes))
}

// --- Sealed files ---------------------------------------------------------------

/// What starts a sealed file: its format.
const MAGIC: &[u8; 8] = b"HPCLIP\x00\x01";
const NONCE: usize = 24;

/// `plain` sealed as the file `name`.
fn seal(cipher: &XChaCha20Poly1305, name: &str, plain: &[u8]) -> Option<Vec<u8>> {
    let nonce = XNonce::try_generate().ok()?;
    let sealed = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: plain,
                aad: name.as_bytes(),
            },
        )
        .ok()?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Some(out)
}

/// The file `name`'s `sealed` content opened, if it was sealed with this
/// key under this name.
fn unseal(cipher: &XChaCha20Poly1305, name: &str, sealed: &[u8]) -> Option<Vec<u8>> {
    let rest = sealed.strip_prefix(MAGIC.as_slice())?;
    if rest.len() < NONCE {
        return None;
    }
    let (nonce, sealed) = rest.split_at(NONCE);
    let nonce = XNonce::try_from(nonce).ok()?;
    cipher
        .decrypt(
            &nonce,
            Payload {
                msg: sealed,
                aad: name.as_bytes(),
            },
        )
        .ok()
}

/// An item's file's content: its details, a blank line, then its text or
/// its thumbnail's pixels.
fn item_bytes(item: &Item) -> Vec<u8> {
    let app: String = item
        .app
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut head = String::from("hocuspanda clipboard 1\n");
    match &item.picture {
        Some(p) => head.push_str(&format!("kind png {} {} {}\n", p.width, p.height, p.side)),
        None => head.push_str("kind text\n"),
    }
    head.push_str(&format!(
        "app {app}\nat {}\npinned {}\ndigest {:016x}\n\n",
        item.at,
        u8::from(item.pinned),
        item.digest
    ));
    let mut bytes = head.into_bytes();
    match &item.picture {
        Some(p) => bytes.extend_from_slice(&p.rgba),
        None => bytes.extend_from_slice(item.text.as_bytes()),
    }
    bytes
}

/// An item read back from its file's content.
fn item_of(key: u64, bytes: &[u8]) -> Option<Item> {
    let end = bytes.windows(2).position(|w| w == b"\n\n")?;
    let head = std::str::from_utf8(&bytes[..end]).ok()?;
    let body = &bytes[end + 2..];
    let mut lines = head.lines();
    if lines.next()? != "hocuspanda clipboard 1" {
        return None;
    }
    let mut picture = None;
    let (mut app, mut at, mut pinned, mut hash) = (String::new(), None, false, None);
    let mut text = false;
    for line in lines {
        let (field, value) = line.split_once(' ').unwrap_or((line, ""));
        match field {
            "kind" if value == "text" => text = true,
            "kind" => {
                let mut words = value.split(' ');
                if words.next()? != "png" {
                    return None;
                }
                let mut number = || words.next()?.parse::<u32>().ok();
                let (width, height, side) = (number()?, number()?, number()?);
                if body.len() != (side as usize).pow(2) * 4 {
                    return None;
                }
                picture = Some(Picture::of_thumb(width, height, side, body.into()));
            }
            "app" => app = value.to_owned(),
            "at" => at = value.parse().ok(),
            "pinned" => pinned = value == "1",
            "digest" => hash = u64::from_str_radix(value, 16).ok(),
            _ => {}
        }
    }
    let (kind, text) = match (&picture, text) {
        (Some(_), false) => (Kind::Image, Arc::from("")),
        (None, true) => {
            let text = std::str::from_utf8(body).ok()?;
            (Kind::of(text), Arc::from(text))
        }
        _ => return None,
    };
    Some(Item {
        key,
        kind,
        text,
        app,
        at: at?,
        pinned,
        picture,
        digest: hash?,
    })
}

/// The history's directory, and the key its files are sealed with.
struct Files {
    dir: PathBuf,
    cipher: XChaCha20Poly1305,
}

impl Files {
    /// The history in `dir`, its key found or made; `None` without one.
    fn open(dir: PathBuf, bus: Option<&str>) -> Option<Self> {
        let key = key(&dir, bus)?;
        if let Err(err) = std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&dir)
        {
            tracing::warn!(%err, dir = %dir.display(), "cannot keep the clipboard's history");
            return None;
        }
        Some(Self {
            dir,
            cipher: XChaCha20Poly1305::new(&key),
        })
    }

    fn write(&self, name: &str, plain: &[u8]) {
        let Some(sealed) = seal(&self.cipher, name, plain) else {
            tracing::warn!(name, "could not seal an item of the clipboard's history");
            return;
        };
        if let Err(err) = write_private(&self.dir.join(name), &sealed) {
            tracing::warn!(%err, name, "could not keep an item of the clipboard's history");
        }
    }

    fn write_item(&self, item: &Item) {
        self.write(&format!("{:016x}.clip", item.key), &item_bytes(item));
    }

    fn write_png(&self, key: u64, png: &[u8]) {
        self.write(&format!("{key:016x}.png"), png);
    }

    fn read_png(&self, key: u64) -> Option<Vec<u8>> {
        let name = format!("{key:016x}.png");
        let sealed = std::fs::read(self.dir.join(&name)).ok()?;
        unseal(&self.cipher, &name, &sealed)
    }

    fn remove(&self, key: u64) {
        for name in [format!("{key:016x}.clip"), format!("{key:016x}.png")] {
            match std::fs::remove_file(self.dir.join(&name)) {
                Ok(()) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!(%err, name, "could not remove a history's item"),
            }
        }
    }

    /// Every item that opens with this key.
    fn load(&self) -> Vec<Item> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut items = Vec::new();
        let mut closed = 0;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(key) = name
                .strip_suffix(".clip")
                .and_then(|k| u64::from_str_radix(k, 16).ok())
            else {
                continue;
            };
            let item = std::fs::read(entry.path())
                .ok()
                .and_then(|sealed| unseal(&self.cipher, &name, &sealed))
                .and_then(|plain| item_of(key, &plain));
            match item {
                Some(item) => items.push(item),
                None => closed += 1,
            }
        }
        if closed > 0 {
            tracing::warn!(
                closed,
                "items of the clipboard's history do not open with its key"
            );
        }
        items
    }
}

/// Writes `bytes` to `file`, the user's alone, whole or not at all.
fn write_private(file: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = file.file_name().unwrap_or_default().to_owned();
    name.push(".new");
    let new = file.with_file_name(name);
    let mut out = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&new)?;
    out.write_all(bytes)?;
    out.sync_all()?;
    std::fs::rename(&new, file)
}

// --- The key --------------------------------------------------------------------

type SealKey = Key<XChaCha20Poly1305>;

/// The key file beside the history `dir`.
fn key_file(dir: &Path) -> PathBuf {
    dir.with_extension("key")
}

/// The history's key: its key file's, else the keyring's, else a new key
/// file's. `None` when there is none to be had (nothing is kept on disk).
fn key(dir: &Path, bus: Option<&str>) -> Option<SealKey> {
    let file = key_file(dir);
    match std::fs::read_to_string(&file) {
        Ok(text) => {
            let key = from_hex(text.trim());
            if key.is_none() {
                tracing::warn!(file = %file.display(), "the clipboard's key file does not read: nothing kept");
            }
            return key;
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(%err, "cannot read the clipboard's key file: nothing kept");
            return None;
        }
    }
    if let Some(bus) = bus {
        match keyring::find(bus) {
            keyring::Found::Key(bytes) => {
                tracing::info!("the clipboard's key is the keyring's");
                return SealKey::try_from(bytes.as_slice()).ok();
            }
            keyring::Found::Absent => {
                let key = SealKey::try_generate().ok()?;
                if keyring::keep(bus, key.as_slice()) {
                    tracing::info!("the clipboard's key is kept in the keyring");
                    return Some(key);
                }
            }
            keyring::Found::NoAnswer => {}
        }
    }
    // The history's files were sealed with the keyring's key, which did
    // not answer: they stay as they are.
    let sealed = std::fs::read_dir(dir).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".clip"))
    });
    if sealed {
        tracing::warn!("the keyring holding the clipboard's key does not answer: nothing kept");
        return None;
    }
    let key = SealKey::try_generate().ok()?;
    let made = (|| {
        if let Some(parent) = file.parent() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)?;
        }
        let mut out = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&file)?;
        out.write_all(format!("{}\n", hex(key.as_slice())).as_bytes())?;
        out.sync_all()
    })();
    match made {
        Ok(()) => {
            tracing::info!(file = %file.display(), "the clipboard's key is in a file");
            Some(key)
        }
        Err(err) => {
            tracing::warn!(%err, "cannot make the clipboard's key file: nothing kept");
            None
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> Option<SealKey> {
    if text.len() != 64 {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..32)
        .map(|i| u8::from_str_radix(text.get(2 * i..2 * i + 2)?, 16).ok())
        .collect();
    SealKey::try_from(bytes?.as_slice()).ok()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_sealed_file_reads_back_only_with_its_key_and_its_name() {
        let key = SealKey::try_generate().expect("a key");
        let cipher = XChaCha20Poly1305::new(&key);
        let sealed = seal(&cipher, "00ff.clip", b"git push --force").expect("sealed");
        assert!(!sealed.windows(4).any(|w| w == b"push"));
        assert_eq!(
            unseal(&cipher, "00ff.clip", &sealed).as_deref(),
            Some(b"git push --force".as_slice())
        );
        assert!(unseal(&cipher, "0100.clip", &sealed).is_none());
        let other = XChaCha20Poly1305::new(&SealKey::try_generate().expect("a key"));
        assert!(unseal(&other, "00ff.clip", &sealed).is_none());
    }

    #[test]
    fn an_item_reads_back_as_written() {
        let mut item = super::super::text_item(42, "ls -la\n~/notes", "kitty", 1_793_000_000_000);
        item.pinned = true;
        let back = item_of(42, &item_bytes(&item)).expect("read");
        assert_eq!(back.text, item.text);
        assert_eq!(
            (back.app.as_str(), back.at, back.pinned),
            ("kitty", item.at, true)
        );
        assert_eq!(back.digest, item.digest);
        let side = 2;
        let picture = Picture::of_thumb(1280, 800, side, vec![7; 16].into());
        let image = Item {
            kind: Kind::Image,
            text: "".into(),
            picture: Some(picture),
            ..item
        };
        let back = item_of(42, &item_bytes(&image)).expect("read");
        let picture = back.picture.expect("a picture");
        assert_eq!(
            (picture.width, picture.height, picture.side),
            (1280, 800, 2)
        );
        assert_eq!(back.kind, Kind::Image);
        assert!(item_of(42, b"something else\n\n").is_none());
    }

    #[test]
    fn without_a_keyring_the_key_is_a_private_file_made_once() {
        let root = tempfile::tempdir().expect("a directory");
        let dir = root.path().join("hocuspanda").join("clipboard");
        let key = key(&dir, None).expect("a key");
        let file = key_file(&dir);
        let mode = std::fs::metadata(&file).expect("made").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        // Kept: the same key next time.
        assert_eq!(super::key(&dir, None).expect("a key"), key);
        // Its history's files sealed with it, private too.
        let files = Files::open(dir.clone(), None).expect("open");
        let item = super::super::text_item(5, "hunter-two-not-a-secret", "", 1);
        files.write_item(&item);
        let written = dir.join(format!("{:016x}.clip", 5));
        let mode = std::fs::metadata(&written)
            .expect("written")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
        let raw = std::fs::read(&written).expect("read");
        assert!(!raw.windows(6).any(|w| w == b"hunter"));
        let back = files.load();
        assert_eq!(back.len(), 1);
        assert_eq!(&*back[0].text, "hunter-two-not-a-secret");
        files.remove(5);
        assert!(files.load().is_empty());
    }

    #[test]
    fn a_history_sealed_by_a_keyring_that_does_not_answer_is_left_alone() {
        let root = tempfile::tempdir().expect("a directory");
        let dir = root.path().join("clipboard");
        std::fs::create_dir_all(&dir).expect("made");
        std::fs::write(dir.join("0000000000000001.clip"), b"sealed elsewhere").expect("written");
        // No bus answers at this address.
        let bus = format!("unix:path={}", root.path().join("no-bus").display());
        assert!(key(&dir, Some(&bus)).is_none());
        assert!(!key_file(&dir).exists());
    }
}
