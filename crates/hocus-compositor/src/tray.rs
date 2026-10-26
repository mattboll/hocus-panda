//! The system tray (#73): the watcher and host run on a worker thread
//! (`hocus_platform::tray`); the column shows its applications behind a
//! chip of the status row, and their menus.

use hocus_platform::tray::{MenuEntry, Tray, TrayCommand, TrayEvent};
use hocus_shell::column::{TrayApp, TrayMenuEntry};
use smithay::reexports::calloop::{LoopHandle, channel};

use crate::{App, State};

/// Icons are drawn 24 pixels tall, up to twice for HiDPI screens.
const ICON_PIXELS: u32 = 48;

#[derive(Debug, Default)]
pub struct TrayState {
    tray: Option<Tray>,
    pub apps: Vec<TrayApp>,
}

pub fn start(handle: &LoopHandle<'static, App>, state: &mut State, session_bus: Option<&str>) {
    let Some(bus) = session_bus else {
        return;
    };
    let (sender, received) = channel::channel::<TrayEvent>();
    let inserted = handle.insert_source(received, |event, _, app| {
        if let channel::Event::Msg(event) = event {
            app.timed("tray", |app| app.state.tray_event(event));
        }
    });
    if let Err(err) = inserted {
        tracing::warn!(%err.error, "no tray");
        return;
    }
    let sender = std::sync::Mutex::new(sender);
    let sink = move |event| {
        if let Ok(sender) = sender.lock() {
            let _ = sender.send(event);
        }
    };
    match Tray::start(bus.to_owned(), ICON_PIXELS, sink) {
        Ok(tray) => state.tray.tray = Some(tray),
        Err(err) => tracing::warn!(%err, "no tray"),
    }
}

fn menu_entry(e: MenuEntry) -> TrayMenuEntry {
    TrayMenuEntry {
        id: e.id,
        label: e.label,
        enabled: e.enabled,
        separator: e.separator,
        toggle: e.toggle,
        children: e.children.into_iter().map(menu_entry).collect(),
    }
}

impl State {
    fn tray_event(&mut self, event: TrayEvent) {
        match event {
            TrayEvent::Item(item) => {
                let icon = item.icon.and_then(|i| {
                    let premultiplied: Vec<u8> = i
                        .rgba
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|p| {
                            let a = u16::from(p[3]);
                            let m = |c: u8| ((u16::from(c) * a + 127) / 255) as u8;
                            [m(p[0]), m(p[1]), m(p[2]), p[3]]
                        })
                        .collect();
                    Some((
                        u16::try_from(i.width).ok()?,
                        u16::try_from(i.height).ok()?,
                        premultiplied.into(),
                    ))
                });
                let app = TrayApp {
                    title: if item.title.is_empty() {
                        item.key.split('/').next().unwrap_or_default().to_owned()
                    } else {
                        item.title
                    },
                    key: item.key,
                    attention: item.attention,
                    icon,
                };
                match self.tray.apps.iter_mut().find(|a| a.key == app.key) {
                    Some(known) => *known = app,
                    None => self.tray.apps.push(app),
                }
                // Items come from threads in any order: by name, always
                // the same.
                self.tray
                    .apps
                    .sort_by(|a, b| a.title.cmp(&b.title).then(a.key.cmp(&b.key)));
            }
            TrayEvent::Gone(key) => self.tray.apps.retain(|a| a.key != key),
            TrayEvent::Menu { key, entries } => {
                if let Some(shell) = self.shell.as_mut() {
                    shell
                        .column
                        .show_tray_menu(key, entries.into_iter().map(menu_entry).collect());
                }
            }
        }
        self.needs_redraw = true;
    }

    /// Where the pointer is, for an item placing its own window or menu.
    fn tray_at(&self) -> (i32, i32) {
        self.seat
            .get_pointer()
            .map(|p| {
                let at = p.current_location();
                (at.x as i32, at.y as i32)
            })
            .unwrap_or_default()
    }

    pub fn tray_activate(&mut self, key: String) {
        let (x, y) = self.tray_at();
        if self.tray.tray.is_none() {
            return;
        }
        let token = self.shell_token();
        if let Some(tray) = &self.tray.tray {
            tray.send(TrayCommand::Activate { key, x, y, token });
        }
    }

    pub fn tray_menu(&mut self, key: String) {
        let (x, y) = self.tray_at();
        if let Some(tray) = &self.tray.tray {
            tray.send(TrayCommand::Menu { key, x, y });
        }
    }

    pub fn tray_chosen(&mut self, key: String, id: i32) {
        if let Some(tray) = &self.tray.tray {
            tray.send(TrayCommand::Chosen { key, id });
        }
    }
}
