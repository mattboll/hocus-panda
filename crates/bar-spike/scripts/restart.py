# Kills and restarts the bar while foot is open (maximized) and measures,
# from the restart, when the bar is on the screen again (the meeting card's
# blue bar at 1740,115 in a screenshot), and what happens to foot.
import os, subprocess, sys, time, json, signal
from PIL import Image
S = os.path.dirname(os.path.abspath(__file__))
V = os.environ["V1_BIN"] + "/hocus"
D, binary, sig, runs = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
SOCK = D + "/run/hocus-0.api"
def msg(*a): return subprocess.run([V, "msg", "--socket", SOCK, *map(str, a)], capture_output=True, text=True).stdout
def shown():
    msg("screenshot", "--output", "HEADLESS-1", "-o", D + "/probe.png")
    return Image.open(D + "/probe.png").convert("RGB").getpixel((1740, 115)) == (143, 179, 255)
def foot(): 
    w = json.loads(msg("windows"))
    return [(x["app_id"], x["state"], x["geometry"]) for x in w]
def bar_pid(): return int(open(D + "/bar.pid").read())
print("before:", foot())
for i in range(runs):
    pid = bar_pid()
    os.kill(pid, signal.SIGKILL if sig == "kill" else signal.SIGTERM)
    t = time.monotonic()
    while os.path.exists(f"/proc/{pid}") and time.monotonic() - t < 3:
        time.sleep(0.005)
    time.sleep(0.3)
    print(f"run {i}: bar {pid} gone ({sig}); foot now {foot()}; bar shown: {shown()}")
    t0 = time.monotonic()
    subprocess.run([S + "/bar.sh", D, "BAR_FAKE_TIME=14:37:46"], env={"BAR_BIN": binary, "PATH": "/usr/bin:/bin"})
    while not shown():
        if time.monotonic() - t0 > 5:
            print("  not shown after 5 s"); break
    dt = 1000 * (time.monotonic() - t0)
    time.sleep(0.3)
    first = [l for l in open(D + "/bar.log") if "first draw" in l]
    print(f"  back on screen after {dt:.0f} ms (bar's own first draw: {first[0].split(']')[0].split()[-2] if first else '?'} ms); foot {foot()}")
