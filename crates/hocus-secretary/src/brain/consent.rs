//! Her sources' consent in the drawer, and her first run (§10.1, §15,
//! #750).
//!
//! Her Google source says, at each of its cycles, the accounts Online
//! Accounts has ([`super::Synced::found`]). The first time a shell (or a
//! test) is there once she knows them, she introduces herself and shows
//! her first run's three steps (#758, [`Body::Steps`]): her key, what she
//! may read of each Google account, what the user works on. Her key
//! first: while she misses one, nothing else is asked. Then **one card
//! per account** (its sheet: a switch per kind, the calendar and the mail
//! on, « Travail | Privé » suggested by the account's domain, what she
//! reads and what leaves the machine folded under a link), for each
//! account with a kind not decided yet. « Autoriser l'agenda et les
//! mails » writes a grant for each kind switched on, in the notebook
//! chosen, and her source reads it at once; the kinds switched off, and
//! "Pas maintenant", write nothing and go on to the next account. Then
//! « Dis-moi sur quoi tu travailles, et avec qui. », and the first run is
//! over (her memory's `meta` `first-run`), never asked again.
//!
//! A granted card keeps "Ne plus lire": a revocation of what it granted,
//! and her source forgets what came from it at once. The Settings write the same book
//! (Settings › Agents): a card granted or revoked there says so at her
//! next cycle.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use hocus_secretary_link::{
    Action, Body, Card, CardKind, Decision, Notebook,
    calendars::{CALENDARS, CalendarInfo, Calendars},
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
/// Her memory's `meta` key of the accounts she knows of, once her first
/// run is over: one found after is asked about (#752).
const KNOWN: &str = "accounts-known";
/// Her memory's `meta` key of each source's notebook as last filed.
const NOTEBOOKS: &str = "source-notebooks";
/// Her memory's `meta` key of her first run's steps item, and whether it
/// shows her key's step (#758).
const STEPS: &str = "first-run-steps";

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

/// A consent card: its source (an account's first kind), the account's
/// e-mail, and the kinds it asks for (#758; none: its source's only, a
/// card of before).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Asked {
    pub source: SourceKey,
    pub identity: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<SourceKind>,
}

impl Asked {
    fn kinds(&self) -> Vec<SourceKind> {
        if self.kinds.is_empty() {
            vec![self.source.kind]
        } else {
            self.kinds.clone()
        }
    }

    fn sources(&self) -> Vec<SourceKey> {
        self.kinds()
            .into_iter()
            .map(|k| SourceKey::new(&self.source.account, k))
            .collect()
    }
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
    /// Each account's calendars, as her source last listed them (#752).
    pub calendars: std::collections::BTreeMap<String, Vec<CalendarInfo>>,
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

    /// The kinds of her Google accounts she may read now.
    pub(super) fn granted_kinds(&self) -> Vec<SourceKind> {
        self.book()
            .granted()
            .iter()
            .map(|g| g.source().kind)
            .collect()
    }

    /// `account`'s kinds still to ask about: no decision in the book, not
    /// put off this run.
    fn undecided(&self, book: &ConsentBook, account: &GoogleAccount) -> Vec<SourceKind> {
        account
            .kinds()
            .into_iter()
            .filter(|kind| {
                let source = SourceKey::new(&account.id, *kind);
                !book.records().iter().any(|r| r.source == source)
                    && !self.consents.later.contains(&source)
            })
            .collect()
    }

