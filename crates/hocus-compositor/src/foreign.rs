//! xdg-foreign (unstable v2, #595): a window of one client made the child
//! of another's, as the desktop portal's dialogs are made children of the
//! window that asked for them (`parent_window`, `wayland:HANDLE`).
//!
//! A client exports its toplevel and gets a handle; another imports that
//! handle and makes its own toplevel a child of it. The child is the
//! parent's dialog: it opens above it, with the focus when its parent has
//! it, and stays above it ([`crate::model::Model::raise`]). The shell's
//! surfaces are layer surfaces, which the protocol cannot export: the
//! shell asks the compositor over its link instead
//! ([`hocus_link::ToCompositor::Export`]), and a child of that handle is
//! a dialog the user asked for from the shell (the capture's editor's
//! "Enregistrer sous…"), which takes the focus.
//!
//! Not smithay's `XdgForeignState`: its import refuses any parent but an
//! xdg toplevel with a protocol error, which would end the portal's
//! backend, and the shell's handle names a layer surface.

use std::{collections::HashMap, time::Duration};

use smithay::{
    reexports::{
        wayland_protocols::xdg::foreign::zv2::server::{
            zxdg_exported_v2::{self, ZxdgExportedV2},
            zxdg_exporter_v2::{self, ZxdgExporterV2},
            zxdg_imported_v2::{self, ZxdgImportedV2},
            zxdg_importer_v2::{self, ZxdgImporterV2},
        },
        wayland_server::{
            Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
            backend::ClientId, protocol::wl_surface::WlSurface,
        },
    },
    wayland::{
        compositor::{get_role, with_states},
        shell::{
            is_valid_parent,
            xdg::{XDG_TOPLEVEL_ROLE, XdgToplevelSurfaceData},
        },
    },
};

use crate::{State, model::WindowId};

/// How long a handle made for the shell lets a window be its child: the
/// portal's backend may take a while to start the first time.
const SHELL_KEPT: Duration = Duration::from_secs(60);

/// What a handle names.
#[derive(Debug, Clone, PartialEq)]
enum Exported {
    /// A client's toplevel, exported through `zxdg_exporter_v2`.
    Toplevel(WlSurface),
    /// A surface of the shell's, exported for it, when.
    Shell(Duration),
}

/// A toplevel made a child through a handle.
#[derive(Debug)]
struct Child {
    surface: WlSurface,
    handle: String,
    parent: Exported,
    imported: ZxdgImportedV2,
}

/// The handles made, and the children made through them.
#[derive(Debug, Default)]
pub struct Foreign {
    exported: HashMap<String, Exported>,
    children: Vec<Child>,
    made: u64,
}

/// Whose dialog a window is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogOf {
    /// A window's: its xdg parent, set by its own client or through a
    /// handle.
    Window(WindowId),
    /// The shell's: a child of a handle made for it.
    Shell,
}

impl State {
    /// The protocol's globals.
    pub fn foreign_globals(dh: &DisplayHandle) {
        dh.create_global::<State, ZxdgExporterV2, _>(1, ());
        dh.create_global::<State, ZxdgImporterV2, _>(1, ());
    }

    /// A handle for the shell's layer surface of `namespace`, if one is
    /// there ([`hocus_link::ToCompositor::Export`]).
    pub fn export_for_shell(&mut self, namespace: &str) -> Option<String> {
        let there = self.space.outputs().any(|output| {
            smithay::desktop::layer_map_for_output(output)
                .layers()
                .any(|l| l.namespace() == namespace)
        });
        if !there {
            tracing::info!(namespace, "no shell surface to export");
            return None;
        }
        let now = self.now();
        self.foreign.exported.retain(|_, e| match e {
            Exported::Shell(made) => now.saturating_sub(*made) < SHELL_KEPT,
            Exported::Toplevel(_) => true,
        });
        let handle = self.foreign.handle();
        tracing::debug!(namespace, "a shell surface exported");
        self.foreign
            .exported
            .insert(handle.clone(), Exported::Shell(now));
        Some(handle)
    }

