# Configuration

`~/.config/hocuspanda/config.toml` (or `--config FILE`), applied as soon as
it is saved. Every value has a default: the file holds only what you change.
A file that does not read is refused with its line and key, and the previous
configuration stays; `hocus msg config` shows the configuration in effect and
the last error.

```toml
[windows]
snap = true        # dragged to a screen's edge or corner, a window snaps there (halves, quarters,
                   #   the top maximizes); false: it moves freely, its edges still stick
wobbly = true      # a dragged window bends like jelly; false: stiff (thrown, it still glides)

[windows.gaps]
inner = 8          # between windows, logical pixels (0 to 64)
outer = 8          # around them, at the edges of the work area (0 to 64)

[desktops]
per_output = 4     # desktops of an output seen for the first time (1 to 8)

[[desktops.looks]]  # a desktop's own theme and wallpaper, to know where you are; the column crossfades
desktop = 2         # from 1
output = "eDP-1"    # only this screen's desktop 2; every screen's when left out
name = "Website"    # what it is called, unless the screen's desktop_names names it
theme = "paper"     # as [appearance] theme; accent too
kind = "colour"     # as [wallpaper]: kind, image, folder, mode, colour
colour = "#efe7d6"

[keyboard]
layouts = ["fr", "fr(bepo)", "us"]   # XKB layouts, each with its variant in brackets as XKB
                                     #   names it (fr is AZERTY, fr(bepo) BÉPO, us(intl)…);
                                     #   the first active at start, US when none
options = "caps:escape"  # XKB options
repeat_delay = 200       # ms
repeat_rate = 25         # per second
numlock = true           # Num Lock on at start

[mouse]                    # mice, trackballs, pointing sticks: set through libinput on a session of
                           #   its own (in a window of another session, that one's settings apply)
natural_scroll = false     # the content follows the wheel
left_handed = false        # the buttons swapped, the touchpads' too
speed = 0.0                # -1 (slowest) to 1 (fastest)

[touchpad]
natural_scroll = false     # the content follows the fingers
tap_to_click = false       # one finger taps the left button, two the right, three the middle
disable_while_typing = true
speed = 0.0                # -1 to 1

[decorations]
radius = 14        # window corners (0 to 32)
shadow = true
ring_width = 2     # focus ring
accent = "theme"   # the focus ring: theme (the accent in effect) or "#rrggbb"
title_bars = false # a bar with the title and buttons above each window (decision 0006)
buttons = "right"  # which side of the bar has minimize, maximize and close: right, left

[appearance]
theme = "dark"             # dark, light, matrix, hud, paper
accent = "theme"           # theme, bamboo, amber, blue, pink, or wallpaper (taken from it, made readable)
text_size = 100            # percent of the theme's text size, 80 to 150
high_contrast = false      # borders and muted text closer to the text colour
reduced_motion = false     # animations jump to their end
transparency = true        # frosted glass behind panels: see-through over a blur of what is behind them, where the
                           #   GPU can blur (not with pixman), unless high contrast, reduced motion or the power saver is on

[wallpaper]                # behind the windows of every screen
kind = "glow"              # glow (soft light by the time of day; in the Matrix theme, its rain falling), image,
                           #   folder (its images through the day), colour, rain (the Matrix rain), stars: these move,
                           #   still with reduced motion, on the power saver, under a fullscreen window and without a GPU
image = "~/Pictures/hills.jpg"
folder = "~/Pictures/day"
mode = "fill"              # fill, fit, stretch, center
colour = "#0e0f12"         # around a fitted image, or the whole screen; the theme's background if not set

[[wallpaper.screens]]      # one screen's own, over the settings above
output = "DP-1"
image = "~/Pictures/wide.png"

[session]                  # the last session, at start
restore = "ask"            # ask (a card), always, never
autostart = true           # start the XDG autostart entries; Settings → Applications turns one
                           #   off in the user's copy (~/.config/autostart), as every desktop reads it

[idle]                     # nobody at the keyboard (on the charger, or without a battery)
curtain_after = 300        # seconds before the curtain dims the screens (0: never)
lock_after = 30            # seconds of curtain before the session locks
lock = true                # false: the curtain only dims
sleep_after = 0            # seconds before the machine sleeps, locked first (0: never)
sleep = "suspend"          # or "hibernate", where the machine can (it suspends otherwise)
fullscreen_awake = true    # a fullscreen window keeps the session awake, as applications asking do

[idle.battery]             # on battery (UPower says the charger is out); plugging or unplugging
curtain_after = 120        #   the charger counts as input, the other delays start from then; left
sleep_after = 900          #   out: these, or the charger's when shorter

[clipboard]                # the history (Super+V)
history = true             # keeps what is copied; never what password managers mark secret
entries = 100              # copies kept, pinned ones besides
keep = true                # across sessions, encrypted

[screenshots]              # Print and its variants
folder = ""                # empty: Screenshots in the pictures folder
save = true                # into the folder
copy = true                # onto the clipboard (as PNG)
ocr = "tesseract"          # reads the text for "Copy the text" and circling to ask, when installed
annotate = "hocus-annotate" # the toast's "Annotate", given the capture's file

[updates]                  # the system's, shown in the column and the settings
enabled = true             # false: neither looked for nor shown in the column
every_hours = 6            # between looks, 1 to 168; never while fullscreen or on the power saver
check = ""                 # a command printing NAME VERSION (or NAME OLD -> NEW) per update;
install = ""               #   empty: apt, dnf, pacman or pkg, installed through polkit

[phone]                    # through KDE Connect, when it runs: a phone paired and within reach
enabled = true             # its line in the column (battery, Ring), its popover
notifications = true       # its notifications as toasts, with a reply typed into them
adb = false                # wireless debugging when adb is installed: pairing by a QR code,
                           #   mirroring its screen when scrcpy is installed too

[notifications]            # toasts and the column's history (Settings → Notifications)
hold_while_sharing = true  # held while the screen is shared, then summed up in one toast
history = 20               # how many the column keeps, 0 to 100

[notifications.apps]       # per application, by the name it gives: show (the default),
Chat = "history"           #   history (the column only, no toast) or off; critical ones always show

[[calendars]]              # one per account or subscription (Settings → Accounts and calendar)
name = "Work"
kind = "caldav"            # or "subscription": an ICS link, read as it is
url = "https://dav.example.org/calendars/alex/"  # https (or webcal); plain http to this machine only
user = "alex"              # the password is in the keyring, never here
hidden_lists = []          # its task lists (calendars that hold VTODO) left out of
                           #   the column's tasks, by name: ["Groceries"]; all show by default
                           #   (Settings → Accounts and calendar, a switch per list)
                           # Google calendars come from GNOME Online Accounts
                           #   (sign in to Google there), not from this list, and
                           #   their Google Tasks with them: all their lists show

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
editor = "hocus-trim"      # the toast's "Trim", given the recording's file

[settings]                 # the settings app (Super+Comma)
command = "hocus-settings" # `--page <page>` is added for a page
app_id = "hocus-settings"  # how its window is recognised

[dropdown]                 # the drop-down terminal (Super+Backtick)
command = "foot --app-id=hocus-dropdown"
app_id = "hocus-dropdown"  # how its window is recognised
height = 0.45              # share of the screen's height (0.2 to 0.9)

[launcher]                 # what the launcher offers besides applications (Settings → Search)
calculator = true          # 2*21
units = true               # 12 km in miles
urls = true                # example.org
web_search = true          # always last
search_engine = "https://duckduckgo.com/?q={}"
large_type = true          # large <words>
qr = true                  # qr <words>
intents = true             # "capture de slack dans 3 secondes", "open the keyboard settings": understood
                           #   offline in English and French, shown to confirm before anything runs

[ai]                       # questions typed in the command field; the answer streams into it (Settings → AI and agents)
enabled = false            # off until turned on: nothing about the AI shows, nothing is sent
provider = "ollama"        # ollama, llamacpp (its server), openai (or any server speaking its chat
                           #   completions: LM Studio, vLLM…), claude
url = ""                   # empty: the provider's usual address (ollama http://127.0.0.1:11434, llama.cpp
                           #   http://127.0.0.1:8080), used by the session only; https, or http to this machine
model = ""                 # empty: asked of the provider at the first question and kept for the session:
                           #   the newest Claude Opus it lists, ollama's or an OpenAI-like server's first
                           #   model, the llama.cpp server's own
                           # keys are in the keyring, never here: Settings → AI and agents, or
                           #   hocus msg ai-key claude < key.txt

[search]                   # documents found from the command field, under "Documents" (#45; Settings → Search)
documents = true           # false: nothing is read, and the index kept is deleted
folders = []               # e.g. ["~/Documents", "/srv/notes"]; empty: the documents, desktop and download
                           #   folders (XDG user dirs), in the session only: nested or in tests, only the
                           #   folders named here are searched
screenshots = true         # the screenshots folder's pictures too, their text read by [screenshots] ocr
exclude = []               # names left out, as globs: ["*.log", "drafts"]
meaning = "local"          # search by meaning too, with vectors (embeddings) from the [ai] provider when it
                           #   is on and can make them (ollama, llama.cpp's server, OpenAI-like; not
                           #   Claude): "local" only from a model on this machine, "on" wherever [ai]
                           #   points (the start of every document's text is sent there), "off" words only
embedding_model = ""       # empty: the first model the provider lists with "embed" in its name
                           #   (nomic-embed-text, mxbai-embed-large…); none: words only
                           # never read, whatever this says: ~/.ssh, ~/.gnupg, keyrings, password stores
                           #   (pass, KeePass .kdbx, Bitwarden, 1Password), browser and mail profiles,
                           #   ~/.aws, ~/.kube, ~/.config/gcloud, hidden folders named for secrets, .env*,
                           #   id_*, *.pem, *.key, *.p12, credentials*, secrets*; links are not followed

[dock]                     # Super+D; shown on an empty desktop or after resting at the bottom edge
enabled = true             # false: neither the dock nor its hint
pinned = [                 # desktop entries, in order; those not installed are left out
  "org.codeberg.dnkl.foot.desktop", "firefox.desktop", "firefox-esr.desktop",
  "org.gnome.Nautilus.desktop", "thunderbird.desktop",
]

[panel]                    # the widget panel's form (Settings → Appearance), applied at once
form = "strip"             # "strip": 72 px on the right (the time, the next meeting, a panda per active
                           #   agent, the tasks and notifications counted, the account); resting the
                           #   pointer on it (150 ms), a click or Super+C slides the whole column over
                           #   the windows, leaving it or Escape slides it back
                           # "column": the whole column on the right, always there (320 px)
                           # "top": one line at the top (the active window and its buttons, the time,
                           #   the agents, the tasks); a click on it brings the column out
                           # "hidden": the whole screen to the windows; resting at the right edge
                           #   (300 ms) or Super+C brings the column out; toasts still show
screens = "primary"        # "primary": on the primary screen only
                           # "all": each screen its own strip, bar or hint, its windows leaving
                           #   room for it; the column comes out on the screen it is asked on (the
                           #   strip or edge rested on or clicked, the bar clicked, the screen under
                           #   the pointer for Super+C); each top bar shows its screen's active
                           #   window, and its buttons act on it. The "column" form stays on the
                           #   primary screen. Offered on the Appearance page with two screens or more

[pandas]                   # they show what happens; they move a moment when it changes, then hold still
enabled = true             # false: no panda anywhere (also from quick settings, for the session)

[agents]                   # coding agents in the column, from herdr; one waiting asks in a toast (Settings → AI and agents)
enabled = true
herdr_socket = ""          # one herdr whose agents belong to no desktop (shown wherever you are); empty,
                           #   and no [[agents.herdr]]: $XDG_CONFIG_HOME/herdr/herdr.sock, looked for by
                           #   the session only
journal_text = false       # true: the agents' journal keeps what MCP agents type; otherwise how many characters
show_before_click_ms = 600 # an MCP agent's ring and intent show this long before its click lands; 0: at once
                           #   (these three also in Settings → AI and agents)
confirm_timeout_s = 60     # a sensitive action waits this long for you to confirm it (5 to 300), then is refused
confirm_apps = [           # app ids (globs) an MCP agent acts on only once you confirm; typing into a
  "hocus-settings",        #   password field waits too (docs/mcp.md); [] for none
  "org.keepassxc.KeePassXC", "com.bitwarden.desktop", "Bitwarden", "1Password",
  "org.gnome.seahorse.Application", "org.gnome.World.Secrets", "org.kde.kwalletmanager*",
]

[[agents.herdr]]           # a project's herdr: its agents in full on its desktop, in a line elsewhere,
                           #   and its desktop's badge says when one waits or is done
socket = "~/.config/herdr/website.sock"
desktop = 3                # from 1; left out: wherever you are, as herdr_socket's
output = "DP-1"            # that desktop's screen; the primary one when left out

[[agents.mcp]]             # an AI agent using the desktop through `hocus mcp --agent NAME` (docs/mcp.md)
name = "assistant"         # what it names itself; a name not listed here is refused
access = "read"            # read: windows, the shell's interface, captures; act: also clicks, typing, scrolls
apps = ["foot", "org.mozilla.*"]  # app ids (globs) whose windows it may act on; ["*"] for all
shell = false              # true: it may also click the shell's own elements (never the lock nor a password dialog)

[language]
interface = "auto"         # en or fr; auto follows the session's locale (LANG); changed at once

[clock]                    # the column's, the lock screen's and the agenda's
format = "24h"             # or 12h (2:37 PM); the time zone is the system's: the settings' Language
                           #   and region page sets it through systemd-timedated (polkit asks), and the
                           #   clock follows it, and /etc/localtime, as they change

[hot_corners]              # the pointer resting in a screen's corner; one against another screen does nothing
delay_ms = 150             # so that passing by does nothing
top_left = "none"          # overview, launcher, show-desktop or none
top_right = "none"
bottom_left = "none"
bottom_right = "none"

[hot_corners.outputs."HDMI-A-1"]   # one screen's own, the others' otherwise
top_left = "launcher"

[circle]                   # circle to ask: a left drag with these keys draws a loop around anything;
enabled = true             #   its text is read (see [screenshots] ocr), then copied, searched, noted,
modifiers = "Super+Shift"  #   or explained, translated or asked about while [ai] is on. Super, Ctrl,
                           #   Alt, Shift; "Super" alone takes the place of drawing on the screen

[radial]                   # the window menu at the pointer: this button with these keys over a window
enabled = true             #   opens its actions around the pointer (a flick picks one at once)
modifiers = "Super"        #   Super, Ctrl, Alt, Shift
button = "right"           #   right or middle; Super with the left one draws on the screen

[gestures]                 # Super alone and the touchpad; overview, launcher, show-desktop or none
super_alone = "overview"   # Super pressed and let go with nothing else between
three_sideways = true      # three fingers sideways switch desktops
three_up = "overview"
three_down = "none"
four_up = "launcher"
four_down = "none"

[[rules]]                  # windows matching app_id and title (globs: * and ?)
app_id = "chat*"
output = "eDP-1"           # opens there, when plugged
desktop = 1                # on that desktop (1 to 8)
size = [900, 700]
maximized = false
opacity = 0.95             # 0.1 to 1
pinned = true              # on every desktop of its output
on_top = false             # above the other windows
blur = true                # what is behind it blurred, for a see-through window (with opacity), where panels blur

[[outputs]]                # a screen, by name (`hocus msg screens`)
name = "eDP-1"
enabled = true             # false: off; its desktops and windows wait for it
mode = "2560x1600@60"      # WIDTHxHEIGHT[@HZ]; the screen's preferred one otherwise
scale = 1.5                # 0.5 to 4, rounded to 1/120; the one of its density otherwise (see Screens)
transform = "normal"       # 90, 180, 270, flipped, flipped-90, flipped-180, flipped-270
position = [0, 0]          # logical, top-left corner
vrr = false                # adaptive sync, where the screen and GPU can
primary = true             # where unplugged screens' desktops and windows go
desktops = 1               # its own desktops, 1 to 8 (desktops.per_output otherwise)
desktop_names = ["Mail", "Code"]  # its desktops' names, in order (at most 8, 32 characters each); one
                           #   empty or left out goes by its number. Shown in the column, the
                           #   switcher's strip, Alt+Tab, the overview, and to bars (ext-workspace)
mirror = "eDP-1"           # shows that screen's picture, scaled to fit; its own desktops wait

[[layouts]]                # how a set of screens is placed (the Screens page writes them)
name = "Home"
outputs = ["eDP-1", "DP-1"]           # in effect when exactly these are connected
positions = { DP-1 = [0, 0], eDP-1 = [320, 1440] }

[shortcuts]                # the Keyboard page's editor writes them
"Super+Return" = "spawn foot"
"Ctrl+Alt+Right" = "next-desktop all"
"Alt+F4" = "none"            # removes a default
```

