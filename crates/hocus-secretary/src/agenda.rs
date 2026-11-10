//! Her agenda (#762): every event of the calendars she reads between two
//! instants, as her day's part and her `agenda` tool give it.
//!
//! An event is its latest episode (a moved event's earlier ones are what
//! it was), with its day record when it has one (its title as the
//! calendar gives it, its end, whether the user declined it). Declined
//! and cancelled events are not on the user's day; the same meeting on
//! several calendars (the user's and a colleague's shared with them) is
//! one line, the user's copy kept.
//!
//! **The user's own** come first when a list has to be cut: those of a
//! calendar of theirs (the accounts' listings say which, kept in her
//! memory's `meta` as each source's part, [`keep_own`]) and the
//! invitations they accepted. What is cut is said, with how to get it.

use std::collections::{BTreeMap, BTreeSet};

use hocus_secretary_link::{
    Notebook,
    calendars::CalendarInfo,
    consent::{SourceKey, SourceKind},
};
use hocus_sources::facts;
use jiff::{SignedDuration, Timestamp, civil::Date, tz::TimeZone};
use rusqlite::params;

use crate::{
    context::tokens,
    fold::fold,
    inert,
    store::{Result, Store, stamp, unstamp},
};

/// The user's own calendars, as their events' source parts, by account.
const OWN: &str = "own-calendars";

/// An event on the agenda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub episode: i64,
    pub pointer: String,
    pub start: Timestamp,
    pub end: Option<Timestamp>,
    pub title: String,
    pub calendar: String,
    /// The user's own: on a calendar of theirs, or accepted.
    pub mine: bool,
    pub notebook: Notebook,
}

/// Which calendars of the accounts listed are the user's own, kept for
/// the day's list: an account not listed this time keeps what it had.
pub fn keep_own(store: &Store, found: &BTreeMap<String, Vec<CalendarInfo>>) -> Result<()> {
    let mut own: BTreeMap<String, Vec<String>> = store
        .meta(OWN)?
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    for (account, list) in found {
        let source = SourceKey::new(account.as_str(), SourceKind::Calendar).to_string();
        own.insert(
            account.clone(),
            list.iter()
                .filter(|c| c.own())
                .map(|c| format!("{source}#{}", c.id))
                .collect(),
        );
    }
    let text = serde_json::to_string(&own).unwrap_or_else(|_| "{}".to_owned());
    store.set_meta(OWN, &text)
}

/// The sources (`ACCOUNT:calendar#ID`) of the user's own calendars.
fn own(store: &Store) -> BTreeSet<String> {
    store
        .meta(OWN)
        .ok()
        .flatten()
        .and_then(|t| serde_json::from_str::<BTreeMap<String, Vec<String>>>(&t).ok())
        .unwrap_or_default()
        .into_values()
        .flatten()
        .collect()
}

