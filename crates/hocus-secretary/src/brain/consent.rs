//! Her sources' consent in the drawer, and her first run (§10.1, §15,
//! #750).
//!
//! Her Google source says, at each of its cycles, the accounts Online
//! Accounts has ([`super::Synced::found`]). The first time a shell (or a
//! test) is there once she knows them, she introduces herself and asks,
//! **one source at a time**: a consent card (its sheet: what she reads,
//! what she never does there, what leaves the machine, the notebook
//! suggested by the account's domain) for each kind of each account that
//! has no decision yet. "Autoriser" writes the grant in the consent book,
//! in the notebook chosen on the card, and her source reads it at once;
//! "Pas maintenant" writes nothing and goes on to the next. Then
//! « Dis-moi sur quoi tu travailles et avec qui. », and the first run is
//! over (her memory's `meta` `first-run`), never asked again.
//!
//! A granted card keeps "Ne plus lire": a revocation, and her source
//! forgets what came from it at once. The Settings write the same book
//! (Settings › Agents): a card granted or revoked there says so at her
//! next cycle.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use hocus_secretary_link::{
    Action, Body, Card, CardKind, Decision, Notebook,
    consent::{ConsentBook, SourceKey, SourceKind},
    goa::GoogleAccount,
    sheets,
};

use super::Brain;
use crate::audit;

/// Her memory's `meta` key of the first run: `done` once over.
const FIRST_RUN: &str = "first-run";
/// Her memory's `meta` key of the consent cards in her thread, by item.
const CARDS: &str = "consent-cards";
/// Her Google source's name.
const GOOGLE: &str = "google";

/// Where the first run is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum FirstRun {
    /// Not begun: she waits for a shell and her accounts.
    #[default]
    Waiting,
    /// Her consent card `item` waits for the user.
    Asking(u64),
    Done,
}

/// A consent card: its source, and the account's e-mail.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Asked {
    pub source: SourceKey,
    pub identity: String,
}

#[derive(Debug, Default)]
pub(super) struct Consents {
    /// The consent book's file: none, no consent from the drawer.
    pub book: Option<PathBuf>,
    /// The Google accounts, as her source last found them; none before
    /// it did (or without one).
    pub accounts: Option<Vec<GoogleAccount>>,
    /// The consent cards of her thread.
    pub cards: HashMap<u64, Asked>,
    /// "Pas maintenant" this run.
    pub later: HashSet<SourceKey>,
    pub first_run: FirstRun,
}

impl Brain {
    /// Her first run's state, and her consent cards, as her memory kept
    /// them.
    pub(super) fn consents_restored(&mut self) {
        if self.store.meta(FIRST_RUN).ok().flatten().as_deref() == Some("done") {
            self.consents.first_run = FirstRun::Done;
        }
        if let Some(cards) = self
            .store
            .meta(CARDS)
            .ok()
            .flatten()
            .and_then(|c| serde_json::from_str::<Vec<(u64, Asked)>>(&c).ok())
        {
            let kept: HashSet<u64> = self.talk.items.iter().map(|i| i.id).collect();
            self.consents.cards = cards
                .into_iter()
                .filter(|(i, _)| kept.contains(i))
                .collect();
        }
        // A card still waiting is where the first run is.
        if self.consents.first_run == FirstRun::Waiting
            && let Some(waiting) = self
                .consents
                .cards
                .keys()
                .copied()
                .filter(|i| self.card_waits(*i))
                .max()
        {
            self.consents.first_run = FirstRun::Asking(waiting);
        }
    }

    fn card_waits(&self, item: u64) -> bool {
        self.talk.items.iter().any(|i| {
            i.id == item
                && matches!(&i.body, Body::Card { card } if card.decided.is_none() && card.outcome.is_none())
        })
    }

    fn keep_cards(&self) {
        let cards: Vec<(&u64, &Asked)> = self.consents.cards.iter().collect();
        let written = serde_json::to_string(&cards)
            .map_err(|e| e.to_string())
            .and_then(|c| self.store.set_meta(CARDS, &c).map_err(|e| e.to_string()));
        if let Err(err) = written {
            tracing::warn!(%err, "could not keep her consent cards");
        }
    }

