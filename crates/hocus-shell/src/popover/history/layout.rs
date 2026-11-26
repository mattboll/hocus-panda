//! The bell's list laid out (#923): the pins, each application's card
//! (a lone line, or a header over its newest line and the strips standing
//! for the rest), its fold of seen ones, the rule, the applications with
//! seen ones only; their words cut to fit once. Pure: the history's
//! notifications in, places out ([`lay_out`]).

use super::*;

/// An application of the list, by its place in the shell's table of them:
/// kept until the shell restarts, so that what is unfolded stays and a
/// control's id says its application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct App(u32);

static APPS: Mutex<Vec<String>> = Mutex::new(Vec::new());

impl App {
    /// Application `id`'s (lowercase, as the list groups them).
    pub(super) fn of(id: &str) -> Self {
        let mut apps = APPS.lock().unwrap_or_else(PoisonError::into_inner);
        let at = apps.iter().position(|a| a == id).unwrap_or_else(|| {
            apps.push(id.to_owned());
            apps.len() - 1
        });
        Self(u32::try_from(at).unwrap_or(u32::MAX))
    }

    /// The id it stands for.
    pub(super) fn id(self) -> String {
        APPS.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(self.0 as usize)
            .cloned()
            .unwrap_or_default()
    }
}

/// What can be acted on in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Clear,
    Switch,
    Chip(Choice),
    Item(u64),
    Close(u64),
    /// Its "⋯" (#899): its application's levels.
    More(u64),
    Action(u64, usize),
    /// A phone's reply field, unfolded on its line, and its send button
    /// (#957).
    ReplyField(u64),
    ReplySend(u64),
    /// An application's "3 de plus", or "Réduire" (#923).
    Expand(App),
    /// Its fold, "12 vues", or its line of seen ones only.
    Fold(App),
    /// Its ✕: all its lines but the pinned ones.
    CloseApp(App),
}

impl Target {
    /// Its control's id: what tests, and screen readers, know it by.
    pub(super) fn id(self) -> String {
        match self {
            Self::Expand(app) => format!("history:Expand({})", app.id()),
            Self::Fold(app) => format!("history:Fold({})", app.id()),
            Self::CloseApp(app) => format!("history:CloseApp({})", app.id()),
            _ => format!("history:{self:?}").replace(' ', ""),
        }
    }

    /// The ring's corners on it.
    pub(super) fn radius(self) -> f32 {
        match self {
            Self::Item(_) => ITEM_RADIUS,
            Self::Close(_) | Self::More(_) | Self::CloseApp(_) | Self::Expand(_) => CLOSE_RADIUS,
            Self::Chip(_) => CHIP / 2.0,
            Self::Switch => SWITCH.height / 2.0,
            Self::Clear => GHOST_RADIUS,
            Self::Action(..) | Self::Fold(_) | Self::ReplyField(_) | Self::ReplySend(_) => {
                FOLD_RADIUS
            }
        }
    }

    /// The line it belongs to, if it is a line's.
    pub(super) fn key(self) -> Option<u64> {
        match self {
            Self::Item(k)
            | Self::Close(k)
            | Self::More(k)
            | Self::Action(k, _)
            | Self::ReplyField(k)
            | Self::ReplySend(k) => Some(k),
            _ => None,
        }
    }
}

/// A line's words, cut to fit once: they change only with it.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Cut {
    pub(super) app: String,
    pub(super) app_width: f32,
    pub(super) title: String,
    pub(super) body: Vec<String>,
    pub(super) actions: Vec<(String, String, f32)>,
    /// A pin's reason, cut on one line.
    pub(super) reason: Option<String>,
}

/// How a line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Look {
    /// Its icon and its application's name: a pin, a lone line.
    Full,
    /// Its age first, under its application's header or fold.
    Bare,
}

/// A line, laid out: its place in the list's content (from its top).
#[derive(Debug, Clone)]
pub(super) struct Row {
    pub(super) key: u64,
    pub(super) rect: Rectangle,
    pub(super) look: Look,
    pub(super) icon: Option<Icon>,
    pub(super) letter: String,
    pub(super) cut: Cut,
    pub(super) when: String,
    /// It is new: its blue dot.
    pub(super) dot: bool,
    /// Seen: under its fold, quieter.
    pub(super) seen: bool,
    /// The agent that pinned it.
    pub(super) pin: Option<String>,
    pub(super) actions: Vec<(String, Rectangle)>,
    /// A phone's reply field, unfolded by its "Répondre" (#957): the row
    /// of the field and its send button.
    pub(super) reply: Option<Rectangle>,
    /// Its application has a level (#899): not the desktop's own.
    pub(super) leveled: bool,
}

impl Row {
    #[cfg(test)]
    pub(super) fn close(&self) -> Rectangle {
        Rectangle::new(
            Point::new(
                self.rect.x + self.rect.width - CLOSE_IN.0 - CLOSE,
                self.rect.y + CLOSE_IN.1,
            ),
            Size::new(CLOSE, CLOSE),
        )
    }

