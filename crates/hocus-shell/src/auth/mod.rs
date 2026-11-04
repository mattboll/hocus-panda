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
//! cancelling, the same. As for the launcher, nothing goes through iced's
//! messages: the dialog's state is here, the keys, the pointer and the
//! helper change it and draw its surface alone.

mod accounts;
mod agent;
pub mod draw;
mod helper;

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{Mutex, PoisonError},
    time::Instant,
};

use hocus_link::{AUTH_NAMESPACE, AUTH_NEXT, AUTH_OUT, AUTH_TOP, AuthAsk, AuthAt, ToCompositor};
use iced::{keyboard, window};
use iced_layershell::reexport::KeyboardInteractivity;
use tokio::sync::oneshot;

pub use accounts::Account;
use helper::{Conversation, Said};

use crate::{clock::time::Lang, network::secret::Secret, popover::Effect};

/// Wrong passwords in a row before the request is refused.
pub const TRIES: u32 = 3;
/// The characters the field keeps at most.
const LONGEST: usize = 256;

pub fn start() {
    agent::start();
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
    pub field: Secret,
    /// Wrong passwords in a row, and whether the line says the last was.
    pub wrong: u32,
    pub wrong_note: bool,
    /// What PAM said last (and whether it was an error).
    pub said: Option<(String, bool)>,
    pub details: bool,
    pub chooser: bool,
    pub focus: Target,
    pub hover: Option<Target>,
    pub pressed: Option<Target>,
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
            field: Secret::longest(LONGEST),
            wrong: 0,
            wrong_note: false,
            said: None,
            details: false,
            chooser: false,
            focus: Target::Field,
            hover: None,
            pressed: None,
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

    /// What holds the keyboard in turn, as `Tab` goes.
    pub fn order(&self) -> Vec<Target> {
        let mut order = Vec::new();
        if self.several() {
            order.push(Target::Change);
            if self.chooser {
                order.extend((0..self.request.accounts.len()).map(Target::Account));
            }
        }
        order.extend([
            Target::Field,
            Target::Details,
            Target::Cancel,
            Target::Authorize,
        ]);
        order
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
            move |said| heard(id, generation, said),
        ));
    }

    fn end_conversation(&mut self) {
        if let Some(old) = self.conversation.take() {
            old.end();
        }
    }
}

/// The dialog and the requests waiting.
#[derive(Debug)]
struct Auth {
    waiting: VecDeque<Request>,
    shown: Option<Dialog>,
    /// Surfaces fading out, and when they go.
    closing: Vec<(window::Id, Instant)>,
    /// When the next request opens.
    next: Option<Instant>,
    caps: bool,
    answers: BTreeMap<u64, oneshot::Sender<Outcome>>,
}

static AUTH: Mutex<Auth> = Mutex::new(Auth {
    waiting: VecDeque::new(),
    shown: None,
    closing: Vec::new(),
    next: None,
    caps: false,
    answers: BTreeMap::new(),
});

fn with<T>(with: impl FnOnce(&mut Auth) -> T) -> T {
    with(&mut AUTH.lock().unwrap_or_else(PoisonError::into_inner))
}

/// `with` on the dialog shown, if its surface is `id`, with Caps Lock's
/// state and the requests waiting.
pub fn with_dialog<T>(
    id: window::Id,
    with: impl FnOnce(&mut Dialog, bool, usize) -> T,
) -> Option<T> {
    self::with(|auth| {
        let (caps, waiting) = (auth.caps, auth.waiting.len());
        auth.shown
            .as_mut()
            .filter(|d| d.surface == Some(id))
            .map(|d| with(d, caps, waiting))
    })
}

/// Whether surface `id` is the dialog's, shown or fading out.
pub fn is_surface(id: window::Id) -> bool {
    with(|auth| {
        auth.shown.as_ref().is_some_and(|d| d.surface == Some(id))
            || auth.closing.iter().any(|(c, _)| *c == id)
    })
}

