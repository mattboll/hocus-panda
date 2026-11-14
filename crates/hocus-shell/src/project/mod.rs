//! Project desktops in the overview (docs/design/projects.md §2.1, §4,
//! board 1): the form that makes a desktop a project, and the card that
//! says what a project holds, each a layer surface of the shell's under
//! the desktop's thumbnail, which the compositor draws on the overview.
//!
//! - **The form** (`hocus-project-form`, 400 px, the popovers' glass):
//!   "Faire du bureau 2 un projet", the line saying its windows stay; the
//!   name, the shell's text field, filled with the chosen folder's last
//!   part and following it until the user types in it; the folders, the
//!   recent git repositories ([`repos`], found on a thread), the first
//!   chosen, then "Choisir un autre dossier…" (the desktop portal's file
//!   chooser on the session bus the compositor names, else a field for a
//!   path); "Annuler" and "Créer le projet". `Enter` creates, `Esc`
//!   cancels, `Tab` goes round. The compositor makes the project
//!   (`project-make`) or says why not, which shows under the field.
//!   Made: the form closes and a toast says so; the overview stays.
//! - **The card** (`hocus-project-card`, 600 px), open: the project's
//!   monogram, name, "ouvert · bureau 1", its folder, since when it is
//!   open; its desktop's sketch and windows, where each one is; that
//!   private windows are never read. `Esc` closes it.
//!
//! - **The confirmation** (`hocus-project-close`, 480 px, §5, board 4),
//!   centred on the project's screen over the compositor's dim: what
//!   will come back, what closing interrupts, "Annuler" and "Fermer le
//!   projet" (`Enter` closes, `Esc` cancels).
//! - **Closed** (§5): a toast says so and how it comes back. The card of
//!   a closed project opens centred, from the launcher (`→` on its row),
//!   with "Rouvrir".
//!
//! The compositor moves them in and out as the emoji picker, draws their
//! glass and shadow, gives them the keys over the overview, and closes
//! them with the overview (`project-end`). The projects it says
//! (`projects`) are kept here for the launcher's rows ([`rows`]).

pub mod card;
pub mod confirm;
pub mod form;
pub mod repos;

