//! An agent's conversation, kept with a project and resumed as it reopens
//! (docs/design/projects.md §8.1, lot D of #789).
//!
//! - **Kept**: what its hooks said (`hocus agent-hook`'s `cwd`,
//!   `transcript` and first prompt, docs/agents.md), with its tool and
//!   session's id, as `[window.agent]` beside the window of the terminal it
//!   ran in. Never logged: [`SavedAgent`]'s `Debug` says its tool alone.
//! - **Resumed**: the terminal opens in its folder running the tool's
//!   command through the user's shell, so that leaving the agent leaves a
//!   shell there (`$SHELL -c 'claude --resume ID; exec $SHELL'`); the id
//!   is checked first (letters, digits and `-`). Its transcript gone, or
//!   its program not on the `PATH`: the terminal opens in the folder
//!   alone, and the notification says the conversation could not resume.
//! - **Codex**: its hooks' `session_id` is not documented as the id `codex
//!   resume` takes; the id in its transcript's file name
//!   (`rollout-…-ID.jsonl`) is, so that one is resumed when it reads.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// An agent's conversation as a project keeps it (§6, §9).
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SavedAgent {
    /// `claude-code` or `codex`.
    pub tool: String,
    /// Its session's id, as its hooks say it.
    pub session: String,
    pub folder: PathBuf,
    pub transcript: PathBuf,
    /// The first prompt of its session, cut at 60 characters (§4).
    #[serde(skip_serializing_if = "String::is_empty")]
    pub first_prompt: String,
}

impl std::fmt::Debug for SavedAgent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SavedAgent")
            .field("tool", &self.tool)
            .finish_non_exhaustive()
    }
}

impl SavedAgent {
    /// Its tool as people know it.
    pub fn title(&self) -> &str {
        tool_title(&self.tool)
    }

    /// The id its tool resumes: Claude Code's session's; Codex's in its
    /// transcript's file name when it has one, else its session's. None
    /// when it is not letters, digits and `-`.
    pub fn resume_id(&self) -> Option<String> {
        let from_file = (self.tool == "codex")
            .then(|| codex_id(&self.transcript))
            .flatten();
        from_file
            .or_else(|| Some(self.session.clone()))
            .filter(|id| id_ok(id))
    }

    /// Its tool's command resuming it (`claude --resume ID`).
    pub fn command_line(&self) -> Option<String> {
        let id = self.resume_id()?;
        match self.tool.as_str() {
            "claude-code" => Some(format!("claude --resume {id}")),
            "codex" => Some(format!("codex resume {id}")),
            _ => None,
        }
    }

    /// Its tool's program.
    pub fn program(&self) -> Option<&'static str> {
        match self.tool.as_str() {
            "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            _ => None,
        }
    }

    /// What its terminal runs, through `shell`: its command, then a shell
    /// in its folder.
    pub fn command(&self, shell: &str) -> Option<Vec<String>> {
        let line = self.command_line()?;
        Some(vec![
            shell.to_owned(),
            "-c".to_owned(),
            format!("{line}; exec $SHELL"),
        ])
    }

    /// Whether it can resume, its programs found in `path`: its id reads,
    /// its transcript is there, its program too. Looked at off the loop.
    pub fn resumable(&self, path: &[PathBuf]) -> bool {
        self.command_line().is_some()
            && self.transcript.is_file()
            && self.program().is_some_and(|p| on_path(p, path))
    }
}

/// A tool as people know it.
pub fn tool_title(tool: &str) -> &str {
    match tool {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        other => other,
    }
}

/// Whether an id may be given on a command line: letters, digits and `-`,
/// 128 at most.
pub fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && !id.starts_with('-')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The id at the end of a Codex transcript's name (`rollout-2026-11-13T09-12-00-ID.jsonl`,
/// its id a UUID).
fn codex_id(transcript: &Path) -> Option<String> {
    let stem = transcript.file_stem()?.to_str()?;
    let at = stem.len().checked_sub(36)?;
    let id = stem.get(at..)?;
    let uuid = id.split('-').map(str::len).collect::<Vec<_>>() == [8, 4, 4, 4, 12]
        && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    uuid.then(|| id.to_owned())
}

/// Whether `program` is in one of `path`'s folders, runnable.
pub fn on_path(program: &str, path: &[PathBuf]) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.iter().any(|dir| {
        std::fs::metadata(dir.join(program))
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    })
}

/// The `PATH`'s folders.
pub fn path_of(var: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    var.map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(tool: &str, session: &str, transcript: &str) -> SavedAgent {
        SavedAgent {
            tool: tool.into(),
            session: session.into(),
            folder: "/home/alex/site".into(),
            transcript: transcript.into(),
            first_prompt: "refais l'accueil".into(),
        }
    }

    #[test]
    fn an_id_is_letters_digits_and_dashes() {
        assert!(id_ok("3f6c2a1e-9b0d-4c55-8e21-7d0a94c1b2e6"));
        for bad in [
            "",
            "-rf",
            "a b",
            "a;rm",
            "$(x)",
            "a'b",
            "../x",
            "a".repeat(129).as_str(),
        ] {
            assert!(!id_ok(bad), "{bad}");
        }
        let claude = agent("claude-code", "3f6c-9b0d", "/x/3f6c-9b0d.jsonl");
        assert_eq!(
            claude.command("/bin/zsh"),
            Some(vec![
                "/bin/zsh".to_owned(),
                "-c".to_owned(),
                "claude --resume 3f6c-9b0d; exec $SHELL".to_owned()
            ])
        );
        assert_eq!(
            agent("claude-code", "x; rm -rf ~", "").command("/bin/sh"),
            None
        );
        assert_eq!(
            format!("{claude:?}"),
            "SavedAgent { tool: \"claude-code\", .. }"
        );
    }

    #[test]
    fn codex_resumes_the_id_of_its_transcripts_name() {
        let codex = agent(
            "codex",
            "thr_123",
            "/home/alex/.codex/sessions/2026/11/13/rollout-2026-11-13T09-12-00-0199a213-81c0-7800-8aa1-bbab2a035a53.jsonl",
        );
        assert_eq!(
            codex.command_line().as_deref(),
            Some("codex resume 0199a213-81c0-7800-8aa1-bbab2a035a53")
        );
        // Its name says none: its session's, when it reads.
        let bare = agent("codex", "0199a213-81c0", "/tmp/rollout.jsonl");
        assert_eq!(
            bare.command_line().as_deref(),
            Some("codex resume 0199a213-81c0")
        );
        assert_eq!(
            agent("codex", "thr_123", "/tmp/rollout.jsonl").command_line(),
            None
        );
    }

    #[test]
    fn it_resumes_only_with_its_transcript_and_its_program() -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir()?;
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin)?;
        let transcript = dir.path().join("3f6c.jsonl");
        std::fs::write(&transcript, "{}\n")?;
        let claude = agent("claude-code", "3f6c", &transcript.display().to_string());
        assert!(!claude.resumable(std::slice::from_ref(&bin)), "no claude");
        std::fs::write(bin.join("claude"), "#!/bin/sh\n")?;
        std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755))?;
        assert!(claude.resumable(std::slice::from_ref(&bin)));
        std::fs::remove_file(&transcript)?;
        assert!(!claude.resumable(&[bin]), "its transcript gone");
        Ok(())
    }
}
