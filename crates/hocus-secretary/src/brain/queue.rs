//! Her triage queue shown, and senders set aside (#754).
//!
//! - **The view** ([`hocus_secretary_link::queue::View`]): asked by the
//!   drawer when her status line is clicked, sent again after each event
//!   that changes it, only while it is open, and only when it changed:
//!   nothing at rest, nothing once closed. It is made from what her loop
//!   already holds (the queue, the items held with their sender, subject
//!   and date): no read of her memory, no request.
//! - **Set aside**: a sender's rule (`learn::Compiled::Aside`) keeps its
//!   items out of the queue, those waiting and those to come; they keep
//!   their sender, subject and date (their episode), and their text stays
//!   held for the triage, checked only when the user asks about one (a
//!   question's retrieval names it: it goes first). The rules are read
//!   again after each line of a connection, so a rule taken back in "Ce
//!   qu'elle sait" brings its items back at once.
//! - **Her proposal**: a sender whose mails say they were sent by a
//!   machine (`hocus_sources::mail::automated`), with
//!   [`AUTOMATED_MOST`] of them waiting or more, is asked about once, as an
//!   observation (« Oui · Non · Plus tard »); one at a time.

use std::collections::BTreeMap;

use hocus_secretary_link::{
    FromSecretary, Notebook,
    knows::Act as KnownAct,
    queue::{Act, Group, View, Waiting},
};
use hocus_sources::Content;
use jiff::Timestamp;

use super::Brain;
use crate::{
    graph::Origin,
    learn::{self, Compiled, Noticed},
    triage::{Pause, State},
};

/// A sender with this many machine-sent mails waiting is asked about.
pub const AUTOMATED_MOST: usize = 3;
/// The view lists this many senders at most, the rest summed up.
const GROUPS_SHOWN: usize = 40;

/// The address in a `From` ("GitHub <notifications@github.com>"),
/// lowercase.
pub fn address_of(from: &str) -> String {
    from.rsplit_once('<')
        .map_or(from, |(_, a)| a.trim_end_matches('>'))
        .trim()
        .to_lowercase()
}

/// What groups an item in the view, how it is shown, whether it can be
/// set aside (a mail sender's), and why it looks automated.
struct Of {
    key: String,
    label: String,
    mail: bool,
    automated: Option<String>,
}

fn of(item: &hocus_sources::Item) -> Of {
    match &item.content {
        Content::Mail(mail) => {
            let address = address_of(&mail.from);
            Of {
                label: if address.is_empty() {
                    mail.from.clone()
                } else {
                    address.clone()
                },
                key: address,
                mail: true,
                automated: mail.automated.clone(),
            }
        }
        Content::Notification(n) => Of {
            key: format!("app:{}", n.app.to_lowercase()),
            label: n.app.clone(),
            mail: false,
            automated: None,
        },
        Content::Event(e) => Of {
            key: format!("calendar:{}", e.calendar),
            label: e.calendar_name.clone(),
            mail: false,
            automated: None,
        },
        Content::Birthday(_) => Of {
            key: "birthdays".to_owned(),
            label: "Anniversaires".to_owned(),
            mail: false,
            automated: None,
        },
    }
}

/// `seconds` as the user reads a span: "39 s", "4 min", "10 h 41".
pub fn span(seconds: u64, fr: bool) -> String {
    match seconds {
        s if s < 60 => format!("{s} s"),
        s if s < 3600 => format!("{} min", (s + 30) / 60),
        s => {
            let minutes = (s + 30) / 60;
            if fr {
                format!("{} h {:02}", minutes / 60, minutes % 60)
            } else {
                format!("{} h {:02} min", minutes / 60, minutes % 60)
            }
        }
    }
}

fn day_time(at: Timestamp, fr: bool) -> String {
    let zoned = at.to_zoned(jiff::tz::TimeZone::system());
    let time = format!("{:02}:{:02}", zoned.hour(), zoned.minute());
    if fr {
        format!("{} {time}", hocus_sources::mail::short_date(zoned.date()))
    } else {
        format!("{} {time}", zoned.strftime("%-d %b"))
    }
}

impl Brain {
    /// An item with its text, for the triage: queued, or set aside when
    /// a rule covers its sender.
    pub(super) fn enqueue(&mut self, pointer: String) {
        if self.triager.is_none() {
            return;
        }
        if self.covered(&pointer) {
            self.queue.put_aside(pointer);
        } else {
            self.queue.push(pointer);
        }
    }

    /// A rule sets `pointer`'s sender aside.
    fn covered(&self, pointer: &str) -> bool {
        if self.aside_rules.is_empty() {
            return false;
        }
        match self.inbox.get(pointer).map(|h| &h.redacted.item.content) {
            Some(Content::Mail(mail)) => {
                let address = address_of(&mail.from);
                self.aside_rules.iter().any(|s| learn::covers(s, &address))
            }
            _ => false,
        }
    }

