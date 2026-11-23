//! NetworkManager's secret agent (#930), on a thread of its own: it
//! serves `org.freedesktop.NetworkManager.SecretAgent` on the system bus
//! and registers with NetworkManager's `AgentManager`, again whenever
//! NetworkManager starts again. What a request asks is read by
//! [`hocus_network::agent`]; what the session's keyring holds answers
//! without asking; else the dialog asks ([`super::form`]) and the call is
//! answered once it is: the secrets typed, `UserCanceled` when cancelled,
//! `AgentCanceled` when NetworkManager withdrew it, `NoSecrets` when
//! nothing may be asked. Only NetworkManager's own calls are taken.
//!
//! The agent keeps the secrets their flags give it (agent-owned) in the
//! session's keyring ([`crate::keyring`], libnm's attributes), when the
//! user said "Retenir le mot de passe" and when NetworkManager says
//! `SaveSecrets`; never a one-time code, nor a secret asked each time.
//!
//! The shell is the agent only when it is the session's
//! (`HOCUS_AUTH_AGENT`, as for polkit): a shell nested in another session
//! leaves that session's agent alone.

use std::time::Instant;

use hocus_link::AUTH_AGENT_ENV as ENV;
use hocus_network::agent::{self, Settings};
use iced::futures::StreamExt;
use tokio::sync::oneshot;
use zbus::{Connection, OwnedObjectPath, fdo::DBusProxy, message::Header};

use super::{
    Answer, Come,
    form::{self, Given},
};
use crate::{
    keyring,
    levels::{RETRY_FIRST, next_retry},
    surfaces::Asked,
};

const NM: &str = "org.freedesktop.NetworkManager";

pub fn start() {
    if std::env::var_os(ENV).is_none() {
        tracing::info!("not the session's own shell: no NetworkManager secret agent");
        return;
    }
    let started = std::thread::Builder::new()
        .name("nm-agent".to_owned())
        .spawn(|| {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(runtime) => runtime.block_on(run()),
                Err(err) => tracing::warn!(%err, "no runtime for the secret agent"),
            }
        });
    if let Err(err) = started {
        tracing::warn!(%err, "cannot start the secret agent");
    }
}

/// NetworkManager's errors, as its agents answer.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.NetworkManager.SecretAgent")]
enum Refusal {
    #[zbus(error)]
    ZBus(zbus::Error),
    NotAuthorized(String),
    UserCanceled(String),
    AgentCanceled(String),
    NoSecrets(String),
}

/// The session's keyring's bus: the session bus the compositor names.
fn keyring_bus() -> Option<String> {
    crate::day::session_bus()
}

struct Agent;

impl Agent {
    /// Whether `header`'s call comes from NetworkManager.
    async fn from_nm(connection: &Connection, header: &Header<'_>) -> bool {
        let Some(sender) = header.sender() else {
            return false;
        };
        let Ok(bus) = DBusProxy::new(connection).await else {
            return false;
        };
        let Ok(name) = NM.try_into() else {
            return false;
        };
        bus.get_name_owner(name)
            .await
            .is_ok_and(|owner| owner.as_str() == sender.as_str())
    }
}

