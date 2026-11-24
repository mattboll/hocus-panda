//! The system's authentication dialog (7.5, variant A,
//! docs/design/polkit.md): when an action of the system needs an
//! administrator's password, polkit asks the session's agent, the shell
//! ([`agent`]), which shows who asks, why, for which account, the
//! password's field and the details, folded.
//!
//! One dialog at a time: a request that comes while one shows waits,
//! counted in the footer, and opens [`AUTH_NEXT`] after it closed. The
//! shell asks the compositor where it goes (`auth open`, the asking
//! process and its parents), and opens its surface ([`AUTH_NAMESPACE`])
//! there once told (`auth-at`); the compositor dims every screen, moves it
//! in and out and keeps everything else from the pointer and the keys.
//!
//! The password goes to polkit's helper ([`helper`]), which a conversation
//! starts as the dialog opens, again after each wrong password and for
//! another account. Granted, the dialog closes; wrong, the field shakes
//! and empties; after three wrong passwords in a row, the request is
//! refused and the dialog closes. Cancelled (`Esc`, Annuler), or polkit
//! cancelling, the same. The dialog's state is here; its surface is the
//! widget kit's ([`view`]): its controls declared there, its keys, its
//! ring and its screen readers' rows the kit's; the helper changes it and
//! draws its surface alone.
//!
//! NetworkManager's secrets (#930) are asked in the same dialog, on the
//! same surface and in the same queue: the shell is its secret agent too
//! ([`nm`]), and a request's form is [`form`]'s, drawn by [`form_view`].

mod accounts;
mod agent;
mod form;
pub mod form_view;
mod helper;
mod nm;
pub mod view;

use hocus_words::shell::auth as txt;
use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{AUTH_NAMESPACE, AUTH_NEXT, AUTH_OUT, AUTH_TOP, AuthAsk, AuthAt, ToCompositor};
use iced::{Element, window};
use iced_exwlshell::reexport::KeyboardInteractivity;
use tokio::sync::oneshot;

pub use accounts::Account;
use helper::{Conversation, Said};

use crate::{
    clock::time::Lang,
    field::{self, Entry, Took},
    network::secret::Secret,
    popover::{Effect, Pressed},
    surfaces::{Asked, Context, Lent, Surface},
};

/// Wrong passwords in a row before the request is refused.
pub const TRIES: u32 = 3;
/// The characters the field keeps at most.
const LONGEST: usize = 256;

pub fn start() {
    agent::start();
    nm::start();
}

/// Requests' numbers, polkit's and NetworkManager's: one queue, one
/// surface.
static REQUESTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn next_id() -> u64 {
    REQUESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// How a request ended, for polkit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Granted,
    Cancelled,
    Refused,
}

/// A request from polkit, as the dialog shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub cookie: String,
    /// polkit's action id, its message (translated by the action's policy)
    /// and its icon's name.
    pub action: String,
    pub message: String,
    pub icon: String,
    /// The accounts that can answer, and the one chosen.
    pub accounts: Vec<Account>,
    pub chosen: usize,
    /// The asking process, and it then its parents.
    pub pid: Option<u32>,
    pub pids: Vec<u32>,
    /// The program polkit names (the action's), else the process's.
    pub program: String,
    /// The name of whoever started the process.
    pub started_by: Option<String>,
}

impl Request {
    pub fn account(&self) -> Option<&Account> {
        self.accounts.get(self.chosen)
    }
}

/// What can hold the keyboard or be clicked, in the keyboard's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    Change,
    Account(usize),
    Field,
    Details,
    Cancel,
    Authorize,
    /// NetworkManager's form: a secret's field, "Retenir le mot de passe",
    /// Se connecter.
    Entry(usize),
    Remember,
    Connect,
}

/// Whether the password is being typed, or checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Typing,
    Checking,
}

/// The dialog shown.
#[derive(Debug)]
pub struct Dialog {
    pub request: Request,
    /// Its surface, once the compositor said where it goes.
    pub surface: Option<window::Id>,
    pub at: Option<AuthAt>,
    pub phase: Phase,
    /// The password's field: the shell's text field (#771) as a secret,
    /// masked, never copied out.
    pub field: Entry,
    /// Wrong passwords in a row, and whether the line says the last was.
    pub wrong: u32,
    pub wrong_note: bool,
    /// What PAM said last (and whether it was an error).
    pub said: Option<(String, bool)>,
    pub details: bool,
    pub chooser: bool,
    pub focus: Target,
    /// Since when the field shakes, and Details' chevron turns.
    pub shake: Option<Instant>,
    pub turned: Option<Instant>,
    pub size: (u32, u32),
    conversation: Option<Conversation>,
    /// The conversation's number: what an ended one says is not heard.
    generation: u64,
    /// Prompts the helper made in this conversation.
    prompts: u32,
}

impl Dialog {
    fn new(request: Request) -> Self {
        Self {
            request,
            surface: None,
            at: None,
            phase: Phase::Typing,
            field: Entry::new(field::Kind::Secret, LONGEST),
            wrong: 0,
            wrong_note: false,
            said: None,
            details: false,
            chooser: false,
            focus: Target::Field,
            shake: None,
            turned: None,
            size: (0, 0),
            conversation: None,
            generation: 0,
            prompts: 0,
        }
    }

    /// Whether more than one account can answer.
    pub fn several(&self) -> bool {
        self.request.accounts.len() > 1
    }

    /// A new conversation with the helper, for the account chosen.
    fn converse(&mut self) {
        if let Some(old) = self.conversation.take() {
            old.end();
        }
        self.generation += 1;
        self.prompts = 0;
        let Some(account) = self.request.account() else {
            return;
        };
        let (id, generation) = (self.request.id, self.generation);
        self.conversation = Some(Conversation::start(
            account.login.clone(),
            self.request.cookie.clone(),
            move |said| {
                crate::surfaces::ask(Asked::Auth(Come::Heard {
                    id,
                    generation,
                    said,
                }));
            },
        ));
    }

