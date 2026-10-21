//! A blocking client, for the `hocus` tool and for tests.

use std::{
    collections::VecDeque,
    fmt,
    io::{self, BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::Path,
    time::Duration,
};

use crate::{ApiError, Event, Reply, Request, RequestLine, ServerLine};

#[derive(Debug)]
pub struct Client {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
    /// Events that arrived while waiting for a reply.
    events: VecDeque<Event>,
}

#[derive(Debug)]
pub enum ClientError {
    Io(io::Error),
    /// The compositor closed the connection.
    Closed,
    /// A line that is not of this protocol.
    Protocol(String),
    /// The compositor refused or failed the request.
    Api(ApiError),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "{err}"),
            Self::Closed => write!(f, "the compositor closed the connection"),
            Self::Protocol(line) => write!(f, "unexpected line from the compositor: {line}"),
            Self::Api(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<io::Error> for ClientError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

impl Client {
    pub fn connect(path: &Path) -> Result<Self, ClientError> {
        let writer = UnixStream::connect(path)?;
        let reader = BufReader::new(writer.try_clone()?);
        Ok(Self {
            reader,
            writer,
            next_id: 1,
            events: VecDeque::new(),
        })
    }

    /// How long to wait for a line before failing; `None` waits forever.
    pub fn set_timeout(&self, timeout: Option<Duration>) -> Result<(), ClientError> {
        self.writer.set_read_timeout(timeout)?;
        Ok(())
    }

    /// Sends `request` and waits for its reply.
    pub fn request(&mut self, request: Request) -> Result<Reply, ClientError> {
        let id = self.next_id;
        self.next_id += 1;
        let mut line = serde_json::to_vec(&RequestLine { id, request })
            .map_err(|err| ClientError::Protocol(err.to_string()))?;
        line.push(b'\n');
        self.writer.write_all(&line)?;
        loop {
            match self.read_line()? {
                ServerLine::Reply { id: got, ok } if got == id => return Ok(ok),
                ServerLine::Error {
                    id: Some(got),
                    error,
                } if got == id => return Err(ClientError::Api(error)),
                ServerLine::Error { id: None, error } => return Err(ClientError::Api(error)),
                ServerLine::Event { event } => self.events.push_back(event),
                // A reply to an earlier request that timed out.
                ServerLine::Reply { .. } | ServerLine::Error { .. } => {}
            }
        }
    }

    /// The next event; [`Request::Subscribe`] first.
    pub fn next_event(&mut self) -> Result<Event, ClientError> {
        if let Some(event) = self.events.pop_front() {
            return Ok(event);
        }
        loop {
            if let ServerLine::Event { event } = self.read_line()? {
                return Ok(event);
            }
        }
    }

    fn read_line(&mut self) -> Result<ServerLine, ClientError> {
        let mut line = String::new();
        if self.reader.read_line(&mut line)? == 0 {
            return Err(ClientError::Closed);
        }
        serde_json::from_str(&line).map_err(|_| ClientError::Protocol(line.trim_end().to_string()))
    }
}
