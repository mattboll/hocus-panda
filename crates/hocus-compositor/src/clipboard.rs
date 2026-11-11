//! The clipboard's history (docs/design/bar.md, 5.6): its source, and its
//! paste. The shell keeps the history; the compositor, which sees every
//! selection, hands it what clients copy, and pastes what the user picks.
//!
//! - **What is copied**: when a client takes the clipboard (a Wayland
//!   client's selection, or an X11 client's through XWayland), its text
//!   (`text/plain;charset=utf-8`, else `UTF8_STRING` or `text/plain`) or,
//!   offered no text, its PNG image is read through pipes on a thread of
//!   its own, 8 MB at most and for 3 s at most, so that a client that hangs
//!   or offers too much stalls nothing; then the shell is told, with the
//!   application of the window focused as it was copied (`copied`).
//! - **A capture** (5.8b): the clipboard offering it is the compositor's
//!   own, never read back; the capture's thread makes its line instead
//!   (`capture-copied`, [`capture_line`]), the capture's number with it,
//!   by which a later line (the annotated image, 5.9) replaces its item.
//! - **Never a secret**: a selection marked so (`x-kde-passwordManagerHint`
//!   saying `secret`, `CLIPBOARD_STATE` saying `sensitive`, either unread
//!   counting as so), or copied from a password manager's window
//!   (Bitwarden, KeePassXC, 1Password, and `[clipboard] ignore`'s
//!   applications), is not read, and the shell hears nothing of it; unless
//!   `[clipboard] skip-secrets` is off (6.17).
//! - **Off** (`[clipboard] history = false`, 6.17): nothing is read, a
//!   capture's line is not made either. The shell is told the history's
//!   rules (`clipboard-rules`) as it connects and as the file changes
//!   them, and says how many items it holds (`clipboard-count`), which the
//!   Settings' lines hear; their "Effacer l'historique" asks it to clear.
//! - **Pasting** (`clip paste`): the clipboard takes the item, offered to
//!   Wayland and X11 clients alike; once the window that had the keyboard
//!   holds it again (the history lets it go as it closes), it gets
//!   `Ctrl+V`, or `Ctrl+Shift+V` when it is a terminal: its desktop entry,
//!   found by its application's id in the data directories
//!   ([`crate::Config::applications`]) off the loop, says
//!   `TerminalEmulator`. The keys are the active layout's: the one typing
//!   `v`, wherever it is. Given up after 1 s, as the window may have gone.

use std::{
    io::{PipeReader, Read},
    os::fd::OwnedFd,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

use hocus_link::{CLIP_MAX, Clip, ClipKind, ToShell};
use smithay::{
    backend::input::KeyState,
    input::keyboard::{FilterResult, Keycode, Keysym},
    reexports::calloop::{
        channel::{self, Event, Sender},
        timer::{TimeoutAction, Timer},
    },
    utils::SERIAL_COUNTER,
    wayland::selection::{SelectionTarget, data_device},
};

use crate::{State, focus::KeyboardFocus, model::WindowId, programs};

/// The text types read, best first (UTF8_STRING for X11 clients).
pub(crate) const TEXT: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
pub(crate) const PNG: &str = "image/png";
/// The marks of a secret selection, and what they say then.
const HINTS: [(&str, &str); 2] = [
    ("x-kde-passwordManagerHint", "secret"),
    ("CLIPBOARD_STATE", "sensitive"),
];
/// Password managers, by a part of their application's id (lower case).
const PASSWORD_MANAGERS: [&str; 3] = ["bitwarden", "keepassxc", "1password"];
/// How long a client has to write what it copied.
const READ_WAIT: Duration = Duration::from_secs(3);
/// How long a paste waits for its window to hold the keyboard.
const PASTE_WAIT: Duration = Duration::from_secs(1);

/// The clipboard's history's side of the compositor.
#[derive(Debug, Default)]
pub struct Clipboard {
    /// Each selection a client took, numbered: a reading that comes back
    /// after a later one's is let go, so that the history keeps their order.
    taken: u64,
    told: u64,
    /// The pastes asked, numbered, and the one waiting for its window.
    pastes: u64,
    paste: Option<Paste>,
    done: Option<Sender<Done>>,
    /// The types the client holding the clipboard offered, and where it
    /// is: what the emoji picker's paste reads to give it back (5.7).
    pub(crate) offered: Option<(Vec<String>, Source)>,
    /// How many items the shell's history holds, and how many are pinned,
    /// as it last said (6.17): for the Settings' lines.
    pub(crate) count: Option<(u32, u32)>,
}

/// A paste waiting for its window to hold the keyboard, and for whether it
/// is a terminal.
#[derive(Debug)]
struct Paste {
    number: u64,
    window: WindowId,
    terminal: Option<bool>,
}

/// What a thread found, for the loop.
#[derive(Debug)]
enum Done {
    /// The selection `number`'s line for the shell (`copied`), if it is
    /// kept.
    Read { number: u64, line: Option<String> },
    /// Whether paste `number`'s window is a terminal.
    Terminal { number: u64, terminal: bool },
}

/// Where a selection is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Wayland,
    X11,
}

