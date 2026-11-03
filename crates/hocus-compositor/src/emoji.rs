//! The emoji picker (5.7, docs/design/emoji.md): the shell's layer surface,
//! namespace [`EMOJI_NAMESPACE`], and what the compositor does for it.
//!
//! - **Where**: `Super+.` (`[shortcuts] emoji`) tells the shell the screen
//!   of the focused window and where the picker goes there: under the
//!   caret its text input reports (text-input-v3's cursor rectangle,
//!   [`crate::text_input`]), else centred on the window, else on the
//!   screen under the pointer (`emoji`). The shell opens it, or closes it.
//!   It looks and moves as a popover ([`crate::popover`]): the glass behind
//!   it, its shadow, and it fades in coming down a little.
//! - **Inserting** (`emoji insert TEXT`, `emoji keep TEXT`): the emoji goes
//!   into the focused window once it holds the keyboard again (the shell
//!   lets it go first). When that window took text through a text input
//!   as the keyboard left it, the compositor waits for the application to
//!   enable it again and commits the emoji there (`commit_string`).
//!   Otherwise, or when no text input comes back within a second, it
//!   pastes: what the clipboard holds is read first (its text, else its
//!   image, as the clipboard's history reads it, off the loop), the emoji
//!   is offered instead, the window gets `Ctrl+V` (`Ctrl+Shift+V` in a
//!   terminal), and once it has read the emoji (or a second after, when it
//!   does not), the clipboard is given back what it held. Kept open
//!   (`keep`), the picker is told when the emoji is in
//!   ([`ToShell::EmojiInserted`]) and takes the keyboard back.

use std::{
    sync::Arc,
    time::{Duration, Instant},
};

use hocus_link::{Clip, ClipKind, EMOJI_NAMESPACE, EmojiAt, Rect, ToShell};
use smithay::{
    reexports::{
        calloop::{
            channel::{self, Event, Sender},
            timer::{TimeoutAction, Timer},
        },
        wayland_server::protocol::wl_surface::WlSurface,
    },
    wayland::selection::{
        SelectionTarget,
        data_device::{
            clear_data_device_selection, current_data_device_selection_userdata,
            set_data_device_selection,
        },
    },
};

use crate::{
    State,
    clipboard::{self, Source},
    focus::KeyboardFocus,
    model::WindowId,
    programs,
    text_input::TextInputs,
};

/// How long an insertion waits for its window, its text input and its
/// paste to be read, at each step.
const WAIT: Duration = Duration::from_secs(1);
/// What is read of the clipboard to give it back: as the history reads.
const READ_MOST: usize = hocus_link::CLIP_MAX;
const READ_WAIT: Duration = Duration::from_secs(1);

/// The text inputs, and the emoji on its way.
#[derive(Debug, Default)]
pub struct Emoji {
    pub inputs: TextInputs,
    insert: Option<Insert>,
    inserts: u64,
    done: Option<Sender<Done>>,
}

impl Emoji {
    pub fn new(dh: &smithay::reexports::wayland_server::DisplayHandle) -> Self {
        crate::text_input::global(dh);
        Self::default()
    }
}

#[derive(Debug)]
struct Insert {
    number: u64,
    text: String,
    keep: bool,
    window: WindowId,
    how: How,
}

#[derive(Debug)]
enum How {
    /// Waiting for the window to hold the keyboard with its text input
    /// enabled again.
    TextInput,
    /// Waiting for what the clipboard held, and whether the window is a
    /// terminal; then for the window to hold the keyboard.
    Paste {
        saved: Option<Saved>,
        terminal: Option<bool>,
    },
    /// Pasted: waiting for the window to read it.
    Pasted { saved: Saved, offered: Arc<[u8]> },
}

/// What the clipboard held before the paste.
#[derive(Debug)]
enum Saved {
    Nothing,
    Clip(Clip),
    /// It could not be read: it is not given back.
    Unread,
}

/// What a thread found, for the loop.
#[derive(Debug)]
enum Done {
    Saved { number: u64, saved: Saved },
    Terminal { number: u64, terminal: bool },
}

