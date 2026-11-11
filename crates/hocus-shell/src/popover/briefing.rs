//! The secretary's briefing (bar.md §7 B, 4.6), opened by a click on her
//! at the head of the agents' group, beside the bar, 320 px wide, its top
//! on her row's; it never opens by itself. In this order:
//!
//! - her head and "Le point" (the clock is just above: no time, #503);
//! - "T'ATTEND": each agent waiting, since when, and what it asks, as its
//!   tool said it (a request "Autoriser Bash : npm install ?", a question,
//!   a notification's text); one whose tool says nothing (herdr's) shows
//!   its name and since when only;
//! - "BLOQUÉ": each agent stuck, since when, and why, from its failure's
//!   message;
//! - "À VOIR": each agent finished and not seen yet, and since when;
//! - in the mini form, where it also opens by hovering her item (4.7):
//!   "AU TRAVAIL", each agent working and since when, and "AU REPOS", each
//!   agent resting;
//! - the divider, how many work and rest, and "Sources", which opens the
//!   Sources popover in its place.
//!
//! The next meeting and the notifications come with the calendar and
//! sprint 5: nothing shows of them before they exist. A section with no
//! agent does not show. Each agent's line goes to it as its row does
//! (§8, 4.3): the popover closes and the streak runs from the line. `Tab`
//! goes through the lines and "Sources", `Enter` or `Space` acts, `Esc`
//! closes.
//!
//! What a tool said may hold code or a secret: it is drawn, never logged
//! (its length is).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_a11y::Role;
use hocus_link::{Form, Note};
use iced::{
    Color, Element, Length, Padding, Point, Rectangle, Size,
    alignment::Vertical,
    widget::{Space, button, column, container, mouse_area, row, space, svg, text},
    window,
};

use super::{
    Event as Seen, Item, event,
    kit::{self, FILL},
};
use crate::pandas::{self, Burst};
use crate::theme::Ink;
use crate::{
    agents::{self, Brief, Entry, Look, Words},
    clock::time::Lang,
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb},
    ui::{
        Around, control,
        parts::{gap, moving, spaced_by},
        style::{self, boxed, said},
    },
};

type El<'a> = Element<'a, Msg>;

// --- The mockup's sizes, at ×1 --------------------------------------------------

/// Its width (§9: the briefing's).
pub const WIDTH: u32 = 320;
const BORDER: f32 = 1.0;
const PAD_TOP: f32 = 12.0;
const PAD_SIDE: f32 = 14.0;
const PAD_BOTTOM: f32 = 14.0;
const LEFT: f32 = BORDER + PAD_SIDE;
const RIGHT: f32 = WIDTH as f32 - BORDER - PAD_SIDE;
const ROOM: f32 = RIGHT - LEFT;
/// Its parts 8 px apart.
const GAP: f32 = 8.0;
/// The header: her head 34 px, 10 px before the title (16 px, 700).
const HEAD: f32 = 34.0;
const HEAD_GAP: f32 = 10.0;
const TITLE: f32 = 16.0;
/// A section's label: 11 px, 700, spaced 0.08 em, in its colour, 4 px
/// more above; its lines 2 px apart.
const LABEL: f32 = 11.0;
const LABEL_SPACING: f32 = 0.08;
const LABEL_PAD: f32 = 4.0;
const LINE_GAP: f32 = 2.0;
/// An agent's line: 5 px in, 6 px past the sides, radius 7; its panda
/// (48 px, #649) 10 px before its name (13 px, 700) and since when
/// (`text.faint`) 6 px apart, over what it asks or why it failed
/// (12.5 px, `text.dim`), 1 px apart, in the panda's middle.
const LINE_PAD: f32 = 5.0;
const PANDA: f32 = 48.0;
const PANDA_GAP: f32 = 10.0;
const TEXT_LEFT: f32 = LEFT + PANDA + PANDA_GAP;
const TEXT_ROOM: f32 = RIGHT - TEXT_LEFT;
const LINE_OUT: f32 = 6.0;
const LINE_RADIUS: f32 = 7.0;
const WHO: f32 = 13.0;
const WHEN_GAP: f32 = 6.0;
const WHAT: f32 = 12.5;
const WHAT_GAP: f32 = 1.0;
/// The divider, 2 px from what is around it; the footer 12.5 px.
const RULE_PAD: f32 = 2.0;
const RULE: Ink = Ink::Dark(Color::from_rgba8(255, 255, 255, 0.08));
const FOOTER: f32 = 12.5;
/// "Sources" lit around it, for the pointer.
const LINK_PAD: f32 = 6.0;
/// The sections' colours: the rows' words'.
const WAITING: Ink = Ink::Dark(rgb(0xF5C46E));
const STUCK: Ink = Ink::Dark(rgb(0xF7A77A));
const TO_SEE: Ink = Ink::Dark(rgb(0x7EE0A0));
/// The mini form's last two (4.7): `text.dim`, and the resting rows' word.
const AT_WORK: Ink = DIM;
const RESTING: Ink = Ink::Dark(rgb(0x6A7384));