/// Whether the application `app` is a password manager.
pub fn password_manager(app: &str) -> bool {
    let app = app.to_lowercase();
    PASSWORD_MANAGERS.iter().any(|name| app.contains(name))
}

/// Whether the application `app` is one of `ignored` (`[clipboard]
/// ignore`, by their ids, case aside).
pub fn ignored(app: &str, ignored: &[String]) -> bool {
    !app.is_empty() && ignored.iter().any(|id| id.trim().eq_ignore_ascii_case(app))
}

/// What of a selection offered as `types` is read: the marks it has, and
/// the text, else the image; nothing without either.
pub(crate) fn wanted(types: &[String]) -> Option<(Vec<&'static str>, &'static str, ClipKind)> {
    let offers = |t: &str| types.iter().any(|o| o == t);
    let hints = HINTS
        .iter()
        .map(|(h, _)| *h)
        .filter(|h| offers(h))
        .collect();
    let (main, kind) = match TEXT.iter().find(|t| offers(t)) {
        Some(text) => (*text, ClipKind::Text),
        None if offers(PNG) => (PNG, ClipKind::Png),
        None => return None,
    };
    Some((hints, main, kind))
}

impl State {
    /// A client took the clipboard, offering `types`: what it holds is
    /// read once the selection is in place (smithay keeps it after telling
    /// of it), off the loop.
    pub fn selection_taken(&mut self, types: Vec<String>, from: Source) {
        // What the emoji picker's paste gives back afterwards (5.7).
        self.clipboard.offered = Some((types.clone(), from));
        if !self.shell_linked() {
            return;
        }
        let rules = &self.config.clipboard;
        if !rules.history {
            tracing::debug!("the clipboard's history is off: a copy not read");
            return;
        }
        self.clipboard.taken += 1;
        let number = self.clipboard.taken;
        let app = self
            .model
            .focused()
            .and_then(|id| self.windows.get(&id))
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        let skip = rules.skip_secrets;
        if skip && (password_manager(&app) || ignored(&app, &rules.ignore)) {
            tracing::debug!(app, "a password manager's copy: not kept");
            return;
        }
        let Some((mut hints, main, kind)) = wanted(&types) else {
            tracing::debug!(?types, "a copy of nothing kept");
            return;
        };
        // Secrets kept (6.17): their marks are not read.
        if !skip {
            hints.clear();
        }
        self.loop_handle.insert_idle(move |app_| {
            app_.state
                .read_selection(number, app, (hints, main, kind), from);
        });
    }

