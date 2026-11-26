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

/// An effect as messages for iced, or a line to the compositor.
// Over 200 lines: one arm per kind of effect.
pub(super) fn carry_out(effect: Effect) -> Vec<crate::Message> {
    use crate::Message;
    use iced_exwlshell::reexport::{Anchor, Layer, NewLayerShellSettings, OutputOption};
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
            vec![crate::layer::open(
                id,
                NewLayerShellSettings {
                    size: crate::layer::size(size),
                    layer: Layer::Top,
                    anchor: crate::side::anchor(Anchor::Right | Anchor::Bottom),
                    exclusive_zone: Some(0),
                    margin: Some(crate::layer::margin(crate::side::margin(margin))),
                    keyboard_interactivity: keyboard,
                    output_option: if bar == crate::ALONE {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(bar)
                    },
                    blur_option: iced_exwlshell::reexport::BlurOption::None,
                    events_transparent: false,
                    namespace: Some(POPOVER_NAMESPACE.to_owned()),
                },
            )]
        }
        Effect::Keyboard(id, keyboard_interactivity) => {
            vec![Message::KeyboardInteractivityChange {
                id,
                keyboard_interactivity,
            }]
        }
        Effect::Resize { id, size, margin } => crate::layer::resize(id, size)
            .into_iter()
            .chain(margin.map(|margin| crate::layer::moved(id, crate::side::moved(id, margin))))
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
            vec![crate::layer::close(id)]
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
            // All of it, or none.
            vec![crate::layer::input(id, takes.then_some(crate::layer::ALL))]
        }
        Effect::NextLayout => {
            tell(ToCompositor::NextLayout);
            Vec::new()
        }
        Effect::Ring { bar, rect } => {
            if bar != crate::ALONE {
                tell(ToCompositor::Ring(row_on(bar, rect)));
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
            let row = (bar != crate::ALONE).then(|| row_on(bar, row));
            tell(ToCompositor::GoToAgent { id, row });
            Vec::new()
        }
        Effect::PeekClose => {
            tell(ToCompositor::PeekClose);
            Vec::new()
        }
        Effect::Mirror(on) => {
            tell(ToCompositor::Mirror(on));
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
            vec![crate::layer::open(
                id,
                NewLayerShellSettings {
                    size: crate::layer::size(size),
                    layer: Layer::Top,
                    anchor: crate::side::anchor(Anchor::Right | Anchor::Top),
                    exclusive_zone: Some(0),
                    margin: Some(crate::layer::margin(crate::side::margin(margin))),
                    keyboard_interactivity: KeyboardInteractivity::None,
                    output_option: if bar == crate::ALONE {
                        OutputOption::Active
                    } else {
                        OutputOption::OutputName(bar)
                    },
                    blur_option: iced_exwlshell::reexport::BlurOption::None,
                    events_transparent: false,
                    namespace: Some(namespace.to_owned()),
                },
            )]
        }
        Effect::Region { id, rects } => vec![crate::layer::input(id, rects)],
        Effect::Launcher { id, output } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                // All of the screen: anchored to its four edges.
                size: crate::layer::size((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((0, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::LAUNCHER_NAMESPACE.to_owned()),
            },
        )],
        Effect::Toast { id, output, size } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                // Centred across the space the bar leaves.
                anchor: Anchor::Bottom,
                exclusive_zone: Some(0),
                margin: Some(crate::layer::margin((
                    0,
                    0,
                    crate::launcher::toast::BOTTOM,
                    0,
                ))),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: true,
                namespace: Some(hocus_link::LAUNCHER_TOAST_NAMESPACE.to_owned()),
            },
        )],
        Effect::Agent {
            id,
            output,
            size,
            at: (x, y),
            kind,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                // From the screen's corner, whatever the bar keeps.
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((y.max(0), 0, 0, x.max(0)))),
                // The card takes the keyboard once clicked, never before.
                keyboard_interactivity: if kind == crate::mcp::Surface::Card {
                    KeyboardInteractivity::OnDemand
                } else {
                    KeyboardInteractivity::None
                },
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: kind == crate::mcp::Surface::Tag,
                namespace: Some(kind.namespace().to_owned()),
            },
        )],
        Effect::Emoji {
            id,
            output,
            size,
            margin,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                // Its margins from the screen's corner, whatever other
                // surfaces keep.
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin(margin)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::EMOJI_NAMESPACE.to_owned()),
            },
        )],
        Effect::Project {
            id,
            output,
            size,
            margin,
            namespace,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin(margin)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(namespace.to_owned()),
            },
        )],
        Effect::Actions {
            id,
            output,
            size,
            margin,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Left,
                // Its margins from the screen's corner, whatever other
                // surfaces keep.
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin(margin)),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::ACTIONS_NAMESPACE.to_owned()),
            },
        )],
        Effect::Tell(message) => {
            tell(message);
            Vec::new()
        }
        Effect::Tasks(act) => {
            crate::tasks::act(act);
            Vec::new()
        }
        Effect::CaptureBar { id, output, size } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Bottom,
                exclusive_zone: Some(0),
                margin: Some(crate::layer::margin((
                    0,
                    0,
                    hocus_link::CAPTURE_TOOLBAR_BOTTOM.round() as i32,
                    0,
                ))),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_TOOLBAR_NAMESPACE.to_owned()),
            },
        )],
        Effect::CaptureThumb {
            id,
            output,
            size,
            margin,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                anchor: Anchor::Bottom | Anchor::Right,
                // Its margins from the screen's corner, whatever the bar
                // keeps.
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin(margin)),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_THUMB_NAMESPACE.to_owned()),
            },
        )],
        Effect::CaptureEditor { id, output } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((0, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::CAPTURE_EDITOR_NAMESPACE.to_owned()),
            },
        )],
        Effect::LockSurface { id, output } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size((0, 0)),
                layer: Layer::Overlay,
                anchor: Anchor::Top | Anchor::Bottom | Anchor::Left | Anchor::Right,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((0, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::LOCK_NAMESPACE.to_owned()),
            },
        )],
        Effect::CurtainSurface { id, output, top } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size((
                    hocus_link::PILL_WIDTH as u32,
                    hocus_link::PILL_HEIGHT as u32,
                )),
                layer: Layer::Overlay,
                // Centred across the screen, its top fixed.
                anchor: Anchor::Top,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((top, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::None,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: true,
                namespace: Some(hocus_link::CURTAIN_NAMESPACE.to_owned()),
            },
        )],
        Effect::Auth {
            id,
            output,
            size,
            top,
        } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                // Centred across the screen, its top fixed: it grows down.
                anchor: Anchor::Top,
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((top, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::AUTH_NAMESPACE.to_owned()),
            },
        )],
        Effect::ShareChooser { id, output, size } => vec![crate::layer::open(
            id,
            NewLayerShellSettings {
                size: crate::layer::size(size),
                layer: Layer::Overlay,
                // No anchor: centred on the screen.
                anchor: Anchor::empty(),
                exclusive_zone: Some(-1),
                margin: Some(crate::layer::margin((0, 0, 0, 0))),
                keyboard_interactivity: KeyboardInteractivity::Exclusive,
                output_option: if output == crate::ALONE {
                    OutputOption::Active
                } else {
                    OutputOption::OutputName(output)
                },
                blur_option: iced_exwlshell::reexport::BlurOption::None,
                events_transparent: false,
                namespace: Some(hocus_link::SHARE_CHOOSER_NAMESPACE.to_owned()),
            },
        )],
    }
}

/// `rect` on bar `bar`, as the compositor is told it: in its pixels, at
/// the text size.
fn row_on(bar: String, rect: iced::Rectangle) -> hocus_link::Row {
    let [x, y, width, height] = crate::layer::told(rect);
    hocus_link::Row {
        output: bar,
        x,
        y,
        width,
        height,
    }
}