    /// The rules read again: items of a sender newly set aside leave the
    /// queue; those of a rule taken back come back to it.
    pub(super) fn aside_again(&mut self) {
        let today = self.store.today();
        let rules = match learn::set_aside(&self.store, today) {
            Ok(rules) => rules,
            Err(err) => {
                tracing::warn!(%err, "the rules setting senders aside do not read");
                return;
            }
        };
        if rules == self.aside_rules {
            return;
        }
        self.aside_rules = rules;
        let covered: Vec<String> = self
            .queue
            .waiting()
            .chain(self.queue.set_aside())
            .filter(|p| self.covered(p))
            .cloned()
            .collect();
        let gone = self.queue.aside_where(|p| covered.iter().any(|c| c == p));
        let back = self.queue.back_where(|p| !covered.iter().any(|c| c == p));
        tracing::info!(
            rules = self.aside_rules.len(),
            gone,
            back,
            "the senders set aside"
        );
    }

    /// A question's request names `items`: those set aside or waiting are
    /// triaged first (#754).
    pub(super) fn asked_about(&mut self, items: &[String]) {
        let mut first = 0;
        for pointer in items.iter().rev() {
            if self.queue.is_aside(pointer) && self.queue.first(pointer) {
                first += 1;
            }
        }
        if first > 0 {
            tracing::info!(first, "items set aside, asked about: triaged first");
        }
    }

    /// A sender that looks automated, with enough of its mails waiting, is
    /// asked about once; one question at a time.
    pub(super) fn notice_automated(&mut self) {
        if self.triager.is_none() {
            return;
        }
        let seen = learn::load_seen(&self.store);
        let now = self.clock.now();
        if seen
            .iter()
            .any(|s| matches!(s.noticed, Noticed::Automated { .. }) && s.open(now))
        {
            return;
        }
        let mut senders: BTreeMap<String, (usize, String, Notebook)> = BTreeMap::new();
        for pointer in self.queue.waiting() {
            let Some(held) = self.inbox.get(pointer) else {
                continue;
            };
            let Content::Mail(mail) = &held.redacted.item.content else {
                continue;
            };
            let Some(why) = &mail.automated else {
                continue;
            };
            let entry = senders.entry(address_of(&mail.from)).or_insert((
                0,
                why.clone(),
                held.redacted.item.notebook,
            ));
            entry.0 += 1;
        }
        let asked: Vec<String> = seen.iter().map(|s| s.noticed.key()).collect();
        let most = senders
            .into_iter()
            .filter(|(sender, (count, _, _))| {
                *count >= AUTOMATED_MOST
                    && !sender.is_empty()
                    && !asked.contains(&format!("aside:{sender}"))
            })
            .max_by_key(|(_, (count, _, _))| *count);
        if let Some((sender, (count, why, notebook))) = most {
            self.notice(Noticed::Automated {
                sender,
                count: u32::try_from(count).unwrap_or(u32::MAX),
                why,
                notebook,
            });
        }
    }

    // --- The view ------------------------------------------------------------------------

    /// The view asked (`open`), or let go.
    pub(super) fn queue_open(&mut self, conn: u64, id: u64, open: bool) {
        if !open {
            self.queue_watch.remove(&conn);
            return;
        }
        let view = self.queue_view();
        self.queue_watch.insert(conn, (id, view.clone()));
        self.send(
            conn,
            &FromSecretary::Queue {
                id,
                view: Box::new(view),
            },
        );
    }

    /// The view sent again to those who have it open, when it changed.
    pub(super) fn queue_refresh(&mut self) {
        if self.queue_watch.is_empty() {
            return;
        }
        let view = self.queue_view();
        let changed: Vec<(u64, u64)> = self
            .queue_watch
            .iter()
            .filter(|(_, (_, last))| *last != view)
            .map(|(conn, (id, _))| (*conn, *id))
            .collect();
        for (conn, id) in changed {
            self.queue_watch.insert(conn, (id, view.clone()));
            self.send(
                conn,
                &FromSecretary::Queue {
                    id,
                    view: Box::new(view.clone()),
                },
            );
        }
    }

    /// Something done on the view: a sender set aside, or brought back.
    pub(super) fn queue_act(&mut self, conn: u64, id: u64, act: Act) {
        match act {
            Act::Aside { key } => self.set_sender_aside(&key),
            Act::Back { key } => self.bring_sender_back(&key),
        }
        self.aside_again();
        self.knows_refresh();
        let view = self.queue_view();
        self.queue_watch.insert(conn, (id, view.clone()));
        self.send(
            conn,
            &FromSecretary::Queue {
                id,
                view: Box::new(view),
            },
        );
    }

