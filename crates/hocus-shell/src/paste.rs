//! Pasting into the shell's text fields: `Ctrl+V` and `Shift+Insert` take
//! the clipboard's text (the clipboard, not the primary selection), its
//! line breaks and other control characters dropped. A field asks with
//! [`request`] and gets the text back once, off the loop.
//!
//! The shell reads the clipboard itself, on the Wayland connection iced
//! uses (its surfaces hold the keyboard, so the compositor offers it the
//! selection there): a data device on an event queue of its own, which no
//! thread waits on. Its events are read with iced's and handled only when
//! a paste asks, or when a popover closes ([`tidy`]), so nothing wakes for
//! them. iced's own clipboard is off: it reads on the loop and waits there
//! for as long as the client that copied takes to write, forever if it
//! hangs, and it keeps a thread awake for every key and every move of the
//! pointer.
//!
//! The text comes through a pipe, read on a thread of its own for 2 s at
//! most, into a buffer of fixed size that is overwritten with zeros once
//! the field has taken what it wants: it may be a password. Nothing here
//! logs it, nor its length.
//!
//! The same data device takes a drag of files over a phone's row (bar.md
//! "The phone", #957): the compositor says when one is over the
//! connections ([`hocus_link::ToShell::FileDrag`]), since the shell cannot
//! tell which of its surfaces a drag's `enter` names; the phone's row then
//! reads the files the drag carries ([`drag_files`]), takes the offer or
//! refuses it ([`drag_take`]), and let go, finishes it ([`drag_finish`]).
//! Those wait for the data device's events on a thread of their own, never
//! on the loop, for [`WAIT`] at most.

use std::{
    io::{PipeReader, Read},
    os::fd::AsFd,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use wayland_client::{
    Connection, Dispatch, EventQueue, Proxy, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::{
        wl_data_device::{self, WlDataDevice},
        wl_data_device_manager::{DndAction, WlDataDeviceManager},
        wl_data_offer::{self, WlDataOffer},
        wl_registry::WlRegistry,
        wl_seat::WlSeat,
    },
};

/// The text types read, best first (UTF8_STRING for X11 clients).
const TEXT: [&str; 3] = ["text/plain;charset=utf-8", "UTF8_STRING", "text/plain"];
/// How long the client that copied has to write.
const WAIT: Duration = Duration::from_secs(2);
/// The most read: a paste into a field is a line, not a document.
const ROOM: usize = 4096;
/// The types kept per offer: a client may offer many.
const TYPES: usize = 32;
/// The type a drag of files offers.
const URIS: &str = "text/uri-list";
/// The most read of a drag's files' addresses.
const URIS_ROOM: usize = 64 * 1024;

/// How a field takes the clipboard's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clean {
    /// Its line breaks and other control characters dropped: a password
    /// (3.5b).
    Dropped,
    /// On one line: its line breaks and runs of white space one space, its
    /// ends trimmed, its other control characters dropped: a task's title
    /// (Day.4b).
    Spaced,
}

/// Reads the clipboard's text off the loop, then hands it to `done` on the
/// reader's thread: cleaned as `clean` says, or `None` when it could not
/// be read in time, is too long or is not UTF-8. The text lives only for
/// the call. `false` when there is nothing to read (no clipboard, no text
/// in it): `done` is then never called.
pub fn request(clean: Clean, done: impl FnOnce(Option<&str>) + Send + 'static) -> bool {
    let pipe = {
        let mut clipboard = CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(clipboard) = clipboard.as_mut() else {
            return false;
        };
        match clipboard.receive() {
            Ok(Some(pipe)) => pipe,
            Ok(None) => return false,
            Err(err) => {
                tracing::warn!(%err, "could not ask for the clipboard");
                return false;
            }
        }
    };
    let reader = std::thread::Builder::new()
        .name("paste".to_owned())
        .spawn(move || read(pipe, clean, done));
    if let Err(err) = reader {
        tracing::warn!(%err, "could not read the clipboard");
        return false;
    }
    true
}

/// Handles what the compositor said about the clipboard since the last
/// time: offers replaced are let go.
pub fn tidy() {
    let mut clipboard = CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(clipboard) = clipboard.as_mut()
        && let Err(err) = clipboard.queue.dispatch_pending(&mut clipboard.offers)
    {
        tracing::warn!(%err, "the clipboard's events");
    }
}

/// Binds a data device on `connection`, the one iced draws on.
pub fn start(connection: &Connection) {
    match Clipboard::bind(connection) {
        Ok(clipboard) => {
            *CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner) = Some(clipboard);
        }
        Err(err) => tracing::warn!(%err, "no clipboard: nothing can be pasted"),
    }
}

static CLIPBOARD: Mutex<Option<Clipboard>> = Mutex::new(None);

