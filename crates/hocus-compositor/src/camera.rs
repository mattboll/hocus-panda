//! The Hocuspanda camera's program (#619, lot 2, docs/design/camera.md):
//! `hocus-camera`, started by the compositor, publishes the "Hocuspanda"
//! source on PipeWire and reads the real camera only while it is read.
//!
//! - **Its life**: started from the loop's turn once its program is known
//!   (the session's, beside the compositor; a test's), on a thread of its
//!   own which reads what it says and hands it to the loop; started again
//!   when it ends, after 2 s, then twice as long each time up to a minute.
//!   It ends when its input does: when the compositor ends, or lets it go.
//! - **What it is told** ([`CameraLook`]): Lumière on or not, and the
//!   banner (`[camera] banner`, its name, the current project desktop's
//!   name in the desktop's language): at once as it starts, then as that
//!   changes, looked at once a turn of the loop.
//! - **What it says** ([`FromCamera`]): read or not, the camera it reads,
//!   and the light on the face while it is read, which the screen light's
//!   `auto` strength follows ([`State::screen_light_glow`]): below
//!   [`DARK`] a step stronger, above [`BRIGHT`] a step softer, a step at
//!   most every [`STEP`], [`AUTO_STEPS`] from soft to strong. The light it
//!   adds is in what it measures: between the two, nothing moves.
//! - **The shell** is told what the camera's window shows
//!   ([`CameraShown`]) as it connects and as it changes.

