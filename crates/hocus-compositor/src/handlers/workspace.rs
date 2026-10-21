//! ext-workspace-v1: each output is a workspace group, its desktops are the
//! workspaces. Clients (bars, switchers) see what the model shows and can
//! activate a desktop; creating, removing and moving desktops are not
//! offered.
//!
//! After every change of the model, each client gets what changed since what
//! it was last told, then `done`.

use hocus_core::{DesktopId, OutputName};
use smithay::{
    output::Output,
    reexports::{
        wayland_protocols::ext::workspace::v1::server::{
            ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
            ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
            ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            backend::GlobalId,
        },
    },
    utils::SERIAL_COUNTER,
};

use crate::State;

/// The global and what each bound manager was told.
#[derive(Debug)]
pub struct WorkspaceState {
    _global: GlobalId,
    managers: Vec<Manager>,
}

#[derive(Debug)]
struct Manager {
    resource: ExtWorkspaceManagerV1,
    groups: Vec<(OutputName, ExtWorkspaceGroupHandleV1)>,
    workspaces: Vec<Told>,
    /// Activations asked for, applied on commit.
    pending: Vec<DesktopId>,
}

/// A workspace and what its client last heard about it.
#[derive(Debug)]
struct Told {
    desktop: DesktopId,
    handle: ExtWorkspaceHandleV1,
    name: String,
    index: u32,
    group: Option<OutputName>,
    active: bool,
}

/// What the model says about one desktop.
#[derive(Debug, Clone, PartialEq)]
struct Truth {
    desktop: DesktopId,
    name: String,
    index: u32,
    group: OutputName,
    active: bool,
}

impl WorkspaceState {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            _global: dh.create_global::<State, ExtWorkspaceManagerV1, _>(1, ()),
            managers: Vec::new(),
        }
    }
}

impl State {
    /// Tells every workspace client what changed in the model.
    pub fn refresh_workspaces(&mut self) {
        let truth = self.workspace_truth();
        let outputs: Vec<Output> = self.space.outputs().cloned().collect();
        let dh = self.display_handle.clone();
        for manager in &mut self.workspaces.managers {
            manager.update(&dh, &truth, &outputs);
        }
    }

    fn workspace_truth(&self) -> Vec<Truth> {
        self.model
            .outputs()
            .iter()
            .flat_map(|output| {
                output.desktops.iter().enumerate().filter_map(|(i, id)| {
                    let desktop = self.model.desktop(*id)?;
                    Some(Truth {
                        desktop: *id,
                        name: desktop.name.clone(),
                        index: i as u32,
                        group: output.name.clone(),
                        active: output.shown == *id,
                    })
                })
            })
            .collect()
    }
}

impl Manager {
    fn update(&mut self, dh: &DisplayHandle, truth: &[Truth], outputs: &[Output]) {
        let Ok(client) = dh.get_client(self.resource.id()) else {
            return;
        };
        // Groups: one per output with desktops.
        let mut wanted: Vec<OutputName> = Vec::new();
        for t in truth {
            if !wanted.contains(&t.group) {
                wanted.push(t.group.clone());
            }
        }
        // Workspaces leave a group before it goes.
        for told in &mut self.workspaces {
            let still = truth.iter().find(|t| t.desktop == told.desktop);
            if told.group.as_ref() != still.map(|t| &t.group)
                && let Some(old) = told.group.take()
                && let Some((_, group)) = self.groups.iter().find(|(name, _)| *name == old)
            {
                group.workspace_leave(&told.handle);
            }
        }
        self.workspaces.retain(|told| {
            let keep = truth.iter().any(|t| t.desktop == told.desktop);
            if !keep {
                told.handle.removed();
            }
            keep
        });
        self.groups.retain(|(name, group)| {
            let keep = wanted.contains(name);
            if !keep {
                group.removed();
            }
            keep
        });
        for name in &wanted {
            if self.groups.iter().any(|(n, _)| n == name) {
                continue;
            }
            let Ok(group) = client.create_resource::<ExtWorkspaceGroupHandleV1, _, State>(
                dh,
                self.resource.version(),
                name.clone(),
            ) else {
                continue;
            };
            self.resource.workspace_group(&group);
            group.capabilities(ext_workspace_group_handle_v1::GroupCapabilities::empty());
            if let Some(output) = outputs.iter().find(|o| o.name() == name.0) {
                for wl_output in output.client_outputs(&client) {
                    group.output_enter(&wl_output);
                }
            }
            self.groups.push((name.clone(), group));
        }
        for t in truth {
            let index = match self.workspaces.iter().position(|w| w.desktop == t.desktop) {
                Some(index) => index,
                None => {
                    let Ok(handle) = client.create_resource::<ExtWorkspaceHandleV1, _, State>(
                        dh,
                        self.resource.version(),
                        t.desktop,
                    ) else {
                        continue;
                    };
                    self.resource.workspace(&handle);
                    // Stable across hot plug, like the desktop.
                    handle.id(format!("hocuspanda-desktop-{}", t.desktop.0));
                    handle.capabilities(ext_workspace_handle_v1::WorkspaceCapabilities::Activate);
                    self.workspaces.push(Told {
                        desktop: t.desktop,
                        handle,
                        name: String::new(),
                        index: u32::MAX,
                        group: None,
                        active: false,
                    });
                    // Sent below as changes from nothing.
                    self.workspaces.len() - 1
                }
            };
            let told = &mut self.workspaces[index];
            if told.name != t.name {
                told.handle.name(t.name.clone());
                told.name = t.name.clone();
            }
            if told.index != t.index {
                told.handle.coordinates(t.index.to_ne_bytes().to_vec());
                told.index = t.index;
            }
            if told.group.as_ref() != Some(&t.group) {
                if let Some((_, group)) = self.groups.iter().find(|(n, _)| *n == t.group) {
                    group.workspace_enter(&told.handle);
                }
                told.group = Some(t.group.clone());
                // The state goes with the first announcement too.
                told.active = !t.active;
            }
            if told.active != t.active {
                let state = if t.active {
                    ext_workspace_handle_v1::State::Active
                } else {
                    ext_workspace_handle_v1::State::empty()
                };
                told.handle.state(state);
                told.active = t.active;
            }
        }
        self.resource.done();
    }
}