    /// Its text's left, from its own.
    pub(super) fn text_x(&self) -> f32 {
        match self.look {
            Look::Full => ITEM_PAD + ICON + ICON_GAP,
            Look::Bare => ITEM_PAD,
        }
    }
}

/// An application's header, its several open lines under it.
#[derive(Debug, Clone)]
pub(super) struct Header {
    pub(super) app: App,
    pub(super) rect: Rectangle,
    pub(super) icon: Option<Icon>,
    pub(super) letter: String,
    pub(super) name: String,
    pub(super) count: usize,
    /// "3 de plus" or "Réduire", and its place in the header.
    pub(super) toggle: (String, Rectangle),
    pub(super) close: Rectangle,
}

/// A fold: "12 vues", its chevron turned while unfolded.
#[derive(Debug, Clone)]
pub(super) struct Fold {
    pub(super) app: App,
    pub(super) rect: Rectangle,
    pub(super) said: String,
    pub(super) open: bool,
}

/// An application with seen ones only: its line.
#[derive(Debug, Clone)]
pub(super) struct SeenLine {
    pub(super) app: App,
    pub(super) rect: Rectangle,
    pub(super) icon: Option<Icon>,
    pub(super) letter: String,
    pub(super) name: String,
    pub(super) said: String,
    pub(super) when: String,
    pub(super) open: bool,
}

/// What the list shows, top first.
#[derive(Debug, Clone)]
pub(super) enum Block {
    Line(Row),
    Header(Header),
    /// The strips under a card's newest line, and how many they stand for.
    Strips(Rectangle, usize),
    Fold(Fold),
    Rule(Rectangle),
    SeenLine(SeenLine),
}

impl Block {
    pub(super) fn rect(&self) -> Rectangle {
        match self {
            Self::Line(r) => r.rect,
            Self::Header(h) => h.rect,
            Self::Strips(r, _) | Self::Rule(r) => *r,
            Self::Fold(f) => f.rect,
            Self::SeenLine(s) => s.rect,
        }
    }

    /// The controls it holds.
    pub(super) fn targets(&self) -> Vec<Target> {
        match self {
            Self::Line(r) => {
                let mut all = vec![Target::Item(r.key), Target::Close(r.key)];
                if r.leveled {
                    all.push(Target::More(r.key));
                }
                all.extend((0..r.actions.len()).map(|i| Target::Action(r.key, i)));
                if r.reply.is_some() {
                    all.extend([Target::ReplyField(r.key), Target::ReplySend(r.key)]);
                }
                all
            }
            Self::Header(h) => vec![Target::Expand(h.app), Target::CloseApp(h.app)],
            Self::Fold(f) => vec![Target::Fold(f.app)],
            Self::SeenLine(s) => vec![Target::Fold(s.app), Target::CloseApp(s.app)],
            Self::Strips(..) | Self::Rule(_) => Vec::new(),
        }
    }
}

/// The history, laid out.
#[derive(Debug, Clone)]
pub(super) struct Laid {
    pub(super) clear: Option<(Rectangle, f32)>,
    pub(super) card: Rectangle,
    pub(super) row_middle: f32,
    pub(super) said: Vec<String>,
    pub(super) said_top: f32,
    pub(super) switch: Rectangle,
    pub(super) chips: Vec<(Choice, Rectangle)>,
    pub(super) dnd: Dnd,
    /// The list's viewport, and its content's height.
    pub(super) view: Rectangle,
    pub(super) content: f32,
    pub(super) blocks: Vec<Block>,
    pub(super) height: f32,
}

/// What is unfolded, kept until the shell restarts (not on disk): the
/// applications whose open lines all show, and those whose seen ones do.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct Folds {
    pub(super) expanded: BTreeSet<App>,
    pub(super) unfolded: BTreeSet<App>,
    /// The line whose reply field is unfolded (#957).
    pub(super) replying: Option<u64>,
}

static FOLDS: Mutex<Option<Folds>> = Mutex::new(None);

pub(super) fn folds<T>(with: impl FnOnce(&mut Folds) -> T) -> T {
    let mut folds = FOLDS.lock().unwrap_or_else(PoisonError::into_inner);
    with(folds.get_or_insert_with(Folds::default))
}

/// What it shows, as the stack has it.
#[derive(Debug, Clone)]
pub(super) struct Shown {
    pub(super) items: Vec<Kept>,
    pub(super) dnd: Dnd,
    pub(super) version: u64,
    pub(super) folds: Folds,
}

pub(super) fn shown() -> Shown {
    let folds = folds(|f| f.clone());
    notifications::with(|c| Shown {
        items: c.stack.history.items().to_vec(),
        dnd: c.stack.dnd,
        version: c.stack.history.version(),
        folds,
    })
}

/// A text's width at a size in a font.
type Measure<'a> = &'a mut dyn FnMut(&str, f32, Font) -> f32;

