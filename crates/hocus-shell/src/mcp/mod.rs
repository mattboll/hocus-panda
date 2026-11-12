//! What the agents acting on the desktop through `hocus mcp` show
//! (docs/design/mcp.md): the compositor says all of it in one message
//! ([`hocus_link::McpShown`]) as it changes, and the shell draws it.
//!
//! - **The tag** ([`tag`]): the agent's name over its intent, on what it
//!   is about to touch, a surface of its own while the compositor shows
//!   the action (it rings the window itself; the bar's desktops ring a
//!   desktop's tile, [`tile`]).
//! - **Its row** in the agents' group: one row per agent, its mark and
//!   what it does ([`row_word`]); a click opens the journal
//!   ([`crate::popover::journal`]).
//! - **The toast** ([`toast`]) once a request settled: one sentence,
//!   "Annuler" and "Voir".

pub mod tag;
pub mod toast;

use std::sync::{Mutex, PoisonError};

use hocus_link::{
    AgentShown, McpDoing, McpKind, McpRefusal, McpRow, McpSaid, McpShown, McpSummary,
};
use hocus_words::{Lang, shell::mcp as txt};
use iced::{Element, window};

use crate::{
    Message,
    popover::{self, Effect, Item},
};

/// The agents' rows' ids in the agents' group: above any the compositor
/// gives its other agents.
pub const ROW_BASE: u64 = 1 << 48;

/// Whether an agents' group's row is an MCP agent's.
pub fn is_row(id: u64) -> bool {
    id >= ROW_BASE
}

#[derive(Debug, Default)]
struct Kept {
    shown: McpShown,
    /// The tag's surface and what it shows.
    tag: Option<(window::Id, hocus_link::McpShow)>,
    /// The toast's surface and its request.
    toast: Option<(window::Id, hocus_link::McpToast)>,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);

fn kept<T>(with: impl FnOnce(&mut Kept) -> T) -> T {
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    with(kept.get_or_insert_with(Kept::default))
}

/// What the agents show now.
pub fn shown() -> McpShown {
    kept(|k| k.shown.clone())
}

/// The compositor says what the agents show: the surfaces to open or
/// remove.
pub fn set(shown: McpShown) -> Vec<Effect> {
    let words = Words::new(Lang::of_messages());
    let mut effects = Vec::new();
    let (rows, tile_changed) = kept(|k| {
        let tile_before = tile_of(&k.shown);
        // The tag: one surface per action shown.
        if k.tag.as_ref().map(|(_, s)| s) != shown.show.as_ref() {
            if let Some((id, _)) = k.tag.take() {
                effects.push(Effect::Remove(id));
            }
            if let Some(show) = &shown.show {
                let id = window::Id::unique();
                let (size, at) = tag::place(show);
                tracing::debug!(
                    agent = show.agent,
                    intent = show.intent,
                    output = show.output,
                    "an MCP agent's tag shows"
                );
                effects.push(Effect::Agent {
                    id,
                    output: show.output.clone(),
                    size,
                    at,
                    toast: false,
                });
                k.tag = Some((id, show.clone()));
            }
        }
        // The toast: one surface per request.
        if k.toast.as_ref().map(|(_, t)| t) != shown.toast.as_ref() {
            if let Some((id, _)) = k.toast.take() {
                effects.push(Effect::Remove(id));
            }
            if let Some(toast) = &shown.toast {
                let id = window::Id::unique();
                let (size, at) = toast::place(words, toast);
                tracing::debug!(
                    request = toast.request,
                    said = words.toast(toast),
                    undo = toast.undo,
                    "an MCP agent's toast shows"
                );
                effects.push(Effect::Agent {
                    id,
                    output: toast.output.clone(),
                    size,
                    at,
                    toast: true,
                });
                k.toast = Some((id, toast.clone()));
            }
        }
        let changed = tile_before != tile_of(&shown);
        k.shown = shown;
        (rows_of(&k.shown), changed)
    });
    crate::agents::set_mcp(rows);
    crate::ui::touch();
    popover::changed(Item::Journal);
    if tile_changed {
        effects.push(Effect::Wake(hocus_link::DESKTOPS_NAMESPACE));
    }
    effects.push(Effect::Wake(hocus_link::MCP_TOAST_NAMESPACE));
    effects
}

