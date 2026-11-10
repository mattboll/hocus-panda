//! Work / Secretary (#759, docs/design/secretary.md §22): the secretary's
//! own page, after Agents. Her mode, her name, her key and what she knows;
//! what she reads, grouped by account (each kind of each Google account of
//! Online Accounts, [`crate::google_sources`]; the applications whose
//! notifications she may read, [`crate::notifiers`]); and the triage on
//! this computer: its state, when it runs, its model.
//!
//! Her mode, her name and when the triage runs are keys of the file
//! (`[secretary] mode`, `name`, `[secretary.triage] battery`), which she
//! and the compositor follow as the file changes. What she reads is her
//! consent book's, written on the page's thread; her key and the triage's
//! state are hers, asked on her socket.

use std::sync::OnceLock;

use crate::{
    lang::{Lang, w},
    model::{Choice, Codec, Kind, Outside, Place, Places, RowGroup, SectionId, Setting, Value},
};

pub const HER: RowGroup = RowGroup {
    label: w("LA SECRÉTAIRE", "THE SECRETARY"),
    per_source: false,
};
pub const READS: RowGroup = RowGroup {
    label: w("CE QU’ELLE LIT", "WHAT SHE READS"),
    per_source: false,
};
pub const TRIAGE: RowGroup = RowGroup {
    label: w("LE TRI SUR CET ORDINATEUR", "TRIAGE ON THIS COMPUTER"),
    per_source: false,
};

/// `[secretary] mode` (docs/design/secretary.md §21).
pub const MODE: &str = "secretary.mode";
/// `[secretary] name` (#752): her name, empty for "ta secrétaire".
pub const NAME: &str = "secretary.name";
/// `[secretary.triage] battery`: on battery, the triage runs only above
/// this charge.
pub const BATTERY: &str = "secretary.triage.battery";
/// The page's own rows, for the search and the lit row.
pub const KEY: &str = "secretary.key";
pub const KNOWS: &str = "secretary.knows";
pub const GOOGLE: &str = "secretary.google";
pub const NOTIFIERS: &str = "secretary.notifiers";
pub const ADD_GOOGLE: &str = "secretary.add-google";
pub const QUEUE: &str = "secretary.queue";
pub const MODEL: &str = "secretary.model";

/// The charges the triage's menu offers; the file may hold another.
const CHARGES: [i64; 4] = [0, 20, 50, 80];

/// The page's rows the model does not hold, for the search.
pub fn outside() -> &'static [Outside] {
    const ALL: [Outside; 7] = [
        Outside {
            id: KEY,
            section: SectionId::Secretary,
            group: HER.label,
            name: w("Clé Anthropic", "Anthropic key"),
            keywords: (
                &["clé", "anthropic", "claude", "api", "trousseau", "sk-ant"],
                &["key", "anthropic", "claude", "api", "keyring", "sk-ant"],
            ),
        },
        Outside {
            id: KNOWS,
            section: SectionId::Secretary,
            group: HER.label,
            name: w("Ce qu’elle sait", "What she knows"),
            keywords: (
                &[
                    "mémoire",
                    "souvenirs",
                    "oublier",
                    "sait",
                    "personnes",
                    "projets",
                ],
                &["memory", "forget", "knows", "people", "projects"],
            ),
        },
        Outside {
            id: GOOGLE,
            section: SectionId::Secretary,
            group: READS.label,
            name: w("Tes comptes Google", "Your Google accounts"),
            keywords: (
                &[
                    "google",
                    "gmail",
                    "agenda",
                    "agendas",
                    "calendrier",
                    "mails",
                    "courriel",
                    "anniversaires",
                    "contacts",
                    "travail",
                    "privé",
                    "lire",
                    "autoriser",
                ],
                &[
                    "google",
                    "gmail",
                    "calendar",
                    "calendars",
                    "mail",
                    "email",
                    "birthdays",
                    "contacts",
                    "work",
                    "private",
                    "read",
                    "allow",
                ],
            ),
        },
        Outside {
            id: NOTIFIERS,
            section: SectionId::Secretary,
            group: READS.label,
            name: w(
                "Les notifications de tes applications",
                "Your applications’ notifications",
            ),
            keywords: (
                &[
                    "notifications",
                    "slack",
                    "signal",
                    "messages",
                    "applications",
                ],
                &[
                    "notifications",
                    "slack",
                    "signal",
                    "messages",
                    "applications",
                ],
            ),
        },
        Outside {
            id: ADD_GOOGLE,
            section: SectionId::Secretary,
            group: READS.label,
            name: w("Ajouter un compte Google", "Add a Google account"),
            keywords: (
                &["compte", "google", "ajouter", "connecter"],
                &["account", "google", "add", "sign in"],
            ),
        },
        Outside {
            id: QUEUE,
            section: SectionId::Secretary,
            group: TRIAGE.label,
            name: w("Le tri de tes mails", "Sorting your mail"),
            keywords: (
                &["tri", "trier", "file", "attente", "mots de passe", "local"],
                &["triage", "sort", "queue", "waiting", "passwords", "local"],
            ),
        },
        Outside {
            id: MODEL,
            section: SectionId::Secretary,
            group: TRIAGE.label,
            name: w("Le modèle", "The model"),
            keywords: (
                &[
                    "modèle",
                    "ollama",
                    "local",
                    "qwen",
                    "processeur",
                    "carte graphique",
                ],
                &[
                    "model",
                    "ollama",
                    "local",
                    "qwen",
                    "processor",
                    "graphics card",
                ],
            ),
        },
    ];
    &ALL
}