    /// "Ne pas trier, chercher si besoin" on `key`'s group: her question
    /// about it answered "Oui" when she asked one, else a rule the user
    /// gave.
    fn set_sender_aside(&mut self, key: &str) {
        let key = key.to_lowercase();
        if key.is_empty() || key.contains(':') {
            return;
        }
        if self.aside_rules.iter().any(|s| learn::covers(s, &key)) {
            return;
        }
        let now = self.clock.now();
        let open = learn::load_seen(&self.store)
            .into_iter()
            .find(|s| s.noticed.key() == format!("aside:{key}") && s.open(now));
        if let Some(seen) = open {
            if let Some(said) = self.answer_seen_by(seen.id, KnownAct::Accept) {
                tracing::info!(said, "her question about a sender answered from the queue");
            }
            return;
        }
        let notebook = self
            .queue
            .waiting()
            .chain(self.queue.set_aside())
            .filter_map(|p| self.inbox.get(p))
            .find(|h| matches!(&h.redacted.item.content, Content::Mail(m) if address_of(&m.from) == key))
            .map_or(Notebook::Pro, |h| h.redacted.item.notebook);
        let compiled = Compiled::Aside {
            sender: key.clone(),
        };
        let added = learn::add_rule(
            &mut self.store,
            &format!("les mails de {key} : pas triés, cherchés si besoin"),
            &compiled,
            &Origin::told("queue"),
            notebook,
        );
        match added {
            Ok((rule, _)) => tracing::info!(rule, "a sender set aside"),
            Err(err) => tracing::warn!(%err, "could not set a sender aside"),
        }
    }

    /// "Trier quand même" on `key`: the rules covering it end today.
    fn bring_sender_back(&mut self, key: &str) {
        let key = key.to_lowercase();
        let today = self.store.today();
        let rules = learn::compiled(&self.store, today).unwrap_or_default();
        for (rule, compiled) in rules {
            if let Compiled::Aside { sender } = compiled
                && (learn::covers(&sender, &key) || sender.to_lowercase() == key)
            {
                match learn::close_rule(&mut self.store, rule, today) {
                    Ok(_) => tracing::info!(rule, "a sender brought back to the triage"),
                    Err(err) => tracing::warn!(%err, "could not bring a sender back"),
                }
            }
        }
    }

