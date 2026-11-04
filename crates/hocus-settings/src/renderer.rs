//! Which of iced's renderers draws the window, as the shell chooses its
//! own (tech-choices, "The bar's renderer (3.0)"; `hocus-shell`'s
//! `renderer.rs`): wgpu on the GPU the compositor draws with, preferring
//! the integrated one, or tiny-skia on the CPU when wgpu would only offer
//! a software adapter (llvmpipe) or none.
//!
//! iced reads its choice from its environment as it starts: the Settings
//! start themselves again once with it set.

use std::{os::unix::process::CommandExt, process::Command};

const BACKEND: &str = "ICED_BACKEND";
const POWER: &str = "WGPU_POWER_PREF";

/// Returns once the renderer is chosen; does not return when the program
/// starts again with its choice.
pub fn choose() {
    if std::env::var_os(BACKEND).is_some() {
        return;
    }
    let backend = match adapter() {
        Some(info) if info.device_type != wgpu::DeviceType::Cpu => {
            tracing::info!(adapter = info.name, "drawing on the GPU (wgpu)");
            "wgpu"
        }
        found => {
            tracing::info!(
                adapter = found.as_ref().map(|i| i.name.as_str()),
                "no GPU adapter: drawing on the CPU (tiny-skia)"
            );
            "tiny-skia"
        }
    };
    let err = std::env::current_exe()
        .map(|exe| {
            let mut command = Command::new(exe);
            command
                .args(std::env::args_os().skip(1))
                .env(BACKEND, backend);
            if std::env::var_os(POWER).is_none() {
                // A laptop's discrete GPU stays asleep.
                command.env(POWER, "low");
            }
            command.exec()
        })
        .unwrap_or_else(|err| err);
    tracing::warn!(%err, "could not start again with the renderer chosen");
}

/// The adapter wgpu offers iced, with the integrated GPU preferred.
fn adapter() -> Option<wgpu::AdapterInfo> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
        ..wgpu::InstanceDescriptor::default()
    });
    let options = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::from_env()
            .unwrap_or(wgpu::PowerPreference::LowPower),
        force_fallback_adapter: false,
        compatible_surface: None,
    };
    let runtime = tokio::runtime::Builder::new_current_thread().build().ok()?;
    runtime
        .block_on(instance.request_adapter(&options))
        .ok()
        .map(|a| a.get_info())
}
