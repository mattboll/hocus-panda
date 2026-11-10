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
//! Her text is selected and copied as in any chat ([`select`], #757,
//! #761): a drag (past the thread's top or bottom, it scrolls), a double
//! or triple click, `Ctrl+C`, the right click's « Copier »; a click
//! elsewhere or `Esc` clears it. No button copies.
//!
//! Its field and her key's are the shell's text field ([`crate::field`],
//! #763), edited as everywhere: Enter sends and Shift+Enter breaks the
//! line; her key's is masked and never copied out. One selection at a
//! time: a press in a field lets her text's go, and the other way round.
//!
//! What she says reaches it from the shell's line to her
//! ([`crate::secretary`]), off iced's loop: the drawer is woken only while
//! it is open. Closed, nothing of hers draws.

pub mod draw;
pub mod select;

use std::{
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use hocus_link::{
    Clip, ClipKind, DRAWER_CLOSE, DRAWER_WIDE, EDGE, SECRETARY_NAMESPACE, ToCompositor,
};
use hocus_secretary_link::{
    Body, Decision, Mood, Notebook,
    knows::{Act, Do, Kind, Query},
};
use iced::{Point, Size, keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use crate::{
    clock::time::Lang,
    field::{self, Field, Outcome},
    popover::Effect,
};

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

    /// The field's placeholder, to her by her name (#758).
    pub fn placeholder(self, name: &str) -> String {
        if self.fr {
            format!("Écris à {name}…")
        } else {
            format!("Write to {name}…")
        }
    }

    /// Under the field: nothing that does not help to write (#758).
    pub fn keys(self) -> &'static str {
        self.pick(
            "Entrée envoie · Maj+Entrée : à la ligne",
            "Enter sends · Shift+Enter: new line",
        )
    }

    /// Her header's button to her queue (#758).
    pub fn to_sort(self, n: u64) -> String {
        match (self.fr, n) {
            (true, 1) => "1 mail à trier ›".to_owned(),
            (true, n) => format!("{n} mails à trier ›"),
            (false, 1) => "1 mail to sort ›".to_owned(),
            (false, n) => format!("{n} mails to sort ›"),
        }
    }

    /// What can be asked of her, in an empty drawer (#758).
    pub fn examples(self) -> [&'static str; 3] {
        if self.fr {
            [
                "Qu’est-ce que j’ai aujourd’hui ?",
                "Qui attend une réponse de moi ?",
                "Rappelle-moi d’appeler Paul à 16 h",
            ]
        } else {
            [
                "What do I have today?",
                "Who is waiting for an answer from me?",
                "Remind me to call Paul at 4 pm",
            ]
        }
    }

    /// Under a draft waiting for its click (#758).
    pub fn nothing_leaves(self) -> &'static str {
        self.pick(
            "Rien ne part sans ce clic.",
            "Nothing leaves without this click.",
        )
    }

    /// A question's label (#758): blue, a question, not an alert.
    pub fn question(self) -> &'static str {
        self.pick("QUESTION", "QUESTION")
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

    /// A sub-view's way back (#758).
    pub fn back(self) -> &'static str {
        self.pick("‹ Retour à la discussion", "‹ Back to the conversation")
    }

    pub fn open_point(self) -> &'static str {
        self.pick("Voir le point", "See the briefing")
    }

    /// A briefing's mail to answer (#758): it opens, never marked read.
    pub fn open_mail(self) -> &'static str {
        self.pick("Ouvrir le mail ↗", "Open the mail ↗")
    }

    pub fn remind_me(self) -> &'static str {
        self.pick("Rappelle-le-moi", "Remind me")
    }

    /// The queue view's title, and the briefing's.
    pub fn queue_title(self) -> &'static str {
        self.pick("Le tri de tes mails", "Sorting your mail")
    }

    /// Her key's band (#758): what a key is, where to make one, what it
    /// costs; its field, its note.
    pub fn key_title(self, name: &str) -> String {
        if self.fr {
            format!("Pour te répondre, il me faut une clé {name}")
        } else {
            format!("To answer you, I need an {name} key")
        }
    }

    pub fn key_explain(self) -> &'static str {
        self.pick(
            "Je réfléchis avec Claude, d’Anthropic. Crée une clé d’API dans la console d’Anthropic, puis colle-la ici. Elle est facturée à l’usage, à part d’un abonnement Claude.",
            "I think with Claude, by Anthropic. Create an API key in Anthropic’s console, then paste it here. It is billed by use, apart from a Claude subscription.",
        )
    }

    pub fn key_console(self) -> &'static str {
        self.pick(
            "Ouvrir la console d’Anthropic ↗",
            "Open Anthropic’s console ↗",
        )
    }

    pub fn key_placeholder(self) -> &'static str {
        self.pick(
            "Colle ta clé ici (elle commence par sk-ant-)",
            "Paste your key here (it starts with sk-ant-)",
        )
    }

    pub fn key_note(self) -> &'static str {
        self.pick(
            "Je la garde dans le trousseau de ta session, avec tes mots de passe ; jamais dans un fichier.",
            "I keep it in your session’s keyring, with your passwords; never in a file.",
        )
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
            "Tout ce qu’elle sait de toi et de ton travail, et d’où elle le tient",
            "All she knows of you and your work, and where she has it from",
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
            ["Travail et privé", "Travail", "Privé"]
        } else {
            ["Work and private", "Work", "Private"]
        }
    }

    /// An act's button, for an item (`seen:`, `edit:` or another).
    pub fn act(self, act: Act, item: &str) -> &'static str {
        let seen = item.starts_with("seen:");
        let merge = item.starts_with("edit:");
        match act {
            Act::Edit => self.pick("Modifier", "Edit"),
            Act::Close => self.pick("Ce n’est plus vrai", "No longer true"),
            Act::Forget => self.pick("Oublier", "Forget"),
            Act::Keep => self.pick("Garder", "Keep"),
            Act::Accept if seen => self.pick("Oui", "Yes"),
            Act::Accept => self.pick("C’est juste", "That’s right"),
            Act::Reject if seen => self.pick("Non", "No"),
            Act::Reject if merge => self.pick("Défaire", "Undo"),
            Act::Reject => self.pick("Faux", "Wrong"),
            Act::Later => self.pick("Plus tard", "Later"),
        }
    }

    pub fn memories(self, n: u64) -> String {
        match (self.fr, n) {
            (true, 1) => "1 chose, chiffrée sur cet ordinateur".to_owned(),
            (true, n) => format!("{n} choses, chiffrées sur cet ordinateur"),
            (false, 1) => "1 thing, encrypted on this computer".to_owned(),
            (false, n) => format!("{n} things, encrypted on this computer"),
        }
    }

    /// "Effacer… ▾", apart from "Exporter" (#758).
    pub fn erase(self) -> &'static str {
        self.pick("Effacer… ▾", "Erase… ▾")
    }

    /// "···"'s "Voir d'où je le sais".
    pub fn whence(self) -> &'static str {
        self.pick("Voir d’où je le sais", "See where I have it from")
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

    /// Her text's « Copier » (#757), in the right click's menu.
    pub fn copy(self) -> &'static str {
        self.pick("Copier", "Copy")
    }

    /// The toast once something is copied.
    pub fn copied(self) -> &'static str {
        self.pick("Copié", "Copied")
    }

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
    /// One of the empty drawer's examples (#758): it is said to her.
    Example(u8),
    /// An account's card's switch `kind` (#758).
    Switch {
        item: u64,
        kind: hocus_secretary_link::consent::SourceKind,
    },
    /// An account's card's « Travail | Privé » (#758).
    Book {
        item: u64,
        perso: bool,
    },
    /// An account's card's « Ce que je lis exactement… ›», and its
    /// calendars' « choisir › » (#758): they unfold.
    Details(u64),
    Calendars(u64),
    /// Her key's band's « Ouvrir la console d'Anthropic ↗ » (#758).
    Console,
    /// A briefing's mail to answer, line `index` of the briefing shown:
    /// « Ouvrir le mail ↗ », « Rappelle-le-moi » (#758).
    OpenMail(u32),
    Remind(u32),
    /// "Ce qu'elle sait": item `index`'s « ··· », its menu (#758).
    KnownMenu(u32),
    /// "Effacer… ▾": its menu (#758).
    Erase,
    /// The right click's menu's « Copier ».
    CopyMenu,
}

