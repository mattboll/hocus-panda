//! Screenshots' folder, format and shutter followed live (6.17, issue
//! #451; docs/design/screenshots.md): `[capture] folder` and `format`
//! taken by the next capture, the clipboard still getting PNG; a folder
//! that cannot be written passed over for the default one, which comes
//! back once it can; the shutter heard unless the output is muted or
//! `[capture] sound` is off.
//!
//! No shell: `Shift+Print` takes the screen at once. The file is written
//! by hand after the start, as the Settings' worker would, and each test
//! waits until the compositor has read it. The pictures' folder and the
//! shutter's programs (`wpctl`, `pw-play`) are the test's own fakes, which
//! say what they were asked: no sound is ever played.

mod common;

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::Context;
use common::{KEY_LEFTSHIFT, WAIT, chord, wait};
use hocus_compositor::{
    CaptureFormat, CaptureSettings, Config, Headless, HeadlessConfig, RendererChoice, SoundPrograms,
};

/// Print (`KEY_SYSRQ`).
const KEY_PRINT: u32 = 99;

struct Bed {
    hp: Headless,
    dir: tempfile::TempDir,
    file: PathBuf,
}

impl Bed {
    fn start() -> anyhow::Result<Self> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config.toml");
        std::fs::write(&file, "")?;
        let fakes = dir.path().join("bin");
        std::fs::create_dir_all(&fakes)?;
        let log = dir.path().join("sound.log");
        let volume = dir.path().join("volume");
        std::fs::write(&volume, "Volume: 0.40\n")?;
        script(
            &fakes.join("wpctl"),
            &format!(
                "echo \"wpctl $*\" >> '{}'\ncat '{}'\n",
                log.display(),
                volume.display()
            ),
        )?;
        script(
            &fakes.join("pw-play"),
            &format!(
                "echo \"pw-play $*\" >> '{}'\ncat > /dev/null\n",
                log.display()
            ),
        )?;
        let mut config = Config::default();
        config.keyboard.layouts = vec!["us".to_owned()];
        config.plain_background = true;
        config.file = Some(file.clone());
        config.home = Some(dir.path().to_path_buf());
        config.pictures = Some(dir.path().join("Pictures"));
        config.state_dir = Some(dir.path().join("state"));
        config.sound_programs = SoundPrograms::In(fakes);
        let hp = Headless::start_with(
            HeadlessConfig {
                outputs: vec!["1280x720".parse()?],
                renderer: RendererChoice::Pixman,
                ..HeadlessConfig::default()
            },
            config,
        )?;
        let mut bed = Self { hp, dir, file };
        let drawn = bed.hp.dispatch_until(WAIT, |hp| hp.frames() > 0)?;
        anyhow::ensure!(drawn, "nothing drawn");
        Ok(bed)
    }

    /// `[capture]` now says `lines`, once the compositor has read it as
    /// `read` says.
    fn capture(
        &mut self,
        lines: &str,
        read: impl Fn(&CaptureSettings) -> bool,
    ) -> anyhow::Result<()> {
        let new = self.file.with_extension("new");
        std::fs::write(&new, format!("[capture]\n{lines}\n"))?;
        std::fs::rename(&new, &self.file)?;
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| read(&hp.state().config.capture))?;
        anyhow::ensure!(done, "the file is not read: {lines}");
        Ok(())
    }

    /// `Shift+Print`: the screen, at once; its file once written.
    fn shoot(&mut self) -> anyhow::Result<PathBuf> {
        let before = self.hp.captures().0;
        chord(&mut self.hp, &[KEY_LEFTSHIFT, KEY_PRINT]);
        let done = self
            .hp
            .dispatch_until(WAIT, |hp| hp.captures().0 > before)?;
        anyhow::ensure!(done, "no capture made");
        self.hp.captures().1.context("no file written")
    }

    fn at(&self, sub: &str) -> PathBuf {
        self.dir.path().join(sub)
    }

    /// The default folder's.
    fn shots(&self) -> PathBuf {
        self.at("Pictures/Screenshots")
    }

    /// What the shutter's programs were asked, a line each.
    fn sounds(&self) -> Vec<String> {
        std::fs::read_to_string(self.at("sound.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Runs the compositor until the shutter's programs were asked `n`
    /// things.
    fn sounds_until(&mut self, n: usize) -> anyhow::Result<Vec<String>> {
        let log = self.at("sound.log");
        let count = |log: &Path| {
            std::fs::read_to_string(log)
                .unwrap_or_default()
                .lines()
                .count()
        };
        let done = self.hp.dispatch_until(WAIT, |_| count(&log) >= n)?;
        anyhow::ensure!(done, "the shutter's programs heard {:?}", self.sounds());
        Ok(self.sounds())
    }
}

/// An executable shell script at `path`.
fn script(path: &Path, body: &str) -> anyhow::Result<()> {
    std::fs::write(path, format!("#!/bin/sh\n{body}"))?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[test]
fn a_capture_goes_to_the_chosen_folder_as_jpeg_and_the_clipboard_gets_png() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    // The default first: a PNG in the pictures' folder's own.
    let first = bed.shoot()?;
    assert_eq!(first.parent(), Some(bed.shots().as_path()));
    assert!(first.to_string_lossy().ends_with(".png"), "{first:?}");
    // A folder from the home, and JPEG: the next capture takes them.
    std::fs::create_dir_all(bed.at("Bureau/Captures"))?;
    bed.capture("folder = \"~/Bureau/Captures\"\nformat = \"jpeg\"", |c| {
        c.format == CaptureFormat::Jpeg
    })?;
    let file = bed.shoot()?;
    assert_eq!(file.parent(), Some(bed.at("Bureau/Captures").as_path()));
    let name = file.file_name().context("no name")?.to_string_lossy();
    assert!(
        name.starts_with("Screenshot from ") && name.ends_with(".jpg"),
        "{name}"
    );
    let bytes = std::fs::read(&file)?;
    assert_eq!(
        image::guess_format(&bytes)?,
        image::ImageFormat::Jpeg,
        "{name}"
    );
    let jpeg = image::load_from_memory(&bytes)?;
    assert_eq!((jpeg.width(), jpeg.height()), (1280, 720));
    // The clipboard: PNG, of the same image.
    let copied = bed.hp.clipboard_bytes().context("nothing copied")?;
    assert_eq!(image::guess_format(&copied)?, image::ImageFormat::Png);
    let png = image::load_from_memory(&copied)?;
    assert_eq!((png.width(), png.height()), (1280, 720));
    Ok(())
}

#[test]
fn an_unwritable_folder_falls_back_to_the_default_one_until_it_is_back() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    let key = bed.at("usb");
    // A USB key unplugged: its folder is not there.
    bed.capture(&format!("folder = \"{}\"", key.display()), |c| {
        !c.folder.is_empty()
    })?;
    let file = bed.shoot()?;
    assert_eq!(file.parent(), Some(bed.shots().as_path()));
    assert!(!key.exists(), "the folder gone is not made again");
    // Read-only: passed over too (unless this user writes anywhere).
    std::fs::create_dir_all(&key)?;
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o555))?;
    let probe = key.join("probe");
    if std::fs::write(&probe, b"").is_ok() {
        std::fs::remove_file(&probe)?;
        eprintln!("this user writes in a read-only folder: that part is skipped");
    } else {
        let file = bed.shoot()?;
        assert_eq!(file.parent(), Some(bed.shots().as_path()));
    }
    // Back as it was: the key applies again.
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o755))?;
    let file = bed.shoot()?;
    assert_eq!(file.parent(), Some(key.as_path()));
    Ok(())
}

#[test]
fn the_shutter_plays_unless_the_output_is_muted_or_it_is_off() -> anyhow::Result<()> {
    let mut bed = Bed::start()?;
    // On by default: the default output asked, then the click played.
    bed.shoot()?;
    let heard = bed.sounds_until(2)?;
    assert_eq!(heard[0], "wpctl get-volume @DEFAULT_AUDIO_SINK@");
    assert!(heard[1].starts_with("pw-play"), "{heard:?}");
    // Muted: asked, and nothing played.
    std::fs::write(bed.at("volume"), "Volume: 0.40 [MUTED]\n")?;
    bed.shoot()?;
    bed.sounds_until(3)?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    let heard = bed.sounds();
    assert_eq!(heard.len(), 3, "{heard:?}");
    assert_eq!(heard[2], "wpctl get-volume @DEFAULT_AUDIO_SINK@");
    // Off: not even asked.
    std::fs::write(bed.at("volume"), "Volume: 0.40\n")?;
    bed.capture("sound = false", |c| !c.sound)?;
    bed.shoot()?;
    wait(&mut bed.hp, Duration::from_millis(300))?;
    assert_eq!(bed.sounds().len(), 3, "{:?}", bed.sounds());
    Ok(())
}
