//! KDE's job view, served for KDE Connect's transfers (bar.md "The phone",
//! "A transfer's progress").
//!
//! KDE Connect reports its transfers through KIO's job tracker, which asks
//! `org.kde.kuiserver` at `/JobViewServer` whether it implements
//! `org.kde.JobViewServerV2` (by introspection) and whether it still
//! needs a tracker of its own (`requiresJobTracker`, false here), then
//! asks `org.kde.JobViewServer` for a view (`requestView`) and tells that
//! view the job's progress (`org.kde.JobViewV3`: `update`, `terminate`),
//! listening for its `cancelRequested`. Both names are taken here, without
//! queueing: another owner already there (a Plasma session's) keeps them,
//! and no progress shows.
//!
//! Every application is answered; what the views say goes to the worker,
//! which keeps KDE Connect's alone.

use std::{
    collections::HashMap,
    sync::atomic::{AtomicU32, Ordering},
};

use tokio::sync::mpsc::UnboundedSender;
use zbus::{
    Connection, ObjectServer, OwnedObjectPath, OwnedVariant, fdo::RequestNameFlags,
    fdo::RequestNameReply,
};

/// The names KIO's tracker asks.
const KUISERVER: &str = "org.kde.kuiserver";
const JOB_VIEW_SERVER: &str = "org.kde.JobViewServer";
pub(crate) const PATH: &str = "/JobViewServer";
pub(crate) const VIEW: &str = "org.kde.JobViewV3";
/// KDE Connect's desktop entry, which its views are asked with.
pub(crate) const KDECONNECT_ENTRY: &str = "org.kde.kdeconnect.daemon";

/// What a view says, for the worker.
#[derive(Debug)]
pub(crate) enum Job {
    /// A view asked for: its number, its application's desktop entry and
    /// what it says already.
    Requested {
        view: u32,
        entry: String,
        hints: HashMap<String, OwnedVariant>,
    },
    Update {
        view: u32,
        props: HashMap<String, OwnedVariant>,
    },
    /// Ended: 0 done, else stopped.
    Terminated { view: u32, code: u32 },
}

/// A view's object path.
pub(crate) fn view_path(view: u32) -> String {
    format!("{PATH}/JobView_{view}")
}

/// Serves the job view on `connection`, what its views say going to
/// `jobs`: whether it has both names.
pub(crate) async fn serve(connection: &Connection, jobs: UnboundedSender<Job>) -> bool {
    let server = connection.object_server();
    let served = async {
        server.at(PATH, Kuiserver).await?;
        server
            .at(
                PATH,
                ViewServer {
                    jobs,
                    next: AtomicU32::new(1),
                },
            )
            .await?;
        Ok::<_, zbus::Error>(())
    }
    .await;
    if let Err(err) = served {
        tracing::warn!(%err, "the job view: not served");
        return false;
    }
    for name in [KUISERVER, JOB_VIEW_SERVER] {
        match connection
            .request_name_with_flags(name, RequestNameFlags::DO_NOT_QUEUE)
            .await
        {
            Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {}
            Ok(reply) => {
                tracing::info!(
                    name,
                    ?reply,
                    "the job view has another owner: no transfer's progress"
                );
                return false;
            }
            Err(err) => {
                tracing::info!(name, %err, "the job view: no transfer's progress");
                return false;
            }
        }
    }
    tracing::info!("the job view is served");
    true
}

/// `org.kde.kuiserver`: no tracker of the application's own is needed.
struct Kuiserver;

#[zbus::interface(name = "org.kde.kuiserver")]
impl Kuiserver {
    #[zbus(name = "requiresJobTracker")]
    fn requires_job_tracker(&self) -> bool {
        false
    }
}

/// `org.kde.JobViewServerV2`: a view for each job.
struct ViewServer {
    jobs: UnboundedSender<Job>,
    next: AtomicU32,
}

#[zbus::interface(name = "org.kde.JobViewServerV2")]
impl ViewServer {
    #[zbus(name = "requestView")]
    async fn request_view(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        desktop_entry: String,
        _capabilities: i32,
        hints: HashMap<String, OwnedVariant>,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let view = self.next.fetch_add(1, Ordering::Relaxed);
        let path = view_path(view);
        server
            .at(
                path.as_str(),
                View {
                    view,
                    jobs: self.jobs.clone(),
                },
            )
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        tracing::debug!(view, entry = desktop_entry, "a job view asked for");
        let _ = self.jobs.send(Job::Requested {
            view,
            entry: desktop_entry,
            hints,
        });
        OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
    }
}

/// `org.kde.JobViewV3`: one job's progress.
struct View {
    view: u32,
    jobs: UnboundedSender<Job>,
}

#[zbus::interface(name = "org.kde.JobViewV3")]
impl View {
    #[zbus(name = "update")]
    fn update(&self, properties: HashMap<String, OwnedVariant>) {
        let _ = self.jobs.send(Job::Update {
            view: self.view,
            props: properties,
        });
    }

    #[zbus(name = "terminate")]
    async fn terminate(
        &self,
        #[zbus(object_server)] server: &ObjectServer,
        error_code: u32,
        _error_message: String,
        _hints: HashMap<String, OwnedVariant>,
    ) {
        let _ = self.jobs.send(Job::Terminated {
            view: self.view,
            code: error_code,
        });
        let path = view_path(self.view);
        if let Err(err) = server.remove::<Self, _>(path.as_str()).await {
            tracing::debug!(%err, view = self.view, "a job view already gone");
        }
    }
}