/// The text column's room in a line with its icon, and in a bare one.
const COLUMN: f32 = W - 2.0 * LIST_SIDE - ITEM_PAD - ICON - ICON_GAP - ITEM_RIGHT;
const BARE_COLUMN: f32 = W - 2.0 * LIST_SIDE - ITEM_PAD - ITEM_RIGHT;

/// `kept`'s words cut to fit a column `room` wide.
fn cut(kept: &Kept, room: f32, measure: Measure<'_>) -> Cut {
    let note = &kept.note;
    let app = kit::elide(note.app.trim(), room * 0.5, |t| {
        measure(t, META, MANROPE_BOLD)
    });
    let app_width = measure(&app, META, MANROPE_BOLD);
    let title = kit::elide(note.summary.trim(), room, |t| {
        measure(t, ITEM_TITLE, MANROPE_BOLD)
    });
    let mut body: Vec<String> = note
        .body
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .flat_map(|l| kit::wrap(l, room, |t| measure(t, BODY, MANROPE)))
        .collect();
    let more = body.len() > BODY_LINES;
    body.truncate(BODY_LINES);
    let count = body.len();
    for (i, line) in body.iter_mut().enumerate() {
        let shown = if i + 1 == count && more {
            format!("{line}…")
        } else {
            line.clone()
        };
        *line = kit::elide(&shown, room, |t| measure(t, BODY, MANROPE));
    }
    // A phone's reply in place: its "Répondre" unfolds the field (#957).
    let reply = note.phone.as_ref().and_then(|p| p.reply.as_ref()).map(|_| {
        (
            REPLY_KEY.to_owned(),
            hocus_words::shell::phone::REPLY_LINE
                .get(crate::clock::time::Lang::of_messages())
                .to_owned(),
        )
    });
    let actions = note
        .actions
        .iter()
        .chain(reply.as_ref())
        .map(|(key, label)| {
            let label = kit::elide(label, room - 2.0 * CHIP_PAD, |t| {
                measure(t, CHIP_TEXT, MANROPE_SEMIBOLD)
            });
            let width = (measure(&label, CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD).ceil();
            (key.clone(), label, width)
        })
        .collect();
    // The pin's line: its pin, the agent's name, "·", its reason.
    let reason = kept.pin.as_ref().map(|pin| {
        let agent = measure(&pin.agent, PIN_TEXT, MANROPE_BOLD);
        let room = room - PIN - 2.0 * PIN_GAP - agent - measure("· ", PIN_TEXT, MANROPE);
        pin.reason
            .as_deref()
            .map(|r| kit::elide(r, room.max(0.0), |t| measure(t, PIN_TEXT, MANROPE)))
            .unwrap_or_default()
    });
    Cut {
        app,
        app_width,
        title,
        body,
        actions,
        reason,
    }
}

/// The action a phone's "Répondre" stands for: not invoked, it unfolds
/// the field.
pub(super) const REPLY_KEY: &str = "inline-reply";
/// The reply's row: a 34 px field and its send button.
pub(super) const REPLY_ROW: f32 = 34.0;

/// The lines' words as cut, by key, arrival, title and room: a line's
/// change comes as a new arrival.
pub(super) type Cuts = HashMap<(u64, i64, String, bool), Cut>;

pub(super) static CUTS: Mutex<Option<Cuts>> = Mutex::new(None);

/// The letter of an application without an icon.
fn letter_of(app: &str) -> String {
    app.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect::<String>())
        .unwrap_or_default()
}

/// Lays the list's blocks out, from its content's top.
struct Lay<'m, 'c> {
    pub(super) words: Words,
    pub(super) now: &'c Zoned,
    pub(super) cuts: &'c mut Cuts,
    pub(super) measure: Measure<'m>,
    pub(super) width: f32,
    pub(super) y: f32,
    pub(super) blocks: Vec<Block>,
    /// The line whose reply field is unfolded.
    pub(super) replying: Option<u64>,
}

impl Lay<'_, '_> {
    /// A new card: 6 px under the one before.
    pub(super) fn card(&mut self) {
        if !self.blocks.is_empty() {
            self.y += CARD_APART;
        }
    }

    /// The next part of a card: 2 px under the one before.
    pub(super) fn within(&mut self) {
        self.y += GAP;
    }