use std::{
    io::{BufRead, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use hocus_link::{
    CameraLook, CameraShown, FromCamera, ToShell,
    camera::{Away, AwayWhy, Banner, Scene},
};
use hocus_words::compositor::camera as words;
use smithay::reexports::calloop::channel::{self, Event, Sender};

use crate::{
    State,
    config::{AUTO_STEPS, Strength},
    programs,
};

/// The face is lit too little below this, too much above [`BRIGHT`].
pub const DARK: f32 = 0.36;
pub const BRIGHT: f32 = 0.50;
/// The screen light's `auto` takes a step at most this often.
pub const STEP: Duration = Duration::from_millis(600);
/// A cut-out is the camera's frame at most, and room for its shadow.
const MOST_CUTOUT: usize = 4 * 2048 * 2048;
/// The program is started again after this, doubled each time.
const AGAIN: Duration = Duration::from_secs(2);
const AGAIN_MOST: Duration = Duration::from_secs(60);

/// What the program's thread tells the loop.
#[derive(Debug)]
pub enum FromProgram {
    Started {
        pid: u32,
    },
    Said(FromCamera),
    /// The person cut out, for the presenter.
    Cutout(Silhouette),
    Ended,
}

/// The person cut out (Présentateur): B, G, R, A premultiplied, `width`
/// × `height`.
#[derive(Debug, Clone)]
pub struct Silhouette {
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
}

/// The Hocuspanda camera, as the loop knows it.
#[derive(Debug, Default)]
pub struct HocusCamera {
    /// Its input's writer, while it runs.
    to: Option<mpsc::Sender<String>>,
    back: Option<Sender<FromProgram>>,
    /// The look it was told last.
    told: Option<CameraLook>,
    /// When it may start (again), on the monotonic clock.
    start_at: Duration,
    again: Duration,
    /// Its source is on the graph.
    pub ready: bool,
    /// An application reads it.
    pub read: bool,
    pub source: Option<String>,
    /// The light on the face, while it is read.
    pub face: Option<f32>,
    /// The screen light's `auto` step, from 0 (soft) to [`AUTO_STEPS`].
    pub level: u8,
    stepped: Duration,
    /// What the shell was told last.
    shown: Option<CameraShown>,
    /// « Je reviens » shows in its pictures, and why.
    pub away: Option<AwayWhy>,
    /// The person cut out last, while the presenter is wanted, and how
    /// many came (tests).
    pub silhouette: Option<crate::presenter::Shown>,
    pub silhouettes: u64,
}

impl State {
    /// Once a turn of the loop: the program started when it should, told
    /// its look when it changed; the screen light's `auto` steps; the
    /// shell told what the window shows when it changed.
    pub(crate) fn tick_camera(&mut self, now: Duration) {
        if self.camera.to.is_none()
            && self.config.camera_program.is_some()
            && now >= self.camera.start_at
        {
            self.camera_start(now);
        }
        if self.camera.to.is_some() {
            let look = self.camera_look();
            if self.camera.told.as_ref() != Some(&look) {
                self.camera_tell(&look);
                self.camera.told = Some(look);
            }
        }
        self.camera_auto(now);
        if self.shell_linked() {
            let shown = self.camera_shown();
            if self.camera.shown.as_ref() != Some(&shown) {
                self.camera.shown = Some(shown.clone());
                self.tell_shell(ToShell::Camera(shown));
            }
        }
    }

    /// The shell connected: it is told again.
    pub(crate) fn camera_shell_connected(&mut self) {
        self.camera.shown = None;
    }

    fn camera_shown(&self) -> CameraShown {
        let c = &self.config.camera;
        CameraShown {
            lighting: c.lighting,
            banner: c.banner,
            screen_light: c.screen_light,
            strength: c.screen_light_strength.word().to_owned(),
            ready: self.camera.ready,
            read: self.camera.read,
            source: self.camera.source.clone(),
            background: c.background.word().to_owned(),
            away: c.away,
            presenter: c.presenter,
            hush: self.camera_hush(),
        }
    }

    /// The microphone held muted: the screen locked during a call (the
    /// Hocuspanda camera read), « Je reviens » on.
    fn camera_hush(&self) -> bool {
        self.config.camera.away && self.camera.read && self.locked()
    }

    /// Whether the presenter is wanted now: on, and the desktop sharing a
    /// screen or a window.
    pub(crate) fn presenter_wanted(&self) -> bool {
        self.config.camera.presenter && self.share.shares.values().any(|s| s.node.is_some())
    }

    /// The living wallpaper under « Je reviens »'s card: Fond's when it
    /// is one, else the desktop's own (its koi, then its aurora), else
    /// the aurora.
    fn away_scene(&self) -> Scene {
        if let hocus_link::camera::Backdrop::Scene(scene) = self.config.camera.background.backdrop()
        {
            return scene;
        }
        if self.config.effects.koi {
            Scene::Koi
        } else {
            Scene::Aurora
        }
    }

    /// How the camera's frames look now.
    fn camera_look(&self) -> CameraLook {
        let c = &self.config.camera;
        let lang = self.config.lang;
        let banner = c.banner.then(|| Banner {
            name: c.banner_name.trim().to_owned(),
            project: c
                .banner_project
                .then(|| self.current_project())
                .flatten()
                .map(|name| words::project(lang, name)),
        });
        CameraLook {
            lighting: c.lighting,
            banner,
            backdrop: c.background.backdrop(),
            away: c.away.then(|| Away {
                scene: self.away_scene(),
                lang: lang.code().to_owned(),
            }),
            locked: self.locked(),
            presenter: self.presenter_wanted(),
        }
    }

    /// The project open on the focused screen's current desktop.
    fn current_project(&self) -> Option<String> {
        let output = self.bubbles_screen()?;
        let desktop = self.strip(&output).current();
        self.project_on(&output, desktop).map(|p| p.name.clone())
    }

    fn camera_tell(&mut self, look: &CameraLook) {
        let Some(to) = &self.camera.to else {
            return;
        };
        if to.send(look.line()).is_err() {
            self.camera.to = None;
        }
    }

    fn camera_start(&mut self, now: Duration) {
        let Some(program) = self.config.camera_program.clone() else {
            return;
        };
        let Some(back) = self.camera_back() else {
            return;
        };
        self.camera.again = if self.camera.again.is_zero() {
            AGAIN
        } else {
            (self.camera.again * 2).min(AGAIN_MOST)
        };
        // Not again before its delay, whatever happens.
        self.camera.start_at = now + self.camera.again;
        let (to, lines) = mpsc::channel::<String>();
        self.camera.to = Some(to);
        self.camera.told = None;
        let remote = self.config.pipewire_remote.clone();
        let device = self.config.camera_device.clone();
        let video4linux = self.config.camera_video4linux.clone();
        programs::on_a_thread("hocus-camera".to_owned(), move || {
            run(&program, remote, &device, video4linux, lines, &back);
        });
    }

    fn camera_back(&mut self) -> Option<Sender<FromProgram>> {
        if let Some(back) = &self.camera.back {
            return Some(back.clone());
        }
        let (sender, receiver) = channel::channel::<FromProgram>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(said) = event {
                app.state.camera_heard(said);
            }
        });
        match inserted {
            Ok(_) => {
                self.camera.back = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not listen to hocus-camera");
                None
            }
        }
    }

    fn camera_heard(&mut self, from: FromProgram) {
        match from {
            FromProgram::Started { pid } => tracing::info!(pid, "hocus-camera runs"),
            FromProgram::Said(said) => {
                tracing::debug!(?said, "hocus-camera says");
                match said {
                    FromCamera::Ready { node } => {
                        if !self.camera.ready {
                            tracing::info!(node, "the Hocuspanda camera is on the graph");
                        }
                        self.camera.ready = true;
                        // Running well: a next ending starts it again soon.
                        self.camera.again = Duration::ZERO;
                    }
                    FromCamera::Read(read) => {
                        tracing::info!(read, "the Hocuspanda camera");
                        self.camera.read = read;
                        if !read {
                            self.camera.face = None;
                        }
                    }
                    FromCamera::Source(source) => self.camera.source = source,
                    FromCamera::Face(face) => {
                        if self.camera.read {
                            self.camera.face = Some(face);
                        }
                    }
                    FromCamera::Device(device) => {
                        tracing::info!(?device, "the Hocuspanda camera's frames are made");
                    }
                    FromCamera::Away(why) => {
                        tracing::info!(?why, "the Hocuspanda camera's « Je reviens »");
                        self.camera.away = why;
                    }
                    // Its pixels come as `Cutout`.
                    FromCamera::Presenter { .. } => {}
                }
            }
            FromProgram::Cutout(cut) => {
                self.camera.silhouettes += 1;
                if self.presenter_wanted() {
                    let shown =
                        crate::presenter::Shown::new(cut).after(self.camera.silhouette.as_ref());
                    self.camera.silhouette = Some(shown);
                    self.share.streams_due = true;
                } else {
                    self.camera.silhouette = None;
                }
            }
            FromProgram::Ended => {
                tracing::info!("hocus-camera ended");
                self.camera.to = None;
                self.camera.ready = false;
                self.camera.read = false;
                self.camera.face = None;
                self.camera.source = None;
                self.camera.away = None;
                self.camera.silhouette = None;
            }
        }
    }

    /// The screen light's `auto` follows the light on the face; back to
    /// soft when nothing measures it.
    fn camera_auto(&mut self, now: Duration) {
        let auto = self.config.camera.screen_light_strength == Strength::Auto;
        let level = self.camera.level;
        let next = match self.camera.face.filter(|_| auto) {
            None => 0,
            Some(face) if now.saturating_sub(self.camera.stepped) < STEP => {
                let _ = face;
                level
            }
            Some(face) if face < DARK => (level + 1).min(AUTO_STEPS),
            Some(face) if face > BRIGHT => level.saturating_sub(1),
            Some(_) => level,
        };
        if next != level {
            tracing::debug!(
                face = self.camera.face,
                level = next,
                "the screen light follows the face"
            );
            self.camera.level = next;
            self.camera.stepped = now;
            self.redraw = true;
        }
    }

    /// The screen light's glow now: its strength's, or `auto`'s step.
    pub(crate) fn screen_light_glow(&self) -> (f64, f64) {
        match self.config.camera.screen_light_strength {
            Strength::Auto => Strength::auto_glow(self.camera.level),
            fixed => fixed.glow(),
        }
    }

    /// Tests: « Je reviens » in the camera's pictures, and how many
    /// cut-outs of the person came.
    pub(crate) fn camera_lot3(&self) -> (Option<AwayWhy>, u64) {
        (self.camera.away, self.camera.silhouettes)
    }

    /// Tests: whether the Hocuspanda camera is read, the light on the face,
    /// and the screen light's `auto` step.
    pub(crate) fn camera_info(&self) -> (bool, Option<f32>, u8) {
        (self.camera.read, self.camera.face, self.camera.level)
    }
}