/// What a menu's button says, shorter than its choice when it must be.
pub fn button_said(id: &str, value: &Value, lang: Lang) -> Option<String> {
    match (id, value) {
        (BATTERY, Value::Int(0)) => Some(w("Toujours", "Always").get(lang).to_owned()),
        (BATTERY, Value::Int(n)) => Some(match lang {
            Lang::French => format!("Sur secteur, ou batterie au-dessus de {n} %"),
            Lang::English => format!("On AC, or battery above {n} %"),
        }),
        _ => None,
    }
}

fn charge(n: i64) -> Choice {
    let label = match n {
        0 => w("Toujours, même sur batterie", "Always, on battery too"),
        20 => w(
            "Sur secteur, ou batterie au-dessus de 20 %",
            "On AC, or battery above 20 %",
        ),
        50 => w(
            "Sur secteur, ou batterie au-dessus de 50 %",
            "On AC, or battery above 50 %",
        ),
        _ => w(
            "Sur secteur, ou batterie au-dessus de 80 %",
            "On AC, or battery above 80 %",
        ),
    };
    Choice {
        label,
        value: Value::Int(n),
    }
}

/// The page's settings, in its order.
pub fn settings() -> &'static [Setting] {
    static SETTINGS: OnceLock<Vec<Setting>> = OnceLock::new();
    SETTINGS.get_or_init(|| {
        let one = |key, codec, default| {
            Places::One(Place {
                key,
                codec,
                default,
            })
        };
        vec![
            Setting {
                id: MODE,
                section: SectionId::Secretary,
                group: HER,
                name: w("La secrétaire", "The secretary"),
                line: Some(w(
                    "Elle lit ce que tu lui autorises, sans rien marquer comme lu, et te propose ; tu décides. Coupée, elle ne parle que des agents.",
                    "She reads what you allow her, without marking anything read, and proposes; you decide. Off, she only speaks of the agents.",
                )),
                keywords: (
                    &["secrétaire", "assistante", "mémoire", "classique", "agenda"],
                    &["secretary", "assistant", "memory", "classic", "calendar"],
                ),
                kind: Kind::Switch,
                places: one(
                    &["secretary", "mode"],
                    Codec::Flag {
                        off: "classic",
                        on: "assistant",
                    },
                    Value::Bool(false),
                ),
                needs: None,
            },
            Setting {
                id: NAME,
                section: SectionId::Secretary,
                group: HER,
                name: w("Son nom", "Her name"),
                line: Some(w(
                    "Dans son tiroir, sa bulle, et quand elle se présente. Vide : « Ta secrétaire ».",
                    "In her drawer, her bubble, and when she introduces herself. Empty: “Your secretary”.",
                )),
                keywords: (
                    &["secrétaire", "nom", "prénom", "appelle"],
                    &["secretary", "name", "called"],
                ),
                kind: Kind::Text,
                places: one(&["secretary", "name"], Codec::Text, Value::Text(String::new())),
                needs: None,
            },
            Setting {
                id: BATTERY,
                section: SectionId::Secretary,
                group: TRIAGE,
                name: w("Quand trier", "When to sort"),
                line: Some(w(
                    "Sur batterie, le tri attend pour ne pas la vider.",
                    "On battery, sorting waits so as not to drain it.",
                )),
                keywords: (
                    &["tri", "batterie", "secteur", "charge", "quand"],
                    &["triage", "sort", "battery", "ac", "charge", "when"],
                ),
                kind: Kind::Menu(CHARGES.into_iter().map(charge).collect()),
                places: one(
                    &["secretary", "triage", "battery"],
                    Codec::Int { min: 0, max: 100 },
                    Value::Int(50),
                ),
                needs: Some(MODE),
            },
        ]
    })
}

