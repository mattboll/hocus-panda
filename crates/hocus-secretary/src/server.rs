//! Her private socket: `0600`, in the session's runtime directory. Each
//! connection has a reader thread (its lines to her loop) and a writer
//! thread (her lines to it), so that her loop never waits on a client.

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::Path,
    sync::mpsc::{self, Sender},
};

use crate::brain::Event;

/// The longest line read: a question, a graph's query.
const LONGEST: usize = 1 << 20;

/// Listens on `path` (a socket left there by an earlier run is replaced),
/// on a thread of its own.
pub fn serve(path: &Path, events: Sender<Event>) -> std::io::Result<()> {
    if std::fs::symlink_metadata(path).is_ok() {
        std::fs::remove_file(path)?;
    }
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    std::thread::Builder::new()
        .name("socket".to_owned())
        .spawn(move || {
            let mut next = 1u64;
            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let conn = next;
                        next += 1;
                        if let Err(err) = connected(conn, stream, &events) {
                            tracing::warn!(conn, %err, "a connection could not be taken");
                        }
                    }
                    Err(err) => tracing::warn!(%err, "a connection failed"),
                }
            }
        })?;
    Ok(())
}

fn connected(conn: u64, stream: UnixStream, events: &Sender<Event>) -> std::io::Result<()> {
    let (writer, lines) = mpsc::channel::<String>();
    let mut out = stream.try_clone()?;
    let closer = stream.try_clone()?;
    std::thread::Builder::new()
        .name(format!("write {conn}"))
        .spawn(move || {
            for line in lines {
                if out.write_all(line.as_bytes()).is_err() {
                    break;
                }
            }
            let _ = out.shutdown(std::net::Shutdown::Write);
        })?;
    if events
        .send(Event::Connected {
            conn,
            writer,
            closer: Some(closer),
        })
        .is_err()
    {
        return Ok(());
    }
    let events = events.clone();
    std::thread::Builder::new()
        .name(format!("read {conn}"))
        .spawn(move || {
            let mut reader = BufReader::new(stream);
            let mut text = String::new();
            loop {
                text.clear();
                match (&mut reader).take(LONGEST as u64).read_line(&mut text) {
                    Ok(0) | Err(_) => break,
                    Ok(_) if !text.ends_with('\n') && text.len() >= LONGEST => {
                        tracing::info!(conn, "a line too long: the connection closes");
                        break;
                    }
                    Ok(_) => {
                        let line = text.trim_end().to_owned();
                        if line.is_empty() {
                            continue;
                        }
                        if events.send(Event::Line { conn, line }).is_err() {
                            break;
                        }
                    }
                }
            }
            let _ = events.send(Event::Closed { conn });
        })?;
    Ok(())
}
