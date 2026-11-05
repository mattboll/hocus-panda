//! One window at a time: the first Settings listen on a socket in the
//! session's runtime directory; a second launch hands them its section and
//! its activation token there, and ends. The first then opens that section
//! and asks for the focus with the token ([`crate::activate`]).

use std::{
    io::{BufRead as _, BufReader, Write as _},
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    time::Duration,
};

/// What a second launch asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Again {
    /// The section to open (`power`), if it named one.
    pub section: Option<String>,
    /// Its activation token, to take the focus with.
    pub token: Option<String>,
    /// A Bluetooth device to pair, by its address (`--pair`).
    pub pair: Option<String>,
}

impl Again {
    fn line(&self) -> String {
        format!(
            "open {} {} {}\n",
            self.section.as_deref().unwrap_or("-"),
            self.token.as_deref().unwrap_or("-"),
            self.pair.as_deref().unwrap_or("-")
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let mut words = line.split_whitespace();
        (words.next()? == "open").then_some(())?;
        let word = |w: Option<&str>| w.filter(|w| *w != "-").map(str::to_owned);
        Some(Self {
            section: word(words.next()),
            token: word(words.next()),
            pair: word(words.next()),
        })
    }
}

/// How this launch starts.
#[derive(Debug)]
pub enum Start {
    /// The first: it opens the window and listens.
    First(Option<UnixListener>),
    /// Another runs: it was told.
    Told,
}

/// The socket's path: per Wayland display, in the runtime directory.
fn path() -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|d| !d.is_empty())?;
    let display = std::env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_owned());
    let name: String = std::path::Path::new(&display)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    Some(PathBuf::from(runtime).join(format!("hocus-settings-{name}")))
}

/// Tells a running Settings `again`, or becomes the one that runs.
pub fn start(again: &Again) -> Start {
    let Some(path) = path() else {
        tracing::warn!("no runtime directory: more than one window may open");
        return Start::First(None);
    };
    match UnixStream::connect(&path) {
        Ok(mut stream) => {
            let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
            match stream.write_all(again.line().as_bytes()) {
                Ok(()) => {
                    tracing::info!(section = ?again.section, "the Settings already run: told them");
                    return Start::Told;
                }
                Err(err) => tracing::warn!(%err, "the running Settings did not listen"),
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => {
            // Left by one that ended.
            let _ = std::fs::remove_file(&path);
        }
    }
    match UnixListener::bind(&path) {
        Ok(listener) => Start::First(Some(listener)),
        Err(err) => {
            tracing::warn!(%err, path = %path.display(), "cannot listen for a second launch");
            Start::First(None)
        }
    }
}

/// Listens on a thread; `told` takes each second launch's ask, and
/// returns `false` once the window is gone.
pub fn listen(listener: UnixListener, mut told: impl FnMut(Again) -> bool + Send + 'static) {
    let started = std::thread::Builder::new()
        .name("settings' second launches".to_owned())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else {
                    continue;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
                let mut line = String::new();
                if BufReader::new(stream).read_line(&mut line).is_err() {
                    continue;
                }
                if let Some(again) = Again::parse(&line)
                    && !told(again)
                {
                    return;
                }
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot listen for a second launch");
    }
}

/// The socket goes with the window.
pub fn stop() {
    if let Some(path) = path() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ask_on_a_line_and_back() {
        let again = Again {
            section: Some("power".to_owned()),
            token: Some("hocus-12".to_owned()),
            pair: None,
        };
        assert_eq!(Again::parse(&again.line()), Some(again));
        let again = Again {
            section: Some("bluetooth".to_owned()),
            token: None,
            pair: Some("5C:12:AA:01:02:05".to_owned()),
        };
        assert_eq!(Again::parse(&again.line()), Some(again));
        let bare = Again {
            section: None,
            token: None,
            pair: None,
        };
        assert_eq!(bare.line(), "open - - -\n");
        assert_eq!(Again::parse(&bare.line()), Some(bare));
        assert_eq!(Again::parse("close"), None);
    }
}
