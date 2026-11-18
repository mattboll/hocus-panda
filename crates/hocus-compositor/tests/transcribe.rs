//! A meeting's transcription (#618, docs/design/meetings.md).
//!
//! The bar's (the real shell on the CPU, in French, its day a fixture's
//! file at `HOCUS_CLOCK_AT`, against a PipeWire and a D-Bus of the test's
//! own as 5.12's tests start them, `levels/fakes.rs`): the card's
//! Transcrire only while its meeting is in progress and an application
//! uses the microphone; asking, then the in-use tile, the red dot and the
//! stop, the call's own output streams linked to its track.
//!
//! The text's (the compositor alone, told as the shell tells it, on a
//! D-Bus of the test's own with a notifications server of the test's,
//! `hocus-transcribe` with its fake engine, `HOCUS_TRANSCRIBE_FAKE`): the
//! stop writes the file, "Moi" and the voices in order, and the sound
//! goes; a failure keeps it, and "Réessayer" makes it.
//!
//! Each daemon is stopped by its own process id; never the session's. The
//! real engine has its own test, ignored (hocus-transcribe's
//! `real::tests`).

use crate::common;
use crate::fakes;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::Context;
use common::{WAIT, wait};
use fakes::{FakeSink, Fakes, TWO_SINKS};
use hocus_compositor::{
    Config, FormChoice, Headless, HeadlessConfig, RendererChoice, TranscribeStep, model::Rect,
};
use hocus_link::{MEETING_NAMESPACE, TranscribeAsk, TranscribeMeeting};

const MIC: &str = "hocus-test-mic";
/// The call's application, as its streams name it.
const APP: &str = "Visio";
const KEY_TAB: u32 = 15;
const KEY_ENTER: u32 = 28;
const SCREEN: (i32, i32) = (1920, 1080);
/// The full form's card, and its button's middle (Day.2's tests').
const CARD: (i32, i32, i32, i32) = (1920 - 8 - 196 + 12, 8 + 100, 172, 44);
const BUTTON: (i32, i32) = (CARD.0 + 172 - 7 - 15, CARD.1 + 22);

/// What the fake engine says: "Moi", then two voices; the user's echo of
/// the first voice, which the text leaves out.
const SAID: &str = r#"{
    "step_ms": 300,
    "me": [[0.5, 2.0, "Bonjour à tous."], [4.1, 5.0, "On commence par la démo ?"], [9.0, 10.0, "Merci, à jeudi."]],
    "call": [[4.0, 5.0, 1, "On commence par la démo ?"], [6.0, 7.0, 2, "Oui, je partage mon écran."]]
}"#;

/// "Point produit" from `start` to `end` (fixture times), with a call.
fn day(start: &str, end: &str) -> String {
    format!(
        "[[calendars]]\nid = \"work\"\nname = \"Travail\"\ncolor = \"#F5A35B\"\n\n\
         [[events]]\nid = \"point\"\ntitle = \"Point produit\"\nstart = \"{start}\"\nend = \"{end}\"\n\
         video = \"https://meet.example.org/abc-defg\"\nattendees = [\"Alex Martin\", \"Sam\"]\n\n\
         [[events]]\ntitle = \"Revue de sprint\"\nstart = \"today 16:00\"\nend = \"today 17:00\"\n"
    )
}

/// The places a test's compositor keeps a meeting in, and the program it
/// runs: `hocus-transcribe` with the fake engine reading `fake`.
struct Places {
    root: tempfile::TempDir,
}

impl Places {
    fn new() -> anyhow::Result<Self> {
        let root = tempfile::tempdir()?;
        for sub in ["state", "data", "documents", "bin", "home"] {
            std::fs::create_dir_all(root.path().join(sub))?;
        }
        Ok(Self { root })
    }

    fn path(&self, sub: &str) -> PathBuf {
        self.root.path().join(sub)
    }

    fn fake(&self) -> PathBuf {
        self.path("fake.json")
    }

    /// The engine says `said` from now on.
    fn say(&self, said: &str) -> anyhow::Result<()> {
        std::fs::write(self.fake(), said)?;
        Ok(())
    }