    fn read_selection(
        &mut self,
        number: u64,
        app: String,
        (hints, main, kind): (Vec<&'static str>, &'static str, ClipKind),
        from: Source,
    ) {
        // A later copy came meanwhile: it is read instead.
        if number != self.clipboard.taken {
            return;
        }
        let Some(done) = self.clipboard_done() else {
            return;
        };
        let mut marks = Vec::new();
        for hint in hints {
            match self.ask_selection(hint, from) {
                Some(pipe) => marks.push((hint, Some(pipe))),
                None => marks.push((hint, None)),
            }
        }
        let Some(pipe) = self.ask_selection(main, from) else {
            return;
        };
        programs::on_a_thread("read the clipboard".to_owned(), move || {
            let line = read(marks, pipe, kind).map(|clip| ToShell::Copied { app, clip }.line());
            let _ = done.send(Done::Read { number, line });
        });
    }

    /// Asks the selection's client for `mime`: what it writes comes
    /// through the pipe returned.
    pub(crate) fn ask_selection(&mut self, mime: &str, from: Source) -> Option<PipeReader> {
        let (reader, writer) = match std::io::pipe() {
            Ok(pipe) => pipe,
            Err(err) => {
                tracing::warn!(%err, "no pipe to read the clipboard");
                return None;
            }
        };
        let fd = OwnedFd::from(writer);
        let asked = match from {
            Source::Wayland => {
                data_device::request_data_device_client_selection(&self.seat, mime.to_owned(), fd)
                    .map_err(|e| format!("{e:?}"))
            }
            Source::X11 => match self.xwayland.wm.as_mut() {
                Some(xwm) => xwm
                    .send_selection(
                        SelectionTarget::Clipboard,
                        mime.to_owned(),
                        fd,
                        self.loop_handle.clone(),
                    )
                    .map_err(|e| format!("{e:?}")),
                None => Err("no X11".to_owned()),
            },
        };
        match asked {
            Ok(()) => Some(reader),
            Err(err) => {
                tracing::debug!(err, mime, "could not read the clipboard");
                None
            }
        }
    }