/// The events that overlap `[from, to)`, of `notebook` if given, by
/// start: declined and cancelled ones left out, one line for a meeting
/// several calendars show.
pub fn between(
    store: &Store,
    (from, to): (Timestamp, Timestamp),
    notebook: Option<Notebook>,
) -> Result<Vec<Entry>> {
    let own = own(store);
    let mut statement = store.conn.prepare(
        "SELECT e.id, e.pointer, e.at, coalesce(e.ends, a.end), coalesce(a.title, e.digest),
                coalesce(e.calendar, a.calendar, ''), coalesce(a.mine, 0),
                coalesce(a.declined, 0), e.notebook, coalesce(e.source, '')
         FROM episodes e LEFT JOIN agenda a ON a.pointer = e.pointer
         WHERE e.kind = 'event' AND e.at < ?2
           AND (e.at >= ?1 OR coalesce(e.ends, a.end, e.at) > ?1)
           AND (?3 IS NULL OR e.notebook = ?3)
           AND NOT EXISTS (SELECT 1 FROM episodes n WHERE n.pointer = e.pointer AND n.id > e.id)
         ORDER BY e.at, e.id",
    )?;
    let rows = statement.query_map(
        params![stamp(from), stamp(to), notebook.map(Notebook::word)],
        |r| {
            Ok((
                Entry {
                    episode: r.get(0)?,
                    pointer: r.get(1)?,
                    start: unstamp(&r.get::<_, String>(2)?).unwrap_or(from),
                    end: r.get::<_, Option<String>>(3)?.and_then(|t| unstamp(&t)),
                    title: r.get(4)?,
                    calendar: r.get(5)?,
                    mine: r.get(6)?,
                    notebook: Notebook::of(&r.get::<_, String>(8)?).unwrap_or(Notebook::Pro),
                },
                r.get::<_, bool>(7)?,
                r.get::<_, String>(9)?,
            ))
        },
    )?;
    let mut out: Vec<Entry> = Vec::new();
    // A meeting on several calendars, by its title and times.
    let mut seen: BTreeMap<(String, Timestamp, Option<Timestamp>), usize> = BTreeMap::new();
    for row in rows {
        let (mut entry, declined, source) = row?;
        if declined {
            continue;
        }
        entry.mine |= own.contains(&source);
        let key = (fold(&entry.title), entry.start, entry.end);
        match seen.get(&key) {
            Some(&at) => {
                if entry.mine && !out[at].mine {
                    out[at] = entry;
                }
            }
            None => {
                seen.insert(key, out.len());
                out.push(entry);
            }
        }
    }
    Ok(out)
}

/// What a list holds, and what it left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub lines: Vec<String>,
    /// Its events' items (`episode:ID`), for what left.
    pub items: Vec<String>,
    /// Left out: the user's own, the others.
    pub cut_mine: usize,
    pub cut_others: usize,
}

impl Listing {
    pub fn cut(&self) -> usize {
        self.cut_mine + self.cut_others
    }
}

fn hm(at: Timestamp, zone: &TimeZone) -> String {
    let here = at.to_zoned(zone.clone());
    format!("{:02}:{:02}", here.hour(), here.minute())
}

fn midnight(at: Timestamp, zone: &TimeZone) -> bool {
    let here = at.to_zoned(zone.clone());
    here.hour() == 0 && here.minute() == 0
}

/// The day an entry is listed under: its start's, or the first of `days`
/// when it began before.
fn day_of(entry: &Entry, days: &[Date], zone: &TimeZone) -> Date {
    let start = entry.start.to_zoned(zone.clone()).date();
    days.first().map_or(start, |first| start.max(*first))
}

/// An entry as a line, under `day`.
fn line(entry: &Entry, day: Date, zone: &TimeZone) -> String {
    let start = entry.start.to_zoned(zone.clone());
    let all_day = midnight(entry.start, zone)
        && entry.end.is_some_and(|e| {
            midnight(e, zone) && e.duration_since(entry.start) >= SignedDuration::from_hours(23)
        });
    let when = if all_day {
        // Its last day: the one before its end.
        let last = entry
            .end
            .and_then(|e| e.checked_sub(SignedDuration::from_secs(1)).ok())
            .map(|e| e.to_zoned(zone.clone()).date());
        match last {
            Some(last) if last > day => {
                format!(
                    "toute la journée, jusqu'au {}",
                    crate::learn::short(last, true)
                )
            }
            _ => "toute la journée".to_owned(),
        }
    } else {
        let from = if start.date() < day {
            format!(
                "depuis le {} {}",
                crate::learn::short(start.date(), true),
                hm(entry.start, zone)
            )
        } else {
            hm(entry.start, zone)
        };
        match entry.end.filter(|e| *e > entry.start) {
            Some(end) if end.to_zoned(zone.clone()).date() == start.date() => {
                format!("{from}–{}", hm(end, zone))
            }
            Some(end) => format!(
                "{from}–{} {}",
                crate::learn::short(end.to_zoned(zone.clone()).date(), true),
                hm(end, zone)
            ),
            None => from,
        }
    };
    let whose = if entry.mine {
        if entry.calendar.is_empty() {
            "à toi".to_owned()
        } else {
            format!("à toi ({})", entry.calendar)
        }
    } else if entry.calendar.is_empty() {
        "agenda partagé".to_owned()
    } else {
        format!("agenda partagé « {} »", entry.calendar)
    };
    let perso = if entry.notebook == Notebook::Perso {
        " · privé"
    } else {
        ""
    };
    // Its title and its calendar's name are their source's words: one
    // line, and no tag ([`inert`]).
    inert::line(&format!(
        "- {when} · {} · {whose}{perso}",
        entry.title.trim()
    ))
    .into_owned()
}