/// The triage as her status says it (`idle`, `loaded`, `running`,
/// `resting`, `paused: <why>`), with what waits, in plain words: "21 mails
/// en attente", "En pause : la batterie est à 32 %".
pub fn triage_said(state: &str, pending: u64, lang: Lang) -> String {
    let fr = lang == Lang::French;
    let waiting = match (pending, fr) {
        (0, true) => "Rien à trier pour l’instant.".to_owned(),
        (0, false) => "Nothing to sort for now.".to_owned(),
        (1, true) => "1 en attente.".to_owned(),
        (1, false) => "1 waiting.".to_owned(),
        (n, true) => format!("{n} en attente."),
        (n, false) => format!("{n} waiting."),
    };
    let Some(why) = state.strip_prefix("paused: ") else {
        return waiting;
    };
    let pause = if let Some(charge) = why
        .strip_prefix("battery ")
        .and_then(|c| c.trim_end_matches(" %").trim().parse::<u8>().ok())
    {
        if fr {
            format!("En pause : la batterie est à {charge} %.")
        } else {
            format!("Paused: the battery is at {charge} %.")
        }
    } else if why == "model unavailable" {
        w(
            "En pause : le modèle ne répond pas.",
            "Paused: the model does not answer.",
        )
        .get(lang)
        .to_owned()
    } else {
        w(
            "En pause : l’ordinateur chauffe, il reprend une fois refroidi.",
            "Paused: the computer is hot, it goes on once cooler.",
        )
        .get(lang)
        .to_owned()
    };
    if pending == 0 {
        pause
    } else {
        format!("{pause} {waiting}")
    }
}

/// The triage's model as the user reads it: who runs it and where, its
/// name, whether it is ready. `model` is her status's `PROVIDER/MODEL`,
/// `kind` the provider's kind in the file (`ollama`, `openai`,
/// `anthropic`), `state` the triage's.
pub fn model_said(model: &str, kind: Option<&str>, state: &str, lang: Lang) -> String {
    let fr = lang == Lang::French;
    let (provider, name) = model.split_once('/').unwrap_or(("", model));
    let kind = kind.unwrap_or(match provider {
        "local" => "ollama",
        other => other,
    });
    let who = match (kind, fr) {
        ("ollama", true) => "Ollama, sur cet ordinateur".to_owned(),
        ("ollama", false) => "Ollama, on this computer".to_owned(),
        ("openai", true) => "Un serveur compatible OpenAI".to_owned(),
        ("openai", false) => "An OpenAI-compatible server".to_owned(),
        ("anthropic", _) => "Claude (Anthropic)".to_owned(),
        ("fake", true) => "Un modèle d’essai".to_owned(),
        ("fake", false) => "A test model".to_owned(),
        (other, _) => other.to_owned(),
    };
    let ready = if state == "paused: model unavailable" {
        w("ne répond pas", "does not answer")
    } else {
        w("prêt", "ready")
    };
    if name.is_empty() {
        format!("{who} · {}", ready.get(lang))
    } else {
        format!("{who} · {name} · {}", ready.get(lang))
    }
}

/// `provider`'s kind as the configuration file says it
/// (`[secretary.providers.NAME] kind`), none when it says none. Blocking
/// (a file).
pub fn provider_kind(provider: &str) -> Option<String> {
    let text = std::fs::read_to_string(hocus_config::file()?).ok()?;
    let doc = text.parse::<toml_edit::DocumentMut>().ok()?;
    doc.get("secretary")?
        .get("providers")?
        .get(provider)?
        .get("kind")?
        .as_str()
        .map(str::to_owned)
}