use hocus_words::shell::project as txt;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use hocus_link::{
    NameFault, POPOVER_CLOSE, PROJECT_CARD_NAMESPACE, PROJECT_CLOSE_NAMESPACE,
    PROJECT_FORM_NAMESPACE, ProjectCard, ProjectConfirm, ProjectForm, ProjectRefusal, ProjectRow,
    Rect, ToCompositor, WindowPlace,
};
use iced::{Element, keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;

use crate::{
    clock::time::Lang,
    field::{self, Entry},
    popover::{Effect, Pressed},
    surfaces::{Asked, Context, Lent, Surface},
};

/// The repositories shown at most (the mockup's four).
pub const REPOS_SHOWN: usize = 4;
/// The margin kept to the screen's edges, and how far under its
/// thumbnail a surface opens.
const EDGE: i32 = 8;
const UNDER: i32 = 48;

/// The form's and the card's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn form_title(self, number: usize) -> String {
        txt::form_title(self.lang, number)
    }

    pub fn form_line(self) -> &'static str {
        txt::FORM_LINE.get(self.lang)
    }

    pub fn name(self) -> &'static str {
        txt::NAME.get(self.lang)
    }

    pub fn folder_heading(self) -> &'static str {
        txt::FOLDER_HEADING.get(self.lang)
    }

    pub fn no_repository(self) -> &'static str {
        txt::NO_REPOSITORY.get(self.lang)
    }

    pub fn other_folder(self) -> &'static str {
        txt::OTHER_FOLDER.get(self.lang)
    }

    pub fn folder_path(self) -> &'static str {
        txt::FOLDER_PATH.get(self.lang)
    }

    pub fn choose_folder(self) -> &'static str {
        txt::CHOOSE_FOLDER.get(self.lang)
    }

    pub fn cancel(self) -> &'static str {
        txt::CANCEL.get(self.lang)
    }

    pub fn create(self) -> &'static str {
        txt::CREATE.get(self.lang)
    }

    pub fn terminals_there(self) -> &'static str {
        txt::TERMINALS_THERE.get(self.lang)
    }

    /// Why the project was not made.
    pub fn fault(self, fault: &Fault) -> String {
        match fault {
            Fault::Refused(ProjectRefusal::NameTaken(name)) => txt::name_taken(self.lang, name),
            Fault::Refused(ProjectRefusal::Name(NameFault::TooLong)) => {
                txt::NAME_TOO_LONG.get(self.lang).to_owned()
            }
            Fault::Refused(ProjectRefusal::Name(NameFault::Empty)) => {
                txt::NAME.get(self.lang).to_owned()
            }
            Fault::Refused(ProjectRefusal::NoFolder) | Fault::NoFolder => {
                txt::NO_FOLDER.get(self.lang).to_owned()
            }
            Fault::Refused(ProjectRefusal::Desktop) => txt::DESKTOP_TAKEN.get(self.lang).to_owned(),
        }
    }

    pub fn made(self, number: usize, name: &str) -> String {
        txt::made(
            self.lang,
            txt::Made {
                number: &number,
                name,
            },
        )
    }

    pub fn open_on(self, number: usize) -> String {
        txt::open_on_desktop(self.lang, number)
    }

    pub fn since(self, time: &str) -> String {
        txt::since(self.lang, time)
    }

    pub fn windows_heading(self, count: usize) -> String {
        txt::windows_heading(self.lang, count)
    }

    pub fn private(self) -> &'static str {
        txt::PRIVATE_NEVER_READ.get(self.lang)
    }

    pub fn place(self, place: WindowPlace) -> &'static str {
        let text = match place {
            WindowPlace::Floating => txt::PLACE_FLOATING,
            WindowPlace::Maximized => txt::PLACE_MAXIMIZED,
            WindowPlace::Fullscreen => txt::PLACE_FULLSCREEN,
            WindowPlace::Left => txt::PLACE_LEFT,
            WindowPlace::Right => txt::PLACE_RIGHT,
            WindowPlace::TopLeft => txt::PLACE_TOP_LEFT,
            WindowPlace::TopRight => txt::PLACE_TOP_RIGHT,
            WindowPlace::BottomLeft => txt::PLACE_BOTTOM_LEFT,
            WindowPlace::BottomRight => txt::PLACE_BOTTOM_RIGHT,
            WindowPlace::Minimized => txt::PLACE_MINIMIZED,
        };
        text.get(self.lang)
    }

    /// When a repository last changed: "il y a 2 h", "hier".
    pub fn when(self, when: std::time::SystemTime) -> String {
        use crate::notifications::history::Ago;
        use hocus_words::shell::popover_clipboard as ago;
        let at = when
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
        let now = jiff::Zoned::now();
        match crate::notifications::history::ago(at, &now) {
            Ago::Now => ago::NOW.get(self.lang).to_owned(),
            Ago::Minutes(n) => ago::min_ago(self.lang, n),
            Ago::Hours(n) => ago::h_ago(self.lang, n),
            Ago::Yesterday => ago::YESTERDAY.get(self.lang).to_owned(),
            Ago::Days(n) => ago::days_ago(self.lang, n),
        }
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// Why the form says the project is not made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    Refused(ProjectRefusal),
    /// No folder chosen, or none typed.
    NoFolder,
}

impl Fault {
    /// It is the name's (shown under the name), else the folder's.
    pub fn of_name(&self) -> bool {
        matches!(
            self,
            Self::Refused(ProjectRefusal::NameTaken(_) | ProjectRefusal::Name(_))
        )
    }
}

/// The folder chosen in the form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Folder {
    /// The repository at that place in the list.
    Repo(usize),
    /// The one the file chooser gave.
    Chosen(PathBuf),
    /// The one typed in the path's field.
    Typed,
}

/// "Choisir un autre dossier…": a button, the chooser asked, or the field
/// for a path (no portal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Other {
    Button,
    Asking,
    Field,
}

/// What holds the keyboard in the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Name,
    Repo(usize),
    Other,
    Path,
    Cancel,
    Create,
}