struct Clipboard {
    connection: Connection,
    queue: EventQueue<Offers>,
    offers: Offers,
    _device: WlDataDevice,
}

/// What the compositor offers: the clipboard's selection, and what a drag
/// over the shell carries, with its `enter`'s serial (let go when it
/// leaves); let go on the shell, it is kept until [`drag_finish`] takes it.
#[derive(Debug, Default)]
struct Offers {
    selection: Option<WlDataOffer>,
    dragged: Option<(WlDataOffer, u32)>,
    dropped: Option<WlDataOffer>,
}

/// An offer's types, as they come.
#[derive(Debug, Default)]
struct Types(Mutex<Vec<String>>);

impl Clipboard {
    fn bind(connection: &Connection) -> Result<Self, String> {
        let (globals, queue) =
            registry_queue_init::<Offers>(connection).map_err(|e| e.to_string())?;
        let qh = queue.handle();
        let manager: WlDataDeviceManager = globals
            .bind(&qh, 1..=3, ())
            .map_err(|e| format!("wl_data_device_manager: {e}"))?;
        // The compositor has one seat.
        let seat: WlSeat = globals
            .bind(&qh, 1..=1, ())
            .map_err(|e| format!("wl_seat: {e}"))?;
        let device = manager.get_data_device(&seat, &qh, ());
        Ok(Self {
            connection: connection.clone(),
            queue,
            offers: Offers::default(),
            _device: device,
        })
    }

    /// Asks the selection's client for its text: the pipe it writes it to.
    fn receive(&mut self) -> Result<Option<PipeReader>, String> {
        self.queue
            .dispatch_pending(&mut self.offers)
            .map_err(|e| e.to_string())?;
        let Some(offer) = &self.offers.selection else {
            tracing::debug!("the clipboard is empty");
            return Ok(None);
        };
        let types = offer
            .data::<Types>()
            .map(|t| t.0.lock().unwrap_or_else(PoisonError::into_inner).clone());
        let Some(kind) = TEXT.iter().find(|t| {
            types
                .as_ref()
                .is_some_and(|types| types.iter().any(|o| o == *t))
        }) else {
            tracing::debug!(?types, "the clipboard holds no text");
            return Ok(None);
        };
        let reader = ask(&self.connection, offer, kind)?;
        tracing::debug!(kind, "the clipboard's text asked for");
        Ok(Some(reader))
    }
}

// --- A drag of files -----------------------------------------------------------------

/// The files the drag over the shell carries, their `text/uri-list` lines
/// (`#` comments left out), read from its offer. Waits for the offer and
/// for what its client writes, [`WAIT`] at most each: on a thread of its
/// own, never the loop's. An error when there is none, it carries no files
/// or its client did not write them in time.
pub fn drag_files() -> Result<Vec<String>, String> {
    let until = Instant::now() + WAIT;
    let pipe = wait_for(until, |clipboard| {
        let (offer, _) = clipboard.offers.dragged.as_ref()?;
        Some(ask(&clipboard.connection, offer, URIS))
    })
    .ok_or("no drag over the shell")??;
    let mut room = vec![0; URIS_ROOM];
    let len = read_until(&pipe, &mut room, Instant::now() + WAIT)?;
    let text = std::str::from_utf8(&room[..len]).map_err(|_| "the files' list is not UTF-8")?;
    Ok(text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect())
}

/// Takes the drag's offer (its files, copied), or refuses it: let go, it
/// comes to the shell only taken. Nothing when no drag is over the shell.
pub fn drag_take(take: bool) {
    let mut clipboard = CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner);
    let Some(clipboard) = clipboard.as_mut() else {
        return;
    };
    if let Err(err) = clipboard.queue.dispatch_pending(&mut clipboard.offers) {
        tracing::warn!(%err, "the clipboard's events");
    }
    let Some((offer, serial)) = &clipboard.offers.dragged else {
        return;
    };
    if take {
        offer.accept(*serial, Some(URIS.to_owned()));
        offer.set_actions(DndAction::Copy, DndAction::Copy);
    } else {
        offer.accept(*serial, None);
        offer.set_actions(DndAction::empty(), DndAction::None);
    }
    flush(&clipboard.connection);
}

/// The drag let go on the shell is finished: its client hears its files
/// were taken. Waits for its `drop`, [`WAIT`] at most: on a thread of its
/// own. Whether it came.
pub fn drag_finish() -> bool {
    let finished = wait_for(Instant::now() + WAIT, |clipboard| {
        let offer = clipboard.offers.dropped.take()?;
        offer.finish();
        offer.destroy();
        flush(&clipboard.connection);
        Some(())
    });
    finished.is_some()
}

