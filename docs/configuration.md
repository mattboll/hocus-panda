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
title_bars = false # a bar with the title and buttons above each window (decision 0006)
buttons = "right"  # which side of the bar has minimize, maximize and close: right, left

[appearance]
theme = "dark"             # dark, light, matrix, hud, paper
accent = "theme"           # theme, bamboo, amber, blue, pink, or wallpaper (taken from it, made readable)
text_size = 100            # percent of the theme's text size, 80 to 150
high_contrast = false      # borders and muted text closer to the text colour
reduced_motion = false     # animations jump to their end
transparency = true        # frosted glass behind panels (with blur, #37)

[wallpaper]                # behind the windows of every screen
kind = "glow"              # glow (soft light by the time of day), image, folder (its images through the day), colour
image = "~/Pictures/hills.jpg"
folder = "~/Pictures/day"
mode = "fill"              # fill, fit, stretch, center
colour = "#0e0f12"         # around a fitted image, or the whole screen; the theme's background if not set

[[wallpaper.screens]]      # one screen's own, over the settings above
output = "DP-1"
image = "~/Pictures/wide.png"

[session]                  # the last session, at start
restore = "ask"            # ask (a card), always, never
autostart = true           # start the XDG autostart entries

[idle]                     # nobody at the keyboard
curtain_after = 300        # seconds before the curtain dims the screens (0: never)
lock_after = 30            # seconds of curtain before the session locks
lock = true                # false: the curtain only dims

[clipboard]                # the history (Super+V)
history = true             # keeps what is copied; never what password managers mark secret
entries = 100              # copies kept, pinned ones besides
keep = true                # across sessions, encrypted

[screenshots]              # Print and its variants
folder = ""                # empty: Screenshots in the pictures folder
save = true                # into the folder
copy = true                # onto the clipboard (as PNG)
ocr = "tesseract"          # reads the text for "Copy the text", when installed
annotate = "hocus-annotate" # the toast's "Annotate", given the capture's file

[updates]                  # the system's, shown in the column and the settings
enabled = true
every_hours = 6            # between looks, 1 to 168; never while fullscreen or on the power saver
check = ""                 # a command printing NAME VERSION (or NAME OLD -> NEW) per update;
install = ""               #   empty: apt, dnf, pacman or pkg, installed through polkit

[[calendars]]              # one per account or subscription (Settings → Accounts and calendar)
name = "Work"
kind = "caldav"            # or "subscription": an ICS link, read as it is
url = "https://dav.example.org/calendars/alex/"  # https (or webcal); plain http to this machine only
user = "alex"              # the password is in the keyring, never here

[night_light]              # warmer colours in the evening (quick settings' tile)
enabled = false
schedule = "fixed"         # or "sunset": from sunset to sunrise where you are
from = "21:00"             # fixed hours, your time
to = "07:00"
latitude = 48.85           # for "sunset"
longitude = 2.35
temperature = 3500         # kelvin at night, 1500 to 6500
except = []                # outputs it leaves alone, by name

[recording]                # Super+Shift+R
folder = ""                # empty: Screencasts in the videos folder
fps = 30                   # 10 to 60
countdown = 3              # seconds before it starts, 0 to 10
show_clicks = true         # rings in the video, never on the screen
hardware = true            # VA-API encoding when the graphics card has it
encoder = "ffmpeg"         # the encoder, run as a process of its own
microphone = false         # the options offered when picking what to record, as they start
system_sound = false       #   (the picker's buttons change them for the session)
camera = false             #   a webcam bubble, moved with the pointer while recording
show_keys = false          #   combinations (Ctrl+C, Super+Tab…) drawn in the video; never plain typing
sound = "pulse"            # how the encoder reads sound: pulse (PipeWire too), oss (FreeBSD), alsa
microphone_device = "default"
system_device = "@DEFAULT_MONITOR@"  # what plays
camera_input = "v4l2"
camera_device = "/dev/video0"

[settings]                 # the settings app (Super+Comma)
command = "hocus-settings" # `--page <page>` is added for a page
app_id = "hocus-settings"  # how its window is recognised

[dropdown]                 # the drop-down terminal (Super+Backtick)
command = "foot --app-id=hocus-dropdown"
app_id = "hocus-dropdown"  # how its window is recognised
height = 0.45              # share of the screen's height (0.2 to 0.9)

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

[[outputs]]                # a screen, by name (`hocus msg screens`)
name = "eDP-1"
enabled = true             # false: off; its desktops and windows wait for it
mode = "2560x1600@60"      # WIDTHxHEIGHT[@HZ]; the screen's preferred one otherwise
scale = 1.5                # 0.5 to 4, rounded to 1/120
transform = "normal"       # 90, 180, 270, flipped, flipped-90, flipped-180, flipped-270
position = [0, 0]          # logical, top-left corner
vrr = false                # adaptive sync, where the screen and GPU can
primary = true             # where unplugged screens' desktops and windows go
desktops = 1               # its own desktops, 1 to 8 (desktops.per_output otherwise)

