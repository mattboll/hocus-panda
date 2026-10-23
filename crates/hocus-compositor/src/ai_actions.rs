//! What an AI's answer proposes (#197), offered under it and run once the
//! user confirms: the shell reads and checks the proposals
//! (`hocus_shell::proposals`); here they meet the desktop. A configuration
//! change is shown against the file as last read and written on a worker
//! thread by the settings app's own writer, so the file's comments stay;
//! a question handed to an agent goes to its herdr and into the agents'
//! journal (#176). While agents are stopped, nothing is offered, and
//! nothing offered before runs.

use hocus_shell::{
    Lang,
    proposals::{self, Context, Offer, Run},
};

use crate::{State, ai::News};

impl State {
    /// What the answer `text` proposes, as it can be offered now: `prompt`
    /// is what an agent would get.
    pub(crate) fn proposal_offers(&self, text: &str, prompt: &str) -> Vec<Offer> {
        let lang = self.shell_lang();
        let apps = self
            .launcher
            .as_ref()
            .map(|p| p.launcher.apps())
            .unwrap_or_default();
        let found = proposals::read(text, apps);
        if found.is_empty() {
            return Vec::new();
        }
        let agents = self.agent_choices();
        let context = Context {
            stopped: self.agents_stopped(),
            config: self.config_path.as_ref().and(self.config_text.as_deref()),
            agents: &agents,
            prompt,
        };
        let offers = proposals::offers(&found, &context, lang);
        tracing::info!(proposed = found.len(), offered = offers.len(), "AI actions");
        offers
    }

    /// The command field's answer to question `id` is all there.
    pub(crate) fn offer_launcher_proposals(&mut self, id: u64) {
        let lang = self.shell_lang();
        let Some(answer) = self
            .launcher
            .as_ref()
            .and_then(|p| p.launcher.answer())
            .filter(|a| matches!(a.state, hocus_shell::answer::State::Done(_)))
        else {
            return;
        };
        let prompt = proposals::prompt(lang, &answer.question, None, &answer.plain());
        let offers = self.proposal_offers(&answer.text, &prompt);
        if let Some(panel) = self.launcher.as_mut() {
            panel.launcher.set_offers(id, offers);
        }
    }

    /// Runs what the user confirmed. Agents stopped since it was offered:
    /// nothing runs.
    pub(crate) fn run_proposal(&mut self, run: Run) {
        let lang = self.shell_lang();
        if self.agents_stopped() {
            tracing::info!("an AI action was not run: agents are stopped");
            self.shell_toast(lang.ai_action_stopped(), "");
            return;
        }
        tracing::info!(?run, "AI action confirmed");
        match run {
            Run::Settings(page) => self.open_settings(Some(page)),
            Run::Intent(intent) => self.run_intent(intent),
            Run::Config { .. } => self.write_config_change(&run, lang),
            Run::Agent { key, name, text } => {
                if self.prompt_agent(&key, &text) {
                    self.journal_prompt(&name, &text);
                    self.shell_toast(&lang.handed_to(&name), "");
                } else {
                    self.shell_toast(lang.agent_gone(), &name);
                }
            }
        }
    }

    /// Writes one key on a worker, as the settings app would; the file's
    /// watcher applies it.
    fn write_config_change(&mut self, run: &Run, lang: Lang) {
        let (Some(change), Some(path), Some(news)) =
            (run.change(), self.config_path.clone(), self.ai.news.clone())
        else {
            self.shell_toast(lang.config_not_changed(), "");
            return;
        };
        let said = match run {
            Run::Config {
                section,
                key,
                value,
            } => format!("[{section}] {key} = {}", value.written()),
            _ => String::new(),
        };
        let spawned = std::thread::Builder::new()
            .name("config write".into())
            .spawn(move || {
                let written = hocus_core::config_edit::write(&path, &change)
                    .map(|()| said)
                    .map_err(|e| e.to_string());
                let _ = news.send(News::Written(written));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "could not start a thread");
        }
    }

    /// The worker wrote the configuration, or could not.
    pub(crate) fn config_written(&mut self, written: Result<String, String>) {
        let lang = self.shell_lang();
        match written {
            Ok(said) => {
                tracing::info!(said, "configuration changed from an AI answer");
                self.shell_toast(lang.config_changed(), &said);
            }
            Err(err) => {
                tracing::warn!(err, "the configuration was not changed");
                self.shell_toast(lang.config_not_changed(), &err);
            }
        }
    }
}
