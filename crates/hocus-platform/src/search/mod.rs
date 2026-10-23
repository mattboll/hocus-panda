//! Document search (#45): an index of the words of the documents in some
//! folders (text, Markdown, HTML, PDF, office files, and the text of
//! screenshots), and of their meaning when a model can embed, searched
//! from the command field.
//!
//! Two threads of their own, never the compositor's loop: the indexer, at
//! the lowest priority (nice 19, which the I/O scheduler follows too),
//! walks the folders in small batches with pauses between them, reads
//! what changed since the index was kept (by date and size), and waits
//! while the laptop is on battery; the searcher answers queries from the
//! index, the newest only. Results and the indexer's state come back
//! through a callback.
//!
//! What is never read ([`deny`]) is checked by name before anything else
//! is asked of an entry; symbolic links are never followed; every open
//! goes through one gate that checks again.

pub mod deny;
mod extract;
mod index;
mod pdf;
mod zip;

use std::{
    collections::{HashSet, VecDeque},
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        mpsc::{self, RecvTimeoutError, TryRecvError},
    },
    time::{Duration, Instant, UNIX_EPOCH},
};

pub use extract::Format;
pub use index::{Piece, fold};

use crate::{
    ai::{self, Embedder},
    secrets::Secrets,
};
use index::Index;

/// Between two batches of the indexer.
const PACE: Duration = Duration::from_millis(40);
/// Entries (a folder's listing, or a file) per batch.
const BATCH: usize = 8;
/// Texts per request for vectors.
const EMBED_BATCH: usize = 8;
/// The start of a text a vector stands for, and how many per document.
const CHUNK: usize = 1500;
const CHUNKS: usize = 3;
/// How often the folders are looked at again.
const RESCAN: Duration = Duration::from_secs(30 * 60);
/// How deep folders go.
const MAX_DEPTH: usize = 24;
/// Documents kept, at most.
const MAX_DOCUMENTS: usize = 50_000;
/// Documents a query finds, at most.
const RESULTS: usize = 4;
/// A query is embedded once typing stopped this long.
const SETTLE: Duration = Duration::from_millis(250);

/// A folder searched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    pub folder: Folder,
    /// The screenshots folder: its pictures only, read by OCR.
    pub pictures: bool,
}

/// Where a folder is: named, or one of the user's, found by the indexer
/// (never on the caller's loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folder {
    Path(PathBuf),
    /// The user's folder `XDG_<key>_DIR` from `user-dirs.dirs` (else
    /// `~/<fallback>`), and `sub` in it.
    User {
        key: &'static str,
        fallback: &'static str,
        sub: &'static str,
    },
}

impl Folder {
    /// Where it is; `None` for a user's folder set to the home itself
    /// (turned off, as the XDG rules say).
    pub fn resolve(&self, home: Option<&Path>) -> Option<PathBuf> {
        match self {
            Self::Path(path) => Some(path.clone()),
            Self::User { key, fallback, sub } => {
                let home = home?;
                let config = std::env::var_os("XDG_CONFIG_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".config"));
                let named = fs::read_to_string(config.join("user-dirs.dirs"))
                    .ok()
                    .and_then(|dirs| {
                        dirs.lines().find_map(|l| {
                            let value = l.strip_prefix(&format!("XDG_{key}_DIR="))?;
                            let value = value.trim().trim_matches('"');
                            Some(PathBuf::from(
                                value.replace("$HOME", &home.to_string_lossy()),
                            ))
                        })
                    });
                let dir = named.unwrap_or_else(|| home.join(fallback));
                if dir == home {
                    return None;
                }
                Some(if sub.is_empty() { dir } else { dir.join(sub) })
            }
        }
    }
}

/// What the indexer is told by the configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub roots: Vec<Root>,
    /// Names left out besides those never read, as globs.
    pub exclude: Vec<String>,
    /// The program reading text in pictures, as tesseract.
    pub ocr: String,
    /// Who makes vectors; `None`: words only. Its key, if it needs one,
    /// is read from the keyring by the indexer.
    pub embedder: Option<Embedder>,
    /// The index's file; `None`: kept in memory only.
    pub store: Option<PathBuf>,
    /// Before the first look at the folders.
    pub delay: Duration,
    /// The home, for the places of it never read.
    pub home: Option<PathBuf>,
}

/// What the indexer does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Turned off.
    #[default]
    Off,
    /// Waits to start, or for the next look at the folders.
    Waiting,
    /// On battery: waits for the charger.
    Paused,
    Indexing,
    /// Up to date.
    Ready,
}

