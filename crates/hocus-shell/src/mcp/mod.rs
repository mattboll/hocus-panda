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
//! - **The pointers' names**: a tag beside each agent's pointer, which the
//!   compositor draws (its arrow and ring have no surface of the shell's:
//!   on the CPU the shell's surfaces have no alpha).
//! - **The card** ([`card`], scene 3) of an action waiting for the user.

pub mod card;
pub mod tag;
pub mod toast;

/// The kinds of the agents' surfaces of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// A tag, taking neither the pointer nor the keyboard.
    Tag,
    /// The toast, which takes the pointer for its buttons.
    Toast,
    /// The card, which takes the keyboard too once clicked.
    Card,
}

impl Surface {
    pub fn namespace(self) -> &'static str {
        match self {
            Self::Tag => hocus_link::MCP_TAG_NAMESPACE,
            Self::Toast => hocus_link::MCP_TOAST_NAMESPACE,
            Self::Card => hocus_link::MCP_CARD_NAMESPACE,
        }
    }
}

use std::sync::{Mutex, PoisonError};

use hocus_link::{
    AgentShown, McpAsk, McpDoing, McpKind, McpRefusal, McpRow, McpSaid, McpShown, McpSummary,
    McpWhy,
};
use hocus_words::{Lang, shell::mcp as txt};
use iced::{Element, window};

use crate::{
    Message,
    popover::{self, Effect, Item},
};