/// What can be clicked in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Agent(u64),
    Sources,
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    fn id(self) -> String {
        format!("briefing:{self:?}")
    }
}

/// A section, laid out.
#[derive(Debug, Clone, PartialEq)]
struct Section {
    look: Look,
    label_middle: f32,
    lines: Vec<Line>,
}

/// An agent's line, laid out: where it is, its name and since when cut to
/// fit, what its tool said (none: it said nothing).
#[derive(Clone, PartialEq)]
struct Line {
    id: u64,
    rect: Rectangle,
    who: String,
    when: String,
    when_x: f32,
    what: Option<String>,
    /// The note's length, for the log.
    said: Option<(&'static str, usize)>,
}

impl std::fmt::Debug for Line {
    // What it asks may hold a secret: its length only.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Line")
            .field("id", &self.id)
            .field("who", &self.who)
            .field("said", &self.said)
            .finish_non_exhaustive()
    }
}

/// The briefing, laid out from its top left.
#[derive(Debug, Clone, PartialEq)]
struct Laid {
    mood: agents::Mood,
    sections: Vec<Section>,
    rule: f32,
    footer: String,
    footer_middle: f32,
    sources: Rectangle,
    end: f32,
}

/// Whole minutes from `since` to `now` (microseconds of
/// `CLOCK_MONOTONIC`).
fn minutes(now: u64, since: u64) -> u64 {
    now.saturating_sub(since) / 60_000_000
}

/// `brief` laid out at `now` for a bar in `form`, `measure` giving a
/// text's width: in the mini form, who works and who rests too (4.7).
fn lay_out(
    brief: &Brief,
    words: Words,
    now: u64,
    form: Form,
    mut measure: impl FnMut(&str, f32, iced::Font) -> f32,
) -> Laid {
    let mut y = BORDER + PAD_TOP + HEAD;
    let mut sections = Vec::new();
    let mini = form == Form::Mini;
    for (look, entries) in [
        (Look::Waiting, &brief.waiting),
        (Look::Stuck, &brief.stuck),
        (Look::ToSee, &brief.to_see),
        (Look::Working, &brief.working),
        (Look::Resting, &brief.resting),
    ] {
        let shown = mini || !matches!(look, Look::Working | Look::Resting);
        if entries.is_empty() || !shown {
            continue;
        }
        y += GAP;
        let label_middle = y + LABEL_PAD + LABEL * LINE / 2.0;
        y += LABEL_PAD + LABEL * LINE;
        let lines = entries
            .iter()
            .map(|entry| {
                y += LINE_GAP;
                let line = line(entry, look, words, now, y, &mut measure);
                y += line.rect.height;
                line
            })
            .collect();
        sections.push(Section {
            look,
            label_middle,
            lines,
        });
    }
    y += GAP + RULE_PAD;
    let rule = y;
    y += 1.0 + RULE_PAD + GAP;
    let footer_middle = y + FOOTER * LINE / 2.0;
    let sources_width = measure(words.sources(), FOOTER, MANROPE);
    let sources = Rectangle::new(
        Point::new(RIGHT - sources_width - LINK_PAD, footer_middle - 12.0),
        Size::new(sources_width + 2.0 * LINK_PAD, 24.0),
    );
    let footer = kit::elide(&words.footer(brief), sources.x - LEFT - GAP, |t| {
        measure(t, FOOTER, MANROPE)
    });
    y += FOOTER * LINE;
    Laid {
        mood: brief.mood(),
        sections,
        rule,
        footer,
        footer_middle,
        sources,
        end: y,
    }
}

