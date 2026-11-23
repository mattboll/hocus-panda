//! The bell's tools (#923, docs/design/mcp.md "Notifications"):
//! `list_notifications`, `search_notifications`, `pin_notification`,
//! `unpin_notification` and `tidy_notifications`.
//!
//! The shell holds the notifications (decision 0003): the loop checks the
//! right, shows, journals and undoes as for any call, and asks the shell
//! ([`ToShell::Notes`]), which answers off its window's thread
//! ([`hocus_link::ToCompositor::Notes`], by token). An ask the shell does
//! not answer in [`ANSWER`] (it restarted meanwhile) is answered with an
//! error, never waited for on the loop. Reading shows nothing; the journal
//! says how many were read, never what they say.

use std::time::Duration;

use hocus_agents::mcp::{Call, MAX_NOTES, NOTES};
use hocus_link::{McpKind, McpSaid, NotesAnswer, NotesAsk, ToShell};
use serde_json::Value;
use smithay::reexports::calloop::{
    RegistrationToken,
    timer::{TimeoutAction, Timer},
};

use super::{Entry, Waiting, clock};
use crate::State;

/// How long the shell has to answer.
pub const ANSWER: Duration = Duration::from_secs(5);

/// What waits for the shell's answer.
#[derive(Debug)]
pub(super) struct Asked {
    what: For,
    timer: Option<RegistrationToken>,
}

#[derive(Debug)]
enum For {
    /// A reading call: list or search.
    Read {
        session: u64,
        id: u64,
        agent: String,
        tool: &'static str,
    },
    /// A pin, an unpin or a tidying, shown and now done.
    Act(Box<Waiting>),
}

/// A date (`2026-11-20`: its day's start, or its end for `until`) or an
/// RFC 3339 time, as milliseconds since the epoch, in `zone`.
fn when(text: &str, end: bool, zone: &jiff::tz::TimeZone) -> Result<i64, String> {
    let text = text.trim();
    if let Ok(stamp) = text.parse::<jiff::Timestamp>() {
        return Ok(stamp.as_millisecond());
    }
    let date: jiff::civil::Date = text.parse().map_err(|_| {
        format!("search_notifications: {text:?} is neither a date (2026-11-20) nor RFC 3339")
    })?;
    let day = date.to_zoned(zone.clone()).map_err(|e| e.to_string())?;
    let at = if end {
        day.tomorrow()
            .map_err(|e| e.to_string())?
            .timestamp()
            .as_millisecond()
            - 1
    } else {
        day.timestamp().as_millisecond()
    };
    Ok(at)
}

/// What a call asks of the bell.
pub(super) fn ask_of(call: &Call, agent: &str) -> Result<NotesAsk, String> {
    let limit = |l: Option<u32>| l.unwrap_or(NOTES).clamp(1, MAX_NOTES);
    let zone = jiff::tz::TimeZone::system();
    Ok(match call {
        Call::ListNotifications(c) => NotesAsk::List {
            app: c.app.clone(),
            state: c.state,
            limit: limit(c.limit),
        },
        Call::SearchNotifications(c) => NotesAsk::Search {
            text: c.text.clone(),
            app: c.app.clone(),
            since: c
                .since
                .as_deref()
                .map(|s| when(s, false, &zone))
                .transpose()?,
            until: c
                .until
                .as_deref()
                .map(|s| when(s, true, &zone))
                .transpose()?,
            limit: limit(c.limit),
        },
        Call::PinNotification(c) => NotesAsk::Pin {
            id: c.id,
            agent: agent.to_owned(),
            reason: c.reason.clone(),
        },
        Call::UnpinNotification(c) => NotesAsk::Unpin { id: c.id },
        Call::TidyNotifications(c) => NotesAsk::Tidy {
            ids: c.ids.clone(),
            app: c.app.clone(),
        },
        _ => return Err("not a tool of the bell".to_owned()),
    })
}