## Document search

`[search]` (#45) indexes the documents of its folders on two threads of
their own, never the compositor's loop: text and Markdown, HTML, PDF (its
text layer), OpenDocument and Word, Excel and PowerPoint files, and the
screenshots folder's pictures through `[screenshots] ocr`. The indexer runs
at the lowest priority (nice 19, which the I/O scheduler follows), in small
batches with pauses between them, waits a minute after the session starts,
waits while the laptop is on battery and goes on once on the charger, and
looks at the folders again every half hour, reading only what changed (by
date and size); `hocus msg search-again` (or Search again now in the
settings) looks at once. Hidden files and folders are not searched; what
is never read (see above) is not even opened, and symbolic links are
never followed. The index (words, the start of each text for the line shown
under a result, vectors) is kept in `$XDG_STATE_HOME/hocuspanda/search/`,
readable by the user only; `documents = false` deletes it. `hocus msg
search` says what the indexer does.

Search by meaning sends the start of every document's text (a few
thousand characters each) to the `[ai]` provider to get its vectors, and
what is typed in the command field once typing stops. With `meaning =
"local"` (the default) that happens only when the provider is on this
machine; `meaning = "on"` lets a provider elsewhere (OpenAI…) read them.
Claude makes no vectors: with it, documents are searched by their words.