impl Phase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Waiting => "waiting",
            Self::Paused => "paused",
            Self::Indexing => "indexing",
            Self::Ready => "ready",
        }
    }
}

/// The indexer's state, for the control API.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Status {
    pub phase: Phase,
    pub documents: usize,
    /// Documents with vectors.
    pub embedded: usize,
    /// Entries never read, in the last look.
    pub denied: usize,
    /// Entries that could not be read, in the last look.
    pub unreadable: usize,
    /// Search by meaning works.
    pub meaning: bool,
}

/// A document found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub path: PathBuf,
    pub snippet: Vec<Piece>,
}

/// What comes back.
#[derive(Debug, Clone, PartialEq)]
pub enum News {
    Status(Status),
    /// The documents for query `id`: by words first, then again with
    /// their meaning when a model makes vectors.
    Found {
        id: u64,
        hits: Vec<Hit>,
    },
}

/// Sees every path the indexer opens (folders, files, pictures given to
/// OCR) before it does: for tests.
pub type Probe = Arc<dyn Fn(&Path) + Send + Sync>;

enum Command {
    Configure(Box<Settings>),
    /// Off: the index kept there goes.
    Off(Option<PathBuf>),
    Battery(bool),
}

/// The search service; dropping it stops its threads.
#[derive(Debug)]
pub struct Search {
    commands: mpsc::Sender<Command>,
    queries: mpsc::Sender<(u64, String)>,
}

impl Search {
    /// Starts the threads, turned off until configured.
    pub fn start(
        secrets: Option<Secrets>,
        news: impl Fn(News) + Send + Sync + 'static,
        probe: Option<Probe>,
    ) -> io::Result<Self> {
        let news: Arc<dyn Fn(News) + Send + Sync> = Arc::new(news);
        let index = Arc::new(RwLock::new(Index::default()));
        let embedder = Arc::new(Mutex::new(None::<Embedder>));
        let (commands, received) = mpsc::channel();
        let (queries, asked) = mpsc::channel();
        let indexer = Indexer {
            settings: None,
            battery: false,
            index: index.clone(),
            embedder: embedder.clone(),
            secrets,
            gate: Gate { home: None, probe },
            news: news.clone(),
            work: Work::Idle,
            status: Status::default(),
            sent: None,
            dirty: false,
            saved: Instant::now(),
            embed_failed: false,
        };
        std::thread::Builder::new()
            .name("search index".into())
            .spawn(move || {
                lowest_priority();
                indexer.run(&received);
            })?;
        std::thread::Builder::new()
            .name("search".into())
            .spawn(move || searcher(&asked, &index, &embedder, news.as_ref()))?;
        Ok(Self { commands, queries })
    }

    /// Turns indexing on with `settings`.
    pub fn configure(&self, settings: Settings) {
        let _ = self.commands.send(Command::Configure(Box::new(settings)));
    }

    /// Turns indexing off: the index kept in `store`, if any, is deleted.
    pub fn turn_off(&self, store: Option<PathBuf>) {
        let _ = self.commands.send(Command::Off(store));
    }

    /// On battery the indexer waits; on the charger it goes on.
    pub fn set_battery(&self, battery: bool) {
        let _ = self.commands.send(Command::Battery(battery));
    }

    /// Looks for `text`; the answer comes as [`News::Found`] with `id`.
    pub fn query(&self, id: u64, text: String) {
        let _ = self.queries.send((id, text));
    }
}

/// This thread at nice 19: its reads go last too.
fn lowest_priority() {
    #[cfg(target_os = "linux")]
    {
        let tid = rustix::thread::gettid();
        if let Err(err) = rustix::process::setpriority_process(Some(tid), 19) {
            tracing::debug!(%err, "the indexer keeps its priority");
        }
    }
}

