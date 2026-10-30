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

use std::time::Instant;

use hocus_link::{Form, Note};
use iced::{
    Color, Point, Rectangle, Renderer, Size, Theme,
    advanced::svg,
    keyboard, mouse,
    widget::{
        canvas::{self, Frame, Geometry},
        text::Alignment,
    },
    window,
};

use super::{
    Event as Seen, Item, Pressed, event,
    kit::{self, FILL, Input, Pointer, text, text_in},
};
use crate::{
    agents::{self, Brief, Entry, Look, Words},
    clock::time::Lang,
    levels::draw::{DIM, FAINT, LINE, MANROPE, MANROPE_BOLD, TEXT, rgb, ring},
};

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
const LABEL_SPACING: f32 = 0.08 * LABEL;
const LABEL_PAD: f32 = 4.0;
const LINE_GAP: f32 = 2.0;
/// An agent's line: 5 px in, 6 px past the sides, radius 7; its name
/// (13 px, 700) and since when (`text.faint`) 6 px apart, over what it
/// asks or why it failed (12.5 px, `text.dim`), 1 px apart.
const LINE_PAD: f32 = 5.0;
const LINE_OUT: f32 = 6.0;
const LINE_RADIUS: f32 = 7.0;
const WHO: f32 = 13.0;
const WHEN_GAP: f32 = 6.0;
const WHAT: f32 = 12.5;
const WHAT_GAP: f32 = 1.0;
/// The divider, 2 px from what is around it; the footer 12.5 px.
const RULE_PAD: f32 = 2.0;
const RULE: Color = Color::from_rgba8(255, 255, 255, 0.08);
const FOOTER: f32 = 12.5;
/// "Sources" lit around it, for the pointer.
const LINK_PAD: f32 = 6.0;
/// The sections' colours: the rows' words'.
const WAITING: Color = rgb(0xF5C46E);
const STUCK: Color = rgb(0xF7A77A);
const TO_SEE: Color = rgb(0x7EE0A0);
/// The mini form's last two (4.7): `text.dim`, and the resting rows' word.
const AT_WORK: Color = DIM;
const RESTING: Color = rgb(0x6A7384);

/// What can be clicked in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Agent(u64),
    Sources,
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
    let who = kit::elide(&entry.name, ROOM - WHEN_GAP - when_width, |t| {
        measure(t, WHO, MANROPE_BOLD)
    });
    let when_x = LEFT + measure(&who, WHO, MANROPE_BOLD) + WHEN_GAP;
    // Only a waiting or a stuck agent's line says what its tool said; a
    // finished or a working one's says since when, a resting one's its
    // name alone.
    let note = entry
        .note
        .as_ref()
        .filter(|_| matches!(look, Look::Waiting | Look::Stuck));
    let what = note.map(|note| kit::elide(&words.note(note), ROOM, |t| measure(t, WHAT, MANROPE)));
    let height =
        2.0 * LINE_PAD + WHO * LINE + what.as_ref().map_or(0.0, |_| WHAT_GAP + WHAT * LINE);
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

/// What the surface keeps between draws.
#[derive(Debug, Default)]
pub struct State {
    /// The agents as last laid out, the minute and the bar's form then.
    said: agents::Said,
    minute: u64,
    form: Option<Form>,
    laid: Option<Laid>,
    pointer: Pointer<Target>,
    focus: Option<Target>,
}

impl State {
    fn targets(&self) -> Vec<(Target, Rectangle)> {
        self.laid.as_ref().map_or_else(Vec::new, Laid::targets)
    }
}