impl Target {
    /// Whether it is a line of text that a click acts on ("Ce qu'elle
    /// sait"'s item unfolds): its words are selected by a drag or a
    /// double click as the rest (#761), a click alone acts.
    pub fn holds_text(self) -> bool {
        matches!(self, Self::Expand(_))
    }

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
            Self::Example(n) => format!("Example({n})"),
            Self::Switch { item, kind } => format!("Switch({item},{})", kind.as_str()),
            Self::Book { item, perso } => format!("Book({item},{perso})"),
            Self::Details(item) => format!("Details({item})"),
            Self::Calendars(item) => format!("Calendars({item})"),
            Self::Console => "Console".to_owned(),
            Self::OpenMail(n) => format!("OpenMail({n})"),
            Self::Remind(n) => format!("Remind({n})"),
            Self::KnownMenu(n) => format!("KnownMenu({n})"),
            Self::Erase => "Erase".to_owned(),
            Self::CopyMenu => "CopyMenu".to_owned(),
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
    /// The field (#763), edited as everywhere.
    pub field: Field,
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
    /// Her key as it is pasted or typed in her band (masked, never
    /// copied, never logged; overwritten once sent), and whether the
    /// keyboard types there rather than in the field (#752).
    pub key: Field,
    pub key_focus: bool,
    /// Where an input method opens for the field the keyboard types in,
    /// and what it takes, as last laid out (#763).
    pub ime: Option<(iced::Rectangle, iced::advanced::input_method::Purpose)>,
    /// Her text selected (#757): gone as the drawer closes.
    pub selection: Option<select::Selection>,
    /// A drag selecting held past the thread's top or bottom (#761):
    /// where the pointer is, and when the thread last scrolled for it.
    pub edge: Option<(Point, Instant)>,
    /// The last press on text: when, where, and how many in a row (a
    /// double click takes a word, a triple its paragraph).
    pub clicks: Option<(Instant, Point, u8)>,
    /// The right click's menu: where, and what its « Copier » copies.
    pub menu: Option<Menu>,
    /// Each account's card's kinds switched off (#758), its details and
    /// its calendars unfolded.
    pub off: std::collections::HashMap<
        u64,
        std::collections::BTreeSet<hocus_secretary_link::consent::SourceKind>,
    >,
    pub unfolded_cards: std::collections::HashSet<(u64, bool)>,
    /// "Ce qu'elle sait": the item whose « ··· » menu shows, and
    /// « Effacer… ▾ »'s (#758).
    pub known_menu: Option<u32>,
    pub erase: bool,
    /// When each « Noté » was first shown (#758): its bar empties over
    /// its ten seconds from then.
    pub noted: std::collections::HashMap<u64, Instant>,
    /// Until when « Copié » shows.
    pub copied: Option<Instant>,
}