/// An MCP agent's row in the agents' group: its mark, 26 px, its name
/// over what it does, 8 px after it.
pub mod row {
    pub const MARK: f32 = 26.0;
    pub const GAP: f32 = 8.0;
    pub const NAME: f32 = 13.0;
    pub const WORD: f32 = 11.5;
}

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
    /// The resting pointers' tags, their names alone.
    pointers: Vec<(window::Id, hocus_link::McpShow)>,
    /// The card's surface and what it asks.
    card: Option<(window::Id, hocus_link::McpCard)>,
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
                    kind: Surface::Tag,
                });
                k.tag = Some((id, show.clone()));
            }
        }
        // The resting pointers' names: one surface each.
        let names: Vec<hocus_link::McpShow> = shown
            .pointers
            .iter()
            .map(|p| hocus_link::McpShow {
                agent: p.agent.clone(),
                intent: String::new(),
                output: p.output.clone(),
                at: hocus_link::McpAt::Pointer { x: p.x, y: p.y },
            })
            .collect();
        if k.pointers.iter().map(|(_, s)| s).ne(names.iter()) {
            for (id, _) in k.pointers.drain(..) {
                effects.push(Effect::Remove(id));
            }
            for show in names {
                let id = window::Id::unique();
                let (size, at) = tag::place(&show);
                effects.push(Effect::Agent {
                    id,
                    output: show.output.clone(),
                    size,
                    at,
                    kind: Surface::Tag,
                });
                k.pointers.push((id, show));
            }
        }
        // The card: one surface per action waiting.
        if k.card.as_ref().map(|(_, c)| c) != shown.card.as_ref() {
            if let Some((id, _)) = k.card.take() {
                effects.push(Effect::Remove(id));
            }
            if let Some(asked) = &shown.card {
                let id = window::Id::unique();
                let (size, at) = card::place(words, asked);
                tracing::debug!(
                    card = asked.id,
                    agent = asked.agent,
                    said = words.wants(asked),
                    "an MCP agent's card shows"
                );
                effects.push(Effect::Agent {
                    id,
                    output: asked.output.clone(),
                    size,
                    at,
                    kind: Surface::Card,
                });
                k.card = Some((id, asked.clone()));
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
                    kind: Surface::Toast,
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
    effects.push(Effect::Wake(hocus_link::MCP_CARD_NAMESPACE));
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
                (McpDoing::Asking, false) => "waiting",
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
    let (tag, toast, pointer, asked) = kept(|k| {
        (
            k.tag.clone(),
            k.toast.clone(),
            k.pointers.iter().find(|(p, _)| *p == id).cloned(),
            k.card.clone(),
        )
    });
    if let Some((tag_id, show)) = tag
        && tag_id == id
    {
        return Some(tag::view(show));
    }
    if let Some((_, show)) = pointer {
        return Some(tag::view(show));
    }
    if let Some((card_id, asked)) = asked
        && card_id == id
    {
        return Some(card::view(id, asked));
    }
    if let Some((toast_id, toast)) = toast
        && toast_id == id
    {
        return Some(toast::view(id, toast, glass));
    }
    None
}

/// Whether card `card` still waits.
pub fn card_shows(card: u64) -> bool {
    kept(|k| k.card.as_ref().is_some_and(|(_, c)| c.id == card))
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
        if s.clicked > 0 {
            parts.push(txt::clicked(lang, s.clicked));
        }
        if s.scrolled > 0 {
            parts.push(txt::SCROLLED.get(lang).to_owned());
        }
        if s.typed > 0 {
            parts.push(txt::typed(lang, s.typed));
        }
        if s.pressed > 0 {
            parts.push(txt::pressed(lang, s.pressed));
        }
        if s.closed > 0 {
            parts.push(txt::closed(lang, s.closed));
        }
        match s.settings.as_slice() {
            [] => {}
            sets @ ([_] | [_, _]) => parts.extend(sets.iter().map(|set| self.setting(set))),
            sets => parts.push(txt::settings(lang, sets.len())),
        }
        match parts.split_last() {
            None => String::new(),
            Some((last, [])) => last.clone(),
            Some((last, rest)) => format!("{}{}{last}", rest.join(", "), txt::AND.get(lang)),
        }
    }

    /// A setting changed, in the Settings' own words: its row's name, and
    /// its choice's label ("mis « Thème » sur « Sombre »"), on or off for
    /// a switch (`hocus_config::labels`, the Settings' rows); a key no row
    /// writes, "changé un réglage".
    pub fn setting(self, set: &hocus_link::McpSet) -> String {
        use hocus_config::labels::Said;
        let lang = self.lang;
        let Some(label) = hocus_config::labels::of(&set.key) else {
            return txt::CHANGED_A_SETTING.get(lang).to_owned();
        };
        let name = label.name.get(lang);
        match (label.said, label.value(&set.value, lang)) {
            (Said::Switch, _) => match set.value.as_str() {
                "true" => txt::turned_on(lang, name),
                "false" => txt::turned_off(lang, name),
                _ => txt::changed(lang, name),
            },
            (_, Some(value)) => txt::set_to(
                lang,
                txt::SetTo {
                    name,
                    value: &value,
                },
            ),
            (_, None) => txt::changed(lang, name),
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
            McpDoing::Asking => txt::ASKING.get(lang).to_owned(),
            McpDoing::Taken => txt::TAKEN.get(lang).to_owned(),
            McpDoing::Acting(kind) => match kind {
                McpKind::Windows => txt::ACTING_WINDOWS,
                McpKind::Desktop => txt::ACTING_DESKTOP,
                McpKind::Launch => txt::ACTING_LAUNCH,
                McpKind::Setting => txt::ACTING_SETTING,
                McpKind::Input => txt::ACTING_INPUT,
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
            "click" => txt::TOOL_CLICK,
            "scroll" => txt::TOOL_SCROLL,
            "type" => txt::TOOL_TYPE,
            "press_keys" => txt::TOOL_PRESS_KEYS,
            "close_window" => txt::TOOL_CLOSE_WINDOW,
            "accessibility_tree" | "wait_for" => txt::TOOL_READ_TREE,
            _ => txt::TOOL_OTHER,
        }
        .get(self.lang)
    }

    fn why(self, why: &McpRefusal) -> &'static str {
        match why {
            McpRefusal::Unknown => txt::WHY_UNKNOWN,
            McpRefusal::ReadOnly => txt::WHY_READ_ONLY,
            McpRefusal::NoSettings => txt::WHY_NO_SETTINGS,
            McpRefusal::NoWindows => txt::WHY_NO_WINDOWS,
            McpRefusal::NoDesktops => txt::WHY_NO_DESKTOPS,
            McpRefusal::NoInput => txt::WHY_NO_INPUT,
            McpRefusal::Locked => txt::WHY_LOCKED,
            McpRefusal::Asking => txt::WHY_ASKING,
            McpRefusal::Stopped => txt::WHY_STOPPED,
            McpRefusal::Forbidden => txt::WHY_FORBIDDEN,
            McpRefusal::NotItsApp => txt::WHY_NOT_ITS_APP,
            McpRefusal::X11 => txt::WHY_X11,
            McpRefusal::TakenOver => txt::WHY_TAKEN_OVER,
            McpRefusal::UserRefused => txt::WHY_USER_REFUSED,
            McpRefusal::NoAnswer => txt::WHY_NO_ANSWER,
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

    /// A card's head: "Claude veut fermer « Courses »".
    pub fn wants(self, card: &hocus_link::McpCard) -> String {
        let lang = self.lang;
        let agent = card.agent.as_str();
        match &card.ask {
            McpAsk::Click { title } => txt::wants_click(lang, txt::WantsClick { agent, title }),
            McpAsk::Type { title } => txt::wants_type(lang, txt::WantsType { agent, title }),
            McpAsk::Keys { title, keys } => {
                txt::wants_keys(lang, txt::WantsKeys { agent, keys, title })
            }
            McpAsk::Close { title } => txt::wants_close(lang, txt::WantsClose { agent, title }),
        }
    }

    /// Why the card asks.
    pub fn why_card(self, card: &hocus_link::McpCard) -> String {
        match &card.why {
            McpWhy::Password => txt::WHY_PASSWORD.get(self.lang).to_owned(),
            McpWhy::App { name } => txt::why_app(self.lang, name),
        }
    }

    /// Its first button: "Fermer quand même" for a close, else "Autoriser".
    pub fn allow(self, card: &hocus_link::McpCard) -> &'static str {
        match card.ask {
            McpAsk::Close { .. } => txt::CLOSE_ANYWAY,
            _ => txt::ALLOW,
        }
        .get(self.lang)
    }

    pub fn refuse(self) -> &'static str {
        txt::REFUSE.get(self.lang)
    }

    pub fn take_over(self) -> &'static str {
        txt::TAKE_OVER.get(self.lang)
    }

    pub fn to_confirm(self) -> &'static str {
        txt::TO_CONFIRM.get(self.lang)
    }

    /// The card's last line: its time, or its keys once it has them.
    pub fn card_note(self, keys: bool) -> &'static str {
        if keys {
            txt::CARD_KEYS
        } else {
            txt::NO_ANSWER_REFUSED
        }
        .get(self.lang)
    }

    pub fn hand_back(self) -> &'static str {
        txt::HAND_BACK.get(self.lang)
    }

    pub fn taken_note(self, agent: &str) -> String {
        txt::taken_note(self.lang, agent)
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
            settings: vec![hocus_link::McpSet {
                key: "appearance.theme".into(),
                value: "dark".into(),
            }],
            ..McpSummary::default()
        };
        assert_eq!(
            fr.summary(&summary),
            "rangé 3 fenêtres et mis « Thème » sur « Sombre »"
        );
        let set = |key: &str, value: &str| hocus_link::McpSet {
            key: key.into(),
            value: value.into(),
        };
        assert_eq!(
            fr.setting(&set("appearance.reduced-motion", "true")),
            "activé « Mouvements réduits »"
        );
        assert_eq!(
            fr.setting(&set("idle.delay", "600")),
            "mis « Assombrir l’écran après » sur « 10 min »"
        );
        // Any row's key, by its row's words (lot A's table had nine).
        assert_eq!(
            fr.setting(&set("pointer.acceleration", "flat")),
            "mis « Accélération » sur « Aucune »"
        );
        assert_eq!(
            fr.setting(&set("no.such-key", "false")),
            "changé un réglage"
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
            "arranged 3 windows and set “Theme” to “Dark”"
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
