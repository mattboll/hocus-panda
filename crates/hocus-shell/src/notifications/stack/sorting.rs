//! What the user's focus and the agents do with the bell (#923,
//! docs/design/notifications.md "The bell keeps what still needs you"): a
//! window taking the focus folds its application's notifications (by its
//! id, else by its sender's window); an agent lists them, pins one, tidies
//! some, and its answer says what it changed for « Annuler ».

use hocus_link::{AppFocus, NotePin, NotesAnswer, NotesAsk};

use super::{Effect, Kept, Sight, Stack, Urgency, journal};

impl Stack {
    /// An agent's ask (#923): the bell read, or sorted; the answer said.
    pub(super) fn ask(&mut self, token: u64, ask: NotesAsk, effects: &mut Vec<Effect>) {
        let zone = crate::notifications::zone();
        let answer = match ask {
            NotesAsk::List { app, state, limit } => {
                let mut items: Vec<&Kept> = self
                    .history
                    .items()
                    .iter()
                    .filter(|k| app.as_deref().is_none_or(|a| of_app(k, a)))
                    .filter(|k| state.is_none_or(|s| k.seen.state() == s))
                    .collect();
                items.sort_by_key(|k| std::cmp::Reverse(k.note.at));
                items.truncate(limit as usize);
                let listed: Vec<serde_json::Value> = items.iter().map(|k| said(k, &zone)).collect();
                tracing::info!(count = listed.len(), "an agent reads the bell");
                NotesAnswer::Done {
                    value: serde_json::json!({ "notifications": listed }).to_string(),
                    count: listed.len() as u32,
                    app: None,
                    before: Vec::new(),
                }
            }
            NotesAsk::Search {
                text,
                app,
                since,
                until,
                limit,
            } => {
                let states = self
                    .history
                    .items()
                    .iter()
                    .map(|k| (k.key, k.seen.state()))
                    .collect();
                effects.push(Effect::Search {
                    token,
                    search: journal::Search {
                        text,
                        app,
                        since,
                        until,
                        limit: limit as usize,
                    },
                    states,
                });
                return;
            }
            NotesAsk::Pin { id, agent, reason } => {
                let pin = NotePin {
                    agent,
                    reason: reason.as_deref().and_then(reason_text),
                    at: crate::notifications::wall_ms(),
                };
                let app = self.history.get(id).map(|k| k.note.app.clone());
                match self.history.pin(id, pin) {
                    Ok(before) => {
                        tracing::info!(id, "an agent pins a notification");
                        NotesAnswer::Done {
                            value: serde_json::json!({ "pinned": id }).to_string(),
                            count: 1,
                            app,
                            before: vec![before],
                        }
                    }
                    Err(why) => NotesAnswer::Refused(why),
                }
            }
            NotesAsk::Unpin { id } => {
                let app = self.history.get(id).map(|k| k.note.app.clone());
                match self.history.unpin(id) {
                    Ok(before) => NotesAnswer::Done {
                        value: serde_json::json!({ "unpinned": id }).to_string(),
                        count: 1,
                        app,
                        before: vec![before],
                    },
                    Err(why) => NotesAnswer::Refused(why),
                }
            }
            NotesAsk::Tidy { ids, app } => {
                let keys: Vec<u64> = match &app {
                    Some(app) => self
                        .history
                        .items()
                        .iter()
                        .filter(|k| of_app(k, app) && k.seen != Sight::Seen)
                        .map(|k| k.key)
                        .collect(),
                    None => ids,
                };
                let befores = self.history.tidy(&keys);
                let mut apps: Vec<String> = befores
                    .iter()
                    .filter_map(|b| self.history.get(b.id))
                    .map(|k| k.note.app.clone())
                    .collect();
                apps.sort();
                apps.dedup();
                let app = (apps.len() == 1).then(|| apps.remove(0));
                tracing::info!(count = befores.len(), "an agent tidies the bell");
                NotesAnswer::Done {
                    value: serde_json::json!({ "tidied": befores.len() }).to_string(),
                    count: befores.len() as u32,
                    app,
                    before: befores,
                }
            }
        };
        effects.push(Effect::Answer { token, answer });
    }

