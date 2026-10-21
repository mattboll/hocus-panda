//! What bars and switchers see over Wayland: ext-workspace (one group per
//! output, desktops as workspaces) and ext-foreign-toplevel-list (#16).

use std::{os::unix::net::UnixStream, path::PathBuf, thread, time::Duration};

use hocus_api::{Reply, Request};
use hocus_compositor::{Headless, HeadlessConfig, OutputSpec, RendererChoice};
use hocus_test::client::SolidWindow;
use wayland_client::{
    Connection, Dispatch, EventQueue, QueueHandle,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols::ext::{
    foreign_toplevel_list::v1::client::{
        ext_foreign_toplevel_handle_v1::{self, ExtForeignToplevelHandleV1},
        ext_foreign_toplevel_list_v1::{self, ExtForeignToplevelListV1},
    },
    workspace::v1::client::{
        ext_workspace_group_handle_v1::{self, ExtWorkspaceGroupHandleV1},
        ext_workspace_handle_v1::{self, ExtWorkspaceHandleV1},
        ext_workspace_manager_v1::{self, ExtWorkspaceManagerV1},
    },
};

const TIMEOUT: Duration = Duration::from_secs(10);

/// What a bar would know.
#[derive(Debug, Default)]
struct Bar {
    groups: Vec<ExtWorkspaceGroupHandleV1>,
    /// Workspace, its name, whether active, its group (index in `groups`).
    workspaces: Vec<(ExtWorkspaceHandleV1, String, bool, Option<usize>)>,
    toplevels: Vec<String>,
    done: u32,
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Bar {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ExtWorkspaceManagerV1, ()> for Bar {
    fn event(
        bar: &mut Self,
        _: &ExtWorkspaceManagerV1,
        event: ext_workspace_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            ext_workspace_manager_v1::Event::WorkspaceGroup { workspace_group } => {
                bar.groups.push(workspace_group)
            }
            ext_workspace_manager_v1::Event::Workspace { workspace } => {
                bar.workspaces.push((workspace, String::new(), false, None))
            }
            ext_workspace_manager_v1::Event::Done => bar.done += 1,
            _ => {}
        }
    }

    wayland_client::event_created_child!(Bar, ExtWorkspaceManagerV1, [
        ext_workspace_manager_v1::EVT_WORKSPACE_GROUP_OPCODE => (ExtWorkspaceGroupHandleV1, ()),
        ext_workspace_manager_v1::EVT_WORKSPACE_OPCODE => (ExtWorkspaceHandleV1, ()),
    ]);
}

impl Dispatch<ExtWorkspaceGroupHandleV1, ()> for Bar {
    fn event(
        bar: &mut Self,
        group: &ExtWorkspaceGroupHandleV1,
        event: ext_workspace_group_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_workspace_group_handle_v1::Event::WorkspaceEnter { workspace } = event {
            let index = bar.groups.iter().position(|g| g == group);
            if let Some(w) = bar.workspaces.iter_mut().find(|w| w.0 == workspace) {
                w.3 = index;
            }
        }
    }
}

impl Dispatch<ExtWorkspaceHandleV1, ()> for Bar {
    fn event(
        bar: &mut Self,
        handle: &ExtWorkspaceHandleV1,
        event: ext_workspace_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some(w) = bar.workspaces.iter_mut().find(|w| w.0 == *handle) else {
            return;
        };
        match event {
            ext_workspace_handle_v1::Event::Name { name } => w.1 = name,
            ext_workspace_handle_v1::Event::State { state } => {
                w.2 = state
                    .into_result()
                    .is_ok_and(|s| s.contains(ext_workspace_handle_v1::State::Active))
            }
            _ => {}
        }
    }
}

impl Dispatch<ExtForeignToplevelListV1, ()> for Bar {
    fn event(
        _: &mut Self,
        _: &ExtForeignToplevelListV1,
        _: ext_foreign_toplevel_list_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }

    wayland_client::event_created_child!(Bar, ExtForeignToplevelListV1, [
        ext_foreign_toplevel_list_v1::EVT_TOPLEVEL_OPCODE => (ExtForeignToplevelHandleV1, ()),
    ]);
}

impl Dispatch<ExtForeignToplevelHandleV1, ()> for Bar {
    fn event(
        bar: &mut Self,
        _: &ExtForeignToplevelHandleV1,
        event: ext_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let ext_foreign_toplevel_handle_v1::Event::AppId { app_id } = event {
            bar.toplevels.push(app_id);
        }
    }
}

/// Runs `talk` as a Wayland client in a thread while the compositor serves.
fn as_client<T: Send + 'static>(
    compositor: &mut Headless,
    talk: impl FnOnce(
        &mut EventQueue<Bar>,
        &mut Bar,
        &wayland_client::globals::GlobalList,
    ) -> anyhow::Result<T>
    + Send
    + 'static,
) -> anyhow::Result<T> {
    let socket: PathBuf = compositor.socket_path().to_path_buf();
    let thread = thread::spawn(move || -> anyhow::Result<T> {
        let conn = Connection::from_socket(UnixStream::connect(socket)?)?;
        let (globals, mut queue) = registry_queue_init::<Bar>(&conn)?;
        let mut bar = Bar::default();
        talk(&mut queue, &mut bar, &globals)
    });
    compositor.dispatch_until(TIMEOUT, |_| thread.is_finished())?;
    thread
        .join()
        .map_err(|_| anyhow::anyhow!("the client thread panicked"))?
}

fn compositor() -> anyhow::Result<Headless> {
    Headless::start(HeadlessConfig {
        outputs: vec![
            OutputSpec::new(1280, 800).desktops(1),
            OutputSpec::new(1920, 1080),
        ],
        renderer: RendererChoice::Pixman,
    })
}

#[test]
fn bars_see_one_workspace_group_per_output_and_can_activate_a_desktop() -> anyhow::Result<()> {
    let mut compositor = compositor()?;

    let (groups, workspaces) = as_client(&mut compositor, |queue, bar, globals| {
        let qh = queue.handle();
        let manager: ExtWorkspaceManagerV1 = globals.bind(&qh, 1..=1, ())?;
        queue.roundtrip(bar)?;
        let summary: Vec<(String, bool, Option<usize>)> = bar
            .workspaces
            .iter()
            .map(|w| (w.1.clone(), w.2, w.3))
            .collect();
        // Activate the external output's third desktop.
        if let Some(third) = bar.workspaces.iter().filter(|w| w.3 == Some(1)).nth(2) {
            third.0.activate();
        }
        manager.commit();
        queue.roundtrip(bar)?;
        Ok((bar.groups.len(), summary))
    })?;

    assert_eq!(groups, 2);
    assert_eq!(
        workspaces,
        [
            ("1".into(), true, Some(0)),
            ("1".into(), true, Some(1)),
            ("2".into(), false, Some(1)),
            ("3".into(), false, Some(1)),
            ("4".into(), false, Some(1)),
        ]
    );
    let Reply::Desktops(outputs) = compositor.request(Request::Desktops)? else {
        anyhow::bail!("expected desktops");
    };
    let external = &outputs[1];
    assert_eq!(external.shown, external.desktops[2].id);
    Ok(())
}

#[test]
fn taskbars_see_windows_with_their_app_id() -> anyhow::Result<()> {
    let mut compositor = compositor()?;
    let window = SolidWindow {
        app_id: "listed".into(),
        width: 100,
        height: 100,
        argb: 0xffff_ffff,
    };
    let _client = window.spawn(compositor.socket_path())?;
    compositor.dispatch_until(TIMEOUT, |c| c.window_count() == 1)?;

    let toplevels = as_client(&mut compositor, |queue, bar, globals| {
        let qh = queue.handle();
        let _list: ExtForeignToplevelListV1 = globals.bind(&qh, 1..=1, ())?;
        queue.roundtrip(bar)?;
        Ok(bar.toplevels.clone())
    })?;

    assert_eq!(toplevels, ["listed"]);
    Ok(())
}