The settings' Search page edits `[search]` and `[launcher]`: documents on
or off, the folders (typed, from `/` or `~/`; one inside a place never read
is refused and the page says which place), the screenshots' text, the
names left out, search by meaning (saying plainly, when it may go
anywhere, that the start of every document and what is typed leave the
machine) and its model, and the command field's other results with the
web search's address. It shows what the indexer does: documents counted,
paused on battery, when it last looked, with Search again now.

## Window rules

Each `[[rules]]` applies to the windows whose `app_id` and `title` match its
patterns (both optional); when several match, later rules override earlier
ones, field by field. Super+wheel over a window changes its opacity, and the
new opacity is remembered for that application. The settings' Windows page
lists the rules, adds one for an app, turns what it sets (maximized, on every
desktop, above the others, blur) on or off, and removes it; the rest of a
rule is written in the file.

## Screens

Each `[[outputs]]` sets one screen; what it does not set goes back to how the
screen was lit (its preferred mode, the scale of its density, no rotation, to
the right of the others). Changes apply at once and move nothing else: each window keeps its
place on its screen, maximized and fullscreen ones fill it again, and a
layout seen before puts every window back where it was. When a connected
screen comes to offer other modes (a monitor behind a dock or a switch, a
virtual machine's window resized), it takes the one it now prefers, unless
the mode chosen for it is still offered. Screen tools
(wlr-randr, kanshi, wdisplays) can change the same for the running session
through wlr-output-management; the file wins again once it changes. Turning
a screen's light off and on (output-power, as swayidle does) keeps it in the
layout.

A screen with no `scale` set takes the one of its pixel density, from its
physical size (EDID) and the mode it is lit with: a unit of scale per 112
dpi along its diagonal, to the nearest quarter, from 1 to 3. Scale 1 is
drawn for about 96 dpi, but desk screens of 92 to 110 dpi read well at it;
a scale is taken once a screen is clearly denser. A screen that gives no
size, or one that cannot be its own, stays at 1: under 8 inches across (a
TV or projector giving its aspect ratio), of another shape than its pixels,
or denser than 600 dpi. Virtual machines stay at 1 too: QEMU's virtual GPU
gives a size at 100 dpi whatever its window. For example:

| Screen | Pixels | Size | Density | Scale |
|---|---|---|---|---|
| 24-inch | 1920x1080 | 530x300 mm | 92 dpi | 1 |
| 27-inch | 2560x1440 | 597x336 mm | 109 dpi | 1 |
| 32-inch 4K | 3840x2160 | 708x398 mm | 137 dpi | 1.25 |
| 15.6-inch laptop | 1920x1080 | 344x194 mm | 141 dpi | 1.25 |
| 27-inch 4K | 3840x2160 | 597x336 mm | 163 dpi | 1.5 |
| 13-inch laptop | 1920x1200 | 286x179 mm | 170 dpi | 1.5 |
| 16-inch laptop | 2560x1600 | 340x210 mm | 192 dpi | 1.75 |
| 27-inch 5K | 5120x2880 | 597x336 mm | 218 dpi | 2 |
| 14-inch laptop | 2880x1800 | 301x188 mm | 243 dpi | 2.25 |
| 15.6-inch 4K laptop | 3840x2160 | 344x194 mm | 282 dpi | 2.5 |

The scale `scale` sets wins; without it again, the screen goes back to the
one of its density. The Screens page shows it as Automatic (1.75) beside
the scales to choose; a scale chosen stays chosen, even the same as the
automatic one, and Automatic takes `scale` out. Another screen plugged
where one was takes its own.

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
| `notes` | notes in the launcher's card: what is typed is added; a line typed as a to-do (`- [ ] …`) goes to the tasks instead; Delete removes the line chosen. Kept in `~/.local/share/hocuspanda/notes.md`, plain Markdown, whose to-dos move to the tasks; the command field finds notes |
| `focus-mode` | everything but the focused window dims; again, it lifts |
| `night-light` | night light on or off, until the schedule changes |
| `zoom-in`, `zoom-out` | the magnifier, around the pointer, up to 8×; back to 1 it is off (also Super+Alt+wheel) |
| `record` | records the screen: a region drawn, a window clicked, or Enter for the screen under the pointer; after a countdown, a red frame marks it and a bar shows the time, pause and stop; again to stop. Saved as MP4, then a toast offers to open or delete it |
| `screenshot MODE [SECONDS]` | `screen` (under the pointer), `all` (every screen, each at its own pixel density), `window` (the one clicked next), `region` (drawn with the pointer, its size shown; Escape cancels), after a delay of up to 60 s: copied and saved, then a toast offers to open, delete or copy its text |
| `suspend`, `hibernate` | sleeps now, locked first; hibernating only where the machine can |
| `dropdown` | shows the drop-down terminal on the screen under the pointer, starting it the first time, or hides it (it keeps running) |
| `switch-windows`, `switch-windows all` | the window switcher (#39) |
| `dock` | shows the dock, or hides it again |
| `panel` | slides the whole column out over the windows, or back, when the panel is a strip, a top bar or hidden (`[panel] form`), on the screen under the pointer with `[panel] screens = "all"`; nothing when it is the column |
| `overview` | every screen's windows in a grid, or closes it |
| `show-desktop` | the desktop of the screen under the pointer, its windows out of the way, or back |
| `welcome` | the first session's welcome again: language, keyboard, theme, and where to go next |
| `about` | Hocuspanda's version, licences and credits |
| `stop-agents` | stops every MCP agent at once: their requests are refused, reading included, until the same key (or the column's button) allows them again; works even while a window holds the shortcuts back or a panel is open; not kept across sessions ([mcp](mcp.md)) |
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
| Super+D | `dock` |
| Super+C | `panel` |
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
| Super+Shift+Escape | `stop-agents` |
| Ctrl+Alt+BackSpace | `quit` |

With the pointer, besides: a drag on a window's edge (the 8 px band around
it, where the cursor shows the resize) resizes it, from its top edge moves
it; Alt+drag moves a window and Alt+right drag resizes it, Super+drag draws on the screen, `[circle] modifiers` (Super+
Shift) with a drag circles something to ask about it, and `[radial]`
(Super and the right button) over a window opens its radial menu: held, a
flick or a slow move picks an action on release; clicked, it stays for a
click or the arrows, Enter and Escape. Its Close is never flicked: rest on
it 300 ms, or click it.
