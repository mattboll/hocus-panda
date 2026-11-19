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
//!   is checked first (letters, digits and `-`). Its program is looked
//!   for where the agent's own process found it (its `PATH`, read as its
//!   hooks are heard), then on the session's `PATH`, then in the home's
//!   usual folders (`~/.local/bin`, where Claude Code's installer puts
//!   it, …): a session started by a display manager has a `PATH` of its
//!   own, without the folders the user's shell adds. Found off the
//!   session's `PATH`, its folder goes first on the command's. Its
//!   transcript gone, or its program nowhere: the terminal opens in the
//!   folder alone, and the notification says why and what to type.
//! - **Codex**: its hooks' `session_id` is not documented as the id `codex
//!   resume` takes; the id in its transcript's file name
//!   (`rollout-…-ID.jsonl`) is, so that one is resumed when it reads.
//! - **From a notification** (#823, notifications.md "An agent's
//!   conversation"): a click on the notification of an agent with no window
//!   opens the user's terminal on its conversation the same way.

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
    /// Its tool's program as its process found it (`~/.local/bin/claude`),
    /// for a session whose `PATH` does not have it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<PathBuf>,
}

/// Where an agent's program was found to resume it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// On the session's `PATH`: its command as it is.
    OnPath,
    /// In this folder, put first on its command's `PATH`.
    In(PathBuf),
}

/// Why an agent's conversation cannot resume (§8.1), as the notification
/// says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotResumed {
    /// Its transcript's file is gone: nothing to resume.
    Transcript,
    /// Its program is nowhere it was looked for.
    Program,
    /// Its folder is gone, its terminal cannot be given a command, or it
    /// was not heard back.
    Other,
}

/// The home's folders where programs installed for one user usually are,
/// besides the `PATH`: those of Claude Code's and Codex's installers and
/// of the usual package managers. nvm's are each of its Node versions'.
fn home_folders(home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = [
        ".local/bin",
        "bin",
        ".npm-global/bin",
        ".bun/bin",
        ".volta/bin",
        ".cargo/bin",
    ]
    .iter()
    .map(|d| home.join(d))
    .collect();
    if let Ok(versions) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        let mut nvm: Vec<PathBuf> = versions.flatten().map(|v| v.path().join("bin")).collect();
        // The newest version's first, as its name says it.
        nvm.sort();
        nvm.reverse();
        out.extend(nvm);
    }
    out
}

/// `program` as process `pid` finds it: in its own `PATH`
/// (`processes`/PID/environ, `processes` being `/proc`). Read off the
/// loop, as its hooks are heard.
pub fn program_of_process(processes: &Path, pid: u32, program: &str) -> Option<PathBuf> {
    let environ = std::fs::read(processes.join(pid.to_string()).join("environ")).ok()?;
    let path = environ
        .split(|b| *b == 0)
        .find_map(|var| var.strip_prefix(b"PATH="))?;
    let path = std::ffi::OsStr::new(std::str::from_utf8(path).ok()?);
    std::env::split_paths(path)
        .filter(|d| d.is_absolute())
        .map(|d| d.join(program))
        .find(|p| runnable(p))
}

/// `word` quoted for a POSIX shell.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
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
        Self::program_for(&self.tool)
    }

    /// Its tool's program for the name its tool goes by.
    pub fn program_for(tool: &str) -> Option<&'static str> {
        match tool {
            "claude-code" => Some("claude"),
            "codex" => Some("codex"),
            _ => None,
        }
    }

    /// What its terminal runs, through `shell`, its program `found`: its
    /// command, then a shell in its folder.
    pub fn command(&self, shell: &str, found: &Found) -> Option<Vec<String>> {
        let line = self.command_line()?;
        let line = match found {
            Found::OnPath => line,
            Found::In(dir) => {
                format!("PATH={}:\"$PATH\" {line}", quoted(&dir.to_string_lossy()))
            }
        };
        Some(vec![
            shell.to_owned(),
            "-c".to_owned(),
            format!("{line}; exec $SHELL"),
        ])
    }

    /// Where its program is: where its process found it, else in `path`
    /// (the session's), else in `home`'s usual folders. Looked at off the
    /// loop.
    pub fn found(&self, path: &[PathBuf], home: Option<&Path>) -> Option<Found> {
        let program = self.program()?;
        let named = |p: &Path| p.file_name().is_some_and(|n| n == program);
        let recorded = self
            .program
            .as_deref()
            .filter(|p| p.is_absolute() && named(p) && runnable(p))
            .and_then(Path::parent);
        if let Some(dir) = recorded {
            return Some(if path.iter().any(|d| d == dir) {
                Found::OnPath
            } else {
                Found::In(dir.to_path_buf())
            });
        }
        if on_path(program, path) {
            return Some(Found::OnPath);
        }
        home.into_iter()
            .flat_map(home_folders)
            .find(|d| runnable(&d.join(program)))
            .map(Found::In)
    }

    /// Whether it can resume, and its program's place: its id reads, its
    /// transcript is there, its program is found ([`Self::found`]). Looked
    /// at off the loop.
    pub fn resumable(&self, path: &[PathBuf], home: Option<&Path>) -> Result<Found, NotResumed> {
        if self.command_line().is_none() {
            return Err(NotResumed::Other);
        }
        if !self.transcript.is_file() {
            return Err(NotResumed::Transcript);
        }
        self.found(path, home).ok_or(NotResumed::Program)
    }
}