    fn end_conversation(&mut self) {
        if let Some(old) = self.conversation.take() {
            old.end();
        }
    }
}

/// The answer for a request (polkit's, NetworkManager's): handed to the
/// dialog with it, sent once as it ends. Uncontended: each request has its
/// own.
pub struct Answer<T = Outcome>(Arc<Mutex<Option<oneshot::Sender<T>>>>);

impl<T> Answer<T> {
    fn new(answer: oneshot::Sender<T>) -> Self {
        Self(Arc::new(Mutex::new(Some(answer))))
    }

    fn send(&self, outcome: T) {
        let answer = self.0.lock().unwrap_or_else(PoisonError::into_inner).take();
        if let Some(answer) = answer {
            let _ = answer.send(outcome);
        }
    }
}

impl<T> Clone for Answer<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::fmt::Debug for Answer<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Answer")
    }
}

/// What other threads tell the dialog, as messages for the loop that owns
/// it ([`crate::surfaces::ask`]).
#[derive(Debug, Clone)]
pub enum Come {
    /// A request from polkit (on the agent's thread): it shows, or waits.
    Begin(Box<Request>, Answer),
    /// polkit cancels the request of this cookie (its application gave
    /// up).
    Cancel(String),
    /// NetworkManager asks for a connection's secrets (on its agent's
    /// thread): its form shows, or waits.
    Secrets(Box<form::Ask>),
    /// NetworkManager withdraws its request for this connection's setting.
    Withdraw { path: String, setting: String },
    /// What the helper of request `id`'s conversation `generation` said.
    Heard {
        id: u64,
        generation: u64,
        said: Said,
    },
    /// The clipboard's text for the field of the dialog on surface `id`,
    /// never in a `String` of ours.
    Pasted(window::Id, Arc<Secret>),
    /// A screen reader acts on a control.
    Act(Target),
    /// What was due is due ([`Auth::due`]).
    Tick,
}

/// What changes the dialog.
#[derive(Debug, Clone)]
pub enum Told {
    /// From another thread.
    Come(Come),
    /// `auth-at`: the dialog goes there.
    Placed(AuthAt),
    /// `caps`: Caps Lock's state.
    Caps(bool),
}

/// A request waiting its turn: polkit's, or NetworkManager's.
#[derive(Debug)]
enum Waiting {
    Polkit(Request),
    Secrets(form::Ask),
}

impl Waiting {
    fn id(&self) -> u64 {
        match self {
            Self::Polkit(r) => r.id,
            Self::Secrets(a) => a.id,
        }
    }
}

/// The dialog shown: polkit's, or a form for NetworkManager.
#[derive(Debug)]
pub enum Shown {
    Polkit(Box<Dialog>),
    Secrets(Box<form::Form>),
}

impl Shown {
    fn id(&self) -> u64 {
        match self {
            Self::Polkit(d) => d.request.id,
            Self::Secrets(f) => f.ask.id,
        }
    }

    fn surface(&self) -> Option<window::Id> {
        match self {
            Self::Polkit(d) => d.surface,
            Self::Secrets(f) => f.surface,
        }
    }

    fn polkit(&self) -> Option<&Dialog> {
        match self {
            Self::Polkit(d) => Some(d),
            Self::Secrets(_) => None,
        }
    }

    fn form(&self) -> Option<&form::Form> {
        match self {
            Self::Secrets(f) => Some(f),
            Self::Polkit(_) => None,
        }
    }

    /// Its size, as what it shows now lays it out.
    fn fit(&self, waiting: usize, caps: bool) -> (u32, u32) {
        match self {
            Self::Polkit(d) => view::Layout::of(d, words(), waiting, caps).surface(),
            Self::Secrets(f) => form_view::Layout::of(f, form_view::words()).surface(),
        }
    }
}

/// The dialog and the requests waiting, the shell's own (#773).
#[derive(Debug, Default)]
pub struct Auth {
    waiting: VecDeque<Waiting>,
    shown: Option<Shown>,
    /// Surfaces fading out, when they go, and the dialog they show as it
    /// closed.
    closing: Vec<(window::Id, Instant, Shown)>,
    /// When the next request opens.
    next: Option<Instant>,
    caps: bool,
    answers: BTreeMap<u64, Answer>,
    /// The time a tick was asked for.
    asked: Option<Instant>,
    /// What its view last said in the log.
    said: Option<Vec<String>>,
}

/// What screen readers are told of the dialog ([`a11y`]).
#[derive(Debug, Clone)]
pub struct Seen {
    surface: window::Id,
    name: String,
    message: String,
    size: (u32, u32),
}

/// Its view is built again and draws (an icon came). Nothing draws when
/// no dialog shows.
pub fn wake() {
    crate::ui::touch();
    crate::popover::send(vec![Effect::Wake(AUTH_NAMESPACE)]);
}