    /// Her Google source listed these accounts' calendars (#752): kept in
    /// her folder for the Settings' Agenda rows, shown on an Agenda's
    /// card still waiting, and those she could not read said in her
    /// status.
    pub(super) fn calendars_found(
        &mut self,
        found: std::collections::BTreeMap<String, Vec<CalendarInfo>>,
    ) {
        if self.consents.calendars == found {
            return;
        }
        self.consents.calendars = found;
        if let Some(path) = self
            .consents
            .book
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(|d| d.join(CALENDARS))
            && let Err(err) = Calendars(self.consents.calendars.clone()).save(&path)
        {
            tracing::warn!(%err, "could not keep the calendars' list");
        }
        let cards: Vec<(u64, Asked)> = self
            .consents
            .cards
            .iter()
            .filter(|(_, a)| a.kinds().contains(&SourceKind::Calendar))
            .map(|(i, a)| (*i, a.clone()))
            .collect();
        for (item, asked) in cards {
            let list = self
                .consents
                .calendars
                .get(&asked.source.account)
                .cloned()
                .unwrap_or_default();
            self.change_item(item, |body| {
                if let Body::Card { card } = body
                    && let Some(sheet) = &mut card.sheet
                {
                    sheet.calendars = list;
                }
            });
        }
        self.tell_look();
    }

    /// The calendars she could not read at her last cycle.
    pub(super) fn unreadable_calendars(&self) -> Vec<&CalendarInfo> {
        let book = self.book();
        self.consents
            .calendars
            .iter()
            .filter(|(account, _)| {
                book.grant(&SourceKey::new(account.as_str(), SourceKind::Calendar))
                    .is_some()
            })
            .flat_map(|(_, list)| list)
            .filter(|c| c.unreadable.is_some())
            .collect()
    }

    /// Her Google source found these accounts: the cards follow the book,
    /// and the first run goes on.
    pub(super) fn accounts_found(&mut self, accounts: Vec<GoogleAccount>) {
        self.consents.accounts = Some(accounts);
        self.cards_follow_the_book();
        self.first_run_step();
        self.new_accounts_step();
    }

    /// An account added since her first run (#752): she says so, then
    /// asks for each of its kinds, one source at a time, as at her first
    /// run. The accounts there at its end are known from then on.
    pub(super) fn new_accounts_step(&mut self) {
        if self.consents.first_run != FirstRun::Done {
            return;
        }
        let Some(accounts) = self.consents.accounts.clone() else {
            return;
        };
        let mut known: Vec<String> = match self.store.meta(KNOWN).ok().flatten() {
            Some(text) => serde_json::from_str(&text).unwrap_or_default(),
            None => {
                // Her first run just ended: these were asked about there.
                let all: Vec<String> = accounts.iter().map(|a| a.id.clone()).collect();
                self.keep_known(&all);
                all
            }
        };
        if !self.conns.keys().any(|c| self.talker(*c)) {
            return;
        }
        let waiting = self.consents.cards.keys().any(|i| self.card_waits(*i));
        if waiting {
            return;
        }
        let book = self.book();
        let new: Vec<&GoogleAccount> = accounts.iter().filter(|a| !known.contains(&a.id)).collect();
        for account in new {
            let kinds = self.undecided(&book, account);
            if kinds.is_empty() {
                known.push(account.id.clone());
                self.keep_known(&known);
                continue;
            }
            let said = if self.talk.fr() {
                format!(
                    "J’ai trouvé un nouveau compte Google : {}. Je ne lis rien sans ton accord.",
                    account.identity
                )
            } else {
                format!(
                    "I found a new Google account: {}. I read nothing without your consent.",
                    account.identity
                )
            };
            self.add_item(
                None,
                Body::Her {
                    text: said,
                    note: None,
                    assumed: false,
                },
            );
            tracing::info!(account = account.id, "a new account: she asks for it");
            self.consent_card(account, &kinds, None, None);
            self.tell_look();
            return;
        }
    }

    fn keep_known(&self, known: &[String]) {
        let written = serde_json::to_string(known)
            .map_err(|e| e.to_string())
            .and_then(|t| self.store.set_meta(KNOWN, &t).map_err(|e| e.to_string()));
        if let Err(err) = written {
            tracing::warn!(%err, "could not keep the accounts known");
        }
    }

