//! The actions palette (#736, docs/design/actions.md): `Super+P` drops a
//! list of every action of the desktop from the top of the target's
//! screen, the window that had the keyboard ringed under it; typing
//! filters it ([`list`]), `Enter` runs the selected action on that window,
//! or its screen. An action asking for a desktop or an agent turns the
//! list into its targets (a second step); a digit picks a desktop at once.
//!
//! Its surface is a layer surface of the shell's ([`ACTIONS_NAMESPACE`]),
//! in the overlay layer, 720 px wide, its top 64 px under the screen's,
//! centred across the work area, holding the keyboard. The compositor
//! dims the screen under it, rings the target, and moves it in and out as
//! a popover. Its surface is the widget kit's ([`view`]): what its
//! widgets ask is handled at once, and it alone draws again.
//!
//! The compositor's actions are asked for on the line once the palette
//! has faded out (`action ID ARG WINDOW`); the shell's own (the bar's
//! popovers, the Settings, the agents) are carried out here then. The
//! recent ones are kept ([`store`]).

pub mod list;
pub mod store;
pub mod view;

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use hocus_link::{
    ACTIONS_CLOSE, ACTIONS_NAMESPACE, ActionsShown, RunAction, ToCompositor,
    actions::{self, Def, Runs, Step},
};
use iced::{Element, keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use self::list::{Elsewhere, Row, Target};
use crate::{
    clock::time::Lang,
    field::{self, Entry, Took},
    popover::{Effect, Pressed},
    surfaces::{Asked, Context, Lent, Surface},
};

/// The characters the field keeps at most.
const QUERY_MOST: usize = 120;

/// The palette's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub fr: bool,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self {
            fr: lang == Lang::French,
        }
    }

    fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn title(self) -> &'static str {
        "Actions"
    }

    pub fn placeholder(self) -> &'static str {
        self.pick(
            "Une action : fenêtre, bureau, réglage, session…",
            "An action: window, desktop, setting, session…",
        )
    }

    pub fn step_placeholder(self, step: Step) -> &'static str {
        match step {
            Step::Desktop => self.pick("numéro ou nom du bureau", "number or name of the desktop"),
            Step::Agent => self.pick("nom de l’agent", "name of the agent"),
        }
    }

    pub fn escape(self) -> &'static str {
        self.pick("Échap", "Esc")
    }

    pub fn search(self) -> &'static str {
        self.pick("Chercher une action", "Search for an action")
    }

    pub fn keyword(self, word: &str) -> String {
        if self.fr {
            format!("mot-clé\u{a0}: {word}")
        } else {
            format!("keyword: {word}")
        }
    }

    pub fn state(self, on: bool) -> &'static str {
        match on {
            true => self.pick("activé", "on"),
            false => self.pick("désactivé", "off"),
        }
    }

    /// A step's heading: what goes where.
    pub fn step_head(self, def: &Def, title: &str) -> String {
        match (def.id, self.fr) {
            ("send-to-desktop" | "take-to-desktop", true) if !title.is_empty() => {
                format!("{title} part sur…")
            }
            ("send-to-desktop" | "take-to-desktop", false) if !title.is_empty() => {
                format!("{title} goes to…")
            }
            _ => def.name(self.fr).to_owned(),
        }
    }

    pub fn elsewhere(self, to: Elsewhere, query: &str) -> String {
        match (to, self.fr) {
            (Elsewhere::Launcher, true) => {
                format!("Chercher «\u{a0}{query}\u{a0}» dans le lanceur")
            }
            (Elsewhere::Launcher, false) => {
                format!("Search the launcher for \u{201c}{query}\u{201d}")
            }
            (Elsewhere::Settings, true) => {
                format!("Chercher «\u{a0}{query}\u{a0}» dans les Réglages")
            }
            (Elsewhere::Settings, false) => {
                format!("Search the Settings for \u{201c}{query}\u{201d}")
            }
        }
    }

    pub fn elsewhere_line(self, to: Elsewhere) -> &'static str {
        match to {
            Elsewhere::Launcher => self.pick(
                "Applications, calcul, web",
                "Applications, calculation, web",
            ),
            Elsewhere::Settings => self.pick("Réglages", "Settings"),
        }
    }

    /// The footer.
    pub fn footer(self, matched: usize, step: bool, empty: bool) -> String {
        if step {
            return self
                .pick(
                    "Un chiffre choisit tout de suite · Retour arrière revient aux actions",
                    "A digit picks at once · Backspace goes back to the actions",
                )
                .to_owned();
        }
        let keys = self.pick(
            "↑↓ choisir · Entrée lancer · Tab compléter",
            "↑↓ choose · Enter runs · Tab completes",
        );
        let tie = self.pick(
            "les récentes d’abord à score égal",
            "the recent first on a tie",
        );
        match (empty, matched) {
            (true, _) => format!("{matched} actions · {keys}"),
            (false, 0) => keys.to_owned(),
            (false, _) => format!("{matched} actions · {tie}"),
        }
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// A step under way: the action asking, and the query typed before it.
#[derive(Debug, Clone)]
pub struct StepOpen {
    pub def: &'static Def,
    pub before: String,
}

/// The palette open.
#[derive(Debug)]
pub struct Open {
    pub surface: window::Id,
    pub shown: ActionsShown,
    /// The filter's field (#771).
    pub query: Entry,
    pub step: Option<StepOpen>,
    pub rows: Vec<Row>,
    /// The row selected, in [`Open::rows`].
    pub selected: usize,
    /// How far its list is scrolled, as it said or was told.
    pub scroll: f32,
    /// Its width, and its size and margins (top, left) as last asked.
    pub width: f32,
    pub size: (u32, u32),
    pub place: (i32, i32),
    /// What it last said it shows, in the log.
    pub logged: String,
}

impl Open {
    /// The rows again, the first selectable one selected.
    fn relist(&mut self, words: Words) {
        let recent = store::recent();
        let agents = agents();
        let desks = desks(&self.shown);
        let seen = list::Seen {
            fr: words.fr,
            shown: &self.shown,
            recent: &recent,
            dnd: dnd(),
            agents: &agents,
            desks: &desks,
        };
        self.rows = match &self.step {
            Some(step) => {
                let mut rows = vec![Row::Head(words.step_head(step.def, &self.shown.title))];
                rows.extend(list::targets(step.def, self.query.text(), &seen));
                rows
            }
            None => list::actions(self.query.text(), &seen),
        };
        self.selected = self.first();
        view::to_top(self);
    }

    /// The first row the selection goes to.
    fn first(&self) -> usize {
        let selectable = |r: &Row| r.selectable();
        // The first row that can run; an inert one only when none can.
        self.rows
            .iter()
            .position(|r| selectable(r) && r.inert().is_none())
            .or_else(|| self.rows.iter().position(selectable))
            .unwrap_or(0)
    }

    /// The selection moved by `by` selectable rows.
    fn step_selection(&mut self, by: i32) {
        let selectable: Vec<usize> = (0..self.rows.len())
            .filter(|i| self.rows[*i].selectable())
            .collect();
        let Some(at) = selectable.iter().position(|i| *i == self.selected) else {
            if let Some(first) = selectable.first() {
                self.selected = *first;
            }
            return;
        };
        let to = (at as i64 + i64::from(by)).clamp(0, selectable.len() as i64 - 1) as usize;
        self.selected = selectable[to];
        view::reveal(self);
    }

    /// How many actions the list shows.
    pub fn matched(&self) -> usize {
        self.rows
            .iter()
            .filter(|r| matches!(r, Row::Action(_)))
            .count()
    }

    /// Its size and margins for its rows now; the effect to resize it if
    /// they changed.
    pub fn refit(&mut self) -> Option<Effect> {
        let size = view::size(&self.rows, self.width);
        if size == self.size {
            return None;
        }
        self.size = size;
        Some(Effect::Resize {
            id: self.surface,
            size,
            margin: None,
        })
    }
}

/// The agents, in the bar's rows' order.
fn agents() -> Vec<list::Agent> {
    let mut shown = crate::agents::said().agents;
    shown.sort_by_key(|a| {
        let look = crate::agents::Look::of(&a.state);
        let waited = if look == crate::agents::Look::Waiting {
            a.since
        } else {
            0
        };
        (look, waited, a.id)
    });
    shown
        .into_iter()
        .map(|a| list::Agent {
            id: a.id,
            name: a.name.clone(),
            waiting: a.state == "waiting",
            state: a.state,
        })
        .collect()
}

/// Do not disturb is on.
fn dnd() -> bool {
    crate::notifications::with(|c| c.stack.dnd.is_on())
}

/// The desktops of the palette's screen, as the bar knows them.
fn desks(shown: &ActionsShown) -> list::Desks {
    let Some(snapshot) = crate::desktops::snapshot(&shown.output) else {
        return list::Desks::default();
    };
    let count = snapshot.count;
    let mut windows = vec![Vec::new(); usize::from(count)];
    let mut window = None;
    for w in &snapshot.windows {
        if Some(w.id) == shown.window {
            window = Some(w.desktop);
        }
        if let Some(list) = windows.get_mut(usize::from(w.desktop))
            && w.look != hocus_link::Look::Minimized
        {
            list.push(w.rect);
        }
    }
    list::Desks {
        count,
        current: snapshot.current,
        window,
        names: (0..usize::from(count)).map(crate::desktops::name).collect(),
        windows,
    }
}

#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Closed,
    Open(Box<Open>),
}

