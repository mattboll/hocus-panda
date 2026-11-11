//! The controller's effects carried out: layer-shell actions and lines to
//! the compositor.

use super::*;

/// Carries out the controller's effects, and its deadlines: layer-shell
/// actions (iced runs them without drawing the other surfaces) and lines
/// to the compositor. A shell alone wakes its surfaces with a message.
pub fn effects() -> impl iced::futures::Stream<Item = crate::Message> {
    use iced::futures::SinkExt;
    iced::stream::channel(16, async |mut output| {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        *EFFECTS.lock().unwrap_or_else(PoisonError::into_inner) = Some(sender);
        loop {
            let deadline = with(|c| c.deadline());
            let deadline = [
                deadline,
                crate::launcher::deadline(),
                crate::capture::thumb::deadline(),
                crate::tray::tip::deadline(),
                crate::tasks::deadline(),
            ]
            .into_iter()
            .flatten()
            .min();
            let effects = match deadline {
                Some(at) => {
                    use iced::futures::future::{Either, select};
                    let due = std::pin::pin!(tokio::time::sleep_until(at.into()));
                    let next = std::pin::pin!(receiver.recv());
                    match select(next, due).await {
                        Either::Left((effects, _)) => effects,
                        Either::Right(((), _)) => {
                            let now = Instant::now();
                            let mut due = with(|c| c.tick(now));
                            due.extend(crate::launcher::tick(now));
                            due.extend(crate::capture::thumb::tick(now));
                            due.extend(crate::tray::tip::tick(now));
                            due.extend(crate::tasks::tick(now));
                            Some(due)
                        }
                    }
                }
                None => receiver.recv().await,
            };
            let Some(effects) = effects else {
                return;
            };
            for message in effects.into_iter().flat_map(carry_out) {
                if output.send(message).await.is_err() {
                    return;
                }
            }
        }
    })
}

/// `effects` as iced's messages, for a task to run in its order: a bar's
/// surfaces opened after its own, so that they are in front of it.
pub fn messages(effects: Vec<Effect>) -> Vec<crate::Message> {
    effects.into_iter().flat_map(carry_out).collect()
}

/// Wider than any surface: a region as large as can be.
pub(super) const EVERYWHERE: i32 = 1 << 20;

pub(super) type WlRegion = wayland_client::protocol::wl_region::WlRegion;

