//! The control socket, served on the compositor's calloop loop. Nothing on it
//! blocks: sockets are non-blocking, and a client that stops reading is
//! dropped once its pending output passes a bound, instead of growing it
//! forever.

use std::{
    collections::HashMap,
    io::{self, Read, Write},
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::{Path, PathBuf},
};

use calloop::{Interest, LoopHandle, Mode, PostAction, generic::Generic};

use crate::{ApiError, ErrorCode, Event, Permission, Reply, Request, RequestLine, ServerLine};

/// Longest request line; nothing legitimate comes close.
const MAX_LINE: usize = 1 << 20;
/// Most output waiting for a client: a few full-size screenshots.
const MAX_PENDING: usize = 256 << 20;

/// One client connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionId(u64);

/// What the loop's data provides to serve the socket.
pub trait Handler: Sized + 'static {
    fn server(&mut self) -> &mut Server;

    /// Answers a request the connection is allowed to make. `None` when the
    /// answer comes later, through [`Server::reply`].
    fn handle(
        &mut self,
        connection: ConnectionId,
        id: u64,
        request: Request,
    ) -> Option<Result<Reply, ApiError>>;
}

#[derive(Debug)]
pub struct Server {
    path: PathBuf,
    permission: Permission,
    connections: HashMap<ConnectionId, Connection>,
    next_id: u64,
}

#[derive(Debug)]
struct Connection {
    stream: UnixStream,
    input: Vec<u8>,
    output: Vec<u8>,
    subscribed: bool,
}