/// The form open.
#[derive(Debug)]
pub struct Form {
    pub surface: window::Id,
    pub ask: ProjectForm,
    pub place: (i32, i32),
    pub size: (u32, u32),
    pub name: Entry,
    /// The user typed in the name: it no longer follows the folder.
    pub named: bool,
    /// The repositories found; none until they come.
    pub repos: Option<Vec<repos::Repo>>,
    pub folder: Folder,
    pub other: Other,
    pub path: Entry,
    pub focus: Focus,
    pub fault: Option<Fault>,
    /// The compositor was asked: until it answers, nothing more is.
    pub waiting: bool,
    /// Which form the repositories and the chooser answer.
    pub serial: u64,
    /// What it last said it shows, in the log.
    pub logged: String,
}

impl Form {
    /// The repositories shown.
    pub fn shown(&self) -> &[repos::Repo] {
        self.repos
            .as_deref()
            .map_or(&[][..], |r| &r[..r.len().min(REPOS_SHOWN)])
    }

    /// The folder chosen, as it stands.
    pub fn folder(&self) -> Option<PathBuf> {
        match &self.folder {
            Folder::Repo(i) => self.shown().get(*i).map(|r| r.path.clone()),
            Folder::Chosen(path) => Some(path.clone()),
            Folder::Typed => {
                let typed = self.path.text().trim();
                (!typed.is_empty()).then(|| expand(typed))
            }
        }
    }

    /// The name follows the folder until the user types in it.
    fn follow(&mut self) {
        if self.named {
            return;
        }
        let name = self
            .folder()
            .and_then(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        let name: String = name.chars().take(hocus_link::NAME_MOST).collect();
        self.name.set(&name);
    }

    /// The places the keyboard goes round, in order.
    fn ring(&self) -> Vec<Focus> {
        let mut ring = vec![Focus::Name];
        ring.extend((0..self.shown().len()).map(Focus::Repo));
        ring.push(if self.other == Other::Field {
            Focus::Path
        } else {
            Focus::Other
        });
        ring.extend([Focus::Cancel, Focus::Create]);
        ring
    }
}

/// A folder typed, `~` its home.
fn expand(typed: &str) -> PathBuf {
    match typed.strip_prefix('~') {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(format!("{home}{rest}"))
        }
        _ => PathBuf::from(typed),
    }
}

/// The card open.
#[derive(Debug)]
pub struct Card {
    pub surface: window::Id,
    pub shown: ProjectCard,
    pub logged: String,
    /// Its tabs all shown ("3 de plus"), not four a window.
    pub more: bool,
}

/// The confirmation open, and what holds the keyboard there.
#[derive(Debug)]
pub struct Confirm {
    pub surface: window::Id,
    pub shown: ProjectConfirm,
    pub focus: confirm::Focus,
    pub logged: String,
}

/// The projects as the compositor last said them.
static ROWS: std::sync::Mutex<Vec<ProjectRow>> = std::sync::Mutex::new(Vec::new());