/// The agents' rows, as the agents' group lists them.
fn rows_of(shown: &McpShown) -> Vec<AgentShown> {
    shown
        .agents
        .iter()
        .map(|row| AgentShown {
            id: ROW_BASE + row.id,
            state: match (&row.doing, shown.stopped) {
                (McpDoing::Acting(_), false) => "working",
                _ => "idle",
            }
            .to_owned(),
            since: 0,
            source: "mcp".to_owned(),
            window: 0,
            name: row.name.clone(),
            note: None,
        })
        .collect()
}

/// What row `id` of the agents' group says on its right.
pub fn row_word(id: u64) -> String {
    let words = Words::new(Lang::of_messages());
    kept(|k| {
        let stopped = k.shown.stopped;
        k.shown
            .agents
            .iter()
            .find(|r| ROW_BASE + r.id == id)
            .map(|r| words.doing(r, stopped))
            .unwrap_or_default()
    })
}

/// Whether row `id` is acting now (its row lit violet).
pub fn row_acting(id: u64) -> bool {
    kept(|k| {
        !k.shown.stopped
            && k.shown
                .agents
                .iter()
                .any(|r| ROW_BASE + r.id == id && matches!(r.doing, McpDoing::Acting(_)))
    })
}

fn tile_of(shown: &McpShown) -> Option<(String, u8)> {
    let show = shown.show.as_ref()?;
    match show.at {
        hocus_link::McpAt::Desktop { index, .. } => Some((show.output.clone(), index)),
        _ => None,
    }
}

/// The desktop of `output` an action is about to show or send a window
/// to: its tile ringed in the agent's colour.
pub fn tile(output: &str) -> Option<u8> {
    kept(|k| tile_of(&k.shown))
        .filter(|(on, _)| on == output)
        .map(|(_, index)| index)
}

/// The tag's or the toast's view, if surface `id` is theirs.
pub fn view(id: window::Id, glass: bool) -> Option<Element<'static, Message>> {
    let (tag, toast) = kept(|k| (k.tag.clone(), k.toast.clone()));
    if let Some((tag_id, show)) = tag
        && tag_id == id
    {
        return Some(tag::view(show));
    }
    if let Some((toast_id, toast)) = toast
        && toast_id == id
    {
        return Some(toast::view(id, toast, glass));
    }
    None
}

/// Whether the toast of request `request` still shows.
pub fn toast_shows(request: u64) -> bool {
    kept(|k| k.toast.as_ref().is_some_and(|(_, t)| t.request == request))
}

// --- Words ---------------------------------------------------------------------

/// What the agents' surfaces say, in the shell's language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Words {
    pub lang: Lang,
}