/// The dialog's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    fn pick(self, text: hocus_words::Text) -> &'static str {
        text.get(self.lang)
    }

    pub fn title(self) -> &'static str {
        self.pick(txt::AUTHORIZATION_REQUIRED)
    }

    /// The dialog's name for screen readers.
    pub fn name(self, app: &str) -> String {
        txt::authorization_required_2(self.lang, app)
    }

    pub fn administrator(self) -> &'static str {
        self.pick(txt::ADMINISTRATOR)
    }

    pub fn change(self) -> &'static str {
        self.pick(txt::CHANGE)
    }

    pub fn password(self) -> &'static str {
        self.pick(txt::PASSWORD)
    }

    /// The field's name for screen readers.
    pub fn password_of(self, name: &str) -> String {
        if self.lang == Lang::French {
            let elided = name
                .chars()
                .next()
                .is_some_and(|c| "AEIOUYÉÈÊaeiouyéèêhH".contains(c));
            if elided {
                format!("Mot de passe d'{name}")
            } else {
                format!("Mot de passe de {name}")
            }
        } else {
            format!("{name}'s password")
        }
    }

    pub fn caps(self) -> &'static str {
        self.pick(txt::CAPS_LOCK_IS_ON)
    }

    pub fn checking(self) -> &'static str {
        self.pick(txt::CHECKING)
    }

    /// A wrong password, Caps Lock on or not.
    pub fn wrong(self, caps: bool) -> String {
        let wrong = self.pick(txt::WRONG_PASSWORD_TRY_AGAIN);
        if caps {
            format!("{wrong} {}", self.pick(txt::CAPS_LOCK_IS_ON_2))
        } else {
            wrong.to_owned()
        }
    }

    pub fn details(self) -> &'static str {
        self.pick(txt::DETAILS)
    }

    pub fn action(self) -> &'static str {
        "Action"
    }

    pub fn program(self) -> &'static str {
        self.pick(txt::PROGRAM)
    }

    pub fn process(self) -> &'static str {
        self.pick(txt::PROCESS)
    }

    pub fn window(self) -> &'static str {
        self.pick(txt::WINDOW)
    }

    /// The process's line: its id, and who started it.
    pub fn started(self, pid: Option<u32>, by: Option<&str>) -> String {
        let pid = pid.map_or_else(|| "?".to_owned(), |p| p.to_string());
        match by {
            Some(by) => txt::started_by(self.lang, txt::StartedBy { by: &by, pid: &pid }),
            None => pid,
        }
    }

    pub fn none(self) -> &'static str {
        self.pick(txt::NONE)
    }

    /// The requests waiting behind it.
    pub fn waiting(self, n: usize) -> String {
        match n {
            0 => String::new(),
            1 => txt::N_1_MORE_REQUEST_WAITING.get(self.lang).to_owned(),
            n => txt::more_requests_waiting(self.lang, n),
        }
    }

    pub fn cancel(self) -> &'static str {
        self.pick(txt::CANCEL)
    }

    pub fn authorize(self) -> &'static str {
        self.pick(txt::AUTHORIZE)
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// A request from polkit (on the agent's thread): to the loop.
fn begin(request: Request, answer: oneshot::Sender<Outcome>) {
    crate::surfaces::ask(Asked::Auth(Come::Begin(
        Box::new(request),
        Answer::new(answer),
    )));
}

/// polkit cancels the request of `cookie`: to the loop.
fn cancel_cookie(cookie: &str) {
    crate::surfaces::ask(Asked::Auth(Come::Cancel(cookie.to_owned())));
}

/// The launcher, the emoji picker and the bar's popover close: the dialog
/// alone holds the keyboard.
fn one_at_a_time() {
    crate::popover::event(crate::popover::Event::PressElsewhere);
    crate::launcher::close();
    crate::emoji::close();
    crate::actions::close();
}

/// The surface drawn again, its view built again.
fn woken() -> Vec<Effect> {
    crate::ui::touch();
    vec![Effect::Wake(AUTH_NAMESPACE)]
}

impl Auth {
    /// `told` taken in at `now`: what the shell then does.
    pub fn update(&mut self, told: Told, now: Instant) -> Vec<Effect> {
        let effects = match told {
            Told::Come(Come::Begin(request, answer)) => {
                self.answers.insert(request.id, answer);
                self.begin(Waiting::Polkit(*request), now)
            }
            Told::Come(Come::Secrets(ask)) => self.begin(Waiting::Secrets(*ask), now),
            Told::Come(Come::Cancel(cookie)) => self.cancel_cookie(&cookie, now),
            Told::Come(Come::Withdraw { path, setting }) => self.withdraw(&path, &setting, now),
            Told::Come(Come::Heard {
                id,
                generation,
                said,
            }) => self.heard(id, generation, said, now),
            Told::Come(Come::Pasted(id, pasted)) => {
                let edited = self.edited(id, |entry| match entry.paste(pasted.as_str()) {
                    true => Took::Edited,
                    false => Took::Same,
                });
                if edited { woken() } else { Vec::new() }
            }
            Told::Come(Come::Act(target)) => match self.shown_surface() {
                Some(id) => {
                    let mut effects = self.act_on(id, target, now);
                    effects.extend(woken());
                    effects
                }
                None => Vec::new(),
            },
            Told::Come(Come::Tick) => {
                self.asked = None;
                self.tick(now)
            }
            Told::Placed(at) => self.place(at),
            Told::Caps(on) => {
                let changed = std::mem::replace(&mut self.caps, on) != on;
                if changed && self.shown_surface().is_some() {
                    woken()
                } else {
                    Vec::new()
                }
            }
        };
        crate::a11y::shown(crate::a11y::Shown::Auth(self.seen()));
        effects
    }

    /// When a tick is due, if one was not asked for by then already: a
    /// surface faded out, the next request, a form's request out of time.
    pub fn due(&mut self) -> Option<Instant> {
        let deadlines = self
            .waiting
            .iter()
            .filter_map(|w| match w {
                Waiting::Secrets(ask) => Some(ask.deadline()),
                Waiting::Polkit(_) => None,
            })
            .chain(
                self.shown
                    .as_ref()
                    .and_then(Shown::form)
                    .map(|f| f.ask.deadline()),
            );
        let at = self
            .closing
            .iter()
            .map(|(_, at, _)| *at)
            .chain(self.next)
            .chain(deadlines)
            .min()?;
        if self.asked.is_some_and(|asked| asked <= at) {
            return None;
        }
        self.asked = Some(at);
        Some(at)
    }

