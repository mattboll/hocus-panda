//! A notification as the server takes it (the Desktop Notifications
//! Specification 1.2): what its bubble shows, how long it stays, and
//! whether it is only an agent's tool saying that the agent waits or has
//! finished, which the agents' group already shows (5.3).

use std::time::Duration;

use crate::levels::icons::Icon;

/// How long a normal notification's bubble stays, unless its application
/// says otherwise.
pub const LIFE: Duration = Duration::from_secs(6);

/// A notification.
#[derive(Debug, Clone)]
pub struct Note {
    /// The server's id for it, as `Notify` returned it.
    pub id: u32,
    /// The application's name, as it gives it (`app_name`).
    pub app: String,
    /// Its desktop entry (the `desktop-entry` hint), without `.desktop`.
    pub entry: Option<String>,
    /// The icon found for it: its image, its icon's file or name, else its
    /// desktop entry's.
    pub icon: Option<Icon>,
    pub summary: String,
    /// The body, its markup taken out.
    pub body: String,
    /// The actions offered as buttons, by their key and their label;
    /// `default` is not one of them.
    pub actions: Vec<(String, String)>,
    /// It has a `default` action: a click on the card invokes it.
    pub default: bool,
    pub urgency: Urgency,
    pub timeout: Timeout,
    /// The `value` hint: a progress, in percent.
    pub progress: Option<u8>,
    /// The `resident` hint: an action does not close it.
    pub resident: bool,
    /// The process that sent it (0: not known).
    pub pid: u32,
    /// An agent's tool saying only that the agent waits or has finished:
    /// to the history, no bubble.
    pub agent: bool,
    /// What its icon was found by, to find it again for the history after
    /// a restart (an image sent as pixels is not kept).
    pub icon_from: IconFrom,
    /// When it came, on the wall clock (milliseconds since the epoch): the
    /// history says how long ago, and keeps it 7 days.
    pub at: i64,
}

/// What a notification's icon is looked for by, besides its application's
/// name and desktop entry: its `app_icon`, and its `image-path` hint.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconFrom {
    pub app_icon: String,
    pub image_path: Option<String>,
}

/// The `urgency` hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    /// The hint's byte (0 low, 1 normal, 2 critical); normal otherwise.
    pub fn of(byte: Option<u8>) -> Self {
        match byte {
            Some(0) => Self::Low,
            Some(2) => Self::Critical,
            _ => Self::Normal,
        }
    }
}

/// How long a bubble stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeout {
    /// Until the user or the application closes it.
    Never,
    After(Duration),
}

impl Timeout {
    /// What `expire_timeout` asks (milliseconds; -1: the server's choice,
    /// 0: never) for a notification of `urgency`: a critical one stays.
    pub fn of(expire: i32, urgency: Urgency) -> Self {
        match (urgency, expire) {
            (Urgency::Critical, _) | (_, 0) => Self::Never,
            (_, ms) if ms > 0 => Self::After(Duration::from_millis(ms.unsigned_abs().into())),
            _ => Self::After(LIFE),
        }
    }
}

/// The actions' list as `Notify` gives it (key, label, key, label…):
/// the buttons, and whether a `default` is there. An action without a
/// label shows its key.
pub fn actions(list: &[String]) -> (Vec<(String, String)>, bool) {
    let mut buttons = Vec::new();
    let mut default = false;
    for pair in list.chunks(2) {
        let key = pair[0].clone();
        let label = pair.get(1).cloned().unwrap_or_default();
        if key == "default" {
            default = true;
            continue;
        }
        let label = if label.trim().is_empty() {
            key.clone()
        } else {
            label
        };
        buttons.push((key, label));
    }
    (buttons, default)
}

/// `body` without its markup (the specification's `<b>`, `<i>`, `<u>`,
/// `<a>`, `<img>`), its entities read: the bubbles show plain text.
pub fn plain(body: &str) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(at) = rest.find(['<', '&']) {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        if rest.starts_with('<') {
            match rest.find('>') {
                Some(end) => {
                    // An image says what it shows, if it does.
                    let tag = &rest[..end];
                    if tag.starts_with("<img")
                        && let Some(alt) = attribute(tag, "alt")
                    {
                        out.push_str(&alt);
                    }
                    rest = &rest[end + 1..];
                }
                None => {
                    out.push_str(rest);
                    rest = "";
                }
            }
        } else {
            let (text, used) = entity(rest);
            out.push_str(&text);
            rest = &rest[used..];
        }
    }
    out.push_str(rest);
    out
}

