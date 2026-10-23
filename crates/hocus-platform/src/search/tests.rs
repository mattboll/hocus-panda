use std::{
    os::unix::fs::PermissionsExt,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use super::*;

/// A service in a private folder, what it says and what it opens kept.
struct Running {
    search: Search,
    news: Arc<Mutex<Vec<News>>>,
    opened: Arc<Mutex<Vec<PathBuf>>>,
}

fn start() -> Running {
    let news = Arc::new(Mutex::new(Vec::new()));
    let opened = Arc::new(Mutex::new(Vec::new()));
    let (kept, seen) = (news.clone(), opened.clone());
    let search = Search::start(
        None,
        move |n| kept.lock().expect("a lock").push(n),
        Some(Arc::new(move |p: &Path| {
            seen.lock().expect("a lock").push(p.to_owned());
        })),
    )
    .expect("the threads");
    Running {
        search,
        news,
        opened,
    }
}

impl Running {
    fn status(&self) -> Option<Status> {
        self.news
            .lock()
            .expect("a lock")
            .iter()
            .rev()
            .find_map(|n| match n {
                News::Status(s) => Some(s.clone()),
                News::Found { .. } => None,
            })
    }

    fn wait(&self, what: &str, ok: impl Fn(&Status) -> bool) -> Status {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(s) = self.status().filter(|s| ok(s)) {
                return s;
            }
            assert!(Instant::now() < deadline, "{what}: {:?}", self.status());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn find(&self, id: u64, text: &str) -> Vec<String> {
        self.search.query(id, text.into());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let found = self
                .news
                .lock()
                .expect("a lock")
                .iter()
                .find_map(|n| match n {
                    News::Found { id: i, hits } if *i == id => Some(hits.clone()),
                    _ => None,
                });
            if let Some(hits) = found {
                return hits
                    .iter()
                    .filter_map(|h| h.path.file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect();
            }
            assert!(Instant::now() < deadline, "no answer to {text:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn settings(root: &Path, store: Option<PathBuf>) -> Settings {
    Settings {
        roots: vec![Root {
            folder: Folder::Path(root.to_owned()),
            pictures: false,
        }],
        exclude: Vec::new(),
        ocr: String::new(),
        embedder: None,
        store,
        delay: Duration::ZERO,
        home: None,
    }
}

fn write(path: &Path, contents: &[u8]) {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).expect("a folder");
    }
    fs::write(path, contents).expect("written");
}

#[test]
fn nothing_under_ssh_or_named_as_a_secret_is_ever_opened() {
    let dir = tempfile::tempdir().expect("a folder");
    let docs = dir.path().join("Documents");
    write(&docs.join("report.md"), b"The bamboo report.");
    write(&docs.join(".ssh/id_ed25519"), b"bamboo: not a key");
    write(&docs.join(".ssh/notes.txt"), b"bamboo");
    write(&docs.join("project/.env"), b"TOKEN=bamboo");
    write(&docs.join("project/server.pem"), b"bamboo");
    write(&docs.join("project/credentials.txt"), b"bamboo");
    write(&docs.join(".my-vault/plan.md"), b"bamboo");
    // A link out of the folder, to a secret elsewhere.
    write(&dir.path().join("elsewhere/secret.txt"), b"bamboo");
    std::os::unix::fs::symlink(dir.path().join("elsewhere"), docs.join("link")).expect("a link");
    std::os::unix::fs::symlink(
        dir.path().join("elsewhere/secret.txt"),
        docs.join("link.txt"),
    )
    .expect("a link");
    // Unreadable as well: reading them would show as an error.
    for path in [docs.join(".ssh/id_ed25519"), docs.join(".ssh")] {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).expect("chmod");
    }
    let run = start();
    run.search.configure(settings(&docs, None));
    let status = run.wait("indexed", |s| s.phase == Phase::Ready);
    fs::set_permissions(docs.join(".ssh"), fs::Permissions::from_mode(0o700)).expect("chmod");
    let opened = run.opened.lock().expect("a lock").clone();
    for path in &opened {
        let shown = path.to_string_lossy();
        for never in [
            ".ssh",
            ".env",
            "server.pem",
            "credentials",
            ".my-vault",
            "elsewhere",
            "link",
        ] {
            assert!(!shown.contains(never), "{shown} was opened");
        }
    }
    assert!(opened.contains(&docs.join("report.md")), "{opened:?}");
    assert_eq!(status.unreadable, 0, "no attempt failed: {status:?}");
    assert_eq!(status.denied, 5, "{status:?}");
    assert_eq!(status.documents, 1);
    assert_eq!(run.find(1, "bamboo"), ["report.md"]);
}

#[test]
fn a_folder_inside_one_never_read_is_not_searched() {
    let dir = tempfile::tempdir().expect("a folder");
    let keys = dir.path().join(".gnupg");
    write(&keys.join("notes.txt"), b"bamboo");
    let run = start();
    run.search.configure(settings(&keys, None));
    let status = run.wait("looked", |s| s.phase == Phase::Ready);
    assert_eq!(status.documents, 0);
    assert!(run.opened.lock().expect("a lock").is_empty());
}

#[test]
fn on_battery_the_indexer_waits_and_on_the_charger_it_runs() {
    let dir = tempfile::tempdir().expect("a folder");
    write(&dir.path().join("plan.txt"), b"Plant bamboo in spring.");
    let run = start();
    run.search.set_battery(true);
    run.search.configure(settings(dir.path(), None));
    run.wait("paused", |s| s.phase == Phase::Paused);
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        run.opened.lock().expect("a lock").is_empty(),
        "nothing read"
    );
    assert_eq!(run.status().map(|s| s.documents), Some(0));
    run.search.set_battery(false);
    let status = run.wait("indexed", |s| s.phase == Phase::Ready);
    assert_eq!(status.documents, 1);
    assert_eq!(run.find(1, "spring"), ["plan.txt"]);
}

#[test]
fn pdf_and_office_documents_are_found_by_their_words() {
    let dir = tempfile::tempdir().expect("a folder");
    write(
        &dir.path().join("report.pdf"),
        &pdf::write(&["Quarterly report", "Bamboo harvest up"], true, true),
    );
    write(
        &dir.path().join("minutes.odt"),
        &zip::write(
            &[(
                "content.xml",
                br#"<office:document-content xmlns:office="o" xmlns:text="t"><text:p>Meeting about the greenhouse</text:p></office:document-content>"#,
            )],
            true,
        ),
    );
    write(
        &dir.path().join("letter.docx"),
        &zip::write(
            &[(
                "word/document.xml",
                br#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Dear zookeeper</w:t></w:r></w:p></w:body></w:document>"#,
            )],
            true,
        ),
    );
    write(
        &dir.path().join("page.html"),
        b"<p>Visit the <b>aviary</b></p>",
    );
    write(&dir.path().join("photo.png"), b"\x89PNG not searched here");
    let run = start();
    run.search.configure(settings(dir.path(), None));
    let status = run.wait("indexed", |s| s.phase == Phase::Ready);
    assert_eq!(
        status.documents, 4,
        "pictures only in the screenshots folder"
    );
    assert_eq!(run.find(1, "harvest"), ["report.pdf"]);
    assert_eq!(run.find(2, "greenho"), ["minutes.odt"]);
    assert_eq!(run.find(3, "zookeeper"), ["letter.docx"]);
    assert_eq!(run.find(4, "aviary"), ["page.html"]);
    assert_eq!(run.find(5, "letter"), ["letter.docx"], "by name too");
}

#[test]
fn screenshots_are_read_by_the_ocr_program() {
    let dir = tempfile::tempdir().expect("a folder");
    let shots = dir.path().join("Screenshots");
    write(&shots.join("Screenshot 1.png"), b"\x89PNG");
    write(&shots.join("notes.txt"), b"not a picture");
    let ocr = dir.path().join("ocr");
    write(
        &ocr,
        b"#!/bin/sh\nprintf 'segmentation fault in libpanda\\n'\n",
    );
    fs::set_permissions(&ocr, fs::Permissions::from_mode(0o755)).expect("chmod");
    let run = start();
    let mut s = settings(&shots, None);
    s.roots[0].pictures = true;
    s.ocr = ocr.display().to_string();
    run.search.configure(s);
    let status = run.wait("indexed", |s| s.phase == Phase::Ready);
    assert_eq!(status.documents, 1);
    assert_eq!(run.find(1, "libpanda"), ["Screenshot 1.png"]);
}

#[test]
fn the_index_is_kept_and_read_again_only_for_what_changed() {
    let dir = tempfile::tempdir().expect("a folder");
    let docs = dir.path().join("docs");
    let store = dir.path().join("state/search/index");
    write(&docs.join("a.txt"), b"alpha bamboo");
    write(&docs.join("b.txt"), b"beta bamboo");
    {
        let run = start();
        run.search.configure(settings(&docs, Some(store.clone())));
        run.wait("indexed", |s| s.phase == Phase::Ready && s.documents == 2);
    }
    let mode = fs::metadata(&store).expect("kept").permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "the user's only");
    // A new session: found at once, before the folders are looked at.
    let run = start();
    let mut later = settings(&docs, Some(store.clone()));
    later.delay = Duration::from_secs(3600);
    run.search.configure(later);
    run.wait("loaded", |s| s.documents == 2);
    assert_eq!(run.find(1, "alpha"), ["a.txt"]);
    assert!(run.opened.lock().expect("a lock").is_empty());
    drop(run);
    // Then only what changed is read again.
    write(&docs.join("b.txt"), b"gamma bamboo, longer now");
    let run = start();
    run.search.configure(settings(&docs, Some(store.clone())));
    run.wait("indexed", |s| s.phase == Phase::Ready);
    let opened = run.opened.lock().expect("a lock").clone();
    assert!(!opened.contains(&docs.join("a.txt")), "{opened:?}");
    assert!(opened.contains(&docs.join("b.txt")), "{opened:?}");
    assert_eq!(run.find(2, "gamma"), ["b.txt"]);
    // Turned off: nothing kept.
    run.search.turn_off(Some(store.clone()));
    run.wait("off", |s| s.phase == Phase::Off);
    let deadline = Instant::now() + Duration::from_secs(5);
    while store.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!store.exists());
}