/// Handles the data device's events until `take` finds what it waits for,
/// reading them from the connection meanwhile (iced reads them too: the
/// first to read hands each queue its own), until `until`.
fn wait_for<T>(until: Instant, mut take: impl FnMut(&mut Clipboard) -> Option<T>) -> Option<T> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    loop {
        let guard = {
            let mut clipboard = CLIPBOARD.lock().unwrap_or_else(PoisonError::into_inner);
            let clipboard = clipboard.as_mut()?;
            if let Err(err) = clipboard.queue.dispatch_pending(&mut clipboard.offers) {
                tracing::warn!(%err, "the clipboard's events");
                return None;
            }
            if let Some(found) = take(clipboard) {
                return Some(found);
            }
            clipboard.queue.prepare_read()
        };
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return None;
        }
        // Events of its own already read: handled at once.
        let Some(guard) = guard else {
            continue;
        };
        let timeout = Timespec::try_from(left).ok()?;
        let fd = guard.connection_fd();
        let mut fds = [PollFd::new(&fd, PollFlags::IN)];
        let ready = matches!(poll(&mut fds, Some(&timeout)), Ok(n) if n > 0);
        if ready {
            match guard.read() {
                Ok(_) => {}
                Err(wayland_client::backend::WaylandError::Io(e))
                    if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    tracing::warn!(%err, "could not read the data device's events");
                    return None;
                }
            }
        }
    }
}

/// Asks `offer` for `kind`: the pipe its client writes it to.
fn ask(connection: &Connection, offer: &WlDataOffer, kind: &str) -> Result<PipeReader, String> {
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    offer.receive(kind.to_owned(), writer.as_fd());
    drop(writer);
    flush(connection);
    Ok(reader)
}

/// Sends what is asked; the loop flushes what is left when the socket is
/// full.
fn flush(connection: &Connection) {
    match connection.flush() {
        Ok(()) => {}
        Err(wayland_client::backend::WaylandError::Io(e))
            if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(err) => tracing::warn!(%err, "could not send to the compositor"),
    }
}

/// Reads `pipe` and hands the text to `done`; both buffers are zeroed
/// after.
fn read(pipe: PipeReader, how: Clean, done: impl FnOnce(Option<&str>)) {
    let mut raw = Wiped([0; ROOM]);
    let mut clean = Wiped([0; ROOM]);
    let read = read_until(&pipe, &mut raw.0, Instant::now() + WAIT);
    drop(pipe);
    let text = match read {
        Ok(len) => match std::str::from_utf8(&raw.0[..len]) {
            Ok(text) => Some(match how {
                Clean::Dropped => without_controls(text, &mut clean.0),
                Clean::Spaced => on_one_line(text, &mut clean.0),
            }),
            Err(_) => {
                tracing::debug!("the clipboard's text is not UTF-8: nothing pasted");
                None
            }
        },
        Err(why) => {
            tracing::debug!(why, "nothing pasted");
            None
        }
    };
    done(text);
}

/// What comes through `pipe` until it closes, into `into`; past `until`,
/// or past `into`'s size, nothing.
fn read_until(pipe: &PipeReader, into: &mut [u8], until: Instant) -> Result<usize, &'static str> {
    use rustix::event::{PollFd, PollFlags, Timespec, poll};
    let mut len = 0;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err("the clipboard's client did not write it in time");
        }
        let timeout = Timespec::try_from(left).map_err(|_| "a wait")?;
        let mut fds = [PollFd::new(pipe, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(rustix::io::Errno::INTR) => continue,
            Ok(_) => {}
            Err(_) => return Err("could not wait for the clipboard"),
        }
        // Full: only its end may come still.
        let mut past = Wiped([0; 1]);
        let room = if len < into.len() {
            &mut into[len..]
        } else {
            &mut past.0[..]
        };
        match (&*pipe).read(room) {
            Ok(0) => return Ok(len),
            Ok(_) if len == into.len() => return Err("the clipboard's text is too long"),
            Ok(n) => len += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => return Err("could not read the clipboard"),
        }
    }
}

/// `text` without its control characters (line breaks, tabs), written
/// into `into`, which is as long as `text` at least.
fn without_controls<'a>(text: &str, into: &'a mut [u8]) -> &'a str {
    let mut len = 0;
    for c in text.chars().filter(|c| !c.is_control()) {
        len += c.encode_utf8(&mut into[len..]).len();
    }
    // Whole characters only were written.
    std::str::from_utf8(&into[..len]).unwrap_or_default()
}