/// An attribute's value in a tag.
fn attribute(tag: &str, name: &str) -> Option<String> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let end = tag[at..].find('"')?;
    Some(plain(&tag[at..at + end]))
}

/// The entity `text` starts with: what it stands for and its length; a
/// lone `&` stays one.
fn entity(text: &str) -> (String, usize) {
    let Some(end) = text.find(';').filter(|e| *e <= 10) else {
        return ("&".to_owned(), 1);
    };
    let name = &text[1..end];
    let read = match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{a0}'),
        _ => name
            .strip_prefix("#x")
            .or_else(|| name.strip_prefix("#X"))
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32),
    };
    match read {
        Some(c) => (c.to_string(), end + 1),
        None => ("&".to_owned(), 1),
    }
}

// --- The agents' tools' notifications ------------------------------------------

/// An agents' tool the agents' group follows (4.0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    ClaudeCode,
    Codex,
    Herdr,
}

impl Tool {
    const ALL: [Self; 3] = [Self::ClaudeCode, Self::Codex, Self::Herdr];

    /// The compositor's word for it.
    pub const fn word(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Herdr => "herdr",
        }
    }

    /// The names it sends its notifications under (its name as the
    /// application's, its desktop entry, a title: Claude Code titles the
    /// ones its terminal shows "Claude Code").
    fn names(self) -> &'static [&'static str] {
        match self {
            Self::ClaudeCode => &["claude code", "claude", "claude-code"],
            Self::Codex => &["codex"],
            Self::Herdr => &["herdr"],
        }
    }

    /// Its program's name, as `/proc/PID/comm` has it.
    fn program(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Herdr => "herdr",
        }
    }
}

/// What a notification's sender is known by, to tell an agents' tool's.
#[derive(Debug, Clone, Default)]
pub struct Sender<'a> {
    pub app: &'a str,
    pub entry: Option<&'a str>,
    pub summary: &'a str,
    pub body: &'a str,
    /// The programs' names of the process that sent it and of its parents
    /// (`notify-send` run by a hook, by herdr).
    pub programs: &'a [String],
}

/// Whether a notification is a followed agents' tool's saying only that
/// its agent waits for the user or has finished: the agent's row, its
/// panda and the halo show it already (5.3). `followed` says whether the
/// agents' group follows a tool.
pub fn agent_state(sender: &Sender<'_>, followed: impl Fn(Tool) -> bool) -> bool {
    let from = |tool: Tool| {
        let named = |text: &str| {
            let text = text.trim().to_lowercase();
            tool.names().contains(&text.as_str())
        };
        named(sender.app)
            || sender.entry.is_some_and(named)
            || named(sender.summary)
            || sender.programs.iter().any(|p| p == tool.program())
    };
    Tool::ALL.into_iter().any(|t| followed(t) && from(t))
        && says_waits_or_done(&format!("{} {}", sender.summary, sender.body))
}

/// Whether `text` says an agent waits for the user or has finished, as the
/// tools word it ("Claude is waiting for your input", "Claude needs your
/// permission to use Bash", "refacto finished", Codex's "Agent turn
/// complete" and "Approval requested", herdr's "needs attention"); not a
/// failure.
fn says_waits_or_done(text: &str) -> bool {
    let text = text.trim().to_lowercase();
    if text.contains("failed") || text.contains("error") {
        return false;
    }
    const WAITS: [&str; 9] = [
        "waiting for your input",
        "is waiting",
        "needs your permission",
        "needs permission",
        "needs your input",
        "needs your attention",
        "needs attention",
        "approval requested",
        "wants to edit",
    ];
    const DONE: [&str; 5] = [
        "agent turn complete",
        "turn complete",
        "has finished",
        "is done",
        "finished",
    ];
    WAITS.iter().chain(&DONE).any(|said| text.contains(said))
}

