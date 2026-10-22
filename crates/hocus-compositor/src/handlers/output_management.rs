//! wlr-output-management and wlr-output-power-management, for third-party
//! screen tools (wlr-randr, kanshi, wdisplays, swayidle). Both are hidden
//! from sandboxed clients.
//!
//! Each manager hears of every screen, on or off, with its modes, then
//! `done` with a serial; a configuration made against an older serial is
//! cancelled. An applied configuration is a setting for the running
//! session: it is carried out after the turn of the loop (the backend does
//! it), then answered.

use std::sync::Mutex;

use hocus_core::outputs::{Mode, Transform};
use smithay::reexports::{
    wayland_protocols_wlr::{
        output_management::v1::server::{
            zwlr_output_configuration_head_v1::{self, ZwlrOutputConfigurationHeadV1},
            zwlr_output_configuration_v1::{self, ZwlrOutputConfigurationV1},
            zwlr_output_head_v1::{self, AdaptiveSyncState, ZwlrOutputHeadV1},
            zwlr_output_manager_v1::{self, ZwlrOutputManagerV1},
            zwlr_output_mode_v1::{self, ZwlrOutputModeV1},
        },
        output_power_management::v1::server::{
            zwlr_output_power_manager_v1::{self, ZwlrOutputPowerManagerV1},
            zwlr_output_power_v1::{self, ZwlrOutputPowerV1},
        },
    },
    wayland_server::{
        Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource, WEnum,
        backend::GlobalId, protocol::wl_output::Transform as WlTransform,
    },
};

use super::protocols::trusted;
use crate::{
    State,
    outputs::{Head, Setting},
};

const MANAGER_VERSION: u32 = 4;

#[derive(Debug)]
pub struct OutputManagement {
    _manager: GlobalId,
    _power: GlobalId,
    /// Bumped each time what the managers were told changes.
    serial: u32,
    /// What the backend last said about its screens.
    heads: Vec<Head>,
    managers: Vec<Told>,
    powers: Vec<ZwlrOutputPowerV1>,
    /// Configurations applied, answered once the loop settled screens.
    pub answering: Vec<ZwlrOutputConfigurationV1>,
    /// Screens dark through output-power (on in the layout): name, on.
    pub power_wanted: Vec<(String, bool)>,
    /// Screens kept dark through output-power.
    pub dark: Vec<String>,
    /// A setting failed while settling: the configurations answered next
    /// failed.
    pub failed: bool,
}

/// One manager and the head and mode objects it was given.
#[derive(Debug)]
struct Told {
    manager: ZwlrOutputManagerV1,
    heads: Vec<(Head, ZwlrOutputHeadV1, Vec<ZwlrOutputModeV1>)>,
}

/// A configuration being built.
#[derive(Debug, Default)]
pub struct Building {
    serial: u32,
    /// Each head named: the object its settings arrive on, or `None` to
    /// turn it off.
    heads: Vec<(String, Option<ZwlrOutputConfigurationHeadV1>)>,
    used: bool,
}

/// What a configuration sets for one head.
#[derive(Debug, Default)]
pub struct Pending {
    name: String,
    mode: Option<Mode>,
    /// Asked with set_custom_mode: a size, and a rate or any (0).
    custom: Option<(i32, i32, i32)>,
    position: Option<(i32, i32)>,
    transform: Option<Transform>,
    scale: Option<f64>,
    vrr: Option<bool>,
}

impl OutputManagement {
    pub fn new(dh: &DisplayHandle) -> Self {
        Self {
            _manager: dh.create_global::<State, ZwlrOutputManagerV1, _>(MANAGER_VERSION, ()),
            _power: dh.create_global::<State, ZwlrOutputPowerManagerV1, _>(1, ()),
            serial: 1,
            heads: Vec::new(),
            managers: Vec::new(),
            powers: Vec::new(),
            answering: Vec::new(),
            power_wanted: Vec::new(),
            dark: Vec::new(),
            failed: false,
        }
    }
}

