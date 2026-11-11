//! The texts the messages carry as the other side shows them: on one
//! line, trimmed, a title no longer than its list ever shows. A message is
//! written through [`tidy`], whatever its sender gave it.

use std::fmt;

/// The characters of a title a [`crate::ToShell::Titles`] line carries at most:
/// the list cuts titles to its 190 px long before that.
pub const TITLE_MAX: usize = 200;

/// A title as a [`crate::ToShell::Titles`] line carries it: on one line, its
/// control characters made spaces, trimmed, [`TITLE_MAX`] characters at
/// most.
pub fn title_text(title: &str) -> String {
    let title: String = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(TITLE_MAX)
        .collect();
    title.trim().to_owned()
}

/// A name as a line carries it (a screen's in [`crate::ToShell::Screens`], an
/// agent's): on one line, its control characters made spaces, trimmed.
pub fn screen_name(name: &str) -> String {
    let name: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    name.trim().to_owned()
}

/// A line of the screen readers' tree as a [`crate::ToCompositor::A11yLine`]
/// carries it: its control characters made spaces, its indent kept.
pub fn a11y_text(line: &str) -> String {
    line.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim_end()
        .to_owned()
}

/// A line that does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLine(pub String);

impl fmt::Display for BadLine {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BadLine {}

/// How the messages' texts are written (`serialize_with`): a text the
/// other side shows goes tidied, so that a window's title with a line
/// break in it never draws on two lines of a list.
pub(crate) mod tidy {
    use serde::Serializer;

    use super::{a11y_text, screen_name, title_text};

    /// A name ([`screen_name`]).
    pub fn name<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&screen_name(text))
    }

    pub fn names<S: Serializer>(texts: &[String], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(texts.iter().map(|text| screen_name(text)))
    }

    /// A title ([`title_text`]).
    pub fn title<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&title_text(text))
    }

    pub fn titles<S: Serializer>(texts: &[String], serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(texts.iter().map(|text| title_text(text)))
    }

    /// Windows' titles by their ids.
    pub fn titles_by_id<S: Serializer>(
        titles: &[(u64, String)],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.collect_seq(titles.iter().map(|(id, title)| (id, title_text(title))))
    }

    /// A line of the screen readers' tree ([`a11y_text`]).
    pub fn a11y<S: Serializer>(text: &str, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&a11y_text(text))
    }

    /// A program's path, as it reads: on one line.
    pub fn path<S: Serializer>(path: &std::path::Path, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&screen_name(&path.to_string_lossy()))
    }
}
