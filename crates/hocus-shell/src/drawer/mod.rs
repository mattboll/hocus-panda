//! The secretary's drawer (docs/design/secretary.md §5), in assistant
//! mode only: `Super+S` or a click on her head opens it beside the bar of
//! that screen, 420 px wide (360 on a screen under 1600 px), its top and
//! bottom on the bar's, over the windows; "Ce qu'elle sait" widens it to
//! 720 px. Its header (her head in her mood, her status line, the gear and
//! ✕), its tabs, the agents' line (4.6's words, which unfold her briefing
//! in it), the thread ([`draw`]) and the field.
//!
//! It stays open when a window takes the keyboard, so that the user can
//! copy a name: `Esc` with the keyboard in it, `Super+S`, a click on her
//! head or ✕ close it. Its surface ([`SECRETARY_NAMESPACE`]) asks for the
//! keyboard on demand: the compositor gives it the keyboard as it first
//! shows (its field is focused), a window clicked takes it back. The
//! compositor moves it in and out and blurs behind it, as a popover.
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
use hocus_secretary_link::{Body, Decision, Mood};
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

    pub fn knows_soon(self) -> &'static str {
        self.pick(
            "Ce qu’elle sait de toi, de ton équipe et de tes projets s’affichera ici.",
            "What she knows of you, your team and your projects will show here.",
        )
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
    /// "Annuler" under what she kept.
    Undo(u64),
    /// "corriger" under an assumption: its sentence into the field.
    Correct(u64),
    Field,
    Send,
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
            Self::Undo(item) => format!("Undo({item})"),
            Self::Correct(item) => format!("Correct({item})"),
            Self::Field => "Field".to_owned(),
            Self::Send => "Send".to_owned(),
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
}

impl Open {
    fn changed(&mut self) {
        self.rev += 1;
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
        tracing::info!(bar, width = base, screen, "the secretary's drawer opens");
        effects.push(Effect::Drawer {
            id: surface,
            bar: bar.clone(),
            width: base,
            margin,
        });
        d.phase = Phase::Open(Box::new(Open {
            surface,
            bar,
            base,
            width: base,
            tab: Tab::Talk,
            field: String::new(),
            recall: None,
            briefing: false,
            scroll: 0.0,
            hover: None,
            pressed: None,
            pointer: None,
            size: Size::new(base as f32, 0.0),
            opened: now,
            mood: None,
            rev: 0,
        }));
        (effects, true)
    });
    if opened {
        // Her briefing popover, if it shows, gives way to the drawer.
        crate::popover::event(crate::popover::Event::PressElsewhere);
    }
    crate::secretary::drawer(opened);
    crate::popover::send(effects);
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

/// When [`tick`] has something to do.
pub fn deadline() -> Option<Instant> {
    with(|d| d.closing.iter().map(|(_, at)| *at).min())
}

/// What is due at `now`: its surface faded out goes.
pub fn tick(now: Instant) -> Vec<Effect> {
    with(|d| {
        let mut effects = Vec::new();
        d.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        effects
    })
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

/// What `target` does, clicked on the drawer on `id`.
pub fn act(id: window::Id, target: Target) {
    enum Then {
        Nothing,
        Draw,
        Close,
        Send(String),
        Effects(Vec<Effect>),
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
                open.briefing = true;
                open.changed();
                Then::Draw
            }
            Target::Back => {
                open.briefing = false;
                open.changed();
                Then::Draw
            }
            Target::Agent(agent) => {
                tracing::info!(agent, "the drawer goes to an agent");
                let mut effects = vec![Effect::Tell(ToCompositor::GoToAgent {
                    id: agent,
                    row: None,
                })];
                d.close(Instant::now(), &mut effects);
                return Then::Effects(effects);
            }
            Target::Reply { item, choice } => {
                crate::secretary::choose(item, choice);
                Then::Nothing
            }
            Target::Button { item, decision } => {
                crate::secretary::decide(item, decision);
                Then::Nothing
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
            Target::Field => Then::Nothing,
            Target::Send => match sentence(open) {
                Some(text) => Then::Send(text),
                None => Then::Nothing,
            },
        }
    });
    match then {
        Then::Nothing => {}
        Then::Draw => wake(),
        Then::Close => close(),
        Then::Send(text) => {
            crate::secretary::ask(text);
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

/// A key on the open drawer on `id`: what it does. Whether it was its.
pub fn key_on(
    id: window::Id,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
) -> bool {
    use keyboard::{Key, key::Named};
    enum Then {
        Nothing,
        Close,
        Send(String),
        Draw,
    }
    let then = with(|d| {
        let open = d.open_mut(id)?;
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