#[test]
fn the_user_s_folders_come_from_user_dirs() {
    let dir = tempfile::tempdir().expect("a folder");
    let home = dir.path();
    let documents = Folder::User {
        key: "DOCUMENTS",
        fallback: "Documents",
        sub: "",
    };
    // XDG_CONFIG_HOME is not set by these tests: ~/.config.
    if std::env::var_os("XDG_CONFIG_HOME").is_none() {
        assert_eq!(documents.resolve(Some(home)), Some(home.join("Documents")));
        write(
            &home.join(".config/user-dirs.dirs"),
            b"XDG_DOCUMENTS_DIR=\"$HOME/Papiers\"\nXDG_DESKTOP_DIR=\"$HOME/\"\n",
        );
        assert_eq!(documents.resolve(Some(home)), Some(home.join("Papiers")));
        let desktop = Folder::User {
            key: "DESKTOP",
            fallback: "Desktop",
            sub: "",
        };
        assert_eq!(desktop.resolve(Some(home)), None, "the home itself: off");
    }
    assert_eq!(documents.resolve(None), None);
}

#[test]
fn globs_leave_names_out() {
    assert!(glob("*.log", "build.LOG"));
    assert!(glob("draft-??.md", "draft-01.md"));
    assert!(!glob("*.log", "log.txt"));
    assert!(glob("*", "anything"));
}