    /// Whose dialog `surface`'s window is, if anyone's.
    pub fn dialog_of(&self, surface: &WlSurface) -> Option<DialogOf> {
        if self
            .foreign
            .children
            .iter()
            .any(|c| c.surface == *surface && matches!(c.parent, Exported::Shell(_)))
        {
            return Some(DialogOf::Shell);
        }
        // smithay's surface data is not there once a surface is gone.
        if !surface.is_alive() {
            return None;
        }
        let parent = with_states(surface, |states| {
            states
                .data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok().and_then(|d| d.parent.clone()))
        })?;
        let window = self.window_for(&parent)?;
        self.id_of(&window).map(DialogOf::Window)
    }

    /// Whether `surface`'s window, a dialog, takes the focus: the shell's
    /// does, and a window's whose parent has it.
    pub fn dialog_focuses(&self, surface: &WlSurface) -> bool {
        match self.dialog_of(surface) {
            Some(DialogOf::Shell) => true,
            Some(DialogOf::Window(parent)) => self.model.focused() == Some(parent),
            None => false,
        }
    }

    /// `child` (an xdg toplevel) made the child of what `handle` names.
    fn set_parent_of(&mut self, imported: &ZxdgImportedV2, handle: &str, child: WlSurface) {
        if !child.is_alive() {
            return;
        }
        if get_role(&child) != Some(XDG_TOPLEVEL_ROLE) {
            imported.post_error(
                zxdg_imported_v2::Error::InvalidSurface,
                "the child is not an xdg_toplevel",
            );
            return;
        }
        let Some(parent) = self.foreign.exported.get(handle).cloned() else {
            return;
        };
        self.foreign.children.retain(|c| c.surface.is_alive());
        // One parent at a time.
        self.unparent(|c| c.surface == child);
        if let Exported::Toplevel(parent) = &parent {
            if !parent.is_alive() || !is_valid_parent(&child, parent) {
                tracing::debug!("a foreign parent refused: gone, or a loop");
                return;
            }
            with_states(&child, |states| {
                if let Some(Ok(mut role)) = states
                    .data_map
                    .get::<XdgToplevelSurfaceData>()
                    .map(|d| d.lock())
                {
                    role.parent = Some(parent.clone());
                }
            });
        }
        tracing::debug!(
            shell = matches!(parent, Exported::Shell(_)),
            "a foreign child"
        );
        self.foreign.children.push(Child {
            surface: child.clone(),
            handle: handle.to_owned(),
            parent,
            imported: imported.clone(),
        });
        self.parent_changed_of(&child);
    }

    /// The children made through a handle that match `which` are no
    /// longer.
    fn unparent(&mut self, which: impl Fn(&Child) -> bool) {
        let (gone, kept): (Vec<Child>, Vec<Child>) = std::mem::take(&mut self.foreign.children)
            .into_iter()
            .partition(|c| which(c));
        self.foreign.children = kept;
        for child in gone {
            if !child.surface.is_alive() {
                continue;
            }
            if let Exported::Toplevel(parent) = &child.parent {
                with_states(&child.surface, |states| {
                    if let Some(Ok(mut role)) = states
                        .data_map
                        .get::<XdgToplevelSurfaceData>()
                        .map(|d| d.lock())
                        && role.parent.as_ref() == Some(parent)
                    {
                        role.parent = None;
                    }
                });
            }
            self.parent_changed_of(&child.surface);
        }
    }

    /// `surface`'s parent changed: its window's place in the stack
    /// follows, and it comes in front if it takes the focus now.
    pub fn parent_changed_of(&mut self, surface: &WlSurface) {
        let Some(id) = self.window_for(surface).and_then(|w| self.id_of(&w)) else {
            return;
        };
        let parent = match self.dialog_of(surface) {
            Some(DialogOf::Window(parent)) => Some(parent),
            _ => None,
        };
        self.model.set_parent(id, parent);
        if self.dialog_focuses(surface) && self.model.focused() != Some(id) {
            self.activate(id, true);
        } else if let Some(focused) = self.model.focused() {
            // Re-raised, the focused window takes its dialogs above it.
            self.model.raise(focused);
            self.restack();
        }
        self.redraw = true;
    }
}

