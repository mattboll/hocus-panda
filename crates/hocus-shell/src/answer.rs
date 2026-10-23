//! An AI's answer (#44), and the card it is shown on: a panda with what
//! answers, the text as it streams in (paragraphs, lists, code), and the
//! buttons that act on it. The command field shows it; circling to ask
//! (#46) reuses the same card. An intent's confirmation is drawn on it too.

use std::time::Duration;

use hocus_ui::{
    Align, ButtonKind, Direction, Insets, Kind, Length, Look, NodeId, Paint, Style, TextStyle,
    Theme, Tree, Typeface,
};

use crate::{
    i18n::Lang,
    panda::{Character, Mood, Panda},
};

/// Who answers, as the card says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A model on this machine: the question stays here.
    Local,
    /// A provider elsewhere, by name: "Claude", "OpenAI".
    Named(String),
}

impl Source {
    pub fn label(&self, lang: Lang) -> String {
        match self {
            Self::Local => lang.pick("Local AI", "IA locale").to_owned(),
            Self::Named(name) => name.clone(),
        }
    }
}

/// Where an answer is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Streaming,
    /// All there, after so long.
    Done(Duration),
    /// It stopped, and why, in the user's words.
    Failed(String),
}

/// An answer, as far as it came.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub question: String,
    pub source: Source,
    pub text: String,
    pub state: State,
}

/// What comes for an answer from the worker that asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum News {
    /// More of the text.
    Text(String),
    Done(Duration),
    Failed(String),
}

impl Answer {
    pub fn new(question: impl Into<String>, source: Source) -> Self {
        Self {
            question: question.into(),
            source,
            text: String::new(),
            state: State::Streaming,
        }
    }

    pub fn apply(&mut self, news: News) {
        match news {
            News::Text(more) if self.state == State::Streaming => self.text.push_str(&more),
            News::Text(_) => {}
            News::Done(took) => self.state = State::Done(took),
            News::Failed(why) => self.state = State::Failed(why),
        }
    }

    /// "Answer · Local AI · 3 s", as the card's caption.
    pub fn caption(&self, lang: Lang) -> String {
        let source = self.source.label(lang);
        match &self.state {
            State::Streaming => format!("{} · {source}", lang.pick("Answer", "Réponse")),
            State::Done(took) => format!(
                "{} · {source} · {}\u{a0}s",
                lang.pick("Answer", "Réponse"),
                took.as_secs()
            ),
            State::Failed(_) => format!("{} · {source}", lang.pick("No answer", "Pas de réponse")),
        }
    }

    pub fn mood(&self) -> Mood {
        match self.state {
            State::Streaming => Mood::Work,
            State::Done(_) => Mood::Done,
            State::Failed(_) => Mood::Error,
        }
    }

