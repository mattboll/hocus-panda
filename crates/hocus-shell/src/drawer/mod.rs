//! The secretary's drawer (docs/design/secretary.md §5), in assistant
//! mode only: `Super+S` or a click on her head opens it beside the bar of
//! that screen, 420 px wide (360 on a screen under 1600 px), its top and
//! bottom on the bar's, over the windows; "Ce qu'elle sait" widens it to
//! 720 px. Its header (her head in her mood, her status line, the gear and
//! ✕), its tabs, the agents' line (4.6's words, which unfold her briefing
//! in it), the thread ([`draw`]) and the field.
//!
//! A press elsewhere (a window, the desktop) closes it, as the bar's
//! menus do (#755); so do `Esc` with the keyboard in it, `Super+S`, a
//! click on her head or ✕. It opens again where it was (its tab, what was
//! being typed, where the thread was), and an answer being written goes
//! on meanwhile and waits in the thread. Its surface
//! ([`SECRETARY_NAMESPACE`]) asks for the keyboard on demand: the
//! compositor gives it the keyboard as it first shows (its field is
//! focused) and at each press on it. The compositor moves it in and out
//! and blurs behind it, as a popover.
//!
//! Her briefings (lot 5) are thread items: the morning's folded on one
//! card with "Ouvrir le point", which unfolds it in place of the thread
//! ([`Open::point`]), as her peek's bubble does. In a public moment (a
//! meeting, a share, the camera, [`crate::speak::moment`]) no perso title
//! shows, in the thread or a briefing: only how many are hidden, and
//! "Montrer", which reveals them by a deliberate click, never while the
//! screen is shared.
//!
//! What she says reaches it from the shell's line to her
//! ([`crate::secretary`]), off iced's loop: the drawer is woken only while
//! it is open. Closed, nothing of hers draws.

pub mod draw;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{DRAWER_CLOSE, DRAWER_WIDE, EDGE, SECRETARY_NAMESPACE, ToCompositor};
use hocus_secretary_link::{
    Body, Decision, Mood, Notebook,
    knows::{Act, Do, Kind, Query},
};
use iced::{Point, Size, keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use crate::{clock::time::Lang, popover::Effect};

/// The characters the field keeps at most.
const FIELD_MOST: usize = 2000;

/// The drawer's words, in the shell's language.
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

    pub fn pick(self, fr: &'static str, en: &'static str) -> &'static str {
        if self.fr { fr } else { en }
    }

    pub fn title(self) -> &'static str {
        self.pick("Ta secrétaire", "Your secretary")
    }

    pub fn talk(self) -> &'static str {
        self.pick("Discussion", "Conversation")
    }

    pub fn knows(self) -> &'static str {
        self.pick("Ce qu’elle sait", "What she knows")
    }

    pub fn placeholder(self) -> &'static str {
        self.pick("Dis-lui quelque chose…", "Tell her something…")
    }

    pub fn keys(self) -> &'static str {
        self.pick("Entrée envoie · Échap ferme", "Enter sends · Esc closes")
    }

    pub fn writing(self) -> &'static str {
        self.pick("elle écrit", "she is writing")
    }

    pub fn unavailable(self) -> &'static str {
        self.pick("Indisponible", "Unavailable")
    }

    pub fn connecting(self) -> &'static str {
        self.pick("Elle arrive…", "She is coming…")
    }

    pub fn assumed(self) -> &'static str {
        self.pick("Supposé", "Assumed")
    }

    pub fn correct(self) -> &'static str {
        self.pick("corriger", "correct")
    }

    pub fn undo(self) -> &'static str {
        self.pick("Annuler", "Undo")
    }

    pub fn back(self) -> &'static str {
        self.pick("‹ Discussion", "‹ Conversation")
    }

    pub fn open_point(self) -> &'static str {
        self.pick("Ouvrir le point", "Open the briefing")
    }

    pub fn show(self) -> &'static str {
        self.pick("Montrer", "Show")
    }

    pub fn hidden(self, n: usize) -> String {
        match (self.fr, n) {
            (true, 1) => "1 chose privée masquée".to_owned(),
            (true, n) => format!("{n} choses privées masquées"),
            (false, 1) => "1 private thing hidden".to_owned(),
            (false, n) => format!("{n} private things hidden"),
        }
    }

    pub fn night(self) -> &'static str {
        self.pick("CETTE NUIT, TES AGENTS", "LAST NIGHT, YOUR AGENTS")
    }

    pub fn agents(self, n: usize) -> String {
        match (n, self.fr) {
            (1, true) => "1 agent ›".to_owned(),
            (1, false) => "1 agent ›".to_owned(),
            (n, _) => format!("{n} agents ›"),
        }
    }

    pub fn waits(self, name: &str) -> String {
        if self.fr {
            format!("{name} t’attend")
        } else {
            format!("{name} is waiting")
        }
    }

    pub fn stuck(self, name: &str) -> String {
        if self.fr {
            format!("{name} bloqué")
        } else {
            format!("{name} stuck")
        }
    }

    pub fn today(self) -> &'static str {
        self.pick("Aujourd’hui", "Today")
    }

    pub fn knows_subtitle(self) -> &'static str {
        self.pick(
            "Tout ce qu’elle sait, d’où elle le tient",
            "All she knows, and where she has it from",
        )
    }

    pub fn search(self) -> &'static str {
        self.pick(
            "Chercher : une personne, un projet, une règle…",
            "Search: a person, a project, a rule…",
        )
    }

    pub fn notebooks(self) -> [&'static str; 3] {
        if self.fr {
            ["Tout", "Travail", "Privé"]
        } else {
            ["All", "Work", "Private"]
        }
    }

    /// An act's button, for an item (`seen:`, `edit:` or another).
    pub fn act(self, act: Act, item: &str) -> &'static str {
        let seen = item.starts_with("seen:");
        let merge = item.starts_with("edit:");
        match act {
            Act::Edit => self.pick("Modifier", "Edit"),
            Act::Close => self.pick("N’est plus vrai", "No longer true"),
            Act::Forget => self.pick("Oublier", "Forget"),
            Act::Keep => self.pick("Garder", "Keep"),
            Act::Accept if seen => self.pick("Oui", "Yes"),
            Act::Accept => self.pick("C’est vrai", "It’s true"),
            Act::Reject if seen => self.pick("Non", "No"),
            Act::Reject if merge => self.pick("Défaire", "Undo"),
            Act::Reject => self.pick("Oublier", "Forget"),
            Act::Later => self.pick("Plus tard", "Later"),
        }
    }

    pub fn memories(self, n: u64) -> String {
        match (self.fr, n) {
            (true, 1) => "1 souvenir, sur cette machine, chiffré".to_owned(),
            (true, n) => format!("{n} souvenirs, sur cette machine, chiffrés"),
            (false, 1) => "1 memory, on this machine, encrypted".to_owned(),
            (false, n) => format!("{n} memories, on this machine, encrypted"),
        }
    }

    pub fn export(self) -> &'static str {
        self.pick("Exporter", "Export")
    }

    pub fn forget_perso(self) -> &'static str {
        self.pick("Oublier tout le privé…", "Forget everything private…")
    }

    pub fn forget_all(self) -> &'static str {
        self.pick("Tout oublier…", "Forget everything…")
    }

    pub fn open_source(self) -> &'static str {
        self.pick("Ouvrir la source ↗", "Open the source ↗")
    }

    /// The confirmation of a forget of a whole notebook: its title, its
    /// words, its button.
    pub fn confirm(self, what: Confirm) -> (&'static str, &'static str, &'static str) {
        match what {
            Confirm::Perso => (
                self.pick("Oublier tout le privé ?", "Forget everything private?"),
                self.pick(
                    "Tout ce qu’elle sait de ta vie privée — proches, dossiers, souvenirs — sera effacé de cette machine, résumés compris. Le travail reste. Ça ne s’annule pas.",
                    "All she knows of your private life — close ones, files, memories — will be erased from this machine, summaries included. Work stays. This cannot be undone.",
                ),
                self.pick("Oublier tout le privé", "Forget everything private"),
            ),
            Confirm::All => (
                self.pick("Tout oublier ?", "Forget everything?"),
                self.pick(
                    "Tout ce qu’elle sait, travail et privé, sera effacé de cette machine. Ça ne s’annule pas.",
                    "All she knows, work and private, will be erased from this machine. This cannot be undone.",
                ),
                self.pick("Tout oublier", "Forget everything"),
            ),
        }
    }

    pub fn keep_it(self) -> &'static str {
        self.pick("Garder", "Keep")
    }

    /// "Modifier" from "Ce qu'elle sait": the field's start.
    pub fn about(self, title: &str) -> String {
        if self.fr {
            format!("À propos de « {title} » : ")
        } else {
            format!("About “{title}”: ")
        }
    }
}