impl State {
    /// `Super+.`: the picker's screen and place, for the shell.
    pub fn toggle_emoji(&mut self) {
        if !self.shell_linked() {
            tracing::info!("no shell: no emoji picker");
            return;
        }
        let focused = self
            .model
            .focused()
            .filter(|id| self.windows.contains_key(id) && !self.model.is_minimized(*id));
        let Some(output) = focused
            .and_then(|id| self.output_of(id))
            .or_else(|| self.output_under_pointer())
        else {
            return;
        };
        let Some(screen) = self.space.output_geometry(&output) else {
            return;
        };
        let local = |r: smithay::utils::Rectangle<i32, smithay::utils::Logical>| Rect {
            x: r.loc.x - screen.loc.x,
            y: r.loc.y - screen.loc.y,
            width: r.size.w,
            height: r.size.h,
        };
        let at = match focused {
            Some(id) => match self.caret_of(id) {
                Some(caret) => EmojiAt::Caret(local(caret)),
                None => match self.space.element_geometry(&self.windows[&id].window) {
                    Some(rect) => EmojiAt::Window(local(rect)),
                    None => EmojiAt::Screen,
                },
            },
            None => EmojiAt::Screen,
        };
        tracing::debug!(?at, output = output.name(), "the emoji picker");
        let name = output.name();
        let scale = self.bar_scale(&name);
        self.tell_shell(ToShell::Emoji {
            output: name,
            scale,
            width: screen.size.w,
            height: screen.size.h,
            at,
        });
    }

    /// The caret of window `id`'s text input, global, if it says where it
    /// is.
    fn caret_of(
        &self,
        id: WindowId,
    ) -> Option<smithay::utils::Rectangle<i32, smithay::utils::Logical>> {
        let window = &self.windows.get(&id)?.window;
        let surface = window.toplevel()?.wl_surface().clone();
        let mut caret = self.emoji.inputs.caret(&surface)?;
        // The surface's origin: its geometry's corner less what of it is
        // outside (a client-side shadow).
        let origin = self.space.element_location(window)? - window.geometry().loc;
        caret.loc += origin;
        // An empty caret still has a place.
        caret.size.w = caret.size.w.max(1);
        caret.size.h = caret.size.h.max(1);
        Some(caret)
    }

    /// The window's surface, for its text input (none for an X11 window).
    fn text_surface(&self, id: WindowId) -> Option<WlSurface> {
        let window = &self.windows.get(&id)?.window;
        window.toplevel().map(|t| t.wl_surface().clone())
    }

    /// Whether window `id` holds the keyboard.
    fn holds_keyboard(&self, id: WindowId) -> bool {
        let Some(managed) = self.windows.get(&id) else {
            return false;
        };
        let target = match managed.window.x11_surface() {
            Some(x11) => KeyboardFocus::X11(x11.clone()),
            None => match managed.window.toplevel() {
                Some(toplevel) => KeyboardFocus::Wayland(toplevel.wl_surface().clone()),
                None => return false,
            },
        };
        self.seat
            .get_keyboard()
            .and_then(|k| k.current_focus())
            .is_some_and(|f| f == target)
    }

    /// The shell chose `text` (the picker, or the launcher's emoji row):
    /// it goes into the focused window.
    pub fn emoji_asked(&mut self, keep: bool, text: String) {
        let window = self
            .model
            .focused()
            .filter(|id| self.windows.contains_key(id));
        let Some(window) = window else {
            tracing::info!("no window to insert an emoji into");
            if keep {
                self.tell_shell(ToShell::EmojiInserted);
            }
            return;
        };
        // One at a time: a later one replaces it.
        self.emoji_finish();
        self.emoji.inserts += 1;
        let number = self.emoji.inserts;
        let typed = self
            .text_surface(window)
            .is_some_and(|s| self.emoji.inputs.takes_text(&s));
        tracing::debug!(keep, typed, "an emoji to insert");
        self.emoji.insert = Some(Insert {
            number,
            text,
            keep,
            window,
            how: How::TextInput,
        });
        if typed {
            self.emoji_wait(number);
            self.emoji_try();
        } else {
            self.emoji_paste(number);
        }
    }

    /// Insertion `number` waits a second at most for its next step.
    fn emoji_wait(&mut self, number: u64) {
        let timer = Timer::from_duration(WAIT);
        let given = self.loop_handle.insert_source(timer, move |_, _, app| {
            app.state.emoji_waited(number);
            TimeoutAction::Drop
        });
        if let Err(err) = given {
            tracing::warn!(%err.error, "could not time the emoji's insertion");
        }
    }

    /// A second went by for insertion `number`: its text input never came
    /// back (it is pasted instead), its window never held the keyboard (it
    /// is given up), or its paste was never read (the clipboard goes back).
    fn emoji_waited(&mut self, number: u64) {
        let Some(insert) = self.emoji.insert.as_ref().filter(|i| i.number == number) else {
            return;
        };
        match insert.how {
            How::TextInput => {
                tracing::debug!("no text input came back: the emoji is pasted");
                self.emoji_paste(number);
            }
            How::Paste { .. } => {
                tracing::info!("the window never got the keyboard back: no emoji inserted");
                self.emoji_finish();
            }
            How::Pasted { .. } => self.emoji_give_back(),
        }
    }