/// The projects, as the launcher lists them.
pub fn rows() -> Vec<ProjectRow> {
    ROWS.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// The compositor said the projects: kept, and an open launcher shows them.
pub fn set_rows(rows: Vec<ProjectRow>) {
    tracing::debug!(projects = rows.len(), "the projects");
    *ROWS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = rows;
    crate::launcher::changed();
}

/// An application's name, by its desktop entry (else its app id), as the
/// launcher lists it.
pub fn app_name(entry: &str, app: &str) -> String {
    let catalog = crate::launcher::apps::catalog();
    let found = |id: &str| {
        catalog
            .apps
            .iter()
            .find(|a| !id.is_empty() && a.entry.id.eq_ignore_ascii_case(id))
            .map(|a| a.entry.name.clone())
    };
    found(entry).or_else(|| found(app)).unwrap_or_else(|| {
        let id = if entry.is_empty() { app } else { entry };
        id.rsplit('.').next().unwrap_or(id).to_owned()
    })
}

/// The form and the card, and their surfaces fading out: the shell's own
/// (#773).
#[derive(Debug, Default)]
pub struct Projects {
    pub form: Option<Box<Form>>,
    pub card: Option<Box<Card>>,
    pub confirm: Option<Box<Confirm>>,
    closing: Vec<(window::Id, Instant)>,
    asked: Option<Instant>,
    serial: u64,
}

/// Whether the form or the card holds the keyboard, for the threads that
/// do not own them (a key that reached another surface).
static HOLDING: AtomicBool = AtomicBool::new(false);

/// What other threads and surfaces ask of the form and the card, as
/// messages for the loop that owns them.
#[derive(Debug, Clone)]
pub enum Come {
    Form(ProjectForm),
    Card(ProjectCard),
    /// "Fermer le projet…": the confirmation.
    Confirm(ProjectConfirm),
    /// A project closed: the toast.
    Closed {
        output: String,
        name: String,
        kept: u32,
        stayed: Vec<String>,
    },
    Made {
        output: String,
        desktop: u8,
        name: String,
    },
    Refused(ProjectRefusal),
    /// The overview closed, a press beside them, another surface: they
    /// close.
    Close,
    /// The repositories found for form `serial`.
    Repos(u64, Vec<repos::Repo>),
    /// The file chooser answered form `serial`: a folder, nothing, or no
    /// portal.
    Chosen(u64, crate::capture::portal::Chosen),
    /// A key that reached another of the shell's surfaces.
    Key(Pressed),
    /// The clipboard's text for the field holding the keyboard.
    Pasted(String),
    Tick,
}

/// Where a surface `size` large opens under `at` (its thumbnail) on a
/// screen `screen` large: its margins from the screen's top left corner,
/// centred under the thumbnail, 48 px under it, on the screen.
pub fn place(at: Option<Rect>, size: (u32, u32), screen: (i32, i32)) -> (i32, i32) {
    let Some(at) = at else {
        return centred(size, screen);
    };
    let (w, h) = (size.0 as i32, size.1 as i32);
    let x = at.x + at.width / 2 - w / 2;
    let y = at.y + at.height + UNDER;
    let x = x.min(screen.0 - EDGE - w).max(EDGE);
    let y = y.min(screen.1 - EDGE - h).max(EDGE);
    (x, y)
}

/// Where a surface `size` large opens centred on a screen `screen` large.
pub fn centred(size: (u32, u32), screen: (i32, i32)) -> (i32, i32) {
    let (w, h) = (size.0 as i32, size.1 as i32);
    (
        ((screen.0 - w) / 2).max(EDGE),
        ((screen.1 - h) / 2).max(EDGE),
    )
}

/// A press outside, another surface: they close, if they are open; asked
/// of the loop, from any thread.
pub fn close() {
    if HOLDING.load(Ordering::Relaxed) {
        crate::surfaces::ask(Asked::Project(Come::Close));
    }
}

/// A key that reached another of the shell's surfaces while the form or
/// the card holds the keyboard: theirs, handed to the loop. Whether it was
/// taken.
pub fn key(key: &keyboard::Key, modifiers: keyboard::Modifiers, text: Option<&str>) -> bool {
    if !HOLDING.load(Ordering::Relaxed) {
        return false;
    }
    crate::surfaces::ask(Asked::Project(Come::Key(Pressed {
        key: key.clone(),
        modifiers,
        text: text.map(str::to_owned),
    })));
    true
}

impl Projects {
    pub fn form_on(&self, id: window::Id) -> Option<&Form> {
        self.form.as_deref().filter(|f| f.surface == id)
    }

    pub fn form_mut(&mut self, id: window::Id) -> Option<&mut Form> {
        self.form.as_deref_mut().filter(|f| f.surface == id)
    }

    pub fn card_on(&self, id: window::Id) -> Option<&Card> {
        self.card.as_deref().filter(|c| c.surface == id)
    }

    pub fn confirm_on(&self, id: window::Id) -> Option<&Confirm> {
        self.confirm.as_deref().filter(|c| c.surface == id)
    }

    /// Whether surface `id` is one of theirs, open or fading out.
    pub fn is_surface(&self, id: window::Id) -> bool {
        self.form_on(id).is_some()
            || self.card_on(id).is_some()
            || self.confirm_on(id).is_some()
            || self.closing.iter().any(|(c, _)| *c == id)
    }

    /// When a tick is due, if one was not asked for by then already.
    pub fn due(&mut self) -> Option<Instant> {
        let at = self.closing.iter().map(|(_, at)| *at).min()?;
        if self.asked.is_some_and(|asked| asked <= at) {
            return None;
        }
        self.asked = Some(at);
        Some(at)
    }

    /// `come` taken in at `now`: what the shell then does.
    pub fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        match come {
            Come::Form(ask) => self.open_form(ask, now, &mut effects),
            Come::Card(shown) => self.open_card(shown, now, &mut effects),
            Come::Confirm(shown) => self.open_confirm(shown, now, &mut effects),
            Come::Closed {
                output,
                name,
                kept,
                stayed,
            } => {
                let (said, then) =
                    confirm::closed_said(Words::new(Lang::of_messages()), &name, kept, &stayed);
                effects.extend(crate::launcher::toast::show_then(
                    output,
                    said,
                    Some(then),
                    now,
                ));
            }
            Come::Made {
                output,
                desktop,
                name,
            } => {
                // The form's, or the launcher's folder made a project.
                tracing::info!(desktop = desktop + 1, "the project is made");
                if self.form.as_ref().is_some_and(|f| f.waiting) {
                    self.close(now, &mut effects);
                }
                let said = words().made(usize::from(desktop) + 1, &name);
                effects.extend(crate::launcher::toast::show(output, said, now));
            }
            Come::Refused(why) => {
                if let Some(form) = &mut self.form {
                    tracing::info!(?why, "the project is not made");
                    form.waiting = false;
                    form.fault = Some(Fault::Refused(why));
                    effects.extend(form::refit(form));
                }
            }
            Come::Close => self.close(now, &mut effects),
            Come::Repos(serial, found) => {
                if let Some(form) = self.form.as_mut().filter(|f| f.serial == serial) {
                    tracing::debug!(repos = found.len(), "the form's repositories");
                    form.repos = Some(found);
                    if form.folder == Folder::Repo(0) || form.shown().is_empty() {
                        form.folder = Folder::Repo(0);
                    }
                    form.follow();
                    effects.extend(form::refit(form));
                }
            }
            Come::Chosen(serial, chosen) => {
                if let Some(form) = self.form.as_mut().filter(|f| f.serial == serial) {
                    use crate::capture::portal::Chosen;
                    match chosen {
                        Chosen::Path(path) => {
                            form.other = Other::Button;
                            form.folder = Folder::Chosen(path);
                            form.fault = None;
                            form.follow();
                        }
                        Chosen::Cancelled => form.other = Other::Button,
                        Chosen::NoPortal => {
                            form.other = Other::Field;
                            form.folder = Folder::Typed;
                            form.focus = Focus::Path;
                            form.follow();
                        }
                    }
                    effects.extend(form::refit(form));
                    effects.extend(woken());
                }
            }
            Come::Key(pressed) => {
                if self.confirm.is_some() {
                    effects.extend(confirm::key(self, &pressed, now));
                } else if let Some(id) = self.form.as_ref().map(|f| f.surface) {
                    effects.extend(form::key(self, id, &pressed, now));
                } else if self.card.is_some() {
                    effects.extend(card::key(self, &pressed, now));
                }
                effects.extend(woken());
            }
            Come::Pasted(pasted) => {
                if let Some(form) = &mut self.form {
                    let entry = if form.focus == Focus::Path {
                        &mut form.path
                    } else {
                        &mut form.name
                    };
                    if entry.paste(&pasted) {
                        form::edited(form);
                        effects.extend(woken());
                    }
                }
            }
            Come::Tick => {
                self.asked = None;
                self.closing.retain(|(id, at)| {
                    let due = *at <= now;
                    if due {
                        effects.push(Effect::Remove(*id));
                    }
                    !due
                });
            }
        }
        self.said();
        effects
    }

    /// Whether they hold the keyboard, for other threads.
    fn said(&self) {
        HOLDING.store(
            self.form.is_some() || self.card.is_some() || self.confirm.is_some(),
            Ordering::Relaxed,
        );
    }

    /// They close at `now`: the keyboard goes back at once, the compositor
    /// fades them out, and they go after that.
    pub fn close(&mut self, now: Instant, effects: &mut Vec<Effect>) {
        if let Some(form) = self.form.take() {
            tracing::debug!("the project's form closes");
            self.closing.push((form.surface, now + POPOVER_CLOSE));
            effects.push(Effect::Keyboard(form.surface, KeyboardInteractivity::None));
            effects.push(Effect::Tell(ToCompositor::Closing(
                PROJECT_FORM_NAMESPACE.to_owned(),
            )));
        }
        if let Some(card) = self.card.take() {
            tracing::debug!("the project's card closes");
            self.closing.push((card.surface, now + POPOVER_CLOSE));
            effects.push(Effect::Keyboard(card.surface, KeyboardInteractivity::None));
            effects.push(Effect::Tell(ToCompositor::Closing(
                PROJECT_CARD_NAMESPACE.to_owned(),
            )));
        }
        if let Some(confirm) = self.confirm.take() {
            tracing::debug!("the project's confirmation closes");
            self.closing.push((confirm.surface, now + POPOVER_CLOSE));
            effects.push(Effect::Keyboard(
                confirm.surface,
                KeyboardInteractivity::None,
            ));
            effects.push(Effect::Tell(ToCompositor::Closing(
                PROJECT_CLOSE_NAMESPACE.to_owned(),
            )));
        }
    }

    fn open_confirm(&mut self, shown: ProjectConfirm, now: Instant, effects: &mut Vec<Effect>) {
        self.close(now, effects);
        let surface = window::Id::unique();
        let size = confirm::size(&shown, words());
        let place = centred(size, shown.screen);
        tracing::debug!(name = shown.name, ?place, "a project's confirmation opens");
        effects.push(Effect::Project {
            id: surface,
            output: shown.output.clone(),
            size,
            margin: (place.1, 0, 0, place.0),
            namespace: PROJECT_CLOSE_NAMESPACE,
        });
        self.confirm = Some(Box::new(Confirm {
            surface,
            shown,
            focus: confirm::Focus::Close,
            logged: String::new(),
        }));
        others_close();
    }

    fn open_form(&mut self, ask: ProjectForm, now: Instant, effects: &mut Vec<Effect>) {
        self.close(now, effects);
        self.serial += 1;
        let serial = self.serial;
        let surface = window::Id::unique();
        let mut form = Form {
            surface,
            place: (0, 0),
            size: (hocus_link::PROJECT_FORM_WIDTH, 0),
            name: Entry::new(field::Kind::Line, hocus_link::NAME_MOST),
            named: false,
            repos: None,
            folder: Folder::Repo(0),
            other: Other::Button,
            path: Entry::new(field::Kind::Line, 4096),
            focus: Focus::Name,
            fault: None,
            waiting: false,
            serial,
            logged: String::new(),
            ask,
        };
        form.size = form::size(&form, words());
        form.place = place(Some(form.ask.at), form.size, form.ask.screen);
        tracing::debug!(
            desktop = form.ask.desktop + 1,
            place = ?form.place,
            "the project's form opens"
        );
        effects.push(Effect::Project {
            id: surface,
            output: form.ask.output.clone(),
            size: form.size,
            margin: (form.place.1, 0, 0, form.place.0),
            namespace: PROJECT_FORM_NAMESPACE,
        });
        look_for_repos(serial, &form.ask);
        self.form = Some(Box::new(form));
        others_close();
    }

    fn open_card(&mut self, shown: ProjectCard, now: Instant, effects: &mut Vec<Effect>) {
        self.close(now, effects);
        let surface = window::Id::unique();
        let size = card::size(&shown, words());
        let place = place(shown.at, size, shown.screen);
        tracing::debug!(name = shown.name, ?place, "a project's card opens");
        effects.push(Effect::Project {
            id: surface,
            output: shown.output.clone(),
            size,
            margin: (place.1, 0, 0, place.0),
            namespace: PROJECT_CARD_NAMESPACE,
        });
        self.card = Some(Box::new(Card {
            surface,
            shown,
            logged: String::new(),
            more: false,
        }));
        others_close();
    }
}