/// `entry`'s line in its section of `look`, from `top`.
fn line(
    entry: &Entry,
    look: Look,
    words: Words,
    now: u64,
    top: f32,
    measure: &mut impl FnMut(&str, f32, iced::Font) -> f32,
) -> Line {
    // A resting agent's line says only its name.
    let when = match look {
        Look::Resting => String::new(),
        _ => words.since(minutes(now, entry.since), look == Look::ToSee),
    };
    let when_width = match look {
        Look::Resting => -WHEN_GAP,
        _ => measure(&when, WHO, MANROPE),
    };
    let who = kit::elide(&entry.name, TEXT_ROOM - WHEN_GAP - when_width, |t| {
        measure(t, WHO, MANROPE_BOLD)
    });
    let when_x = TEXT_LEFT + measure(&who, WHO, MANROPE_BOLD) + WHEN_GAP;
    // Only a waiting or a stuck agent's line says what its tool said; a
    // finished or a working one's says since when, a resting one's its
    // name alone.
    let note = entry
        .note
        .as_ref()
        .filter(|_| matches!(look, Look::Waiting | Look::Stuck));
    let what =
        note.map(|note| kit::elide(&words.note(note), TEXT_ROOM, |t| measure(t, WHAT, MANROPE)));
    let text = WHO * LINE + what.as_ref().map_or(0.0, |_| WHAT_GAP + WHAT * LINE);
    let height = 2.0 * LINE_PAD + text.max(PANDA);
    Line {
        id: entry.id,
        rect: Rectangle::new(
            Point::new(LEFT - LINE_OUT, top),
            Size::new(ROOM + 2.0 * LINE_OUT, height),
        ),
        who,
        when,
        when_x,
        what,
        said: note.map(|note| {
            let kind = match note {
                Note::Allow { .. } => "asks",
                Note::Says(_) => "says",
            };
            (kind, note.len())
        }),
    }
}

impl Laid {
    fn targets(&self) -> Vec<(Target, Rectangle)> {
        self.sections
            .iter()
            .flat_map(|s| s.lines.iter())
            .map(|l| (Target::Agent(l.id), l.rect))
            .chain([(Target::Sources, self.sources)])
            .collect()
    }

    /// What it shows, for the log: the notes by their length.
    fn said(&self, words: Words) -> String {
        let mut said: Vec<String> = Vec::new();
        for section in &self.sections {
            for line in &section.lines {
                let note = line.said.map_or(String::new(), |(kind, n)| {
                    format!(" ({kind}, {n} characters)")
                });
                let when = match line.when.as_str() {
                    "" => String::new(),
                    when => format!(" {when}"),
                };
                said.push(format!(
                    "{} {}{when}{note}",
                    words.section(section.look),
                    line.who
                ));
            }
        }
        said.push(self.footer.clone());
        said.join(" · ")
    }
}

/// Its height as it shows now beside a bar in `form`.
pub fn height(form: Form) -> u32 {
    let words = Words::new(Lang::of_messages());
    let brief = Brief::of(&agents::said().agents);
    (lay_out(&brief, words, agents::now_us(), form, kit::measure).end + PAD_BOTTOM + BORDER).round()
        as u32
}

// --- The panel ------------------------------------------------------------------

/// The briefing on its surface.
#[derive(Debug, Clone)]
pub struct Panel {
    pub id: window::Id,
    pub words: Words,
    pub glass: bool,
}