fn wl_transform(t: Transform) -> WlTransform {
    match t {
        Transform::Normal => WlTransform::Normal,
        Transform::Rotate90 => WlTransform::_90,
        Transform::Rotate180 => WlTransform::_180,
        Transform::Rotate270 => WlTransform::_270,
        Transform::Flipped => WlTransform::Flipped,
        Transform::Flipped90 => WlTransform::Flipped90,
        Transform::Flipped180 => WlTransform::Flipped180,
        Transform::Flipped270 => WlTransform::Flipped270,
    }
}

fn from_wl_transform(t: WlTransform) -> Option<Transform> {
    Some(match t {
        WlTransform::Normal => Transform::Normal,
        WlTransform::_90 => Transform::Rotate90,
        WlTransform::_180 => Transform::Rotate180,
        WlTransform::_270 => Transform::Rotate270,
        WlTransform::Flipped => Transform::Flipped,
        WlTransform::Flipped90 => Transform::Flipped90,
        WlTransform::Flipped180 => Transform::Flipped180,
        WlTransform::Flipped270 => Transform::Flipped270,
        _ => return None,
    })
}

impl State {
    /// Tells every manager how the screens changed, and the power objects
    /// whether their screen is on.
    pub fn refresh_output_management(&mut self, heads: &[Head]) {
        let om = &mut self.output_management;
        if om.heads.as_slice() == heads {
            return;
        }
        om.heads = heads.to_vec();
        om.serial = om.serial.wrapping_add(1);
        let dh = self.display_handle.clone();
        for told in &mut om.managers {
            tell(&dh, told, heads, om.serial);
        }
        // A screen turned off or gone: its power objects fail.
        om.powers.retain(|p| p.is_alive());
        for power in &om.powers {
            if let Some(name) = power.data::<String>()
                && !heads.iter().any(|h| &h.name == name && h.now.enabled)
            {
                power.failed();
            }
        }
    }

    /// Tells the power objects of `name` whether its light is on.
    pub fn power_changed(&mut self, name: &str) {
        let om = &mut self.output_management;
        let on = !om.dark.iter().any(|n| n == name);
        om.powers.retain(|p| p.is_alive());
        for power in &om.powers {
            if power.data::<String>().is_some_and(|n| n == name) {
                send_power(power, on, true);
            }
        }
    }

    /// Answers the configurations applied this turn.
    pub fn answer_output_configurations(&mut self) {
        let om = &mut self.output_management;
        let failed = std::mem::take(&mut om.failed);
        for config in om.answering.drain(..) {
            if config.is_alive() {
                if failed {
                    config.failed();
                } else {
                    config.succeeded();
                }
            }
        }
    }
}

fn send_power(power: &ZwlrOutputPowerV1, on: bool, exists: bool) {
    if !exists {
        power.failed();
        return;
    }
    power.mode(if on {
        zwlr_output_power_v1::Mode::On
    } else {
        zwlr_output_power_v1::Mode::Off
    });
}

/// Sends a manager what changed since it was last told, then `done`.
fn tell(dh: &DisplayHandle, told: &mut Told, heads: &[Head], serial: u32) {
    let Some(client) = told.manager.client() else {
        return;
    };
    // Gone.
    told.heads.retain(|(head, handle, modes)| {
        let here = heads.iter().any(|h| h.name == head.name);
        if !here {
            for mode in modes {
                mode.finished();
            }
            handle.finished();
        }
        here
    });
    for head in heads {
        let known = told.heads.iter().position(|(h, ..)| h.name == head.name);
        match known {
            Some(i) => {
                let (last, handle, modes) = &mut told.heads[i];
                if last.modes != head.modes {
                    // New modes: the old ones go, the new ones come.
                    for mode in modes.drain(..) {
                        mode.finished();
                    }
                    *modes = send_modes(dh, &client, handle, head);
                    send_state(handle, modes, head, None);
                } else {
                    send_state(handle, modes, head, Some(last));
                }
                *last = head.clone();
            }
            None => {
                let Ok(handle) = client.create_resource::<ZwlrOutputHeadV1, _, State>(
                    dh,
                    told.manager.version(),
                    head.name.clone(),
                ) else {
                    continue;
                };
                told.manager.head(&handle);
                handle.name(head.name.clone());
                handle.description(format!("{} {} ({})", head.make, head.model, head.name));
                if head.physical_size != (0, 0) {
                    handle.physical_size(head.physical_size.0, head.physical_size.1);
                }
                if handle.version() >= 2 {
                    handle.make(head.make.clone());
                    handle.model(head.model.clone());
                }
                let modes = send_modes(dh, &client, &handle, head);
                send_state(&handle, &modes, head, None);
                told.heads.push((head.clone(), handle, modes));
            }
        }
    }
    told.manager.done(serial);
}

