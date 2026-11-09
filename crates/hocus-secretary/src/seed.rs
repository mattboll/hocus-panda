//! A memory fixture (tests, screenshots): people, projects, their facts and
//! rules, written into an empty memory at her start (`--memory-fixture`),
//! with the dates they were said or read at.
//!
//! ```toml
//! [[node]]
//! name = "Nadia Ferreira"
//! kind = "person"              # person, project, client, tool, user…
//! notebook = "pro"
//! aliases = ["Nad"]
//! [[edge]]
//! src = "Nadia Ferreira"
//! type = "leads"
//! dst = "Chapitô"              # or value = "…"
//! from = "2026-03-01"
//! provenance = "told"          # told, extracted (source, item, quote), inferred
//! said = "2026-10-12T09:00:00Z" # when she learned it
//! accepted = true              # a proposal when false
//! [[rule]]
//! sentence = "jamais de rappel pendant le daily"
//! compiled = { kind = "restraint", event = "daily" }
//! said = "2026-10-14T09:00:00Z"
//! ```

use hocus_secretary_link::{Notebook, Provenance};
use jiff::{Timestamp, civil::Date};
use rusqlite::params;
use serde::Deserialize;

use crate::{
    graph::{NewEdge, NodeKind, Origin, Target},
    learn::{self, Compiled},
    store::{Item, Result, Store, Who, stamp},
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seed {
    #[serde(default)]
    pub node: Vec<SeedNode>,
    #[serde(default)]
    pub edge: Vec<SeedEdge>,
    #[serde(default)]
    pub rule: Vec<SeedRule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedNode {
    pub name: String,
    #[serde(default = "person")]
    pub kind: String,
    #[serde(default = "pro")]
    pub notebook: Notebook,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub said: Option<Timestamp>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedEdge {
    pub src: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub dst: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub from: Option<Date>,
    #[serde(default)]
    pub to: Option<Date>,
    #[serde(default = "told")]
    pub provenance: Provenance,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub item: Option<String>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub pointer: Option<String>,
    #[serde(default)]
    pub said: Option<Timestamp>,
    #[serde(default)]
    pub accepted: Option<bool>,
    #[serde(default = "pro")]
    pub notebook: Notebook,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeedRule {
    pub sentence: String,
    #[serde(default)]
    pub compiled: Option<Compiled>,
    #[serde(default = "told")]
    pub provenance: Provenance,
    #[serde(default)]
    pub said: Option<Timestamp>,
    #[serde(default = "pro")]
    pub notebook: Notebook,
}

fn person() -> String {
    "person".to_owned()
}

const fn pro() -> Notebook {
    Notebook::Pro
}

const fn told() -> Provenance {
    Provenance::Told
}

impl Seed {
    pub fn from_file(path: &std::path::Path) -> std::result::Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Written into `store` when it holds no node yet: once.
    pub fn write(&self, store: &mut Store) -> Result<bool> {
        if store.count("nodes")? > 0 {
            return Ok(false);
        }
        let at = |store: &Store, table: &str, id: i64, said: Option<Timestamp>| -> Result<()> {
            if let Some(said) = said {
                let column = if table == "nodes" {
                    "created"
                } else {
                    "recorded_at"
                };
                store.conn.execute(
                    &format!("UPDATE {table} SET {column} = ?1 WHERE id = ?2"),
                    params![stamp(said), id],
                )?;
                if table != "nodes" {
                    store.conn.execute(
                        &format!(
                            "UPDATE {table} SET accepted_at = ?1 WHERE id = ?2 AND accepted_at IS NOT NULL"
                        ),
                        params![stamp(said), id],
                    )?;
                }
            }
            Ok(())
        };
        for node in &self.node {
            let kind = NodeKind::of(&node.kind).unwrap_or(NodeKind::Topic);
            let who = node.source.clone().map_or(Who::User, Who::Source);
            let (id, _) = store.add_node(kind, &node.name, node.notebook, &who, "a fixture")?;
            if !node.summary.is_empty() {
                store.conn.execute(
                    "UPDATE nodes SET summary = ?1 WHERE id = ?2",
                    params![node.summary, id],
                )?;
            }
            at(store, "nodes", id, node.said)?;
            for alias in &node.aliases {
                let (alias, _) =
                    store.add_alias(id, alias, &Origin::told("fixture"), node.notebook)?;
                if let Some(said) = node.said {
                    store.conn.execute(
                        "UPDATE aliases SET since = ?1, accepted_at = ?1 WHERE id = ?2",
                        params![stamp(said), alias],
                    )?;
                }
            }
        }
        let named = |store: &Store, name: &str| -> Result<i64> {
            store
                .find(name)?
                .into_iter()
                .next()
                .ok_or(crate::store::Error::Refused(format!(
                    "no node {name:?} in the fixture"
                )))
        };
        for edge in &self.edge {
            let src = named(store, &edge.src)?;
            let target = match (&edge.dst, &edge.value) {
                (Some(dst), _) => Target::Node(named(store, dst)?),
                (None, Some(value)) => Target::Value(value.clone()),
                (None, None) => Target::Value(String::new()),
            };
            let origin = match edge.provenance {
                Provenance::Told => {
                    Origin::told(edge.pointer.clone().unwrap_or_else(|| "fixture".to_owned()))
                }
                Provenance::Extracted => Origin::extracted(
                    edge.source.as_deref().unwrap_or("fixture"),
                    edge.item.as_deref().unwrap_or("1"),
                    edge.quote.clone().unwrap_or_default(),
                ),
                Provenance::Inferred => Origin::inferred(
                    edge.pointer.clone().unwrap_or_else(|| "fixture".to_owned()),
                    0.8,
                ),
            };
            let (id, _) = store.add_edge(&NewEdge {
                src,
                kind: edge.kind.clone(),
                target,
                valid_from: edge.from,
                valid_to: edge.to,
                origin,
                notebook: edge.notebook,
            })?;
            let proposal = store.edge(id)?.is_proposal();
            if proposal && edge.accepted == Some(true) {
                store.accept(Item::Edge(id))?;
            }
            at(store, "edges", id, edge.said)?;
        }
        for rule in &self.rule {
            let compiled = rule.compiled.clone().unwrap_or(Compiled::Restraint {
                event: String::new(),
            });
            let origin = match rule.provenance {
                Provenance::Inferred => Origin::inferred("seen:0", 0.8),
                _ => Origin::told("fixture"),
            };
            let (id, _) =
                learn::add_rule(store, &rule.sentence, &compiled, &origin, rule.notebook)?;
            if rule.compiled.is_none() {
                store
                    .conn
                    .execute("UPDATE rules SET compiled = NULL WHERE id = ?1", [id])?;
            }
            if rule.provenance == Provenance::Inferred {
                store.accept(Item::Rule(id))?;
            }
            at(store, "rules", id, rule.said)?;
        }
        Ok(true)
    }
}