impl Words {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    /// What a request did, as one phrase without its subject: "rangé 3
    /// fenêtres et changé appearance.theme".
    pub fn summary(self, s: &McpSummary) -> String {
        let lang = self.lang;
        let mut parts = Vec::new();
        if s.moved > 0 {
            parts.push(txt::moved(lang, s.moved));
        }
        if s.focused > 0 {
            parts.push(txt::focused(lang, s.focused));
        }
        if let Some(n) = s.switched {
            parts.push(txt::switched(lang, n));
        }
        for name in &s.launched {
            parts.push(txt::launched(lang, name));
        }
        match s.settings.as_slice() {
            [] => {}
            [key] => parts.push(txt::setting(lang, key)),
            keys => parts.push(txt::settings(lang, keys.len())),
        }
        match parts.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{}{}{last}", rest.join(", "), txt::AND.get(lang)),
        }
    }

    /// The toast's sentence: "Claude a rangé 3 fenêtres".
    pub fn toast(self, toast: &hocus_link::McpToast) -> String {
        txt::did(
            self.lang,
            txt::Did {
                agent: &toast.agent,
                what: self.summary(&toast.summary),
            },
        )
    }

    /// The toast's second line: the request's first intent, and whether
    /// it can be undone.
    pub fn toast_under(self, toast: &hocus_link::McpToast) -> String {
        let quoted = txt::quoted(self.lang, &toast.intent);
        if toast.undo {
            quoted
        } else {
            format!("{quoted} · {}", txt::CANNOT_UNDO.get(self.lang))
        }
    }

    /// What an agent's row says on its right.
    pub fn doing(self, row: &McpRow, stopped: bool) -> String {
        let lang = self.lang;
        if stopped {
            return txt::STOPPED.get(lang).to_owned();
        }
        match &row.doing {
            McpDoing::Ready => txt::READY.get(lang).to_owned(),
            McpDoing::Acting(kind) => match kind {
                McpKind::Windows => txt::ACTING_WINDOWS,
                McpKind::Desktop => txt::ACTING_DESKTOP,
                McpKind::Launch => txt::ACTING_LAUNCH,
                McpKind::Setting => txt::ACTING_SETTING,
            }
            .get(lang)
            .to_owned(),
            McpDoing::Did { summary, at } => {
                let what = self.summary(summary);
                let first = what.split(", ").next().unwrap_or_default();
                let first = first.split(txt::AND.get(lang)).next().unwrap_or_default();
                txt::row_did(lang, txt::RowDid { at, what: first })
            }
        }
    }

    /// A journal's line: its first line ("Claude · rangé 3 fenêtres"),
    /// its second (where, or why it was refused), and whether it is a
    /// refusal.
    pub fn line(self, line: &hocus_link::McpLine) -> (String, String, bool) {
        let lang = self.lang;
        let first = |what: String| {
            txt::line(
                lang,
                txt::Line {
                    agent: &line.agent,
                    what,
                },
            )
        };
        match &line.said {
            McpSaid::Did {
                summary, desktop, ..
            } => {
                let place = if summary.settings.is_empty() || summary.moved > 0 {
                    desktop.map(|n| txt::desktop_n(lang, n)).unwrap_or_default()
                } else {
                    txt::IN_SETTINGS.get(lang).to_owned()
                };
                (first(self.summary(summary)), place, false)
            }
            McpSaid::Undone { summary } => (
                first(txt::undone(lang, self.summary(summary))),
                String::new(),
                false,
            ),
            McpSaid::Refused { tool, why } => (
                first(self.tool(tool).to_owned()),
                txt::refused(lang, self.why(why)),
                true,
            ),
        }
    }

    fn tool(self, tool: &str) -> &'static str {
        match tool {
            "focus_window" => txt::TOOL_FOCUS_WINDOW,
            "move_window" => txt::TOOL_MOVE_WINDOW,
            "switch_desktop" => txt::TOOL_SWITCH_DESKTOP,
            "launch" => txt::TOOL_LAUNCH,
            "set_setting" => txt::TOOL_SET_SETTING,
            _ => txt::TOOL_OTHER,
        }
        .get(self.lang)
    }

    fn why(self, why: &McpRefusal) -> &'static str {
        match why {
            McpRefusal::Unknown => txt::WHY_UNKNOWN,
            McpRefusal::ReadOnly => txt::WHY_READ_ONLY,
            McpRefusal::NoSettings => txt::WHY_NO_SETTINGS,
            McpRefusal::Locked => txt::WHY_LOCKED,
            McpRefusal::Asking => txt::WHY_ASKING,
            McpRefusal::Stopped => txt::WHY_STOPPED,
            McpRefusal::Forbidden => txt::WHY_FORBIDDEN,
        }
        .get(self.lang)
    }

    pub fn journal(self) -> &'static str {
        txt::JOURNAL.get(self.lang)
    }

    pub fn stop_all(self) -> &'static str {
        txt::STOP_ALL.get(self.lang)
    }

    pub fn go_on(self) -> &'static str {
        txt::GO_ON.get(self.lang)
    }

    pub fn today(self, stopped: bool) -> &'static str {
        if stopped {
            txt::STOPPED_NOTE.get(self.lang)
        } else {
            txt::TODAY.get(self.lang)
        }
    }

    pub fn nothing_yet(self) -> &'static str {
        txt::NOTHING_YET.get(self.lang)
    }

    pub fn undo(self) -> &'static str {
        txt::UNDO.get(self.lang)
    }

    pub fn see(self) -> &'static str {
        txt::SEE.get(self.lang)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_one_sentence() {
        let fr = Words::new(Lang::French);
        let summary = McpSummary {
            moved: 3,
            settings: vec!["appearance.theme".into()],
            ..McpSummary::default()
        };
        assert_eq!(
            fr.summary(&summary),
            "rangé 3 fenêtres et changé appearance.theme"
        );
        let more = McpSummary {
            moved: 1,
            switched: Some(2),
            launched: vec!["Firefox".into()],
            ..McpSummary::default()
        };
        assert_eq!(
            fr.summary(&more),
            "rangé 1 fenêtre, affiché le bureau 2 et ouvert Firefox"
        );
        let en = Words::new(Lang::English);
        assert_eq!(
            en.summary(&summary),
            "arranged 3 windows and changed appearance.theme"
        );
        let row = McpRow {
            id: 1,
            name: "Claude".into(),
            doing: McpDoing::Did {
                summary,
                at: "10:43".into(),
            },
        };
        assert_eq!(fr.doing(&row, false), "a rangé 3 fenêtres · 10:43");
        assert_eq!(fr.doing(&row, true), "arrêté");
    }
}
