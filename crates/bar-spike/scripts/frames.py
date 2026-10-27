# Frame statistics from a bar log: draw intervals in each animation window
# (from the bar's trace), and, with WAYLAND_DEBUG, the time from each frame
# callback's `done` to the surface commit that answers it.
import re, sys, datetime
L = open(sys.argv[1], encoding="utf-8", errors="replace").read().splitlines()
draws = []
for l in L:
    m = re.match(r"\[bar\s+([\d.]+) ms\] draw #", l)
    if m:
        draws.append(float(m.group(1)))
events = []
for l in L:
    m = re.match(r"\[bar\s+([\d.]+) ms\] (halo \w+ -> \w+|form switch to \w+)", l)
    if m:
        events.append((float(m.group(1)), m.group(2)))
for t0, what in events:
    dur = 240 if "form" in what else 600
    ds = sorted(set(round(d, 1) for d in draws if t0 <= d <= t0 + dur + 30))
    iv = [round(b - a, 1) for a, b in zip(ds, ds[1:]) if b - a > 1.0]
    print(f"{what:28s} {len(ds):3d} frames in {dur} ms; interval mean {sum(iv)/len(iv):5.1f} max {max(iv):5.1f} ms")
def ts(l):
    m = re.match(r"\[(\d+):(\d+):([\d.]+)\]", l)
    return (int(m.group(1)) * 3600 + int(m.group(2)) * 60 + float(m.group(3))) * 1000 if m else None
lat, pending, frame_ids = [], None, set()
for l in L:
    m = re.search(r"wl_surface#\d+\.frame\(new id wl_callback#(\d+)\)", l)
    if m:
        frame_ids.add(m.group(1))
    m = re.search(r"wl_callback#(\d+)\.done\(", l)
    if m and m.group(1) in frame_ids:
        pending = ts(l)
    elif pending is not None and re.search(r"-> wl_surface#\d+\.commit\(\)", l):
        lat.append(ts(l) - pending)
        pending = None
if lat:
    lat.sort()
    print(f"frame callback -> commit: n={len(lat)} median {lat[len(lat)//2]:.1f} p95 {lat[int(len(lat)*0.95)]:.1f} max {lat[-1]:.1f} ms")
