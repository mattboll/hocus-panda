# Configuration

`~/.config/hocuspanda/config.toml` (or `--config FILE`), applied as soon as
it is saved. Every value has a default: the file holds only what you change.
A file that does not read is refused with its line and key, and the previous
configuration stays; `hocus msg config` shows the configuration in effect and
the last error.

```toml
[windows.gaps]
inner = 8          # between windows, logical pixels (0 to 64)
outer = 8          # around them, at the edges of the work area (0 to 64)

[desktops]
per_output = 4     # desktops of an output seen for the first time (1 to 8)

[keyboard]
layouts = ["fr", "us"]   # XKB layouts, the first active at start
options = "caps:escape"  # XKB options
repeat_delay = 200       # ms
repeat_rate = 25         # per second
numlock = true           # Num Lock on at start

[decorations]
radius = 12        # window corners (0 to 32)
shadow = true
ring_width = 2     # focus ring
accent = "#7bc47f"

[shortcuts]
"Super+Return" = "spawn foot"
"Ctrl+Alt+Right" = "next-desktop all"
"Alt+F4" = "none"            # removes a default
```

## Shortcuts

A combination is modifiers (`Super`, `Ctrl`, `Alt`, `Shift`) and a key joined
by `+`. Digits are the keys of the top row, whatever the layout (on AZERTY,
`Super+2` is the `é` key); other keys go by their name in the active layout
(`P`, `Right`, `Return`, `F4`, `Space`, `BackSpace`…). When the file binds the
same keys twice, the first one wins and the second is reported.

| Action | Does |
|---|---|
| `desktop N` | shows desktop N (1 to 9) on the output under the pointer |
| `next-desktop`, `previous-desktop` | the next or previous desktop, round; add `all` for every output |
| `send-to-desktop N` | sends the focused window to desktop N of the output under the pointer |
| `send-to-next-desktop`, `send-to-previous-desktop` | the same, to the next or previous desktop of its output |
| `send-to-output-left`, `send-to-output-right` | sends the focused window to the output beside |
| `toggle-maximized`, `toggle-fullscreen`, `minimize`, `close` | the focused window |
| `switch-layout` | the next keyboard layout |
| `switch-windows`, `switch-windows all` | the window switcher (#39) |
| `spawn COMMAND` | runs a command through the shell |
| `quit` | ends the session |
| `none` | removes a default shortcut |

Defaults:

| Keys | Action |
|---|---|
| Super+1 … Super+9 | `desktop N` |
| Super+Shift+1 … 9 | `send-to-desktop N` |
| Ctrl+Alt+Left / Right | `previous-desktop` / `next-desktop` |
| Ctrl+Alt+Shift+Left / Right | `send-to-previous-desktop` / `send-to-next-desktop` |
| Super+Shift+Left / Right | `send-to-output-left` / `send-to-output-right` |
| Super+Up | `toggle-maximized` |
| Super+Down | `minimize` |
| Super+Shift+F | `toggle-fullscreen` |
| Alt+F4 | `close` |
| Super+Ctrl+Space | `switch-layout` |
| Alt+Tab / Super+Tab | `switch-windows` / `switch-windows all` |
| Ctrl+Alt+BackSpace | `quit` |