    /// `kept`'s line, `look` so.
    pub(super) fn line(&mut self, kept: &Kept, look: Look) {
        let wide = look == Look::Bare;
        let room = if wide { BARE_COLUMN } else { COLUMN };
        let id = (kept.key, kept.note.at, kept.note.summary.clone(), wide);
        let cut = match self.cuts.get(&id) {
            Some(cut) => cut.clone(),
            None => {
                let made = cut(kept, room, self.measure);
                self.cuts.insert(id, made.clone());
                made
            }
        };
        let y = self.y;
        let when = self.words.ago(history::ago(kept.note.at, self.now));
        let x = LIST_SIDE
            + match look {
                Look::Full => ITEM_PAD + ICON + ICON_GAP,
                Look::Bare => ITEM_PAD,
            };
        let mut column = META * LINE + TEXT_GAP + ITEM_TITLE * LINE;
        if !cut.body.is_empty() {
            column += TEXT_GAP + cut.body.len() as f32 * BODY * BODY_LINE;
        }
        if kept.pin.is_some() {
            column += PIN_TOP + PIN_TEXT * LINE;
        }
        // Its actions while its sender holds it.
        let mut actions = Vec::new();
        if kept.live.is_some() && !cut.actions.is_empty() {
            let mut ax = x;
            let mut ay = y + ITEM_PAD + column + ACTION_GAP;
            for (key, _, w) in &cut.actions {
                if ax > x && ax + w > x + room {
                    ax = x;
                    ay += ACTION + ACTION_GAP;
                }
                actions.push((
                    key.clone(),
                    Rectangle::new(Point::new(ax, ay), Size::new(*w, ACTION)),
                ));
                ax += w + ACTION_GAP;
            }
            column = actions
                .last()
                .map_or(column, |(_, r)| r.y + r.height - y - ITEM_PAD);
        }
        let reply = (self.replying == Some(kept.key) && kept.live.is_some()).then(|| {
            let top = y + ITEM_PAD + column + ACTION_GAP;
            column += ACTION_GAP + REPLY_ROW;
            Rectangle::new(Point::new(x, top), Size::new(room, REPLY_ROW))
        });
        let icon_room = if look == Look::Full { ICON } else { 0.0 };
        let height = (2.0 * ITEM_PAD + column.max(icon_room)).max(ITEM_LEAST);
        self.blocks.push(Block::Line(Row {
            key: kept.key,
            rect: Rectangle::new(Point::new(LIST_SIDE, y), Size::new(self.width, height)),
            look,
            icon: kept.note.icon.clone(),
            letter: letter_of(&kept.note.app),
            cut,
            when,
            dot: kept.seen == history::Sight::New,
            seen: kept.seen.folded(),
            pin: kept.pin.as_ref().map(|p| p.agent.clone()),
            actions,
            reply,
            leveled: !kept.own,
        }));
        self.y += height;
    }

    /// An application's header over its `open` lines.
    pub(super) fn header(&mut self, app: App, first: &Kept, count: usize, expanded: bool) {
        let label = if expanded {
            self.words.collapse().to_owned()
        } else {
            self.words.more(count - 1)
        };
        let toggle_w =
            ((self.measure)(&label, TOGGLE_TEXT, MANROPE_SEMIBOLD) + 2.0 * TOGGLE_PAD).ceil();
        let height = GROUP_PAD.1 + TOGGLE + GROUP_PAD.2;
        let rect = Rectangle::new(Point::new(LIST_SIDE, self.y), Size::new(self.width, height));
        let close = Rectangle::new(
            Point::new(self.width - GROUP_PAD.0 - TOGGLE, GROUP_PAD.1),
            Size::new(TOGGLE, TOGGLE),
        );
        let toggle = Rectangle::new(
            Point::new(close.x - SMALL_GAP - toggle_w, GROUP_PAD.1),
            Size::new(toggle_w, TOGGLE),
        );
        let room = toggle.x - GROUP_PAD.0 - SMALL_ICON - SMALL_GAP - SMALL_GAP;
        let measure = &mut *self.measure;
        let name = kit::elide(first.note.app.trim(), room * 0.7, |t| {
            measure(t, GROUP_NAME, MANROPE_BOLD)
        });
        self.blocks.push(Block::Header(Header {
            app,
            rect,
            icon: first.note.icon.clone(),
            letter: letter_of(&first.note.app),
            name,
            count,
            toggle: (label, toggle),
            close,
        }));
        self.y += height;
    }

    /// The strips under a card's newest line, standing for `hidden` more:
    /// against it (the mockup's go 2 px up into it, where nothing shows).
    pub(super) fn strips(&mut self, hidden: usize) {
        let top = self.y;
        let height = STRIPS.0 + GAP + STRIPS.1;
        self.blocks.push(Block::Strips(
            Rectangle::new(
                Point::new(LIST_SIDE + STRIPS_IN.0, top),
                Size::new(self.width - 2.0 * STRIPS_IN.0, height),
            ),
            hidden,
        ));
        self.y = top + height + STRIPS_UNDER;
    }

    /// The fold of `seen` ones, at the text's column under a lone line
    /// (`indent`).
    pub(super) fn fold(&mut self, app: App, seen: usize, open: bool, indent: bool) {
        let said = self.words.seen(seen);
        let width = (FOLD_PAD
            + CHEVRON
            + CHEVRON_GAP
            + (self.measure)(&said, FOLD_TEXT, MANROPE_SEMIBOLD)
            + FOLD_PAD)
            .ceil();
        let x = if indent {
            ITEM_PAD + ICON + ICON_GAP - FOLD_PAD
        } else {
            0.0
        };
        self.blocks.push(Block::Fold(Fold {
            app,
            rect: Rectangle::new(Point::new(LIST_SIDE + x, self.y), Size::new(width, FOLD)),
            said,
            open,
        }));
        self.y += FOLD;
    }

