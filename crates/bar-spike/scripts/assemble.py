# Builds the report's comparison images from a run directory's screenshots.
import os, sys
from PIL import Image
D, OUT = sys.argv[1], sys.argv[2]
M = os.environ.get("MOCK_DIR", "mock")
img = lambda p: Image.open(p).convert("RGB")
def pair(mock, real, out, x0, rx0, w, h=1080):
    a = img(mock).crop((x0, 0, x0 + w, h)); b = img(real).crop((rx0, 0, rx0 + w, h))
    o = Image.new("RGB", (2 * w + 20, h), (255, 255, 255)); o.paste(a, (0, 0)); o.paste(b, (w + 20, 0)); o.save(out, optimize=True)
    return o
o = pair(M + "/full.png", D + "/rest.png", OUT + "/cmp-full.png", 280, 1700, 220)
o.crop((0, 0, 460, 330)).resize((920, 660), Image.NEAREST).save(OUT + "/cmp-full-top-2x.png", optimize=True)
pair(M + "/full-agent.png", D + "/glow-agent.png", OUT + "/cmp-halo-agent.png", 240, 1660, 260)
pair(M + "/full-meeting.png", D + "/glow-meeting.png", OUT + "/cmp-halo-meeting.png", 240, 1660, 260)
pair(M + "/mini-none.png", D + "/form-mini.png", OUT + "/cmp-mini.png", 380, 1800, 120)
def strip(names, box, out):
    ims = [img(D + f"/{n}.png").crop(box) for n in names]
    w = box[2] - box[0]
    o = Image.new("RGB", (len(ims) * (w + 20) - 20, box[3] - box[1]), (255, 255, 255))
    for i, im in enumerate(ims): o.paste(im, (i * (w + 20), 0))
    o.save(out, optimize=True)
strip(["wave-120", "wave-250", "glow-agent"], (1660, 0, 1920, 1080), OUT + "/wave-strip.png")
strip(["form-90", "form-mini", "form-full"], (1660, 0, 1920, 400), OUT + "/form-strip.png")
m = img(M + "/full.png")
raw = img(os.environ.get("RAW_SHOT", D + "/shot1.png")).crop((1726, 170, 1902, 250)).resize((528, 240), Image.NEAREST)
pre = img(D + "/rest.png").crop((1726, 170, 1902, 250)).resize((528, 240), Image.NEAREST)
mk = m.crop((306, 178, 482, 258)).resize((528, 240), Image.NEAREST)
o = Image.new("RGB", (528, 740), (255, 255, 255)); o.paste(mk, (0, 0)); o.paste(raw, (0, 250)); o.paste(pre, (0, 500)); o.save(OUT + "/cmp-pandas-3x.png", optimize=True)
img(D + "/pop-aim.png").crop((1380, 680, 1920, 1080)).save(OUT + "/popover.png", optimize=True)
names = ["1-placeholder", "2-focused-empty", "3-typed", "4-selected", "5-home"]
z = [img(D + f"/tf-{n}.png").crop((1438, 985, 1712, 1030)).resize((822, 135), Image.NEAREST) for n in names]
o = Image.new("RGB", (822, 141 * len(z)), (255, 255, 255))
for i, c in enumerate(z): o.paste(c, (0, i * 141))
o.save(OUT + "/text-field-3x.png", optimize=True)