    fn shown_surface(&self) -> Option<window::Id> {
        self.shown.as_ref().and_then(Shown::surface)
    }

    /// polkit's dialog shown, if its surface is `id`.
    fn dialog_on(&mut self, id: window::Id) -> Option<&mut Dialog> {
        match self.shown.as_mut() {
            Some(Shown::Polkit(d)) if d.surface == Some(id) => Some(d),
            _ => None,
        }
    }

    /// NetworkManager's form shown, if its surface is `id`.
    fn form_on(&mut self, id: window::Id) -> Option<&mut form::Form> {
        match self.shown.as_mut() {
            Some(Shown::Secrets(f)) if f.surface == Some(id) => Some(f),
            _ => None,
        }
    }

    /// What surface `id` shows, open or fading out as it closed.
    fn shown_on(&self, id: window::Id) -> Option<&Shown> {
        self.shown
            .as_ref()
            .filter(|s| s.surface() == Some(id))
            .or_else(|| {
                self.closing
                    .iter()
                    .find(|(c, _, _)| *c == id)
                    .map(|(_, _, s)| s)
            })
    }

    /// `with` on polkit's dialog surface `id` shows, open or fading out as
    /// it closed, with Caps Lock's state and the requests waiting.
    pub fn drawn<T>(
        &self,
        id: window::Id,
        with: impl FnOnce(&Dialog, bool, usize) -> T,
    ) -> Option<T> {
        self.shown_on(id)
            .and_then(Shown::polkit)
            .map(|d| with(d, self.caps, self.waiting.len()))
    }

    /// `with` on NetworkManager's form surface `id` shows, open or fading
    /// out, with Caps Lock's state.
    pub fn drawn_form<T>(
        &self,
        id: window::Id,
        with: impl FnOnce(&form::Form, bool) -> T,
    ) -> Option<T> {
        self.shown_on(id)
            .and_then(Shown::form)
            .map(|f| with(f, self.caps))
    }

    /// Whether surface `id` is the dialog's, shown or fading out.
    pub fn is_surface(&self, id: window::Id) -> bool {
        self.shown_on(id).is_some()
    }

    /// Whether surface `id` shows a form for NetworkManager.
    pub fn is_form(&self, id: window::Id) -> bool {
        self.shown_on(id).and_then(Shown::form).is_some()
    }

    /// Whether surface `id` fades out (or is gone): it takes nothing then.
    pub fn closing(&self, id: window::Id) -> bool {
        self.shown.as_ref().is_none_or(|s| s.surface() != Some(id))
    }

    /// The entry of polkit's dialog shown (or fading out), for the field's
    /// widget.
    pub fn entry(&mut self, with: &mut dyn FnMut(&mut Entry)) {
        let dialog = match self.shown.as_mut() {
            Some(Shown::Polkit(dialog)) => Some(dialog),
            _ => self.closing.last_mut().and_then(|(_, _, s)| match s {
                Shown::Polkit(d) => Some(d),
                Shown::Secrets(_) => None,
            }),
        };
        if let Some(dialog) = dialog {
            with(&mut dialog.field);
        }
    }

    /// Field `i` of the form on surface `id` (shown or fading out), for its
    /// widget.
    pub fn entry_at(&mut self, id: window::Id, i: usize, with: &mut dyn FnMut(&mut Entry)) {
        let form = match self.shown.as_mut() {
            Some(Shown::Secrets(f)) if f.surface == Some(id) => Some(f),
            _ => self
                .closing
                .iter_mut()
                .find(|(c, _, _)| *c == id)
                .and_then(|(_, _, s)| match s {
                    Shown::Secrets(f) => Some(f),
                    Shown::Polkit(_) => None,
                }),
        };
        if let Some(entry) = form.and_then(|f| f.entries.get_mut(i)) {
            with(entry);
        }
    }

    fn seen(&self) -> Option<Seen> {
        match self.shown.as_ref()? {
            Shown::Polkit(dialog) => Some(Seen {
                surface: dialog.surface?,
                name: words().name(&view::who(dialog).0),
                message: dialog.request.message.trim().to_owned(),
                size: dialog.size,
            }),
            Shown::Secrets(form) => Some(Seen {
                surface: form.surface?,
                name: form.ask.asked.name.clone(),
                message: form_view::words().asks(&form.ask.asked).to_owned(),
                size: form.size,
            }),
        }
    }

    /// A request comes: it shows, or waits.
    fn begin(&mut self, waiting: Waiting, now: Instant) -> Vec<Effect> {
        self.waiting.push_back(waiting);
        let mut effects = self.pump(now);
        if !effects.is_empty() {
            one_at_a_time();
        } else if matches!(self.shown, Some(Shown::Polkit(_))) && self.shown_surface().is_some() {
            // The footer counts it.
            effects.extend(woken());
        }
        effects
    }

    /// polkit cancels the request of `cookie`: its dialog closes at once.
    fn cancel_cookie(&mut self, cookie: &str, now: Instant) -> Vec<Effect> {
        let at = self
            .waiting
            .iter()
            .position(|w| matches!(w, Waiting::Polkit(r) if r.cookie == cookie));
        if let Some(at) = at {
            if let Some(waiting) = self.waiting.remove(at) {
                self.answer(waiting.id(), Outcome::Cancelled);
            }
            return woken();
        }
        if matches!(&self.shown, Some(Shown::Polkit(d)) if d.request.cookie == cookie) {
            tracing::debug!("polkit cancels the dialog's request");
            return self.finish(Outcome::Cancelled, now);
        }
        Vec::new()
    }