/// What is carried out once the palette has faded out.
#[derive(Debug, Clone)]
enum Then {
    /// The compositor runs it.
    Tell(ToCompositor),
    /// The shell's own action `def`, with `arg`, on `output`.
    Shell {
        def: &'static Def,
        arg: Option<u64>,
        output: String,
    },
    /// The same search in the launcher, or the Settings.
    Elsewhere {
        to: Elsewhere,
        query: String,
        output: String,
        scale: u32,
    },
}

/// The palette, and its surfaces fading out until they go: the shell's own
/// (#773).
#[derive(Debug, Default)]
pub struct Palette {
    pub phase: Phase,
    closing: Vec<(window::Id, Instant, Option<Then>)>,
    /// The time a tick was asked for.
    asked: Option<Instant>,
}

/// Whether the palette is open, for the threads that do not own it (the
/// link's, a press outside; a key that reached another surface): a flag
/// it keeps, nothing they read of its state.
static OPEN: AtomicBool = AtomicBool::new(false);

/// What other threads and surfaces ask of the palette, as messages for
/// the loop that owns it ([`crate::surfaces::ask`]).
#[derive(Debug, Clone)]
pub enum Come {
    /// `Super+P`: it opens for that, or closes.
    Toggle(ActionsShown),
    /// A press on none of the shell's surfaces, the target gone, another
    /// surface opening, the lock: it closes.
    Close,
    /// A key that reached another of the shell's surfaces.
    Key(Pressed),
    /// The clipboard's text for the field of the palette on that surface.
    Pasted(window::Id, String),
    /// A screen reader clicks a row.
    Row(usize),
    /// What was due is due ([`Palette::due`]).
    Tick,
}