    /// The text without its Markdown marks, for the clipboard and notes.
    pub fn plain(&self) -> String {
        blocks(&self.text)
            .iter()
            .map(|b| match b {
                Block::Paragraph(t) | Block::Item(t) | Block::Code(t) => t.as_str(),
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// How a model is asked to answer in the field: briefly, plainly, in the
/// user's language. Not shown.
pub fn instructions(lang: Lang) -> String {
    let language = lang.pick("English", "French");
    format!(
        "You answer in the command field of a Linux desktop. Answer in {language}, \
         briefly: one or two sentences first, then at most a few short list items \
         or a short code block when they help. No headings, no tables."
    )
}

/// A piece of an answer, as the card lays it out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Paragraph(String),
    /// A list item, its number or bullet kept.
    Item(String),
    Code(String),
}

/// Inline marks taken out: bold, italics, code, links' targets.
fn inline(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' | '`' => {}
            '_' if chars.peek() == Some(&'_') => {
                chars.next();
            }
            '[' => {
                // [label](target): the label.
                let label: String = chars.by_ref().take_while(|c| *c != ']').collect();
                out.push_str(&label);
                if chars.peek() == Some(&'(') {
                    for c in chars.by_ref() {
                        if c == ')' {
                            break;
                        }
                    }
                }
            }
            c => out.push(c),
        }
    }
    out
}

/// The answer's blocks: paragraphs, list items and code, from the little
/// Markdown models write. An unclosed code block (still streaming) is
/// code to the end.
pub fn blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut code: Option<Vec<&str>> = None;
    let flush = |paragraph: &mut Vec<&str>, out: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            out.push(Block::Paragraph(inline(&paragraph.join(" "))));
            paragraph.clear();
        }
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            match code.take() {
                Some(lines) => out.push(Block::Code(lines.join("\n"))),
                None => {
                    flush(&mut paragraph, &mut out);
                    code = Some(Vec::new());
                }
            }
            continue;
        }
        if let Some(lines) = code.as_mut() {
            lines.push(line);
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut out);
            continue;
        }
        let heading = trimmed.trim_start_matches('#');
        if heading.len() != trimmed.len() && heading.starts_with(' ') {
            flush(&mut paragraph, &mut out);
            out.push(Block::Paragraph(inline(heading.trim())));
            continue;
        }
        let bullet = ["- ", "* ", "• "]
            .iter()
            .find_map(|b| trimmed.strip_prefix(b))
            .map(|rest| format!("• {}", inline(rest.trim())));
        let numbered = trimmed
            .split_once(". ")
            .filter(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
            .map(|(n, rest)| format!("{n}. {}", inline(rest.trim())));
        if let Some(item) = bullet.or(numbered) {
            flush(&mut paragraph, &mut out);
            out.push(Block::Item(item));
            continue;
        }
        paragraph.push(trimmed);
    }
    flush(&mut paragraph, &mut out);
    if let Some(lines) = code {
        out.push(Block::Code(lines.join("\n")));
    }
    out
}

/// What a card shows.
#[derive(Debug, Clone)]
pub struct Card {
    pub mood: Mood,
    /// In capitals, in the accent: "Answer · Local AI · 3 s".
    pub caption: String,
    pub blocks: Vec<Block>,
    /// Small facts as chips under the text.
    pub chips: Vec<String>,
    /// Said in the error colour.
    pub error: Option<String>,
    /// The first is the main one.
    pub buttons: Vec<String>,
}

/// What [`fill`] made: the buttons in order, and the box of the text, to
/// cut a long answer.
#[derive(Debug, Clone)]
pub struct Filled {
    pub buttons: Vec<NodeId>,
    pub text: NodeId,
}

/// The size of the board's text against the theme's body text, so that
/// the user's text size applies.
fn sized(theme: &Theme, px: f32, weight: u16, line_height: f32, color: Paint) -> TextStyle {
    let scale = theme.text.body.size / 14.0;
    TextStyle {
        size: px * scale,
        weight,
        line_height,
        color,
        ..theme.text.body
    }
}

/// Fills `room` with `card`, as the command field's board draws it.
pub fn fill(tree: &mut Tree, room: NodeId, card: &Card) -> Filled {
    let theme = tree.theme().clone();
    let missing = || unreachable!("adding to a node of this tree");
    let add = |tree: &mut Tree, parent, style, kind| {
        tree.add(parent, style, kind).unwrap_or_else(missing)
    };
    let head = add(
        tree,
        room,
        Style {
            direction: Direction::Row,
            gap: 12.0,
            align: Align::Center,
            grow: 0.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    add(
        tree,
        head,
        Style {
            width: Length::Px(52.0),
            height: Length::Px(52.0),
            grow: 0.0,
            ..Style::default()
        },
        Kind::Custom {
            draw: Box::new(Panda::themed(Character::Apprentice, card.mood, &theme)),
            label: None,
        },
    );
    add(
        tree,
        head,
        Style::default(),
        Kind::text(
            card.caption.to_uppercase(),
            sized(&theme, 13.0, 700, 1.3, Paint::Accent),
        ),
    );
    let text = add(
        tree,
        room,
        Style {
            gap: 8.0,
            clip: true,
            grow: 0.0,
            ..Style::default()
        },
        Kind::Box(Look::default()),
    );
    let mut first = true;
    for block in &card.blocks {
        match block {
            Block::Paragraph(t) => {
                let style = if first {
                    sized(&theme, 19.0, 500, 1.55, Paint::Text)
                } else {
                    sized(&theme, 15.0, 500, 1.5, Paint::Text)
                };
                add(tree, text, Style::default(), Kind::text(t.as_str(), style));
            }
            Block::Item(t) => {
                add(
                    tree,
                    text,
                    Style::default(),
                    Kind::text(t.as_str(), sized(&theme, 15.0, 500, 1.5, Paint::Muted)),
                );
            }
            Block::Code(t) => {
                let code = add(
                    tree,
                    text,
                    Style {
                        padding: Insets::symmetric(14.0, 16.0),
                        ..Style::default()
                    },
                    Kind::Box(Look {
                        background: Some(Paint::Background),
                        radius: 12.0,
                        ..Look::default()
                    }),
                );
                add(
                    tree,
                    code,
                    Style::default(),
                    Kind::text(
                        t.as_str(),
                        TextStyle {
                            typeface: Typeface::Mono,
                            ..sized(&theme, 13.0, 400, 1.7, Paint::Muted)
                        },
                    ),
                );
            }
        }
        first = false;
    }
    if let Some(error) = &card.error {
        add(
            tree,
            text,
            Style::default(),
            Kind::text(error.as_str(), sized(&theme, 15.0, 500, 1.5, Paint::Error)),
        );
    }
    if !card.chips.is_empty() {
        let chips = add(
            tree,
            room,
            Style {
                direction: Direction::Row,
                gap: 8.0,
                grow: 0.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        for chip in &card.chips {
            let b = add(
                tree,
                chips,
                Style {
                    padding: Insets::symmetric(6.0, 12.0),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::Box(Look {
                    background: Some(Paint::SurfaceHover),
                    radius: 8.0,
                    ..Look::default()
                }),
            );
            add(
                tree,
                b,
                Style::default(),
                Kind::text(chip.as_str(), theme.text.label.with_color(Paint::Muted)),
            );
        }
    }
    let mut buttons = Vec::new();
    if !card.buttons.is_empty() {
        let row = add(
            tree,
            room,
            Style {
                direction: Direction::Row,
                gap: 10.0,
                grow: 0.0,
                ..Style::default()
            },
            Kind::Box(Look::default()),
        );
        for (i, label) in card.buttons.iter().enumerate() {
            let kind = if i == 0 {
                ButtonKind::Primary
            } else {
                ButtonKind::Plain
            };
            buttons.push(add(
                tree,
                row,
                Style {
                    height: Length::Px(48.0),
                    padding: Insets::symmetric(0.0, 20.0),
                    grow: 0.0,
                    ..Style::default()
                },
                Kind::button(label.as_str(), kind, &theme),
            ));
        }
    }
    Filled { buttons, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_paragraphs_items_and_code() {
        let text = "The **cache** is off since `9:12`.\n\n1. No hits.\n2. The old [variable](https://x).\n- a bullet\n\n```sh\npkg install x\n```\nAfter.";
        assert_eq!(
            blocks(text),
            vec![
                Block::Paragraph("The cache is off since 9:12.".into()),
                Block::Item("1. No hits.".into()),
                Block::Item("2. The old variable.".into()),
                Block::Item("• a bullet".into()),
                Block::Code("pkg install x".into()),
                Block::Paragraph("After.".into()),
            ]
        );
        // Still streaming: an open code block is code.
        assert_eq!(
            blocks("Run:\n```\ncargo b"),
            vec![
                Block::Paragraph("Run:".into()),
                Block::Code("cargo b".into())
            ]
        );
    }

    #[test]
    fn the_caption_says_who_answered_and_how_long_it_took() {
        let mut a = Answer::new("why?", Source::Local);
        a.apply(News::Text("Because.".into()));
        assert_eq!(a.caption(Lang::Fr), "Réponse · IA locale");
        a.apply(News::Done(Duration::from_millis(3_400)));
        assert_eq!(a.caption(Lang::Fr), "Réponse · IA locale · 3\u{a0}s");
        a.apply(News::Text("late".into()));
        assert_eq!(a.text, "Because.", "nothing after the end");
        assert_eq!(a.mood(), Mood::Done);
    }
}