/// One at a time: the bar's popover, the launcher, the palette and the
/// emoji picker close.
fn others_close() {
    crate::popover::event(crate::popover::Event::PressElsewhere);
    crate::launcher::close();
    crate::actions::close();
    crate::emoji::close();
}

/// Their surfaces drawn again, their views built again.
pub fn woken() -> Vec<Effect> {
    crate::ui::touch();
    vec![
        Effect::Wake(PROJECT_FORM_NAMESPACE),
        Effect::Wake(PROJECT_CARD_NAMESPACE),
    ]
}

/// The repositories for form `serial`, found on a thread and handed to the
/// loop.
fn look_for_repos(serial: u64, ask: &ProjectForm) {
    let terminals = ask.terminals.clone();
    let projects: Vec<PathBuf> = ask.folders.iter().map(PathBuf::from).collect();
    let started = std::thread::Builder::new()
        .name("project repositories".to_owned())
        .spawn(move || {
            let found = repos::Places::from_env()
                .map(|places| repos::find(&places, &terminals, &projects))
                .unwrap_or_default();
            crate::surfaces::ask(Asked::Project(Come::Repos(serial, found)));
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread to look for the repositories");
    }
}

/// "Choisir un autre dossier…": the file chooser asked on a thread, its
/// answer handed to the loop.
fn choose_folder(serial: u64) {
    let title = words().choose_folder().to_owned();
    let started = std::thread::Builder::new()
        .name("project folder".to_owned())
        .spawn(move || {
            let chosen = crate::capture::portal::choose_folder(&title, PROJECT_FORM_NAMESPACE);
            crate::surfaces::ask(Asked::Project(Come::Chosen(serial, chosen)));
        });
    if let Err(err) = started {
        tracing::warn!(%err, "no thread for the file chooser");
        crate::surfaces::ask(Asked::Project(Come::Chosen(
            serial,
            crate::capture::portal::Chosen::NoPortal,
        )));
    }
}

/// The clipboard's text, on one line, for the field holding the keyboard,
/// read off the loop and handed to it.
fn paste() {
    crate::paste::request(crate::paste::Clean::Spaced, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        crate::surfaces::ask(Asked::Project(Come::Pasted(pasted.to_owned())));
    });
}

