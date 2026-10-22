//! The headless compositor never reads the system's fonts: reference
//! images are the same on every machine.

use hocus_compositor::{HarnessOptions, Headless, HeadlessConfig, RendererChoice};

#[test]
fn the_headless_compositor_keeps_the_bundled_fonts() -> anyhow::Result<()> {
    let mut compositor = Headless::start_options(HarnessOptions {
        headless: HeadlessConfig {
            renderer: RendererChoice::Pixman,
            ..Default::default()
        },
        api_permission: hocus_api::Permission::Test,
        watchdog: None,
        xwayland: false,
        shell: true,
        session_bus: None,
        system_bus: None,
        data_dirs: None,
        child_env: None,
    })?;
    compositor.advance(hocus_compositor::FRAME * 10)?;
    assert!(!hocus_ui::text::system_fonts_installed());
    Ok(())
}
