//! Her read-only memory tools (§7.3, persona.md): `graph` and `search`,
//! for the model to go further than what context assembly gave it. Their
//! schemas are strict (every field required, nothing else allowed; an
//! optional one is nullable). They read her memory on her loop, which
//! owns it: [`run`] is called there, with a call the model made.
//!
//! The conversation's ([`TALK`], lot 2): `ask_user`, a question with its
//! quick replies, and the `propose_*` tools, each a card in the drawer that
//! acts only once the user clicks it (§2.1). They are never run here: the
//! conversation shows them.

use hocus_secretary_link::Notebook;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    brain::as_of,
    model::ToolCall,
    retrieve,
    store::{Item, Store},
};

/// A tool as a model is told of it.
#[derive(Debug, Clone, PartialEq)]
pub struct Spec {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: Value,
}

/// The read-only tools' names.
pub const READ: [&str; 2] = ["graph", "search"];

/// The conversation's tools: a question, and the proposals.
pub const TALK: [&str; 10] = [
    "ask_user",
    "propose_message",
    "propose_post",
    "propose_mail",
    "propose_invitation_answer",
    "propose_event",
    "propose_file",
    "propose_task",
    "propose_agent",
    "propose_delegation",
];

/// A proposal's input: the card's words (every field required, nullable
/// when it may be left out).
fn proposal(description: &'static str, name: &'static str) -> Spec {
    Spec {
        name,
        description,
        schema: json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["to", "label", "text", "draft", "lines"],
            "properties": {
                "to": {"type": ["string", "null"], "description": "À qui, s'il y a quelqu'un."},
                "label": {"type": ["string", "null"], "description": "Le titre de la carte, court."},
                "text": {"type": ["string", "null"], "description": "Une phrase qui dit ce que c'est."},
                "draft": {"type": ["string", "null"], "description": "Le texte exact proposé, s'il y en a un."},
                "lines": {"type": "array", "items": {"type": "string"}, "description": "Ses lignes : les étapes d'un plan, les tâches."}
            }
        }),
    }
}

pub fn spec(name: &str) -> Option<Spec> {
    match name {
        "ask_user" => Some(Spec {
            name: "ask_user",
            description: "Poser UNE question à l'utilisateur, avec ses réponses rapides (2 à 4, \
                courtes). Ce qu'elle a trouvé va dans note.",
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["question", "replies", "note"],
                "properties": {
                    "question": {"type": "string"},
                    "replies": {"type": "array", "items": {"type": "string"}},
                    "note": {"type": ["string", "null"]}
                }
            }),
        }),
        "propose_message" => Some(proposal(
            "Proposer un message (Slack, SMS) : une carte, rien n'est envoyé avant le clic de l'utilisateur.",
            "propose_message",
        )),
        "propose_post" => Some(proposal(
            "Proposer une publication : une carte, rien n'est publié avant le clic.",
            "propose_post",
        )),
        "propose_mail" => Some(proposal(
            "Proposer un mail : une carte, rien n'est envoyé avant le clic.",
            "propose_mail",
        )),
        "propose_invitation_answer" => Some(proposal(
            "Proposer une réponse à une invitation : une carte, rien n'est répondu avant le clic.",
            "propose_invitation_answer",
        )),
        "propose_event" => Some(proposal(
            "Proposer un événement d'agenda : une carte, rien n'est créé avant le clic.",
            "propose_event",
        )),
        "propose_file" => Some(proposal(
            "Proposer d'écrire un fichier : une carte, rien n'est écrit avant le clic.",
            "propose_file",
        )),
        "propose_task" => Some(proposal(
            "Proposer des tâches ou un plan de la journée : une carte, rien n'est ajouté avant le clic.",
            "propose_task",
        )),
        "propose_agent" => Some(proposal(
            "Proposer de lancer un agent : une carte, rien n'est lancé avant le clic.",
            "propose_agent",
        )),
        "propose_delegation" => Some(proposal(
            "Proposer de confier un travail à Claude Code : une carte, rien n'est lancé avant le clic.",
            "propose_delegation",
        )),
        "graph" => Some(Spec {
            name: "graph",
            description: "Le graphe de sa mémoire autour d'une personne, d'un projet, d'un client \
                ou d'un outil : les faits valides aujourd'hui, ou à une date (as_of, AAAA-MM-JJ). \
                Lecture seule.",
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["node", "hops", "as_of"],
                "properties": {
                    "node": {"type": "string", "description": "Un nom ou un alias."},
                    "hops": {"type": ["integer", "null"], "description": "Profondeur, 2 par défaut, 3 au plus."},
                    "as_of": {"type": ["string", "null"], "description": "AAAA-MM-JJ ; null : aujourd'hui."}
                }
            }),
        }),
        "search" => Some(Spec {
            name: "search",
            description: "Recherche plein texte dans sa mémoire (ce qu'elle a vu, ses résumés, \
                les fiches), entre deux dates si elles sont données. Lecture seule.",
            schema: json!({
                "type": "object",
                "additionalProperties": false,
                "required": ["query", "from", "to"],
                "properties": {
                    "query": {"type": "string"},
                    "from": {"type": ["string", "null"], "description": "AAAA-MM-JJ ou null."},
                    "to": {"type": ["string", "null"], "description": "AAAA-MM-JJ ou null."}
                }
            }),
        }),
        _ => None,
    }
}