/// The program, on its thread: started, its output read and told, until
/// it ends; its input written from `lines` on a thread of its own.
fn run(
    program: &std::path::Path,
    remote: Option<PathBuf>,
    device: &str,
    video4linux: Option<PathBuf>,
    lines: mpsc::Receiver<String>,
    back: &Sender<FromProgram>,
) {
    let mut command = Command::new(program);
    command
        .arg("--device")
        .arg(if device.is_empty() { "auto" } else { device })
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(remote) = remote {
        command.arg("--remote").arg(remote);
    }
    // On the CPU, everything: its living wallpapers on the CPU's Vulkan
    // alone (lavapipe), the loader shown no other, so that no GPU is
    // opened.
    if device == "cpu" {
        match lavapipe() {
            Some(lvp) => {
                command
                    .env("VK_DRIVER_FILES", &lvp)
                    .env("VK_ICD_FILENAMES", &lvp)
                    .env("WGPU_BACKEND", "vulkan");
            }
            None => {
                command.env("WGPU_BACKEND", "none");
            }
        }
    }
    if let Some(dir) = video4linux {
        command.env("HOCUS_VIDEO4LINUX", dir);
    }
    programs::default_signals(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            tracing::warn!(%err, program = %program.display(), "hocus-camera does not start");
            let _ = back.send(FromProgram::Ended);
            return;
        }
    };
    let _ = back.send(FromProgram::Started { pid: child.id() });
    let input = child.stdin.take();
    let output = child.stdout.take();
    std::thread::scope(|scope| {
        if let Some(mut input) = input {
            scope.spawn(move || {
                for line in lines {
                    if writeln!(input, "{line}")
                        .and_then(|()| input.flush())
                        .is_err()
                    {
                        break;
                    }
                }
                // Dropped: its input ends, and so does it.
            });
        }
        if let Some(output) = output {
            let mut output = std::io::BufReader::new(output);
            let mut line = String::new();
            loop {
                line.clear();
                match output.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                match FromCamera::read(&line) {
                    // The cut-out's pixels follow its line, raw.
                    Some(FromCamera::Presenter { width, height }) => {
                        let len = width as usize * height as usize * 4;
                        if len > MOST_CUTOUT {
                            tracing::warn!(
                                width,
                                height,
                                "a cut-out too large: hocus-camera let go"
                            );
                            break;
                        }
                        let mut bgra = vec![0u8; len];
                        if std::io::Read::read_exact(&mut output, &mut bgra).is_err() {
                            break;
                        }
                        let _ = back.send(FromProgram::Cutout(Silhouette {
                            width,
                            height,
                            bgra,
                        }));
                    }
                    Some(said) => {
                        let _ = back.send(FromProgram::Said(said));
                    }
                    None => {}
                }
            }
        }
        let _ = child.wait();
        let _ = back.send(FromProgram::Ended);
    });
}

/// lavapipe's description for Vulkan's loader, if it is installed.
fn lavapipe() -> Option<PathBuf> {
    ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"]
        .iter()
        .flat_map(std::fs::read_dir)
        .flatten()
        .flatten()
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("lvp_icd"))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_words_say_the_project() {
        assert_eq!(
            words::project(hocus_words::Lang::French, "hocus-panda".to_owned()),
            "projet hocus-panda"
        );
    }
}