fn send_modes(
    dh: &DisplayHandle,
    client: &Client,
    head: &ZwlrOutputHeadV1,
    info: &Head,
) -> Vec<ZwlrOutputModeV1> {
    info.modes
        .iter()
        .enumerate()
        .filter_map(|(i, mode)| {
            let handle = client
                .create_resource::<ZwlrOutputModeV1, _, State>(
                    dh,
                    head.version().min(3),
                    (info.name.clone(), *mode),
                )
                .ok()?;
            head.mode(&handle);
            handle.size(mode.width, mode.height);
            if mode.refresh > 0 {
                handle.refresh(mode.refresh);
            }
            if info.preferred == Some(i) {
                handle.preferred();
            }
            Some(handle)
        })
        .collect()
}

/// Sends what is set, all of it or what changed since `last`.
fn send_state(
    handle: &ZwlrOutputHeadV1,
    modes: &[ZwlrOutputModeV1],
    head: &Head,
    last: Option<&Head>,
) {
    let now = &head.now;
    let was = last.map(|l| l.now);
    let changed = |f: fn(&Setting) -> bool| was.is_none_or(|w| f(&w) != f(now));
    if was.is_none_or(|w| w.enabled != now.enabled) {
        handle.enabled(i32::from(now.enabled));
    }
    if !now.enabled {
        return;
    }
    let fresh = was.is_none_or(|w| !w.enabled);
    if (fresh || was.is_some_and(|w| w.mode != now.mode))
        && let Some(i) = head.modes.iter().position(|m| *m == now.mode)
    {
        handle.current_mode(&modes[i]);
    }
    if fresh || was.is_some_and(|w| w.position != now.position) {
        handle.position(now.position.0, now.position.1);
    }
    if fresh || was.is_some_and(|w| w.transform != now.transform) {
        handle.transform(wl_transform(now.transform));
    }
    if fresh || was.is_some_and(|w| w.scale != now.scale) {
        handle.scale(now.scale);
    }
    if handle.version() >= 4 && (fresh || changed(|s| s.vrr)) {
        handle.adaptive_sync(if now.vrr {
            AdaptiveSyncState::Enabled
        } else {
            AdaptiveSyncState::Disabled
        });
    }
}

impl GlobalDispatch<ZwlrOutputManagerV1, ()> for State {
    fn bind(
        state: &mut Self,
        dh: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrOutputManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        let manager = data.init(resource, ());
        let mut told = Told {
            manager,
            heads: Vec::new(),
        };
        let om = &mut state.output_management;
        tell(dh, &mut told, &om.heads.clone(), om.serial);
        om.managers.push(told);
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ZwlrOutputManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        manager: &ZwlrOutputManagerV1,
        request: zwlr_output_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        match request {
            zwlr_output_manager_v1::Request::CreateConfiguration { id, serial } => {
                data.init(
                    id,
                    Mutex::new(Building {
                        serial,
                        ..Default::default()
                    }),
                );
            }
            zwlr_output_manager_v1::Request::Stop => {
                state
                    .output_management
                    .managers
                    .retain(|t| t.manager != *manager);
                manager.finished();
            }
            _ => {}
        }
    }

    fn destroyed(
        state: &mut Self,
        _: smithay::reexports::wayland_server::backend::ClientId,
        manager: &ZwlrOutputManagerV1,
        _: &(),
    ) {
        state
            .output_management
            .managers
            .retain(|t| t.manager != *manager);
    }
}

impl Dispatch<ZwlrOutputHeadV1, String> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwlrOutputHeadV1,
        _: zwlr_output_head_v1::Request,
        _: &String,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // Release only.
    }
}

impl Dispatch<ZwlrOutputModeV1, (String, Mode)> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        _: &ZwlrOutputModeV1,
        _: zwlr_output_mode_v1::Request,
        _: &(String, Mode),
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        // Release only.
    }
}