    /// Where the threads' findings come back, made once.
    fn clipboard_done(&mut self) -> Option<Sender<Done>> {
        if let Some(done) = &self.clipboard.done {
            return Some(done.clone());
        }
        let (sender, receiver) = channel::channel::<Done>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(done) = event {
                app.state.clipboard_found(done);
            }
        });
        match inserted {
            Ok(_) => {
                self.clipboard.done = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not watch the clipboard's readers");
                None
            }
        }
    }

    fn clipboard_found(&mut self, done: Done) {
        match done {
            Done::Read { number, line } => {
                let Some(line) = line else {
                    return;
                };
                if number <= self.clipboard.told {
                    tracing::debug!(number, "a copy read after a later one: let go");
                    return;
                }
                self.clipboard.told = number;
                tracing::debug!(number, "a copy for the clipboard's history");
                self.tell_shell_line(line);
            }
            Done::Terminal { number, terminal } => {
                if let Some(paste) = self.clipboard.paste.as_mut()
                    && paste.number == number
                {
                    paste.terminal = Some(terminal);
                    self.try_paste();
                }
            }
        }
    }

    /// The shell's history picked `clip`: the clipboard takes it, and with
    /// `paste`, the focused window gets it once it holds the keyboard.
    pub fn clip_asked(&mut self, paste: bool, clip: Clip) {
        let types = clip.kind.types();
        tracing::debug!(
            paste,
            kind = clip.kind.word(),
            "the clipboard takes a history's item"
        );
        self.offer_clipboard(types, clip.data);
        if !paste {
            return;
        }
        let Some(window) = self
            .model
            .focused()
            .filter(|id| self.windows.contains_key(id))
        else {
            tracing::debug!("no window to paste into");
            return;
        };
        let app = self
            .windows
            .get(&window)
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        self.clipboard.pastes += 1;
        let number = self.clipboard.pastes;
        self.clipboard.paste = Some(Paste {
            number,
            window,
            terminal: None,
        });
        let Some(done) = self.clipboard_done() else {
            return;
        };
        let dirs = self.config.applications.clone();
        programs::on_a_thread("a terminal?".to_owned(), move || {
            let terminal = terminal(&app, &dirs);
            let _ = done.send(Done::Terminal { number, terminal });
        });
        let given_up =
            self.loop_handle
                .insert_source(Timer::from_duration(PASTE_WAIT), move |_, _, app| {
                    if app
                        .state
                        .clipboard
                        .paste
                        .as_ref()
                        .is_some_and(|p| p.number == number)
                    {
                        tracing::info!("the window never got the keyboard back: nothing pasted");
                        app.state.clipboard.paste = None;
                    }
                    TimeoutAction::Drop
                });
        if let Err(err) = given_up {
            tracing::warn!(%err.error, "could not time the paste");
        }
    }

    /// The clipboard takes `data`, offered as `types` to Wayland and X11
    /// clients alike: an item of the history, a capture (5.8). The bytes it
    /// holds, by which a capture knows it still holds them (5.9).
    pub fn offer_clipboard(&mut self, types: Vec<String>, data: Vec<u8>) -> Arc<[u8]> {
        let data: Arc<[u8]> = Arc::from(data);
        data_device::set_data_device_selection(
            &self.display_handle,
            &self.seat,
            types.clone(),
            Some(data.clone()),
        );
        // X11 clients see it offered too.
        if let Some(xwm) = self.xwayland.wm.as_mut()
            && let Err(err) = xwm.new_selection(SelectionTarget::Clipboard, Some(types))
        {
            tracing::warn!(?err, "could not offer the clipboard to X11");
        }
        data
    }

    /// A capture's line for the clipboard's history ([`capture_line`]):
    /// told to the shell, which lists the capture first, or replaces the
    /// item the same capture made (its annotated image, 5.9). The
    /// clipboard offering a capture is the compositor's own selection,
    /// never read back as a client's copy is: this line is how the history
    /// hears of it.
    pub fn capture_in_history(&mut self, line: String) {
        if !self.shell_linked() || !self.config.clipboard.history {
            return;
        }
        tracing::debug!("a capture for the clipboard's history");
        self.tell_shell_line(line);
    }

    /// The shell's history holds `items`, `pinned` of them pinned: the
    /// Settings' lines hear it.
    pub fn clipboard_counted(&mut self, items: u32, pinned: u32) {
        if self.clipboard.count == Some((items, pinned)) {
            return;
        }
        self.clipboard.count = Some((items, pinned));
        let line = clipboard_count_line(items, pinned);
        self.displays_send(None, &line);
    }

    /// The Settings' "Effacer l'historique": the shell's history keeps its
    /// pinned items only.
    pub(crate) fn clipboard_clear(&mut self) {
        if !self.shell_linked() {
            tracing::info!("no shell: no clipboard's history to clear");
            return;
        }
        tracing::info!("the Settings clear the clipboard's history");
        self.tell_shell(ToShell::ClipboardClear);
    }

    /// `[clipboard]` changed (6.17): what is read from now on, and the
    /// shell's history told its rules if they changed.
    pub(crate) fn clipboard_config_changed(&mut self, new: &crate::config::Clipboard) -> bool {
        if *new == self.config.clipboard {
            return false;
        }
        let rules = new.rules();
        let told = rules != self.config.clipboard.rules();
        self.config.clipboard = new.clone();
        if told && self.shell_linked() {
            self.tell_shell(ToShell::ClipboardRules(rules));
        }
        true
    }

    /// The keyboard went somewhere: a paste waiting for its window may go
    /// on, after this turn of the loop (the keyboard is busy now).
    pub fn clipboard_focus_changed(&mut self) {
        if self.clipboard.paste.is_some() {
            self.loop_handle.insert_idle(|app| app.state.try_paste());
        }
    }

    /// The paste waiting, if its window holds the keyboard and whether it
    /// is a terminal is known: its keys.
    fn try_paste(&mut self) {
        let Some(paste) = &self.clipboard.paste else {
            return;
        };
        let Some(terminal) = paste.terminal else {
            return;
        };
        let Some(managed) = self.windows.get(&paste.window) else {
            self.clipboard.paste = None;
            return;
        };
        let window = managed.window.clone();
        let target = match window.x11_surface() {
            Some(x11) => KeyboardFocus::X11(x11.clone()),
            None => match window.toplevel() {
                Some(toplevel) => KeyboardFocus::Wayland(toplevel.wl_surface().clone()),
                None => return,
            },
        };
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        if keyboard.current_focus() != Some(target) {
            return;
        }
        self.clipboard.paste = None;
        self.paste_keys(terminal);
    }

    /// `Ctrl+V` to what holds the keyboard, `Ctrl+Shift+V` for a
    /// `terminal`: the active layout's keys, as the keyboard's own.
    pub(crate) fn paste_keys(&mut self, terminal: bool) {
        let Some(keyboard) = self.seat.get_keyboard() else {
            return;
        };
        let v = keyboard.with_xkb_state(self, |context| {
            let xkb = context.xkb().lock().ok()?;
            let layout = xkb.active_layout();
            (8..=255).map(Keycode::new).find(|code| {
                xkb.raw_syms_for_key_in_layout(*code, layout)
                    .contains(&Keysym::v)
            })
        });
        // XKB's codes are evdev's plus 8: the left Ctrl and Shift, and V
        // where no layout moved it.
        let ctrl = Keycode::new(29 + 8);
        let shift = Keycode::new(42 + 8);
        let v = v.unwrap_or(Keycode::new(47 + 8));
        let mut keys = vec![(ctrl, KeyState::Pressed)];
        if terminal {
            keys.push((shift, KeyState::Pressed));
        }
        keys.push((v, KeyState::Pressed));
        keys.push((v, KeyState::Released));
        if terminal {
            keys.push((shift, KeyState::Released));
        }
        keys.push((ctrl, KeyState::Released));
        tracing::debug!(terminal, "pasting");
        let time = u32::try_from(self.now().as_millis() % u128::from(u32::MAX)).unwrap_or(0);
        for (code, state) in keys {
            keyboard.input::<(), _>(
                self,
                code,
                state,
                SERIAL_COUNTER.next_serial(),
                time,
                |_, _, _| FilterResult::Forward,
            );
        }
    }

    /// `Super+V`: the history opens on the bar of the focused window's
    /// screen, else the pointer's; open, it closes.
    pub fn toggle_clipboard(&mut self) {
        let Some(output) = self.bubbles_screen() else {
            return;
        };
        if !self.shell_linked() {
            tracing::info!("no shell: no clipboard's history");
            return;
        }
        self.tell_shell(ToShell::Clipboard {
            output: output.name(),
        });
    }
}

