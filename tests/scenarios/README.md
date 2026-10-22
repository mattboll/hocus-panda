# Scenarios

Each `.toml` file here is a scenario: steps and expectations run against its
own headless compositor (pixman renderer, virtual clock, private runtime
directory), through the control API. `cargo nextest run` runs them all, one
test per file; `hocus scenario run FILE…` runs some.

```toml
description = "A click on a window behind another focuses it and raises it"
# WIDTHxHEIGHT[@SCALE][+X+Y][/DESKTOPS]; default: one 1280x800 output with 4 desktops
outputs = ["2560x1600@2/1", "1920x1080"]
# Starts XWayland for `open_x11`; the scenario is skipped where Xwayland is missing
xwayland = false
# Shows the widget column on the primary output (in English, at a fixed
# Wednesday 21 October 14:37 plus the virtual time, for user "Alex")
shell = false
# A private D-Bus bus for the compositor (notifications); never the session's
dbus = false
# Programs the steps run: the scenario is skipped where one is missing
needs = ["notify-send"]
# Where the launcher reads desktop entries, relative to the scenario
data_dirs = ["../fixtures"]
# What started applications inherit instead of the test's environment
# (PATH is added when missing)
child_env = { LD_PRELOAD = "/tmp/not-a-library.so" }

[[step]]
open = { app_id = "back", size = [400, 300], color = "#3399ff" }
[[step]]
pointer_move = [450, 260]
[[step]]
click = "left"
[[step]]
expect_focus = "back"
```

Steps, one per `[[step]]` table:

| Step | Does |
|---|---|
| `open = { app_id, size = [w, h], color = "#rrggbb" }` | starts a test client with a solid window, waits until it is shown |
| `open_x11 = { app_id, size = [w, h], color = "#rrggbb" }` | the same with an X11 client through XWayland (WM_CLASS is the app id) |
| `copy = { app_id, text }` | the client of that app id, Wayland or X11, takes the clipboard with `text` |
| `expect_clipboard = { app_id, text }` | the client of that app id reads `text` from the clipboard (a Wayland client is offered it when it gets the keyboard) |
| `type = "fir"` | types text as on a US keyboard |
| `expect_runtime_file = { name, contains?, lacks? }` | a file a started program wrote in the compositor's runtime directory, with or without these strings |
| `notify_send = ["--app-name=Mail", "Title", "Body"]` | runs `notify-send` on the private bus |
| `notify = { app, summary, body?, actions? = [["key", "Label"]], urgency?, timeout_ms?, replaces? }` | sends a notification on the private bus (urgency `low`, `normal`, `critical`; `timeout_ms = 0` never expires) |
| `click_ui = "Close"` | clicks the shell's button with this label or accessible name (`hocus msg ui` lists them) |
| `expect_ui = { label, disabled?, selected?, shown?, inside? }` | the shell shows an element with this label (disabled or not, the current choice of a list or not, within `inside = [x, y, w, h]`); `shown = false` checks it does not |
| `close = "app_id"` | asks the window to close, waits until it is gone |
| `pointer_move = [x, y]` | moves the pointer, global logical coordinates |
| `click = "left"` | presses and releases a button (`left`, `right`, `middle`) |
| `press = "left"`, `release = "left"` | holds and lets go a button, for drags |
| `scroll = 15` | scrolls the wheel, logical pixels (15 per notch, down positive) |
| `key_down = "alt"`, `key_up = "alt"` | holds and lets go a key, for Alt+drag |
| `key = 30` | presses and releases a key, by evdev code (`KEY_A` is 30) |
| `keys = "super+shift+2"` | presses keys in order, releases them in reverse: `super`, `shift`, `ctrl`, `alt`, digits, letters (by position on a US keyboard), `enter`, `escape`, `tab`, `space`, `backspace`, arrows, or evdev codes |
| `send = { app_id, output, desktop? }` | sends a window to an output: its desktop `desktop` (from 1), or the shown one |
| `plug = { spec = "1920x1080+1280+0", name? }` | plugs an output; `name` brings back one unplugged before |
| `unplug = "HEADLESS-2"` | unplugs an output |
| `set_state = { app_id, state = "maximized" }` | maximizes, makes fullscreen, minimizes or restores (`normal`) a window |
| `config = """…"""` | writes the configuration file (TOML) and waits until the compositor has read it |
| `expect_config_error = "line 2"` | checks the last file was refused with that in the message; `""` checks it was applied |
| `expect_config_warning = "first one wins"` | checks what the configuration in effect left out |
| `expect_event = { type = "keyboard_layout", has? = "French" }` | checks the compositor said it in its event stream since the start |
| `advance_ms = 500` | moves the virtual clock forward |
| `frames = 1` | moves the virtual clock forward by whole frames |
| `expect_window = { app_id, output?, geometry? = [x, y, w, h], focused?, visible?, state?, opacity?, pinned? }` | checks a window, waiting a moment for its client to resize; `visible` is whether its desktop is shown and it is not minimized |
| `expect_no_window = "app_id"` | checks a window is gone |
| `expect_focus = "app_id"` | checks which window has keyboard focus |
| `expect_stack = ["below", "above"]` | checks the shown windows, bottom to top |
| `expect_shown = { output, desktop }` | checks which desktop an output shows, from 1 |
| `expect_received = { app_id, event = "key 30 pressed" }` | checks the client got an event: `configure WxH`, `pointer_enter`, `pointer_leave`, `button CODE pressed`, `keyboard_enter`, `keyboard_leave`, `key CODE pressed`, `close` (and `released`) |
| `expect_not_received = { app_id, event }` | checks the client never got an event |
| `expect_pixel = { output, at = [x, y], color = "#rrggbb", tolerance? = 24, is? = true }` | checks one pixel of an output, in its pixels; `is = false` checks it is not that colour |
| `expect_screenshot = { output, reference = "file.png", channel_tolerance? = 2, pixel_tolerance? = 0.0, cursor? = false }` | compares the output with a reference image next to the scenario; `cursor` draws the pointer's cursor too |

Reference images are recorded with `HOCUS_BLESS=1 cargo nextest run -p
hocus-test --test scenarios` (or `hocus scenario run --bless FILE`); look at
them before committing. When a comparison fails, the actual, expected and diff
images are left in `target/test-artifacts/<scenario>/`, which CI keeps as an
artifact.