    /// A window took the user's focus (#923): its application's
    /// notifications fold, but the pinned ones and the desktop's own, and
    /// those of an application set not to.
    pub(super) fn app_focused(&mut self, focus: &AppFocus) {
        // The lock takes the focus: nothing folds while locked.
        if self.locked {
            return;
        }
        let keys: Vec<u64> = self
            .history
            .items()
            .iter()
            .filter(|k| {
                !k.own
                    && k.pin.is_none()
                    && self.levels.folds_on_focus(&k.app_id())
                    && belongs(k, focus)
            })
            .map(|k| k.key)
            .collect();
        let folded = self.history.user_saw(&keys, false);
        tracing::debug!(
            app = focus.app,
            folded,
            "a window takes the focus: its notifications seen"
        );
    }
}

/// Whether `kept` is of the application of the window that took the focus:
/// by its id; or, its application having no window, by its sender's
/// window or its sender's nearest parent's.
fn belongs(kept: &Kept, focus: &AppFocus) -> bool {
    let app = kept.app_id();
    if hocus_link::same_app(&app, &focus.app) {
        return true;
    }
    if focus
        .windows
        .iter()
        .any(|(window, _)| hocus_link::same_app(&app, window))
    {
        return false;
    }
    kept.note
        .ancestry
        .iter()
        .find(|pid| focus.windows.iter().any(|(_, p)| p == *pid))
        == Some(&focus.pid)
}

/// Whether `kept` is of application `app`, by its id or its name,
/// ignoring case.
fn of_app(kept: &Kept, app: &str) -> bool {
    let app = app.trim();
    kept.app_id().eq_ignore_ascii_case(app) || kept.note.app.trim().eq_ignore_ascii_case(app)
}

/// `kept` as an agent reads it.
fn said(kept: &Kept, zone: &jiff::tz::TimeZone) -> serde_json::Value {
    let note = &kept.note;
    let mut value = serde_json::json!({
        "id": kept.key,
        "app": note.app,
        "app_id": note.app_id(),
        "title": note.summary,
        "body": note.body,
        "at": journal::rfc3339(note.at, zone),
        "state": kept.seen.state().word(),
        "urgency": match note.urgency {
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::Critical => "critical",
        },
    });
    if let Some(pin) = &kept.pin {
        value["pinned"] = serde_json::json!({ "agent": pin.agent, "reason": pin.reason });
    }
    if kept.live.is_some() && !note.actions.is_empty() {
        let labels: Vec<&str> = note.actions.iter().map(|(_, l)| l.as_str()).collect();
        value["actions"] = serde_json::json!(labels);
    }
    value
}