/// Whether a call of the bell's can be done as asked.
pub(super) fn check(call: &Call) -> Result<(), String> {
    match call {
        Call::TidyNotifications(c) if c.ids.is_empty() && c.app.is_none() => {
            Err("tidy_notifications: say which, with ids or app".to_owned())
        }
        Call::TidyNotifications(c) if !c.ids.is_empty() && c.app.is_some() => {
            Err("tidy_notifications: ids or app, not both".to_owned())
        }
        Call::SearchNotifications(c) => {
            let zone = jiff::tz::TimeZone::system();
            for (date, end) in [(&c.since, false), (&c.until, true)] {
                if let Some(date) = date {
                    when(date, end, &zone)?;
                }
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

impl State {
    /// Asks the shell `ask` for `what`; false when there is no shell.
    fn mcp_notes_ask(&mut self, ask: NotesAsk, what: For) -> Result<(), (For, String)> {
        if !self.shell_linked() {
            return Err((
                what,
                "the bell is not there: the desktop's shell is not running".to_owned(),
            ));
        }
        self.mcp.next_notes += 1;
        let token = self.mcp.next_notes;
        let timer = self
            .loop_handle
            .insert_source(Timer::from_duration(ANSWER), move |_, _, app| {
                app.state.mcp_notes_answered(
                    token,
                    NotesAnswer::Refused("the bell did not answer in time".to_owned()),
                );
                TimeoutAction::Drop
            })
            .map_err(|err| tracing::warn!(error = %err.error, "no timer for the bell's answer"))
            .ok();
        self.mcp.notes.insert(token, Asked { what, timer });
        self.tell_shell(ToShell::Notes { token, ask });
        Ok(())
    }

    /// A reading call of the bell's: asked, answered when the shell says.
    pub(super) fn mcp_notes_read(
        &mut self,
        session: u64,
        id: u64,
        agent: &str,
        call: &Call,
    ) -> Option<Result<Value, String>> {
        let ask = match ask_of(call, agent) {
            Ok(ask) => ask,
            Err(why) => return Some(Err(why)),
        };
        let what = For::Read {
            session,
            id,
            agent: agent.to_owned(),
            tool: call.tool(),
        };
        self.mcp_notes_ask(ask, what).err().map(|(_, why)| Err(why))
    }

    /// A pin, an unpin or a tidying shown long enough: asked of the shell,
    /// the queue waiting for its answer.
    pub(super) fn mcp_notes_do(&mut self, waiting: Waiting) {
        let ask = match ask_of(&waiting.call, &waiting.agent) {
            Ok(ask) => ask,
            Err(why) => {
                self.mcp_answer(waiting.session, waiting.id, Err(why));
                self.mcp_after(&waiting.agent);
                return self.mcp_next();
            }
        };
        self.mcp_request(&waiting.agent, &waiting.intent, McpKind::Notifications);
        self.mcp.busy = true;
        if let Err((For::Act(waiting), why)) = self.mcp_notes_ask(ask, For::Act(Box::new(waiting)))
        {
            self.mcp.busy = false;
            self.mcp_answer(waiting.session, waiting.id, Err(why));
            self.mcp_after(&waiting.agent);
            self.mcp_next();
        }
    }

    /// The shell's answer to ask `token` (or its time ran out).
    pub(crate) fn mcp_notes_answered(&mut self, token: u64, answer: NotesAnswer) {
        let Some(asked) = self.mcp.notes.remove(&token) else {
            return;
        };
        if let Some(timer) = asked.timer {
            self.loop_handle.remove(timer);
        }
        self.mcp.dirty = true;
        match asked.what {
            For::Read {
                session,
                id,
                agent,
                tool,
            } => {
                let result = match answer {
                    NotesAnswer::Done { value, count, .. } => {
                        // How many, never what they say.
                        let (said, what) = if tool == "search_notifications" {
                            (McpSaid::SearchedNotes, format!("{count} found"))
                        } else {
                            (McpSaid::ReadNotes { count }, format!("{count} read"))
                        };
                        self.mcp_line_shown(&agent, said);
                        self.mcp_journal(Entry {
                            at: clock(true),
                            agent: agent.clone(),
                            tool,
                            what,
                            intent: None,
                            refused: None,
                        });
                        serde_json::from_str(&value).map_err(|e| e.to_string())
                    }
                    NotesAnswer::Refused(why) => Err(why),
                };
                self.mcp_answer(session, id, result);
            }
            For::Act(waiting) => {
                self.mcp.busy = false;
                let agent = waiting.agent.clone();
                match answer {
                    NotesAnswer::Done {
                        value,
                        count,
                        app,
                        before,
                    } => {
                        // The request ends 1.5 s after this, not after it
                        // was asked: the shell may take its time.
                        self.mcp_request(&agent, &waiting.intent, McpKind::Notifications);
                        if let Some(request) = self.mcp.open.get_mut(&agent) {
                            let summary = &mut request.summary;
                            // One application's for the whole request,
                            // else none said.
                            let first = summary.pinned + summary.unpinned + summary.tidied == 0;
                            summary.notes_app = match app {
                                Some(app) if first || summary.notes_app.as_ref() == Some(&app) => {
                                    Some(app)
                                }
                                _ => None,
                            };
                            match &waiting.call {
                                Call::PinNotification(_) => summary.pinned += count,
                                Call::UnpinNotification(_) => summary.unpinned += count,
                                _ => summary.tidied += count,
                            }
                            request.before.notes.extend(before);
                        }
                        let what = format!("{count} notifications");
                        let result = serde_json::from_str::<Value>(&value)
                            .map(|v| (v, what))
                            .map_err(|e| e.to_string());
                        self.mcp_done(&waiting, result);
                    }
                    NotesAnswer::Refused(why) => {
                        self.mcp_answer(waiting.session, waiting.id, Err(why));
                        self.mcp_after(&agent);
                        self.mcp_next();
                    }
                }
            }
        }
    }

    /// The shell went: what waited for it is answered at once.
    pub(crate) fn mcp_notes_gone(&mut self) {
        let tokens: Vec<u64> = self.mcp.notes.keys().copied().collect();
        for token in tokens {
            self.mcp_notes_answered(
                token,
                NotesAnswer::Refused("the bell went: the desktop's shell restarts".to_owned()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_date_is_its_day_and_rfc_3339_its_time() {
        let utc = jiff::tz::TimeZone::UTC;
        assert_eq!(when("2026-11-20", false, &utc), Ok(1_795_132_800_000));
        assert_eq!(
            when("2026-11-20", true, &utc),
            Ok(1_795_132_800_000 + 86_400_000 - 1)
        );
        assert_eq!(
            when("2026-11-20T10:00:00+01:00", false, &utc),
            Ok(1_795_132_800_000 + 9 * 3_600_000)
        );
        assert!(when("hier", false, &utc).is_err());
    }
}