/// The searcher's thread: the newest query answered by words at once,
/// then, once typing stops, by meaning too.
fn searcher(
    asked: &mpsc::Receiver<(u64, String)>,
    index: &RwLock<Index>,
    embedder: &Mutex<Option<Embedder>>,
    news: &(dyn Fn(News) + Send + Sync),
) {
    let mut next = None;
    loop {
        let (mut id, mut text) = match next.take() {
            Some(q) => q,
            None => match asked.recv() {
                Ok(q) => q,
                Err(_) => return,
            },
        };
        while let Ok(newer) = asked.try_recv() {
            (id, text) = newer;
        }
        let read = |vector: Option<&[f32]>| {
            let index = index.read().unwrap_or_else(|e| e.into_inner());
            let hits = index
                .search(&text, vector, RESULTS)
                .into_iter()
                .map(|f| Hit {
                    path: f.path,
                    snippet: f.snippet,
                })
                .collect();
            (hits, index.embedded() > 0)
        };
        let (hits, embedded) = read(None);
        news(News::Found { id, hits });
        let model = embedder.lock().map(|e| e.clone()).unwrap_or(None);
        let Some(model) = model.filter(|_| embedded) else {
            continue;
        };
        match asked.recv_timeout(SETTLE) {
            Ok(newer) => {
                next = Some(newer);
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
        }
        match ai::embed(&model, std::slice::from_ref(&text)) {
            Ok(mut vectors) => {
                if let Some(mut v) = vectors.pop() {
                    index::normalize(&mut v);
                    let (hits, _) = read(Some(&v));
                    news(News::Found { id, hits });
                }
            }
            Err(err) => tracing::debug!(%err, "the query has no vector"),
        }
    }
}

/// Every read of the indexer goes through here: what is never read is
/// refused, and a test sees the rest.
struct Gate {
    home: Option<PathBuf>,
    probe: Option<Probe>,
}

impl Gate {
    fn check(&self, path: &Path) -> io::Result<()> {
        if deny::denied(path, self.home.as_deref()) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "never read",
            ));
        }
        if let Some(probe) = &self.probe {
            probe(path);
        }
        Ok(())
    }

    fn read_dir(&self, path: &Path) -> io::Result<fs::ReadDir> {
        self.check(path)?;
        fs::read_dir(path)
    }

    fn read(&self, path: &Path, format: Format) -> io::Result<Vec<u8>> {
        use std::os::unix::fs::OpenOptionsExt;
        self.check(path)?;
        let file = fs::OpenOptions::new()
            .read(true)
            // Not through a link put there since the folder was listed.
            .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
            .open(path)?;
        extract::read_all(file, format)
    }

    fn ocr(&self, program: &str, path: &Path) -> io::Result<Option<String>> {
        self.check(path)?;
        Ok(extract::ocr(program, path))
    }
}

