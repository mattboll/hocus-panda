//! The system's fonts, for what the bundled ones lack (emoji, other
//! scripts): read once on a worker thread at start, then given to the
//! loop, whose trees lay their text out again. Tests and the virtual clock
//! never read them, so that reference images are the same everywhere.

use hocus_ui::text::SystemFonts;
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::App;

pub fn start(handle: &LoopHandle<'static, App>) {
    let (sender, fonts) = channel::channel::<SystemFonts>();
    let inserted = handle.insert_source(fonts, |event, _, app| {
        if let channel::Event::Msg(fonts) = event {
            app.timed("system fonts", |app| {
                fonts.install();
                tracing::info!("system fonts in use");
                app.state.needs_redraw = true;
            });
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "bundled fonts only");
        return;
    }
    let worker = std::thread::Builder::new()
        .name("fonts".into())
        .spawn(move || {
            let _ = sender.send(SystemFonts::load());
        });
    if let Err(err) = worker {
        tracing::warn!(%err, "bundled fonts only");
    }
}