    /// Insertion `number` goes by the clipboard: what it holds is read off
    /// the loop, and whether the window is a terminal.
    fn emoji_paste(&mut self, number: u64) {
        let Some(done) = self.emoji_done() else {
            return;
        };
        let Some(insert) = self.emoji.insert.as_mut().filter(|i| i.number == number) else {
            return;
        };
        insert.how = How::Paste {
            saved: None,
            terminal: None,
        };
        let window = insert.window;
        self.emoji_wait(number);
        let app = self
            .windows
            .get(&window)
            .and_then(|m| crate::windows::app_id(&m.window))
            .unwrap_or_default();
        let dirs = self.config.applications.clone();
        let sent = done.clone();
        programs::on_a_thread("a terminal?".to_owned(), move || {
            let terminal = clipboard::terminal(&app, &dirs);
            let _ = sent.send(Done::Terminal { number, terminal });
        });
        self.save_selection(number, done);
    }

    /// What the clipboard holds, read for insertion `number`: the
    /// compositor's own at once, a client's on a thread.
    fn save_selection(&mut self, number: u64, done: Sender<Done>) {
        let own = current_data_device_selection_userdata(&self.seat).map(|d| d.clone());
        let read = match own {
            // The compositor's own: its bytes, text or image.
            Some(Some(bytes)) => {
                let kind = if std::str::from_utf8(&bytes).is_ok() {
                    ClipKind::Text
                } else {
                    ClipKind::Png
                };
                let saved = Saved::Clip(Clip {
                    kind,
                    data: bytes.to_vec(),
                });
                let _ = done.send(Done::Saved { number, saved });
                return;
            }
            // An X11 client's, through XWayland.
            Some(None) => self
                .clipboard
                .offered
                .clone()
                .filter(|(_, from)| *from == Source::X11),
            // A Wayland client's, or none.
            None => self
                .clipboard
                .offered
                .clone()
                .filter(|(_, from)| *from == Source::Wayland),
        };
        let wanted = read
            .as_ref()
            .and_then(|(types, from)| Some((clipboard::wanted(types)?, *from)));
        let Some(((_, mime, kind), from)) = wanted else {
            let saved = if read.is_none() && own.is_none() {
                Saved::Nothing
            } else {
                Saved::Unread
            };
            let _ = done.send(Done::Saved { number, saved });
            return;
        };
        let Some(pipe) = self.ask_selection(mime, from) else {
            // No client holds it any more.
            let _ = done.send(Done::Saved {
                number,
                saved: Saved::Nothing,
            });
            return;
        };
        programs::on_a_thread("save the clipboard".to_owned(), move || {
            let saved = match clipboard::read_all(&pipe, READ_MOST, Instant::now() + READ_WAIT) {
                Ok(data) if !data.is_empty() => Saved::Clip(Clip { kind, data }),
                Ok(_) => Saved::Nothing,
                Err(why) => {
                    tracing::debug!(why, "the clipboard could not be kept for the emoji");
                    Saved::Unread
                }
            };
            let _ = done.send(Done::Saved { number, saved });
        });
    }

    /// Where the threads' findings come back, made once.
    fn emoji_done(&mut self) -> Option<Sender<Done>> {
        if let Some(done) = &self.emoji.done {
            return Some(done.clone());
        }
        let (sender, receiver) = channel::channel::<Done>();
        let inserted = self.loop_handle.insert_source(receiver, |event, _, app| {
            if let Event::Msg(done) = event {
                app.state.emoji_found(done);
            }
        });
        match inserted {
            Ok(_) => {
                self.emoji.done = Some(sender.clone());
                Some(sender)
            }
            Err(err) => {
                tracing::warn!(%err.error, "could not watch the emoji's threads");
                None
            }
        }
    }

    fn emoji_found(&mut self, done: Done) {
        let Some(insert) = self.emoji.insert.as_mut() else {
            return;
        };
        let How::Paste { saved, terminal } = &mut insert.how else {
            return;
        };
        match done {
            Done::Saved { number, saved: s } if number == insert.number => *saved = Some(s),
            Done::Terminal {
                number,
                terminal: t,
            } if number == insert.number => *terminal = Some(t),
            _ => return,
        }
        self.emoji_try();
    }

    /// The keyboard went somewhere: an emoji waiting for its window may go
    /// in, after this turn of the loop (the keyboard is busy now).
    pub fn emoji_focus_changed(&mut self, focus: Option<&KeyboardFocus>) {
        let surface = focus.and_then(|f| match f {
            KeyboardFocus::Wayland(s) => Some(s.clone()),
            KeyboardFocus::X11(_) => None,
        });
        self.emoji.inputs.focus_changed(surface.as_ref());
        if self.emoji.insert.is_some() {
            self.loop_handle.insert_idle(|app| app.state.emoji_try());
        }
    }