    /// Her consent book changed (the Settings wrote it, #752): her cards
    /// say it now, her Google source reads it now, her first run goes on.
    pub(super) fn consents_changed(&mut self) {
        tracing::info!("her consent book changed");
        self.refile();
        self.cards_follow_the_book();
        self.google_now();
        self.first_run_step();
        self.new_accounts_step();
        self.tell_look();
    }

    /// A source's notebook changed in the book (« C'est privé » on its
    /// row, #752): what came from it moves with it, in her memory and
    /// what she holds. Each source's notebook as filed is kept in her
    /// memory, so that a change made while she was away is taken at her
    /// start.
    pub(super) fn refile(&mut self) {
        if self.consents.book.is_none() {
            return;
        }
        let mut filed: std::collections::BTreeMap<String, Notebook> = self
            .store
            .meta(NOTEBOOKS)
            .ok()
            .flatten()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut changed = false;
        for grant in self.book().granted() {
            let source = grant.source().to_string();
            let notebook = grant.notebook();
            match filed.insert(source.clone(), notebook) {
                Some(before) if before != notebook => {
                    match self.store.refile_source(&source, notebook) {
                        Ok(moved) => tracing::info!(
                            source,
                            notebook = notebook.word(),
                            moved,
                            "a source's notebook changed: what came from it moves"
                        ),
                        Err(err) => tracing::warn!(source, %err, "could not move a source's items"),
                    }
                    self.inbox.refile_source(&source, notebook);
                    changed = true;
                }
                Some(_) => {}
                None => changed = true,
            }
        }
        if changed {
            let kept = serde_json::to_string(&filed)
                .map_err(|e| e.to_string())
                .and_then(|t| {
                    self.store
                        .set_meta(NOTEBOOKS, &t)
                        .map_err(|e| e.to_string())
                });
            if let Err(err) = kept {
                tracing::warn!(%err, "could not keep the sources' notebooks");
            }
        }
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
    /// shell is there, and she knows her accounts; her key first.
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
        // Whether her key is missing is being asked of the keyring.
        if self.key_asked {
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
                    "Bonjour, je suis {me}. Je lis ce que tu m’autorises, je te fais le point le matin, je te rappelle ce qui compte. Je ne fais rien sans ton clic."
                ),
                (false, false) => format!(
                    "Hello, I’m {me}. I read what you allow me to, brief you in the morning, remind you of what matters. I do nothing without your click."
                ),
                (true, true) => format!(
                    "Bonjour, je suis {me}. Je n’ai trouvé aucun compte Google sur cet ordinateur : quand tu en ajouteras un, tu pourras me l’ouvrir dans Réglages › Agents."
                ),
                (true, false) => format!(
                    "Hello, I’m {me}. I found no Google account on this computer: once you add one, you can open it to me in Settings › Agents."
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
            // Her steps under her hello, before anything is asked.
            self.steps(Some(usize::from(self.key_missing.is_none())));
        }
        // Her key first: nothing else is asked while she misses it.
        if self.key_missing.is_some() {
            self.steps(Some(0));
            self.tell_look();
            return;
        }
        let book = self.book();
        let asking: Vec<(GoogleAccount, Vec<SourceKind>)> = accounts
            .iter()
            .map(|a| (a.clone(), self.undecided(&book, a)))
            .filter(|(_, kinds)| !kinds.is_empty())
            .collect();
        match asking.first() {
            Some((account, kinds)) => {
                let total = accounts.len();
                let at = accounts
                    .iter()
                    .position(|a| a.id == account.id)
                    .unwrap_or(0)
                    + 1;
                let then = asking.get(1).map(|(a, _)| a.clone());
                let item = self.consent_card(account, kinds, Some((at, total)), then.as_ref());
                self.consents.first_run = FirstRun::Asking(item);
                self.steps(Some(1));
            }
            None => {
                let ask = self.talk.t(
                    "Dis-moi sur quoi tu travailles, et avec qui.",
                    "Tell me what you work on, and with whom.",
                );
                self.steps(Some(2));
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

    /// Her first run is under way: begun, her accounts known, not over.
    pub(super) fn first_run_under_way(&self) -> bool {
        self.consents.accounts.is_some()
            && self.consents.first_run != FirstRun::Done
            && self.conns.keys().any(|c| self.talker(*c))
    }

    /// Her first run's steps (#758), step `now` the one being taken (none:
    /// all done), added once and changed in place after. The key's step
    /// shows only when she missed one.
    pub(super) fn steps(&mut self, now: Option<usize>) {
        use hocus_secretary_link::{Step, StepState};
        let fr = self.talk.fr();
        let kept: Option<(u64, bool)> = self
            .store
            .meta(STEPS)
            .ok()
            .flatten()
            .and_then(|t| serde_json::from_str(&t).ok());
        let shown = kept.filter(|(item, _)| self.talk.items.iter().any(|i| i.id == *item));
        let with_key = shown.map_or(self.key_missing.is_some(), |(_, k)| k);
        let accounts = self.consents.accounts.as_ref().map_or(0, Vec::len);
        let asking = match self.consents.first_run {
            FirstRun::Asking(item) => self.consents.cards.get(&item).and_then(|a| {
                self.consents
                    .accounts
                    .as_ref()?
                    .iter()
                    .position(|g| g.id == a.source.account)
            }),
            _ => None,
        };
        let state = |n: usize| match now {
            Some(at) if n < at => StepState::Done,
            Some(at) if n == at => StepState::Now,
            Some(_) => StepState::Later,
            None => StepState::Done,
        };
        let mut steps = Vec::new();
        if with_key {
            let st = state(0);
            steps.push(Step {
                title: self
                    .talk
                    .t("Ma clé Anthropic", "My Anthropic key")
                    .to_owned(),
                note: match st {
                    StepState::Done => self.talk.t("enregistrée", "saved"),
                    StepState::Now => self.talk.t("maintenant, ci-dessus", "now, above"),
                    StepState::Later => "",
                }
                .to_owned(),
                state: st,
            });
        }
        let st = state(1);
        steps.push(Step {
            title: self
                .talk
                .t(
                    "Ce que je peux lire de tes comptes Google",
                    "What I may read of your Google accounts",
                )
                .to_owned(),
            note: match (st, asking, fr) {
                (StepState::Now, Some(at), true) => format!("{} sur {accounts}", at + 1),
                (StepState::Now, Some(at), false) => format!("{} of {accounts}", at + 1),
                (StepState::Done, _, _) => String::new(),
                (_, _, true) if accounts > 1 => format!("{accounts} comptes"),
                (_, _, false) if accounts > 1 => format!("{accounts} accounts"),
                (_, _, true) if accounts == 1 => "1 compte".to_owned(),
                (_, _, false) if accounts == 1 => "1 account".to_owned(),
                _ => String::new(),
            },
            state: st,
        });
        steps.push(Step {
            title: self
                .talk
                .t(
                    "Sur quoi tu travailles, et avec qui",
                    "What you work on, and with whom",
                )
                .to_owned(),
            note: String::new(),
            state: state(2),
        });
        let body = Body::Steps { steps };
        match shown {
            Some((item, _)) => self.change_item(item, |b| *b = body),
            None => {
                let item = self.add_item(None, body);
                let written = serde_json::to_string(&(item, with_key))
                    .map_err(|e| e.to_string())
                    .and_then(|t| self.store.set_meta(STEPS, &t).map_err(|e| e.to_string()));
                if let Err(err) = written {
                    tracing::warn!(%err, "could not keep her first run's steps");
                }
            }
        }
    }

    /// Her first run's last step answered: the user told her something.
    pub(super) fn steps_answered(&mut self) {
        if self.consents.first_run != FirstRun::Done {
            return;
        }
        let shown = self
            .store
            .meta(STEPS)
            .ok()
            .flatten()
            .and_then(|t| serde_json::from_str::<(u64, bool)>(&t).ok())
            .map(|(item, _)| item);
        let waits = shown.is_some_and(|item| {
            self.talk.items.iter().any(|i| {
                i.id == item
                    && matches!(&i.body, Body::Steps { steps } if steps.iter().any(|s| s.state != hocus_secretary_link::StepState::Done))
            })
        });
        if waits {
            self.steps(None);
        }
    }

    /// A consent card for `account`'s `kinds` (#758: one card per
    /// account), in its suggested notebook; `at`, which account of how
    /// many; `then`, the account asked about next.
    fn consent_card(
        &mut self,
        account: &GoogleAccount,
        kinds: &[SourceKind],
        at: Option<(usize, usize)>,
        then: Option<&GoogleAccount>,
    ) -> u64 {
        let fr = self.talk.fr();
        let quiet = self.settings.current().speak.restraint().perso;
        let mut sheet = sheets::account(&account.identity, kinds, account.suggested, quiet, fr);
        if kinds.contains(&SourceKind::Calendar) {
            sheet.calendars = self
                .consents
                .calendars
                .get(&account.id)
                .cloned()
                .unwrap_or_default();
        }
        sheet.then = then.map(|next| sheets::Then {
            identity: next.identity.clone(),
            note: match (next.suggested, fr) {
                (Notebook::Perso, true) => "ensuite · je te propose « Privé »".to_owned(),
                (Notebook::Pro, true) => "ensuite · je te propose « Travail »".to_owned(),
                (Notebook::Perso, false) => "next · I suggest “Private”".to_owned(),
                (Notebook::Pro, false) => "next · I suggest “Work”".to_owned(),
            },
        });
        let on: Vec<SourceKind> = sheet
            .reads
            .iter()
            .filter(|r| r.on)
            .map(|r| r.kind)
            .collect();
        let label = match (at, fr) {
            (Some((n, total)), true) if total > 1 => format!("Ton compte Google · {n} sur {total}"),
            (Some((n, total)), false) if total > 1 => {
                format!("Your Google account · {n} of {total}")
            }
            (_, true) => "Ton compte Google".to_owned(),
            (_, false) => "Your Google account".to_owned(),
        };
        let card = Card {
            kind: CardKind::Consent,
            label,
            text: String::new(),
            lines: Vec::new(),
            actions: vec![
                Action {
                    label: sheets::allow_label(&on, fr),
                    decision: Decision::Accept,
                },
                Action {
                    label: self.talk.t("Pas maintenant", "Not now").to_owned(),
                    decision: Decision::Reject,
                },
            ],
            decided: None,
            outcome: None,
            sheet: Some(Box::new(sheet)),
        };
        let item = self.add_item(None, Body::Card { card });
        tracing::info!(
            item,
            account = account.id,
            kinds = kinds.len(),
            "she asks to read an account"
        );
        let first = kinds.first().copied().unwrap_or(SourceKind::Calendar);
        self.consents.cards.insert(
            item,
            Asked {
                source: SourceKey::new(&account.id, first),
                identity: account.identity.clone(),
                kinds: kinds.to_vec(),
            },
        );
        self.keep_cards();
        item
    }

    /// A consent card's button (`true`: it was one). "Autoriser" writes
    /// a grant for each kind switched on (none in `off`) into `notebook`;
    /// "Pas maintenant" nothing; "Ne plus lire" a revocation of what it
    /// granted.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn consent_decide(
        &mut self,
        conn: u64,
        id: u64,
        item: u64,
        decision: Decision,
        notebook: Option<Notebook>,
        left_out: Vec<String>,
        off: Vec<SourceKind>,
    ) -> bool {
        let Some(asked) = self.consents.cards.get(&item).cloned() else {
            return false;
        };
        let Some(path) = self.consents.book.clone() else {
            self.error(conn, Some(id), "she has no consent book");
            return true;
        };
        let mut book = ConsentBook::load(&path);
        let sources = asked.sources();
        let granted: Vec<SourceKey> = sources
            .iter()
            .filter(|s| book.grant(s).is_some())
            .cloned()
            .collect();
        let fr = self.talk.fr();
        let now = self.clock.now();
        let on: Vec<SourceKey> = sources
            .iter()
            .filter(|s| !off.contains(&s.kind))
            .cloned()
            .collect();
        let decision = if decision == Decision::Accept && on.is_empty() {
            Decision::Reject
        } else {
            decision
        };
        let (outcome, actions, decided, wrote) = match (decision, granted.is_empty()) {
            (Decision::Accept, true) => {
                let notebook = notebook
                    .or_else(|| self.card_notebook(item))
                    .unwrap_or_default();
                for source in &on {
                    let left = if source.kind == SourceKind::Calendar {
                        left_out.clone()
                    } else {
                        Vec::new()
                    };
                    book.allow_leaving_out(source.clone(), &asked.identity, notebook, left, now);
                }
                for source in sources.iter().filter(|s| off.contains(&s.kind)) {
                    self.consents.later.insert(source.clone());
                }
                let kinds: Vec<SourceKind> = on.iter().map(|s| s.kind).collect();
                let said = format!(
                    "{} {} · {}",
                    self.talk.t("Je lis", "I read"),
                    what_she_reads_all(&kinds, fr),
                    notebook.said(fr)
                );
                (said, vec![self.stop_reading()], None, true)
            }
            (Decision::Reject, false) => {
                for source in &granted {
                    book.revoke(source, now);
                }
                let kinds: Vec<SourceKind> = granted.iter().map(|s| s.kind).collect();
                let what = what_she_reads_all(&kinds, fr);
                let said = if fr {
                    format!("Je ne lis plus {what}, et j’oublie ce que j’en savais.")
                } else {
                    format!("I no longer read {what}, and forget what I knew of it.")
                };
                (said, Vec::new(), Some(Decision::Reject), true)
            }
            (Decision::Reject, true) => {
                for source in &sources {
                    self.consents.later.insert(source.clone());
                }
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
        tracing::info!(item, account = %asked.source.account, ?decision, off = off.len(), "an account's consent from the drawer");
        self.audited(audit::Entry {
            kind: audit::Kind::Answer,
            what: "consent".to_owned(),
            ids: sources.iter().map(ToString::to_string).collect(),
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
        self.new_accounts_step();
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
            let grants: Vec<_> = asked
                .sources()
                .iter()
                .filter_map(|s| book.grant(s))
                .collect();
            let shown_granted = card.actions.iter().any(|a| a.decision == Decision::Reject)
                && card.outcome.is_some()
                && card.decided.is_none();
            let kinds: Vec<SourceKind> = if grants.is_empty() {
                asked.kinds()
            } else {
                grants.iter().map(|g| g.source().kind).collect()
            };
            let what = what_she_reads_all(&kinds, fr);
            match grants.first() {
                Some(grant) if !shown_granted => {
                    let said = if fr {
                        format!(
                            "Je lis {what} · {} (autorisé dans les Réglages)",
                            grant.notebook().said(true)
                        )
                    } else {
                        format!(
                            "I read {what} · {} (allowed in Settings)",
                            grant.notebook().said(false)
                        )
                    };
                    let stop = self.stop_reading();
                    // The account answered in the Settings: its other
                    // kinds are not asked again this run.
                    for source in asked.sources() {
                        if book.grant(&source).is_none() {
                            self.consents.later.insert(source);
                        }
                    }
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

/// What she reads of `kinds`, in a sentence: "ton agenda et tes mails".
fn what_she_reads_all(kinds: &[SourceKind], fr: bool) -> String {
    let words: Vec<&str> = kinds.iter().map(|k| what_she_reads(*k, fr)).collect();
    let and = if fr { " et " } else { " and " };
    match words.as_slice() {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{}{and}{last}", rest.join(", ")),
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