/// A pin's reason as it shows: one line, [`hocus_link::PIN_REASON`]
/// characters at most; none when it says nothing.
fn reason_text(reason: &str) -> Option<String> {
    let line: Vec<&str> = reason.split_whitespace().collect();
    let line = line.join(" ");
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= hocus_link::PIN_REASON {
        return Some(line);
    }
    let mut cut: String = line.chars().take(hocus_link::PIN_REASON - 1).collect();
    cut.truncate(cut.trim_end().len());
    cut.push('…');
    Some(cut)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::super::tests::{note, notify, restored_now, stack};
    use super::*;
    use crate::notifications::stack::Input;

    #[test]
    fn a_window_taking_the_focus_folds_its_applications_and_a_scripts_by_its_terminal() {
        let mut s = stack();
        let t0 = Instant::now();
        restored_now(&mut s, 0);
        let from = |id: u32, app: &str, ancestry: Vec<u32>| {
            let mut n = note(id);
            app.clone_into(&mut n.app);
            n.urgency = Urgency::Low;
            n.ancestry = ancestry;
            notify(n)
        };
        s.handle(from(1, "Chat", Vec::new()), t0);
        // `notify-send` in a script, run by a shell in a terminal (400).
        s.handle(from(2, "notify-send", vec![500, 400, 300]), t0);
        s.handle(from(3, "Courriel", Vec::new()), t0);
        assert_eq!(s.badge(), 3);
        let focus = |app: &str, pid: u32| {
            Input::AppFocus(AppFocus {
                app: app.to_owned(),
                pid,
                windows: vec![
                    ("org.example.Chat".to_owned(), 100),
                    ("foot".to_owned(), 400),
                    ("org.example.Courriel".to_owned(), 200),
                ],
            })
        };
        s.handle(focus("org.example.Chat", 100), t0);
        assert_eq!(s.badge(), 2);
        s.handle(focus("foot", 400), t0);
        assert_eq!(s.badge(), 1, "the script's, by its terminal");
        // Locked: nothing folds.
        s.handle(Input::Locked(true), t0);
        s.handle(focus("org.example.Courriel", 200), t0);
        assert_eq!(s.badge(), 1);
        s.handle(Input::Locked(false), t0);
        // Set not to: nothing folds.
        s.handle(
            Input::SeenOnFocus {
                id: "Courriel".to_owned(),
                on: false,
            },
            t0,
        );
        s.handle(focus("org.example.Courriel", 200), t0);
        assert_eq!(s.badge(), 1);
        s.handle(
            Input::SeenOnFocus {
                id: "courriel".to_owned(),
                on: true,
            },
            t0,
        );
        s.handle(focus("org.example.Courriel", 200), t0);
        assert_eq!(s.badge(), 0);
    }

    #[test]
    fn an_agent_lists_pins_and_tidies_and_its_answers_say_so() {
        let mut s = stack();
        let t0 = Instant::now();
        restored_now(&mut s, 0);
        for (id, app) in [(1, "Chat"), (2, "Chat"), (3, "Courriel")] {
            let mut n = note(id);
            app.clone_into(&mut n.app);
            n.urgency = Urgency::Low;
            s.handle(notify(n), t0);
        }
        let answer = |effects: Vec<Effect>| {
            effects.into_iter().find_map(|e| match e {
                Effect::Answer { answer, .. } => Some(answer),
                _ => None,
            })
        };
        let listed = answer(s.handle(
            Input::Ask {
                token: 1,
                ask: NotesAsk::List {
                    app: Some("chat".to_owned()),
                    state: None,
                    limit: 50,
                },
            },
            t0,
        ));
        assert!(
            matches!(&listed, Some(NotesAnswer::Done { count: 2, value, .. }) if value.contains("\"app_id\":\"Chat\"")),
            "{listed:?}"
        );
        let tidied = answer(s.handle(
            Input::Ask {
                token: 2,
                ask: NotesAsk::Tidy {
                    ids: Vec::new(),
                    app: Some("Chat".to_owned()),
                },
            },
            t0,
        ));
        let Some(NotesAnswer::Done {
            count, app, before, ..
        }) = tidied
        else {
            panic!("{tidied:?}");
        };
        assert_eq!((count, app.as_deref()), (2, Some("Chat")));
        assert_eq!(s.badge(), 1);
        let pinned = answer(s.handle(
            Input::Ask {
                token: 3,
                ask: NotesAsk::Pin {
                    id: 1,
                    agent: "Claude".to_owned(),
                    reason: Some(format!("{}  long", "x".repeat(100))),
                },
            },
            t0,
        ));
        assert!(matches!(pinned, Some(NotesAnswer::Done { count: 1, .. })));
        assert_eq!(s.badge(), 2);
        let reason = s
            .history
            .get(1)
            .and_then(|k| k.pin.clone())
            .and_then(|p| p.reason);
        assert!(
            reason.is_some_and(|r| r.chars().count() == hocus_link::PIN_REASON && r.ends_with('…'))
        );
        // Undone: as they were.
        s.handle(Input::Undo(before), t0);
        assert_eq!(s.badge(), 3);
    }
}