#[derive(Deserialize)]
struct GraphInput {
    node: String,
    hops: Option<u8>,
    as_of: Option<String>,
}

#[derive(Deserialize)]
struct SearchInput {
    query: String,
    from: Option<String>,
    to: Option<String>,
}

/// What a read-only tool gives, as text for the model; an error is said
/// to it as one. `notebook`: what the moment allows (pro only in public,
/// §7.4).
pub fn run(store: &Store, call: &ToolCall, notebook: Option<Notebook>) -> Result<String, String> {
    match call.name.as_str() {
        "graph" => {
            let input: GraphInput =
                serde_json::from_value(call.input.clone()).map_err(|e| e.to_string())?;
            let day = match input.as_of.as_deref() {
                Some(text) => as_of(text).ok_or_else(|| format!("{text:?} n'est pas une date"))?,
                None => store.today(),
            };
            let start = store
                .find(&input.node)
                .map_err(|e| e.to_string())?
                .into_iter()
                .next()
                .ok_or_else(|| format!("rien sur {:?}", input.node))?;
            let around = store
                .neighbourhood(start, input.hops.unwrap_or(2).min(3), day, notebook)
                .map_err(|e| e.to_string())?;
            let mut out = format!("Valide au {day} :\n");
            for edge in &around.edges {
                out.push_str(&retrieve::edge_line(store, edge));
                out.push('\n');
            }
            Ok(out)
        }
        "search" => {
            let input: SearchInput =
                serde_json::from_value(call.input.clone()).map_err(|e| e.to_string())?;
            let from = input.from.as_deref().and_then(as_of);
            let to = input.to.as_deref().and_then(as_of);
            let mut out = String::new();
            for item in retrieve::search_any(store, &input.query, notebook, 20)
                .map_err(|e| e.to_string())?
            {
                if let Item::Episode(id) = item
                    && let Ok(Some(episode)) = retrieve::episode(store, id)
                {
                    let day = retrieve::day_of(episode.at);
                    if from.is_some_and(|f| day < f) || to.is_some_and(|t| day > t) {
                        continue;
                    }
                    out.push_str(&retrieve::episode_line(&episode));
                    out.push('\n');
                } else if let Item::Node(id) = item
                    && let Ok(node) = store.node(id)
                {
                    out.push_str(&format!("fiche : {} ({})\n", node.name, node.kind.word()));
                }
            }
            if out.is_empty() {
                out.push_str("rien trouvé");
            }
            Ok(out)
        }
        other => Err(format!("{other} n'est pas un de ses outils de lecture")),
    }
}