    /// NetworkManager withdraws its request for `path`'s `setting`.
    fn withdraw(&mut self, path: &str, setting: &str, now: Instant) -> Vec<Effect> {
        let of = |ask: &form::Ask| ask.path == path && ask.asked.setting == setting;
        let mut effects = Vec::new();
        self.waiting.retain(|w| match w {
            Waiting::Secrets(ask) if of(ask) => {
                ask.answer.send(form::Given::Withdrawn);
                false
            }
            _ => true,
        });
        if matches!(&self.shown, Some(Shown::Secrets(f)) if of(&f.ask)) {
            tracing::debug!("NetworkManager withdraws the form's request");
            effects = self.finish_form(form::Given::Withdrawn, now);
        } else if self.shown_surface().is_some() {
            effects = woken();
        }
        effects
    }

    fn answer(&mut self, id: u64, outcome: Outcome) {
        if let Some(answer) = self.answers.remove(&id) {
            answer.send(outcome);
        }
    }

    /// The next request shows if none does and its time came.
    fn pump(&mut self, now: Instant) -> Vec<Effect> {
        if self.shown.is_some() || self.next.is_some_and(|at| at > now) {
            return Vec::new();
        }
        self.next = None;
        let Some(waiting) = self.waiting.pop_front() else {
            return Vec::new();
        };
        let (id, pids, shown) = match waiting {
            Waiting::Polkit(request) => (
                request.id,
                request.pids.clone(),
                Shown::Polkit(Box::new(Dialog::new(request))),
            ),
            Waiting::Secrets(ask) => (
                ask.id,
                Vec::new(),
                Shown::Secrets(Box::new(form::Form::new(ask))),
            ),
        };
        tracing::debug!(id, "the authentication dialog is asked for");
        self.shown = Some(shown);
        if crate::popover::linked_now() {
            vec![Effect::Tell(ToCompositor::Auth(AuthAsk::Open { id, pids }))]
        } else {
            // A shell alone: on the active screen.
            self.place(AuthAt {
                id,
                output: crate::ALONE.to_owned(),
                scale: 120,
                height: 1080,
                app: String::new(),
                title: String::new(),
            })
        }
    }

    /// The compositor said where the dialog goes: it opens there.
    fn place(&mut self, at: AuthAt) -> Vec<Effect> {
        let (waiting, caps) = (self.waiting.len(), self.caps);
        let Some(shown) = self.shown.as_mut() else {
            return Vec::new();
        };
        if shown.id() != at.id || shown.surface().is_some() {
            return Vec::new();
        }
        let id = window::Id::unique();
        let top = (f64::from(at.height) / 2.0 - AUTH_TOP).max(8.0).round() as i32;
        let output = at.output.clone();
        match shown {
            Shown::Polkit(dialog) => {
                dialog.surface = Some(id);
                dialog.at = Some(at);
            }
            Shown::Secrets(form) => {
                form.surface = Some(id);
                form.at = Some(at);
            }
        }
        let size = shown.fit(waiting, caps);
        match shown {
            Shown::Polkit(dialog) => {
                dialog.size = size;
                let app = view::who(dialog).0;
                if let Some(icon) = view::who(dialog).1 {
                    let scale = dialog.at.as_ref().map_or(1.0, |a| a.scale as f32 / 120.0);
                    crate::launcher::icons::want([icon.as_str()], view::ICON_PX, scale);
                }
                tracing::debug!(
                    id = dialog.request.id,
                    output,
                    app,
                    accounts = dialog.request.accounts.len(),
                    "the authentication dialog opens"
                );
                dialog.converse();
            }
            Shown::Secrets(form) => {
                form.size = size;
                tracing::debug!(
                    id = form.ask.id,
                    output,
                    setting = form.ask.asked.setting,
                    fields = form.entries.len(),
                    "the network's dialog opens"
                );
            }
        }
        vec![Effect::Auth {
            id,
            output,
            size,
            top,
        }]
    }

    /// polkit's dialog closes with `outcome`.
    fn finish(&mut self, outcome: Outcome, now: Instant) -> Vec<Effect> {
        let Some(Shown::Polkit(mut dialog)) = self.shown.take() else {
            return Vec::new();
        };
        dialog.end_conversation();
        self.answer(dialog.request.id, outcome);
        tracing::debug!(
            id = dialog.request.id,
            ?outcome,
            "the authentication dialog closes"
        );
        self.closed(Shown::Polkit(dialog), now)
    }

    /// NetworkManager's form closes, answered with `given`.
    fn finish_form(&mut self, given: form::Given, now: Instant) -> Vec<Effect> {
        let Some(Shown::Secrets(mut form)) = self.shown.take() else {
            return Vec::new();
        };
        tracing::debug!(
            id = form.ask.id,
            given = match &given {
                form::Given::Secrets { .. } => "secrets",
                form::Given::Cancelled => "cancelled",
                form::Given::Withdrawn => "withdrawn",
            },
            "the network's dialog closes"
        );
        form.ask.answer.send(given);
        // What was typed is not kept while the surface fades out.
        for entry in &mut form.entries {
            entry.clear();
        }
        self.closed(Shown::Secrets(form), now)
    }

    /// The dialog `shown` closed: the compositor lets the input go, its
    /// surface fades out, the next request opens after a while.
    fn closed(&mut self, shown: Shown, now: Instant) -> Vec<Effect> {
        let mut effects = vec![Effect::Tell(ToCompositor::Auth(AuthAsk::Closing {
            id: shown.id(),
        }))];
        if let Some(surface) = shown.surface() {
            effects.push(Effect::Keyboard(surface, KeyboardInteractivity::None));
            // It shows what it showed as it closed while it fades out.
            self.closing.push((surface, now + AUTH_OUT, shown));
        }
        if !self.waiting.is_empty() {
            self.next = Some(now + AUTH_NEXT);
        }
        effects
    }