impl Surface for Lent<Projects> {
    type Message = Come;

    fn update(&mut self, come: Come, now: Instant) -> Vec<Effect> {
        self.get_mut().update(come, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        let (ours, card, confirm) = self.read(|p| {
            (
                p.is_surface(id),
                p.card_on(id).is_some(),
                p.confirm_on(id).is_some(),
            )
        })?;
        if !ours {
            return None;
        }
        // One fading out shows what it showed: the compositor fades its
        // last frame.
        Some(if confirm {
            confirm::view(self, id, words(), context.look.glass)
        } else if card {
            card::view(self, id, words(), context.look.glass)
        } else {
            form::view(self, id, words(), context.look.glass)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_opens_under_its_thumbnail_on_the_screen() {
        let thumb = Rect {
            x: 508,
            y: 48,
            width: 160,
            height: 100,
        };
        // The mockup's: 400 px, its left at 388, its top 48 px under.
        assert_eq!(place(Some(thumb), (400, 380), (1440, 900)), (388, 196));
        // Near the right edge: pushed in.
        let right = Rect { x: 1300, ..thumb };
        assert_eq!(
            place(Some(right), (400, 380), (1440, 900)).0,
            1440 - 8 - 400
        );
        assert_eq!(
            place(Some(Rect { x: 0, ..thumb }), (600, 300), (1440, 900)).0,
            8
        );
        // No thumbnail: centred, as the confirmation.
        assert_eq!(place(None, (480, 400), (1440, 900)), (480, 250));
    }

    #[test]
    fn a_typed_folder_reads_its_home() {
        let home = std::env::var("HOME").unwrap_or_default();
        assert_eq!(expand("~/code/x"), PathBuf::from(format!("{home}/code/x")));
        assert_eq!(expand("/srv/x"), PathBuf::from("/srv/x"));
        assert_eq!(expand("~x"), PathBuf::from("~x"));
    }
}