/// An effect as messages for iced, or a line to the compositor.
pub(super) fn carry_out(effect: Effect) -> Vec<crate::Message> {
    use crate::Message;
    use iced_layershell::reexport::{Anchor, Layer, NewLayerShellSettings, OutputOption};
    match effect {
        Effect::Open {
            id,
            bar,
            size,
            margin,
            keyboard,
            ..
        } => {
            crate::side::opened(id);
            vec![Message::NewLayerShell {
                settings: NewLayerShellSettings {
                    size: Some(size),
                    layer: Layer::Top,
                    anchor: crate::side::anchor(Anchor::Right | Anchor::Bottom),
                    exclusive_zone: Some(0),
                    margin: Some(crate::side::margin(margin)),
                    keyboard_interactivity: keyboard,
                    output_option: if bar == crate::ALONE {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(bar)
                    },
                    events_transparent: false,
                    namespace: Some(POPOVER_NAMESPACE.to_owned()),
                },
                id,
            }]
        }
        Effect::Keyboard(id, keyboard_interactivity) => {
            vec![Message::KeyboardInteractivityChange {
                id,
                keyboard_interactivity,
            }]
        }
        Effect::Resize { id, size, margin } => std::iter::once(Message::SizeChange { id, size })
            .chain(margin.map(|margin| Message::MarginChange {
                id,
                margin: crate::side::moved(id, margin),
            }))
            .collect(),
        Effect::Closing => {
            tell(ToCompositor::Closing(POPOVER_NAMESPACE.to_owned()));
            Vec::new()
        }
        Effect::FirstControl(id) => {
            kit::opened_by_keyboard(id);
            Vec::new()
        }
        Effect::Remove(id) => {
            crate::side::gone(id);
            vec![Message::RemoveWindow(id)]
        }
        Effect::Rebuild => vec![crate::Message::Keys],
        Effect::Wake(namespace) => {
            if tell(ToCompositor::Wake(namespace.to_owned())) {
                Vec::new()
            } else {
                vec![Message::Levels]
            }
        }
        Effect::Input { id, takes } => {
            // Whatever region it had (each bar's surfaces share one): all
            // of it, or none.
            let callback =
                iced_layershell::actions::ActionCallback::new(move |region: &WlRegion| {
                    region.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
                    if takes {
                        region.add(0, 0, EVERYWHERE, EVERYWHERE);
                    }
                });
            vec![Message::SetInputRegion { id, callback }]
        }
        Effect::NextLayout => {
            tell(ToCompositor::NextLayout);
            Vec::new()
        }
        Effect::Ring { bar, rect } => {
            if bar != crate::ALONE {
                tell(ToCompositor::Ring(hocus_link::Row {
                    output: bar,
                    x: rect.x.round() as i32,
                    y: rect.y.round() as i32,
                    width: rect.width.round() as i32,
                    height: rect.height.round() as i32,
                }));
            }
            Vec::new()
        }
        Effect::Level(key) => {
            crate::levels::key(key);
            Vec::new()
        }
        Effect::Desktop { bar, to } => {
            tell(ToCompositor::Desktop { output: bar, to });
            Vec::new()
        }
        Effect::Bring(id) => {
            tell(ToCompositor::Bring(id));
            Vec::new()
        }
        Effect::GoToAgent { id, bar, row } => {
            let row = (bar != crate::ALONE).then(|| hocus_link::Row {
                output: bar,
                x: row.x.round() as i32,
                y: row.y.round() as i32,
                width: row.width.round() as i32,
                height: row.height.round() as i32,
            });
            tell(ToCompositor::GoToAgent { id, row });
            Vec::new()
        }
        Effect::PeekClose => {
            tell(ToCompositor::PeekClose);
            Vec::new()
        }
        Effect::Plug(tool) => {
            tell(ToCompositor::Plug(tool.word().to_owned()));
            Vec::new()
        }
        Effect::Unplug(tool) => {
            tell(ToCompositor::Unplug(tool.word().to_owned()));
            Vec::new()
        }
        Effect::Decline => {
            tell(ToCompositor::Decline);
            Vec::new()
        }
        Effect::FollowHerdr => {
            tell(ToCompositor::Follow("herdr".to_owned()));
            Vec::new()
        }
        Effect::Briefing(open) => {
            tell(ToCompositor::Briefing { open });
            Vec::new()
        }
        Effect::Copy(text) => {
            tell(ToCompositor::Copy(text));
            Vec::new()
        }
        Effect::OpenCalendar { bar, day, event } => {
            calendar::open_on(&bar, day, event);
            Vec::new()
        }
        Effect::TrayMenu { bar, key, rect } => {
            crate::tray::ask(crate::tray::Ask::Menu { key, bar, rect });
            Vec::new()
        }
        Effect::ToggleLauncher(bar) => {
            // The scale as the compositor's line says it, in 120ths.
            let scale = (kit::scale(&bar) * 120.0).round() as u32;
            crate::launcher::toggle(bar, scale);
            Vec::new()
        }
        Effect::Layer {
            id,
            bar,
            namespace,
            size,
            margin,
        } => {
            crate::side::opened(id);
            vec![Message::NewLayerShell {
                settings: NewLayerShellSettings {
                    size: Some(size),
                    layer: Layer::Top,
                    anchor: crate::side::anchor(Anchor::Right | Anchor::Top),
                    exclusive_zone: Some(0),
                    margin: Some(crate::side::margin(margin)),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: if bar == crate::ALONE {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(bar)
                    },
                    events_transparent: false,
                    namespace: Some(namespace.to_owned()),
                },
                id,
            }]
        }
        Effect::Region { id, rects } => {
            let rects: Vec<(i32, i32, i32, i32)> = rects
                .iter()
                .map(|r| {
                    let (x, y) = (r.x.floor() as i32, r.y.floor() as i32);
                    let right = (r.x + r.width).ceil() as i32;
                    let bottom = (r.y + r.height).ceil() as i32;
                    (x, y, right - x, bottom - y)
                })
                .collect();
            let callback =
                iced_layershell::actions::ActionCallback::new(move |region: &WlRegion| {
                    region.subtract(-EVERYWHERE, -EVERYWHERE, 2 * EVERYWHERE, 2 * EVERYWHERE);
                    for &(x, y, w, h) in &rects {
                        region.add(x, y, w, h);
                    }
                });
            vec![Message::SetInputRegion { id, callback }]
        }
        Effect::Launcher { id, output } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                // All of the screen: anchored to its four edges.
                size: Some((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some((0, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::LAUNCHER_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::Toast { id, output, size } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                // Centred across the space the bar leaves.
                anchor: Anchor::Bottom,
                exclusive_zone: Some(0),
                margin: Some((0, 0, crate::launcher::toast::BOTTOM, 0)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: true,
                namespace: Some(hocus_link::LAUNCHER_TOAST_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::Emoji {
            id,
            output,
            size,
            margin,
        } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                // Its margins from the screen's corner, whatever other
                // surfaces keep.
                exclusive_zone: Some(-1),
                margin: Some(margin),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::EMOJI_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::Actions {
            id,
            output,
            size,
            margin,
        } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                // Its margins from the screen's corner, whatever other
                // surfaces keep.
                exclusive_zone: Some(-1),
                margin: Some(margin),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::ACTIONS_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::Tell(message) => {
            tell(message);
            Vec::new()
        }
        Effect::Tasks(act) => {
            crate::tasks::act(act);
            Vec::new()
        }
        Effect::CaptureBar { id, output, size } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                anchor: Anchor::Bottom,
                exclusive_zone: Some(0),
                margin: Some((0, 0, hocus_link::CAPTURE_TOOLBAR_BOTTOM.round() as i32, 0)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_TOOLBAR_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::CaptureThumb {
            id,
            output,
            size,
            margin,
        } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                anchor: Anchor::Bottom | Anchor::Right,
                // Its margins from the screen's corner, whatever the bar
                // keeps.
                exclusive_zone: Some(-1),
                margin: Some(margin),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_THUMB_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::CaptureEditor { id, output } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some((0, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_EDITOR_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::LockSurface { id, output } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some((0, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::LOCK_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::CurtainSurface { id, output, top } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some((
                    hocus_link::PILL_WIDTH as u32,
                    hocus_link::PILL_HEIGHT as u32,
                )),
                layer: Layer::Overlay,
                // Centred across the screen, its top fixed.
                anchor: Anchor::Top,
                exclusive_zone: Some(-1),
                margin: Some((top, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: true,
                namespace: Some(hocus_link::CURTAIN_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::Auth {
            id,
            output,
            size,
            top,
        } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                // Centred across the screen, its top fixed: it grows down.
                anchor: Anchor::Top,
                exclusive_zone: Some(-1),
                margin: Some((top, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::AUTH_NAMESPACE.to_owned()),
            },
            id,
        }],
        Effect::ShareChooser { id, output, size } => vec![Message::NewLayerShell {
            settings: NewLayerShellSettings {
                size: Some(size),
                layer: Layer::Overlay,
                // No anchor: centred on the screen.
                anchor: Anchor::empty(),
                exclusive_zone: Some(-1),
                margin: Some((0, 0, 0, 0)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                events_transparent: false,
                namespace: Some(hocus_link::SHARE_CHOOSER_NAMESPACE.to_owned()),
            },
            id,
        }],
    }
}