/// What screen readers are told of the palette ([`a11y`]).
#[derive(Debug, Clone)]
pub struct Seen {
    surface: window::Id,
    title: String,
    focus: String,
    size: (u32, u32),
}

/// Reads the recent actions.
pub fn start() {
    store::start();
}

/// It closes, if it is open: asked of the loop, from any thread.
pub fn close() {
    if OPEN.load(Ordering::Relaxed) {
        crate::surfaces::ask(Asked::Actions(Come::Close));
    }
}

/// A key that reached another of the shell's surfaces while the palette
/// is open: its, handed to the loop. Whether it was taken.
pub fn key(key: &keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> bool {
    if !OPEN.load(Ordering::Relaxed) {
        return false;
    }
    crate::surfaces::ask(Asked::Actions(Come::Key(Pressed {
        key: key.clone(),
        modifiers,
        text: text.map(str::to_owned),
    })));
    true
}

impl Palette {
    pub fn open_on(&self, id: window::Id) -> Option<&Open> {
        match &self.phase {
            Phase::Open(open) if open.surface == id => Some(open),
            _ => None,
        }
    }

    pub fn open_mut(&mut self, id: window::Id) -> Option<&mut Open> {
        match &mut self.phase {
            Phase::Open(open) if open.surface == id => Some(open),
            _ => None,
        }
    }

    fn surface(&self) -> Option<window::Id> {
        match &self.phase {
            Phase::Open(open) => Some(open.surface),
            Phase::Closed => None,
        }
    }

    /// Whether surface `id` is the palette's, open or fading out.
    pub fn is_surface(&self, id: window::Id) -> bool {
        self.open_on(id).is_some() || self.closing.iter().any(|(c, ..)| *c == id)
    }

    /// `come` taken in at `now`: what the shell then does.
    pub fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        let effects = match come {
            Come::Toggle(shown) => self.toggle(shown, now),
            Come::Close => {
                let mut effects = Vec::new();
                self.close(now, None, &mut effects);
                effects
            }
            Come::Key(pressed) => match self.surface() {
                Some(id) => {
                    let mut effects = self
                        .key_on(
                            id,
                            &pressed.key,
                            pressed.modifiers,
                            pressed.text.as_deref(),
                            now,
                        )
                        .unwrap_or_default();
                    crate::ui::touch();
                    effects.push(Effect::Wake(ACTIONS_NAMESPACE));
                    effects
                }
                None => Vec::new(),
            },
            Come::Pasted(id, pasted) => {
                let pasted = |entry: &mut Entry| match entry.paste(&pasted) {
                    true => Took::Edited,
                    false => Took::Same,
                };
                let (draws, mut effects) = self.edited(id, pasted);
                if draws {
                    crate::ui::touch();
                    effects.push(Effect::Wake(ACTIONS_NAMESPACE));
                }
                effects
            }
            Come::Row(at) => match self.surface() {
                Some(id) => self.act(id, at, now),
                None => Vec::new(),
            },
            Come::Tick => {
                self.asked = None;
                self.tick(now)
            }
        };
        self.said();
        effects
    }

    /// What the threads that do not own it are told of it: whether it is
    /// open, and what screen readers read.
    pub fn said(&self) {
        OPEN.store(self.surface().is_some(), Ordering::Relaxed);
        crate::a11y::shown(crate::a11y::Shown::Actions(self.seen()));
    }

    fn seen(&self) -> Option<Seen> {
        let Phase::Open(open) = &self.phase else {
            return None;
        };
        let focus = if open.rows.get(open.selected).is_some_and(Row::selectable) {
            format!("actions:Row({})", open.selected)
        } else {
            "actions:Field".to_owned()
        };
        Some(Seen {
            surface: open.surface,
            title: open.shown.title.clone(),
            focus,
            size: open.size,
        })
    }

    /// When a tick is due, if one was not asked for by then already.
    pub fn due(&mut self) -> Option<Instant> {
        let at = self.closing.iter().map(|(_, at, _)| *at).min()?;
        if self.asked.is_some_and(|asked| asked <= at) {
            return None;
        }
        self.asked = Some(at);
        Some(at)
    }

    /// It closes at `now`, `then` carried out once it has faded out: the
    /// keyboard goes back at once, the compositor fades it out.
    fn close(&mut self, now: Instant, then: Option<Then>, effects: &mut Vec<Effect>) {
        let Phase::Open(open) = std::mem::take(&mut self.phase) else {
            return;
        };
        tracing::debug!("the actions palette closes");
        self.closing.push((open.surface, now + ACTIONS_CLOSE, then));
        effects.push(Effect::Keyboard(open.surface, KeyboardInteractivity::None));
        effects.push(Effect::Tell(ToCompositor::Closing(
            ACTIONS_NAMESPACE.to_owned(),
        )));
    }

    /// `Super+P` (the compositor's line): it opens for `shown`, or closes.
    fn toggle(&mut self, shown: ActionsShown, now: Instant) -> Vec<Effect> {
        let words = words();
        let mut effects = Vec::new();
        if matches!(self.phase, Phase::Open(_)) {
            self.close(now, None, &mut effects);
            return effects;
        }
        let surface = window::Id::unique();
        let (area_x, area_width) = shown.area;
        let width = view::width(area_width as f32);
        let mut open = Open {
            surface,
            shown,
            query: Entry::new(field::Kind::Line, QUERY_MOST),
            step: None,
            rows: Vec::new(),
            selected: 0,
            scroll: 0.0,
            width,
            size: (0, 0),
            place: (0, 0),
            logged: String::new(),
        };
        open.relist(words);
        open.size = view::size(&open.rows, width);
        let left = area_x as f32 + (area_width as f32 - width) / 2.0;
        open.place = (hocus_link::ACTIONS_TOP as i32, left.round().max(0.0) as i32);
        tracing::debug!(
            output = open.shown.output,
            window = open.shown.window,
            place = ?open.place,
            "the actions palette opens"
        );
        effects.push(Effect::Actions {
            id: surface,
            output: open.shown.output.clone(),
            size: open.size,
            margin: (open.place.0, 0, 0, open.place.1),
        });
        self.phase = Phase::Open(Box::new(open));
        // One at a time: the bar's popover, the launcher and the emoji
        // picker close.
        crate::popover::event(crate::popover::Event::PressElsewhere);
        crate::launcher::close();
        crate::emoji::close();
        effects
    }

    /// What is due at `now`: the surfaces faded out go, and what was
    /// chosen is carried out.
    fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        let mut due = Vec::new();
        self.closing.retain_mut(|(id, at, then)| {
            if *at > now {
                return true;
            }
            effects.push(Effect::Remove(*id));
            due.extend(then.take());
            false
        });
        for then in due {
            effects.extend(carry_out(then));
        }
        effects
    }

    /// Row `at` of the palette on `id` is chosen: an action runs (once the
    /// palette has faded out), a step opens, or nothing for an inert one.
    pub fn act(&mut self, id: window::Id, at: usize, now: Instant) -> Vec<Effect> {
        let words = words();
        let mut effects = Vec::new();
        let Some(open) = self.open_mut(id) else {
            return effects;
        };
        let Some(row) = open.rows.get(at).cloned() else {
            return effects;
        };
        if let Some(why) = row.inert() {
            tracing::debug!(?why, "an inert row: nothing runs");
            return effects;
        }
        let output = open.shown.output.clone();
        let window = open.shown.window;
        let run = |def: &'static Def, arg: Option<u64>| {
            store::ran(def.id);
            match def.runs {
                Runs::Compositor => Then::Tell(ToCompositor::Action(RunAction {
                    id: def.id.to_owned(),
                    arg,
                    window,
                })),
                Runs::Shell => Then::Shell {
                    def,
                    arg,
                    output: output.clone(),
                },
            }
        };
        let then = match row {
            Row::Action(a) if a.def.step.is_some() && a.arg.is_none() => {
                enter_step(open, a.def, words);
                effects.extend(open.refit());
                return effects;
            }
            Row::Action(a) => run(a.def, a.arg),
            Row::Target(t) => {
                let Some(step) = &open.step else {
                    return effects;
                };
                let arg = match t.target {
                    Target::Desktop { index, .. } => u64::from(index),
                    Target::Agent { id, .. } => id,
                };
                run(step.def, Some(arg))
            }
            Row::Elsewhere(to) => Then::Elsewhere {
                to,
                query: open.query.text().trim().to_owned(),
                output: output.clone(),
                scale: open.shown.scale,
            },
            Row::Head(_) | Row::Nothing(_) => return effects,
        };
        self.close(now, Some(then), &mut effects);
        effects
    }

    /// A key on the open palette on `id`: what it does, if it was its.
    pub fn key_on(
        &mut self,
        id: window::Id,
        key: &keyboard::Key,
        modifiers: keyboard::Modifiers,
        text: Option<&str>,
        now: Instant,
    ) -> Option<Vec<Effect>> {
        use keyboard::{Key, key::Named};
        enum Then {
            Nothing,
            Close,
            Act(usize),
            Draw(Option<Effect>),
            Paste,
        }
        let words = words();
        let open = self.open_mut(id)?;
        let moved = |open: &mut Open, by: i32| {
            open.step_selection(by);
            Then::Draw(open.refit())
        };
        let changed = |open: &mut Open| {
            open.relist(words);
            Then::Draw(open.refit())
        };
        let then = match key {
            Key::Named(Named::Escape) => {
                if open.step.is_some() {
                    leave_step(open, words);
                    Then::Draw(open.refit())
                } else {
                    Then::Close
                }
            }
            Key::Named(Named::Enter) => Then::Act(open.selected),
            Key::Named(Named::Tab) => match open.rows.get(open.selected) {
                Some(Row::Action(a)) if a.def.step.is_some() && a.arg.is_none() => {
                    Then::Act(open.selected)
                }
                _ => Then::Nothing,
            },
            // Right goes into a step once the caret has nowhere to go.
            Key::Named(Named::ArrowRight)
                if modifiers.is_empty()
                    && open.query.at_end()
                    && matches!(
                        open.rows.get(open.selected),
                        Some(Row::Action(a)) if a.def.step.is_some() && a.arg.is_none()
                    ) =>
            {
                Then::Act(open.selected)
            }
            Key::Named(Named::ArrowDown) => moved(open, 1),
            Key::Named(Named::ArrowUp) => moved(open, -1),
            Key::Named(Named::PageDown) => moved(open, view::MOST_ROWS as i32),
            Key::Named(Named::PageUp) => moved(open, -(view::MOST_ROWS as i32)),
            // In a step's empty field: back to the actions.
            Key::Named(Named::Backspace) if open.query.is_empty() && open.step.is_some() => {
                leave_step(open, words);
                Then::Draw(open.refit())
            }
            _ => {
                // A digit picks a desktop at once.
                let digit = open
                    .step
                    .as_ref()
                    .filter(|step| {
                        step.def.step == Some(Step::Desktop)
                            && !(modifiers.control() || modifiers.alt() || modifiers.logo())
                    })
                    .and(text)
                    .and_then(|t| t.parse::<u8>().ok())
                    .filter(|d| (1..=9).contains(d));
                match digit {
                    Some(digit) => open
                        .rows
                        .iter()
                        .position(|r| matches!(r, Row::Target(t) if t.digit == Some(digit)))
                        .map_or(Then::Nothing, Then::Act),
                    None => match open.query.key(key, modifiers, text) {
                        Took::No | Took::Same => Then::Nothing,
                        Took::Moved => Then::Draw(None),
                        Took::Edited => changed(open),
                        Took::Paste => Then::Paste,
                    },
                }
            }
        };
        Some(match then {
            Then::Nothing => Vec::new(),
            Then::Close => {
                let mut effects = Vec::new();
                self.close(now, None, &mut effects);
                effects
            }
            Then::Act(at) => self.act(id, at, now),
            Then::Draw(resize) => resize.into_iter().collect(),
            Then::Paste => {
                paste(id);
                Vec::new()
            }
        })
    }

    /// The field of the palette on `id` edited: its list follows, and its
    /// surface its height. Whether it draws again.
    fn edited(
        &mut self,
        id: window::Id,
        edit: impl FnOnce(&mut Entry) -> Took,
    ) -> (bool, Vec<Effect>) {
        let words = words();
        let Some(open) = self.open_mut(id) else {
            return (false, Vec::new());
        };
        let took = edit(&mut open.query);
        let resize = if took == Took::Edited {
            open.relist(words);
            open.refit()
        } else {
            None
        };
        (took.draws(), resize.into_iter().collect())
    }

    /// What the field of the palette on `id` took of an input method or
    /// of the pointer: its list follows what was typed; a paste is asked
    /// for.
    pub fn took(&mut self, id: window::Id, took: Took, words: Words) -> Vec<Effect> {
        match took {
            Took::Edited => self
                .open_mut(id)
                .and_then(|open| {
                    open.relist(words);
                    open.refit()
                })
                .into_iter()
                .collect(),
            Took::Paste => {
                paste(id);
                Vec::new()
            }
            Took::No | Took::Same | Took::Moved => Vec::new(),
        }
    }
}

