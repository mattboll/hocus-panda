# Scenarios

Each `.toml` file here is a scenario: steps and expectations run against its
own headless compositor (pixman renderer, virtual clock, private runtime
directory), through the control API. `cargo nextest run` runs them all, one
test per file; `hocus scenario run FILE…` runs some.

```toml
description = "A click on a window behind another focuses it and raises it"
outputs = ["2560x1600@2", "1920x1080"]   # default: one 1280x800 output

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
| `close = "app_id"` | asks the window to close, waits until it is gone |
| `pointer_move = [x, y]` | moves the pointer, global logical coordinates |
| `click = "left"` | presses and releases a button (`left`, `right`, `middle`) |
| `key = 30` | presses and releases a key, by evdev code (`KEY_A` is 30) |
| `advance_ms = 500` | moves the virtual clock forward |
| `frames = 1` | moves the virtual clock forward by whole frames |
| `expect_window = { app_id, output?, geometry? = [x, y, w, h], focused? }` | checks a window |
| `expect_no_window = "app_id"` | checks a window is gone |
| `expect_focus = "app_id"` | checks which window has keyboard focus |
| `expect_stack = ["below", "above"]` | checks all windows, bottom to top |
| `expect_received = { app_id, event = "key 30 pressed" }` | checks the client got an event: `configure WxH`, `pointer_enter`, `pointer_leave`, `button CODE pressed`, `keyboard_enter`, `keyboard_leave`, `key CODE pressed`, `close` (and `released`) |
| `expect_screenshot = { output, reference = "file.png", channel_tolerance? = 2, pixel_tolerance? = 0.0 }` | compares the output with a reference image next to the scenario |

Reference images are recorded with `HOCUS_BLESS=1 cargo nextest run -p
hocus-test --test scenarios` (or `hocus scenario run --bless FILE`); look at
them before committing. When a comparison fails, the actual, expected and diff
images are left in `target/test-artifacts/<scenario>/`, which CI keeps as an
artifact.