/// "Elle en lit 3 sur 4 · Famille laissé de côté": an Agenda's calendars
/// read, and those left out by name; `None` before she listed them.
pub fn calendars_said(names: &[(String, bool)], lang: Lang) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    let fr = lang == Lang::French;
    let read = names.iter().filter(|(_, read)| *read).count();
    let out: Vec<&str> = names
        .iter()
        .filter(|(_, read)| !*read)
        .map(|(name, _)| name.as_str())
        .collect();
    let all = names.len();
    let head = match (out.is_empty(), fr) {
        (true, true) if all == 1 => "Elle le lit".to_owned(),
        (true, true) => format!("Elle les lit tous les {all}"),
        (true, false) if all == 1 => "She reads it".to_owned(),
        (true, false) => format!("She reads all {all}"),
        (false, true) => format!("Elle en lit {read} sur {all}"),
        (false, false) => format!("She reads {read} of {all}"),
    };
    if out.is_empty() {
        return Some(head);
    }
    let left = match (out.len(), fr) {
        (1, true) => format!("{} laissé de côté", out[0]),
        (_, true) => format!("{} laissés de côté", out.join(", ")),
        (_, false) => format!("{} left out", out.join(", ")),
    };
    Some(format!("{head} · {left}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(query: &str) -> Vec<&'static str> {
        crate::search::find(query, Lang::French)
            .iter()
            .map(|f| f.name)
            .collect()
    }

    #[test]
    fn searching_finds_her_page_s_rows() {
        assert!(names("secrétaire").contains(&"Secrétaire"), "the page");
        assert!(names("secrétaire").contains(&"La secrétaire"));
        assert!(names("clé anthropic").contains(&"Clé Anthropic"));
        assert!(names("gmail").contains(&"Tes comptes Google"));
        assert!(names("anniversaires").contains(&"Tes comptes Google"));
        assert!(names("slack").contains(&"Les notifications de tes applications"));
        assert!(names("batterie tri").contains(&"Quand trier"));
        assert!(names("ollama").contains(&"Le modèle"));
        assert!(names("secretary.triage.battery").contains(&"Quand trier"));
        for outside in outside() {
            assert_eq!(outside.section, SectionId::Secretary);
        }
    }

    #[test]
    fn the_triage_s_menu_writes_the_battery_key() {
        let setting = crate::model::setting(BATTERY).expect("the setting");
        let Places::One(place) = &setting.places else {
            panic!("one key");
        };
        assert_eq!(place.dotted(), "secretary.triage.battery");
        assert_eq!(place.default, Value::Int(50), "hers by default");
        assert_eq!(
            place
                .codec
                .write(&Value::Int(80))
                .and_then(|v| v.as_integer()),
            Some(80)
        );
        assert_eq!(
            place.codec.read(&101_i64.into()),
            None,
            "a charge, at most 100"
        );
        let Kind::Menu(choices) = &setting.kind else {
            panic!("a menu");
        };
        let values: Vec<&Value> = choices.iter().map(|c| &c.value).collect();
        assert_eq!(
            values,
            [
                &Value::Int(0),
                &Value::Int(20),
                &Value::Int(50),
                &Value::Int(80)
            ]
        );
        assert_eq!(
            button_said(BATTERY, &Value::Int(50), Lang::French).as_deref(),
            Some("Sur secteur, ou batterie au-dessus de 50 %")
        );
        // Her mode and her name moved here from Agents, their keys kept.
        assert_eq!(
            crate::model::setting(MODE).map(|s| s.section),
            Some(SectionId::Secretary)
        );
        assert_eq!(
            crate::model::setting(NAME).map(|s| s.section),
            Some(SectionId::Secretary)
        );
    }

    #[test]
    fn the_triage_and_its_model_are_said_in_plain_words() {
        assert_eq!(triage_said("running", 21, Lang::French), "21 en attente.");
        assert_eq!(
            triage_said("idle", 0, Lang::French),
            "Rien à trier pour l’instant."
        );
        assert_eq!(
            triage_said("paused: battery 32 %", 4, Lang::French),
            "En pause : la batterie est à 32 %. 4 en attente."
        );
        assert_eq!(
            triage_said("paused: model unavailable", 0, Lang::English),
            "Paused: the model does not answer."
        );
        assert!(
            triage_said(
                "paused: package 101 °C on average, resumes below 90 °C",
                0,
                Lang::French
            )
            .contains("chauffe")
        );
        assert_eq!(
            model_said("local/qwen3:8b", None, "idle", Lang::French),
            "Ollama, sur cet ordinateur · qwen3:8b · prêt"
        );
        assert_eq!(
            model_said(
                "local/qwen3:8b",
                Some("openai"),
                "paused: model unavailable",
                Lang::French
            ),
            "Un serveur compatible OpenAI · qwen3:8b · ne répond pas"
        );
    }

    #[test]
    fn an_agenda_says_how_many_calendars_she_reads() {
        let c = |n: &str, read| (n.to_owned(), read);
        assert_eq!(calendars_said(&[], Lang::French), None);
        assert_eq!(
            calendars_said(
                &[
                    c("Travail", true),
                    c("Équipe", true),
                    c("Claire", true),
                    c("Famille", false)
                ],
                Lang::French
            )
            .as_deref(),
            Some("Elle en lit 3 sur 4 · Famille laissé de côté")
        );
        assert_eq!(
            calendars_said(&[c("Travail", true), c("Perso", true)], Lang::French).as_deref(),
            Some("Elle les lit tous les 2")
        );
    }
}
