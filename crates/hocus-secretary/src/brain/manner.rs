//! Her way of being, on her loop (#753, [`crate::style`]):
//!
//! - a sentence of the user's in the drawer that asks her to be otherwise,
//!   read in code: kept at once, "Retenu : … · Annuler", saying what it
//!   replaces; one that asks to loosen what code enforces, refused with a
//!   plain sentence, nothing kept;
//! - her model's `remember_style`: a card the user keeps ("Garder") or
//!   not, only when the words it quotes are the user's own sentence of
//!   that turn — what a source says never reaches here;
//! - "Oublier" in "Ce qu'elle sait": the item closed (its history kept),
//!   with the toast's "Annuler";
//! - what she observes in her own thread, each night: proposed once,
//!   nothing changed without a "Oui".

use hocus_secretary_link::{Action, Body, Card, CardKind, Decision};
use jiff::Timestamp;

use super::{Brain, memory::Back};
use crate::{
    audit,
    graph::Origin,
    learn::{self, Noticed, short},
    model::ToolCall,
    retrieve::day_of,
    style::{self, Guard, Kept, Style},
};

impl Brain {
    /// A sentence about how she should be, read in code: whether it was
    /// one (and she answered it).
    pub(super) fn styled(&mut self, conn: u64, id: u64, text: &str) -> bool {
        if let Some(guard) = style::guard(text) {
            self.refuse_style(id, guard);
            self.done(conn, id);
            return true;
        }
        let Some(asked) = style::heard(text) else {
            return false;
        };
        let pointer = format!("conversation:{}", self.last_item());
        self.keep_style(id, &asked, text, &Origin::told(pointer));
        self.done(conn, id);
        true
    }