/// What the Settings' lines are told of the history's items.
pub(crate) fn clipboard_count_line(items: u32, pinned: u32) -> String {
    serde_json::json!({ "clipboard": { "items": items, "pinned": pinned } }).to_string()
}

/// The line telling the clipboard's history of capture `capture` (the
/// session's count of captures, 5.8b), its `png`: made off the loop, as a
/// copy's is, its base64 being as long as the image. None for an image
/// larger than the history keeps ([`CLIP_MAX`]). The history lists it as
/// copied from [`hocus_link::CAPTURE_APP`].
pub fn capture_line(capture: u64, png: &[u8]) -> Option<String> {
    if png.len() > CLIP_MAX {
        tracing::info!(
            bytes = png.len(),
            "a capture too large for the clipboard's history"
        );
        return None;
    }
    let clip = Clip {
        kind: ClipKind::Png,
        data: png.to_vec(),
    };
    Some(ToShell::CaptureCopied { capture, clip }.line())
}

/// What a selection holds, read from `pipe` as `kind`, unless one of its
/// `marks` says it is a secret (or could not be read: it may be one).
fn read(
    marks: Vec<(&'static str, Option<PipeReader>)>,
    pipe: PipeReader,
    kind: ClipKind,
) -> Option<Clip> {
    for (mark, pipe) in marks {
        let says = HINTS.iter().find(|(h, _)| *h == mark).map(|(_, s)| *s);
        let read = pipe.and_then(|p| read_all(&p, 64, Instant::now() + READ_WAIT).ok());
        let secret = match read {
            Some(said) => String::from_utf8_lossy(&said).trim() == says.unwrap_or_default(),
            None => true,
        };
        if secret {
            tracing::debug!(mark, "a secret copy: not kept");
            return None;
        }
    }
    let data = match read_all(&pipe, CLIP_MAX, Instant::now() + READ_WAIT) {
        Ok(data) => data,
        Err(why) => {
            tracing::debug!(why, "a copy not kept");
            return None;
        }
    };
    if data.is_empty() {
        return None;
    }
    if kind == ClipKind::Text && std::str::from_utf8(&data).is_err() {
        tracing::debug!("a copy's text is not UTF-8: not kept");
        return None;
    }
    Some(Clip { kind, data })
}

/// What comes through `pipe` until it closes, `most` bytes at most, until
/// `until` at most.
pub(crate) fn read_all(
    pipe: &PipeReader,
    most: usize,
    until: Instant,
) -> Result<Vec<u8>, &'static str> {
    use smithay::reexports::rustix::{
        event::{PollFd, PollFlags, Timespec, poll},
        io::Errno,
    };
    let mut data = Vec::new();
    let mut chunk = vec![0u8; 64 * 1024];
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("its client did not write it in time");
        }
        let timeout = Timespec::try_from(left).map_err(|_| "a wait")?;
        let mut fds = [PollFd::new(pipe, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(Errno::INTR) => continue,
            Ok(_) => {}
            Err(_) => return Err("could not wait for it"),
        }
        match (&*pipe).read(&mut chunk) {
            Ok(0) => return Ok(data),
            Ok(n) => {
                if data.len() + n > most {
                    return Err("too large");
                }
                data.extend_from_slice(chunk.get(..n).unwrap_or(&chunk));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("could not read it"),
        }
    }
}