/// `text` on one line, written into `into` (as long as `text` at least):
/// each run of white space (line breaks, tabs) one space, none at its
/// ends; its other control characters dropped.
fn on_one_line<'a>(text: &str, into: &'a mut [u8]) -> &'a str {
    let mut len = 0;
    let mut space = false;
    for c in text.chars() {
        if c.is_whitespace() {
            space = len > 0;
        } else if !c.is_control() {
            if space {
                len += ' '.encode_utf8(&mut into[len..]).len();
                space = false;
            }
            len += c.encode_utf8(&mut into[len..]).len();
        }
    }
    std::str::from_utf8(&into[..len]).unwrap_or_default()
}

/// Bytes overwritten with zeros when dropped, through `black_box` so that
/// the writes are not dropped as dead (as [`crate::network::secret`]).
struct Wiped<const N: usize>([u8; N]);

impl<const N: usize> Drop for Wiped<N> {
    fn drop(&mut self) {
        self.0.fill(0);
        std::hint::black_box(&mut self.0);
    }
}

// --- The data device's events ------------------------------------------------------

impl Dispatch<WlRegistry, GlobalListContents> for Offers {
    fn event(
        _: &mut Self,
        _: &WlRegistry,
        _: <WlRegistry as Proxy>::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlSeat, ()> for Offers {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: <WlSeat as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDeviceManager, ()> for Offers {
    fn event(
        _: &mut Self,
        _: &WlDataDeviceManager,
        _: <WlDataDeviceManager as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<WlDataDevice, ()> for Offers {
    fn event(
        offers: &mut Self,
        _: &WlDataDevice,
        event: wl_data_device::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_data_device::Event::Selection { id } => {
                if let Some(old) = std::mem::replace(&mut offers.selection, id) {
                    old.destroy();
                }
            }
            wl_data_device::Event::Enter { serial, id, .. } => {
                if let Some((old, _)) =
                    std::mem::replace(&mut offers.dragged, id.map(|o| (o, serial)))
                {
                    old.destroy();
                }
            }
            wl_data_device::Event::Leave => {
                if let Some((old, _)) = offers.dragged.take() {
                    old.destroy();
                }
            }
            // Only once its offer was taken: kept for its files, then
            // finished. One before it, never finished, is let go: its
            // client hears it cancelled.
            wl_data_device::Event::Drop => {
                let dropped = offers.dragged.take().map(|(offer, _)| offer);
                if let Some(old) = std::mem::replace(&mut offers.dropped, dropped) {
                    old.destroy();
                }
            }
            _ => {}
        }
    }

    event_created_child!(Offers, WlDataDevice, [
        wl_data_device::EVT_DATA_OFFER_OPCODE => (WlDataOffer, Types::default()),
    ]);
}

impl Dispatch<WlDataOffer, Types> for Offers {
    fn event(
        _: &mut Self,
        _: &WlDataOffer,
        event: wl_data_offer::Event,
        types: &Types,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_data_offer::Event::Offer { mime_type } = event {
            let mut types = types.0.lock().unwrap_or_else(PoisonError::into_inner);
            if types.len() < TYPES {
                types.push(mime_type);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_breaks_and_other_controls_are_dropped() {
        let mut into = [0; 64];
        assert_eq!(without_controls("azerty&1@ê\n", &mut into), "azerty&1@ê");
        assert_eq!(
            without_controls("\ta b\r\n\u{7f}c\u{85}", &mut into),
            "a bc"
        );
        assert_eq!(without_controls("\n", &mut into), "");
    }

    #[test]
    fn on_one_line_line_breaks_and_runs_of_space_are_one_space_its_ends_trimmed() {
        let mut into = [0; 64];
        assert_eq!(on_one_line("Appeler\nInès\n", &mut into), "Appeler Inès");
        assert_eq!(
            on_one_line("  \tRelire \r\n\n la\u{7f} spec\u{85} ", &mut into),
            "Relire la spec"
        );
        assert_eq!(on_one_line("\n \t", &mut into), "");
    }

    #[test]
    fn a_pipe_is_read_to_its_end_within_its_time_and_its_room() {
        use std::io::Write;
        let read = |text: &[u8], room: usize, close: bool| {
            let (reader, mut writer) = std::io::pipe().expect("a pipe");
            writer.write_all(text).expect("written");
            let kept = (!close).then_some(writer);
            let mut into = vec![0; room];
            let got = read_until(
                &reader,
                &mut into,
                Instant::now() + Duration::from_millis(100),
            );
            drop(kept);
            got.map(|len| into[..len].to_vec())
        };
        assert_eq!(read(b"azerty\n", 16, true), Ok(b"azerty\n".to_vec()));
        assert_eq!(read(b"exactly8", 8, true), Ok(b"exactly8".to_vec()));
        assert!(read(b"too long", 4, true).is_err());
        // A client that never closes it: nothing, once the time is up.
        assert!(read(b"azerty", 16, false).is_err());
    }
}