    /// Its size again, if what it shows changed it.
    fn refit(&mut self) -> Vec<Effect> {
        let (waiting, caps) = (self.waiting.len(), self.caps);
        let Some(shown) = self.shown.as_mut() else {
            return Vec::new();
        };
        let Some(id) = shown.surface() else {
            return Vec::new();
        };
        let size = shown.fit(waiting, caps);
        let was = match shown {
            Shown::Polkit(d) => &mut d.size,
            Shown::Secrets(f) => &mut f.size,
        };
        if size == *was {
            return Vec::new();
        }
        *was = size;
        vec![Effect::Resize {
            id,
            size,
            margin: None,
        }]
    }

    /// The password typed goes to the helper.
    fn submit(&mut self) -> bool {
        let Some(Shown::Polkit(dialog)) = self.shown.as_mut() else {
            return false;
        };
        if dialog.phase != Phase::Typing || dialog.field.is_empty() {
            return false;
        }
        if dialog.conversation.is_none() {
            dialog.converse();
        }
        dialog.phase = Phase::Checking;
        dialog.said = None;
        dialog.wrong_note = false;
        dialog.chooser = false;
        if let Some(conversation) = &dialog.conversation {
            // A copy for the helper, made on purpose.
            let mut secret = Secret::longest(LONGEST);
            secret.push(dialog.field.text());
            conversation.answer(secret);
        }
        true
    }

    /// Account `i` answers: the field empties, a conversation for it
    /// starts.
    fn choose(&mut self, i: usize) {
        let Some(Shown::Polkit(dialog)) = self.shown.as_mut() else {
            return;
        };
        dialog.chooser = false;
        dialog.focus = Target::Field;
        if i == dialog.request.chosen || i >= dialog.request.accounts.len() {
            return;
        }
        dialog.request.chosen = i;
        dialog.field.clear();
        dialog.said = None;
        dialog.wrong = 0;
        dialog.wrong_note = false;
        dialog.converse();
    }

    /// Target `target` acts.
    fn act(&mut self, target: Target, now: Instant) -> Vec<Effect> {
        if let Some(Shown::Secrets(form)) = self.shown.as_mut() {
            match target {
                Target::Cancel => return self.finish_form(form::Given::Cancelled, now),
                Target::Connect => {
                    if let Some(given) = form.submit() {
                        return self.finish_form(given, now);
                    }
                }
                Target::Remember => {
                    form.remember = !form.remember;
                    form.focus = Target::Remember;
                }
                Target::Entry(i) if i < form.entries.len() => form.focus = Target::Entry(i),
                _ => {}
            }
            return self.refit();
        }
        let checking = matches!(&self.shown, Some(Shown::Polkit(d)) if d.phase == Phase::Checking);
        let Some(Shown::Polkit(dialog)) = self.shown.as_mut() else {
            return Vec::new();
        };
        match target {
            Target::Cancel => return self.finish(Outcome::Cancelled, now),
            _ if checking => {}
            Target::Authorize => {
                self.submit();
            }
            Target::Field => dialog.focus = Target::Field,
            Target::Details => {
                dialog.details = !dialog.details;
                dialog.turned = Some(now);
                dialog.focus = Target::Details;
            }
            Target::Change => {
                dialog.chooser = !dialog.chooser;
                dialog.focus = Target::Change;
            }
            Target::Account(i) => self.choose(i),
            Target::Entry(_) | Target::Remember | Target::Connect => {}
        }
        self.refit()
    }

    /// What the helper of request `id`'s conversation `generation` said.
    fn heard(&mut self, id: u64, generation: u64, said: Said, now: Instant) -> Vec<Effect> {
        let Some(Shown::Polkit(dialog)) = self.shown.as_mut().filter(
            |s| matches!(s, Shown::Polkit(d) if d.request.id == id && d.generation == generation),
        ) else {
            return Vec::new();
        };
        match said {
            Said::Prompt { echo: _, text } => {
                dialog.prompts += 1;
                // The first is the password's; others (a second factor)
                // show under the field.
                if dialog.prompts > 1 {
                    dialog.said = Some((text.trim().to_owned(), false));
                    dialog.phase = Phase::Typing;
                    dialog.field.clear();
                }
            }
            Said::Info(text) => dialog.said = Some((text.trim().to_owned(), false)),
            Said::Error(text) => dialog.said = Some((text.trim().to_owned(), true)),
            Said::Done(true) => return self.finish(Outcome::Granted, now),
            Said::Done(false) => {
                dialog.conversation = None;
                if dialog.phase == Phase::Checking {
                    dialog.wrong += 1;
                    tracing::debug!(id, wrong = dialog.wrong, "a wrong password");
                    if dialog.wrong >= TRIES {
                        return self.finish(Outcome::Refused, now);
                    }
                    dialog.phase = Phase::Typing;
                    dialog.field.clear();
                    dialog.shake = Some(now);
                    dialog.wrong_note = true;
                    dialog.said = None;
                    dialog.focus = Target::Field;
                    // The helper again, for the next try.
                    dialog.converse();
                }
            }
        }
        let mut effects = self.refit();
        effects.extend(woken());
        effects
    }