impl GlobalDispatch<ExtWorkspaceManagerV1, ()> for State {
    fn bind(
        state: &mut Self,
        _dh: &DisplayHandle,
        _client: &Client,
        resource: New<ExtWorkspaceManagerV1>,
        _global_data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        let resource = data_init.init(resource, ());
        state.workspaces.managers.push(Manager {
            resource,
            groups: Vec::new(),
            workspaces: Vec::new(),
            pending: Vec::new(),
        });
        state.refresh_workspaces();
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ExtWorkspaceManagerV1,
        request: ext_workspace_manager_v1::Request,
        _data: &(),
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        let Some(index) = state
            .workspaces
            .managers
            .iter()
            .position(|m| m.resource == *resource)
        else {
            return;
        };
        match request {
            ext_workspace_manager_v1::Request::Commit => {
                let pending = std::mem::take(&mut state.workspaces.managers[index].pending);
                for desktop in pending {
                    state.activate_desktop(desktop);
                }
            }
            ext_workspace_manager_v1::Request::Stop => {
                let manager = state.workspaces.managers.remove(index);
                manager.resource.finished();
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _client: smithay::reexports::wayland_server::backend::ClientId,
        resource: &ExtWorkspaceManagerV1,
        _data: &(),
    ) {
        state
            .workspaces
            .managers
            .retain(|m| m.resource != *resource);
    }
}

impl Dispatch<ExtWorkspaceGroupHandleV1, OutputName> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ExtWorkspaceGroupHandleV1,
        request: ext_workspace_group_handle_v1::Request,
        _data: &OutputName,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        // Creating desktops is not offered (no capability): only destroy.
        if let ext_workspace_group_handle_v1::Request::Destroy = request {
            for manager in &mut state.workspaces.managers {
                manager.groups.retain(|(_, g)| g != resource);
            }
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, DesktopId> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ExtWorkspaceHandleV1,
        request: ext_workspace_handle_v1::Request,
        desktop: &DesktopId,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        match request {
            ext_workspace_handle_v1::Request::Activate => {
                if let Some(manager) = state
                    .workspaces
                    .managers
                    .iter_mut()
                    .find(|m| m.workspaces.iter().any(|w| w.handle == *resource))
                {
                    manager.pending.push(*desktop);
                }
            }
            ext_workspace_handle_v1::Request::Destroy => {
                for manager in &mut state.workspaces.managers {
                    manager.workspaces.retain(|w| w.handle != *resource);
                }
            }
            // Not offered in the capabilities.
            _ => {}
        }
    }
}

impl State {
    /// Shows a desktop on the output hosting it, as Super+N would.
    fn activate_desktop(&mut self, desktop: DesktopId) {
        let Some(host) = self.model.host_of(desktop).map(|o| o.name.clone()) else {
            return;
        };
        let Some(output) = self.find_output(&host.0) else {
            return;
        };
        let index = self
            .model
            .output(&host)
            .and_then(|o| o.desktops.iter().position(|d| *d == desktop));
        if let Some(index) = index
            && let Err(err) = self.show_desktop(&output, index, SERIAL_COUNTER.next_serial())
        {
            tracing::warn!(%err, "could not activate a workspace");
        }
    }
}
