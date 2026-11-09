//! The outside effects of the cards the user accepted (§2.1): each kind
//! carried out by its [`Effector`], only after the click on that card,
//! once. The lots that act plug theirs in here ([`Effectors::with`]):
//! lot 5's is the To do list ([`ToDo`]), whose list is the shell's (it
//! owns the tasks' account), so its effect is a line to the shell
//! ([`Outcome::Shell`]), answered when done. A kind with no effector yet
//! says so on its card; with the recorder (tests), every effect is
//! written down as it happens, whichever carries it out.

use std::sync::Arc;

use hocus_secretary_link::{Effect, Notebook};
use jiff::Timestamp;

use crate::model::Recorder;

/// What a card proposes to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Message,
    Post,
    Mail,
    Invitation,
    Event,
    File,
    Task,
    Agent,
    Delegation,
    /// Not an outside effect: a draft prepared, shown as its own card for
    /// the user to send (the move of a meeting, AC10).
    Prepare,
}

impl Kind {
    pub const fn word(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Post => "post",
            Self::Mail => "mail",
            Self::Invitation => "invitation",
            Self::Event => "event",
            Self::File => "file",
            Self::Task => "task",
            Self::Agent => "agent",
            Self::Delegation => "delegation",
            Self::Prepare => "prepare",
        }
    }
}

/// A card's proposal: its kind, the tool call that made it, its input.
#[derive(Debug, Clone, PartialEq)]
pub struct Proposal {
    pub kind: Kind,
    pub tool: String,
    pub input: serde_json::Value,
    pub notebook: Notebook,
}

/// What came of an effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Done: its words under the card.
    Done(String),
    /// For the shell to carry out: the card waits for its answer.
    Shell(Effect),
    /// It cannot be done (yet): why, under the card.
    Unable(String),
}

/// Carries out one kind of effect. Called on her loop: it must not block
/// (a slow effect hands its work to a thread, or to the shell).
pub trait Effector: Send + std::fmt::Debug {
    fn kind(&self) -> Kind;
    /// Proposal `proposal`, accepted on card `card`.
    fn carry_out(&self, card: u64, proposal: &Proposal, english: bool) -> Outcome;
}

/// A task at the top of the To do list (Day.4): the shell adds it to the
/// day's tasks' account.
#[derive(Debug, Default)]
pub struct ToDo;

impl ToDo {
    /// The task's title, from what the model or code gave.
    pub fn title(input: &serde_json::Value) -> Option<String> {
        let field = |name: &str| {
            input
                .get(name)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
        };
        field("title")
            .or_else(|| field("task"))
            .or_else(|| field("text"))
            .or_else(|| {
                input
                    .get("lines")
                    .and_then(|v| v.as_array())
                    .and_then(|a| a.first())
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            })
    }
}

impl Effector for ToDo {
    fn kind(&self) -> Kind {
        Kind::Task
    }

    fn carry_out(&self, card: u64, proposal: &Proposal, english: bool) -> Outcome {
        match Self::title(&proposal.input) {
            Some(title) => Outcome::Shell(Effect::Task {
                id: card,
                title: title.chars().take(200).collect(),
                notebook: proposal.notebook,
            }),
            None => Outcome::Unable(
                if english {
                    "No task to add: nothing was added."
                } else {
                    "Pas de tâche à ajouter : rien n’est ajouté."
                }
                .to_owned(),
            ),
        }
    }
}

/// Every effector, and the recorder.
#[derive(Debug)]
pub struct Effectors {
    list: Vec<Box<dyn Effector>>,
    recorder: Option<Arc<Recorder>>,
}

impl Effectors {
    /// Lot 5's: the To do list; `recorder` writes every effect down.
    pub fn new(recorder: Option<Arc<Recorder>>) -> Self {
        Self {
            list: vec![Box::new(ToDo)],
            recorder,
        }
    }

    /// Another kind's effector, in place of the one it had.
    #[must_use]
    pub fn with(mut self, effector: Box<dyn Effector>) -> Self {
        self.list.retain(|e| e.kind() != effector.kind());
        self.list.push(effector);
        self
    }

    /// Proposal `proposal`, accepted on card `card`, carried out at
    /// `now`.
    pub fn carry_out(
        &self,
        now: Timestamp,
        card: u64,
        proposal: &Proposal,
        english: bool,
    ) -> Outcome {
        if let Some(recorder) = &self.recorder {
            let detail = format!("card:{card} {} {}", proposal.tool, proposal.input);
            recorder.effect(now, proposal.kind.word(), &detail);
        }
        match self.list.iter().find(|e| e.kind() == proposal.kind) {
            Some(effector) => effector.carry_out(card, proposal, english),
            None if self.recorder.is_some() => {
                Outcome::Done(if english { "Done." } else { "Fait." }.to_owned())
            }
            None => Outcome::Unable(
                if english {
                    "I can’t do this myself yet: nothing was sent."
                } else {
                    "Je ne sais pas encore le faire moi-même : rien n’est parti."
                }
                .to_owned(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposal(kind: Kind, input: serde_json::Value) -> Proposal {
        Proposal {
            kind,
            tool: "propose".to_owned(),
            input,
            notebook: Notebook::Pro,
        }
    }

    #[test]
    fn a_task_goes_to_the_shell_and_an_unknown_kind_says_so() {
        let effectors = Effectors::new(None);
        let now = Timestamp::UNIX_EPOCH;
        let task = proposal(
            Kind::Task,
            serde_json::json!({"title": "Envoyer le devis à Léa"}),
        );
        assert_eq!(
            effectors.carry_out(now, 4, &task, false),
            Outcome::Shell(Effect::Task {
                id: 4,
                title: "Envoyer le devis à Léa".to_owned(),
                notebook: Notebook::Pro,
            })
        );
        let mail = proposal(Kind::Mail, serde_json::json!({"to": "Léa"}));
        assert!(matches!(
            effectors.carry_out(now, 5, &mail, false),
            Outcome::Unable(_)
        ));
        // With the recorder, every effect is written down.
        let recorder = Arc::new(Recorder::in_memory());
        let effectors = Effectors::new(Some(recorder.clone()));
        assert!(matches!(
            effectors.carry_out(now, 5, &mail, false),
            Outcome::Done(_)
        ));
        assert!(matches!(
            effectors.carry_out(now, 6, &task, false),
            Outcome::Shell(_)
        ));
        assert_eq!(recorder.lines().len(), 2);
    }
}