/// What starts the user's terminal (`configured`, else the first found on
/// `PATH`, `programs::terminal`) in `agent`'s folder resuming its
/// conversation through `shell`: `None` when its folder or transcript is
/// gone, its program is not in `path`, or the terminal is not one §8.2's
/// table can give a command. Looked at off the loop.
pub fn in_a_terminal(
    agent: &SavedAgent,
    configured: &str,
    terminal_path: Option<&std::ffi::OsStr>,
    path: &[PathBuf],
    home: Option<&Path>,
    shell: &str,
) -> Option<hocus_link::Launch> {
    if !agent.folder.is_dir() {
        return None;
    }
    let found = agent.resumable(path, home).ok()?;
    let terminal = crate::programs::terminal(configured, terminal_path)?;
    let words: Vec<String> = terminal.split_whitespace().map(str::to_owned).collect();
    let app = crate::terminals::app_of_program(words.first()?).to_owned();
    let mut argv = crate::terminals::in_folder(&app, &words, &agent.folder);
    argv.extend(crate::terminals::with_command(
        &app,
        &agent.command(shell, &found)?,
    )?);
    Some(hocus_link::Launch {
        id: app,
        terminal: false,
        dir: agent.folder.display().to_string(),
        argv,
    })
}

impl crate::State {
    /// A notification of an agent's conversation was clicked (#823): a
    /// window of its application, its sender or its agent is brought, as a
    /// notification's click does; without one, the user's terminal opens on
    /// the conversation, resumed in its folder, off the loop. It cannot
    /// resume: nothing more, as before.
    pub(crate) fn converse(
        &mut self,
        pid: u32,
        app: &str,
        conversation: hocus_link::AgentConversation,
    ) {
        let window = self
            .window_of_app(app)
            .or_else(|| self.window_of_process(pid))
            .or_else(|| self.window_of_process(conversation.pid));
        if let Some(id) = window {
            tracing::debug!(
                app,
                pid,
                window = id.0,
                "an agent's notification brings its window"
            );
            self.window_asked(id.0);
            return;
        }
        let agent = SavedAgent {
            tool: conversation.tool,
            session: conversation.session,
            folder: conversation.folder,
            transcript: conversation.transcript,
            first_prompt: String::new(),
            program: None,
        };
        let configured = self.config.programs.terminal.clone();
        let path = self.config.agent_places.as_ref().map_or_else(
            || path_of(std::env::var_os("PATH").as_deref()),
            |p| p.path.clone(),
        );
        let home = self.config.home.clone();
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| s.starts_with('/'))
            .unwrap_or_else(|| "/bin/sh".to_owned());
        let env = crate::programs::Env::of(self).with_activation(self.activation_token());
        crate::programs::on_a_thread("an agent's conversation".to_owned(), move || {
            let terminal_path = std::env::var_os("PATH");
            match in_a_terminal(
                &agent,
                &configured,
                terminal_path.as_deref(),
                &path,
                home.as_deref(),
                &shell,
            ) {
                Some(launch) => {
                    tracing::info!(
                        tool = agent.tool,
                        "an agent's conversation opens in a terminal"
                    );
                    crate::programs::launch(launch, None, env);
                }
                None => tracing::info!(
                    tool = agent.tool,
                    "an agent's conversation cannot resume: its notification opens nothing"
                ),
            }
        });
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
    path.iter().any(|dir| runnable(&dir.join(program)))
}

