//! A wlr-output-management client, as wlr-randr or kanshi: lists the
//! screens, and sets one of them for the running session.

use wayland_client::{
    Connection, Dispatch, Proxy, QueueHandle, event_created_child,
    globals::{GlobalListContents, registry_queue_init},
    protocol::wl_registry,
};
use wayland_protocols_wlr::output_management::v1::client::{
    zwlr_output_configuration_head_v1::ZwlrOutputConfigurationHeadV1,
    zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
    zwlr_output_head_v1::{self, ZwlrOutputHeadV1},
    zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
    zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
};

/// A screen, as the compositor described it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Head {
    pub name: String,
    pub enabled: bool,
    /// Width, height, mHz.
    pub modes: Vec<(i32, i32, i32)>,
    pub current_mode: Option<usize>,
    pub position: (i32, i32),
    pub scale: f64,
}

/// What to change on one screen; the others are kept as they are.
#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub output: String,
    pub enabled: Option<bool>,
    /// `WIDTHxHEIGHT`, at its highest rate.
    pub mode: Option<String>,
    pub scale: Option<f64>,
    pub position: Option<[i32; 2]>,
}

/// How the compositor answered a configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Debug, Default)]
struct Client {
    heads: Vec<(ZwlrOutputHeadV1, Head, Vec<ZwlrOutputModeV1>)>,
    serial: Option<u32>,
    outcome: Option<Outcome>,
}

/// The screens, once the compositor said `done`.
pub fn heads(socket: &std::path::Path) -> anyhow::Result<Vec<Head>> {
    let (_, client, _, _) = connect(socket)?;
    Ok(client.heads.into_iter().map(|(_, h, _)| h).collect())
}

/// Applies `change` (or only tests it), against the serial just received,
/// or against `stale` to see an outdated configuration cancelled.
pub fn apply(
    socket: &std::path::Path,
    change: &Change,
    test_only: bool,
    stale: bool,
) -> anyhow::Result<Outcome> {
    let (manager, mut client, mut queue, qh) = connect(socket)?;
    let serial = client.serial.unwrap_or(0);
    let config = manager.create_configuration(
        if stale {
            serial.wrapping_sub(1)
        } else {
            serial
        },
        &qh,
        (),
    );
    for (head, info, modes) in &client.heads {
        if info.name != change.output {
            // Every head is named: the others as they are.
            if info.enabled {
                let _ = config.enable_head(head, &qh, ());
            } else {
                config.disable_head(head);
            }
            continue;
        }
        if change.enabled == Some(false) {
            config.disable_head(head);
            continue;
        }
        let settings = config.enable_head(head, &qh, ());
        if let Some(mode) = &change.mode {
            let (w, h) = mode
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse::<i32>().ok()?, h.parse::<i32>().ok()?)))
                .ok_or_else(|| anyhow::anyhow!("mode {mode:?}: expected WIDTHxHEIGHT"))?;
            let found = info
                .modes
                .iter()
                .enumerate()
                .filter(|(_, m)| m.0 == w && m.1 == h)
                .max_by_key(|(_, m)| m.2)
                .map(|(i, _)| i)
                .ok_or_else(|| anyhow::anyhow!("{} has no {mode} mode", info.name))?;
            settings.set_mode(&modes[found]);
        }
        if let Some(scale) = change.scale {
            settings.set_scale(scale);
        }
        if let Some([x, y]) = change.position {
            settings.set_position(x, y);
        }
    }
    if test_only {
        config.test();
    } else {
        config.apply();
    }
    while client.outcome.is_none() {
        queue.blocking_dispatch(&mut client)?;
    }
    config.destroy();
    let _ = queue.roundtrip(&mut client);
    client.outcome.ok_or_else(|| anyhow::anyhow!("no answer"))
}

type Connected = (
    ZwlrOutputManagerV1,
    Client,
    wayland_client::EventQueue<Client>,
    QueueHandle<Client>,
);

