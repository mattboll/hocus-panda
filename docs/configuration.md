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

[appearance]
theme = "dark"             # dark, light, matrix, hud, paper
accent = "theme"           # theme, bamboo, amber, blue, pink, or wallpaper (taken from it, made readable)

[wallpaper]                # behind the windows of every screen
kind = "glow"              # glow (soft light by the time of day), image, folder (its images through the day), colour
image = "~/Pictures/hills.jpg"
folder = "~/Pictures/day"
mode = "fill"              # fill, fit, stretch, center
colour = "#0e0f12"         # around a fitted image, or the whole screen; the theme's background if not set

[[wallpaper.screens]]      # one screen's own, over the settings above
output = "DP-1"
image = "~/Pictures/wide.png"

[launcher]                 # what the launcher offers besides applications
calculator = true          # 2*21
units = true               # 12 km in miles
urls = true                # example.org
web_search = true          # always last
search_engine = "https://duckduckgo.com/?q={}"
large_type = true          # large <words>
qr = true                  # qr <words>

[[rules]]                  # windows matching app_id and title (globs: * and ?)
app_id = "chat*"
output = "eDP-1"           # opens there, when plugged
desktop = 1                # on that desktop (1 to 8)
size = [900, 700]
maximized = false
opacity = 0.95             # 0.1 to 1
pinned = true              # on every desktop of its output
on_top = false             # above the other windows

[shortcuts]
"Super+Return" = "spawn foot"
"Ctrl+Alt+Right" = "next-desktop all"
"Alt+F4" = "none"            # removes a default
```

## Window rules

Each `[[rules]]` applies to the windows whose `app_id` and `title` match its
patterns (both optional); when several match, later rules override earlier
ones, field by field. Super+wheel over a window changes its opacity, and the
new opacity is remembered for that application.

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
| `toggle-tiling` | tiles the shown desktop of the output under the pointer as master and stack, or back to floating |
| `make-master`, `grow-master`, `shrink-master` | the focused window becomes the master; the master's share of the width grows or shrinks |
| `toggle-floating` | takes the focused window out of the tiling, or back in as master |
| `snap ZONE` | snaps the focused window: `left`, `right`, `top-left`, `top-right`, `bottom-left`, `bottom-right`, `top` (maximize) |
| `switch-layout` | the next keyboard layout |
| `launcher` | opens the launcher (applications, calculator), or closes it |
| `quick-settings` | opens quick settings on the screen under the pointer, or closes them |
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
| Super+Left / Right | `snap left` / `snap right` |
| Super+Down | `minimize` |
| Super+F11 | `toggle-fullscreen` |
| Super+Shift+T | `toggle-tiling` |
| Super+Shift+J | `make-master` |
| Super+Shift+H / L | `shrink-master` / `grow-master` |
| Super+Shift+F | `toggle-floating` |
| Alt+F4 | `close` |
| Super+Ctrl+Space | `switch-layout` |
| Super+Space | `launcher` |
| Super+A | `quick-settings` |
| Alt+Tab / Super+Tab | `switch-windows` / `switch-windows all` |
| Ctrl+Alt+BackSpace | `quit` |
