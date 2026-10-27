# Drives the headless compositor through `hocus msg`, for the spike's
# popover, text field and halo measurements. Usage: drive.py RUNDIR SCENARIO
import os, subprocess, sys, time, json
V = os.environ["V1_BIN"] + "/hocus"
D = sys.argv[1]
SOCK = D + "/run/hocus-0.api"
T0 = time.monotonic()
def msg(*a):
    return subprocess.run([V, "msg", "--socket", SOCK, *map(str, a)], capture_output=True, text=True).stdout
def log(s):
    print(f"[drv {1000*(time.monotonic()-T0):9.1f} ms] {s}", flush=True)
def move(x, y): msg("pointer-move", x, y)
def click(): msg("click")
def key(*codes): msg("key", *codes)
def shot(name): msg("screenshot", "--output", "HEADLESS-1", "-o", f"{D}/{name}.png"); log(f"shot {name}")
def layers():
    # The scene dump lists no layer surfaces: look at the popover's pixels.
    from PIL import Image
    msg("screenshot", "--output", "HEADLESS-1", "-o", D + "/probe.png")
    px = Image.open(D + "/probe.png").convert("RGB").getpixel((1600, 900))
    return ["hocus-popover"] if px != (14, 15, 18) else []
def path(x0, y0, x1, y1, steps, dt):
    for i in range(1, steps + 1):
        move(x0 + (x1 - x0) * i / steps, y0 + (y1 - y0) * i / steps)
        time.sleep(dt)
GAUGE = (1810, 1015)
def open_popover():
    move(*GAUGE); time.sleep(0.05); click(); time.sleep(0.4)
    log(f"opened? layers={layers()}")
def wait_closed(limit=1.5):
    t = time.monotonic()
    while time.monotonic() - t < limit:
        if "hocus-popover" not in layers():
            return 1000 * (time.monotonic() - t)
        time.sleep(0.01)
    return None
sc = sys.argv[2]
move(900, 100); time.sleep(0.8)
if sc == "aim":
    open_popover()
    log("aim: diagonal from the gauge into the popover, 20 steps of 16 ms")
    path(*GAUGE, 1600, 800, 20, 0.016)
    time.sleep(0.6)
    log(f"after aim + 600 ms: layers={layers()}")
    shot("pop-aim")
elif sc == "escape-up":
    open_popover()
    log("escape-up: straight up, out of the cone")
    path(*GAUGE, 1810, 800, 10, 0.016)
    log("left; polling")
    log(f"closed after {wait_closed()} ms of polling from the last move")
elif sc == "return":
    open_popover()
    log("return: out of the item 150 ms, then back")
    move(1810, 975); time.sleep(0.15); move(*GAUGE); time.sleep(0.6)
    log(f"after return + 600 ms: layers={layers()}")
elif sc == "pause":
    open_popover()
    log("pause: half the way toward the popover, then still")
    path(*GAUGE, 1760, 950, 6, 0.016)
    log("still; polling")
    log(f"closed after {wait_closed()} ms of stillness")
elif sc == "window":
    open_popover()
    log("window: across the gap onto the foot window and back after 500 ms")
    move(1300, 700); time.sleep(0.5)
    log(f"after 500 ms on the window: layers={layers()}")
elif sc == "esc":
    open_popover()
    move(1560, 1008); time.sleep(0.05); click(); time.sleep(0.2)
    log("clicked the text field; typing 'abc'")
    key(30); key(48); key(46); time.sleep(0.2)
    shot("esc-before")
    key(1); time.sleep(0.4)
    log(f"Esc: layers={layers()}")
    log("Tab, to see who has the keyboard now")
    key(15); time.sleep(0.3)
    shot("esc-after")
    w = json.loads(msg("windows")); log(f"windows focus: {[(x['app_id'], x['focused']) for x in w]}")
if sc == "text":
    open_popover()
    move(1560, 1008); time.sleep(0.4)
    shot("tf-1-placeholder")
    click(); time.sleep(0.4)
    shot("tf-2-focused-empty")
    for k in (20, 30, 34, 57, 47, 11, 52, 4):   # "tag v0.3"
        key(k)
    time.sleep(0.3)
    shot("tf-3-typed")
    for _ in range(3):
        key(42, 105)                              # Shift+Left
    time.sleep(0.3)
    shot("tf-4-selected")
    key(102); time.sleep(0.3)                     # Home
    shot("tf-5-home")
    key(28); time.sleep(0.3)                      # Enter: the task is added
    shot("tf-6-added")
    log(f"popover still open: {layers()}")
    key(1); time.sleep(0.4)                       # Esc
    log(f"after Esc: {layers()}")
    key(15); time.sleep(0.3)                      # Tab: who has the keyboard?
    shot("tf-7-after-esc")
    w = json.loads(msg("windows")); log(f"windows focus: {[(x['app_id'], x['focused']) for x in w]}")
if sc == "halo":
    move(1800, 40); time.sleep(0.05); click(); time.sleep(0.5)
    log("key a: agent waiting")
    key(30); time.sleep(0.12); shot("wave-120"); time.sleep(0.1); shot("wave-250")
    time.sleep(1.2); shot("glow-agent")
    key(49); time.sleep(1.0)
    log("key m: meeting")
    key(50); time.sleep(1.2); shot("glow-meeting")
    key(49); time.sleep(1.0)
    log("key b: to mini")
    key(48); time.sleep(0.09); shot("form-90"); time.sleep(0.6); shot("form-mini")
    log("edge click: to full")
    move(1916, 500); time.sleep(0.05); click(); time.sleep(0.6); shot("form-full")
    log("key r with no halo: nothing")
if sc == "cpu":
    def cpu():
        pid = open(D + "/bar.pid").read().strip()
        f = open(f"/proc/{pid}/stat").read().rsplit(")", 1)[1].split()
        return (int(f[11]) + int(f[12])) * 10  # ms (100 Hz ticks)
    move(1800, 40); time.sleep(0.05); click(); time.sleep(0.5)
    c0, t0 = cpu(), time.monotonic(); time.sleep(10); c1, t1 = cpu(), time.monotonic()
    log(f"rest: {c1-c0} ms CPU in {1000*(t1-t0):.0f} ms")
    key(30); time.sleep(0.7)
    c0, t0 = cpu(), time.monotonic()
    for _ in range(10):
        key(19); time.sleep(0.6)
    c1, t1 = cpu(), time.monotonic()
    log(f"10 waves: {c1-c0} ms CPU in {1000*(t1-t0):.0f} ms")
    key(49); time.sleep(0.7)
    c0, t0 = cpu(), time.monotonic()
    for _ in range(10):
        key(48); time.sleep(0.3)
    c1, t1 = cpu(), time.monotonic()
    log(f"10 form switches: {c1-c0} ms CPU in {1000*(t1-t0):.0f} ms")