/// What the surface keeps from a build of its view to the next.
#[derive(Debug, Default)]
struct Ui {
    /// The agents as last laid out, the minute and the bar's form then.
    said: agents::Said,
    minute: u64,
    form: Option<Form>,
    laid: Option<Laid>,
    focus: Option<Target>,
    /// Its pandas playing their moods as it opens, and the one the pointer
    /// came over (#649).
    bursts: HashMap<u64, Burst>,
    opened: bool,
}

static UI: Mutex<Option<Ui>> = Mutex::new(None);

fn ui<T>(with: impl FnOnce(&mut Ui) -> T) -> T {
    let mut ui = UI.lock().unwrap_or_else(PoisonError::into_inner);
    with(ui.get_or_insert_with(Ui::default))
}

/// It opens: nothing holds the ring, its pandas play again.
pub fn opening() {
    ui(|ui| *ui = Ui::default());
}

/// What a widget asks.
#[derive(Debug, Clone)]
pub enum Msg {
    /// A line or "Sources" pressed, or `Enter` on it.
    Act(Target),
    /// The ring comes to it.
    Focus(Target),
    /// The pointer came over an agent's line: its panda plays.
    Over(u64),
    /// A press anywhere on the popover.
    Pressed,
    /// The agents changed under the view, or the minute did.
    Stale,
}

/// The briefing on its surface.
pub fn view<'a>(panel: &Panel) -> Element<'a, crate::Message> {
    let (id, words) = (panel.id, panel.words);
    Around::new(id, build(id, words), handle, Msg::Stale)
        .rebuild(move || build(id, words))
        .stale_when(move || {
            let form = super::with(|c| c.form_of(id));
            let minute = agents::now_us() / 60_000_000;
            ui(|ui| ui.minute != minute || ui.form != form)
        })
        .press(Msg::Pressed)
        .on_glass(panel.glass)
}

/// Its content, as the agents are now.
fn build<'a>(id: window::Id, words: Words) -> El<'a> {
    let form = super::with(|c| c.form_of(id)).unwrap_or(Form::Full);
    let said = agents::said();
    let now = agents::now_us();
    let laid = lay_out(&Brief::of(&said.agents), words, now, form, kit::measure);
    let at = Instant::now();
    ui(|ui| {
        if ui.laid.as_ref() != Some(&laid) {
            tracing::debug!(briefing = %laid.said(words), "the briefing shows it");
        }
        // As it opens, each line's panda plays its mood once (#649).
        if !ui.opened && pandas::lively() {
            for section in &laid.sections {
                for line in &section.lines {
                    ui.bursts
                        .insert(line.id, Burst::of(pandas::mood(section.look), at));
                }
            }
        }
        ui.opened = true;
        ui.bursts.retain(|_, b| b.plays(at));
        let targets: Vec<Target> = laid.targets().into_iter().map(|(t, _)| t).collect();
        if ui.focus.is_some_and(|f| !targets.contains(&f)) {
            ui.focus = None;
        }
        let content = content(&laid, words, &said, ui);
        ui.minute = now / 60_000_000;
        ui.form = Some(form);
        ui.said = said;
        ui.laid = Some(laid);
        content
    })
}