    /// The rule above the applications with seen ones only.
    pub(super) fn rule(&mut self) {
        self.card();
        self.y += RULE_AROUND;
        self.blocks.push(Block::Rule(Rectangle::new(
            Point::new(LIST_SIDE + RULE_IN, self.y),
            Size::new(self.width - 2.0 * RULE_IN, 1.0),
        )));
        self.y += 1.0 + RULE_AROUND;
    }

    /// The line of an application with `seen` ones only, the newest
    /// `first`.
    pub(super) fn seen_line(&mut self, app: App, first: &Kept, seen: usize, open: bool) {
        let room = self.width * 0.5;
        let measure = &mut *self.measure;
        let name = kit::elide(first.note.app.trim(), room, |t| {
            measure(t, GROUP_NAME, MANROPE_BOLD)
        });
        self.blocks.push(Block::SeenLine(SeenLine {
            app,
            rect: Rectangle::new(
                Point::new(LIST_SIDE, self.y),
                Size::new(self.width, SEEN_LINE),
            ),
            icon: first.note.icon.clone(),
            letter: letter_of(&first.note.app),
            name,
            said: format!("· {}", self.words.seen(seen)),
            when: self.words.ago(history::ago(first.note.at, self.now)),
            open,
        }));
        self.y += SEEN_LINE;
    }
}

/// `shown` laid out at `now`, in `words`.
pub(super) fn lay_out(
    shown: &Shown,
    words: Words,
    now: &Zoned,
    cuts: &mut Cuts,
    measure: Measure<'_>,
) -> Laid {
    // The header.
    let clear = (!shown.items.is_empty()).then(|| {
        let width = measure(words.clear(), GHOST_TEXT, MANROPE_SEMIBOLD);
        let w = width + 2.0 * GHOST_PAD;
        (
            Rectangle::new(
                Point::new(W - BORDER - SIDE - w, HEADER_TOP),
                Size::new(w, HEADER),
            ),
            width,
        )
    });
    // The card.
    let card_top = HEADER_TOP + HEADER + HEADER_BOTTOM;
    let inner_left = CARD_LEFT + BORDER + CARD_PAD.1;
    let inner_right = CARD_RIGHT - BORDER - CARD_PAD.1;
    let words_left = inner_left + MOON + CARD_GAP;
    let room = inner_right - SWITCH.width - CARD_GAP - words_left;
    let said = kit::wrap(&words.said(shown.dnd, now), room, |t| {
        measure(t, DND_SAID, MANROPE)
    });
    let column = DND_NAME * LINE + SAID_GAP + said.len() as f32 * DND_SAID * LINE;
    let row_top = card_top + BORDER + CARD_PAD.0;
    let row = column.max(SWITCH.height).max(MOON);
    let row_middle = row_top + row / 2.0;
    let said_top = row_middle - column / 2.0 + DND_NAME * LINE + SAID_GAP;
    let switch = Rectangle::new(
        Point::new(inner_right - SWITCH.width, row_middle - SWITCH.height / 2.0),
        SWITCH,
    );
    let mut y = row_top + row;
    let mut chips = Vec::new();
    if shown.dnd.is_on() {
        y += CHIPS_GAP;
        let mut x = inner_left;
        for choice in Choice::ALL {
            let width =
                (measure(words.chip(choice), CHIP_TEXT, MANROPE_SEMIBOLD) + 2.0 * CHIP_PAD).ceil();
            if x > inner_left && x + width > inner_right {
                x = inner_left;
                y += CHIP + CHIP_GAP;
            }
            chips.push((
                choice,
                Rectangle::new(Point::new(x, y), Size::new(width, CHIP)),
            ));
            x += width + CHIP_GAP;
        }
        y += CHIP;
    }
    let card_bottom = y + CARD_PAD.0 + BORDER;
    let card = Rectangle::new(
        Point::new(CARD_LEFT, card_top),
        Size::new(CARD_RIGHT - CARD_LEFT, card_bottom - card_top),
    );
    // The list, from its content's top.
    let list_top = card_bottom + LIST_TOP;
    let width = W - 2.0 * LIST_SIDE;
    let grouped = history::grouped(&shown.items);
    let mut lay = Lay {
        words,
        now,
        cuts,
        measure,
        width,
        y: 0.0,
        blocks: Vec::new(),
        replying: shown.folds.replying,
    };
    // The pins, the newest pin first.
    if !grouped.pins.is_empty() {
        lay.card();
        for (i, kept) in grouped.pins.iter().enumerate() {
            if i > 0 {
                lay.within();
            }
            lay.line(kept, Look::Full);
        }
    }
    // The applications with something new or noticed.
    for group in &grouped.open {
        let app = App::of(&group.app);
        let (expanded, unfolded) = (
            shown.folds.expanded.contains(&app),
            shown.folds.unfolded.contains(&app),
        );
        lay.card();
        match group.open.as_slice() {
            [lone] => lay.line(lone, Look::Full),
            several => {
                if let Some(first) = several.first() {
                    lay.header(app, first, several.len(), expanded);
                }
                let shown_lines = if expanded { several } else { &several[..1] };
                for kept in shown_lines {
                    lay.within();
                    lay.line(kept, Look::Bare);
                }
                if !expanded {
                    lay.strips(several.len() - 1);
                }
            }
        }
        if !group.seen.is_empty() {
            lay.within();
            lay.fold(app, group.seen.len(), unfolded, group.open.len() == 1);
            if unfolded {
                for kept in &group.seen {
                    lay.within();
                    lay.line(kept, Look::Bare);
                }
            }
        }
    }
    // Under a rule, those with seen ones only.
    if !grouped.seen_only.is_empty() && !lay.blocks.is_empty() {
        lay.rule();
    }
    for group in &grouped.seen_only {
        let app = App::of(&group.app);
        let unfolded = shown.folds.unfolded.contains(&app);
        lay.card();
        if let Some(first) = group.seen.first() {
            lay.seen_line(app, first, group.seen.len(), unfolded);
        }
        if unfolded {
            for kept in &group.seen {
                lay.within();
                lay.line(kept, Look::Bare);
            }
        }
    }
    let (content_y, blocks) = (lay.y, lay.blocks);
    let content = if blocks.is_empty() { EMPTY } else { content_y };
    let room = MOST - list_top - LIST_BOTTOM - BORDER;
    let view = Rectangle::new(
        Point::new(LIST_SIDE, list_top),
        Size::new(width, content.min(room)),
    );
    let height = (list_top + view.height + LIST_BOTTOM + BORDER).round();
    Laid {
        clear,
        card,
        row_middle,
        said,
        said_top,
        switch,
        chips,
        dnd: shown.dnd,
        view,
        content,
        blocks,
        height,
    }
}