/// `entries` under each of `days` (`title` names a day), within `most`
/// tokens: the user's own first when it has to be cut, then the others,
/// each by start; listed by start. A day with nothing says so.
pub fn listing(
    entries: &[Entry],
    days: &[Date],
    zone: &TimeZone,
    most: usize,
    title: impl Fn(Date) -> String,
) -> Listing {
    let lines: Vec<(Date, String)> = entries
        .iter()
        .map(|e| {
            let day = day_of(e, days, zone);
            (day, line(e, day, zone))
        })
        .collect();
    let headers: usize = days.iter().map(|d| tokens(&title(*d)) + 4).sum();
    let mut rank: Vec<usize> = (0..entries.len()).collect();
    rank.sort_by_key(|&i| (!entries[i].mine, entries[i].start, i));
    let mut kept = vec![false; entries.len()];
    let mut used = headers;
    let mut out = Listing::default();
    // Once one is cut, every one after it is: none of lower rank passes.
    let mut full = false;
    for i in rank {
        let cost = tokens(&lines[i].1) + 1;
        if !full && used + cost <= most {
            used += cost;
            kept[i] = true;
            continue;
        }
        full = true;
        if entries[i].mine {
            out.cut_mine += 1;
        } else {
            out.cut_others += 1;
        }
    }
    for day in days {
        out.lines.push(title(*day));
        let mut any = false;
        let mut left = 0;
        for (i, (on, text)) in lines.iter().enumerate() {
            if on != day {
                continue;
            }
            if kept[i] {
                out.lines.push(text.clone());
                out.items.push(format!("episode:{}", entries[i].episode));
                any = true;
            } else {
                left += 1;
            }
        }
        match (any, left) {
            (false, 0) => out.lines.push("- rien à l'agenda".to_owned()),
            (_, 0) => {}
            (_, n) => out.lines.push(format!("- ({n} de plus, non listés)")),
        }
    }
    out
}

