# Decision records

One file per decision with lasting consequences: context, decision,
consequences. A decision is changed by a new record that supersedes it, never
by silently editing the old one.

| # | Decision | Status |
|---|---|---|
| [0001](0001-rust-and-smithay.md) | Rust and smithay | Accepted |
| [0002](0002-mit-and-permissive-dependencies.md) | MIT license, permissive dependencies only | Accepted |
| [0003](0003-shell-in-the-compositor.md) | The shell is drawn by the compositor, in one process | Accepted |
| [0004](0004-plugins-compiled-in.md) | Plugins are Rust crates compiled in; no scripting | Accepted |
| [0005](0005-per-screen-desktops.md) | Virtual desktops are per screen | Accepted |
| [0006](0006-no-title-bars.md) | No title bars by default | Accepted |
| [0007](0007-widget-column-right.md) | The widget panel is a column on the right | Superseded by 0011 |
| [0008](0008-ai-testable-by-design.md) | Every feature is testable by an AI, without a screen | Accepted |
| [0009](0009-ui-rendering-stack.md) | hocus-ui draws on the CPU with vello_cpu and parley | Accepted |
| [0010](0010-settings-as-a-client.md) | The settings app is a Wayland client drawn with hocus-ui | Accepted |
| [0011](0011-panel-strip-by-default.md) | The widget panel is a thin strip by default, opening into the column | Accepted |