impl Laid {
    pub(super) fn most_scroll(&self) -> f32 {
        (self.content - self.view.height).max(0.0)
    }

    /// Its lines, top first.
    pub(super) fn rows(&self) -> impl Iterator<Item = &Row> {
        self.blocks.iter().filter_map(|b| match b {
            Block::Line(row) => Some(row),
            _ => None,
        })
    }

    /// Whether it shows `target`.
    pub(super) fn has(&self, target: Target) -> bool {
        match target {
            Target::Clear => self.clear.is_some(),
            Target::Switch => true,
            Target::Chip(choice) => self.chips.iter().any(|(c, _)| *c == choice),
            _ => self.blocks.iter().any(|b| b.targets().contains(&target)),
        }
    }

    /// The place in the list of the block holding `target`.
    pub(super) fn place_of(&self, target: Target) -> Option<Rectangle> {
        self.blocks
            .iter()
            .find(|b| b.targets().contains(&target))
            .map(Block::rect)
    }

    /// For the log: the list as it shows, top first ("Slack 3: «Hugo
    /// dans #dev» il y a 9 min (new) +2 · 12 vues ▸ | Agenda · 4 vues ▸").
    pub(super) fn said(&self, words: Words) -> String {
        if self.blocks.is_empty() {
            return words.nothing().to_owned();
        }
        let mut said: Vec<String> = Vec::new();
        for block in &self.blocks {
            said.push(match block {
                Block::Line(row) => {
                    let mut line = match row.look {
                        Look::Full => format!("{} «{}» {}", row.cut.app, row.cut.title, row.when),
                        Look::Bare => format!("«{}» {}", row.cut.title, row.when),
                    };
                    if row.dot {
                        line.push_str(" (new)");
                    }
                    if row.seen {
                        line.push_str(" (seen)");
                    }
                    if let Some(agent) = &row.pin {
                        line.push_str(&format!(" (pinned by {agent})"));
                    }
                    if !row.actions.is_empty() {
                        line.push_str(&format!(" [{} actions]", row.actions.len()));
                    }
                    line
                }
                Block::Header(h) => format!("{} {}:", h.name, h.count),
                Block::Strips(_, hidden) => format!("+{hidden}"),
                Block::Fold(f) => format!("{} {}", f.said, if f.open { "▾" } else { "▸" }),
                Block::Rule(_) => "|".to_owned(),
                Block::SeenLine(s) => {
                    format!("{} {} {}", s.name, s.said, if s.open { "▾" } else { "▸" })
                }
            });
        }
        said.join(" ")
    }
}