/// A day as the agenda's heading names it: "mardi 10 novembre 2026".
pub fn long_date(day: Date) -> String {
    format!("{} {}", facts::french_date(day), day.year())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: &str) -> Timestamp {
        t.parse().expect("a time")
    }

    fn entry(id: i64, start: &str, end: &str, title: &str, mine: bool) -> Entry {
        Entry {
            episode: id,
            pointer: format!("cal:{id}"),
            start: at(start),
            end: Some(at(end)),
            title: title.to_owned(),
            calendar: if mine { "Travail" } else { "Salle Everest" }.to_owned(),
            mine,
            notebook: Notebook::Pro,
        }
    }

    #[test]
    fn the_user_s_own_come_first_when_a_list_is_cut() {
        let zone = TimeZone::UTC;
        let day = Date::new(2026, 11, 10).expect("a date");
        let mut entries: Vec<Entry> = (0..40)
            .map(|i| {
                entry(
                    i,
                    "2026-11-10T07:00:00Z",
                    "2026-11-10T08:00:00Z",
                    &format!("Réunion d'équipe numéro {i}"),
                    false,
                )
            })
            .collect();
        entries.push(entry(
            100,
            "2026-11-10T09:15:00Z",
            "2026-11-10T09:30:00Z",
            "Daily",
            true,
        ));
        entries.push(entry(
            101,
            "2026-11-10T17:00:00Z",
            "2026-11-10T18:00:00Z",
            "Point client",
            true,
        ));
        let listing = listing(&entries, &[day], &zone, 200, |_| "Aujourd'hui :".to_owned());
        let text = listing.lines.join("\n");
        assert!(
            text.contains("- 09:15–09:30 · Daily · à toi (Travail)"),
            "{text}"
        );
        assert!(text.contains("- 17:00–18:00 · Point client · à toi (Travail)"));
        assert_eq!(listing.cut_mine, 0);
        assert!(listing.cut_others > 0);
        assert!(text.contains("de plus, non listés"), "{text}");
        // Listed by start: the others' morning before the user's daily.
        let first_other = text.find("Réunion d'équipe").expect("some others");
        assert!(first_other < text.find("Daily").expect("the daily"));
    }

    #[test]
    fn a_moved_event_is_where_it_is_now_and_a_deleted_one_is_gone() -> anyhow::Result<()> {
        let mut store =
            Store::in_memory(crate::clock::Clock::virtual_at(at("2026-11-10T06:00:00Z")))?;
        let add = |store: &mut Store, id: &str, start: &str, title: &str| -> anyhow::Result<()> {
            let (episode, _) = store.add_episode(
                &crate::graph::NewEpisode {
                    at: at(start),
                    kind: "event".to_owned(),
                    digest: title.to_owned(),
                    source: Some("cal".to_owned()),
                    pointer: format!("cal:{id}"),
                    notebook: Notebook::Pro,
                },
                &[],
            )?;
            let end = at(start) + SignedDuration::from_hours(1);
            store.set_episode_extras(episode, Some(end), None, Some("Travail"))?;
            crate::day::keep_event(
                &store.conn,
                &crate::day::Event {
                    pointer: format!("cal:{id}"),
                    source: Some("cal".to_owned()),
                    start: at(start),
                    end,
                    title: title.to_owned(),
                    calendar: "Travail".to_owned(),
                    attendees: Vec::new(),
                    organiser: None,
                    presenter: None,
                    busy: true,
                    notebook: Notebook::Pro,
                    mine: true,
                    declined: false,
                },
            )?;
            Ok(())
        };
        add(&mut store, "e1", "2026-11-10T09:00:00Z", "Revue")?;
        add(&mut store, "e1", "2026-11-10T15:00:00Z", "Revue")?;
        add(&mut store, "e2", "2026-11-10T11:00:00Z", "Annulée")?;
        crate::day::gone(&store.conn, "cal:e2", "cal")?;
        crate::day::gone(&store.conn, "cal:never-read", "cal")?;
        let day = between(
            &store,
            (at("2026-11-10T00:00:00Z"), at("2026-11-11T00:00:00Z")),
            None,
        )?;
        assert_eq!(
            day.iter()
                .map(|e| (e.title.as_str(), e.start))
                .collect::<Vec<_>>(),
            [("Revue", at("2026-11-10T15:00:00Z"))]
        );
        assert!(day[0].mine);
        Ok(())
    }

    #[test]
    fn an_all_day_event_and_an_empty_day_are_said() {
        let zone = TimeZone::UTC;
        let today = Date::new(2026, 11, 10).expect("a date");
        let tomorrow = Date::new(2026, 11, 11).expect("a date");
        let entries = vec![entry(
            1,
            "2026-11-09T00:00:00Z",
            "2026-11-12T00:00:00Z",
            "Congés Sophie",
            false,
        )];
        let listing = listing(&entries, &[today, tomorrow], &zone, 500, |d| {
            format!("{d} :")
        });
        assert_eq!(
            listing.lines,
            [
                "2026-11-10 :",
                "- toute la journée, jusqu'au 11\u{a0}nov. · Congés Sophie · agenda partagé « Salle Everest »",
                "2026-11-11 :",
                "- rien à l'agenda",
            ]
        );
    }
}