[[layouts]]                # how a set of screens is placed (the Screens page writes them)
name = "Home"
outputs = ["eDP-1", "DP-1"]           # in effect when exactly these are connected
positions = { DP-1 = [0, 0], eDP-1 = [320, 1440] }

[shortcuts]                # the Keyboard page's editor writes them
"Super+Return" = "spawn foot"
"Ctrl+Alt+Right" = "next-desktop all"
"Alt+F4" = "none"            # removes a default
```

## Window rules

Each `[[rules]]` applies to the windows whose `app_id` and `title` match its
patterns (both optional); when several match, later rules override earlier
ones, field by field. Super+wheel over a window changes its opacity, and the
new opacity is remembered for that application.

## Screens

Each `[[outputs]]` sets one screen; what it does not set goes back to how the
screen was lit (its preferred mode, scale 1, no rotation, to the right of the
others). Changes apply at once and move nothing else: each window keeps its
place on its screen, maximized and fullscreen ones fill it again, and a
layout seen before puts every window back where it was. Screen tools
(wlr-randr, kanshi, wdisplays) can change the same for the running session
through wlr-output-management; the file wins again once it changes. Turning
a screen's light off and on (output-power, as swayidle does) keeps it in the
layout.

Where screens are depends on which are connected: each `[[layouts]]` places
one set of screens, and wins over the `position` of `[[outputs]]` while
exactly those screens are connected. The settings' Screens page writes one
when a screen is dragged into place, named after its screens until renamed.
Within a session, the windows' places are kept per layout too: plugging a
set of screens again puts its windows back.

## Shortcuts

A combination is modifiers (`Super`, `Ctrl`, `Alt`, `Shift`) and a key joined
by `+`. Digits are the keys of the top row, whatever the layout (on AZERTY,
`Super+2` is the `é` key); other keys go by their name in the active layout
(`P`, `Right`, `Return`, `F4`, `Space`, `BackSpace`…). When the file binds the
same keys twice, the first one wins and the second is reported. `Backtick`
is the key left of 1 whatever the layout (on AZERTY, the `²` key).

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
| `settings` | opens the settings app, or brings its window forward |
| `lock` | locks the session now |
| `log-out`, `restart`, `shut-down` | after a ten-second countdown (Escape cancels), or once the applications holding it back are dealt with: saves the session, asks windows to close, then leaves |
| `clipboard-history` | the clipboard history in the launcher's card: newest first, Enter pastes into the focused application, Tab pins, Delete forgets |
| `palette` | the action palette: every action with its keys and what it acts on, recent ones first; searched by name (English or French) or by keys, typed (`Maj+S`) or pressed; Enter runs it on the window that had the focus, Tab opens the settings to give it keys |
| `emoji` | the emoji picker: searched in English and French, recent ones first, Tab changes the skin tone, Enter types it into the focused application |
| `notes` | notes and to-dos in the launcher's card: what is typed is added, a to-do when it starts `- [ ]`; on an empty line Enter ticks, Delete removes. Kept in `~/.local/share/hocuspanda/notes.md`, plain Markdown; the column shows the to-dos left, the command field finds notes |
| `focus-mode` | everything but the focused window dims; again, it lifts |
| `night-light` | night light on or off, until the schedule changes |
| `zoom-in`, `zoom-out` | the magnifier, around the pointer, up to 8×; back to 1 it is off (also Super+Alt+wheel) |
| `record` | records the screen: a region drawn, a window clicked, or Enter for the screen under the pointer; after a countdown, a red frame marks it and a bar shows the time, pause and stop; again to stop. Saved as MP4, then a toast offers to open or delete it |
| `screenshot MODE [SECONDS]` | `screen` (under the pointer), `all` (every screen, each at its own pixel density), `window` (the one clicked next), `region` (drawn with the pointer, its size shown; Escape cancels), after a delay of up to 60 s: copied and saved, then a toast offers to open, delete or copy its text |
| `suspend`, `hibernate` | sleeps now, locked first; hibernating only where the machine can |
| `dropdown` | shows the drop-down terminal on the screen under the pointer, starting it the first time, or hides it (it keeps running) |
| `switch-windows`, `switch-windows all` | the window switcher (#39) |
| `spawn COMMAND` | runs a command through the shell |
| `quit` | ends the session at once, without asking windows to close |
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
| Super+V | `clipboard-history` |
| Super+Period | `emoji` |
| Super+P | `palette` |
| Print | `screenshot screen` |
| Super+Print | `screenshot all` |
| Shift+Print, Super+Shift+S | `screenshot region` |
| Super+Shift+R | `record` |
| Super+F | `focus-mode` |
| Super+N | `notes` |
| Super+Equal, Super+Minus | `zoom-in`, `zoom-out` |
| Ctrl+Print | `screenshot window` |
| Super+A | `quick-settings` |
| Super+Backtick | `dropdown` |
| Super+Comma | `settings` |
| Super+L | `lock` |
| Alt+Tab / Super+Tab | `switch-windows` / `switch-windows all` |
| Ctrl+Alt+BackSpace | `quit` |