#[test]
fn texts_are_cut_into_a_few_chunks() {
    let long = "word ".repeat(2000);
    let c = chunks(&long);
    assert_eq!(c.len(), CHUNKS);
    assert!(c.iter().all(|c| c.len() <= CHUNK));
    assert_eq!(chunks("short text"), ["short text"]);
}

/// Random edits of good documents, the same on every run (#134): no
/// reader panics, loops or grows without bounds on what a folder holds.
#[test]
fn documents_broken_at_random_are_read_without_harm() {
    struct Rng(u64);
    impl Rng {
        fn below(&mut self, n: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % n.max(1) as u64) as usize
        }
    }
    const PIECES: &[&[u8]] = &[
        b"(",
        b")",
        b"<<",
        b">>",
        b"[",
        b"]",
        b"<",
        b">",
        b"/",
        b"\\",
        b"obj",
        b"endobj",
        b"stream\n",
        b"endstream",
        b"12 0 R",
        b"/Length 99999",
        b"/Filter /FlateDecode",
        b"/Type /ObjStm /N 99999 /First 4",
        b"beginbfrange",
        b"<0000> <FFFF> <0041>",
        b"BT",
        b"Tj",
        b"TJ",
        b"ID",
        b"PK\x03\x04",
        b"PK\x05\x06",
        b"PK\x01\x02",
        b"<script>",
        b"&#x110000;",
        b"&",
        b"\0",
        b"\xff\xfe",
    ];
    let mangle = |seed: &[u8], rng: &mut Rng| -> Vec<u8> {
        let mut bytes = seed.to_vec();
        for _ in 0..=rng.below(8) {
            let at = rng.below(bytes.len() + 1);
            match rng.below(5) {
                0 if !bytes.is_empty() => {
                    let end = (at + rng.below(64)).min(bytes.len());
                    bytes.drain(at.min(end)..end);
                }
                1 => {
                    let piece = PIECES[rng.below(PIECES.len())];
                    bytes.splice(at..at, piece.iter().copied());
                }
                2 if !bytes.is_empty() => {
                    let i = at.min(bytes.len() - 1);
                    bytes[i] = rng.below(256) as u8;
                }
                3 if !bytes.is_empty() => {
                    let from = rng.below(bytes.len());
                    let to = (from + rng.below(200)).min(bytes.len());
                    let copy = bytes[from..to].to_vec();
                    bytes.splice(at..at, copy);
                }
                _ => bytes.truncate(at),
            }
        }
        bytes
    };
    let office = zip::write(
        &[
            ("mimetype", b"application/vnd.oasis.opendocument.text"),
            (
                "content.xml",
                br#"<office:document-content xmlns:text="t"><text:p>Bamboo &amp; <text:s/>tea</text:p></office:document-content>"#,
            ),
            (
                "word/document.xml",
                br#"<w:document xmlns:w="w"><w:p><w:r><w:t>Dear keeper</w:t></w:r></w:p></w:document>"#,
            ),
        ],
        true,
    );
    let seeds: Vec<(Format, Vec<u8>)> = vec![
        (
            Format::Pdf,
            pdf::write(&["Quarterly report", "Bamboo up"], false, false),
        ),
        (Format::Pdf, pdf::write(&["Café crème"], true, true)),
        (Format::OpenDocument, office.clone()),
        (Format::Docx, office),
        (
            Format::Html,
            b"<html><title>T</title><script>x<y</script><p>a &amp; b &#233;</p><!-- c -->".to_vec(),
        ),
        (Format::Text, "Pandas é\nbamboo".as_bytes().to_vec()),
    ];
    let mut rng = Rng(0x5eed_cafe_f00d_d00d);
    let started = Instant::now();
    for (format, seed) in &seeds {
        for _ in 0..2000 {
            let broken = mangle(seed, &mut rng);
            if let Some(text) = extract::text(*format, &broken) {
                assert!(text.len() <= extract::MAX_TEXT);
                let mut index = index::Index::default();
                index.insert("/d/x".into(), 0, 0, &text);
                let _ = index.search("bamboo cafe", None, 4);
            }
        }
    }
    // An index file broken at random reads as an error, never a panic.
    let mut index = index::Index::default();
    index.insert("/d/a.md".into(), 1, 2, "bamboo café");
    index.set_model("m");
    index.set_vectors(Path::new("/d/a.md"), &[vec![0.5, 0.5]]);
    let mut saved = Vec::new();
    index.save(&mut saved).expect("written");
    for _ in 0..600 {
        let broken = mangle(&saved, &mut rng);
        let _ = index::Index::load(&mut broken.as_slice(), |_| true);
    }
    assert!(
        started.elapsed() < Duration::from_secs(60),
        "{:?}",
        started.elapsed()
    );
}