impl Surface for Lent<Palette> {
    type Message = Come;

    fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        self.get_mut().update(come, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        self.read(|p| p.is_surface(id))?
            .then(|| view::view(self, id, words(), context.look.glass))
    }
}

/// Carries out what was chosen.
fn carry_out(then: Then) -> Vec<Effect> {
    match then {
        Then::Tell(message) => vec![Effect::Tell(message)],
        Then::Shell { def, arg, output } => shell_action(def, arg, &output),
        Then::Elsewhere {
            to: Elsewhere::Launcher,
            query,
            output,
            scale,
        } => {
            crate::launcher::open_with(output, scale, &query);
            Vec::new()
        }
        Then::Elsewhere {
            to: Elsewhere::Settings,
            query,
            ..
        } => {
            crate::popover::open_settings_searching(&query);
            Vec::new()
        }
    }
}

/// The shell's own action `def` on `output`'s bar.
fn shell_action(def: &'static Def, arg: Option<u64>, output: &str) -> Vec<Effect> {
    use crate::popover::Item;
    tracing::info!(action = def.id, arg, "the palette runs");
    if let Some(page) = def.settings_page() {
        crate::popover::open_settings_page(page);
        return Vec::new();
    }
    let open = |item: Item, near: Option<Item>| crate::popover::open_asked(output, item, near);
    match def.id {
        "waiting-agent" => {
            if let Some(agent) = agents().into_iter().find(|a| a.waiting) {
                return vec![Effect::Tell(ToCompositor::GoToAgent {
                    id: agent.id,
                    row: None,
                })];
            }
        }
        "go-to-agent" => {
            if let Some(id) = arg {
                return vec![Effect::Tell(ToCompositor::GoToAgent { id, row: None })];
            }
        }
        "briefing" => open(Item::Secretary, None),
        "sources" => open(Item::Sources, Some(Item::Secretary)),
        "notifications" => open(Item::Notifications, None),
        "calendar" => open(Item::Calendar, None),
        "sound" => open(Item::Sound, None),
        "wifi" => open(Item::Wifi, None),
        "add-task" => crate::tasks::act(crate::tasks::Act::Add {
            bar: crate::popover::bar_for(output),
        }),
        "dnd" => {
            use crate::notifications::dnd::{Choice, Dnd};
            let dnd = if dnd() {
                Dnd::Off
            } else {
                Dnd::starting(Choice::Hour, Instant::now(), &crate::notifications::wall())
            };
            crate::notifications::input(crate::notifications::stack::Input::Dnd(dnd));
        }
        other => tracing::warn!(action = other, "nothing runs it in the shell"),
    }
    Vec::new()
}