/// Its surface draws again (something it shows changed).
pub fn wake() {
    if with(|auth| auth.shown.as_ref().is_some_and(|d| d.surface.is_some())) {
        crate::popover::send(vec![Effect::Wake(AUTH_NAMESPACE)]);
    }
}

/// The dialog's words, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    fr: bool,
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
        self.pick("Autorisation nécessaire", "Authorization required")
    }

    /// The dialog's name for screen readers.
    pub fn name(self, app: &str) -> String {
        if self.fr {
            format!("Autorisation nécessaire : {app}")
        } else {
            format!("Authorization required: {app}")
        }
    }

    pub fn administrator(self) -> &'static str {
        self.pick("administrateur", "administrator")
    }

    pub fn change(self) -> &'static str {
        self.pick("Changer", "Change")
    }

    pub fn password(self) -> &'static str {
        self.pick("Mot de passe", "Password")
    }

    /// The field's name for screen readers.
    pub fn password_of(self, name: &str) -> String {
        if self.fr {
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
        self.pick("Majuscules verrouillées", "Caps Lock is on")
    }

    pub fn checking(self) -> &'static str {
        self.pick("Vérification…", "Checking…")
    }

    /// A wrong password, Caps Lock on or not.
    pub fn wrong(self, caps: bool) -> String {
        let wrong = self.pick(
            "Mot de passe incorrect. Réessaie.",
            "Wrong password. Try again.",
        );
        if caps {
            format!(
                "{wrong} {}",
                self.pick("Les majuscules sont verrouillées.", "Caps Lock is on.")
            )
        } else {
            wrong.to_owned()
        }
    }

    pub fn details(self) -> &'static str {
        self.pick("Détails", "Details")
    }

    pub fn action(self) -> &'static str {
        "Action"
    }

    pub fn program(self) -> &'static str {
        self.pick("Programme", "Program")
    }

    pub fn process(self) -> &'static str {
        self.pick("Processus", "Process")
    }

    pub fn window(self) -> &'static str {
        self.pick("Fenêtre", "Window")
    }

    /// The process's line: its id, and who started it.
    pub fn started(self, pid: Option<u32>, by: Option<&str>) -> String {
        let pid = pid.map_or_else(|| "?".to_owned(), |p| p.to_string());
        match (by, self.fr) {
            (Some(by), true) => format!("{pid}, lancé par {by}"),
            (Some(by), false) => format!("{pid}, started by {by}"),
            (None, _) => pid,
        }
    }

    pub fn none(self) -> &'static str {
        self.pick("aucune", "none")
    }

    /// The requests waiting behind it.
    pub fn waiting(self, n: usize) -> String {
        match (n, self.fr) {
            (0, _) => String::new(),
            (1, true) => "1 autre demande en attente".to_owned(),
            (n, true) => format!("{n} autres demandes en attente"),
            (1, false) => "1 more request waiting".to_owned(),
            (n, false) => format!("{n} more requests waiting"),
        }
    }

    pub fn cancel(self) -> &'static str {
        self.pick("Annuler", "Cancel")
    }

    pub fn authorize(self) -> &'static str {
        self.pick("Autoriser", "Authorize")
    }
}

pub fn words() -> Words {
    Words::new(Lang::of_messages())
}

/// A request from polkit (on the agent's thread): it shows, or waits.
fn begin(request: Request, answer: oneshot::Sender<Outcome>) {
    let now = Instant::now();
    let (effects, opened) = with(|auth| {
        auth.answers.insert(request.id, answer);
        auth.waiting.push_back(request);
        let mut effects = auth.pump(now);
        let opened = !effects.is_empty();
        if !opened && auth.shown.as_ref().is_some_and(|d| d.surface.is_some()) {
            // The footer counts it.
            effects.push(Effect::Wake(AUTH_NAMESPACE));
        }
        (effects, opened)
    });
    if opened {
        one_at_a_time();
    }
    crate::popover::send(effects);
}

/// The launcher, the emoji picker and the bar's popover close: the dialog
/// alone holds the keyboard.
fn one_at_a_time() {
    crate::popover::event(crate::popover::Event::PressElsewhere);
    crate::launcher::close();
    crate::emoji::close();
}

