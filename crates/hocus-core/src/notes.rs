//! Notes and to-dos (#79): a plain Markdown file, one line each. `- [ ] …`
//! is a to-do, `- [x] …` a done one, any other text a note; lines it does
//! not understand (headings, blank lines) are kept as they were.

/// One line of the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Todo {
        done: bool,
        text: String,
    },
    Note(String),
    /// Kept as written: headings, blank lines.
    Other(String),
}

impl Line {
    fn parse(raw: &str) -> Self {
        let trimmed = raw.trim_start();
        for (prefix, done) in [
            ("- [ ] ", false),
            ("- [x] ", true),
            ("- [X] ", true),
            ("* [ ] ", false),
            ("* [x] ", true),
        ] {
            if let Some(text) = trimmed.strip_prefix(prefix) {
                return Self::Todo {
                    done,
                    text: text.trim_end().to_owned(),
                };
            }
        }
        if let Some(text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            return Self::Note(text.trim_end().to_owned());
        }
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('>') {
            return Self::Other(raw.to_owned());
        }
        Self::Note(raw.trim().to_owned())
    }

    fn markdown(&self) -> String {
        match self {
            Self::Todo { done: false, text } => format!("- [ ] {text}"),
            Self::Todo { done: true, text } => format!("- [x] {text}"),
            Self::Note(text) => format!("- {text}"),
            Self::Other(raw) => raw.clone(),
        }
    }

    /// What it says, without its Markdown.
    pub fn text(&self) -> &str {
        match self {
            Self::Todo { text, .. } | Self::Note(text) => text,
            Self::Other(raw) => raw,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Notes {
    pub lines: Vec<Line>,
}

impl Notes {
    pub fn parse(text: &str) -> Self {
        Self {
            lines: text.lines().map(Line::parse).collect(),
        }
    }

    pub fn to_markdown(&self) -> String {
        let mut out: String = self
            .lines
            .iter()
            .map(Line::markdown)
            .collect::<Vec<_>>()
            .join("\n");
        out.push('\n');
        out
    }

    /// A line typed: a to-do when it says so (`- [ ] …`, `[] …`), else a
    /// note. Nothing for an empty one.
    pub fn add(&mut self, typed: &str) -> bool {
        let typed = typed.trim();
        if typed.is_empty() {
            return false;
        }
        let line = match typed
            .strip_prefix("[] ")
            .or_else(|| typed.strip_prefix("[ ] "))
        {
            Some(text) => Line::Todo {
                done: false,
                text: text.trim().to_owned(),
            },
            None => match Line::parse(typed) {
                Line::Other(raw) => Line::Note(raw.trim().to_owned()),
                line => line,
            },
        };
        self.lines.push(line);
        true
    }

    /// Ticks the to-do at `index`, or unticks it.
    pub fn toggle(&mut self, index: usize) -> bool {
        match self.lines.get_mut(index) {
            Some(Line::Todo { done, .. }) => {
                *done = !*done;
                true
            }
            _ => false,
        }
    }

    pub fn remove(&mut self, index: usize) -> bool {
        if index < self.lines.len() && !matches!(self.lines[index], Line::Other(_)) {
            self.lines.remove(index);
            true
        } else {
            false
        }
    }

    /// The to-dos not done, with their place in the file.
    pub fn open_todos(&self) -> Vec<(usize, &str)> {
        self.lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| match l {
                Line::Todo { done: false, text } => Some((i, text.as_str())),
                _ => None,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_reads_and_writes_back_as_it_was() {
        let text =
            "# Notes\n\n- [ ] buy bamboo\n- [x] call Léa\n- the wifi code is on the fridge\n";
        let notes = Notes::parse(text);
        assert_eq!(notes.to_markdown(), text);
        assert_eq!(notes.open_todos(), [(2, "buy bamboo")]);
    }

    #[test]
    fn typed_lines_become_todos_or_notes_and_tick() {
        let mut notes = Notes::default();
        assert!(notes.add("- [ ] buy bamboo"));
        assert!(notes.add("[] water the plant"));
        assert!(notes.add("remember the milk"));
        assert!(!notes.add("   "));
        assert!(notes.toggle(0));
        assert!(!notes.toggle(2), "a note does not tick");
        assert_eq!(
            notes.to_markdown(),
            "- [x] buy bamboo\n- [ ] water the plant\n- remember the milk\n"
        );
        assert!(notes.remove(1));
        assert_eq!(notes.open_todos(), []);
    }
}