/// The list turns into `def`'s targets.
fn enter_step(open: &mut Open, def: &'static Def, words: Words) {
    tracing::debug!(action = def.id, "the palette asks for a target");
    let before = open.query.take();
    open.step = Some(StepOpen { def, before });
    open.relist(words);
}

/// Back from a step to the actions, the query as it was.
fn leave_step(open: &mut Open, words: Words) {
    if let Some(step) = open.step.take() {
        open.query.set(&step.before);
        open.relist(words);
    }
}

/// The clipboard's text, on one line, into the field of the palette on
/// `id` at its caret, read off the loop and handed to it.
fn paste(id: window::Id) {
    crate::paste::request(crate::paste::Clean::Spaced, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        crate::surfaces::ask(Asked::Actions(Come::Pasted(id, pasted.to_owned())));
    });
}

/// The action catalogue's size, for the footer of the empty field.
pub fn all() -> usize {
    actions::catalogue().len()
}

/// The open palette for screen readers (A.1): a dialog named "Actions",
/// described by its target, holding what its controls declared (its
/// field, its rows as list items whose selection the arrows move; an
/// inert row disabled, its reason in its description). A row's key is
/// its control's id (`actions:Row(N)`), for a click on it.
pub fn a11y(seen: &Seen) -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    let words = words();
    let rows = crate::a11y::rows::of(seen.surface)?;
    let mut dialog = Node::new("dialog", Role::Dialog, words.title()).children(rows.nodes);
    if !seen.title.is_empty() {
        dialog = dialog.describe(seen.title.clone());
    }
    let mut window = Window::new("actions", words.title());
    window.nodes = vec![dialog];
    window.active = true;
    window.focus = Some(seen.focus.clone());
    window.size = Some((seen.size.0 as f32, seen.size.1 as f32));
    Some(window)
}