#[zbus::interface(name = "org.freedesktop.NetworkManager.SecretAgent")]
impl Agent {
    #[allow(clippy::too_many_arguments)]
    async fn get_secrets(
        &self,
        connection: Settings,
        connection_path: OwnedObjectPath,
        setting_name: String,
        hints: Vec<String>,
        flags: u32,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<Settings, Refusal> {
        if !Self::from_nm(bus, &header).await {
            tracing::warn!("secrets asked by another than NetworkManager: refused");
            return Err(Refusal::NotAuthorized(
                "only NetworkManager asks".to_owned(),
            ));
        }
        let since = Instant::now();
        let mut asked = agent::asked(&connection, &setting_name, &hints, flags);
        tracing::info!(
            setting = asked.setting,
            flags,
            wanted = ?asked.wanted,
            "NetworkManager asks for secrets"
        );
        if asked.wanted.is_empty() {
            return Err(Refusal::NoSecrets("nothing this agent can ask".to_owned()));
        }
        // What the keyring holds.
        let keys = asked.from_keyring();
        if let (false, Some(bus)) = (keys.is_empty(), keyring_bus()) {
            let (uuid, setting) = (asked.uuid.clone(), asked.setting.clone());
            let found = keyring::within(&bus, async move |keyring| {
                let mut found = Vec::new();
                for key in keys {
                    let attributes = agent::attributes(&uuid, &setting, &key);
                    if let Some(value) = keyring.find(&attributes).await? {
                        found.push((key, value));
                    }
                }
                Ok(found)
            })
            .await
            .unwrap_or_default();
            for (key, value) in found {
                let value = String::from_utf8(value).ok().filter(|v| !v.is_empty());
                if let Some(wanted) = asked.wanted.iter_mut().find(|w| w.key == key) {
                    wanted.known = value;
                }
            }
        }
        if asked.answered() {
            tracing::info!("the secrets held answer");
            let values: Vec<(String, String)> = asked
                .wanted
                .iter()
                .filter_map(|w| Some((w.key.clone(), w.known.clone()?)))
                .collect();
            return Ok(agent::reply(&asked.setting, &values));
        }
        if !asked.interactive() || asked.left_to_the_popover() {
            tracing::info!("no secret to answer with, and none to ask");
            return Err(Refusal::NoSecrets("no secrets".to_owned()));
        }
        let (answer, answered) = oneshot::channel();
        let ask = form::Ask {
            id: super::next_id(),
            path: connection_path.as_str().to_owned(),
            asked: asked.clone(),
            since,
            answer: Answer::new(answer),
        };
        crate::surfaces::ask(Asked::Auth(Come::Secrets(Box::new(ask))));
        match answered.await {
            Ok(Given::Secrets { values, remember }) => {
                let values: Vec<(String, String)> = values
                    .iter()
                    .map(|(key, secret)| (key.clone(), secret.as_str().to_owned()))
                    .collect();
                keep(&asked, &values, remember);
                tracing::info!(remember, "the secrets are given");
                Ok(agent::reply(&asked.setting, &values))
            }
            Ok(Given::Withdrawn) => {
                tracing::info!("NetworkManager withdrew its request");
                Err(Refusal::AgentCanceled("withdrawn".to_owned()))
            }
            Ok(Given::Cancelled) | Err(_) => {
                tracing::info!("the user cancelled");
                Err(Refusal::UserCanceled("cancelled".to_owned()))
            }
        }
    }

    async fn cancel_get_secrets(
        &self,
        connection_path: OwnedObjectPath,
        setting_name: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), Refusal> {
        if !Self::from_nm(bus, &header).await {
            return Err(Refusal::NotAuthorized(
                "only NetworkManager cancels".to_owned(),
            ));
        }
        crate::surfaces::ask(Asked::Auth(Come::Withdraw {
            path: connection_path.as_str().to_owned(),
            setting: setting_name,
        }));
        Ok(())
    }

    async fn save_secrets(
        &self,
        connection: Settings,
        _connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), Refusal> {
        if !Self::from_nm(bus, &header).await {
            return Err(Refusal::NotAuthorized(
                "only NetworkManager saves".to_owned(),
            ));
        }
        let (Some(uuid), Some(keyring_bus)) = (agent::uuid_of(&connection), keyring_bus()) else {
            return Ok(());
        };
        let name = name_of(&connection);
        let owned = agent::agent_owned(&connection);
        tracing::info!(secrets = ?owned, "NetworkManager saves the agent's secrets");
        tokio::spawn(async move {
            keyring::within(&keyring_bus, async move |keyring| {
                for secret in owned {
                    let attributes = agent::attributes(&uuid, &secret.setting, &secret.key);
                    let label = agent::label(&name, &secret.setting, &secret.key);
                    keyring
                        .keep(
                            &label,
                            &attributes,
                            secret.value.clone().into_bytes(),
                            "text/plain",
                        )
                        .await?;
                }
                Ok(())
            })
            .await;
        });
        Ok(())
    }

    async fn delete_secrets(
        &self,
        connection: Settings,
        _connection_path: OwnedObjectPath,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] bus: &Connection,
    ) -> Result<(), Refusal> {
        if !Self::from_nm(bus, &header).await {
            return Err(Refusal::NotAuthorized(
                "only NetworkManager deletes".to_owned(),
            ));
        }
        let (Some(uuid), Some(keyring_bus)) = (agent::uuid_of(&connection), keyring_bus()) else {
            return Ok(());
        };
        tracing::info!("NetworkManager deletes the agent's secrets of a connection");
        tokio::spawn(async move {
            keyring::within(&keyring_bus, async move |keyring| {
                keyring.forget(&agent::connection_attributes(&uuid)).await
            })
            .await;
        });
        Ok(())
    }
}

