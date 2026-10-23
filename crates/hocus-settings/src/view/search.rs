//! Search (#67): the documents found from the command field (#45), what
//! the indexer does, the folders it reads and the names it leaves out,
//! search by meaning and the model making its vectors, and what else the
//! command field offers (`[launcher]`). No board draws it: it keeps board
//! R2's template, captioned sections of choices, switch cards and fields
//! written with Enter. A folder inside a place never read is refused here
//! and said why; the indexer refuses it anyway, whatever the file says.

use std::path::{Component, Path, PathBuf};

use hocus_core::config::Meaning;
use hocus_platform::search::deny;

use super::*;

/// The home when it is not known: the places never read under it are
/// still told by their words.
const SOME_HOME: &str = "/~";

/// Why what was typed on the Search page was not written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Refusal {
    /// A folder neither from `/` nor from `~/`: the session would leave
    /// it out.
    NotFullPath,
    /// A folder in this place never read, as the user writes it.
    NeverRead(String),
    /// A folder searched already.
    Already,
    /// A name to leave out with a `/`: names are matched, not paths.
    NotAName,
}

/// The Search page between builds.
#[derive(Debug, Default)]
pub(super) struct SearchPage {
    /// Why the folder or the name typed last was not added.
    pub(super) refused: Option<Refusal>,
    /// The search engine typed last was not written: not an http(s)
    /// address with `{}`.
    pub(super) bad_engine: bool,
}

impl SearchPage {
    /// Off the page, or the file changed: nothing is refused any more.
    pub(super) fn leave(&mut self) {
        self.refused = None;
        self.bad_engine = false;
    }
}

/// `path` with `.` and `..` resolved by its words alone, as the indexer
/// reads it: links are never followed.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// A configured folder as the session reads it: `~/` is the home; a
/// relative one is left out.
fn full_path(folder: &str, home: &Path) -> Option<PathBuf> {
    let path = match folder.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if folder == "~" => home.to_owned(),
        None => Some(PathBuf::from(folder)).filter(|p| p.is_absolute())?,
    };
    Some(lexical(&path))
}

/// A folder typed, as it is written to the file: without the slash at its
/// end.
pub(super) fn folder_written(typed: &str) -> &str {
    let trimmed = typed.trim();
    match trimmed.trim_end_matches('/') {
        "" => trimmed,
        "~" if trimmed != "~" => "~/",
        kept => kept,
    }
}

/// A folder as shown: under the home, from `~/`.
pub(super) fn folder_shown(folder: &str, home: Option<&Path>) -> String {
    let rest = home.and_then(|h| Path::new(folder).strip_prefix(h).ok());
    match rest {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => format!("~/{}", rest.display()),
        None => folder.to_owned(),
    }
}

/// Whether a folder typed may be searched beside `folders`: a full path,
/// not searched already, and nowhere never read, the first such place
/// said as the user writes it.
pub(super) fn check_folder(
    typed: &str,
    home: Option<&Path>,
    folders: &[String],
) -> Result<(), Refusal> {
    let home = home.unwrap_or(Path::new(SOME_HOME));
    let path = full_path(folder_written(typed), home).ok_or(Refusal::NotFullPath)?;
    let mut place = PathBuf::new();
    for part in path.components() {
        place.push(part);
        if deny::denied(&place, Some(home)) {
            let shown = match place.strip_prefix(home) {
                Ok(rest) => format!("~/{}", rest.display()),
                Err(_) => place.display().to_string(),
            };
            return Err(Refusal::NeverRead(shown));
        }
    }
    let already = folders
        .iter()
        .filter_map(|f| full_path(f, home))
        .any(|f| f == path);
    if already {
        return Err(Refusal::Already);
    }
    Ok(())
}

/// A search engine the configuration takes: http(s), `{}` where the words
/// go.
pub(super) fn engine_ok(engine: &str) -> bool {
    (engine.starts_with("https://") || engine.starts_with("http://")) && engine.contains("{}")
}

/// A list of the file as a value; empty, the key goes (back to its
/// default, which is empty too).
fn list_change(key: &'static str, list: &[String]) -> Change {
    let value = (!list.is_empty()).then(|| {
        let array: toml_edit::Array = list.iter().map(String::as_str).collect();
        toml_edit::Value::from(array)
    });
    Change {
        value,
        ..Change::new("search", key, "")
    }
}