/// A screen reader clicks row `key` (`actions:Row(N)`) of the open
/// palette: asked of the loop.
pub fn a11y_click(key: &str) {
    if let Some(at) = view::row_of_id(key) {
        crate::surfaces::ask(Asked::Actions(Come::Row(at)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The palette alone (#773): `Super+P` opens it, typing filters its
    /// list as its view says, a key from elsewhere is its, `Escape` closes
    /// it and its tick takes the surface away once faded out.
    #[test]
    fn the_palette_alone_opens_filters_closes_and_goes() {
        let mut palette = Palette::default();
        let now = Instant::now();
        let shown = ActionsShown {
            output: "eDP-1".into(),
            scale: 120,
            window: Some(7),
            title: "kitty".into(),
            area: (0, 1920),
            on: Vec::new(),
            inert: Vec::new(),
            shortcuts: Vec::new(),
        };
        let effects = palette.update(Come::Toggle(shown), now);
        let Some(Effect::Actions { id, .. }) = effects.first() else {
            panic!("its surface opened: {effects:?}");
        };
        let id = *id;
        let words = Words::new(Lang::English);
        let shows = |p: &Palette| p.open_on(id).map(|o| view::shows(o, words));
        let all = shows(&palette).expect("open");
        let typed = Pressed {
            key: keyboard::Key::Character("z".into()),
            modifiers: keyboard::Modifiers::empty(),
            text: Some("z".into()),
        };
        palette.update(Come::Key(typed), now);
        assert_eq!(
            palette
                .open_on(id)
                .map(|o| o.query.text().to_owned())
                .as_deref(),
            Some("z")
        );
        assert_ne!(
            shows(&palette).expect("open"),
            all,
            "the list follows the query"
        );
        let escape = keyboard::Key::Named(keyboard::key::Named::Escape);
        let effects = palette
            .key_on(id, &escape, keyboard::Modifiers::empty(), None, now)
            .expect("its key");
        assert!(effects.contains(&Effect::Keyboard(id, KeyboardInteractivity::None)));
        assert!(palette.open_on(id).is_none() && palette.is_surface(id));
        let due = palette.due().expect("a tick due");
        assert_eq!(palette.due(), None, "asked once");
        let effects = palette.update(Come::Tick, due);
        assert_eq!(effects, vec![Effect::Remove(id)]);
        assert!(!palette.is_surface(id));
    }
}
