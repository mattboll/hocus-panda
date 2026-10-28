//! Which of iced's renderers draws the bar (tech-choices, "The bar's
//! renderer (3.0)"): wgpu on the compositor's GPU, or tiny-skia on the CPU.
//!
//! iced only falls back to tiny-skia when wgpu fails, and with a software
//! Vulkan driver installed wgpu never fails: the bar would then draw with
//! llvmpipe at a core or two per animation. So the shell asks wgpu which
//! adapter it would get, with the same preference iced has
//! (`WGPU_POWER_PREF`, which the compositor sets to `low`); when that is a
//! CPU, or there is none, it starts itself again with
//! `ICED_BACKEND=tiny-skia`. The compositor also starts it that way when it
//! draws without a GPU itself.

use std::{os::unix::process::CommandExt, process::Command};

/// iced's variable choosing its renderer.
const BACKEND: &str = "ICED_BACKEND";
const TINY_SKIA: &str = "tiny-skia";

/// Whether the bar draws on the CPU. Does not return when the shell starts
/// again with the CPU renderer.
pub fn choose() -> bool {
    if std::env::var(BACKEND).is_ok_and(|b| b == TINY_SKIA) {
        tracing::info!("drawing on the CPU (tiny-skia): bars are opaque");
        return true;
    }
    if std::env::var_os(BACKEND).is_some() {
        // Chosen by hand.
        return false;
    }
    match adapter() {
        Some(info) if info.device_type != wgpu::DeviceType::Cpu => {
            tracing::info!(
                adapter = info.name,
                backend = ?info.backend,
                "drawing on the GPU (wgpu)"
            );
            false
        }
        found => {
            tracing::info!(
                adapter = found.as_ref().map(|i| i.name.as_str()),
                "no GPU adapter: starting again on the CPU"
            );
            let err = std::env::current_exe()
                .map(|exe| {
                    Command::new(exe)
                        .args(std::env::args_os().skip(1))
                        .env(BACKEND, TINY_SKIA)
                        .exec()
                })
                .unwrap_or_else(|err| err);
            tracing::warn!(%err, "could not start again; staying on wgpu");
            false
        }
    }
}

/// The adapter wgpu offers iced, as iced asks for it.
fn adapter() -> Option<wgpu::AdapterInfo> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all()),
        ..wgpu::InstanceDescriptor::default()
    });
    let options = wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::from_env()
            .unwrap_or(wgpu::PowerPreference::HighPerformance),
        force_fallback_adapter: false,
        compatible_surface: None,
    };
    iced::futures::executor::block_on(instance.request_adapter(&options))
        .ok()
        .map(|a| a.get_info())
}