    fn book(&self) -> ConsentBook {
        self.consents
            .book
            .as_deref()
            .map(ConsentBook::load)
            .unwrap_or_default()
    }

    /// Her Google source found these accounts: the cards follow the book,
    /// and the first run goes on.
    pub(super) fn accounts_found(&mut self, accounts: Vec<GoogleAccount>) {
        self.consents.accounts = Some(accounts);
        self.cards_follow_the_book();
        self.first_run_step();
    }

    /// Her consent book changed (the Settings wrote it, #752): her cards
    /// say it now, her Google source reads it now, her first run goes on.
    pub(super) fn consents_changed(&mut self) {
        tracing::info!("her consent book changed");
        self.cards_follow_the_book();
        self.google_now();
        self.first_run_step();
        self.tell_look();
    }

    /// Her Google source reads its book at once.
    fn google_now(&mut self) {
        let now = self.clock.now();
        if let Some(slot) = self.slots.iter_mut().find(|s| s.name == GOOGLE) {
            if slot.busy {
                slot.again = true;
            } else {
                slot.due = slot.due.min(now);
            }
        }
    }

    /// The next step of her first run, when there is one to take: a
    /// shell is there, and she knows her accounts.
    pub(super) fn first_run_step(&mut self) {
        if !matches!(self.consents.first_run, FirstRun::Waiting) {
            return;
        }
        let Some(accounts) = self.consents.accounts.clone() else {
            return;
        };
        if !self.conns.keys().any(|c| self.talker(*c)) {
            return;
        }
        let fr = self.talk.fr();
        let begun = !self.consents.cards.is_empty()
            || self.store.meta(FIRST_RUN).ok().flatten().as_deref() == Some("begun");
        if !begun {
            // Her name, when the user gave her one (#752).
            let name = self.settings.current().name.trim().to_owned();
            let me = match (name.is_empty(), fr) {
                (true, true) => "ta secrétaire".to_owned(),
                (true, false) => "your secretary".to_owned(),
                (false, true) => format!("{name}, ta secrétaire"),
                (false, false) => format!("{name}, your secretary"),
            };
            let hello = match (accounts.is_empty(), fr) {
                (false, true) => format!(
                    "Bonjour, je suis {me}. Je ne lis rien sans ton accord : je te demande, une source à la fois."
                ),
                (false, false) => format!(
                    "Hello, I’m {me}. I read nothing without your consent: I’ll ask you, one source at a time."
                ),
                (true, true) => format!(
                    "Bonjour, je suis {me}. Je n’ai trouvé aucun compte Google dans Comptes en ligne : quand tu en ajouteras un, tu pourras me l’ouvrir dans Réglages › Agents."
                ),
                (true, false) => format!(
                    "Hello, I’m {me}. I found no Google account in Online Accounts: once you add one, you can open it to me in Settings › Agents."
                ),
            };
            self.add_item(
                None,
                Body::Her {
                    text: hello,
                    note: None,
                    assumed: false,
                },
            );
            if let Err(err) = self.store.set_meta(FIRST_RUN, "begun") {
                tracing::warn!(%err, "could not note her first run");
            }
        }
        let book = self.book();
        let next = accounts.iter().find_map(|account| {
            account.kinds().into_iter().find_map(|kind| {
                let source = SourceKey::new(&account.id, kind);
                let undecided = !book.records().iter().any(|r| r.source == source);
                (undecided && !self.consents.later.contains(&source))
                    .then(|| (account.clone(), kind))
            })
        });
        match next {
            Some((account, kind)) => {
                let item = self.consent_card(&account, kind);
                self.consents.first_run = FirstRun::Asking(item);
            }
            None => {
                let ask = self.talk.t(
                    "Dis-moi sur quoi tu travailles et avec qui.",
                    "Tell me what you work on, and with whom.",
                );
                self.add_item(
                    None,
                    Body::Her {
                        text: ask.to_owned(),
                        note: None,
                        assumed: false,
                    },
                );
                self.consents.first_run = FirstRun::Done;
                if let Err(err) = self.store.set_meta(FIRST_RUN, "done") {
                    tracing::warn!(%err, "could not note her first run over");
                }
                tracing::info!("her first run is over");
            }
        }
        self.tell_look();
    }