/// Whether `file` is a file that runs.
fn runnable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
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
            program: None,
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
            claude.command("/bin/zsh", &Found::OnPath),
            Some(vec![
                "/bin/zsh".to_owned(),
                "-c".to_owned(),
                "claude --resume 3f6c-9b0d; exec $SHELL".to_owned()
            ])
        );
        assert_eq!(
            agent("claude-code", "x; rm -rf ~", "").command("/bin/sh", &Found::OnPath),
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
        assert_eq!(
            claude.resumable(std::slice::from_ref(&bin), None),
            Err(NotResumed::Program)
        );
        std::fs::write(bin.join("claude"), "#!/bin/sh\n")?;
        std::fs::set_permissions(bin.join("claude"), std::fs::Permissions::from_mode(0o755))?;
        assert_eq!(
            claude.resumable(std::slice::from_ref(&bin), None),
            Ok(Found::OnPath)
        );
        std::fs::remove_file(&transcript)?;
        assert_eq!(claude.resumable(&[bin], None), Err(NotResumed::Transcript));
        Ok(())
    }

    fn runnable_file(file: &Path) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(file.parent().unwrap_or(Path::new("/")))?;
        std::fs::write(file, "#!/bin/sh\n")?;
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o755))
    }

    /// A session's `PATH` from a display manager lacks `~/.local/bin`,
    /// where Claude Code's installer puts `claude`: found there all the
    /// same, or where its process found it, its folder first on the
    /// command's `PATH`.
    #[test]
    fn its_program_is_found_off_the_sessions_path() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let home = dir.path().join("home");
        let session_path = vec![dir.path().join("usr/bin")];
        let transcript = dir.path().join("3f6c.jsonl");
        std::fs::write(&transcript, "{}\n")?;
        let mut claude = agent("claude-code", "3f6c", &transcript.display().to_string());
        assert_eq!(
            claude.resumable(&session_path, Some(&home)),
            Err(NotResumed::Program)
        );
        // Where its installer puts it.
        runnable_file(&home.join(".local/bin/claude"))?;
        let local = Found::In(home.join(".local/bin"));
        assert_eq!(
            claude.resumable(&session_path, Some(&home)),
            Ok(local.clone())
        );
        assert_eq!(
            claude.command("/bin/zsh", &local),
            Some(vec![
                "/bin/zsh".to_owned(),
                "-c".to_owned(),
                format!(
                    "PATH='{}':\"$PATH\" claude --resume 3f6c; exec $SHELL",
                    home.join(".local/bin").display()
                )
            ])
        );
        // Where its own process found it, first.
        let elsewhere = dir.path().join("opt/it's/bin/claude");
        runnable_file(&elsewhere)?;
        claude.program = Some(elsewhere.clone());
        let found = Found::In(dir.path().join("opt/it's/bin"));
        assert_eq!(
            claude.resumable(&session_path, Some(&home)),
            Ok(found.clone())
        );
        let line = claude.command("/bin/sh", &found).unwrap_or_default();
        assert!(
            line[2].starts_with(&format!(
                "PATH='{}/opt/it'\\''s/bin':",
                dir.path().display()
            )),
            "{line:?}"
        );
        // Gone since: the next place.
        std::fs::remove_file(&elsewhere)?;
        assert_eq!(claude.resumable(&session_path, Some(&home)), Ok(local));
        Ok(())
    }

    #[test]
    fn a_process_finds_its_program_in_its_own_path() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let proc = dir.path().join("proc");
        let bin = dir.path().join("home/.local/bin");
        runnable_file(&bin.join("claude"))?;
        std::fs::create_dir_all(proc.join("42"))?;
        std::fs::write(
            proc.join("42/environ"),
            format!(
                "HOME=/h\0PATH=/usr/bin:relative:{}\0TERM=x\0",
                bin.display()
            ),
        )?;
        assert_eq!(
            program_of_process(&proc, 42, "claude"),
            Some(bin.join("claude"))
        );
        assert_eq!(program_of_process(&proc, 42, "codex"), None);
        assert_eq!(program_of_process(&proc, 43, "claude"), None);
        Ok(())
    }
}