impl SettingsView {
    /// What the indexer does, as the compositor said last.
    pub fn set_search(&mut self, info: hocus_api::SearchInfo) {
        // The last look is said to the minute: a new second alone draws
        // nothing again.
        let minutes = |i: &hocus_api::SearchInfo| i.last_look_seconds.map(|s| s / 60);
        let shown_same = hocus_api::SearchInfo {
            last_look_seconds: None,
            ..info.clone()
        } == hocus_api::SearchInfo {
            last_look_seconds: None,
            ..self.ctx.search.clone()
        } && minutes(&info) == minutes(&self.ctx.search);
        self.ctx.search = info;
        if !shown_same {
            self.dirty = true;
        }
    }

    /// The Search page shows: what the indexer does is worth asking again.
    pub fn shows_search(&self) -> bool {
        self.screen == Screen::Page(Page::Search)
    }

    /// A folder or a name typed on Search, written to its list once it
    /// may be; else said why.
    pub(super) fn commit_list_entry(&mut self, field: &Field, typed: &str) -> Vec<Outcome> {
        let search = &self.ctx.config.search;
        self.search.refused = None;
        if typed.is_empty() {
            self.fields.forget(field);
            return Vec::new();
        }
        let (key, mut list, entry) = if *field == Field::NewSearchFolder {
            let checked = check_folder(typed, self.ctx.home.as_deref(), &search.folders);
            if let Err(why) = checked {
                self.search.refused = Some(why);
                return self.keep_typing(field, typed);
            }
            ("folders", search.folders.clone(), folder_written(typed))
        } else {
            if typed.contains('/') {
                self.search.refused = Some(Refusal::NotAName);
                return self.keep_typing(field, typed);
            }
            ("exclude", search.exclude.clone(), typed)
        };
        self.fields.forget(field);
        if list.iter().any(|e| e.eq_ignore_ascii_case(entry)) {
            return Vec::new();
        }
        list.push(entry.to_owned());
        vec![Outcome::Change(Box::new(list_change(key, &list)))]
    }

    /// What was typed stays in its field, with the keyboard, to be put
    /// right.
    fn keep_typing(&mut self, field: &Field, typed: &str) -> Vec<Outcome> {
        self.fields.type_into(field.clone(), typed.to_owned());
        self.tree.set_focus(self.fields.node(field));
        Vec::new()
    }

    /// The search engine typed, written once the configuration takes it;
    /// empty, the key goes (DuckDuckGo).
    pub(super) fn commit_engine(&mut self, typed: &str) -> Vec<Outcome> {
        self.search.bad_engine = !typed.is_empty() && !engine_ok(typed);
        if self.search.bad_engine {
            return self.keep_typing(&Field::SearchEngine, typed);
        }
        self.fields.type_into(Field::SearchEngine, typed.to_owned());
        let value = (!typed.is_empty()).then(|| toml_edit::Value::from(typed));
        vec![Outcome::Change(Box::new(Change {
            value,
            ..Change::new("launcher", "search_engine", "")
        }))]
    }

    /// Search: the documents and the indexer, its folders, what it leaves
    /// out, search by meaning, and the command field's other results.
    pub(super) fn search_page(&mut self, theme: &Theme, body: NodeId) {
        self.tree.set_style(body, fill_width(column(26.0)));
        self.documents(theme, body);
        self.search_folders(theme, body);
        self.left_out(theme, body);
        self.meaning(theme, body);
        self.also_offered(theme, body);
    }