/// `name` matches the glob `pattern` (`*` and `?`), case aside.
fn glob(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ni));
            pi += 1;
        } else if let Some((sp, sn)) = star {
            pi = sp + 1;
            ni = sn + 1;
            star = Some((sp, sn + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|c| *c == '*')
}

/// A look at the folders, a step at a time.
#[derive(Debug, Default)]
struct Walk {
    /// Folders to list: the folder, whether only its pictures count, its
    /// depth.
    folders: Vec<(PathBuf, bool, usize)>,
    /// Files listed, to read.
    files: VecDeque<(PathBuf, Format, i128, u64)>,
    seen: HashSet<PathBuf>,
    denied: usize,
    unreadable: usize,
}

/// What the indexer is doing: an explicit state machine, a step at a time
/// between commands.
#[derive(Debug)]
enum Work {
    /// Nothing to do until configured.
    Idle,
    /// The next look at the folders is due then.
    Until(Instant),
    Walking(Box<Walk>),
    /// Vectors for documents without.
    Embedding,
}

struct Indexer {
    settings: Option<Settings>,
    battery: bool,
    index: Arc<RwLock<Index>>,
    /// The model making vectors, its key read, for the searcher too.
    embedder: Arc<Mutex<Option<Embedder>>>,
    secrets: Option<Secrets>,
    gate: Gate,
    news: Arc<dyn Fn(News) + Send + Sync>,
    work: Work,
    status: Status,
    sent: Option<(Status, Instant)>,
    dirty: bool,
    saved: Instant,
    /// The model did not answer this look: words only until the next.
    embed_failed: bool,
}

impl Indexer {
    fn run(mut self, commands: &mpsc::Receiver<Command>) {
        loop {
            let busy = self.settings.is_some()
                && !self.battery
                && match &self.work {
                    Work::Idle => false,
                    Work::Until(at) => Instant::now() >= *at,
                    Work::Walking(_) | Work::Embedding => true,
                };
            let command = if busy {
                match commands.try_recv() {
                    Ok(c) => Some(c),
                    Err(TryRecvError::Empty) => None,
                    Err(TryRecvError::Disconnected) => return self.save(),
                }
            } else {
                let wait = match &self.work {
                    Work::Until(at) if self.settings.is_some() && !self.battery => {
                        at.saturating_duration_since(Instant::now())
                    }
                    _ => Duration::from_secs(3600),
                };
                match commands.recv_timeout(wait) {
                    Ok(c) => Some(c),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => return self.save(),
                }
            };
            if let Some(command) = command {
                self.command(command);
                continue;
            }
            if busy {
                self.batch();
                std::thread::sleep(PACE);
            }
            self.report(false);
        }
    }

    fn command(&mut self, command: Command) {
        match command {
            Command::Battery(battery) => {
                if battery != self.battery {
                    tracing::info!(battery, "document search follows the power");
                }
                self.battery = battery;
            }
            Command::Off(store) => {
                self.settings = None;
                *self.index.write().unwrap_or_else(|e| e.into_inner()) = Index::default();
                // Turned off: nothing kept.
                if let Some(store) = &store
                    && let Err(err) = fs::remove_file(store)
                    && err.kind() != io::ErrorKind::NotFound
                {
                    tracing::warn!(%err, "the search index was not deleted");
                }
                self.set_embedder(None);
                self.work = Work::Idle;
                self.status = Status::default();
            }
            Command::Configure(mut settings) => {
                // The home as the folders' paths will read once resolved.
                settings.home = settings
                    .home
                    .take()
                    .map(|h| fs::canonicalize(&h).unwrap_or(h));
                let first = self.settings.is_none();
                let moved = self.settings.as_ref().map(|s| &s.store) != Some(&settings.store);
                self.gate.home = settings.home.clone();
                if moved {
                    self.load(&settings);
                }
                let delay = if first {
                    settings.delay
                } else {
                    Duration::ZERO
                };
                if self.settings.as_ref().map(|s| &s.embedder) != Some(&settings.embedder) {
                    self.set_embedder(None);
                }
                self.settings = Some(*settings);
                self.work = Work::Until(Instant::now() + delay);
                self.embed_failed = false;
            }
        }
        self.report(true);
    }

    fn set_embedder(&self, embedder: Option<Embedder>) {
        if let Ok(mut slot) = self.embedder.lock() {
            *slot = embedder;
        }
    }

    /// The index as it was kept, without what is no longer searched.
    fn load(&mut self, settings: &Settings) {
        let mut fresh = Index::default();
        if let Some(store) = &settings.store {
            match fs::File::open(store) {
                Ok(file) => {
                    let mut reader = io::BufReader::new(file);
                    let home = settings.home.clone();
                    let roots: Vec<PathBuf> = settings
                        .roots
                        .iter()
                        .filter_map(|r| r.folder.resolve(home.as_deref()))
                        .filter(|p| !deny::denied(p, home.as_deref()))
                        .map(|p| fs::canonicalize(&p).unwrap_or(p))
                        .collect();
                    match Index::load(&mut reader, |p| {
                        !deny::denied(p, home.as_deref()) && roots.iter().any(|r| p.starts_with(r))
                    }) {
                        Ok(index) => fresh = index,
                        Err(err) => tracing::warn!(%err, "the search index kept is read again"),
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => tracing::warn!(%err, "the search index kept was not read"),
            }
        }
        *self.index.write().unwrap_or_else(|e| e.into_inner()) = fresh;
    }

    fn save(&mut self) {
        let Some(store) = self.settings.as_ref().and_then(|s| s.store.clone()) else {
            return;
        };
        if !self.dirty {
            return;
        }
        let mut bytes = Vec::new();
        {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            if let Err(err) = index.save(&mut bytes) {
                tracing::warn!(%err, "the search index was not written");
                return;
            }
        }
        let written = (|| -> io::Result<()> {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            if let Some(dir) = store.parent() {
                fs::DirBuilder::new()
                    .recursive(true)
                    .mode(0o700)
                    .create(dir)?;
            }
            let part = store.with_extension("part");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&part)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&part, &store)
        })();
        match written {
            Ok(()) => {
                self.dirty = false;
                self.saved = Instant::now();
            }
            Err(err) => tracing::warn!(%err, "the search index was not written"),
        }
    }

    /// Tells the state when it changed, at most a few times a second
    /// unless `now`.
    fn report(&mut self, now: bool) {
        let phase = match (&self.settings, &self.work) {
            (None, _) => Phase::Off,
            (Some(_), _) if self.battery => Phase::Paused,
            (Some(_), Work::Walking(_) | Work::Embedding) => Phase::Indexing,
            (Some(_), Work::Until(_)) if self.status.phase == Phase::Ready => Phase::Ready,
            (Some(_), _) => Phase::Waiting,
        };
        let (documents, embedded) = {
            let index = self.index.read().unwrap_or_else(|e| e.into_inner());
            (index.len(), index.embedded())
        };
        self.status.phase = phase;
        self.status.documents = documents;
        self.status.embedded = embedded;
        self.status.meaning =
            embedded > 0 && self.embedder.lock().map(|e| e.is_some()).unwrap_or(false);
        let changed = self.sent.as_ref().is_none_or(|(s, _)| *s != self.status);
        let due = now
            || self.sent.as_ref().is_none_or(|(s, at)| {
                s.phase != phase || at.elapsed() >= Duration::from_millis(250)
            });
        if changed && due {
            self.sent = Some((self.status.clone(), Instant::now()));
            (self.news)(News::Status(self.status.clone()));
        }
    }

    /// A few steps of the work.
    fn batch(&mut self) {
        match std::mem::replace(&mut self.work, Work::Idle) {
            Work::Idle => {}
            Work::Until(_) => self.work = Work::Walking(Box::new(self.start_walk())),
            Work::Walking(mut walk) => {
                for _ in 0..BATCH {
                    if !self.step(&mut walk) {
                        self.finish(&walk);
                        self.work = Work::Embedding;
                        return;
                    }
                }
                self.work = Work::Walking(walk);
            }
            Work::Embedding => {
                self.work = if self.embed() {
                    Work::Embedding
                } else {
                    self.save();
                    self.status.phase = Phase::Ready;
                    Work::Until(Instant::now() + RESCAN)
                };
            }
        }
        if self.dirty && self.saved.elapsed() >= Duration::from_secs(60) {
            self.save();
        }
    }

    fn start_walk(&mut self) -> Walk {
        let mut walk = Walk::default();
        let Some(settings) = &self.settings else {
            return walk;
        };
        for root in &settings.roots {
            let Some(path) = root.folder.resolve(settings.home.as_deref()) else {
                continue;
            };
            // Checked as named, then as it leads: a link given as the
            // folder itself is followed, the user named it.
            let denied = |p: &Path| deny::denied(p, settings.home.as_deref());
            let path = if denied(&path) {
                path
            } else {
                fs::canonicalize(&path).unwrap_or(path)
            };
            if denied(&path) {
                tracing::warn!(folder = %path.display(), "a folder never read is not searched");
                walk.denied += 1;
                continue;
            }
            walk.folders.push((path, root.pictures, 0));
        }
        // The roots in the configuration's order, the first listed first.
        walk.folders.reverse();
        self.embed_failed = false;
        let embedder = settings.embedder.clone();
        self.resolve(embedder);
        walk
    }

    /// The model and the key for vectors, when there is a model to ask.
    fn resolve(&mut self, embedder: Option<Embedder>) {
        let Some(mut embedder) = embedder else {
            self.set_embedder(None);
            return;
        };
        if embedder.provider != ai::Provider::Ollama
            && let Some(secrets) = &self.secrets
        {
            match secrets.lookup(embedder.provider.name()) {
                Ok(key) => embedder.key = key,
                Err(err) => tracing::warn!(err, "the keyring did not give the key"),
            }
        }
        if embedder.model.is_empty() {
            match ai::pick_embedding_model(&embedder) {
                Ok(model) => embedder.model = model,
                Err(err) => {
                    tracing::info!(%err, "no model for vectors: documents are searched by words");
                    self.set_embedder(None);
                    return;
                }
            }
        }
        self.index
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .set_model(&embedder.model);
        self.set_embedder(Some(embedder));
    }

    /// One folder listed or one file read; `false` once all is done.
    fn step(&mut self, walk: &mut Walk) -> bool {
        if let Some((path, format, modified, size)) = walk.files.pop_front() {
            self.read(walk, path, format, modified, size);
            return true;
        }
        let Some((folder, pictures, depth)) = walk.folders.pop() else {
            return false;
        };
        let Some(settings) = &self.settings else {
            return false;
        };
        let entries = match self.gate.read_dir(&folder) {
            Ok(entries) => entries,
            Err(err) => {
                tracing::debug!(%err, folder = %folder.display(), "a folder is not read");
                walk.unreadable += 1;
                return true;
            }
        };
        let mut listed: Vec<fs::DirEntry> = entries.filter_map(Result::ok).collect();
        listed.sort_by_key(fs::DirEntry::file_name);
        let mut folders = Vec::new();
        for entry in listed {
            let name = entry.file_name().to_string_lossy().into_owned();
            // First of all, before even its type is asked.
            if deny::denied_name(&name) {
                walk.denied += 1;
                continue;
            }
            // Hidden ones are not searched; nor what is left out.
            if name.starts_with('.') || settings.exclude.iter().any(|g| glob(g, &name)) {
                continue;
            }
            let path = folder.join(&name);
            if deny::denied(&path, settings.home.as_deref()) {
                walk.denied += 1;
                continue;
            }
            let Ok(kind) = entry.file_type() else {
                walk.unreadable += 1;
                continue;
            };
            // Links are never followed: they may lead out of the folder.
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if !pictures && depth < MAX_DEPTH {
                    folders.push((path, pictures, depth + 1));
                }
                continue;
            }
            let Some(format) = Format::of(&name) else {
                continue;
            };
            if (format == Format::Picture) != pictures || !kind.is_file() {
                continue;
            }
            let Ok(meta) = entry.metadata() else {
                walk.unreadable += 1;
                continue;
            };
            if meta.len() > extract::MAX_FILE || walk.seen.len() >= MAX_DOCUMENTS {
                continue;
            }
            if !walk.seen.insert(path.clone()) {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i128);
            walk.files.push_back((path, format, modified, meta.len()));
        }
        // Sub-folders in name order, after this folder's files.
        walk.folders.extend(folders.into_iter().rev());
        true
    }

    /// Reads a file that changed since the index kept it.
    fn read(&mut self, walk: &mut Walk, path: PathBuf, format: Format, modified: i128, size: u64) {
        let unchanged = self
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&path)
            .is_some_and(|d| d.modified == modified && d.size == size);
        if unchanged {
            return;
        }
        let ocr = self
            .settings
            .as_ref()
            .map(|s| s.ocr.clone())
            .unwrap_or_default();
        let text = if format == Format::Picture {
            self.gate.ocr(&ocr, &path)
        } else {
            self.gate
                .read(&path, format)
                .map(|data| extract::text(format, &data))
        };
        match text {
            Ok(text) => {
                self.index
                    .write()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(path, modified, size, text.as_deref().unwrap_or_default());
                self.dirty = true;
            }
            Err(err) => {
                tracing::debug!(%err, path = %path.display(), "a document is not read");
                walk.unreadable += 1;
            }
        }
    }

    /// The look is over: documents no longer there go.
    fn finish(&mut self, walk: &Walk) {
        let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
        for path in index.paths() {
            if !walk.seen.contains(&path) {
                index.remove(&path);
                self.dirty = true;
            }
        }
        drop(index);
        self.status.denied = walk.denied;
        self.status.unreadable = walk.unreadable;
        tracing::info!(
            documents = walk.seen.len(),
            never_read = walk.denied,
            unreadable = walk.unreadable,
            "documents indexed"
        );
        self.save();
    }

    /// Vectors for a few documents without; `false` when none is left or
    /// the model does not answer.
    fn embed(&mut self) -> bool {
        if self.embed_failed {
            return false;
        }
        let Some(embedder) = self.embedder.lock().map(|e| e.clone()).unwrap_or(None) else {
            return false;
        };
        let todo = self
            .index
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .unembedded(EMBED_BATCH);
        if todo.is_empty() {
            return false;
        }
        let mut texts = Vec::new();
        let mut owners = Vec::new();
        for (i, (_, text)) in todo.iter().enumerate() {
            for chunk in chunks(text) {
                texts.push(chunk);
                owners.push(i);
            }
        }
        match ai::embed(&embedder, &texts) {
            Ok(vectors) => {
                let mut index = self.index.write().unwrap_or_else(|e| e.into_inner());
                for (i, (path, _)) in todo.iter().enumerate() {
                    let own: Vec<Vec<f32>> = vectors
                        .iter()
                        .zip(&owners)
                        .filter(|(_, o)| **o == i)
                        .map(|(v, _)| v.clone())
                        .collect();
                    index.set_vectors(path, &own);
                }
                self.dirty = true;
                true
            }
            Err(err) => {
                tracing::warn!(%err, "no vectors: documents are searched by words");
                self.embed_failed = true;
                false
            }
        }
    }
}

/// The start of a text in a few pieces, cut between words.
fn chunks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if current.len() + word.len() + 1 > CHUNK {
            out.push(std::mem::take(&mut current));
            if out.len() == CHUNKS {
                return out;
            }
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests;