impl Server {
    /// Listens at `path`, giving every connection `permission`.
    ///
    /// The path goes with a Wayland socket this process holds the lock of, so
    /// a file left there by a compositor that crashed is replaced.
    pub fn bind<D: Handler>(
        path: &Path,
        permission: Permission,
        handle: &LoopHandle<'static, D>,
    ) -> io::Result<Self> {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        let listener = UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;
        let accept_handle = handle.clone();
        handle
            .insert_source(
                Generic::new(listener, Interest::READ, Mode::Level),
                move |_, listener, data: &mut D| {
                    loop {
                        match listener.accept() {
                            Ok((stream, _)) => accept(&accept_handle, data, stream),
                            Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                            Err(err) => {
                                tracing::warn!(%err, "could not accept a control connection");
                                break;
                            }
                        }
                    }
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|err| io::Error::other(err.error))?;
        Ok(Self {
            path: path.to_path_buf(),
            permission,
            connections: HashMap::new(),
            next_id: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn permission(&self) -> Permission {
        self.permission
    }

    /// Whether anyone listens to events: building them can be skipped.
    pub fn has_subscribers(&self) -> bool {
        self.connections.values().any(|c| c.subscribed)
    }

    /// Answers request `id` of `connection`, if it is still there.
    pub fn reply(&mut self, connection: ConnectionId, id: u64, result: Result<Reply, ApiError>) {
        let line = match result {
            Ok(ok) => ServerLine::Reply { id, ok },
            Err(error) => ServerLine::Error {
                id: Some(id),
                error,
            },
        };
        self.send(connection, &line);
    }

    /// Sends `event` to every subscribed connection.
    pub fn broadcast(&mut self, event: &Event) {
        let subscribed: Vec<_> = self
            .connections
            .iter()
            .filter(|(_, c)| c.subscribed)
            .map(|(id, _)| *id)
            .collect();
        if subscribed.is_empty() {
            return;
        }
        let line = ServerLine::Event {
            event: event.clone(),
        };
        for id in subscribed {
            self.send(id, &line);
        }
    }

    fn send(&mut self, id: ConnectionId, line: &ServerLine) {
        let Some(conn) = self.connections.get_mut(&id) else {
            return;
        };
        if let Err(err) = serde_json::to_writer(&mut conn.output, line) {
            tracing::warn!(%err, "could not encode a control message");
            return;
        }
        conn.output.push(b'\n');
        if conn.output.len() > MAX_PENDING {
            tracing::warn!(?id, "control client does not read its replies; dropping it");
            self.drop_connection(id);
            return;
        }
        self.flush(id);
    }

    /// Writes what the socket takes now; the rest goes when it is writable.
    fn flush(&mut self, id: ConnectionId) {
        let Some(conn) = self.connections.get_mut(&id) else {
            return;
        };
        while !conn.output.is_empty() {
            match (&conn.stream).write(&conn.output) {
                Ok(0) => break,
                Ok(n) => {
                    conn.output.drain(..n);
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.drop_connection(id);
                    return;
                }
            }
        }
    }

    /// Forgets a connection; its loop source sees the shutdown and goes.
    fn drop_connection(&mut self, id: ConnectionId) {
        if let Some(conn) = self.connections.remove(&id) {
            let _ = conn.stream.shutdown(Shutdown::Both);
        }
    }

    /// Reads what arrived and returns the complete lines; `None` when the
    /// connection is gone.
    fn read_lines(&mut self, id: ConnectionId) -> Option<Vec<Vec<u8>>> {
        let conn = self.connections.get_mut(&id)?;
        let mut buf = [0u8; 16 * 1024];
        let mut open = true;
        loop {
            match (&conn.stream).read(&mut buf) {
                Ok(0) => {
                    open = false;
                    break;
                }
                Ok(n) => conn.input.extend_from_slice(&buf[..n]),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => break,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(_) => {
                    open = false;
                    break;
                }
            }
        }
        let mut lines = Vec::new();
        while let Some(end) = conn.input.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = conn.input.drain(..=end).collect();
            lines.push(line);
        }
        if conn.input.len() > MAX_LINE {
            tracing::warn!(?id, "control request line too long; dropping the client");
            self.drop_connection(id);
            return None;
        }
        if !open {
            // Requests sent just before hanging up are still answered, into
            // the void: what matters is that they are carried out.
            self.connections.remove(&id);
        }
        Some(lines)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn accept<D: Handler>(handle: &LoopHandle<'static, D>, data: &mut D, stream: UnixStream) {
    let watched = match stream
        .set_nonblocking(true)
        .and_then(|()| stream.try_clone())
    {
        Ok(watched) => watched,
        Err(err) => {
            tracing::warn!(%err, "could not set up a control connection");
            return;
        }
    };
    let server = data.server();
    let id = ConnectionId(server.next_id);
    server.next_id += 1;
    // Edge-triggered: readable when requests arrive, writable when a full
    // socket drains. The stream in the map does the reading and writing.
    let inserted = handle.insert_source(
        Generic::new(watched, Interest::BOTH, Mode::Edge),
        move |readiness, _, data: &mut D| {
            if readiness.writable {
                data.server().flush(id);
            }
            let Some(lines) = data.server().read_lines(id) else {
                return Ok(PostAction::Remove);
            };
            for line in lines {
                handle_line(data, id, &line);
            }
            if data.server().connections.contains_key(&id) {
                Ok(PostAction::Continue)
            } else {
                Ok(PostAction::Remove)
            }
        },
    );
    if let Err(err) = inserted {
        tracing::warn!(err = %err.error, "could not watch a control connection");
        return;
    }
    server.connections.insert(
        id,
        Connection {
            stream,
            input: Vec::new(),
            output: Vec::new(),
            subscribed: false,
        },
    );
}

fn handle_line<D: Handler>(data: &mut D, conn: ConnectionId, line: &[u8]) {
    if line.iter().all(u8::is_ascii_whitespace) {
        return;
    }
    let RequestLine { id, request } = match serde_json::from_slice(line) {
        Ok(request) => request,
        Err(err) => {
            let line = ServerLine::Error {
                id: request_id(line),
                error: ApiError::new(ErrorCode::BadRequest, err.to_string()),
            };
            data.server().send(conn, &line);
            return;
        }
    };
    let server = data.server();
    if request.permission() > server.permission {
        let error = ApiError::new(
            ErrorCode::PermissionDenied,
            "this request drives input or reads the whole scene: it needs the test \
             permission (start the compositor with --api-permission test)",
        );
        server.reply(conn, id, Err(error));
        return;
    }
    if request == Request::Subscribe {
        if let Some(c) = server.connections.get_mut(&conn) {
            c.subscribed = true;
        }
        server.reply(conn, id, Ok(Reply::Done));
        return;
    }
    if let Some(result) = data.handle(conn, id, request) {
        data.server().reply(conn, id, result);
    }
}

/// The id of a line that is not a valid request, when it has one.
fn request_id(line: &[u8]) -> Option<u64> {
    serde_json::from_slice::<serde_json::Value>(line)
        .ok()?
        .get("id")?
        .as_u64()
}