    /// A text input was enabled again: an emoji may be waiting for it.
    pub fn emoji_text_input_ready(&mut self) {
        if self.emoji.insert.is_some() {
            self.loop_handle.insert_idle(|app| app.state.emoji_try());
        }
    }

    /// The emoji waiting goes in, if what it waits for is there.
    fn emoji_try(&mut self) {
        let Some(insert) = &self.emoji.insert else {
            return;
        };
        if !self.windows.contains_key(&insert.window) {
            tracing::debug!("the emoji's window went");
            self.emoji_finish();
            return;
        }
        if !self.holds_keyboard(insert.window) {
            return;
        }
        let number = insert.number;
        match &insert.how {
            How::TextInput => {
                let Some(surface) = self.text_surface(insert.window) else {
                    return;
                };
                if !self.emoji.inputs.enabled(&surface) {
                    return;
                }
                let text = insert.text.clone();
                if self.emoji.inputs.commit_string(&surface, &text) {
                    tracing::debug!("an emoji committed to its text input");
                    self.emoji_finish();
                }
            }
            How::Paste {
                saved: Some(_),
                terminal: Some(terminal),
            } => {
                let terminal = *terminal;
                let bytes: Arc<[u8]> = Arc::from(insert.text.as_bytes());
                let Some(Insert { how, .. }) = self.emoji.insert.as_mut() else {
                    return;
                };
                let How::Paste { saved, .. } = std::mem::replace(how, How::TextInput) else {
                    return;
                };
                *how = How::Pasted {
                    saved: saved.unwrap_or(Saved::Unread),
                    offered: bytes.clone(),
                };
                self.offer(Some(ClipKind::Text.types()), Some(bytes));
                tracing::debug!(terminal, "an emoji pasted");
                self.paste_keys(terminal);
                self.emoji_wait(number);
            }
            How::Paste { .. } | How::Pasted { .. } => {}
        }
    }

    /// The clipboard holds `bytes` as `types`, for Wayland and X11 clients;
    /// nothing when `types` is `None`.
    fn offer(&mut self, types: Option<Vec<String>>, bytes: Option<Arc<[u8]>>) {
        match (&types, bytes) {
            (Some(types), Some(bytes)) => set_data_device_selection(
                &self.display_handle,
                &self.seat,
                types.clone(),
                Some(bytes),
            ),
            _ => clear_data_device_selection(&self.display_handle, &self.seat),
        }
        if let Some(xwm) = self.xwm.as_mut()
            && let Err(err) = xwm.new_selection(SelectionTarget::Clipboard, types)
        {
            tracing::warn!(?err, "could not offer the clipboard to X11");
        }
    }

    /// A client reads what the compositor offers: if it is the emoji
    /// pasted, the clipboard goes back once this turn is over.
    pub fn emoji_read(&mut self, bytes: &Arc<[u8]>) {
        let pasted = matches!(
            &self.emoji.insert,
            Some(Insert { how: How::Pasted { offered, .. }, .. }) if Arc::ptr_eq(offered, bytes)
        );
        if pasted {
            self.loop_handle
                .insert_idle(|app| app.state.emoji_give_back());
        }
    }

    /// The clipboard takes back what it held before the paste.
    fn emoji_give_back(&mut self) {
        let Some(Insert {
            how: How::Pasted { saved, .. },
            ..
        }) = self.emoji.insert.as_mut()
        else {
            return;
        };
        match std::mem::replace(saved, Saved::Unread) {
            Saved::Nothing => self.offer(None, None),
            Saved::Clip(clip) => {
                let types = clip.kind.types();
                self.offer(Some(types), Some(Arc::from(clip.data)));
            }
            Saved::Unread => {
                tracing::debug!("the clipboard could not be read: the emoji stays on it");
            }
        }
        tracing::debug!("the clipboard is given back");
        self.emoji_finish();
    }

    /// The insertion is over: a picker kept open takes the keyboard back.
    fn emoji_finish(&mut self) {
        if let Some(insert) = self.emoji.insert.take()
            && insert.keep
        {
            self.tell_shell(ToShell::EmojiInserted);
        }
    }

    /// The picker's surface, if one shows on a screen.
    pub fn emoji_layer(&self) -> Option<smithay::desktop::LayerSurface> {
        self.space.outputs().find_map(|o| {
            smithay::desktop::layer_map_for_output(o)
                .layers()
                .find(|l| l.namespace() == EMOJI_NAMESPACE)
                .cloned()
        })
    }
}