/// Its height, as it shows now.
pub fn height() -> u32 {
    let shown = shown();
    let mut cuts = CUTS.lock().unwrap_or_else(PoisonError::into_inner);
    let cuts = cuts.get_or_insert_with(HashMap::new);
    let laid = lay_out(
        &shown,
        Words::new(Lang::of_messages()),
        &notifications::wall(),
        cuts,
        &mut kit::measure,
    );
    laid.height as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notifications::{
        history::{History, Sight as Was},
        note::{IconFrom, Note, Timeout, Urgency},
    };

    fn note(id: u32, app: &str, title: &str, at: i64) -> Note {
        Note {
            id,
            number: u64::from(id),
            app: app.to_owned(),
            entry: None,
            icon: None,
            summary: title.to_owned(),
            body: "J’ai mis à jour la maquette du lanceur, tu peux jeter un œil quand tu as deux \
                   minutes ? Et puis encore des mots pour une troisième ligne."
                .to_owned(),
            actions: vec![("reply".to_owned(), "Répondre".to_owned())],
            default: true,
            urgency: Urgency::Normal,
            timeout: Timeout::Never,
            progress: None,
            resident: false,
            pid: 0,
            agent: false,
            conversation: None,
            icon_from: IconFrom::default(),
            at,
            sound: None,
            transient: false,
            ancestry: Vec::new(),
            phone: None,
        }
    }

    fn measure(t: &str, size: f32, _font: Font) -> f32 {
        t.chars().count() as f32 * size * 0.5
    }

    fn now() -> Zoned {
        "2026-11-02T14:32:00+01:00[Europe/Paris]"
            .parse()
            .expect("a time")
    }

    fn shown(history: &History, dnd: Dnd, folds: Folds) -> Shown {
        Shown {
            items: history.items().to_vec(),
            dnd,
            version: history.version(),
            folds,
        }
    }

    /// The mockup's bell: Courriel pinned, Chat's four new and two seen,
    /// Agenda's one noticed, Mises à jour's seen ones only.
    fn the_bell() -> History {
        let ms = now().timestamp().as_millisecond();
        let min = 60_000;
        let mut h = History::default();
        h.add(
            note(1, "Mises à jour", "12 mises à jour", ms - 1500 * min),
            Was::Seen,
            false,
        );
        h.add(
            note(2, "Chat", "Léa dans #design", ms - 120 * min),
            Was::Seen,
            false,
        );
        h.add(
            note(3, "Chat", "Hugo dans #dev", ms - 95 * min),
            Was::Seen,
            false,
        );
        h.add(
            note(4, "Agenda", "Point produit à 14 h 45", ms - 40 * min),
            Was::Noticed,
            false,
        );
        h.add(
            note(5, "Courriel", "Facture d’octobre", ms - 25 * min),
            Was::New,
            false,
        );
        h.add(
            note(6, "Chat", "Noah dans #général", ms - 21 * min),
            Was::New,
            false,
        );
        h.add(
            note(7, "Chat", "Inès dans #général", ms - 14 * min),
            Was::New,
            false,
        );
        h.add(
            note(8, "Chat", "Hugo dans #dev", ms - 9 * min),
            Was::New,
            false,
        );
        h.add(
            note(9, "Chat", "Camille dans #design", ms - 2 * min),
            Was::New,
            false,
        );
        h.closed(1);
        let pinned = h.pin(
            5,
            hocus_link::NotePin {
                agent: "secretaire".to_owned(),
                reason: Some("À payer avant vendredi".to_owned()),
                at: ms,
            },
        );
        assert!(pinned.is_ok());
        h
    }

    #[test]
    fn it_lists_the_pins_then_each_application_then_those_seen_as_the_mockup_lays_them() {
        let now = now();
        let h = the_bell();
        let laid = lay_out(
            &shown(&h, Dnd::Off, Folds::default()),
            Words::new(Lang::French),
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let said = laid.said(Words::new(Lang::French));
        assert_eq!(
            said,
            "Courriel «Facture d’octobre» il y a 25\u{a0}min (new) (pinned by secretaire) \
             [1 actions] Chat 4: «Camille dans #design» il y a 2\u{a0}min (new) [1 actions] +3 \
             2 vues ▸ Agenda «Point produit à 14 h 45» il y a 40\u{a0}min [1 actions] | \
             Mises à jour · 1 vue ▸"
        );
        let Some(Block::Line(pin)) = laid.blocks.first() else {
            panic!("{said}");
        };
        // 342 px wide, 8 px in from the popover's rim; its body on two
        // lines, cut; its ✕ 6 px from its right and 8 px from its top; its
        // pin's line under its body.
        assert_eq!((pin.rect.x, pin.rect.width), (9.0, 342.0));
        assert_eq!(pin.cut.body.len(), 2);
        assert!(pin.cut.body[1].ends_with('…'));
        assert_eq!(pin.close().x + 22.0 + 6.0, 351.0);
        assert_eq!(pin.close().y, pin.rect.y + 8.0);
        assert_eq!(pin.cut.reason.as_deref(), Some("À payer avant vendredi"));
        // Chat's header 6 px under the pin, 32 px tall; its newest line
        // bare under it; the strips against it; its fold flush left.
        let Some(Block::Header(header)) = laid.blocks.get(1) else {
            panic!("{said}");
        };
        assert_eq!(header.rect.y, pin.rect.y + pin.rect.height + 6.0);
        assert_eq!(header.rect.height, 32.0);
        assert_eq!(header.toggle.0, "3 de plus");
        let Some(Block::Line(newest)) = laid.blocks.get(2) else {
            panic!("{said}");
        };
        assert_eq!(newest.look, Look::Bare);
        assert_eq!(newest.rect.y, header.rect.y + 32.0 + 2.0);
        let Some(Block::Strips(strips, 3)) = laid.blocks.get(3) else {
            panic!("{said}");
        };
        assert_eq!(strips.x, 9.0 + 14.0);
        let Some(Block::Fold(fold)) = laid.blocks.get(4) else {
            panic!("{said}");
        };
        assert_eq!((fold.rect.x, fold.rect.height), (9.0, 26.0));
        // Agenda's lone line, its icon and name; no fold.
        let Some(Block::Line(agenda)) = laid.blocks.get(5) else {
            panic!("{said}");
        };
        assert_eq!(agenda.look, Look::Full);
        // The rule, then Mises à jour's 34 px line.
        assert!(matches!(laid.blocks.get(6), Some(Block::Rule(_))));
        let Some(Block::SeenLine(seen)) = laid.blocks.get(7) else {
            panic!("{said}");
        };
        assert_eq!(seen.rect.height, 34.0);
        assert!(laid.clear.is_some());
        assert!(laid.height <= 600.0);
    }

    #[test]
    fn unfolded_a_cards_lines_all_show_and_its_seen_ones_under_its_fold() {
        let now = now();
        let h = the_bell();
        let chat = App::of("chat");
        let updates = App::of("mises à jour");
        let folds = Folds {
            expanded: [chat].into(),
            unfolded: [chat, updates].into(),
            replying: None,
        };
        let laid = lay_out(
            &shown(&h, Dnd::Off, folds),
            Words::new(Lang::French),
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let said = laid.said(Words::new(Lang::French));
        assert!(
            said.contains(
                "Chat 4: «Camille dans #design» il y a 2\u{a0}min (new) [1 actions] «Hugo dans \
                 #dev» il y a 9\u{a0}min (new)"
            ),
            "{said}"
        );
        assert!(!said.contains('+'), "no strips: {said}");
        assert!(
            said.contains("2 vues ▾ «Hugo dans #dev» il y a 1\u{a0}h (seen)"),
            "{said}"
        );
        assert!(
            said.ends_with("Mises à jour · 1 vue ▾ «12 mises à jour» hier (seen)"),
            "{said}"
        );
        let header = laid.blocks.iter().find_map(|b| match b {
            Block::Header(h) => Some(h.toggle.0.clone()),
            _ => None,
        });
        assert_eq!(header.as_deref(), Some("Réduire"));
        // Under a lone line, the fold at its text's column.
        let mut lone = History::default();
        let ms = now.timestamp().as_millisecond();
        lone.add(note(1, "Chat", "Vu", ms - 9_000_000), Was::Seen, false);
        lone.add(note(2, "Chat", "Nouveau", ms), Was::New, false);
        let laid = lay_out(
            &shown(&lone, Dnd::Off, Folds::default()),
            Words::new(Lang::English),
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let fold = laid.blocks.iter().find_map(|b| match b {
            Block::Fold(f) => Some(f.clone()),
            _ => None,
        });
        assert!(
            fold.is_some_and(|f| f.rect.x == 9.0 + 8.0 + 28.0 + 10.0 - 8.0 && f.said == "1 seen")
        );
    }

    #[test]
    fn do_not_disturb_says_until_when_and_offers_its_chips_while_on() {
        let now = now();
        let words = Words::new(Lang::French);
        let h = History::default();
        let off = lay_out(
            &shown(&h, Dnd::Off, Folds::default()),
            words,
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        assert_eq!(off.said, ["Les bulles s’affichent"]);
        assert!(off.chips.is_empty());
        assert!(off.clear.is_none());
        let at = Instant::now();
        let hour = Dnd::starting(Choice::Hour, at, &now);
        assert_eq!(
            words.said(hour, &now),
            "Jusqu’à 15\u{a0}h\u{a0}32 · seules les urgences passent"
        );
        let morning = Dnd::starting(Choice::Morning, at, &now);
        assert!(
            words
                .said(morning, &now)
                .starts_with("Jusqu’à demain, 8\u{a0}h ·")
        );
        let english = Words::new(Lang::English);
        assert_eq!(
            english.said(hour, &now),
            "Until 3:32\u{a0}PM · only urgent ones come through"
        );
        assert!(
            english
                .said(morning, &now)
                .starts_with("Until tomorrow, 8\u{a0}AM")
        );
        let on = lay_out(
            &shown(&h, hour, Folds::default()),
            words,
            &now,
            &mut Cuts::new(),
            &mut measure,
        );
        let chips: Vec<Choice> = on.chips.iter().map(|(c, _)| *c).collect();
        assert_eq!(chips, Choice::ALL);
        assert!(on.card.height > off.card.height + 26.0);
        // Empty: the panda's room.
        assert_eq!(on.content, EMPTY);
    }
}