    /// `config` with these places, and the fake engine.
    fn config(&self, mut config: Config) -> anyhow::Result<Config> {
        use std::os::unix::fs::PermissionsExt;
        let program = common::transcribe_program();
        let script = self.path("bin/hocus-transcribe");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nexport HOCUS_TRANSCRIBE_FAKE='{}' TZ=UTC\nexec '{}' \"$@\"\n",
                self.fake().display(),
                program.display()
            ),
        )?;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))?;
        config.meeting_places = Some(hocus_meetings::Places {
            state: self.path("state"),
            data: self.path("data"),
        });
        config.documents = Some(self.path("documents"));
        config.transcriber = script;
        config.lang = hocus_words::Lang::French;
        Ok(config)
    }
}

/// The meeting the shell asks to transcribe.
fn point() -> TranscribeMeeting {
    TranscribeMeeting {
        event: "point".to_owned(),
        title: "Point produit".to_owned(),
        start: "2026-11-17T14:00:00Z".to_owned(),
        end: "2026-11-17T14:32:00Z".to_owned(),
        attendees: vec!["Alex Martin".to_owned(), "Sam".to_owned()],
        app: APP.to_owned(),
    }
}

/// The compositor alone, its notifications on a server of the test's own,
/// no PipeWire (its tracks silent).
fn alone(places: &Places) -> anyhow::Result<(Headless, common::Notifications)> {
    let server = common::Notifications::start(&places.path("bus"))?.context("no notifications")?;
    let config = places.config(Config {
        pipewire_remote: Some(places.path("no-pipewire")),
        ..Config::default()
    })?;
    let hp = Headless::start_on_bus(
        HeadlessConfig {
            outputs: vec!["1280x800".parse()?],
            renderer: RendererChoice::Pixman,
            ..HeadlessConfig::default()
        },
        config,
        None,
        Some(server.address.clone()),
    )?;
    Ok((hp, server))
}

/// Waits until job `id` is at a step `pass` takes; that step.
fn step(
    hp: &mut Headless,
    id: u64,
    pass: impl Fn(&TranscribeStep) -> bool,
) -> anyhow::Result<TranscribeStep> {
    let mut seen = None;
    let done = hp.dispatch_until(Duration::from_secs(20), |hp| {
        seen = hp
            .transcriptions()
            .jobs
            .into_iter()
            .find(|(job, _, _)| *job == id)
            .map(|(_, _, step)| step);
        seen.as_ref().is_some_and(&pass)
    })?;
    anyhow::ensure!(done, "job {id} never got there: {seen:?}");
    seen.context("no job")
}

/// Records `point` for `seconds`, then stops: the job's id and folder.
fn record(hp: &mut Headless, seconds: u64) -> anyhow::Result<(u64, PathBuf)> {
    hp.transcribe_ask(TranscribeAsk::Start(point()));
    let (id, dir) = hp
        .transcriptions()
        .recording
        .context("no recording started")?;
    wait(hp, Duration::from_secs(seconds))?;
    // Its folder: its record and its two tracks, as they are recorded.
    for file in ["meeting.json", "me.wav", "call.wav"] {
        anyhow::ensure!(dir.join(file).exists(), "no {file} in {}", dir.display());
    }
    hp.transcribe_ask(TranscribeAsk::Stop);
    anyhow::ensure!(hp.transcriptions().recording.is_none());
    Ok((id, dir))
}

/// The notifications said, waited for until one says `summary`.
fn note(
    hp: &mut Headless,
    server: &common::Notifications,
    summary: &str,
) -> anyhow::Result<common::Note> {
    let done = hp.dispatch_until(WAIT, |_| {
        server.notes().iter().any(|n| n.summary == summary)
    })?;
    anyhow::ensure!(done, "never said {summary:?}: {:?}", server.notes());
    server
        .notes()
        .into_iter()
        .rev()
        .find(|n| n.summary == summary)
        .context("no such note")
}

