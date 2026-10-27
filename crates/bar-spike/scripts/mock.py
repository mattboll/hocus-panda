# Renders the canvas' Bar component (Bar.dc.html) as a static page for a
# given mode and halo, with local fonts and pandas, on the headless
# compositor's desktop colour, for side-by-side comparisons.
import os, re, sys
S = os.path.dirname(os.path.abspath(__file__))
A = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
mode, halo, h, out = sys.argv[1], sys.argv[2], int(sys.argv[3]), sys.argv[4]
src = open(os.environ["BAR_DC_HTML"], encoding="utf-8").read()
body = src.split("<x-dc>")[1].split("</x-dc>")[0]
body = re.sub(r"<helmet>.*?</helmet>", "", body, flags=re.S)
tint = {"agent": "245, 180, 74", "meeting": "143, 179, 255"}.get(halo)
v = dict(
    full=mode == "full", mini=mode == "mini",
    wpx="196px" if mode == "full" else "56px", hpx=f"{h}px", haloOn=bool(tint),
    border=f"rgba({tint}, 0.85)" if tint else "rgba(255, 255, 255, 0.10)",
    glow=(f"0 0 0 1px rgba({tint}, 0.35), 0 0 34px rgba({tint}, 0.45), inset 0 0 22px rgba({tint}, 0.18)"
          if tint else "0 18px 50px rgba(0, 0, 0, 0.35), inset 0 1px 0 rgba(255, 255, 255, 0.06)"),
    sweep=(f"linear-gradient(180deg, rgba({tint}, 0) 18%, rgba({tint}, 0.10) 32%, rgba({tint}, 0.30) 40%, rgba({tint}, 0.10) 48%, rgba({tint}, 0) 62%)" if tint else "none"),
    waitBg="rgba(245, 180, 74, 0.18)" if halo == "agent" else "rgba(245, 180, 74, 0.08)",
    waitBorder="rgba(245, 180, 74, 0.7)" if halo == "agent" else "rgba(245, 180, 74, 0.25)",
    meetBg="rgba(143, 179, 255, 0.18)" if halo == "meeting" else "rgba(255, 255, 255, 0.05)",
    meetBorder="rgba(143, 179, 255, 0.7)" if halo == "meeting" else "transparent",
    meetIn="dans 8 min" if halo == "meeting" else "dans 23 min",
    meetDot="0 0 10px #8FB3FF" if halo == "meeting" else "none",
)
def sc_if(m):
    key = m.group(1)
    return m.group(2) if v[key] else ""
body = re.sub(r'<sc-if value="\{\{(\w+)\}\}"[^>]*>(.*?)</sc-if>', sc_if, body, flags=re.S)
body = re.sub(r"\{\{(\w+)\}\}", lambda m: str(v[m.group(1)]), body)
blobs = {"8aadc3f8067a054ec400bf54ffcff086": "work", "2059957a0a49db5b2b06da2440758b12": "wait",
         "723291d50bdd1d1f8b7faa8fc381affc": "sleep"}
for k, n in blobs.items():
    body = body.replace(f"/_blob/{k}", f"file://{A}/panda-{n}.png")
page = f"""<!doctype html><html><head><meta charset="utf-8"><style>
@font-face {{ font-family: 'Unbounded'; src: url('file://{A}/Unbounded-wght.ttf'); font-weight: 200 900; }}
@font-face {{ font-family: 'Manrope'; src: url('file://{A}/Manrope-wght.ttf'); font-weight: 200 800; }}
html, body {{ margin: 0; background: rgb(14, 15, 18); }}
.wrap {{ position: absolute; top: 8px; right: 8px; }}
</style></head><body><div class="wrap">{body}</div></body></html>"""
open(out, "w", encoding="utf-8").write(page)