/// The right click's menu over her text.
#[derive(Clone)]
pub struct Menu {
    pub at: Point,
    /// Never logged: it may be anything she said.
    pub text: String,
}

impl std::fmt::Debug for Menu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Menu({:?}, {} chars)",
            self.at,
            self.text.chars().count()
        )
    }
}

/// How often a « Noté »'s bar steps down.
const NOTED_STEP: Duration = Duration::from_millis(250);

/// How long « Copié » shows.
pub const COPIED: Duration = Duration::from_millis(1500);
/// Presses this close in time and place count as one double or triple
/// click.
pub const MULTI_CLICK: Duration = Duration::from_millis(500);

impl Drop for Open {
    fn drop(&mut self) {
        self.key.wipe();
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
    field: Field,
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
            field: Field::new(field::Kind::Lines, FIELD_MOST),
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
            key: Field::new(field::Kind::Secret, KEY_MOST),
            ime: None,
            // Her key missing: the keyboard types in its field first.
            key_focus: crate::secretary::conversation(|c| {
                c.look.as_ref().is_some_and(|l| l.key.is_some())
            }),
            selection: None,
            edge: None,
            clicks: None,
            menu: None,
            copied: None,
            off: std::collections::HashMap::new(),
            unfolded_cards: std::collections::HashSet::new(),
            known_menu: None,
            erase: false,
            noted: std::collections::HashMap::new(),
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
    with(|d| {
        let copied = match &d.phase {
            Phase::Open(open) => open.copied,
            Phase::Closed => None,
        };
        // A « Noté »'s bar steps down four times a second while it can
        // be taken back (#758): no redraw at each frame.
        let noted = match &d.phase {
            Phase::Open(open) if open.noted.values().any(|t| t.elapsed() < draw::NOTED) => {
                Some(Instant::now() + NOTED_STEP)
            }
            _ => None,
        };
        d.closing
            .iter()
            .map(|(_, at)| *at)
            .chain(copied)
            .chain(noted)
            .min()
    })
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
        let noting = matches!(&d.phase,
            Phase::Open(open) if open.noted.values().any(|t| now.duration_since(*t) < draw::NOTED + NOTED_STEP));
        let copied_over = match &mut d.phase {
            Phase::Open(open) if open.copied.is_some_and(|until| until <= now) => {
                open.copied = None;
                true
            }
            _ => false,
        };
        if (toast_over || copied_over || noting) && matches!(d.phase, Phase::Open(_)) {
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

/// What the right click's « Copier » copies of thread item `item` with
/// nothing selected: her answer's text as it shows ([`select::unmarked`]),
/// a draft card's draft (its lines, else its text); nothing for the rest.
pub fn item_copy(item: &hocus_secretary_link::Item) -> Option<String> {
    match &item.body {
        Body::Her { text, .. } if !text.is_empty() => Some(select::unmarked(text).into_owned()),
        Body::Card { card }
            if card.kind == hocus_secretary_link::CardKind::Draft && card.sheet.is_none() =>
        {
            let draft = if card.lines.is_empty() {
                card.text.clone()
            } else {
                card.lines.join("\n")
            };
            (!draft.is_empty()).then_some(draft)
        }
        _ => None,
    }
}

/// `text` copied from the drawer on `id` (#757): the clipboard takes it,
/// as the compositor offers the shell's copies (never read back into the
/// history), and « Copié » shows. Neither the text nor its words are
/// logged.
pub fn copy(id: window::Id, text: String) {
    if text.is_empty() {
        return;
    }
    let open = with(|d| match d.open_mut(id) {
        Some(open) => {
            open.copied = Some(Instant::now() + COPIED);
            open.menu = None;
            true
        }
        None => false,
    });
    if !open {
        return;
    }
    tracing::info!(chars = text.chars().count(), "the drawer copies");
    crate::popover::send(vec![Effect::Tell(ToCompositor::Clip {
        paste: false,
        clip: Clip {
            kind: ClipKind::Text,
            data: text.into_bytes(),
        },
    })]);
    wake();
}

/// Where an Anthropic key is made.
const CONSOLE: &str = "https://console.anthropic.com/settings/keys";

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
        Copy(String),
    }
    let then = with(|d| {
        let Some(open) = d.open_mut(id) else {
            return Then::Nothing;
        };
        // A menu of "Ce qu'elle sait" goes at any click but its own.
        if !matches!(target, Target::KnownMenu(_)) {
            open.known_menu = None;
        }
        if !matches!(target, Target::Erase) {
            open.erase = false;
        }
        match target {
            Target::Example(n) => {
                let said = words().examples().get(n as usize).map(|s| (*s).to_owned());
                match said {
                    Some(text) => {
                        open.scroll = 0.0;
                        open.changed();
                        Then::Send(text)
                    }
                    None => Then::Nothing,
                }
            }
            Target::Switch { item, kind } => {
                let off = open.off.entry(item).or_default();
                if !off.remove(&kind) {
                    off.insert(kind);
                }
                tracing::info!(
                    item,
                    kind = kind.as_str(),
                    off = off.len(),
                    "an account's switch"
                );
                open.changed();
                Then::Draw
            }
            Target::Book { item, perso } => {
                let notebook = if perso {
                    Notebook::Perso
                } else {
                    Notebook::Pro
                };
                tracing::info!(
                    item,
                    notebook = notebook.word(),
                    "an account's « Travail | Privé »"
                );
                open.notebooks.insert(item, notebook);
                open.changed();
                Then::Draw
            }
            Target::Details(item) | Target::Calendars(item) => {
                let key = (item, matches!(target, Target::Calendars(_)));
                if !open.unfolded_cards.remove(&key) {
                    open.unfolded_cards.insert(key);
                }
                open.changed();
                Then::Draw
            }
            Target::Console => match crate::launcher::browse(CONSOLE) {
                Some(effect) => {
                    tracing::info!("her key's console opens in the browser");
                    Then::Effects(vec![effect])
                }
                None => Then::Nothing,
            },
            Target::OpenMail(index) | Target::Remind(index) => {
                let line = open.point.and_then(|point| {
                    crate::secretary::conversation(|c| {
                        c.items
                            .iter()
                            .find(|i| i.id == point)
                            .and_then(|i| match &i.body {
                                Body::Briefing { briefing } => briefing
                                    .sections
                                    .iter()
                                    .flat_map(|s| s.lines.iter())
                                    .filter(|l| !l.pointer.is_empty())
                                    .nth(index as usize)
                                    .map(|l| (point, l.pointer.clone(), l.open.clone())),
                                _ => None,
                            })
                    })
                });
                match (target, line) {
                    (Target::OpenMail(_), Some((_, _, Some(url)))) => {
                        // Opened in the browser: never marked read here.
                        match crate::launcher::browse(&url) {
                            Some(effect) => Then::Effects(vec![effect]),
                            None => Then::Nothing,
                        }
                    }
                    (Target::Remind(_), Some((item, pointer, _))) => {
                        crate::secretary::remind_me(item, pointer);
                        Then::Nothing
                    }
                    _ => Then::Nothing,
                }
            }
            Target::KnownMenu(index) => {
                open.known_menu = if open.known_menu == Some(index) {
                    None
                } else {
                    Some(index)
                };
                open.changed();
                Then::Draw
            }
            Target::Erase => {
                open.erase = !open.erase;
                open.changed();
                Then::Draw
            }
            Target::CopyMenu => match open.menu.take() {
                Some(menu) => Then::Copy(menu.text),
                None => Then::Draw,
            },
            Target::Close => Then::Close,
            Target::Gear => {
                crate::popover::session::open_settings(Some("secretary"));
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
                // An account's card: the kinds switched off, those its
                // sheet has off by default included.
                let off = crate::secretary::conversation(|c| {
                    c.items.iter().find_map(|i| match &i.body {
                        Body::Card { card } if i.id == item => card.sheet.as_ref().map(|s| {
                            s.reads
                                .iter()
                                .filter(|r| {
                                    let flipped =
                                        open.off.get(&item).is_some_and(|o| o.contains(&r.kind));
                                    r.on == flipped
                                })
                                .map(|r| r.kind)
                                .collect::<Vec<_>>()
                        }),
                        _ => None,
                    })
                })
                .unwrap_or_default();
                crate::secretary::decide(item, decision, notebook, left_out, off);
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
                    open.field.set(&said);
                    open.key_focus = false;
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
                    // No « Tout » (#758): the chip chosen again lets go.
                    open.kind = if open.kind == kind { None } else { kind };
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
                    open.field.set(&words().about(&title));
                    open.key_focus = false;
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
                // Never alongside a toast that undoes (#758): it goes,
                // out of the drawer's lock.
                open.confirm = Some(Confirm::Perso);
                open.changed();
                Then::Undo(None)
            }
            Target::ForgetAll => {
                open.confirm = Some(Confirm::All);
                open.changed();
                Then::Undo(None)
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
        Then::Copy(text) => copy(id, text),
        Then::Undo(token) => {
            if let Some(token) = token {
                crate::secretary::memory(Do::Restore { token });
            }
            crate::secretary::toast_gone();
            wake();
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
    let key = open.key.text().trim().to_owned();
    open.key.wipe();
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

/// The drawer's fields (#763).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// The composer, to her.
    Composer,
    /// Her key's, in her band.
    Key,
}

impl Open {
    /// Field `entry`.
    pub fn entry(&mut self, entry: Entry) -> &mut Field {
        match entry {
            Entry::Composer => &mut self.field,
            Entry::Key => &mut self.key,
        }
    }

    /// The field the keyboard types in: her key's while her band shows
    /// and has it, else the composer; none on "Ce qu'elle sait".
    pub fn typing(&self) -> Option<Entry> {
        match self.tab {
            Tab::Knows => None,
            Tab::Talk if self.key_focus && key_band().is_some() => Some(Entry::Key),
            Tab::Talk => Some(Entry::Composer),
        }
    }
}

/// The drawer's fields as last laid out: the lines their carets move
/// along.
#[derive(Debug, Clone, Copy, Default)]
pub struct Laids<'a> {
    pub composer: Option<&'a field::view::Laid>,
    pub key: Option<&'a field::view::Laid>,
}

/// What `act` does to field `entry`, along its lines as laid out (by its
/// line breaks if it is not laid out yet).
fn edit(open: &mut Open, entry: Entry, act: field::Act, laid: Option<&field::view::Laid>) -> Then {
    let outcome = match laid {
        Some(laid) => open.entry(entry).apply(act, laid),
        None => {
            let mut text = open.entry(entry).text().to_owned();
            let outcome = open.entry(entry).apply(act, &field::Paragraphs(&text));
            field::wipe(&mut text);
            outcome
        }
    };
    match outcome {
        Outcome::Same => Then::Nothing,
        Outcome::Moved | Outcome::Edited => {
            open.changed();
            Then::Draw
        }
        Outcome::Copy(text) => Then::Clip(text),
        Outcome::Cut(text) => {
            open.changed();
            Then::Clip(text)
        }
        Outcome::Paste => Then::Paste(entry),
    }
}

/// A key in her key's field: `Enter` sends it, `Tab` goes to the
/// composer, `Esc` closes; the rest edits it as any field, but nothing
/// copies it out.
fn key_field_key(
    open: &mut Open,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
    laid: Option<&field::view::Laid>,
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
        _ => match field::keys::act(key, modifiers, text) {
            Some(act) => edit(open, Entry::Key, act, laid),
            None => Then::Nothing,
        },
    }
}

/// The longest key her field takes, in characters.
const KEY_MOST: usize = 512;

/// The field's sentence sent: it empties, and `↑` brings it back.
fn sentence(open: &mut Open) -> Option<String> {
    let text = open.field.text().trim().to_owned();
    if text.is_empty() {
        return None;
    }
    open.field.take();
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
    /// A field's text copied or cut out.
    Clip(String),
    /// The clipboard's text into this field.
    Paste(Entry),
}

/// `text` copied or cut from a field of the drawer on its surface: the
/// clipboard takes it, as the compositor offers the shell's copies.
/// Neither the text nor its words are logged.
fn clip(text: String) {
    tracing::info!(chars = text.chars().count(), "the drawer's field copies");
    crate::popover::send(vec![Effect::Tell(ToCompositor::Clip {
        paste: false,
        clip: Clip {
            kind: ClipKind::Text,
            data: text.into_bytes(),
        },
    })]);
}

/// The clipboard's text read off the loop, then into field `entry` of
/// the drawer on `id` at its caret: her key's on one line, trimmed; the
/// composer's with its line breaks.
fn paste_into(id: window::Id, entry: Entry) {
    let clean = match entry {
        Entry::Composer => crate::paste::Clean::Lines,
        Entry::Key => crate::paste::Clean::Dropped,
    };
    let asked = crate::paste::request(clean, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        let took = with(|d| match d.open_mut(id) {
            Some(open) => {
                let pasted = match entry {
                    Entry::Key => pasted.trim(),
                    Entry::Composer => pasted,
                };
                let took = open.entry(entry).paste(pasted) == Outcome::Edited;
                if took {
                    open.changed();
                }
                took
            }
            None => false,
        });
        if took {
            tracing::debug!(
                ?entry,
                "the clipboard's text pasted into the drawer's field"
            );
            wake();
        }
    });
    if !asked {
        tracing::debug!(?entry, "nothing to paste into the drawer's field");
    }
}

/// Where an input method opens on the drawer on `id`, and what its field
/// takes, while a field there has the keyboard (#763).
pub fn ime(id: window::Id) -> Option<(iced::Rectangle, iced::advanced::input_method::Purpose)> {
    with(|d| d.open_on(id).and_then(|open| open.ime))
}

/// An input method's preedit or commit for the field the keyboard types
/// in on the drawer on `id` (#763): whether it changed.
pub fn input_method(id: window::Id, event: &iced::advanced::input_method::Event) -> bool {
    use iced::advanced::input_method::Event as Ime;
    with(|d| {
        let Some(open) = d.open_mut(id) else {
            return false;
        };
        let Some(entry) = open.typing() else {
            return false;
        };
        let field = open.entry(entry);
        let outcome = match event {
            Ime::Opened => Outcome::Same,
            Ime::Preedit(text, cursor) => field.compose(text, cursor.clone()),
            Ime::Commit(text) => {
                field.compose("", None);
                field.apply(field::Act::Insert(text.clone()), &field::Paragraphs(""))
            }
            Ime::Closed => field.compose("", None),
        };
        if outcome == Outcome::Same {
            return false;
        }
        tracing::debug!(
            ?entry,
            commit = matches!(event, Ime::Commit(_)),
            "an input method in the drawer's field"
        );
        open.changed();
        true
    })
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
        // Another list: what was selected in it goes.
        open.selection = None;
        open.changed();
        crate::secretary::knows(Some(open.query()));
    }
    Then::Draw
}

/// A key on the open drawer on `id`, its fields laid out as `laids`:
/// what it does. Whether it was its.
pub fn key_on(
    id: window::Id,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
    laids: Laids<'_>,
) -> bool {
    use keyboard::{Key, key::Named};
    let then = with(|d| {
        let open = d.open_mut(id)?;
        // The right click's menu goes first, then what is selected (#761).
        if matches!(key, Key::Named(Named::Escape)) {
            if open.menu.take().is_some() {
                return Some(Then::Draw);
            }
            if open.selection.take().is_some_and(|s| !s.is_empty()) {
                open.edge = None;
                tracing::debug!(chars = 0, "the drawer's selection");
                return Some(Then::Draw);
            }
        }
        if open.tab == Tab::Knows {
            return Some(search_key(open, key, modifiers, text));
        }
        if open.typing() == Some(Entry::Key) {
            return Some(key_field_key(open, key, modifiers, text, laids.key));
        }
        let plain = !modifiers.control() && !modifiers.alt() && !modifiers.logo();
        Some(match key {
            Key::Named(Named::Escape) => Then::Close,
            Key::Named(Named::Enter) if modifiers.shift() => edit(
                open,
                Entry::Composer,
                field::Act::Insert("\n".to_owned()),
                laids.composer,
            ),
            Key::Named(Named::Enter) => match sentence(open) {
                Some(text) => Then::Send(text),
                None => Then::Nothing,
            },
            // The last sentence back, in an empty field.
            Key::Named(Named::ArrowUp) if plain && !modifiers.shift() && open.field.is_empty() => {
                match open.recall.clone() {
                    Some(last) => {
                        open.field.set(&last);
                        open.changed();
                        Then::Draw
                    }
                    None => Then::Nothing,
                }
            }
            _ => match field::keys::act(key, modifiers, text) {
                Some(act) => edit(open, Entry::Composer, act, laids.composer),
                None => Then::Nothing,
            },
        })
    });
    match then {
        None => false,
        Some(Then::Nothing | Then::Draw) => true,
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
        Some(Then::Clip(text)) => {
            clip(text);
            true
        }
        Some(Then::Paste(entry)) => {
            paste_into(id, entry);
            true
        }
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