/// polkit cancels the request of `cookie` (its application gave up): its
/// dialog closes at once, without a message.
fn cancel_cookie(cookie: &str) {
    let now = Instant::now();
    let effects = with(|auth| {
        if let Some(at) = auth.waiting.iter().position(|r| r.cookie == cookie) {
            if let Some(request) = auth.waiting.remove(at) {
                auth.answer(request.id, Outcome::Cancelled);
            }
            return vec![Effect::Wake(AUTH_NAMESPACE)];
        }
        if auth
            .shown
            .as_ref()
            .is_some_and(|d| d.request.cookie == cookie)
        {
            tracing::debug!("polkit cancels the dialog's request");
            return auth.finish(Outcome::Cancelled, now);
        }
        Vec::new()
    });
    crate::popover::send(effects);
}

impl Auth {
    fn answer(&mut self, id: u64, outcome: Outcome) {
        if let Some(answer) = self.answers.remove(&id) {
            let _ = answer.send(outcome);
        }
    }

    /// The next request shows if none does and its time came.
    fn pump(&mut self, now: Instant) -> Vec<Effect> {
        if self.shown.is_some() || self.next.is_some_and(|at| at > now) {
            return Vec::new();
        }
        self.next = None;
        let Some(request) = self.waiting.pop_front() else {
            return Vec::new();
        };
        let ask = AuthAsk::Open {
            id: request.id,
            pids: request.pids.clone(),
        };
        tracing::debug!(id = request.id, "the authentication dialog is asked for");
        self.shown = Some(Dialog::new(request));
        if crate::popover::linked_now() {
            vec![Effect::Tell(ToCompositor::Auth(ask))]
        } else {
            // A shell alone: on the active screen.
            self.place(AuthAt {
                id: self.shown.as_ref().map_or(0, |d| d.request.id),
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
        let Some(dialog) = self.shown.as_mut() else {
            return Vec::new();
        };
        if dialog.request.id != at.id || dialog.surface.is_some() {
            return Vec::new();
        }
        let id = window::Id::unique();
        dialog.surface = Some(id);
        let top = (f64::from(at.height) / 2.0 - AUTH_TOP).max(8.0).round() as i32;
        let output = at.output.clone();
        dialog.at = Some(at);
        dialog.size = draw::Layout::of(dialog, words(), self.waiting.len(), self.caps).surface();
        let app = draw::who(dialog).0;
        if let Some(icon) = draw::who(dialog).1 {
            let scale = dialog.at.as_ref().map_or(1.0, |a| a.scale as f32 / 120.0);
            crate::launcher::icons::want([icon.as_str()], draw::ICON_PX, scale);
        }
        tracing::debug!(
            id = dialog.request.id,
            output,
            app,
            accounts = dialog.request.accounts.len(),
            "the authentication dialog opens"
        );
        dialog.converse();
        vec![Effect::Auth {
            id,
            output,
            size: dialog.size,
            top,
        }]
    }

    /// The dialog closes with `outcome`.
    fn finish(&mut self, outcome: Outcome, now: Instant) -> Vec<Effect> {
        let Some(mut dialog) = self.shown.take() else {
            return Vec::new();
        };
        dialog.end_conversation();
        self.answer(dialog.request.id, outcome);
        tracing::debug!(
            id = dialog.request.id,
            ?outcome,
            "the authentication dialog closes"
        );
        let mut effects = vec![Effect::Tell(ToCompositor::Auth(AuthAsk::Closing {
            id: dialog.request.id,
        }))];
        if let Some(surface) = dialog.surface {
            effects.push(Effect::Keyboard(surface, KeyboardInteractivity::None));
            self.closing.push((surface, now + AUTH_OUT));
        }
        if !self.waiting.is_empty() {
            self.next = Some(now + AUTH_NEXT);
        }
        effects
    }

    /// Its size again, if what it shows changed it.
    fn refit(&mut self) -> Vec<Effect> {
        let (waiting, caps) = (self.waiting.len(), self.caps);
        let Some(dialog) = self.shown.as_mut() else {
            return Vec::new();
        };
        let Some(id) = dialog.surface else {
            return Vec::new();
        };
        let size = draw::Layout::of(dialog, words(), waiting, caps).surface();
        if size == dialog.size {
            return Vec::new();
        }
        dialog.size = size;
        vec![Effect::Resize {
            id,
            size,
            margin: None,
        }]
    }

    /// The password typed goes to the helper.
    fn submit(&mut self) -> bool {
        let Some(dialog) = self.shown.as_mut() else {
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
            conversation.answer(dialog.field.duplicate());
        }
        true
    }

    /// Account `i` answers: the field empties, a conversation for it
    /// starts.
    fn choose(&mut self, i: usize) {
        let Some(dialog) = self.shown.as_mut() else {
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
        let checking = self
            .shown
            .as_ref()
            .is_some_and(|d| d.phase == Phase::Checking);
        match target {
            Target::Cancel => return self.finish(Outcome::Cancelled, now),
            _ if checking => {}
            Target::Authorize => {
                self.submit();
            }
            Target::Field => {
                if let Some(d) = self.shown.as_mut() {
                    d.focus = Target::Field;
                }
            }
            Target::Details => {
                if let Some(d) = self.shown.as_mut() {
                    d.details = !d.details;
                    d.turned = Some(now);
                    d.focus = Target::Details;
                }
            }
            Target::Change => {
                if let Some(d) = self.shown.as_mut() {
                    d.chooser = !d.chooser;
                    d.focus = Target::Change;
                }
            }
            Target::Account(i) => self.choose(i),
        }
        self.refit()
    }
}

/// What the helper of request `id`'s conversation `generation` said.
fn heard(id: u64, generation: u64, said: Said) {
    let now = Instant::now();
    let effects = with(|auth| {
        let Some(dialog) = auth
            .shown
            .as_mut()
            .filter(|d| d.request.id == id && d.generation == generation)
        else {
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
            Said::Done(true) => return auth.finish(Outcome::Granted, now),
            Said::Done(false) => {
                dialog.conversation = None;
                if dialog.phase == Phase::Checking {
                    dialog.wrong += 1;
                    tracing::debug!(id, wrong = dialog.wrong, "a wrong password");
                    if dialog.wrong >= TRIES {
                        return auth.finish(Outcome::Refused, now);
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
        let mut effects = auth.refit();
        effects.push(Effect::Wake(AUTH_NAMESPACE));
        effects
    });
    crate::popover::send(effects);
}

/// `auth-at`: the dialog goes there.
pub fn placed(at: AuthAt) {
    let effects = with(|auth| auth.place(at));
    crate::popover::send(effects);
}

/// `caps`: Caps Lock's state.
pub fn caps(on: bool) {
    let changed = with(|auth| std::mem::replace(&mut auth.caps, on) != on);
    if changed {
        wake();
    }
}

/// When [`tick`] has something to do.
pub fn deadline() -> Option<Instant> {
    with(|auth| {
        auth.closing
            .iter()
            .map(|(_, at)| *at)
            .chain(auth.next)
            .min()
    })
}

/// What is due at `now`: the surfaces faded out go; the next request
/// opens.
pub fn tick(now: Instant) -> Vec<Effect> {
    let (effects, opened) = with(|auth| {
        let mut effects = Vec::new();
        auth.closing.retain(|(id, at)| {
            let due = *at <= now;
            if due {
                effects.push(Effect::Remove(*id));
            }
            !due
        });
        let opened = auth.shown.is_none() && !auth.waiting.is_empty();
        let pumped = auth.pump(now);
        let opened = opened && !pumped.is_empty();
        effects.extend(pumped);
        (effects, opened)
    });
    if opened {
        one_at_a_time();
    }
    effects
}

/// What the pointer does on the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pointing {
    Over(Option<Target>),
    Press(Option<Target>),
    Release(Option<Target>),
}

/// What the pointer did on surface `id`, at `now`: whether to draw.
pub fn pointer(id: window::Id, event: Pointing, now: Instant) -> bool {
    let effects = with(|auth| {
        let dialog = auth.shown.as_mut().filter(|d| d.surface == Some(id))?;
        match event {
            Pointing::Over(target) => {
                if dialog.hover == target {
                    return Some((Vec::new(), false));
                }
                dialog.hover = target;
                Some((Vec::new(), true))
            }
            Pointing::Press(target) => {
                dialog.pressed = target;
                Some((Vec::new(), false))
            }
            Pointing::Release(target) => {
                let clicked = dialog.pressed.take().filter(|p| Some(*p) == target);
                match clicked {
                    Some(target) => Some((auth.act(target, now), true)),
                    None => Some((Vec::new(), false)),
                }
            }
        }
    });
    let Some((effects, draw)) = effects else {
        return false;
    };
    crate::popover::send(effects);
    draw
}

/// A key pressed on surface `id`: whether the dialog took it.
pub fn key_on(
    id: window::Id,
    key: &keyboard::Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
    now: Instant,
) -> bool {
    use keyboard::{Key, key::Named};
    let effects = with(|auth| {
        let dialog = auth.shown.as_mut().filter(|d| d.surface == Some(id))?;
        let typing = dialog.phase == Phase::Typing;
        let mut effects = Vec::new();
        match key {
            Key::Named(Named::Escape) => {
                if dialog.chooser {
                    dialog.chooser = false;
                    dialog.focus = Target::Change;
                    effects = auth.refit();
                } else {
                    effects = auth.finish(Outcome::Cancelled, now);
                }
            }
            Key::Named(Named::Tab) => {
                let order = dialog.order();
                let at = order.iter().position(|t| *t == dialog.focus).unwrap_or(0);
                let n = order.len();
                let next = if modifiers.shift() {
                    (at + n - 1) % n
                } else {
                    (at + 1) % n
                };
                dialog.focus = order[next];
            }
            Key::Named(Named::Enter) => {
                let focus = dialog.focus;
                effects = auth.act(
                    if focus == Target::Field {
                        Target::Authorize
                    } else {
                        focus
                    },
                    now,
                );
            }
            Key::Named(Named::Space) if dialog.focus != Target::Field => {
                let focus = dialog.focus;
                effects = auth.act(focus, now);
            }
            Key::Named(Named::Backspace) if typing => {
                if modifiers.control() {
                    dialog.field.clear();
                } else {
                    dialog.field.pop();
                }
                dialog.focus = Target::Field;
                dialog.wrong_note = false;
            }
            _ if typing && !(modifiers.control() || modifiers.logo() || modifiers.alt()) => {
                let text = text.filter(|t| !t.is_empty() && t.chars().all(|c| !c.is_control()))?;
                if dialog.field.push(text) {
                    dialog.focus = Target::Field;
                    dialog.wrong_note = false;
                }
            }
            _ => return None,
        }
        Some(effects)
    });
    let Some(effects) = effects else {
        return false;
    };
    crate::popover::send(effects);
    true
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

    fn dialog(accounts: usize) -> Dialog {
        Dialog::new(Request {
            id: 1,
            cookie: "c".into(),
            action: "org.example.updates.install".into(),
            message: "m".into(),
            icon: String::new(),
            accounts: (0..accounts)
                .map(|i| Account {
                    uid: 1000 + i as u32,
                    login: format!("u{i}"),
                    name: format!("U{i}"),
                })
                .collect(),
            chosen: 0,
            pid: None,
            pids: Vec::new(),
            program: String::new(),
            started_by: None,
        })
    }

    #[test]
    fn the_keyboard_goes_through_what_shows() {
        let one = dialog(1);
        assert_eq!(
            one.order(),
            [
                Target::Field,
                Target::Details,
                Target::Cancel,
                Target::Authorize
            ]
        );
        let mut two = dialog(2);
        assert_eq!(two.order()[0], Target::Change);
        two.chooser = true;
        assert_eq!(
            &two.order()[..4],
            &[
                Target::Change,
                Target::Account(0),
                Target::Account(1),
                Target::Field
            ]
        );
    }
}
