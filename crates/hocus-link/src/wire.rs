//! How the messages go on the line: one JSON object a line, each way,
//! `{"kind":"bar","data":{"output":"eDP-1","form":"full","since":0,"scale":120}}`
//! (`data` left out when the kind says it all), written and read by serde
//! from the messages' types.
//!
//! The line opens with a hello each way: the shell's first line names the
//! protocol's [`VERSION`] ([`hello`]), and the compositor answers with its
//! own. A package upgraded under a running session pairs an old program
//! with a new one: on different versions neither reads the other's
//! messages. The compositor does not feed that shell and starts its
//! installed program again, once; the shell says nothing more and waits
//! to be ended. A first line that is no hello (the words of before
//! version 1) is a mismatch as any other.
//!
//! Between two sides of the same version, a line that does not read (a
//! kind one of them does not know yet) is said in the log and skipped:
//! never fatal.

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::BadLine;

/// The protocol's version: a change that the other side would misread
/// raises it (a kind taken out, a field renamed or of another meaning). A
/// new kind alone does not: the side that does not know it skips it.
///
/// **Version 1** (#774): JSON objects, one a line, after a hello each
/// way. Before it the messages were words separated by spaces, written
/// and read by hand, and no side said which it spoke.
pub const VERSION: u32 = 1;

/// The first line each way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "kebab-case")]
enum First {
    Hello { version: u32 },
}

/// The hello of a side that speaks `version`: its first line.
pub fn hello_of(version: u32) -> String {
    line(&First::Hello { version })
}

/// This program's hello.
pub fn hello() -> String {
    hello_of(VERSION)
}

/// The version a first line says; `None`: it is no hello, its side speaks
/// something else.
pub fn said_version(first: &str) -> Option<u32> {
    match serde_json::from_str(first) {
        Ok(First::Hello { version }) => Some(version),
        Err(_) => None,
    }
}

/// `message` as its line, without the line break.
pub(crate) fn line<T: Serialize>(message: &T) -> String {
    // Nothing a message holds fails to be written (strings, numbers,
    // lists; the round trip of every kind is tested): an empty line, which
    // the other side skips, stands for what cannot happen.
    serde_json::to_string(message).unwrap_or_default()
}

/// The message `line` says. What the error says never holds what the line
/// carries (a clip, a title, a note): its kind, and where it stops
/// reading.
pub(crate) fn read<T: DeserializeOwned>(line: &str) -> Result<T, BadLine> {
    /// A line's kind, whatever it carries.
    #[derive(Deserialize)]
    struct Kind {
        kind: String,
    }
    serde_json::from_str(line).map_err(|err| {
        let Ok(Kind { kind }) = serde_json::from_str(line) else {
            return BadLine(format!("not a message (column {})", err.column()));
        };
        let said = err.to_string();
        if said.starts_with("unknown variant") && said.contains(&format!("`{kind}`")) {
            return BadLine(format!("a kind this side does not know: {kind:?}"));
        }
        // Which field is missing or unknown names nothing private; a value
        // of the wrong type would be quoted.
        if said.starts_with("missing field") || said.starts_with("unknown variant") {
            let what = said.split(" at line").next().unwrap_or_default();
            return BadLine(format!("{kind:?}: {what}"));
        }
        BadLine(format!("{kind:?} does not read (column {})", err.column()))
    })
}

/// A clip's bytes as base64, [`crate::CLIP_MAX`] at most.
pub(crate) mod base64 {
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub fn serialize<S: Serializer>(data: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&base64::engine::general_purpose::STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        // Four characters for three bytes: refused before it is decoded.
        if text.len() / 4 * 3 > crate::CLIP_MAX + 3 {
            return Err(D::Error::custom("a clip too large"));
        }
        let data = base64::engine::general_purpose::STANDARD
            .decode(text)
            .map_err(|_| D::Error::custom("a clip's data"))?;
        if data.len() > crate::CLIP_MAX {
            return Err(D::Error::custom("a clip too large"));
        }
        Ok(data)
    }
}