impl<Message> canvas::Program<Message> for Panel {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match kit::input(self.id, event, bounds, cursor, &mut state.pointer) {
            Input::Redraw(now) => self.refresh(state, now),
            Input::Mouse(seen, at) => self.mouse(state, seen, at),
            Input::Key(pressed) => self.key(state, &pressed),
            Input::Nothing => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        kit::background(&mut frame, bounds.size(), self.glass);
        if let Some(laid) = &state.laid {
            self.briefing(&mut frame, laid, state.pointer.hover);
        }
        if let Some((_, rect)) = state
            .focus
            .and_then(|focus| state.targets().into_iter().find(|(t, _)| *t == focus))
        {
            ring(&mut frame, rect, LINE_RADIUS);
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        state.pointer.interaction(false)
    }
}

/// `word` in capitals spaced as a label's, from `x`, its line centred on
/// `middle`, in `colour`.
fn label(f: &mut Frame, word: &str, (x, middle): (f32, f32), colour: Color) {
    let mut at = x;
    for ch in word.chars() {
        let letter = ch.to_string();
        let width = kit::measure(&letter, LABEL, MANROPE_BOLD);
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                letter,
                Point::new(at, middle),
                LABEL,
                colour,
                Alignment::Left,
            ),
        ));
        at += width + LABEL_SPACING;
    }
}

impl Panel {
    fn briefing(&self, f: &mut Frame, laid: &Laid, hover: Option<Target>) {
        let words = self.words;
        let top = BORDER + PAD_TOP;
        let middle = top + HEAD / 2.0;
        f.draw_svg(
            Rectangle::new(Point::new(LEFT, top), Size::new(HEAD, HEAD)),
            svg::Svg::new(agents::draw::secretary(laid.mood).clone()),
        );
        f.fill_text(text_in(
            MANROPE_BOLD,
            text(
                words.briefing().to_owned(),
                Point::new(LEFT + HEAD + HEAD_GAP, middle),
                TITLE,
                TEXT,
                Alignment::Left,
            ),
        ));
        for section in &laid.sections {
            let colour = match section.look {
                Look::Waiting => WAITING,
                Look::Stuck => STUCK,
                Look::ToSee => TO_SEE,
                Look::Working => AT_WORK,
                Look::Resting => RESTING,
            };
            label(
                f,
                words.section(section.look),
                (LEFT, section.label_middle),
                colour,
            );
            for line in &section.lines {
                if hover == Some(Target::Agent(line.id)) {
                    f.fill(&kit::rounded(line.rect, LINE_RADIUS), FILL);
                }
                let who_middle = line.rect.y + LINE_PAD + WHO * LINE / 2.0;
                f.fill_text(text_in(
                    MANROPE_BOLD,
                    text(
                        line.who.clone(),
                        Point::new(LEFT, who_middle),
                        WHO,
                        TEXT,
                        Alignment::Left,
                    ),
                ));
                f.fill_text(text(
                    line.when.clone(),
                    Point::new(line.when_x, who_middle),
                    WHO,
                    FAINT,
                    Alignment::Left,
                ));
                if let Some(what) = &line.what {
                    f.fill_text(text(
                        what.clone(),
                        Point::new(
                            LEFT,
                            who_middle + WHO * LINE / 2.0 + WHAT_GAP + WHAT * LINE / 2.0,
                        ),
                        WHAT,
                        DIM,
                        Alignment::Left,
                    ));
                }
            }
        }
        f.fill_rectangle(Point::new(LEFT, laid.rule), Size::new(ROOM, 1.0), RULE);
        f.fill_text(text(
            laid.footer.clone(),
            Point::new(LEFT, laid.footer_middle),
            FOOTER,
            FAINT,
            Alignment::Left,
        ));
        let lit = hover == Some(Target::Sources);
        if lit {
            f.fill(&kit::rounded(laid.sources, LINE_RADIUS), FILL);
        }
        f.fill_text(text(
            words.sources().to_owned(),
            Point::new(laid.sources.x + LINK_PAD, laid.footer_middle),
            FOOTER,
            if lit { TEXT } else { FAINT },
            Alignment::Left,
        ));
    }