fn name_of(connection: &Settings) -> String {
    connection
        .get("connection")
        .and_then(|c| c.get("id"))
        .and_then(|v| v.downcast_ref::<String>().ok())
        .unwrap_or_default()
}

/// What the agent may keep of `values` goes in the keyring when the user
/// said to remember it, and out of it when not; after the answer, never
/// holding it up.
fn keep(asked: &agent::Asked, values: &[(String, String)], remember: bool) {
    let Some(bus) = keyring_bus() else {
        return;
    };
    let mine: Vec<(String, String)> = values
        .iter()
        .filter(|(key, _)| asked.wanted.iter().any(|w| &w.key == key && w.in_keyring()))
        .cloned()
        .collect();
    if mine.is_empty() {
        return;
    }
    let (uuid, name, setting) = (
        asked.uuid.clone(),
        asked.name.clone(),
        asked.setting.clone(),
    );
    tokio::spawn(async move {
        keyring::within(&bus, async move |keyring| {
            for (key, value) in mine {
                let attributes = agent::attributes(&uuid, &setting, &key);
                if remember {
                    let label = agent::label(&name, &setting, &key);
                    keyring
                        .keep(&label, &attributes, value.into_bytes(), "text/plain")
                        .await?;
                } else {
                    keyring.forget(&attributes).await?;
                }
            }
            Ok(())
        })
        .await;
    });
}

async fn register(connection: &Connection) -> zbus::Result<()> {
    connection
        .call_method(
            Some(NM),
            agent::MANAGER_PATH,
            Some(agent::MANAGER),
            "RegisterWithCapabilities",
            &(agent::IDENTIFIER, agent::VPN_HINTS),
        )
        .await?;
    Ok(())
}

async fn run() {
    let mut wait = RETRY_FIRST;
    let connection = loop {
        match Connection::system().await {
            Ok(connection) => break connection,
            Err(err) => {
                tracing::info!(%err, "no system bus yet: no secret agent");
                tokio::time::sleep(wait).await;
                wait = next_retry(wait);
            }
        }
    };
    if let Err(err) = connection.object_server().at(agent::PATH, Agent).await {
        tracing::warn!(%err, "cannot serve the secret agent");
        return;
    }
    let owners = match DBusProxy::new(&connection).await {
        Ok(bus) => bus.receive_name_owner_changed_with_args(&[(0, NM)]).await,
        Err(err) => Err(err),
    };
    let mut owners = match owners {
        Ok(owners) => Some(owners),
        Err(err) => {
            tracing::warn!(%err, "NetworkManager's restarts are not followed");
            None
        }
    };
    loop {
        match register(&connection).await {
            Ok(()) => tracing::info!("NetworkManager's secret agent"),
            Err(err) => tracing::info!(
                %err,
                "NetworkManager did not take the secret agent (not running yet?)"
            ),
        }
        // NetworkManager started again (or for the first time): register
        // again.
        let Some(changes) = owners.as_mut() else {
            std::future::pending::<()>().await;
            return;
        };
        loop {
            let Some(changed) = changes.next().await else {
                std::future::pending::<()>().await;
                return;
            };
            if changed.args().is_ok_and(|a| a.new_owner().is_some()) {
                break;
            }
        }
    }
}