    /// A consent card for `account`'s `kind`, in its suggested notebook.
    fn consent_card(&mut self, account: &GoogleAccount, kind: SourceKind) -> u64 {
        let fr = self.talk.fr();
        let sheet = sheets::google(&account.identity, kind, account.suggested, fr);
        let card = Card {
            kind: CardKind::Consent,
            label: format!("{} · {}", sheets::kind_name(kind, fr), account.identity),
            text: String::new(),
            lines: Vec::new(),
            actions: vec![
                Action {
                    label: self.talk.t("Autoriser", "Allow").to_owned(),
                    decision: Decision::Accept,
                },
                Action {
                    label: self.talk.t("Pas maintenant", "Not now").to_owned(),
                    decision: Decision::Reject,
                },
            ],
            decided: None,
            outcome: None,
            sheet: Some(sheet),
        };
        let item = self.add_item(None, Body::Card { card });
        tracing::info!(
            item,
            account = account.id,
            kind = kind.as_str(),
            "she asks to read a source"
        );
        self.consents.cards.insert(
            item,
            Asked {
                source: SourceKey::new(&account.id, kind),
                identity: account.identity.clone(),
            },
        );
        self.keep_cards();
        item
    }

    /// A consent card's button (`true`: it was one). "Autoriser" writes
    /// the grant into `notebook`; "Pas maintenant" nothing; "Ne plus
    /// lire" a revocation.
    pub(super) fn consent_decide(
        &mut self,
        conn: u64,
        id: u64,
        item: u64,
        decision: Decision,
        notebook: Option<Notebook>,
    ) -> bool {
        let Some(asked) = self.consents.cards.get(&item).cloned() else {
            return false;
        };
        let Some(path) = self.consents.book.clone() else {
            self.error(conn, Some(id), "she has no consent book");
            return true;
        };
        let mut book = ConsentBook::load(&path);
        let granted = book.grant(&asked.source);
        let fr = self.talk.fr();
        let now = self.clock.now();
        let what = what_she_reads(asked.source.kind, fr);
        let (outcome, actions, decided, wrote) = match (decision, granted) {
            (Decision::Accept, None) => {
                let notebook = notebook
                    .or_else(|| self.card_notebook(item))
                    .unwrap_or_default();
                book.allow(asked.source.clone(), &asked.identity, notebook, now);
                let said = if fr {
                    format!("Je lis {what} · carnet {}", notebook.word())
                } else {
                    format!("I read {what} · {} notebook", notebook.word())
                };
                (said, vec![self.stop_reading()], None, true)
            }
            (Decision::Reject, Some(_)) => {
                book.revoke(&asked.source, now);
                let said = if fr {
                    format!("Je ne lis plus {what}, et j’oublie ce que j’en savais.")
                } else {
                    format!("I no longer read {what}, and forget what I knew of it.")
                };
                (said, Vec::new(), Some(Decision::Reject), true)
            }
            (Decision::Reject, None) => {
                self.consents.later.insert(asked.source.clone());
                let said = self
                    .talk
                    .t(
                        "Pas maintenant : tu pourras me l’ouvrir dans Réglages › Agents.",
                        "Not now: you can open it to me in Settings › Agents.",
                    )
                    .to_owned();
                (said, Vec::new(), Some(Decision::Reject), false)
            }
            _ => {
                self.error(conn, Some(id), "no such button");
                return true;
            }
        };
        if wrote && let Err(err) = book.save(&path) {
            tracing::warn!(%err, "the consent is not written");
            self.error(conn, Some(id), format!("the consent is not written: {err}"));
            return true;
        }
        tracing::info!(item, source = %asked.source, ?decision, "a source's consent from the drawer");
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "consent".to_owned(),
            ids: vec![asked.source.to_string()],
            outcome: Some(
                match decision {
                    Decision::Accept => "granted",
                    _ if wrote => "revoked",
                    _ => "later",
                }
                .to_owned(),
            ),
            ..Default::default()
        });
        self.change_item(item, |body| {
            if let Body::Card { card } = body {
                card.outcome = Some(outcome);
                card.actions = actions;
                card.decided = decided;
            }
        });
        if wrote {
            self.google_now();
        }
        if self.consents.first_run == FirstRun::Asking(item) {
            self.consents.first_run = FirstRun::Waiting;
            self.first_run_step();
        }
        self.tell_look();
        self.done(conn, id);
        true
    }

    /// "Ne plus lire", once granted.
    fn stop_reading(&self) -> Action {
        Action {
            label: self.talk.t("Ne plus lire", "Stop reading").to_owned(),
            decision: Decision::Reject,
        }
    }

    fn card_notebook(&self, item: u64) -> Option<Notebook> {
        self.talk.items.iter().find_map(|i| match &i.body {
            Body::Card { card } if i.id == item => card.sheet.as_ref().map(|s| s.notebook),
            _ => None,
        })
    }

    /// Her cards say what the book says now (a consent given or taken
    /// back in the Settings).
    fn cards_follow_the_book(&mut self) {
        if self.consents.cards.is_empty() {
            return;
        }
        let book = self.book();
        let fr = self.talk.fr();
        let cards: Vec<(u64, Asked)> = self
            .consents
            .cards
            .iter()
            .map(|(i, a)| (*i, a.clone()))
            .collect();
        for (item, asked) in cards {
            let Some(Body::Card { card }) = self
                .talk
                .items
                .iter()
                .find(|i| i.id == item)
                .map(|i| i.body.clone())
            else {
                continue;
            };
            let granted = book.grant(&asked.source);
            let shown_granted = card.actions.iter().any(|a| a.decision == Decision::Reject)
                && card.outcome.is_some()
                && card.decided.is_none();
            let what = what_she_reads(asked.source.kind, fr);
            match granted {
                Some(grant) if !shown_granted => {
                    let said = if fr {
                        format!(
                            "Je lis {what} · carnet {} (autorisé dans les Réglages)",
                            grant.notebook().word()
                        )
                    } else {
                        format!(
                            "I read {what} · {} notebook (allowed in Settings)",
                            grant.notebook().word()
                        )
                    };
                    let stop = self.stop_reading();
                    self.change_item(item, |body| {
                        if let Body::Card { card } = body {
                            card.outcome = Some(said);
                            card.actions = vec![stop];
                            card.decided = None;
                        }
                    });
                    if self.consents.first_run == FirstRun::Asking(item) {
                        self.consents.first_run = FirstRun::Waiting;
                    }
                }
                None if shown_granted => {
                    let said = if fr {
                        format!("Je ne lis plus {what} (arrêté dans les Réglages).")
                    } else {
                        format!("I no longer read {what} (stopped in Settings).")
                    };
                    self.change_item(item, |body| {
                        if let Body::Card { card } = body {
                            card.outcome = Some(said);
                            card.actions = Vec::new();
                            card.decided = Some(Decision::Reject);
                        }
                    });
                }
                _ => {}
            }
        }
    }
}

/// What she reads of a kind, in a sentence: "ton agenda".
fn what_she_reads(kind: SourceKind, fr: bool) -> &'static str {
    match (kind, fr) {
        (SourceKind::Calendar, true) => "ton agenda",
        (SourceKind::Calendar, false) => "your calendar",
        (SourceKind::Mail, true) => "tes mails",
        (SourceKind::Mail, false) => "your mail",
        (SourceKind::Birthdays, true) => "les anniversaires de tes contacts",
        (SourceKind::Birthdays, false) => "your contacts’ birthdays",
        (SourceKind::Notifications, true) => "ses notifications",
        (SourceKind::Notifications, false) => "its notifications",
    }
}