/// What a whole forget asks to confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Confirm {
    Perso,
    All,
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Its two tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Talk,
    Knows,
}

/// What can be clicked in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Target {
    Close,
    Gear,
    Tab(Tab),
    /// The agents' line: her briefing unfolds in the drawer.
    Agents,
    /// Back from the briefing to the thread.
    Back,
    /// An agent's line of the briefing: it goes to the agent.
    Agent(u64),
    /// Quick reply `choice` of question `item`.
    Reply {
        item: u64,
        choice: u32,
    },
    /// A card's button.
    Button {
        item: u64,
        decision: Decision,
    },
    /// A consent card's « C'est privé », checked or not before its
    /// "Autoriser" (#752).
    Private {
        item: u64,
    },
    /// An Agenda's consent card's calendar `index`, read or left out.
    Calendar {
        item: u64,
        index: u32,
    },
    /// "Annuler" under what she kept.
    Undo(u64),
    /// "corriger" under an assumption: its sentence into the field.
    Correct(u64),
    /// "Ouvrir le point": briefing `item` unfolds in the drawer.
    Point(u64),
    /// Her status line: her triage queue unfolds in the drawer (#754).
    Status,
    /// "Ne pas trier, chercher si besoin" on the queue's group `index`.
    Aside(u32),
    /// "Trier quand même" on the set-aside group `index`.
    Unaside(u32),
    /// "Montrer": the perso items hidden in a public moment show.
    Reveal,
    Field,
    Send,
    /// Her key's field and its "Enregistrer" (#752).
    KeyField,
    KeySave,
    /// "Ce qu'elle sait": the notebook switch (0 Tout, 1 Pro, 2 Perso).
    Notebook(u8),
    /// A filter chip, by its place.
    Chip(u8),
    Search,
    /// Item `index` (its place in the view): an act's button.
    Known {
        index: u32,
        act: Act,
    },
    /// An item's line: its provenance unfolds.
    Expand(u32),
    /// Its source opens.
    Open(u32),
    Export,
    ForgetPerso,
    ForgetAll,
    /// The confirmation's buttons: forget, or keep.
    Confirm(bool),
    ToastUndo,
}