    /// Documents on or off, and what the indexer does.
    fn documents(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let on = self.ctx.config.search.documents;
        let section = self.section(theme, body, words::DOCUMENTS);
        self.switch_card(
            theme,
            section,
            (words::SEARCH_DOCUMENTS, words::SEARCH_DOCUMENTS_SUB),
            on,
            Some(Change::new("search", "documents", !on)),
            Some(&Setting::Documents),
        );
        let info = self.ctx.search.clone();
        let line = self.add(
            section,
            Style {
                padding: Insets::symmetric(12.0, 16.0),
                ..fill_width(row(12.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 16.0,
                ..Look::default()
            }),
        );
        let text = self.add(line, fill_width(column(2.0)), Kind::Box(Look::default()));
        let phase = match info.state.as_str() {
            "" => None,
            "off" => Some((words::INDEX_OFF, Paint::Muted)),
            "waiting" => Some((words::INDEX_WAITING, Paint::Muted)),
            "paused" => Some((words::INDEX_PAUSED, Paint::Waiting)),
            "indexing" => Some((words::INDEX_LOOKING, Paint::Muted)),
            _ => Some((words::INDEX_READY, Paint::Muted)),
        };
        let counted = info.state != "off" && !info.state.is_empty();
        let head = match (phase, counted) {
            (None, _) => words::UNKNOWN.get(lang).to_owned(),
            (Some((said, _)), false) => said.get(lang).to_owned(),
            (Some(_), true) => words::documents(info.documents, lang),
        };
        self.line(text, &head, theme.text.body.with_weight(600));
        if let Some((said, paint)) = phase.filter(|_| counted) {
            let mut under = vec![said.get(lang).to_owned()];
            under.push(words::last_look(info.last_look_seconds, lang));
            self.add(
                text,
                fill_width(Style::default()),
                Kind::text(under.join(" · "), theme.text.label.with_color(paint)),
            );
            let mut counts = Vec::new();
            if info.embedded > 0 {
                counts.push(words::with_meaning(info.embedded, lang));
            }
            if info.never_read > 0 {
                counts.push(words::never_read(info.never_read, lang));
            }
            if info.unreadable > 0 {
                counts.push(words::unreadable(info.unreadable, lang));
            }
            if !counts.is_empty() {
                self.line(
                    text,
                    &counts.join(" · "),
                    theme.text.label.with_color(Paint::Muted),
                );
            }
        }
        if on {
            let mut again = Button::new(words::SEARCH_AGAIN.get(lang), ButtonKind::Plain, theme);
            again.enabled = matches!(info.state.as_str(), "ready" | "waiting");
            let b = self.add(
                line,
                Style {
                    height: Length::Px(40.0),
                    padding: Insets::symmetric(0.0, 16.0),
                    ..Style::default()
                },
                Kind::Button(again),
            );
            self.targets.insert(b, Target::SearchAgain);
        }
        self.note(theme, section, words::DOCUMENTS_NOTE);
    }

    /// A card of a list: its entry in mono, and Remove.
    fn entry_card(&mut self, theme: &Theme, parent: NodeId, entry: &str, remove: Change) {
        let lang = self.lang;
        let card = self.add(
            parent,
            Style {
                padding: Insets::symmetric(8.0, 16.0),
                ..fill_width(row(12.0))
            },
            Kind::Box(Look {
                background: Some(Paint::SurfaceHover),
                radius: 14.0,
                ..Look::default()
            }),
        );
        let name = self.add(
            card,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        let mono = hocus_ui::TextStyle {
            typeface: hocus_ui::Typeface::Mono,
            ..theme.text.body.with_weight(600)
        };
        self.line(name, entry, mono);
        let button = Button::new(words::REMOVE.get(lang), ButtonKind::Plain, theme)
            .with_name(words::remove_named(entry, lang));
        let b = self.add(
            card,
            Style {
                height: Length::Px(36.0),
                padding: Insets::symmetric(0.0, 14.0),
                ..Style::default()
            },
            Kind::Button(button),
        );
        self.targets.insert(b, Target::Set(remove));
    }

    /// A field to add to a list, and Add.
    fn add_line(
        &mut self,
        theme: &Theme,
        parent: NodeId,
        field: Field,
        (name, hint, add): (words::Words, words::Words, words::Words),
    ) {
        let lang = self.lang;
        let line = self.add(parent, fill_width(row(10.0)), Kind::Box(Look::default()));
        self.text_field(
            theme,
            line,
            field.clone(),
            (name.get(lang), hint.get(lang)),
            "",
        );
        let b = self.add(
            line,
            Style {
                height: Length::Px(44.0),
                padding: Insets::symmetric(0.0, 18.0),
                ..Style::default()
            },
            Kind::Button(
                Button::new(words::ADD.get(lang), ButtonKind::Primary, theme)
                    .with_name(add.get(lang)),
            ),
        );
        self.targets.insert(b, Target::Commit(field));
    }

    /// Why the last folder or name was not added, under its field.
    fn refusal(&mut self, theme: &Theme, parent: NodeId, folders: bool) {
        let lang = self.lang;
        let said = match (&self.search.refused, folders) {
            (Some(Refusal::NotFullPath), true) => words::NOT_A_FULL_PATH.get(lang).to_owned(),
            (Some(Refusal::NeverRead(place)), true) => words::never_read_place(place, lang),
            (Some(Refusal::Already), true) => words::ALREADY_SEARCHED.get(lang).to_owned(),
            (Some(Refusal::NotAName), false) => words::NOT_A_NAME.get(lang).to_owned(),
            _ => return,
        };
        self.add(
            parent,
            fill_width(Style::default()),
            Kind::text(
                said,
                theme.text.label.with_weight(500).with_color(Paint::Waiting),
            ),
        );
    }

    /// The folders searched, each with Remove, a field to add one, the
    /// screenshots' text, and what is never read.
    fn search_folders(&mut self, theme: &Theme, body: NodeId) {
        let search = self.ctx.config.search.clone();
        let section = self.section(theme, body, words::FOLDERS_SEARCHED);
        if search.folders.is_empty() {
            self.note(theme, section, words::USUAL_FOLDERS);
        }
        for (i, folder) in search.folders.iter().enumerate() {
            let mut rest = search.folders.clone();
            rest.remove(i);
            let shown = folder_shown(folder, self.ctx.home.as_deref());
            self.entry_card(theme, section, &shown, list_change("folders", &rest));
        }
        self.add_line(
            theme,
            section,
            Field::NewSearchFolder,
            (words::NEW_FOLDER, words::NEW_FOLDER_HINT, words::ADD_FOLDER),
        );
        self.refusal(theme, section, true);
        self.highlighted(section, &Setting::SearchFolders, Look::default());
        self.switch_card(
            theme,
            section,
            (words::SCREENSHOTS_TEXT, words::SCREENSHOTS_TEXT_SUB),
            search.screenshots,
            Some(Change::new("search", "screenshots", !search.screenshots)),
            Some(&Setting::SearchScreenshots),
        );
        self.note(theme, section, words::NEVER_READ_NOTE);
    }

    /// The names left out, each with Remove, and a field to add one.
    fn left_out(&mut self, theme: &Theme, body: NodeId) {
        let exclude = self.ctx.config.search.exclude.clone();
        let section = self.section(theme, body, words::LEFT_OUT);
        if exclude.is_empty() {
            self.note(theme, section, words::NOTHING_LEFT_OUT);
        }
        for (i, name) in exclude.iter().enumerate() {
            let mut rest = exclude.clone();
            rest.remove(i);
            self.entry_card(theme, section, name, list_change("exclude", &rest));
        }
        self.add_line(
            theme,
            section,
            Field::NewExclude,
            (
                words::NEW_EXCLUDE,
                words::NEW_EXCLUDE_HINT,
                words::ADD_EXCLUDE,
            ),
        );
        self.refusal(theme, section, false);
        self.note(theme, section, words::EXCLUDE_NOTE);
        self.highlighted(section, &Setting::SearchExclude, Look::default());
    }

    /// Search by meaning: whose model may read the documents, why it is
    /// words only for now, and the model.
    fn meaning(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let search = self.ctx.config.search.clone();
        let ai = self.ctx.config.ai.clone();
        let section = self.section(theme, body, words::MEANING);
        let choices = [
            (Meaning::Off, words::MEANING_OFF, "off"),
            (Meaning::Local, words::MEANING_LOCAL, "local"),
            (Meaning::On, words::MEANING_ON, "on"),
        ]
        .into_iter()
        .map(|(meaning, name, id)| {
            let target = Target::Set(Change::new("search", "meaning", id));
            (name.get(lang).to_owned(), meaning == search.meaning, target)
        })
        .collect();
        self.named_choices(theme, section, words::MEANING, choices);
        self.note(theme, section, words::MEANING_NOTE);
        if search.meaning == Meaning::On {
            self.note_painted(theme, section, words::MEANING_SENT, Paint::Waiting);
        }
        // Why documents are found by their words only, as `[ai]` is.
        let why = match search.meaning {
            Meaning::Off => None,
            _ if !ai.enabled => Some(words::MEANING_AI_OFF),
            _ if ai.provider == "claude" => Some(words::MEANING_CLAUDE),
            Meaning::Local if !ai.is_local() => Some(words::MEANING_NOT_HERE),
            _ => None,
        };
        let line = self.add(section, fill_width(row(12.0)), Kind::Box(Look::default()));
        let text = self.add(
            line,
            fill_width(Style::default()),
            Kind::Box(Look::default()),
        );
        if let Some(why) = why {
            self.note(theme, text, why);
        }
        let b = self.add(
            line,
            Style {
                height: Length::Px(40.0),
                padding: Insets::symmetric(0.0, 16.0),
                ..Style::default()
            },
            Kind::Button(Button::new(
                Page::Agents.label().get(lang),
                ButtonKind::Plain,
                theme,
            )),
        );
        self.targets.insert(b, Target::Page(Page::Agents));
        if search.meaning != Meaning::Off {
            let holder = self.add(section, fill_width(column(8.0)), Kind::Box(Look::default()));
            // Named apart from the field, as AI and agents' are.
            self.line(
                holder,
                words::MODEL.get(lang),
                theme.text.body.with_weight(600),
            );
            self.text_field(
                theme,
                holder,
                Field::EmbeddingModel,
                (
                    words::EMBEDDING_MODEL.get(lang),
                    words::EMBEDDING_HINT.get(lang),
                ),
                &search.embedding_model,
            );
            self.note(theme, holder, words::EMBEDDING_NOTE);
        }
        self.highlighted(section, &Setting::SearchMeaning, Look::default());
    }

    /// The command field's other results, each on or off, and the web
    /// search's engine.
    fn also_offered(&mut self, theme: &Theme, body: NodeId) {
        let lang = self.lang;
        let l = self.ctx.config.launcher.clone();
        let section = self.section(theme, body, words::ALSO_OFFERED);
        let cards = [
            (
                "calculator",
                words::CALCULATOR,
                words::CALCULATOR_SUB,
                l.calculator,
                Some(Setting::Calculator),
            ),
            ("units", words::UNITS, words::UNITS_SUB, l.units, None),
            ("urls", words::URLS, words::URLS_SUB, l.urls, None),
            (
                "large_type",
                words::LARGE_TYPE,
                words::LARGE_TYPE_SUB,
                l.large_type,
                None,
            ),
            ("qr", words::QR_CODE, words::QR_CODE_SUB, l.qr, None),
            (
                "intents",
                words::REQUESTS,
                words::REQUESTS_SUB,
                l.intents,
                None,
            ),
            (
                "web_search",
                words::WEB_SEARCH,
                words::WEB_SEARCH_SUB,
                l.web_search,
                Some(Setting::WebSearch),
            ),
        ];
        for three in cards.chunks(3) {
            let switches = self.switches(section);
            for (key, name, sub, on, setting) in three {
                let change = Change::new("launcher", *key, !*on);
                self.switch_card(
                    theme,
                    switches,
                    (*name, *sub),
                    *on,
                    Some(change),
                    setting.as_ref(),
                );
            }
        }
        if l.web_search {
            let holder = self.add(section, fill_width(column(8.0)), Kind::Box(Look::default()));
            self.line(
                holder,
                words::SEARCH_ENGINE.get(lang),
                theme.text.body.with_weight(600),
            );
            self.text_field(
                theme,
                holder,
                Field::SearchEngine,
                (
                    words::ENGINE_ADDRESS.get(lang),
                    words::SEARCH_ENGINE_HINT.get(lang),
                ),
                &l.search_engine,
            );
            if self.search.bad_engine {
                self.note_painted(theme, holder, words::BAD_ENGINE, Paint::Waiting);
            } else {
                self.note(theme, holder, words::SEARCH_ENGINE_NOTE);
            }
        }
        self.note(theme, section, words::ALWAYS_OFFERED);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_inside_a_place_never_read_is_refused_and_said() {
        let home = Some(Path::new("/home/alex"));
        let none: &[String] = &[];
        assert_eq!(check_folder("~/Documents", home, none), Ok(()));
        assert_eq!(check_folder("/srv/notes/", home, none), Ok(()));
        assert_eq!(check_folder("~", home, none), Ok(()), "the home holds them");
        assert_eq!(
            check_folder("~/.ssh", home, none),
            Err(Refusal::NeverRead("~/.ssh".into()))
        );
        assert_eq!(
            check_folder("/home/alex/.config/gcloud/configurations", home, none),
            Err(Refusal::NeverRead("~/.config/gcloud".into()))
        );
        assert_eq!(
            check_folder("~/Documents/../.mozilla", home, none),
            Err(Refusal::NeverRead("~/.mozilla".into()))
        );
        assert_eq!(
            check_folder("/srv/team/.password-store/work", home, none),
            Err(Refusal::NeverRead("/srv/team/.password-store".into()))
        );
        // The home unknown: its places are still known by their words.
        assert_eq!(
            check_folder("~/.config/gcloud", None, none),
            Err(Refusal::NeverRead("~/.config/gcloud".into()))
        );
        assert_eq!(
            check_folder("Documents", home, none),
            Err(Refusal::NotFullPath)
        );
        let folders = ["/home/alex/Notes".to_owned()];
        assert_eq!(
            check_folder("~/Notes/", home, &folders),
            Err(Refusal::Already)
        );
        assert_eq!(folder_written(" ~/Notes/ "), "~/Notes");
        assert_eq!(folder_shown("/home/alex/Notes", home), "~/Notes");
        assert_eq!(
            folder_shown("/home/alexandra/Notes", home),
            "/home/alexandra/Notes"
        );
        assert_eq!(folder_shown("~/Notes", home), "~/Notes");
        assert_eq!(folder_written("/"), "/");
        assert_eq!(folder_written("~/"), "~/");
        assert!(engine_ok("https://www.qwant.com/?q={}"));
        assert!(!engine_ok("https://www.qwant.com/?q="));
        assert!(!engine_ok("ftp://example.org/{}"));
    }
}