    /// What is due at `now`: the surfaces faded out go; a request of
    /// NetworkManager's out of time is cancelled; the next request opens.
    fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let mut effects = Vec::new();
        self.closing.retain(|(id, at, _)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        let before = self.waiting.len();
        self.waiting.retain(|w| match w {
            Waiting::Secrets(ask) if ask.deadline() <= now => {
                tracing::info!(
                    id = ask.id,
                    "no answer in time: the network's request is cancelled"
                );
                ask.answer.send(form::Given::Cancelled);
                false
            }
            _ => true,
        });
        if self.waiting.len() != before && self.shown_surface().is_some() {
            effects.extend(woken());
        }
        let late = matches!(&self.shown, Some(Shown::Secrets(f)) if f.ask.deadline() <= now);
        if late {
            tracing::info!("no answer in time: the network's dialog closes");
            effects.extend(self.finish_form(form::Given::Cancelled, now));
        }
        let opened = self.shown.is_none() && !self.waiting.is_empty();
        let pumped = self.pump(now);
        if opened && !pumped.is_empty() {
            one_at_a_time();
        }
        effects.extend(pumped);
        effects
    }

    /// Target `target` of the dialog on `id` acts (clicked, `Enter`, a
    /// screen reader).
    pub fn act_on(&mut self, id: window::Id, target: Target, now: Instant) -> Vec<Effect> {
        if self.shown_surface() != Some(id) {
            return Vec::new();
        }
        self.act(target, now)
    }

    /// The ring comes to `target` (`Tab`, a press in the field).
    pub fn focus_on(&mut self, id: window::Id, target: Target) {
        if let Some(dialog) = self.dialog_on(id) {
            dialog.focus = target;
        } else if let Some(form) = self.form_on(id) {
            form.focus = target;
        }
    }

    /// `Escape`: the accounts' list folds, else the dialog is cancelled.
    pub fn escape(&mut self, id: window::Id, now: Instant) -> Vec<Effect> {
        if self.form_on(id).is_some() {
            return self.finish_form(form::Given::Cancelled, now);
        }
        let Some(dialog) = self.dialog_on(id) else {
            return Vec::new();
        };
        if dialog.chooser {
            dialog.chooser = false;
            dialog.focus = Target::Change;
            self.refit()
        } else {
            self.finish(Outcome::Cancelled, now)
        }
    }

    /// What the field of the dialog on `id` (the form's holding the ring)
    /// did with a key, a press or the input method.
    pub fn took(&mut self, id: window::Id, took: Took) {
        if took == Took::Paste {
            paste(id);
            return;
        }
        if let Some(dialog) = self.dialog_on(id) {
            noted(dialog, took);
        }
    }

    /// A key typed while the ring is not on the field: it is the field's,
    /// wherever the ring is, while the password is typed.
    pub fn typed(&mut self, id: window::Id, pressed: &Pressed) {
        let Some(dialog) = self.dialog_on(id).filter(|d| d.phase == Phase::Typing) else {
            return;
        };
        let took = dialog
            .field
            .key(&pressed.key, pressed.modifiers, pressed.text.as_deref());
        noted(dialog, took);
        if took == Took::Paste {
            paste(id);
        }
    }

    /// Whether a key pressed now goes to the field though the ring is not
    /// on it.
    pub fn types_elsewhere(&self, id: window::Id) -> bool {
        matches!(&self.shown, Some(Shown::Polkit(d))
            if d.surface == Some(id) && d.phase == Phase::Typing && d.focus != Target::Field)
    }

    /// The field of the dialog on `id` edited by `edit` (the form's that
    /// holds the ring), while it is typed in: whether it draws again.
    fn edited(&mut self, id: window::Id, edit: impl FnOnce(&mut Entry) -> Took) -> bool {
        if let Some(form) = self.form_on(id) {
            let Target::Entry(i) = form.focus else {
                return false;
            };
            return form.entries.get_mut(i).is_some_and(|e| edit(e).draws());
        }
        let Some(dialog) = self.dialog_on(id).filter(|d| d.phase == Phase::Typing) else {
            return false;
        };
        let took = edit(&mut dialog.field);
        noted(dialog, took);
        took.draws()
    }
}

impl Surface for Lent<Auth> {
    type Message = Told;

    fn update(&mut self, told: Told, now: Instant) -> Vec<Effect> {
        self.get_mut().update(told, now)
    }

    fn view(&self, id: window::Id, context: &Context) -> Option<Element<'_, crate::Message>> {
        let (shown, form) = self.read(|a| (a.is_surface(id), a.is_form(id)))?;
        if !shown {
            return None;
        }
        Some(if form {
            form_view::view(self, id, form_view::words(), context.look.glass)
        } else {
            view::view(self, id, words(), context.look.glass, context.look.motion)
        })
    }
}

/// What the field took moves the ring to it; an edit clears the line
/// saying the last password was wrong.
fn noted(dialog: &mut Dialog, took: Took) {
    match took {
        Took::Moved => dialog.focus = Target::Field,
        Took::Edited => {
            dialog.focus = Target::Field;
            dialog.wrong_note = false;
        }
        Took::No | Took::Same | Took::Paste => {}
    }
}

/// The clipboard's text, its line breaks dropped, into the field at its
/// caret, read off the loop: never in a `String` of ours, handed to the
/// loop in a secret.
fn paste(id: window::Id) {
    crate::paste::request(crate::paste::Clean::Dropped, move |pasted| {
        let Some(pasted) = pasted.filter(|t| !t.is_empty()) else {
            return;
        };
        let mut secret = Secret::longest(LONGEST);
        if secret.push(pasted) {
            crate::surfaces::ask(Asked::Auth(Come::Pasted(id, Arc::new(secret))));
        }
    });
}

/// The dialog for screen readers: named by who asks, described by why,
/// holding what its controls declared (the accounts, the field, Details,
/// the buttons), the ring where it is.
pub fn a11y(seen: &Seen) -> Option<hocus_a11y::Window> {
    use hocus_a11y::{Node, Role, Window};
    let rows = crate::a11y::rows::of(seen.surface)?;
    let name = seen.name.clone();
    let node = Node::new("dialog", Role::Dialog, name.clone())
        .describe(seen.message.clone())
        .children(rows.nodes);
    let mut window = Window::new("auth", name);
    window.nodes = vec![node];
    window.active = true;
    window.focus = rows.focus;
    window.size = Some((seen.size.0 as f32, seen.size.1 as f32));
    Some(window)
}