#[test]
fn the_stop_writes_the_file_with_moi_and_the_voices_in_order_and_the_sound_goes()
-> anyhow::Result<()> {
    let places = Places::new()?;
    places.say(SAID)?;
    let (mut hp, server) = alone(&places)?;
    let (id, dir) = record(&mut hp, 2)?;
    let TranscribeStep::Written(file) = step(&mut hp, id, |s| {
        matches!(s, TranscribeStep::Written(_) | TranscribeStep::Failed(_))
    })?
    else {
        anyhow::bail!("not written: {:?}", hp.transcriptions());
    };
    // In the documents' "Réunions", named by the meeting's start and title.
    assert_eq!(
        file,
        places
            .path("documents/Réunions")
            .join("2026-11-17 14-00 Point produit.md")
    );
    let text = std::fs::read_to_string(&file)?;
    assert!(
        text.starts_with(
            "# Point produit\n\nmardi 17 novembre 2026, 14:00 – 14:32  \n\
             Invités (agenda) : Alex Martin, Sam\n\n"
        ),
        "{text}"
    );
    let turns: Vec<&str> = text
        .lines()
        .filter(|l| l.starts_with("**"))
        .map(|l| l.split_once(" · ").map_or(l, |(_, rest)| rest))
        .collect();
    assert_eq!(
        turns,
        [
            "Moi** — Bonjour à tous.",
            "Voix 1** — On commence par la démo ?",
            "Voix 2** — Oui, je partage mon écran.",
            "Moi** — Merci, à jeudi.",
        ],
        "{text}"
    );
    // The sound went with its folder.
    assert!(!dir.exists(), "{} is still there", dir.display());
    // The progress, then the text ready, its buttons.
    let working = note(&mut hp, &server, "Transcription de « Point produit »")?;
    assert!(working.value.is_some(), "{working:?}");
    let ready = note(&mut hp, &server, "Transcription prête")?;
    assert_eq!(
        ready.body,
        "Point produit · moins d’une minute · Moi et 2 voix"
    );
    assert_eq!(ready.actions, ["default", "open", "folder"]);
    assert_eq!(ready.id, working.id, "the progress is replaced");
    Ok(())
}