impl Dispatch<ZwlrOutputConfigurationV1, Mutex<Building>> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        config: &ZwlrOutputConfigurationV1,
        request: zwlr_output_configuration_v1::Request,
        building: &Mutex<Building>,
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        let Ok(mut b) = building.lock() else {
            return;
        };
        let already = |b: &Building, head: &ZwlrOutputHeadV1| {
            head.data::<String>()
                .is_some_and(|n| b.heads.iter().any(|(m, _)| m == n))
        };
        match request {
            zwlr_output_configuration_v1::Request::EnableHead { id, head } => {
                let name = head.data::<String>().cloned().unwrap_or_default();
                if already(&b, &head) {
                    config.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyConfiguredHead,
                        "head configured twice",
                    );
                    return;
                }
                let settings = data.init(
                    id,
                    Mutex::new(Pending {
                        name: name.clone(),
                        ..Default::default()
                    }),
                );
                b.heads.push((name, Some(settings)));
            }
            zwlr_output_configuration_v1::Request::DisableHead { head } => {
                if already(&b, &head) {
                    config.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyConfiguredHead,
                        "head configured twice",
                    );
                    return;
                }
                let name = head.data::<String>().cloned().unwrap_or_default();
                b.heads.push((name, None));
            }
            request @ (zwlr_output_configuration_v1::Request::Apply
            | zwlr_output_configuration_v1::Request::Test) => {
                if b.used {
                    config.post_error(
                        zwlr_output_configuration_v1::Error::AlreadyUsed,
                        "configuration used twice",
                    );
                    return;
                }
                b.used = true;
                let apply = matches!(request, zwlr_output_configuration_v1::Request::Apply);
                if b.serial != state.output_management.serial {
                    config.cancelled();
                    return;
                }
                let settings = match settings_of(state, &b) {
                    Ok(settings) => settings,
                    Err(err) => {
                        tracing::info!(%err, "screen configuration refused");
                        config.failed();
                        return;
                    }
                };
                if !apply {
                    config.succeeded();
                    return;
                }
                for (name, setting) in settings {
                    state.override_output(&name, setting);
                }
                state.output_management.answering.push(config.clone());
            }
            _ => {}
        }
    }
}

/// The settings a configuration asks for, over what each head is set to
/// now; an error for what no screen can do.
fn settings_of(state: &State, b: &Building) -> Result<Vec<(String, Setting)>, String> {
    let heads = &state.output_management.heads;
    b.heads
        .iter()
        .map(|(name, settings)| {
            let head = heads
                .iter()
                .find(|h| &h.name == name)
                .ok_or_else(|| format!("{name} is gone"))?;
            let mut setting = head.now;
            let Some(settings) = settings else {
                setting.enabled = false;
                return Ok((name.clone(), setting));
            };
            setting.enabled = true;
            let pending = settings
                .data::<Mutex<Pending>>()
                .and_then(|p| p.lock().ok());
            if let Some(p) = pending {
                if let Some(mode) = p.mode {
                    setting.mode = mode;
                }
                if let Some((w, h, refresh)) = p.custom {
                    setting.mode = *head
                        .modes
                        .iter()
                        .filter(|m| m.width == w && m.height == h)
                        .min_by_key(|m| {
                            if refresh > 0 {
                                (m.refresh - refresh).abs()
                            } else {
                                -m.refresh
                            }
                        })
                        .ok_or_else(|| format!("{name} has no {w}x{h} mode"))?;
                }
                if let Some(at) = p.position {
                    setting.position = at;
                }
                if let Some(t) = p.transform {
                    setting.transform = t;
                }
                if let Some(scale) = p.scale {
                    if !(0.25..=8.0).contains(&scale) {
                        return Err(format!("scale {scale} for {name}"));
                    }
                    setting.scale = hocus_core::outputs::round_scale(scale);
                }
                if let Some(vrr) = p.vrr {
                    if vrr && !head.vrr_capable {
                        return Err(format!("{name} has no adaptive sync"));
                    }
                    setting.vrr = vrr;
                }
            }
            Ok((name.clone(), setting))
        })
        .collect()
}