    fn refuse_style(&mut self, id: u64, guard: Guard) {
        tracing::info!(?guard, "a way of being refused: code enforces it");
        let why = guard.why(self.talk.fr()).to_owned();
        self.her(id, why, None);
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "style".to_owned(),
            outcome: Some("refused".to_owned()),
            ..Default::default()
        });
    }

    /// `asked` kept, said "Retenu : …" with "Annuler", and what it
    /// replaces.
    fn keep_style(&mut self, id: u64, asked: &Style, sentence: &str, origin: &Origin) {
        let fr = self.talk.fr();
        let today = day_of(self.clock.now());
        match style::keep(&mut self.store, asked, sentence, origin, today) {
            Ok(Kept::Already { since }) => {
                let text = if fr {
                    format!(
                        "C’est déjà ce que je fais, depuis le {}\u{a0}: «\u{a0}{}\u{a0}».",
                        short(since, true),
                        asked.said(true)
                    )
                } else {
                    format!(
                        "That’s what I do already, since {}: “{}”.",
                        short(since, false),
                        asked.said(false)
                    )
                };
                self.her(id, text, None);
            }
            Ok(Kept::New {
                rule,
                edits,
                replaced,
            }) => {
                if let Some((old, day)) = replaced {
                    let text = if fr {
                        format!(
                            "Ça remplace «\u{a0}{}\u{a0}», que tu m’avais demandé le {}.",
                            old.said(true),
                            short(day, true)
                        )
                    } else {
                        format!(
                            "It replaces “{}”, which you asked on {}.",
                            old.said(false),
                            short(day, false)
                        )
                    };
                    // "9 nov." ends the sentence already.
                    let text = text.replace("..", ".");
                    self.her(id, text, None);
                }
                let text = if fr {
                    format!("Noté\u{a0}: {}", lower_first(&asked.said(true)))
                } else {
                    format!("Noted: {}", lower_first(&asked.said(false)))
                };
                self.learned(id, text, edits);
                tracing::info!(rule, aspect = asked.aspect(), "a way of being kept");
            }
            Err(err) => {
                tracing::warn!(%err, "could not keep a way of being");
                let text = self
                    .talk
                    .t("Je n’ai pas pu le retenir.", "I could not keep it.");
                self.her(id, text.to_owned(), None);
            }
        }
    }

    /// Her model's `remember_style` in answer to `asked`, the user's
    /// sentence of this turn: a card to keep, or refused. What her model
    /// is told.
    pub(super) fn style_called(&mut self, id: u64, call: &ToolCall, asked: Option<&str>) -> String {
        let field = |name: &str| {
            call.input
                .get(name)
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        };
        let (said, topic, instruction) = (field("said"), field("topic"), field("instruction"));
        // Only the user's own words in the drawer count, never what a
        // source said.
        if !asked.is_some_and(|a| style::said_by_user(&said, a)) {
            tracing::info!("a way of being not said by the user: nothing kept");
            return "Refusé : seules ses phrases dans cette conversation comptent ; rien n'est retenu."
                .to_owned();
        }
        if let Some(guard) = style::guard(&said).or_else(|| style::guard(&instruction)) {
            self.refuse_style(id, guard);
            return "Refusé : ce que le code impose ne change pas ; tu le lui as dit.".to_owned();
        }
        let Some(proposed) = style::of_model(&said, &topic, &instruction) else {
            return "Rien à retenir.".to_owned();
        };
        let fr = self.talk.fr();
        let card = Card {
            kind: CardKind::Rule,
            label: self
                .talk
                .t(
                    "Sa façon d’être · à confirmer",
                    "Her way of being · to confirm",
                )
                .to_owned(),
            text: proposed.said(fr),
            lines: vec![if fr {
                format!("Tu as dit\u{a0}: «\u{a0}{}\u{a0}»", said.trim())
            } else {
                format!("You said: “{}”", said.trim())
            }],
            actions: vec![
                Action {
                    label: self.talk.t("Garder", "Keep").to_owned(),
                    decision: Decision::Accept,
                },
                Action {
                    label: self.talk.t("Non merci", "No thanks").to_owned(),
                    decision: Decision::Reject,
                },
            ],
            decided: None,
            outcome: None,
            sheet: None,
        };
        let item = self.add_item(Some(id), Body::Card { card });
        let pointer = format!("conversation:{item}");
        self.memory
            .styles
            .insert(item, (proposed, said.trim().to_owned(), pointer));
        self.audited(audit::Entry {
            kind: audit::Kind::Proposal,
            what: "style".to_owned(),
            ids: vec![format!("card:{item}")],
            ..Default::default()
        });
        "Montré à l'utilisateur ; rien n'est retenu avant son clic.".to_owned()
    }

    /// A button of her way-of-being card: whether it was one.
    pub(super) fn style_decide(
        &mut self,
        conn: u64,
        id: u64,
        item: u64,
        decision: Decision,
    ) -> bool {
        let Some((proposed, said, pointer)) = self.memory.styles.remove(&item) else {
            return false;
        };
        let outcome = match decision {
            Decision::Accept => {
                self.keep_style(id, &proposed, &said, &Origin::told(pointer));
                self.talk.t("Gardé.", "Kept.")
            }
            _ => self.talk.t("Écarté.", "Dismissed."),
        };
        self.change_item(item, |body| {
            if let Body::Card { card } = body {
                card.decided = Some(decision);
                card.outcome = Some(outcome.to_owned());
            }
        });
        self.tell_look();
        self.done(conn, id);
        true
    }

    /// "Oublier" a way of being in "Ce qu'elle sait": closed today, never
    /// deleted, with "Annuler" in the toast. Whether rule `rule` was one.
    pub(super) fn forget_style(&mut self, conn: u64, id: u64, rule: i64) -> Result<bool, String> {
        if !style::is_style_rule(&self.store, rule) {
            return Ok(false);
        }
        let today = day_of(self.clock.now());
        let edit = learn::close_rule(&mut self.store, rule, today).map_err(|e| e.to_string())?;
        tracing::info!(rule, "a way of being forgotten (closed)");
        let text = self.talk.t("Oublié", "Forgotten");
        self.toast(conn, id, text, Some(Back::Edits(vec![edit])));
        Ok(true)
    }

    /// The user unfolded item `item` of hers (a briefing).
    pub(super) fn unfolded(&mut self, item: u64) {
        if let Err(err) = style::opened(&self.store, item, self.clock.now()) {
            tracing::warn!(%err, "could not note an unfolded item");
        }
    }

    /// What her own thread suggests of her way of being, asked once.
    pub(super) fn style_observe(&mut self, now: Timestamp) {
        match style::observe(&self.store, &self.talk.items, now) {
            Ok(found) => {
                for (style, cause, count) in found {
                    self.notice(Noticed::Style {
                        style,
                        cause,
                        count,
                    });
                }
            }
            Err(err) => tracing::warn!(%err, "could not look at her own thread"),
        }
    }
}

fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}