/// A screen reader acts on control `key` of the dialog: asked of the
/// loop, which owns it.
pub fn a11y_click(key: &str) {
    let target = match key.strip_prefix("auth:") {
        Some("Change") => Target::Change,
        Some("Field") => Target::Field,
        Some("Details") => Target::Details,
        Some("Cancel") => Target::Cancel,
        Some("Authorize") => Target::Authorize,
        Some("Remember") => Target::Remember,
        Some("Connect") => Target::Connect,
        Some(other) => {
            let number = |prefix: &str| {
                other
                    .strip_prefix(prefix)
                    .and_then(|k| k.strip_suffix(')'))
                    .and_then(|n| n.parse().ok())
            };
            match (number("Account("), number("Entry(")) {
                (Some(i), _) => Target::Account(i),
                (_, Some(i)) => Target::Entry(i),
                _ => return,
            }
        }
        None => return,
    };
    crate::surfaces::ask(Asked::Auth(Come::Act(target)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn its_words() {
        let fr = Words::new(Lang::French);
        let en = Words::new(Lang::English);
        assert_eq!(fr.password_of("Alex"), "Mot de passe d'Alex");
        assert_eq!(fr.password_of("Sam"), "Mot de passe de Sam");
        assert_eq!(en.password_of("Alex"), "Alex's password");
        assert_eq!(
            fr.wrong(true),
            "Mot de passe incorrect. Réessaie. Les majuscules sont verrouillées."
        );
        assert_eq!(en.wrong(false), "Wrong password. Try again.");
        assert_eq!(fr.waiting(1), "1 autre demande en attente");
        assert_eq!(fr.waiting(2), "2 autres demandes en attente");
        assert_eq!(en.waiting(2), "2 more requests waiting");
        assert_eq!(fr.waiting(0), "");
        assert_eq!(
            fr.started(Some(48213), Some("Alex")),
            "48213, lancé par Alex"
        );
        assert_eq!(en.started(Some(2210), None), "2210");
        assert_eq!(
            fr.name("Mises à jour"),
            "Autorisation nécessaire : Mises à jour"
        );
        assert_eq!(en.name("Updates"), "Authorization required: Updates");
    }

    fn request(id: u64) -> Request {
        Request {
            id,
            cookie: format!("cookie-{id}"),
            action: "org.example.update".into(),
            message: "Updates need your password".into(),
            icon: String::new(),
            // No account: no helper is reached from a test.
            accounts: Vec::new(),
            chosen: 0,
            pid: None,
            pids: Vec::new(),
            program: "updater".into(),
            started_by: None,
        }
    }

    /// The dialog alone (#773): a request opens it, a second waits; a
    /// wrong password shakes it and its view says so; cancelled, polkit is
    /// answered, it fades out and the next request opens.
    #[test]
    fn the_dialog_alone_opens_says_wrong_and_hands_on_to_the_next() {
        use iced::keyboard::Key;
        let auth: Lent<Auth> = Lent::default();
        let tell = |told: Told, now: Instant| auth.change(|a| a.update(told, now)).expect("free");
        let now = Instant::now();
        let (first, answered) = oneshot::channel();
        let effects = tell(
            Told::Come(Come::Begin(Box::new(request(1)), Answer::new(first))),
            now,
        );
        let Some(Effect::Auth { id, .. }) = effects.first() else {
            panic!("its surface opened: {effects:?}");
        };
        let id = *id;
        let (second, _) = oneshot::channel();
        tell(
            Told::Come(Come::Begin(Box::new(request(2)), Answer::new(second))),
            now,
        );
        // Typed, sent, wrong.
        auth.change(|a| {
            a.typed(
                id,
                &Pressed {
                    key: Key::Character("x".into()),
                    modifiers: iced::keyboard::Modifiers::empty(),
                    text: Some("x".into()),
                },
            );
            a.act_on(id, Target::Authorize, now)
        });
        let generation = auth
            .read(|a| match &a.shown {
                Some(Shown::Polkit(d)) => Some(d.generation),
                _ => None,
            })
            .flatten();
        tell(
            Told::Come(Come::Heard {
                id: 1,
                generation: generation.expect("shown"),
                said: Said::Done(false),
            }),
            now,
        );
        let words = Words::new(Lang::English);
        let shows = auth
            .read(|a| {
                a.drawn(id, |dialog, caps, waiting| {
                    view::content(&auth, dialog, caps, waiting, words, false).1
                })
            })
            .flatten()
            .expect("drawn");
        assert!(
            shows.iter().any(|s| s.contains(&words.wrong(false))),
            "{shows:?}"
        );
        assert!(
            shows.iter().any(|s| s.contains(&words.waiting(1))),
            "{shows:?}"
        );
        // Cancelled: polkit hears it, the surface fades out, then goes.
        tell(Told::Come(Come::Act(Target::Cancel)), now);
        assert_eq!(answered.blocking_recv().ok(), Some(Outcome::Cancelled));
        assert!(auth.read(|a| a.closing(id)).expect("free"));
        let due = auth.change(Auth::due).flatten().expect("a tick due");
        assert_eq!(auth.change(Auth::due).flatten(), None, "asked once");
        let effects = tell(Told::Come(Come::Tick), due.max(now + AUTH_NEXT));
        assert!(effects.contains(&Effect::Remove(id)));
        assert!(
            effects.iter().any(|e| matches!(e, Effect::Auth { .. })),
            "the next request opens: {effects:?}"
        );
    }
}