    /// What it shows, read afresh: laid out again when the agents changed,
    /// or the minute did.
    fn refresh<Message>(
        &self,
        state: &mut State,
        _now: Instant,
    ) -> Option<canvas::Action<Message>> {
        for pressed in kit::keys(self.id) {
            let _: Option<canvas::Action<Message>> = self.key(state, &pressed);
        }
        let form = super::with(|c| c.form_of(self.id))?;
        let said = agents::said();
        let now = agents::now_us();
        let minute = now / 60_000_000;
        if state.laid.is_none()
            || said != state.said
            || minute != state.minute
            || state.form != Some(form)
        {
            let laid = lay_out(
                &Brief::of(&said.agents),
                self.words,
                now,
                form,
                kit::measure,
            );
            if state.laid.as_ref() != Some(&laid) {
                tracing::debug!(briefing = %laid.said(self.words), "the briefing shows it");
                let items: Vec<String> = laid
                    .targets()
                    .iter()
                    .map(|(t, r)| format!("{t:?}@{:.0},{:.0}", r.center().x, r.center().y))
                    .collect();
                tracing::debug!(items = %items.join(" "), "the briefing's items");
            }
            state.laid = Some(laid);
            state.said = said;
            state.minute = minute;
            state.form = Some(form);
            let targets: Vec<Target> = state.targets().into_iter().map(|(t, _)| t).collect();
            if state.focus.is_some_and(|f| !targets.contains(&f)) {
                state.focus = None;
            }
            super::refit(Item::Secretary);
        }
        None
    }

    fn act(&self, state: &State, target: Target) {
        tracing::debug!(?target, "the briefing");
        match target {
            Target::Agent(id) => {
                let row = state
                    .targets()
                    .into_iter()
                    .find(|(t, _)| *t == target)
                    .map(|(_, r)| r);
                if let Some(row) = row {
                    event(Seen::GoTo { id, row });
                }
            }
            Target::Sources => event(Seen::Replace(Item::Sources)),
        }
    }

    fn mouse<Message>(
        &self,
        state: &mut State,
        seen: &mouse::Event,
        at: Option<Point>,
    ) -> Option<canvas::Action<Message>> {
        let target = at.and_then(|at| {
            state
                .targets()
                .into_iter()
                .find(|(_, r)| r.contains(at))
                .map(|(t, _)| t)
        });
        let redraw = Some(canvas::Action::request_redraw().and_capture());
        match seen {
            mouse::Event::CursorEntered | mouse::Event::CursorMoved { .. } => {
                state.pointer.moved();
                state.pointer.over(target).then_some(redraw).flatten()
            }
            mouse::Event::CursorLeft => {
                state.pointer.left();
                redraw
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                state.focus = None;
                state.pointer.pressed = target;
                redraw
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                if let Some(target) = state.pointer.release(target) {
                    self.act(state, target);
                }
                redraw
            }
            _ => None,
        }
    }

    fn key<Message>(
        &self,
        state: &mut State,
        pressed: &Pressed,
    ) -> Option<canvas::Action<Message>> {
        use keyboard::{Key, key::Named};
        let order: Vec<Target> = state.targets().into_iter().map(|(t, _)| t).collect();
        match &pressed.key {
            Key::Named(Named::Escape) => {
                event(Seen::Escape);
                return Some(canvas::Action::capture());
            }
            Key::Named(Named::Tab) => {
                state.focus = kit::step(&order, state.focus, pressed.modifiers.shift());
            }
            Key::Named(arrow @ (Named::ArrowDown | Named::ArrowUp)) => {
                state.focus = kit::step(&order, state.focus, *arrow == Named::ArrowUp);
            }
            Key::Named(Named::Enter | Named::Space) => {
                if let Some(target) = state.focus.filter(|t| order.contains(t)) {
                    self.act(state, target);
                }
            }
            _ => return None,
        }
        Some(canvas::Action::request_redraw().and_capture())
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
        assert!((asks.rect.height - bare.rect.height - (1.0 + 12.5 * LINE)).abs() < 0.01);
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