fn connect(socket: &std::path::Path) -> anyhow::Result<Connected> {
    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Client>(&conn)?;
    let qh = queue.handle();
    let manager: ZwlrOutputManagerV1 = globals.bind(&qh, 1..=4, ())?;
    let mut client = Client::default();
    while client.serial.is_none() {
        queue.blocking_dispatch(&mut client)?;
    }
    Ok((manager, client, queue, qh))
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Client {
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

impl Dispatch<ZwlrOutputManagerV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ZwlrOutputManagerV1,
        event: zwlr_output_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_output_manager_v1::Event::Head { head } => {
                client.heads.push((head, Head::default(), Vec::new()));
            }
            zwlr_output_manager_v1::Event::Done { serial } => client.serial = Some(serial),
            _ => {}
        }
    }

    event_created_child!(Client, ZwlrOutputManagerV1, [
        zwlr_output_manager_v1::EVT_HEAD_OPCODE => (ZwlrOutputHeadV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputHeadV1, ()> for Client {
    fn event(
        client: &mut Self,
        head: &ZwlrOutputHeadV1,
        event: zwlr_output_head_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let Some((_, info, modes)) = client.heads.iter_mut().find(|(h, ..)| h == head) else {
            return;
        };
        match event {
            zwlr_output_head_v1::Event::Name { name } => info.name = name,
            zwlr_output_head_v1::Event::Enabled { enabled } => info.enabled = enabled != 0,
            zwlr_output_head_v1::Event::Mode { mode } => {
                modes.push(mode);
                info.modes.push((0, 0, 0));
            }
            zwlr_output_head_v1::Event::CurrentMode { mode } => {
                info.current_mode = modes.iter().position(|m| *m == mode);
            }
            zwlr_output_head_v1::Event::Position { x, y } => info.position = (x, y),
            zwlr_output_head_v1::Event::Scale { scale } => info.scale = scale,
            _ => {}
        }
    }

    event_created_child!(Client, ZwlrOutputHeadV1, [
        zwlr_output_head_v1::EVT_MODE_OPCODE => (ZwlrOutputModeV1, ()),
    ]);
}

impl Dispatch<ZwlrOutputModeV1, ()> for Client {
    fn event(
        client: &mut Self,
        mode: &ZwlrOutputModeV1,
        event: zwlr_output_mode_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        for (_, info, modes) in &mut client.heads {
            if let Some(i) = modes.iter().position(|m| m == mode) {
                match event {
                    zwlr_output_mode_v1::Event::Size { width, height } => {
                        info.modes[i].0 = width;
                        info.modes[i].1 = height;
                    }
                    zwlr_output_mode_v1::Event::Refresh { refresh } => info.modes[i].2 = refresh,
                    _ => {}
                }
            }
        }
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, ()> for Client {
    fn event(
        client: &mut Self,
        _: &ZwlrOutputConfigurationV1,
        event: zwlr_output_configuration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        client.outcome = Some(match event {
            zwlr_output_configuration_v1::Event::Succeeded => Outcome::Succeeded,
            zwlr_output_configuration_v1::Event::Failed => Outcome::Failed,
            _ => Outcome::Cancelled,
        });
    }
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, ()> for Client {
    fn event(
        _: &mut Self,
        _: &ZwlrOutputConfigurationHeadV1,
        _: <ZwlrOutputConfigurationHeadV1 as Proxy>::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

/// Turns the `index`th output off or on through wlr-output-power-management,
/// and says what the compositor reported back (`Some(on)`, or `None` when it
/// failed).
pub fn power(socket: &std::path::Path, index: usize, on: bool) -> anyhow::Result<Option<bool>> {
    use wayland_client::protocol::wl_output;
    use wayland_protocols_wlr::output_power_management::v1::client::{
        zwlr_output_power_manager_v1::ZwlrOutputPowerManagerV1,
        zwlr_output_power_v1::{self, ZwlrOutputPowerV1},
    };

    #[derive(Default)]
    struct Power {
        modes: Vec<Option<bool>>,
    }
    impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Power {
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
    impl Dispatch<wl_output::WlOutput, ()> for Power {
        fn event(
            _: &mut Self,
            _: &wl_output::WlOutput,
            _: wl_output::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<ZwlrOutputPowerManagerV1, ()> for Power {
        fn event(
            _: &mut Self,
            _: &ZwlrOutputPowerManagerV1,
            _: <ZwlrOutputPowerManagerV1 as Proxy>::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
        }
    }
    impl Dispatch<ZwlrOutputPowerV1, ()> for Power {
        fn event(
            power: &mut Self,
            _: &ZwlrOutputPowerV1,
            event: zwlr_output_power_v1::Event,
            _: &(),
            _: &Connection,
            _: &QueueHandle<Self>,
        ) {
            power.modes.push(match event {
                zwlr_output_power_v1::Event::Mode {
                    mode: wayland_client::WEnum::Value(zwlr_output_power_v1::Mode::On),
                } => Some(true),
                zwlr_output_power_v1::Event::Mode { .. } => Some(false),
                _ => None,
            });
        }
    }

    let conn = Connection::from_socket(std::os::unix::net::UnixStream::connect(socket)?)?;
    let (globals, mut queue) = registry_queue_init::<Power>(&conn)?;
    let qh = queue.handle();
    let manager: ZwlrOutputPowerManagerV1 = globals.bind(&qh, 1..=1, ())?;
    let outputs: Vec<wl_output::WlOutput> = globals
        .contents()
        .clone_list()
        .into_iter()
        .filter(|g| g.interface == "wl_output")
        .map(|g| globals.registry().bind(g.name, 1, &qh, ()))
        .collect();
    let output = outputs
        .get(index)
        .ok_or_else(|| anyhow::anyhow!("no output {index}"))?;
    let handle = manager.get_output_power(output, &qh, ());
    let mut power = Power::default();
    // What it is now, then what it becomes.
    while power.modes.is_empty() {
        queue.blocking_dispatch(&mut power)?;
    }
    handle.set_mode(if on {
        zwlr_output_power_v1::Mode::On
    } else {
        zwlr_output_power_v1::Mode::Off
    });
    while power.modes.len() < 2 {
        queue.blocking_dispatch(&mut power)?;
    }
    Ok(power.modes[1])
}
