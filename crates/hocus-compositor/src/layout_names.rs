//! The names of the keyboard layouts kept that the shell does not name
//! itself (`fr(oss)`, `de(neo)`): XKB's registry and its translations are
//! read on a thread each time the layouts or the language change, never on
//! the loop; the column says their code until the names come.

use hocus_platform::xkb::Registry;
use hocus_shell::{Lang, layouts::Names};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

#[derive(Debug)]
pub struct LayoutNames {
    /// Where the registry is; tests give their copy, or none.
    pub registry: Registry,
    pub names: Names,
    /// The layouts kept as the shell shows them, and the keys that switch
    /// them: made again when the configuration or the names change, not
    /// after every turn of the loop.
    pub kept: std::sync::Arc<[hocus_shell::KeyboardLayout]>,
    pub switch_keys: Option<std::sync::Arc<str>>,
    /// The language and layouts last asked about: an answer to an older
    /// question is dropped.
    asked: Option<(Lang, Vec<String>)>,
    news: Option<channel::Sender<(Lang, Vec<String>, Names)>>,
}

impl Default for LayoutNames {
    fn default() -> Self {
        Self {
            registry: Registry::Off,
            names: Names::new(),
            kept: std::sync::Arc::new([]),
            switch_keys: None,
            asked: None,
            news: None,
        }
    }
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, registry: Registry) {
    state.layout_names.registry = registry;
    let (news, received) = channel::channel::<(Lang, Vec<String>, Names)>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg((lang, codes, names)) = event {
            app.timed("layout names", |app| {
                app.state.layout_names_read(lang, codes, names)
            });
        }
    });
    match inserted {
        Ok(_) => state.layout_names.news = Some(news),
        Err(err) => tracing::warn!(%err.error, "keyboard layouts go by their codes"),
    }
}

impl State {
    /// Asks for the names of the layouts kept that the shell does not
    /// know, in the shell's language, unless already asked.
    pub(crate) fn name_layouts(&mut self) {
        self.keep_layouts();
        let lang = self.shell_lang();
        let codes: Vec<String> = self
            .config
            .keyboard
            .layouts
            .iter()
            .filter(|c| hocus_shell::layouts::known(c, lang).is_none())
            .cloned()
            .collect();
        let n = &mut self.layout_names;
        let question = (lang, codes);
        if n.asked.as_ref() == Some(&question) {
            return;
        }
        n.asked = Some(question.clone());
        let (lang, codes) = question;
        if codes.is_empty() || n.registry == Registry::Off {
            return;
        }
        let Some(news) = n.news.clone() else {
            return;
        };
        let registry = n.registry.clone();
        let spawned = std::thread::Builder::new()
            .name("layout names".into())
            .spawn(move || {
                let code = match lang {
                    Lang::En => "en",
                    Lang::Fr => "fr",
                };
                let names = registry
                    .read(code)
                    .into_iter()
                    .filter(|l| codes.contains(&l.code))
                    .map(|l| (l.code.clone(), l.name().to_owned()))
                    .collect();
                let _ = news.send((lang, codes, names));
            });
        if let Err(err) = spawned {
            tracing::warn!(%err, "keyboard layouts go by their codes");
        }
    }

    fn layout_names_read(&mut self, lang: Lang, codes: Vec<String>, names: Names) {
        let n = &mut self.layout_names;
        if n.asked.as_ref() != Some(&(lang, codes)) {
            return;
        }
        if n.names != names {
            n.names = names;
            self.keep_layouts();
            self.redraw.all();
        }
    }

    /// Tests: the registry is this copy of XKB's data.
    pub fn set_xkb_registry(&mut self, registry: Registry) {
        self.layout_names.registry = registry;
        self.layout_names.asked = None;
        self.name_layouts();
    }

    /// The layouts kept, as the column and quick settings show them (XKB's
    /// default, US, when none is), and the keys that switch them.
    fn keep_layouts(&mut self) {
        let lang = self.shell_lang();
        let layouts = &self.config.keyboard.layouts;
        let us = ["us".to_owned()];
        let codes = if layouts.is_empty() { &us[..] } else { layouts };
        let kept: Vec<_> = codes
            .iter()
            .map(|code| hocus_shell::KeyboardLayout {
                name: hocus_shell::layouts::name(code, lang, &self.layout_names.names),
                short: hocus_shell::layouts::short(code),
            })
            .collect();
        if *self.layout_names.kept != *kept {
            self.layout_names.kept = kept.into();
        }
        let keys = self
            .shortcuts
            .bindings
            .iter()
            .find(|(_, a)| **a == hocus_core::Action::SwitchLayout)
            .map(|(combo, _)| hocus_shell::actions::caps(combo, lang).join("+"));
        if self.layout_names.switch_keys.as_deref() != keys.as_deref() {
            self.layout_names.switch_keys = keys.map(Into::into);
        }
    }
}