impl Target {
    /// Its name in the log's list of places (tests click there).
    pub fn name(self) -> String {
        match self {
            Self::Close => "Close".to_owned(),
            Self::Gear => "Gear".to_owned(),
            Self::Tab(Tab::Talk) => "TabTalk".to_owned(),
            Self::Tab(Tab::Knows) => "TabKnows".to_owned(),
            Self::Agents => "Agents".to_owned(),
            Self::Back => "Back".to_owned(),
            Self::Agent(id) => format!("Agent({id})"),
            Self::Reply { item, choice } => format!("Reply({item},{choice})"),
            Self::Button { item, decision } => format!("Button({item},{decision:?})"),
            Self::Private { item } => format!("Private({item})"),
            Self::Calendar { item, index } => format!("Calendar({item},{index})"),
            Self::Undo(item) => format!("Undo({item})"),
            Self::Correct(item) => format!("Correct({item})"),
            Self::Point(item) => format!("Point({item})"),
            Self::Status => "Status".to_owned(),
            Self::Aside(n) => format!("Aside({n})"),
            Self::Unaside(n) => format!("Unaside({n})"),
            Self::Reveal => "Reveal".to_owned(),
            Self::Field => "Field".to_owned(),
            Self::Send => "Send".to_owned(),
            Self::KeyField => "KeyField".to_owned(),
            Self::KeySave => "KeySave".to_owned(),
            Self::Notebook(n) => format!("Notebook({n})"),
            Self::Chip(n) => format!("Chip({n})"),
            Self::Search => "Search".to_owned(),
            Self::Known { index, act } => format!("Known({index},{act:?})"),
            Self::Expand(n) => format!("Expand({n})"),
            Self::Open(n) => format!("Open({n})"),
            Self::Export => "Export".to_owned(),
            Self::ForgetPerso => "ForgetPerso".to_owned(),
            Self::ForgetAll => "ForgetAll".to_owned(),
            Self::Confirm(yes) => format!("Confirm({yes})"),
            Self::ToastUndo => "ToastUndo".to_owned(),
        }
    }
}

/// The drawer open.
#[derive(Debug)]
pub struct Open {
    pub surface: window::Id,
    /// Its bar (its screen's name).
    pub bar: String,
    /// Its width for the screen (420 or 360), and as it is now (720 on
    /// "Ce qu'elle sait").
    pub base: u32,
    pub width: u32,
    pub tab: Tab,
    pub field: String,
    /// The last sentence sent: `↑` on an empty field brings it back.
    pub recall: Option<String>,
    /// Her briefing unfolded in place of the thread.
    pub briefing: bool,
    /// Her briefing item `point` unfolded in place of the thread.
    pub point: Option<u64>,
    /// Her triage queue unfolded in place of the thread (#754).
    pub queue: bool,
    /// The perso items hidden in a public moment shown ("Montrer").
    pub reveal: bool,
    /// The thread scrolled up by this much from its bottom.
    pub scroll: f32,
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
    pub pointer: Option<Point>,
    /// Its size as last drawn.
    pub size: Size,
    pub opened: Instant,
    /// Her mood as last drawn, and since when: a change plays her motion.
    pub mood: Option<(Mood, Instant)>,
    /// Bumped at each change of its own that moves what it lays out.
    pub rev: u64,
    /// "Ce qu'elle sait": what is searched, the notebook and the kind
    /// shown, the item unfolded, how far the list is scrolled down, the
    /// whole forget waiting for its confirmation.
    pub search: String,
    pub book: Option<Notebook>,
    pub kind: Option<Kind>,
    /// The item unfolded, by what it is in her memory and its line
    /// ([`unfolded`]): the same item may show in two sections.
    pub expanded: Option<String>,
    pub knows_scroll: f32,
    pub confirm: Option<Confirm>,
    /// The notebook chosen on each consent card, before its "Autoriser"
    /// (none: the one it suggests).
    pub notebooks: std::collections::HashMap<u64, Notebook>,
    /// The calendars left out on each Agenda's consent card, by their ids.
    pub left_out: std::collections::HashMap<u64, std::collections::BTreeSet<String>>,
    /// The keyboard is on the drawer (it is given it as it first shows; a
    /// window clicked takes it back).
    pub focused: bool,
    /// Her key as it is pasted or typed in her band (never drawn, never
    /// logged; overwritten once sent), and whether the keyboard types
    /// there rather than in the field (#752).
    pub key: String,
    pub key_focus: bool,
}

impl Drop for Open {
    fn drop(&mut self) {
        hocus_secretary_link::wipe(&mut self.key);
    }
}

impl Open {
    fn changed(&mut self) {
        self.rev += 1;
    }

    /// What "Ce qu'elle sait" asks her now.
    pub fn query(&self) -> Query {
        Query {
            notebook: self.book,
            kind: self.kind,
            search: self.search.trim().to_owned(),
        }
    }
}

#[derive(Debug, Default)]
pub enum Phase {
    #[default]
    Closed,
    Open(Box<Open>),
}

/// The drawer, and its surfaces fading out until they go.
#[derive(Debug, Default)]
pub struct Drawer {
    pub phase: Phase,
    closing: Vec<(window::Id, Instant)>,
    /// Where it was when it closed: it opens there again (#755).
    kept: Option<Kept>,
}

/// What the drawer keeps from one opening to the next: its tab, what was
/// being typed, where the thread and the list were.
#[derive(Debug, Clone)]
struct Kept {
    tab: Tab,
    field: String,
    recall: Option<String>,
    scroll: f32,
    briefing: bool,
    point: Option<u64>,
    search: String,
    book: Option<Notebook>,
    kind: Option<Kind>,
    knows_scroll: f32,
}

impl Drawer {
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

