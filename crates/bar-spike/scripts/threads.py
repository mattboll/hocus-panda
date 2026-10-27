# Per-thread CPU of the bar while its halo waves 10 times.
import os, subprocess, sys, time, glob
D = sys.argv[1]
V = os.environ["V1_BIN"] + "/hocus"
SOCK = D + "/run/hocus-0.api"
def msg(*a): subprocess.run([V, "msg", "--socket", SOCK, *map(str, a)], capture_output=True)
pid = open(D + "/bar.pid").read().strip()
def snap():
    r = {}
    for t in glob.glob(f"/proc/{pid}/task/*"):
        try:
            name = open(t + "/comm").read().strip()
            f = open(t + "/stat").read().rsplit(")", 1)[1].split()
            r[(t.rsplit("/", 1)[1], name)] = (int(f[11]) + int(f[12])) * 10
        except OSError:
            pass
    return r
msg("pointer-move", int(sys.argv[3]) if len(sys.argv) > 3 else 1800, 40); time.sleep(0.05); msg("click"); time.sleep(0.4)
mode = sys.argv[2] if len(sys.argv) > 2 else "wave"
if mode == "wave":
    msg("key", 30); time.sleep(0.7)
a = snap(); t0 = time.monotonic()
for _ in range(10):
    msg("key", 19 if mode == "wave" else 48); time.sleep(0.6 if mode == "wave" else 0.3)
b = snap(); dt = time.monotonic() - t0
tot = 0
for k, v in sorted(b.items(), key=lambda kv: -(kv[1] - a.get(kv[0], 0))):
    d = v - a.get(k, 0); tot += d
    if d > 0: print(f"  {k[1]:24s} {d:6d} ms")
print(f"total {tot} ms CPU over {dt*1000:.0f} ms")
if mode == "wave":
    msg("key", 49)