/// The programs' names of process `pid` and of its parents, nearest first,
/// read in `/proc` (a few small files: the server's thread, never the one
/// that draws).
pub fn programs_above(pid: u32) -> Vec<String> {
    let mut names = Vec::new();
    let mut at = pid;
    for _ in 0..16 {
        if at <= 1 {
            break;
        }
        let Ok(stat) = std::fs::read_to_string(format!("/proc/{at}/stat")) else {
            break;
        };
        // `PID (COMM) STATE PPID …`: the name may hold spaces and
        // parentheses, the last `)` ends it.
        let (Some(open), Some(close)) = (stat.find('('), stat.rfind(')')) else {
            break;
        };
        names.push(stat[open + 1..close].to_owned());
        let parent = stat[close + 1..]
            .split_whitespace()
            .nth(1)
            .and_then(|p| p.parse::<u32>().ok());
        match parent {
            Some(parent) if parent != at => at = parent,
            _ => break,
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_normal_one_stays_six_seconds_or_its_own_time_a_critical_one_stays() {
        assert_eq!(Timeout::of(-1, Urgency::Normal), Timeout::After(LIFE));
        assert_eq!(
            Timeout::of(2500, Urgency::Normal),
            Timeout::After(Duration::from_millis(2500))
        );
        assert_eq!(Timeout::of(0, Urgency::Normal), Timeout::Never);
        assert_eq!(Timeout::of(-1, Urgency::Critical), Timeout::Never);
        assert_eq!(Timeout::of(3000, Urgency::Critical), Timeout::Never);
        assert_eq!(Urgency::of(Some(0)), Urgency::Low);
        assert_eq!(Urgency::of(Some(2)), Urgency::Critical);
        assert_eq!(Urgency::of(None), Urgency::Normal);
    }

    #[test]
    fn the_default_action_is_the_cards_and_the_others_are_buttons() {
        let list: Vec<String> = ["default", "", "reply", "Répondre", "read", ""]
            .map(str::to_owned)
            .into();
        let (buttons, default) = actions(&list);
        assert!(default);
        assert_eq!(
            buttons,
            [
                ("reply".to_owned(), "Répondre".to_owned()),
                ("read".to_owned(), "read".to_owned())
            ]
        );
        assert_eq!(actions(&[]), (Vec::new(), false));
    }

    #[test]
    fn markup_is_taken_out_and_entities_read() {
        assert_eq!(
            plain(
                "<b>Camille</b> &amp; <i>Hugo</i> : 3 &lt; 4 &#233;t&#xE9; <a href=\"x\">lien</a>"
            ),
            "Camille & Hugo : 3 < 4 été lien"
        );
        assert_eq!(plain("<img src=\"a.png\" alt=\"un chat\"/>"), "un chat");
        assert_eq!(plain("AT&T, 1 < 2"), "AT&T, 1 < 2");
    }

    #[test]
    fn an_agents_tool_saying_it_waits_or_is_done_makes_no_bubble() {
        let claude = |tool: Tool| tool == Tool::ClaudeCode;
        fn said<'a>(app: &'a str, summary: &'a str, body: &'a str) -> Sender<'a> {
            Sender {
                app,
                summary,
                body,
                ..Sender::default()
            }
        }
        // Its terminal shows Claude Code's notifications under its title.
        assert!(agent_state(
            &said("foot", "Claude Code", "Claude is waiting for your input"),
            claude
        ));
        assert!(agent_state(
            &said(
                "foot",
                "Claude Code",
                "Claude needs your permission to use Bash"
            ),
            claude
        ));
        assert!(agent_state(
            &said("claude", "refacto", "refacto finished"),
            claude
        ));
        // A hook's notify-send, under Claude Code's process.
        let programs = [
            "notify-send".to_owned(),
            "sh".to_owned(),
            "claude".to_owned(),
        ];
        let hook = Sender {
            app: "notify-send",
            summary: "hocus-panda",
            body: "Claude is waiting for your input",
            programs: &programs,
            ..Sender::default()
        };
        assert!(agent_state(&hook, claude));
        // What else it says is a bubble, and so is a tool not followed, or
        // another application saying it is done.
        assert!(!agent_state(
            &said("foot", "Claude Code", "refacto failed"),
            claude
        ));
        assert!(!agent_state(
            &said("foot", "Claude Code", "Login successful"),
            claude
        ));
        assert!(!agent_state(
            &said("foot", "Claude Code", "Claude is waiting for your input"),
            |_| false
        ));
        assert!(!agent_state(
            &said("Firefox", "Téléchargement terminé", "Download finished"),
            claude
        ));
        let codex = |tool: Tool| tool == Tool::Codex;
        assert!(agent_state(
            &said("Codex", "Codex", "Agent turn complete"),
            codex
        ));
        let herdr = |tool: Tool| tool == Tool::Herdr;
        assert!(agent_state(
            &said("herdr", "refacto", "needs attention"),
            herdr
        ));
    }

    #[test]
    fn this_process_and_its_parents_are_named() {
        let names = programs_above(std::process::id());
        assert!(!names.is_empty(), "{names:?}");
    }
}