    /// The queue as the drawer shows it now.
    pub(super) fn queue_view(&self) -> View {
        let fr = self.talk.fr();
        let t = |f: &'static str, e: &'static str| if fr { f } else { e };
        let settings = self.settings.current().triage;
        let item = |p: &str| self.inbox.get(p).map(|h| &h.redacted.item);
        let now = self.queue.running().and_then(|p| {
            let item = item(p)?;
            let (sender, subject) = match &item.content {
                Content::Mail(m) => (m.from.clone(), m.subject.clone()),
                Content::Notification(n) => (n.app.clone(), n.title.clone()),
                Content::Event(e) => (e.calendar_name.clone(), e.title.clone()),
                Content::Birthday(b) => (t("Anniversaires", "Birthdays").to_owned(), b.name.clone()),
            };
            let source = p.split_once(':').map_or(p, |(s, _)| s);
            Some(Waiting {
                sender,
                subject,
                source: learn::shown_source(source),
                date: day_time(item.at, fr),
            })
        });
        let group = |pointers: &mut dyn Iterator<Item = &String>| {
            let mut groups: BTreeMap<String, Group> = BTreeMap::new();
            for pointer in pointers {
                let Some(item) = item(pointer) else {
                    continue;
                };
                let of = of(item);
                let source = pointer.split_once(':').map_or(pointer.as_str(), |(s, _)| s);
                let group = groups.entry(of.key.clone()).or_insert_with(|| Group {
                    key: of.key.clone(),
                    label: of.label.clone(),
                    count: 0,
                    automated: of.automated.clone(),
                    note: match &of.automated {
                        Some(why) => format!(
                            "{} · {} · {why}",
                            learn::shown_source(source),
                            t("envoyé par une machine", "sent by a machine")
                        ),
                        None => learn::shown_source(source),
                    },
                    can_aside: of.mail && !of.key.is_empty(),
                });
                group.count += 1;
            }
            let mut groups: Vec<Group> = groups.into_values().collect();
            groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.label.cmp(&b.label)));
            groups
        };
        let mut groups = group(&mut self.queue.waiting());
        let mut more = String::new();
        if groups.len() > GROUPS_SHOWN {
            let rest: Vec<Group> = groups.split_off(GROUPS_SHOWN);
            let items: u64 = rest.iter().map(|g| g.count).sum();
            more = if fr {
                format!("… {} autres expéditeurs · {items} éléments", rest.len())
            } else {
                format!("… {} other senders · {items} items", rest.len())
            };
        }
        let aside = group(&mut self.queue.set_aside());
        let waiting = self.queue.pending() as u64;
        let set_aside = self.queue.aside_len() as u64;
        let per_item = self.queue.pace(&settings).map(|d| d.as_secs().max(1));
        let (rate, left, left_seconds) = match per_item {
            Some(each) => {
                let left = each * waiting;
                (
                    if fr {
                        format!("≈ {} par élément", span(each, true))
                    } else {
                        format!("≈ {} an item", span(each, false))
                    },
                    match (waiting, fr) {
                        (0, _) => String::new(),
                        (_, true) => format!("encore ≈ {}", span(left, true)),
                        (_, false) => format!("≈ {} left", span(left, false)),
                    },
                    (waiting > 0).then_some(left),
                )
            }
            None => (String::new(), String::new(), None),
        };
        let pause = match &self.queue.state {
            State::Paused { why, .. } if waiting > 0 => Some(match (why, fr) {
                (Pause::Heat(c), true) => format!(
                    "L’ordinateur chauffe ({c} °C) : je reprends sous {} °C.",
                    settings.heat
                ),
                (Pause::Heat(c), false) => format!(
                    "The computer is hot ({c} °C): I go on below {} °C.",
                    settings.heat
                ),
                (Pause::Battery(c), true) => format!(
                    "Sur batterie ({c} %) : je reprends sur secteur, ou au-dessus de {} %.",
                    settings.battery
                ),
                (Pause::Battery(c), false) => format!(
                    "On battery ({c} %): I go on once plugged in, or above {} %.",
                    settings.battery
                ),
                (Pause::Unavailable, true) => {
                    "Le modèle local ne répond pas : je réessaie dans cinq minutes. Rien n’est perdu."
                        .to_owned()
                }
                (Pause::Unavailable, false) => {
                    "The local model is not answering: I try again in five minutes. Nothing is lost."
                        .to_owned()
                }
            }),
            _ => None,
        };
        let slow = match self.placement {
            Some(crate::providers::openai::Placement::Split) => Some(
                t(
                    "Le modèle tourne en partie sur le processeur : c’est plus lent.",
                    "The model runs partly on the processor: it is slower.",
                )
                .to_owned(),
            ),
            Some(crate::providers::openai::Placement::Cpu) => Some(
                t(
                    "Le modèle tourne sur le processeur : c’est bien plus lent.",
                    "The model runs on the processor: it is much slower.",
                )
                .to_owned(),
            ),
            _ => None,
        };
        let state = match (&self.queue.state, waiting) {
            (State::Running(_), _) => t("Je trie", "Sorting"),
            (State::Paused { .. }, n) if n > 0 => t("En pause", "Paused"),
            (State::Resting { .. }, n) if n > 0 => t("Je souffle entre deux", "Resting between two"),
            (_, 0) => t("Rien à trier", "Nothing to sort"),
            _ => t("Je m’y remets", "Getting back to it"),
        }
        .to_owned();
        let mut counts = if fr {
            vec![
                format!("{waiting} en attente"),
                format!("{} trié{}", self.queue.done, if self.queue.done > 1 { "s" } else { "" }),
            ]
        } else {
            vec![
                format!("{waiting} waiting"),
                format!("{} sorted", self.queue.done),
            ]
        };
        if set_aside > 0 {
            counts.push(if fr {
                format!("{set_aside} mis de côté")
            } else {
                format!("{set_aside} set aside")
            });
        }
        View {
            state,
            now,
            pause,
            slow,
            rate,
            per_item,
            left,
            left_seconds,
            counts: counts.join(" · "),
            waiting,
            done: self.queue.done,
            set_aside,
            model: self
                .triager
                .as_ref()
                .map(|t| {
                    let (provider, model) = t.endpoint();
                    format!("{provider}/{model}")
                })
                .unwrap_or_default(),
            groups,
            more,
            aside,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_addresses_read_as_the_user_reads_them() {
        assert_eq!(span(39, true), "39 s");
        assert_eq!(span(250, true), "4 min");
        assert_eq!(span(38_493, true), "10 h 42");
        assert_eq!(span(38_493, false), "10 h 42 min");
        assert_eq!(
            address_of("GitHub <Notifications@GitHub.com>"),
            "notifications@github.com"
        );
        assert_eq!(address_of("lea@atelier-lune.fr"), "lea@atelier-lune.fr");
    }
}