/// Whether the application `app`'s desktop entry, in one of `dirs` (each
/// an `applications` directory), says it is a terminal.
pub fn terminal(app: &str, dirs: &[PathBuf]) -> bool {
    if app.is_empty() || app.contains('/') {
        return false;
    }
    let mut names = vec![app.to_owned()];
    let lower = app.to_lowercase();
    if lower != app {
        names.push(lower);
    }
    dirs.iter()
        .flat_map(|dir| names.iter().map(move |n| dir.join(format!("{n}.desktop"))))
        .find_map(|file| std::fs::read_to_string(file).ok())
        .is_some_and(|text| says_terminal(&text))
}

/// Whether a desktop entry's `Categories` names `TerminalEmulator`.
fn says_terminal(entry: &str) -> bool {
    let mut inside = false;
    for line in entry.lines().map(str::trim) {
        if line.starts_with('[') {
            inside = line == "[Desktop Entry]";
            continue;
        }
        if inside
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "Categories"
        {
            return value.split(';').any(|c| c.trim() == "TerminalEmulator");
        }
    }
    false
}

/// The `applications` directories of the XDG data directories, the user's
/// first.
pub fn application_dirs() -> Vec<PathBuf> {
    let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
    let home = var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|h| Path::new(&h).join(".local/share")));
    let dirs = var("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    home.into_iter()
        .chain(std::env::split_paths(&dirs))
        .filter(|d| d.is_absolute())
        .map(|d| d.join("applications"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types(list: &[&str]) -> Vec<String> {
        list.iter().map(|t| (*t).to_owned()).collect()
    }

    #[test]
    fn the_text_is_read_else_the_image_with_the_marks_of_a_secret() {
        let (hints, main, kind) = wanted(&types(&[
            "image/png",
            "text/plain",
            "text/plain;charset=utf-8",
            "x-kde-passwordManagerHint",
        ]))
        .expect("kept");
        assert_eq!(hints, ["x-kde-passwordManagerHint"]);
        assert_eq!((main, kind), ("text/plain;charset=utf-8", ClipKind::Text));
        let (hints, main, kind) = wanted(&types(&["image/png", "CLIPBOARD_STATE"])).expect("kept");
        assert_eq!(hints, ["CLIPBOARD_STATE"]);
        assert_eq!((main, kind), ("image/png", ClipKind::Png));
        assert!(wanted(&types(&["text/html", "image/jpeg"])).is_none());
    }

    #[test]
    fn ignored_applications_are_known_by_their_whole_ids() {
        let list = vec!["com.protonpass.ProtonPass".to_owned()];
        assert!(ignored("com.protonpass.ProtonPass", &list));
        assert!(ignored("com.protonpass.protonpass", &list));
        assert!(!ignored("com.protonpass", &list));
        assert!(!ignored("", &list));
    }

    #[test]
    fn password_managers_are_known_by_their_ids() {
        for app in [
            "Bitwarden",
            "org.keepassxc.KeePassXC",
            "1Password",
            "com.bitwarden.desktop",
        ] {
            assert!(password_manager(app), "{app}");
        }
        for app in ["firefox", "kitty", ""] {
            assert!(!password_manager(app), "{app}");
        }
    }

    #[test]
    fn a_secret_mark_or_one_unread_keeps_nothing() {
        let piped = |text: &str| {
            let (reader, mut writer) = std::io::pipe().expect("a pipe");
            std::io::Write::write_all(&mut writer, text.as_bytes()).expect("written");
            drop(writer);
            reader
        };
        let clip = read(Vec::new(), piped("ls -la"), ClipKind::Text).expect("kept");
        assert_eq!(clip.data, b"ls -la");
        let marked = vec![("x-kde-passwordManagerHint", Some(piped("secret")))];
        assert!(read(marked, piped("hunter2"), ClipKind::Text).is_none());
        let marked = vec![("CLIPBOARD_STATE", Some(piped("sensitive\n")))];
        assert!(read(marked, piped("hunter2"), ClipKind::Text).is_none());
        let unread = vec![("CLIPBOARD_STATE", None)];
        assert!(read(unread, piped("hunter2"), ClipKind::Text).is_none());
        // Marked, but not a secret.
        let marked = vec![("CLIPBOARD_STATE", Some(piped("data")))];
        assert!(read(marked, piped("ls"), ClipKind::Text).is_some());
        // Neither empty nor broken text.
        assert!(read(Vec::new(), piped(""), ClipKind::Text).is_none());
    }

    #[test]
    fn a_terminal_says_so_in_its_desktop_entry() {
        let dir = tempfile::tempdir().expect("a directory");
        let apps = dir.path().join("applications");
        std::fs::create_dir_all(&apps).expect("made");
        std::fs::write(
            apps.join("foot.desktop"),
            "[Desktop Entry]\nName=Foot\nCategories=System;TerminalEmulator;\n",
        )
        .expect("written");
        std::fs::write(
            apps.join("org.mozilla.firefox.desktop"),
            "[Desktop Entry]\nName=Firefox\nCategories=Network;WebBrowser;\n\
             [Desktop Action new]\nCategories=TerminalEmulator\n",
        )
        .expect("written");
        let dirs = vec![apps];
        assert!(terminal("foot", &dirs));
        // X11's classes are capitalised.
        assert!(terminal("Foot", &dirs));
        assert!(!terminal("org.mozilla.firefox", &dirs));
        assert!(!terminal("kitty", &dirs));
        assert!(!terminal("../foot", &dirs));
        assert!(!terminal("", &dirs));
    }
}