    /// It closes at `now`: the keyboard goes back at once, the compositor
    /// fades it out, she is told.
    fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) -> bool {
        let Phase::Open(open) = std::mem::take(&mut self.phase) else {
            return false;
        };
        tracing::info!("the secretary's drawer closes");
        self.kept = Some(Kept {
            tab: open.tab,
            field: open.field.clone(),
            recall: open.recall.clone(),
            scroll: open.scroll,
            briefing: open.briefing,
            point: open.point,
            search: open.search.clone(),
            book: open.book,
            kind: open.kind,
            knows_scroll: open.knows_scroll,
        });
        self.closing.push((open.surface, now + DRAWER_CLOSE));
        effects.push(Effect::Keyboard(open.surface, KeyboardInteractivity::None));
        effects.push(Effect::Tell(ToCompositor::Closing(
            SECRETARY_NAMESPACE.to_owned(),
        )));
        true
    }
}

static DRAWER: Mutex<Option<Drawer>> = Mutex::new(None);

/// `with` on the drawer.
pub fn with<T>(with: impl FnOnce(&mut Drawer) -> T) -> T {
    let mut drawer = DRAWER.lock().unwrap_or_else(PoisonError::into_inner);
    with(drawer.get_or_insert_with(Drawer::default))
}

/// Whether surface `id` is the drawer's, open or fading out.
pub fn is_surface(id: window::Id) -> bool {
    with(|d| d.open_on(id).is_some() || d.closing.iter().any(|(c, _)| *c == id))
}

/// Whether it is open.
pub fn is_open() -> bool {
    with(|d| matches!(d.phase, Phase::Open(_)))
}

/// Its surface draws again.
pub fn wake() {
    crate::popover::send(vec![Effect::Wake(SECRETARY_NAMESPACE)]);
}