fn content<'a>(laid: &Laid, words: Words, said: &agents::Said, ui: &Ui) -> El<'a> {
    let head = svg(agents::draw::secretary(laid.mood).clone())
        .width(HEAD)
        .height(HEAD);
    let mut all = column![
        container(
            row![
                head,
                gap(HEAD_GAP),
                said_line(words.briefing(), TITLE, MANROPE_BOLD, TEXT.get()),
            ]
            .height(HEAD)
            .align_y(Vertical::Center)
        )
        .padding([0.0, LINE_OUT])
    ];
    for section in &laid.sections {
        let colour = match section.look {
            Look::Waiting => WAITING,
            Look::Stuck => STUCK,
            Look::ToSee => TO_SEE,
            Look::Working => AT_WORK,
            Look::Resting => RESTING,
        };
        let word = words.section(section.look);
        let mut chars = word.chars();
        let read = chars.next().map_or_else(String::new, |first| {
            first.to_string() + &chars.as_str().to_lowercase()
        });
        all = all.push(crate::ui::line(
            container(spaced_by(
                word,
                LABEL,
                MANROPE_BOLD,
                colour.get(),
                LABEL_SPACING,
            ))
            .padding(Padding {
                top: GAP + LABEL_PAD,
                right: 0.0,
                bottom: 0.0,
                left: LINE_OUT,
            })
            .height(GAP + LABEL_PAD + LABEL * LINE)
            .align_y(Vertical::Bottom),
            read,
        ));
        for line in &section.lines {
            all = all.push(Space::new().height(LINE_GAP)).push(line_of(
                section.look,
                line,
                words,
                said,
                ui,
            ));
        }
    }
    all = all
        .push(Space::new().height(GAP + RULE_PAD))
        .push(
            container(
                container(Space::new().width(Length::Fill).height(1.0))
                    .style(|_| boxed(RULE.get(), 0.0)),
            )
            .padding([0.0, LINE_OUT]),
        )
        // "Sources" reaches 3 px above the footer's line.
        .push(Space::new().height(RULE_PAD + GAP - (24.0 - FOOTER * LINE) / 2.0));
    let sources = Target::Sources;
    let link = button(
        container(
            text(words.sources())
                .size(FOOTER)
                .font(MANROPE)
                .line_height(iced::widget::text::LineHeight::Relative(LINE))
                .shaping(iced::widget::text::Shaping::Advanced)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .center_y(Length::Fill),
    )
    .padding([0.0, LINK_PAD])
    .height(24.0)
    .style(|_, status| {
        let lit = style::lit(status);
        iced::widget::button::Style {
            background: lit.then(|| FILL.get().into()),
            text_color: if lit { TEXT.get() } else { FAINT.get() },
            border: style::round(LINE_RADIUS),
            ..iced::widget::button::Style::default()
        }
    })
    .on_press(Msg::Act(sources));
    let link = control(link, sources.id(), Role::Button, words.sources())
        .focus(ui.focus == Some(sources), Msg::Focus(sources), LINE_RADIUS)
        .activate(Msg::Act(sources))
        .rows(0);
    all = all.push(
        row![
            gap(LINE_OUT),
            crate::ui::line(
                said_line(laid.footer.clone(), FOOTER, MANROPE, FAINT.get()),
                laid.footer.clone()
            ),
            space::horizontal(),
            link,
        ]
        .height(24.0)
        .align_y(Vertical::Center),
    );
    container(all)
        .padding(Padding {
            top: BORDER + PAD_TOP,
            right: LEFT - LINE_OUT,
            bottom: 0.0,
            left: LEFT - LINE_OUT,
        })
        .into()
}

fn said_line<'a>(content: impl Into<String>, size: f32, font: iced::Font, ink: Color) -> El<'a> {
    said(content.into(), size, font, ink).into()
}

/// An agent's line: its panda, its name, since when, what its tool said;
/// lit under the pointer, which replays its panda.
fn line_of<'a>(look: Look, line: &Line, words: Words, said: &agents::Said, ui: &Ui) -> El<'a> {
    let burst = ui.bursts.get(&line.id).copied();
    let alpha = if look == Look::Resting { 0.55 } else { 1.0 };
    let panda = moving(
        PANDA,
        PANDA,
        move |f, s, now| {
            agents::draw::draw_panda(f, Rectangle::new(Point::ORIGIN, s), look, burst, now, alpha);
        },
        move |now| burst.is_some_and(|b| b.plays(now)),
    );
    let mut first = row![said_line(line.who.clone(), WHO, MANROPE_BOLD, TEXT.get())];
    if !line.when.is_empty() {
        first =
            first
                .push(gap(WHEN_GAP))
                .push(said_line(line.when.clone(), WHO, MANROPE, FAINT.get()));
    }
    let mut texts = column![first];
    if let Some(what) = &line.what {
        texts = texts
            .push(Space::new().height(WHAT_GAP))
            .push(crate::ui::unsaid(said_line(
                what.clone(),
                WHAT,
                MANROPE,
                DIM.get(),
            )));
    }
    let target = Target::Agent(line.id);
    let pressed = button(
        row![panda, gap(PANDA_GAP), texts]
            .height(Length::Fill)
            .align_y(Vertical::Center),
    )
    .padding([0.0, LINE_OUT])
    .width(Length::Fill)
    .height(line.rect.height)
    .style(|_, status| {
        if style::lit(status) {
            style::hovered(FILL.get(), LINE_RADIUS)
        } else {
            style::clear()
        }
    })
    .on_press(Msg::Act(target));
    // Its name whole, since when, what its tool said, for screen readers.
    let agent = said.agents.iter().find(|a| a.id == line.id);
    let name = agent.map_or_else(|| line.who.clone(), |a| crate::a11y::plain(&a.name));
    let when = line.when.trim_start_matches('·').trim();
    let name = if when.is_empty() {
        name
    } else {
        format!("{name}, {when}")
    };
    let note = agent
        .and_then(|a| a.note.as_ref())
        .filter(|_| matches!(look, Look::Waiting | Look::Stuck))
        .map(|n| words.note(n));
    let declared = control(
        mouse_area(pressed).on_enter(Msg::Over(line.id)),
        target.id(),
        Role::Button,
        name,
    )
    .node(|n| match note {
        Some(note) => n.describe(note),
        None => n,
    })
    .focus(ui.focus == Some(target), Msg::Focus(target), LINE_RADIUS)
    .activate(Msg::Act(target))
    .rows(0);
    declared.into()
}

// --- What a message does -------------------------------------------------------

fn handle(message: Msg) {
    match message {
        Msg::Stale => {}
        Msg::Pressed => ui(|ui| ui.focus = None),
        Msg::Focus(target) => ui(|ui| ui.focus = Some(target)),
        Msg::Over(id) => ui(|ui| {
            // The pointer coming over a line replays its panda's.
            let now = Instant::now();
            let look = ui.laid.as_ref().and_then(|laid| {
                laid.sections
                    .iter()
                    .find(|s| s.lines.iter().any(|l| l.id == id))
                    .map(|s| s.look)
            });
            if let Some(look) = look
                && pandas::lively()
                && !ui.bursts.get(&id).is_some_and(|b| b.plays(now))
            {
                ui.bursts.insert(id, Burst::of(pandas::mood(look), now));
            }
        }),
        Msg::Act(target) => act(target),
    }
    super::refit(Item::Secretary);
}

fn act(target: Target) {
    tracing::debug!(?target, "the briefing");
    match target {
        Target::Agent(id) => {
            let row = ui(|ui| {
                ui.laid.as_ref().and_then(|laid| {
                    laid.targets()
                        .into_iter()
                        .find(|(t, _)| *t == target)
                        .map(|(_, r)| r)
                })
            });
            if let Some(row) = row {
                event(Seen::GoTo { id, row });
            }
        }
        Target::Sources => event(Seen::Replace(Item::Sources)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, name: &str, note: Option<Note>) -> Entry {
        Entry {
            id,
            name: name.into(),
            since: 0,
            note,
        }
    }

    #[test]
    fn its_sections_show_only_with_agents_in_them() {
        let measure = |t: &str, size: f32, _font: iced::Font| t.chars().count() as f32 * size * 0.5;
        let words = Words::new(Lang::French);
        let brief = Brief {
            waiting: vec![
                entry(1, "site-web", Some(Note::allow("Bash", "npm install"))),
                entry(2, "herdr-agent", None),
            ],
            to_see: vec![entry(3, "api", Some(Note::allow("x", "y")))],
            working: vec![entry(4, "build", None), entry(5, "docs", None)],
            ..Brief::default()
        };
        let laid = lay_out(&brief, words, 120_000_000, Form::Full, measure);
        let looks: Vec<Look> = laid.sections.iter().map(|s| s.look).collect();
        assert_eq!(looks, [Look::Waiting, Look::ToSee]);
        let [asks, bare] = &laid.sections[0].lines[..] else {
            panic!("{:?}", laid.sections);
        };
        // A line with what it asks, under its name: taller by its text.
        assert_eq!(
            asks.what.as_deref(),
            Some("Autoriser Bash\u{a0}: npm install\u{a0}?")
        );
        assert_eq!(asks.when, "· 2\u{a0}min");
        assert_eq!(bare.what, None);
        // Each line as tall as its 48 px panda (#649), 5 px in.
        assert_eq!(asks.rect.height, 58.0);
        assert_eq!(bare.rect.height, 58.0);
        assert_eq!(TEXT_LEFT, LEFT + 58.0);
        // A finished one: since when only.
        let done = &laid.sections[1].lines[0];
        assert_eq!(
            (done.what.as_ref(), done.when.as_str()),
            (None, "· il y a 2\u{a0}min")
        );
        // Its lines 2 px apart, 6 px past the sides; "Sources" last.
        assert_eq!(bare.rect.y, asks.rect.y + asks.rect.height + 2.0);
        assert_eq!(asks.rect.x, LEFT - 6.0);
        let targets: Vec<Target> = laid.targets().into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            targets,
            [
                Target::Agent(1),
                Target::Agent(2),
                Target::Agent(3),
                Target::Sources
            ]
        );
        assert_eq!(laid.footer, "2 au travail · 0 au repos");
        // The log says the notes' lengths, never their text.
        let said = laid.said(words);
        assert!(
            said.contains("site-web · 2\u{a0}min (asks, 15 characters)"),
            "{said}"
        );
        assert!(!said.contains("npm"), "{said}");
        // Nothing to say: the header, the divider and the footer.
        let calm = lay_out(&Brief::default(), words, 0, Form::Full, measure);
        assert!(calm.sections.is_empty());
        assert_eq!(calm.rule, BORDER + PAD_TOP + HEAD + GAP + RULE_PAD);
    }

    #[test]
    fn in_the_mini_form_it_lists_who_works_and_who_rests_last() {
        const MIN: u64 = 60_000_000;
        let measure = |t: &str, size: f32, _font: iced::Font| t.chars().count() as f32 * size * 0.5;
        let words = Words::new(Lang::English);
        let brief = Brief {
            waiting: vec![entry(
                1,
                "site-web",
                Some(Note::allow("Bash", "npm install")),
            )],
            working: vec![entry(2, "build", Note::says("Running the tests"))],
            resting: vec![entry(3, "notes", Note::says("Done"))],
            ..Brief::default()
        };
        let looks = |laid: &Laid| laid.sections.iter().map(|s| s.look).collect::<Vec<_>>();
        // The full form's briefing counts them in its footer only.
        let full = lay_out(&brief, words, 8 * MIN, Form::Full, measure);
        assert_eq!(looks(&full), [Look::Waiting]);
        let mini = lay_out(&brief, words, 8 * MIN, Form::Mini, measure);
        assert_eq!(looks(&mini), [Look::Waiting, Look::Working, Look::Resting]);
        assert_eq!(words.section(Look::Working), "AT WORK");
        assert_eq!(Words::new(Lang::French).section(Look::Resting), "AU REPOS");
        // Who works and since when; who rests, its name alone; neither
        // says what its tool said.
        let working = &mini.sections[1].lines[0];
        assert_eq!(
            (working.when.as_str(), working.what.as_deref()),
            ("· 8\u{a0}min", None)
        );
        let resting = &mini.sections[2].lines[0];
        assert_eq!((resting.when.as_str(), resting.what.as_deref()), ("", None));
        assert_eq!(resting.rect.height, working.rect.height);
        // Each line goes to its agent; "Sources" last; the footer as in
        // the full form.
        let targets: Vec<Target> = mini.targets().into_iter().map(|(t, _)| t).collect();
        assert_eq!(
            targets,
            [
                Target::Agent(1),
                Target::Agent(2),
                Target::Agent(3),
                Target::Sources
            ]
        );
        assert_eq!(mini.footer, full.footer);
        assert!(mini.end > full.end);
    }
}