impl Foreign {
    /// A new handle: 32 letters and digits, random.
    fn handle(&mut self) -> String {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
        self.made += 1;
        let mut bytes = [0u8; 32];
        match rustix::rand::getrandom(&mut bytes[..], rustix::rand::GetRandomFlags::empty()) {
            Ok(32) => bytes
                .iter()
                .filter_map(|b| CHARS.get(usize::from(*b) % CHARS.len()))
                .map(|c| char::from(*c))
                .collect(),
            filled => {
                // Never seen: unique all the same, from the clock and a count.
                tracing::warn!(?filled, "no random bytes for a foreign handle");
                let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
                format!("{:016}{:08}{:08}", self.made, now.tv_sec, now.tv_nsec)
            }
        }
    }
}

impl GlobalDispatch<ZxdgExporterV2, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZxdgExporterV2>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZxdgExporterV2, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZxdgExporterV2,
        request: zxdg_exporter_v2::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zxdg_exporter_v2::Request::ExportToplevel { id, surface } = request {
            let handle = state.foreign.handle();
            let exported = data_init.init(id, handle.clone());
            if !surface.is_alive() || get_role(&surface) != Some(XDG_TOPLEVEL_ROLE) {
                resource.post_error(
                    zxdg_exporter_v2::Error::InvalidSurface,
                    "only an xdg_toplevel is exported",
                );
                return;
            }
            exported.handle(handle.clone());
            state
                .foreign
                .exported
                .insert(handle, Exported::Toplevel(surface));
        }
    }
}

impl Dispatch<ZxdgExportedV2, String> for State {
    fn request(
        _state: &mut Self,
        _client: &Client,
        _resource: &ZxdgExportedV2,
        _request: zxdg_exported_v2::Request,
        _handle: &String,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
    }

    /// Unexported: its children are no longer.
    fn destroyed(state: &mut Self, _client: ClientId, _resource: &ZxdgExportedV2, handle: &String) {
        state.foreign.exported.remove(handle);
        state.unparent(|c| c.handle == *handle);
    }
}

impl GlobalDispatch<ZxdgImporterV2, ()> for State {
    fn bind(
        _state: &mut Self,
        _handle: &DisplayHandle,
        _client: &Client,
        resource: New<ZxdgImporterV2>,
        _data: &(),
        data_init: &mut DataInit<'_, Self>,
    ) {
        data_init.init(resource, ());
    }
}

impl Dispatch<ZxdgImporterV2, ()> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        _resource: &ZxdgImporterV2,
        request: zxdg_importer_v2::Request,
        _data: &(),
        _dh: &DisplayHandle,
        data_init: &mut DataInit<'_, Self>,
    ) {
        if let zxdg_importer_v2::Request::ImportToplevel { id, handle } = request {
            let known = state.foreign.exported.contains_key(&handle);
            let imported = data_init.init(id, handle);
            if !known {
                imported.destroyed();
            }
        }
    }
}

impl Dispatch<ZxdgImportedV2, String> for State {
    fn request(
        state: &mut Self,
        _client: &Client,
        resource: &ZxdgImportedV2,
        request: zxdg_imported_v2::Request,
        handle: &String,
        _dh: &DisplayHandle,
        _data_init: &mut DataInit<'_, Self>,
    ) {
        if let zxdg_imported_v2::Request::SetParentOf { surface } = request {
            state.set_parent_of(resource, handle, surface);
        }
    }

    /// Its children are no longer.
    fn destroyed(state: &mut Self, _client: ClientId, resource: &ZxdgImportedV2, _handle: &String) {
        state.unparent(|c| c.imported == *resource);
    }
}