/// The agents changed, bumped each time: the drawer's line and her
/// briefing are laid out again.
static AGENTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How many times the agents changed.
pub fn agents_seen() -> u64 {
    AGENTS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The agents changed: an open drawer shows them.
pub fn agents_changed() {
    AGENTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    changed();
}

/// What she says changed: an open drawer draws it; closed, nothing.
pub fn changed() {
    if is_open() {
        wake();
    }
}

/// The drawer's margins beside `bar` (top, right, bottom, left), from the
/// screen's edge on the bar's side: past the bar and the gap.
fn margins(bar: &str) -> (i32, i32, i32, i32) {
    let form = crate::popover::with(|c| {
        c.bar_forms()
            .into_iter()
            .find(|(b, _)| b == bar)
            .map(|(_, f)| f)
    })
    .unwrap_or(hocus_link::Form::Full);
    (EDGE, EDGE + form.width() + EDGE, EDGE, 0)
}

/// A click on her head, or `Super+S` answered by the compositor with that
/// screen's width: it opens beside `output`'s bar, or closes wherever it
/// is. Assistant mode only.
pub fn toggle(output: &str, screen: u32) {
    if crate::secretary::state() == crate::secretary::State::Off {
        return;
    }
    let now = Instant::now();
    let (effects, opened) = with(|d| {
        let mut effects = Vec::new();
        if d.close(now, &mut effects) {
            return (effects, false);
        }
        let surface = window::Id::unique();
        let bar = crate::popover::bar_for(output);
        let base = hocus_link::drawer_width(screen);
        let margin = margins(&bar);
        let point = pending_point();
        // Where it was (#755), unless her peek asks for a briefing.
        let kept = d.kept.take().filter(|_| point.is_none()).unwrap_or(Kept {
            tab: Tab::Talk,
            field: String::new(),
            recall: None,
            scroll: if point.is_some() { f32::MAX } else { 0.0 },
            briefing: false,
            point,
            search: String::new(),
            book: None,
            kind: None,
            knows_scroll: 0.0,
        });
        let width = match kept.tab {
            Tab::Talk => base,
            Tab::Knows => DRAWER_WIDE,
        };
        tracing::info!(bar, width, screen, tab = ?kept.tab, "the secretary's drawer opens");
        effects.push(Effect::Drawer {
            id: surface,
            bar: bar.clone(),
            width,
            margin,
        });
        d.phase = Phase::Open(Box::new(Open {
            surface,
            bar,
            base,
            width,
            tab: kept.tab,
            field: kept.field,
            recall: kept.recall,
            briefing: kept.briefing,
            point: kept.point,
            queue: false,
            reveal: false,
            scroll: kept.scroll,
            hover: None,
            pressed: None,
            pointer: None,
            size: Size::new(base as f32, 0.0),
            opened: now,
            mood: None,
            rev: 0,
            search: kept.search,
            book: kept.book,
            kind: kept.kind,
            expanded: None,
            knows_scroll: kept.knows_scroll,
            confirm: None,
            notebooks: std::collections::HashMap::new(),
            left_out: std::collections::HashMap::new(),
            focused: true,
            key: String::new(),
            // Her key missing: the keyboard types in its field first.
            key_focus: crate::secretary::conversation(|c| {
                c.look.as_ref().is_some_and(|l| l.key.is_some())
            }),
        }));
        (effects, true)
    });
    if opened {
        // Her briefing popover, if it shows, gives way to the drawer.
        crate::popover::event(crate::popover::Event::PressElsewhere);
    }
    crate::secretary::drawer(opened);
    // Opened on "Ce qu'elle sait": she is asked for it again.
    if opened
        && let Some(query) = with(|d| match &d.phase {
            Phase::Open(open) if open.tab == Tab::Knows => Some(open.query()),
            _ => None,
        })
    {
        crate::secretary::knows(Some(query));
    }
    crate::popover::send(effects);
}

/// A press outside the shell (a window, the desktop): it closes, as the
/// bar's menus do (#755). An answer being written goes on, and waits in
/// the thread.
pub fn outside() {
    if is_open() {
        tracing::info!("a press elsewhere: the drawer closes");
        close();
    }
}

/// The item her peek's bubble asked the drawer to open on.
static PENDING: Mutex<Option<u64>> = Mutex::new(None);

/// The briefing to unfold as the drawer opens, if her peek asked for one.
fn pending_point() -> Option<u64> {
    let item = PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()?;
    crate::secretary::conversation(|c| {
        c.items
            .iter()
            .any(|i| i.id == item && matches!(i.body, Body::Briefing { .. }))
    })
    .then_some(item)
}

/// Her peek's bubble clicked beside `bar`: the drawer opens there (or
/// stays open) on `item`, a briefing unfolded.
pub fn open_on_item(bar: &str, item: Option<u64>) {
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = item;
    if is_open() {
        let point = pending_point();
        with(|d| {
            if let Phase::Open(open) = &mut d.phase {
                if std::mem::take(&mut open.queue) {
                    crate::secretary::queue(false);
                }
                open.tab = Tab::Talk;
                open.point = point;
                open.briefing = false;
                open.scroll = if point.is_some() { f32::MAX } else { 0.0 };
                open.changed();
            }
        });
        wake();
        return;
    }
    crate::popover::send(vec![Effect::Tell(ToCompositor::SecretaryDrawer(
        bar.to_owned(),
    ))]);
}

/// A click on her head (assistant mode): the compositor is asked for her
/// drawer on that bar's screen, and answers with its width.
pub fn head_clicked(bar: &str) {
    tracing::info!(bar, "a click on her head: her drawer");
    if is_open() {
        close();
        return;
    }
    crate::popover::send(vec![Effect::Tell(ToCompositor::SecretaryDrawer(
        bar.to_owned(),
    ))]);
}

/// It closes.
pub fn close() {
    let (effects, closed) = with(|d| {
        let mut effects = Vec::new();
        let closed = d.close(Instant::now(), &mut effects);
        (effects, closed)
    });
    if closed {
        crate::secretary::drawer(false);
        crate::popover::send(effects);
    }
}

/// When [`tick`] has something to do: a surface fading out, her toast
/// going.
pub fn deadline() -> Option<Instant> {
    let toast = if is_open() {
        crate::secretary::conversation(|c| c.toast.as_ref().map(|t| t.until))
    } else {
        None
    };
    with(|d| d.closing.iter().map(|(_, at)| *at).min())
        .into_iter()
        .chain(toast)
        .min()
}

/// What is due at `now`: its surface faded out goes; her toast goes.
pub fn tick(now: Instant) -> Vec<Effect> {
    let toast_over =
        crate::secretary::conversation(|c| c.toast.as_ref().is_some_and(|t| t.until <= now));
    if toast_over {
        crate::secretary::toast_gone();
    }
    with(|d| {
        let mut effects = Vec::new();
        d.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        if toast_over && matches!(d.phase, Phase::Open(_)) {
            effects.push(Effect::Wake(SECRETARY_NAMESPACE));
        }
        effects
    })
}

/// She is connected again: an open "Ce qu'elle sait" asks her again.
pub fn knows_again() {
    let query = with(|d| match &d.phase {
        Phase::Open(open) if open.tab == Tab::Knows => Some(open.query()),
        _ => None,
    });
    if let Some(query) = query {
        crate::secretary::knows(Some(query));
    }
}

/// She is connected again: an open queue's view asks her again.
pub fn queue_again() {
    if with(|d| matches!(&d.phase, Phase::Open(open) if open.queue)) {
        crate::secretary::queue(true);
    }
}

/// A bar changed form: the drawer follows its edge.
pub fn bar_changed(bar: &str) {
    let effect = with(|d| match &d.phase {
        Phase::Open(open) if open.bar == bar => Some(Effect::Resize {
            id: open.surface,
            size: (open.width, 0),
            margin: Some(margins(bar)),
        }),
        _ => None,
    });
    if let Some(effect) = effect {
        crate::popover::send(vec![effect]);
    }
}

/// What names an unfolded item: what it is, and its line.
pub fn unfolded(known: &hocus_secretary_link::knows::Known) -> String {
    format!("{}|{}", known.item, known.text)
}

/// What `target` does, clicked on the drawer on `id`.
pub fn act(id: window::Id, target: Target) {
    enum Then {
        Nothing,
        Draw,
        Close,
        Send(String),
        Key(String, String),
        Effects(Vec<Effect>),
        Undo(Option<u64>),
    }
    let then = with(|d| {
        let Some(open) = d.open_mut(id) else {
            return Then::Nothing;
        };
        match target {
            Target::Close => Then::Close,
            Target::Gear => {
                crate::popover::session::open_settings(Some("agents"));
                Then::Nothing
            }
            Target::Tab(tab) if tab != open.tab => {
                open.tab = tab;
                open.briefing = false;
                open.point = None;
                if std::mem::take(&mut open.queue) {
                    crate::secretary::queue(false);
                }
                open.confirm = None;
                open.expanded = None;
                crate::secretary::knows(match tab {
                    Tab::Knows => Some(open.query()),
                    Tab::Talk => None,
                });
                open.width = match tab {
                    Tab::Talk => open.base,
                    Tab::Knows => DRAWER_WIDE,
                };
                open.changed();
                tracing::info!(?tab, width = open.width, "the drawer's tab");
                Then::Effects(vec![Effect::Resize {
                    id: open.surface,
                    size: (open.width, 0),
                    margin: None,
                }])
            }
            Target::Tab(_) => Then::Nothing,
            Target::Agents => {
                if std::mem::take(&mut open.queue) {
                    crate::secretary::queue(false);
                }
                open.briefing = true;
                open.changed();
                Then::Draw
            }
            Target::Back => {
                open.briefing = false;
                open.point = None;
                if std::mem::take(&mut open.queue) {
                    crate::secretary::queue(false);
                }
                open.scroll = 0.0;
                open.changed();
                Then::Draw
            }
            Target::Status => {
                if open.queue {
                    open.queue = false;
                    crate::secretary::queue(false);
                } else {
                    tracing::info!("the drawer unfolds her queue");
                    open.tab = Tab::Talk;
                    open.queue = true;
                    open.point = None;
                    open.briefing = false;
                    open.scroll = f32::MAX;
                    crate::secretary::queue(true);
                }
                open.changed();
                Then::Draw
            }
            Target::Aside(index) | Target::Unaside(index) => {
                let aside = matches!(target, Target::Aside(_));
                let key = crate::secretary::conversation(|c| {
                    c.queue.as_ref().and_then(|q| {
                        let groups = if aside { &q.groups } else { &q.aside };
                        groups.get(index as usize).map(|g| g.key.clone())
                    })
                });
                if let Some(key) = key {
                    crate::secretary::queue_act(if aside {
                        hocus_secretary_link::queue::Act::Aside { key }
                    } else {
                        hocus_secretary_link::queue::Act::Back { key }
                    });
                }
                Then::Nothing
            }
            Target::Point(item) => {
                tracing::info!(item, "the drawer unfolds her briefing");
                crate::secretary::unfolded(item);
                if std::mem::take(&mut open.queue) {
                    crate::secretary::queue(false);
                }
                open.point = Some(item);
                open.briefing = false;
                open.scroll = f32::MAX;
                open.changed();
                Then::Draw
            }
            Target::Reveal => {
                // Never while the screen is shared.
                if crate::speak::moment() != hocus_secretary_link::Moment::Sharing {
                    tracing::info!("the perso items shown");
                    open.reveal = true;
                    open.changed();
                }
                Then::Draw
            }
            Target::Agent(agent) => {
                tracing::info!(agent, "the drawer goes to an agent");
                let mut effects = vec![Effect::Tell(ToCompositor::GoToAgent {
                    id: agent,
                    row: None,
                })];
                d.close(Instant::now(), &mut effects);
                Then::Effects(effects)
            }
            Target::Reply { item, choice } => {
                crate::secretary::choose(item, choice);
                Then::Nothing
            }
            Target::Button { item, decision } => {
                // A consent card's: the notebook chosen on it, else the
                // one it suggests.
                let suggested = crate::secretary::conversation(|c| {
                    c.items.iter().find_map(|i| match &i.body {
                        Body::Card { card } if i.id == item => {
                            card.sheet.as_ref().map(|s| s.notebook)
                        }
                        _ => None,
                    })
                });
                let notebook = suggested.map(|n| open.notebooks.get(&item).copied().unwrap_or(n));
                let left_out = open
                    .left_out
                    .get(&item)
                    .map(|l| l.iter().cloned().collect())
                    .unwrap_or_default();
                crate::secretary::decide(item, decision, notebook, left_out);
                Then::Nothing
            }
            Target::Calendar { item, index } => {
                let id = crate::secretary::conversation(|c| {
                    c.items.iter().find_map(|i| match &i.body {
                        Body::Card { card } if i.id == item => card
                            .sheet
                            .as_ref()
                            .and_then(|s| s.calendars.get(index as usize))
                            .map(|c| c.id.clone()),
                        _ => None,
                    })
                });
                if let Some(id) = id {
                    let left = open.left_out.entry(item).or_default();
                    if !left.remove(&id) {
                        left.insert(id);
                    }
                    tracing::info!(
                        item,
                        index,
                        left_out = left.len(),
                        "a consent card's calendar"
                    );
                    open.changed();
                }
                Then::Draw
            }
            Target::Private { item } => {
                let suggested = crate::secretary::conversation(|c| {
                    c.items.iter().find_map(|i| match &i.body {
                        Body::Card { card } if i.id == item => {
                            card.sheet.as_ref().map(|s| s.notebook)
                        }
                        _ => None,
                    })
                });
                let now = open
                    .notebooks
                    .get(&item)
                    .copied()
                    .or(suggested)
                    .unwrap_or_default();
                let next = match now {
                    Notebook::Pro => Notebook::Perso,
                    Notebook::Perso => Notebook::Pro,
                };
                tracing::info!(
                    item,
                    notebook = next.word(),
                    "a consent card's « C'est privé »"
                );
                open.notebooks.insert(item, next);
                open.changed();
                Then::Draw
            }
            Target::Undo(item) => {
                crate::secretary::undo(item);
                Then::Nothing
            }
            Target::Correct(item) => {
                let said = crate::secretary::conversation(|c| {
                    c.items
                        .iter()
                        .find(|i| i.id == item)
                        .and_then(|i| match &i.body {
                            Body::Her { text, .. } => Some(text.clone()),
                            _ => None,
                        })
                });
                if let Some(said) = said {
                    open.field = said;
                    open.changed();
                }
                Then::Draw
            }
            Target::Notebook(n) => {
                open.book = match n {
                    1 => Some(Notebook::Pro),
                    2 => Some(Notebook::Perso),
                    _ => None,
                };
                open.knows_scroll = 0.0;
                open.expanded = None;
                open.changed();
                crate::secretary::knows(Some(open.query()));
                Then::Draw
            }
            Target::Chip(n) => {
                let kind = crate::secretary::conversation(|c| {
                    c.knows
                        .as_ref()
                        .and_then(|k| k.chips.get(n as usize))
                        .map(|chip| chip.kind)
                });
                if let Some(kind) = kind {
                    open.kind = kind;
                    open.knows_scroll = 0.0;
                    open.expanded = None;
                    open.changed();
                    crate::secretary::knows(Some(open.query()));
                }
                Then::Draw
            }
            Target::Search => Then::Nothing,
            Target::Known { index, act } => {
                let known = crate::secretary::conversation(|c| {
                    c.knows
                        .as_ref()
                        .and_then(|k| k.items().nth(index as usize).cloned())
                });
                let Some(known) = known else {
                    return Then::Nothing;
                };
                if act == Act::Edit {
                    // Corrected by telling her, in the conversation.
                    let title = if known.title.is_empty() {
                        known.text.trim().to_owned()
                    } else {
                        known.title.clone()
                    };
                    open.field = words().about(&title);
                    open.tab = Tab::Talk;
                    open.width = open.base;
                    open.changed();
                    crate::secretary::knows(None);
                    return Then::Effects(vec![Effect::Resize {
                        id: open.surface,
                        size: (open.width, 0),
                        margin: None,
                    }]);
                }
                crate::secretary::memory(Do::Item {
                    item: known.item,
                    act,
                });
                Then::Nothing
            }
            Target::Expand(index) => {
                let item = crate::secretary::conversation(|c| {
                    c.knows
                        .as_ref()
                        .and_then(|k| k.items().nth(index as usize).map(unfolded))
                });
                open.expanded = if open.expanded == item { None } else { item };
                open.changed();
                Then::Draw
            }
            Target::Open(index) => {
                let url = crate::secretary::conversation(|c| {
                    c.knows
                        .as_ref()
                        .and_then(|k| k.items().nth(index as usize))
                        .and_then(|k| k.origin.open.clone())
                });
                match url.and_then(|u| crate::launcher::browse(&u)) {
                    Some(effect) => Then::Effects(vec![effect]),
                    None => Then::Nothing,
                }
            }
            Target::Export => {
                crate::secretary::memory(Do::Export {
                    notebook: open.book,
                });
                Then::Nothing
            }
            Target::ForgetPerso => {
                open.confirm = Some(Confirm::Perso);
                open.changed();
                Then::Draw
            }
            Target::ForgetAll => {
                open.confirm = Some(Confirm::All);
                open.changed();
                Then::Draw
            }
            Target::Confirm(yes) => {
                let what = open.confirm.take();
                open.changed();
                if yes {
                    match what {
                        Some(Confirm::Perso) => crate::secretary::memory(Do::ForgetNotebook {
                            notebook: Notebook::Perso,
                        }),
                        Some(Confirm::All) => {
                            for notebook in [Notebook::Perso, Notebook::Pro] {
                                crate::secretary::memory(Do::ForgetNotebook { notebook });
                            }
                        }
                        None => {}
                    }
                }
                Then::Draw
            }
            Target::ToastUndo => {
                let token =
                    crate::secretary::conversation(|c| c.toast.as_ref().and_then(|t| t.undo));
                // Out of the drawer's lock: the toast's going wakes it.
                Then::Undo(token)
            }
            Target::Field => {
                open.key_focus = false;
                open.changed();
                Then::Draw
            }
            Target::KeyField => {
                open.key_focus = true;
                open.changed();
                Then::Draw
            }
            Target::KeySave => match send_key(open) {
                Some((provider, key)) => Then::Key(provider, key),
                None => Then::Draw,
            },
            Target::Send => match sentence(open) {
                Some(text) => Then::Send(text),
                None => Then::Nothing,
            },
        }
    });
    match then {
        Then::Nothing => {}
        Then::Draw => wake(),
        Then::Undo(token) => {
            if let Some(token) = token {
                crate::secretary::memory(Do::Restore { token });
            }
            crate::secretary::toast_gone();
        }
        Then::Close => close(),
        Then::Send(text) => {
            crate::secretary::ask(text);
            wake();
        }
        Then::Key(provider, key) => {
            crate::secretary::give_key(provider, key);
            wake();
        }
        Then::Effects(effects) => {
            let closed = effects
                .iter()
                .any(|e| matches!(e, Effect::Tell(ToCompositor::Closing(_))));
            if closed {
                crate::secretary::drawer(false);
            }
            crate::popover::send(effects);
            wake();
        }
    }
}

/// Whether her drawer shows her key's field now.
fn key_band() -> Option<String> {
    crate::secretary::conversation(|c| c.look.as_ref().and_then(|l| l.key.clone()))
}

/// Her key's field taken to be sent to her (#752), out of the drawer's
/// lock: it empties, its text overwritten.
fn send_key(open: &mut Open) -> Option<(String, String)> {
    let provider = key_band()?;
    let key = open.key.trim().to_owned();
    hocus_secretary_link::wipe(&mut open.key);
    open.changed();
    if key.is_empty() || crate::secretary::conversation(|c| c.key_asked.is_some()) {
        return None;
    }
    Some((provider, key))
}

/// The keyboard came to the drawer on `id`, or left it: whether it
/// changed.
pub fn focused(id: window::Id, on: bool) -> bool {
    with(|d| {
        let Some(open) = d.open_mut(id) else {
            return false;
        };
        if open.focused == on {
            return false;
        }
        open.focused = on;
        open.changed();
        true
    })
}

/// A key in her key's field: `Enter` sends it, `Ctrl+V` pastes, nothing
/// copies it out; `Esc` closes, `Tab` goes to the field.
fn key_field_key(
    id: window::Id,
    open: &mut Open,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> Then {
    use keyboard::{Key, key::Named};
    match key {
        Key::Named(Named::Escape) => Then::Close,
        Key::Named(Named::Enter) => match send_key(open) {
            Some((provider, key)) => Then::Key(provider, key),
            None => Then::Draw,
        },
        Key::Named(Named::Tab) => {
            open.key_focus = false;
            open.changed();
            Then::Draw
        }
        Key::Named(Named::Backspace) => {
            if modifiers.control() {
                hocus_secretary_link::wipe(&mut open.key);
            } else {
                open.key.pop();
            }
            open.changed();
            Then::Draw
        }
        _ if crate::paste::asks(key, modifiers) => {
            let asked = crate::paste::request(crate::paste::Clean::Dropped, move |pasted| {
                let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
                    return;
                };
                let took = with(|d| match d.open_mut(id) {
                    Some(open) if open.key_focus && open.key.len() + pasted.len() <= KEY_MOST => {
                        open.key.push_str(pasted.trim());
                        open.changed();
                        true
                    }
                    _ => false,
                });
                if took {
                    tracing::debug!("a key pasted into her key's field");
                    wake();
                }
            });
            if !asked {
                tracing::debug!("nothing to paste into her key's field");
            }
            Then::Nothing
        }
        _ => match text {
            Some(typed)
                if !modifiers.control()
                    && !modifiers.alt()
                    && !modifiers.logo()
                    && !typed.is_empty()
                    && !typed.chars().any(char::is_control)
                    && open.key.len() + typed.len() <= KEY_MOST =>
            {
                open.key.push_str(typed);
                open.changed();
                Then::Draw
            }
            _ => Then::Nothing,
        },
    }
}

/// The longest key her field takes.
const KEY_MOST: usize = 512;

/// The field's sentence sent: it empties, and `↑` brings it back.
fn sentence(open: &mut Open) -> Option<String> {
    let text = open.field.trim().to_owned();
    if text.is_empty() {
        return None;
    }
    open.field.clear();
    open.recall = Some(text.clone());
    open.scroll = 0.0;
    open.changed();
    Some(text)
}

enum Then {
    Nothing,
    Close,
    Send(String),
    /// Her key, for this provider, to give her.
    Key(String, String),
    Draw,
}

/// A key in "Ce qu'elle sait": the search field takes what is typed;
/// `Esc` leaves a confirmation, else closes.
fn search_key(
    open: &mut Open,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> Then {
    use keyboard::{Key, key::Named};
    let before = open.search.clone();
    match key {
        Key::Named(Named::Escape) if open.confirm.is_some() => {
            open.confirm = None;
            open.changed();
            return Then::Draw;
        }
        Key::Named(Named::Escape) => return Then::Close,
        Key::Named(Named::Backspace) => {
            if modifiers.control() {
                let kept = open.search.trim_end().rfind(' ').map_or(0, |at| at + 1);
                open.search.truncate(kept);
            } else {
                open.search.pop();
            }
        }
        _ => match text {
            Some(typed)
                if !modifiers.control()
                    && !modifiers.alt()
                    && !modifiers.logo()
                    && !typed.is_empty()
                    && !typed.chars().any(char::is_control)
                    && open.search.chars().count() < 200 =>
            {
                open.search.push_str(typed);
            }
            _ => return Then::Nothing,
        },
    }
    if open.search != before {
        open.knows_scroll = 0.0;
        open.expanded = None;
        open.changed();
        crate::secretary::knows(Some(open.query()));
    }
    Then::Draw
}

/// A key on the open drawer on `id`: what it does. Whether it was its.
pub fn key_on(
    id: window::Id,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> bool {
    use keyboard::{Key, key::Named};
    let then = with(|d| {
        let open = d.open_mut(id)?;
        if open.tab == Tab::Knows {
            return Some(search_key(open, key, modifiers, text));
        }
        if open.key_focus && key_band().is_some() {
            return Some(key_field_key(id, open, key, modifiers, text));
        }
        Some(match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) if modifiers.shift() => {
                open.field.push('\n');
                open.changed();
                Then::Draw
            }
            Key::Named(Named::Enter) => match sentence(open) {
                Some(text) => Then::Send(text),
                None => Then::Nothing,
            },
            Key::Named(Named::ArrowUp) if open.field.is_empty() => match open.recall.clone() {
                Some(last) => {
                    open.field = last;
                    open.changed();
                    Then::Draw
                }
                None => Then::Nothing,
            },
            Key::Named(Named::Backspace) => {
                if open.field.is_empty() {
                    return Some(Then::Nothing);
                }
                if modifiers.control() {
                    let kept = open.field.trim_end().rfind(' ').map_or(0, |at| at + 1);
                    open.field.truncate(kept);
                } else {
                    open.field.pop();
                }
                open.changed();
                Then::Draw
            }
            _ => match text {
                Some(typed)
                    if !modifiers.control()
                        && !modifiers.alt()
                        && !modifiers.logo()
                        && !typed.is_empty()
                        && !typed.chars().any(char::is_control) =>
                {
                    if open.field.chars().count() + typed.chars().count() > FIELD_MOST {
                        return Some(Then::Nothing);
                    }
                    open.field.push_str(typed);
                    open.changed();
                    Then::Draw
                }
                _ => Then::Nothing,
            },
        })
    });
    match then {
        None => false,
        Some(Then::Nothing) => true,
        Some(Then::Close) => {
            close();
            true
        }
        Some(Then::Send(text)) => {
            crate::secretary::ask(text);
            true
        }
        Some(Then::Key(provider, key)) => {
            crate::secretary::give_key(provider, key);
            true
        }
        Some(Then::Draw) => true,
    }
}

/// How long her mood plays after a change; composing, it plays while it
/// lasts.
pub fn mood_window(mood: Mood) -> Duration {
    match mood {
        Mood::Type => Duration::from_secs(3600),
        _ => hocus_pandas::BURST,
    }
}

/// Her mood as the pandas draw it.
pub fn panda_mood(mood: Mood) -> hocus_pandas::SecretaryMood {
    use hocus_pandas::SecretaryMood as S;
    match mood {
        Mood::Listen => S::Listen,
        Mood::Type => S::Type,
        Mood::Ask => S::Ask,
        Mood::Nudge => S::Nudge,
        Mood::Happy => S::Happy,
        Mood::Worry => S::Worry,
        Mood::Sleep => S::Sleep,
    }
}
