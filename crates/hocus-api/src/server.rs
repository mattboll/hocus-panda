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
    /// The only user whose connections are served: the compositor's own.
    owner: u32,
    connections: HashMap<ConnectionId, Connection>,
    next_id: u64,
}

#[derive(Debug)]
struct Connection {
    stream: UnixStream,
    input: Vec<u8>,
    output: Vec<u8>,
    subscribed: bool,
    role: Role,
}

/// Who a connection says it is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Role {
    /// The `hocus` tool, the settings, tests: the server's permission.
    Client,
    /// An MCP agent of the configuration, by name (#42): only what agents
    /// may make, whatever the server's permission; the compositor checks
    /// its rights against the configuration in effect at each action, so
    /// that taking them away there applies at once.
    Agent(String),
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
        // The user's alone, whatever the directory around it allows.
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
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
            owner: rustix::process::getuid().as_raw(),
            connections: HashMap::new(),
            next_id: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Serves another user than this process's: tests see a stranger
    /// refused.
    #[doc(hidden)]
    pub fn serve_only(&mut self, uid: u32) {
        self.owner = uid;
    }

    pub fn permission(&self) -> Permission {
        self.permission
    }

    /// The agent `connection` named itself, if it did.
    pub fn agent(&self, connection: ConnectionId) -> Option<&str> {
        match &self.connections.get(&connection)?.role {
            Role::Agent(name) => Some(name),
            Role::Client => None,
        }
    }

    /// `connection` is agent `name` from now on; once named, it stays so.
    pub fn set_agent(&mut self, connection: ConnectionId, name: String) {
        if let Some(c) = self.connections.get_mut(&connection) {
            c.role = Role::Agent(name);
        }
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

/// The user at the other end, as the system tells it.
fn peer_uid(stream: &UnixStream) -> io::Result<u32> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        rustix::net::sockopt::socket_peercred(stream)
            .map(|c| c.uid.as_raw())
            .map_err(io::Error::from)
    }
    #[cfg(target_os = "freebsd")]
    {
        nix::unistd::getpeereid(stream)
            .map(|(uid, _)| uid.as_raw())
            .map_err(|e| io::Error::from_raw_os_error(e as i32))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "freebsd")))]
    {
        let _ = stream;
        Err(io::Error::other("no peer credentials on this system"))
    }
}

fn accept<D: Handler>(handle: &LoopHandle<'static, D>, data: &mut D, stream: UnixStream) {
    // Another user's connection is closed unread.
    let owner = data.server().owner;
    match peer_uid(&stream) {
        Ok(uid) if uid == owner => {}
        Ok(uid) => {
            tracing::warn!(uid, "refused a control connection from another user");
            return;
        }
        Err(err) => {
            tracing::warn!(%err, "refused a control connection: whose is it?");
            return;
        }
    }
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
            role: Role::Client,
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
    let role = server.connections.get(&conn).map(|c| c.role.clone());
    let refusal = match role {
        Some(Role::Agent(name)) if !request.agents_may() => Some(format!(
            "agent {name:?} may not make this request: agents read windows, outputs, the \
             shell's interface and captures, and act through `act`"
        )),
        Some(Role::Agent(_)) => None,
        _ if request.permission() > server.permission => Some(
            "this request drives input or reads the whole scene: it needs the test \
             permission (start the compositor with --api-permission test), or, for \
             `act`, an agent of [[agents.mcp]] with access = \"act\""
                .to_owned(),
        ),
        _ => None,
    };
    if let Some(message) = refusal {
        let error = ApiError::new(ErrorCode::PermissionDenied, message);
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