impl Dispatch<ZwlrOutputConfigurationHeadV1, Mutex<Pending>> for State {
    fn request(
        _: &mut Self,
        _: &Client,
        resource: &ZwlrOutputConfigurationHeadV1,
        request: zwlr_output_configuration_head_v1::Request,
        pending: &Mutex<Pending>,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        use zwlr_output_configuration_head_v1::{Error, Request};
        let Ok(mut p) = pending.lock() else {
            return;
        };
        let twice = |set: bool| {
            if set {
                resource.post_error(Error::AlreadySet, "property set twice");
            }
            set
        };
        match request {
            Request::SetMode { mode } => {
                if twice(p.mode.is_some() || p.custom.is_some()) {
                    return;
                }
                match mode.data::<(String, Mode)>() {
                    Some((head, m)) if *head == p.name => p.mode = Some(*m),
                    _ => resource.post_error(Error::InvalidMode, "mode of another head"),
                }
            }
            Request::SetCustomMode {
                width,
                height,
                refresh,
            } => {
                if twice(p.mode.is_some() || p.custom.is_some()) {
                    return;
                }
                if width <= 0 || height <= 0 || refresh < 0 {
                    resource.post_error(Error::InvalidCustomMode, "mode size or rate invalid");
                    return;
                }
                p.custom = Some((width, height, refresh));
            }
            Request::SetPosition { x, y } => {
                if !twice(p.position.is_some()) {
                    p.position = Some((x, y));
                }
            }
            Request::SetTransform { transform } => {
                if twice(p.transform.is_some()) {
                    return;
                }
                match transform {
                    WEnum::Value(t) => p.transform = from_wl_transform(t),
                    WEnum::Unknown(_) => {
                        resource.post_error(Error::InvalidTransform, "unknown transform");
                    }
                }
            }
            Request::SetScale { scale } => {
                if twice(p.scale.is_some()) {
                    return;
                }
                if scale <= 0.0 {
                    resource.post_error(Error::InvalidScale, "scale not positive");
                    return;
                }
                p.scale = Some(scale);
            }
            Request::SetAdaptiveSync { state: sync } => {
                if twice(p.vrr.is_some()) {
                    return;
                }
                match sync {
                    WEnum::Value(AdaptiveSyncState::Enabled) => p.vrr = Some(true),
                    WEnum::Value(AdaptiveSyncState::Disabled) => p.vrr = Some(false),
                    _ => resource.post_error(
                        Error::InvalidAdaptiveSyncState,
                        "unknown adaptive sync state",
                    ),
                }
            }
            _ => {}
        }
    }
}

impl GlobalDispatch<ZwlrOutputPowerManagerV1, ()> for State {
    fn bind(
        _: &mut Self,
        _: &DisplayHandle,
        _: &Client,
        resource: New<ZwlrOutputPowerManagerV1>,
        _: &(),
        data: &mut DataInit<'_, Self>,
    ) {
        data.init(resource, ());
    }

    fn can_view(client: Client, _: &()) -> bool {
        trusted(&client)
    }
}

impl Dispatch<ZwlrOutputPowerManagerV1, ()> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwlrOutputPowerManagerV1,
        request: zwlr_output_power_manager_v1::Request,
        _: &(),
        _: &DisplayHandle,
        data: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_output_power_manager_v1::Request::GetOutputPower { id, output } = request {
            let name = smithay::output::Output::from_resource(&output)
                .map(|o| o.name())
                .unwrap_or_default();
            let power = data.init(id, name.clone());
            let om = &state.output_management;
            let exists = om.heads.iter().any(|h| h.name == name && h.now.enabled);
            send_power(&power, !om.dark.contains(&name), exists);
            state.output_management.powers.push(power);
        }
    }
}

impl Dispatch<ZwlrOutputPowerV1, String> for State {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &ZwlrOutputPowerV1,
        request: zwlr_output_power_v1::Request,
        name: &String,
        _: &DisplayHandle,
        _: &mut DataInit<'_, Self>,
    ) {
        if let zwlr_output_power_v1::Request::SetMode {
            mode: WEnum::Value(mode),
        } = request
        {
            let on = mode == zwlr_output_power_v1::Mode::On;
            state
                .output_management
                .power_wanted
                .push((name.clone(), on));
            state.output_settings.changed = true;
        }
    }
}