#[test]
fn a_failure_keeps_the_sound_and_retry_makes_the_text() -> anyhow::Result<()> {
    let places = Places::new()?;
    places.say(r#"{ "fail": "le modèle manque" }"#)?;
    let (mut hp, server) = alone(&places)?;
    let (id, dir) = record(&mut hp, 1)?;
    let failed = step(&mut hp, id, |s| {
        matches!(s, TranscribeStep::Written(_) | TranscribeStep::Failed(_))
    })?;
    assert!(
        matches!(&failed, TranscribeStep::Failed(why) if why.contains("le modèle manque")),
        "{failed:?}"
    );
    // The sound is kept for "Réessayer".
    for file in ["meeting.json", "me.wav", "call.wav"] {
        assert!(dir.join(file).exists(), "no {file} kept");
    }
    let said = note(&mut hp, &server, "La transcription n’a pas pu se faire")?;
    assert_eq!(said.body, "« Point produit » · le son est gardé");
    assert_eq!(said.actions, ["retry"]);
    // It can be made now: "Réessayer".
    places.say(SAID)?;
    server.invoke(said.id, "retry")?;
    let made = step(&mut hp, id, |s| matches!(s, TranscribeStep::Written(_)))?;
    let TranscribeStep::Written(file) = made else {
        anyhow::bail!("not written");
    };
    assert!(
        std::fs::read_to_string(&file)?.contains("**"),
        "an empty text"
    );
    assert!(!dir.exists(), "the sound stays after the text");
    note(&mut hp, &server, "Transcription prête")?;
    Ok(())
}

// --- The bar ----------------------------------------------------------------------

struct Bar {
    hp: Headless,
    fakes: Fakes,
    places: Places,
    log: PathBuf,
    fixture: PathBuf,
}

impl Bar {
    /// The real shell, its clock at 14:04, its day `fixture`, PipeWire and
    /// D-Bus the test's own.
    fn start(fixture: &str, form: FormChoice) -> anyhow::Result<Self> {
        let shell = common::shell_program();
        if !fakes::available() {
            common::missing(
                "dbus-daemon, pipewire, pw-dump, wpctl, pw-metadata, pw-cli and pw-link",
            );
        }
        let places = Places::new()?;
        places.say(SAID)?;
        let config_file = places.path("config.toml");
        std::fs::write(
            &config_file,
            format!(
                "[[day.calendars]]\nkind = \"fixture\"\nfile = \"day.toml\"\n\n[panel]\nform = \"{}\"\n\n\
                 [calendar]\nremind = 0\n",
                form.name()
            ),
        )?;
        let fixture_file = places.path("day.toml");
        std::fs::write(&fixture_file, fixture)?;
        let fakes =
            Fakes::start_with_sources(&TWO_SINKS, &[FakeSink::new(MIC, "Microphone du portable")])?;
        let log = places.path("shell.log");
        let at = |sub: &str| places.path(sub).display().to_string();
        let script = fakes.shell_with(
            &shell,
            "fr_FR.UTF-8",
            &[
                ("HOME", at("home")),
                ("XDG_CONFIG_HOME", at("home")),
                ("XDG_CACHE_HOME", at("home")),
                ("XDG_STATE_HOME", at("state")),
                ("XDG_DATA_HOME", at("data")),
                ("XDG_DATA_DIRS", at("data")),
                ("HOCUS_VIDEO_DEVICES", at("home")),
                ("HOCUS_CLOCK_AT", "2026-11-04T14:04:00".to_owned()),
                ("ICED_BACKEND", "tiny-skia".to_owned()),
                ("TZ", "UTC".to_owned()),
                ("RUST_LOG", "warn,hocuspanda_shell=debug".to_owned()),
            ],
            Some(&log),
        )?;
        let mut config = places.config(Config {
            file: Some(config_file),
            plain_background: true,
            pipewire_remote: Some(fakes.pw_socket()),
            ..Config::default()
        })?;
        config.keyboard.layouts = vec!["us".to_owned()];
        config.panel.form = form;
        let mut hp = Headless::start_on_bus(
            HeadlessConfig {
                outputs: vec![format!("{}x{}", SCREEN.0, SCREEN.1).parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
            Some(script),
            Some(fakes.address.clone()),
        )?;
        let up = hp.dispatch_until(Duration::from_secs(20), |hp| hp.bar_commits() > 0)?;
        anyhow::ensure!(up, "no bar");
        hp.pointer_move(400.0, 600.0);
        Ok(Self {
            hp,
            fakes,
            places,
            log,
            fixture: fixture_file,
        })
    }

    fn log(&self) -> String {
        let text =
            String::from_utf8_lossy(&std::fs::read(&self.log).unwrap_or_default()).into_owned();
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    fn mark(&self) -> usize {
        self.log().len()
    }

    /// Waits until a line logged after `mark` holds `what` and all of
    /// `holds`; that line.
    fn logged(&mut self, mark: usize, what: &str, holds: &[&str]) -> anyhow::Result<String> {
        let start = Instant::now();
        while start.elapsed() < WAIT * 2 {
            let log = self.log();
            let found = log
                .get(mark..)
                .unwrap_or_default()
                .lines()
                .filter(|l| l.contains(what))
                .rev()
                .find(|l| holds.iter().all(|h| l.contains(h)))
                .map(str::to_owned);
            if let Some(line) = found {
                return Ok(line);
            }
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        anyhow::bail!("never logged {what} with {holds:?}:\n{}", self.log())
    }

    /// Waits until the card is drawn at rest with its button `button`.
    fn card(&mut self, mark: usize, button: &str) -> anyhow::Result<String> {
        let button = format!("button={button}");
        self.logged(mark, "the meeting card is drawn at rest", &[&button])
    }

    /// The day's file says `fixture` now.
    fn day(&self, fixture: &str) -> anyhow::Result<()> {
        let new = self.fixture.with_extension("new");
        std::fs::write(&new, fixture)?;
        std::fs::rename(&new, &self.fixture)?;
        Ok(())
    }

    /// The call joined: its application takes the microphone, and plays.
    fn join(&mut self) -> anyhow::Result<()> {
        let props = format!(
            "application.name = \"{APP}\" application.process.binary = \"visio\" \
             media.name = \"Point produit\""
        );
        self.fakes
            .capture("hocus-test-visio-mic", "Stream/Input/Audio", &props, MIC)?;
        self.fakes.play("hocus-test-visio-out", APP, None)?;
        Ok(())
    }

    /// A click on the card's button, once the shell draws it lit as `part`.
    fn click_button(&mut self, part: &str) -> anyhow::Result<()> {
        let lit = format!("lit=Some({part})");
        let start = Instant::now();
        loop {
            let mark = self.mark();
            common::over(
                &mut self.hp,
                (f64::from(BUTTON.0), f64::from(BUTTON.1)),
                MEETING_NAMESPACE,
            )?;
            wait(&mut self.hp, Duration::from_millis(150))?;
            if self
                .log()
                .get(mark..)
                .unwrap_or_default()
                .lines()
                .any(|l| l.contains("the meeting card is drawn at rest") && l.contains(&lit))
            {
                break;
            }
            anyhow::ensure!(
                start.elapsed() < WAIT,
                "the card never drew its {part} lit:\n{}",
                self.log()
            );
            self.hp.pointer_move(400.0, 600.0);
            wait(&mut self.hp, Duration::from_millis(50))?;
        }
        self.hp.button(common::BTN_LEFT, true);
        self.hp.button(common::BTN_LEFT, false);
        Ok(())
    }

    fn key(&mut self, key: u32) {
        self.hp.key(key, true);
        self.hp.key(key, false);
    }

    /// The asking popover, opened from the card: what it says.
    fn ask(&mut self) -> anyhow::Result<String> {
        let mark = self.mark();
        self.click_button("Transcribe")?;
        let open = self
            .hp
            .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
        anyhow::ensure!(open, "no popover:\n{}", self.log());
        let said = self.logged(mark, "the transcription's popover shows it", &[])?;
        common::at_rest(&mut self.hp)?;
        Ok(said)
    }

    /// Waits until the compositor records, or not.
    fn recording(&mut self, on: bool) -> anyhow::Result<Option<(u64, PathBuf)>> {
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.transcriptions().recording.is_some() == on)?;
        anyhow::ensure!(done, "recording is not {on}:\n{}", self.log());
        Ok(self.hp.transcriptions().recording)
    }
}

const IN_PROGRESS: (&str, &str) = ("now-4m", "now+28m");
const AHEAD: (&str, &str) = ("now+25m", "now+55m");

#[test]
fn the_button_transcribes_only_a_meeting_in_progress_whose_call_uses_the_microphone()
-> anyhow::Result<()> {
    let mut bar = Bar::start(&day(AHEAD.0, AHEAD.1), FormChoice::Full)?;
    bar.card(0, "Join")?;
    // The call joined before the meeting: still its join button.
    bar.join()?;
    bar.logged(0, "the in-use sign is drawn at rest", &["Microphone"])?;
    let mark = bar.mark();
    wait(&mut bar.hp, Duration::from_millis(500))?;
    bar.card(0, "Join")?;
    // In progress, the microphone used: Transcrire.
    bar.day(&day(IN_PROGRESS.0, IN_PROGRESS.1))?;
    let line = bar.card(mark, "Transcribe")?;
    assert!(line.contains("fin dans"), "{line}");
    // The call leaves the microphone: the join button back, once its use
    // has lingered.
    let mark = bar.mark();
    bar.fakes.stop("hocus-test-visio-mic")?;
    bar.card(mark, "Join")?;
    Ok(())
}

#[test]
fn asking_then_the_tile_the_red_dot_and_the_stop() -> anyhow::Result<()> {
    let mut bar = Bar::start(&day(IN_PROGRESS.0, IN_PROGRESS.1), FormChoice::Full)?;
    bar.join()?;
    bar.card(0, "Transcribe")?;
    // Asked: what happens, the reminder, the model to download.
    let said = bar.ask()?;
    for words in [
        "TRANSCRIRE",
        "Point produit",
        "Le son reste sur ce portable.",
        "Préviens les participants qu’ils sont transcrits.",
        "Télécharger le modèle (610 Mo)",
        "Transcrire | Annuler | [ ] Ne plus demander",
    ] {
        assert!(said.contains(words), "{words:?} not in {said}");
    }
    assert!(bar.hp.transcriptions().recording.is_none());
    // Transcrire: the compositor records.
    let mark = bar.mark();
    bar.key(KEY_TAB);
    bar.key(KEY_ENTER);
    let (_, dir) = bar.recording(true)?.context("no recording")?;
    // The card's stop and its red dot, the in-use sign's tile.
    bar.card(mark, "Stop")?;
    bar.logged(mark, "the in-use sign is drawn at rest", &["Transcription"])?;
    // The call's output stream is linked to its track, not the others'.
    let start = Instant::now();
    loop {
        let links = bar.fakes.links();
        if links.contains("hocus-test-visio-out:") && links.contains("hocuspanda-transcribe-call:")
        {
            break;
        }
        anyhow::ensure!(start.elapsed() < WAIT, "the call is not linked:\n{links}");
        wait(&mut bar.hp, Duration::from_millis(100))?;
    }
    // The stop: the recording ends, its text is made, the sound goes.
    let mark = bar.mark();
    bar.click_button("Stop")?;
    bar.recording(false)?;
    let done = bar.hp.dispatch_until(Duration::from_secs(20), |hp| {
        hp.transcriptions()
            .jobs
            .iter()
            .any(|(_, _, s)| matches!(s, TranscribeStep::Written(_)))
    })?;
    anyhow::ensure!(done, "no text: {:?}", bar.hp.transcriptions());
    assert!(!dir.exists());
    // The calendar's attendees at its head.
    let file = bar
        .hp
        .transcriptions()
        .jobs
        .into_iter()
        .find_map(|(_, _, s)| match s {
            TranscribeStep::Written(file) => Some(file),
            _ => None,
        })
        .context("no file")?;
    let text = std::fs::read_to_string(&file)?;
    assert!(
        text.contains("Invités (agenda) : Alex Martin, Sam"),
        "{text}"
    );
    // The card asks again; the tile goes.
    bar.card(mark, "Transcribe")?;
    bar.logged(
        mark,
        "the in-use sign is drawn at rest",
        &["kinds=[Microphone]"],
    )?;
    Ok(())
}

// --- Screenshots ------------------------------------------------------------------

/// The screen's `(x, y, w, h)`, three times as large, saved as `name`.
fn zoom(
    bar: &mut Bar,
    out: &std::path::Path,
    name: &str,
    (x, y, w, h): (u32, u32, u32, u32),
) -> anyhow::Result<()> {
    common::at_rest(&mut bar.hp)?;
    let (sw, sh, px) = bar.hp.screenshot(0)?;
    let rgba: Vec<u8> = px
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| [p[2], p[1], p[0], 255])
        .collect();
    let image = image::RgbaImage::from_raw(sw as u32, sh as u32, rgba).context("image")?;
    let crop = image::imageops::crop_imm(&image, x, y, w, h).to_image();
    let zoomed = image::imageops::resize(&crop, w * 3, h * 3, image::imageops::FilterType::Nearest);
    let file = out.join(format!("{name}.png"));
    zoomed.save(&file)?;
    eprintln!("{}", file.display());
    Ok(())
}

/// The mockup's six steps, for the pull request (`HOCUS_SHOTS=dir`): the
/// bar's top right, and the screen's corner where the notifications show.
#[test]
fn screenshots_for_the_pull_request() -> anyhow::Result<()> {
    let Some(out) = std::env::var_os("HOCUS_SHOTS").map(PathBuf::from) else {
        return Ok(());
    };
    std::fs::create_dir_all(&out)?;
    let shoot = |bar: &mut Bar, name: &str, (x, y, w, h): (u32, u32, u32, u32)| {
        common::at_rest(&mut bar.hp)?;
        let (sw, sh, px) = bar.hp.screenshot(0)?;
        let rgba: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[2], p[1], p[0], 255])
            .collect();
        let image = image::RgbaImage::from_raw(sw as u32, sh as u32, rgba).context("image")?;
        let crop = image::imageops::crop_imm(&image, x, y, w, h).to_image();
        let file = out.join(format!("{name}.png"));
        crop.save(&file)?;
        eprintln!("{}", file.display());
        anyhow::Ok(())
    };
    let corner = (SCREEN.0 as u32 - 760, 0, 760, 520);
    let mut bar = Bar::start(&day(IN_PROGRESS.0, IN_PROGRESS.1), FormChoice::Full)?;
    bar.places
        .say(&SAID.replace("\"step_ms\": 300", "\"step_ms\": 2500"))?;
    bar.card(0, "Join")?;
    shoot(&mut bar, "1-not-joined", corner)?;
    let mark = bar.mark();
    bar.join()?;
    bar.card(mark, "Transcribe")?;
    bar.logged(mark, "the in-use sign is drawn at rest", &["Microphone"])?;
    shoot(&mut bar, "2-joined", corner)?;
    zoom(
        &mut bar,
        &out,
        "2-joined-zoom",
        (SCREEN.0 as u32 - 200, 100, 200, 110),
    )?;
    bar.ask()?;
    shoot(&mut bar, "3-asking", corner)?;
    let mark = bar.mark();
    bar.key(KEY_TAB);
    bar.key(KEY_ENTER);
    bar.recording(true)?;
    bar.card(mark, "Stop")?;
    bar.logged(mark, "the in-use sign is drawn at rest", &["Transcription"])?;
    wait(&mut bar.hp, Duration::from_secs(3))?;
    bar.hp.pointer_move(400.0, 600.0);
    shoot(&mut bar, "4-recording", corner)?;
    // The card and the sign, three times as large.
    zoom(
        &mut bar,
        &out,
        "4-recording-zoom",
        (SCREEN.0 as u32 - 200, 100, 200, 110),
    )?;
    let mark = bar.mark();
    bar.click_button("Stop")?;
    bar.recording(false)?;
    bar.hp.pointer_move(400.0, 600.0);
    bar.logged(mark, "a notification", &["Hocuspanda"])?;
    wait(&mut bar.hp, Duration::from_millis(1500))?;
    shoot(&mut bar, "5-working", corner)?;
    let done = bar.hp.dispatch_until(Duration::from_secs(30), |hp| {
        hp.transcriptions()
            .jobs
            .iter()
            .any(|(_, _, s)| matches!(s, TranscribeStep::Written(_)))
    })?;
    anyhow::ensure!(done, "no text");
    wait(&mut bar.hp, Duration::from_millis(1500))?;
    shoot(&mut bar, "6-ready", corner)?;
    // The file, as written.
    if let Some((_, _, TranscribeStep::Written(file))) = bar
        .hp
        .transcriptions()
        .jobs
        .into_iter()
        .find(|(_, _, s)| matches!(s, TranscribeStep::Written(_)))
    {
        std::fs::copy(&file, out.join("6-ready.md"))?;
    }
    // The mini form: its popover's Transcrire, and the column's tile.
    let mut mini = Bar::start(&day(IN_PROGRESS.0, IN_PROGRESS.1), FormChoice::Mini)?;
    mini.join()?;
    mini.logged(0, "the in-use sign is drawn at rest", &["Microphone"])?;
    let mark_rect: Rect = common::drawn(&mut mini.hp, MEETING_NAMESPACE)?[0].1;
    common::over(
        &mut mini.hp,
        (
            f64::from(mark_rect.x + mark_rect.width / 2),
            f64::from(mark_rect.y + mark_rect.height / 2),
        ),
        MEETING_NAMESPACE,
    )?;
    let open = mini
        .hp
        .dispatch_until(WAIT, |hp| !common::drawn_popovers(hp).is_empty())?;
    anyhow::ensure!(open, "no mini popover");
    mini.logged(0, "the meeting's popover shows it", &["Transcrire"])?;
    shoot(&mut mini, "7-mini-popover", corner)?;
    mini.hp.pointer_move(400.0, 600.0);
    wait(&mut mini.hp, Duration::from_secs(1))?;
    mini.hp
        .transcribe_ask(TranscribeAsk::Start(TranscribeMeeting {
            app: APP.to_owned(),
            ..point()
        }));
    mini.logged(0, "the in-use sign is drawn at rest", &["Transcription"])?;
    wait(&mut mini.hp, Duration::from_secs(2))?;
    shoot(&mut mini, "8-mini-recording", corner)?;
    Ok(())
}
