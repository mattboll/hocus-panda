#!/usr/bin/env python3
"""Draws the eight pandas the Compte page offers as avatars (6.11,
docs/design/settings.md "Compte"): the same panda, the mockup's, on eight
backgrounds, 512 × 512 PNG, square (the desktop cuts them round).

    scripts/avatars/generate.py crates/hocus-settings/data/avatars

Needs Pillow. Drawn 4 times larger, then made smaller, for smooth edges.
"""

import math
import sys
from pathlib import Path

from PIL import Image, ImageDraw

SIDE = 512
BIG = SIDE * 4
UNIT = BIG / 100

# The backgrounds, from top left to bottom right: green-blue, blue, amber,
# pink, teal, grey, orange, lime.
BACKGROUNDS = [
    ("#6CC287", "#5B8DEF"),
    ("#8FB3FF", "#5B6CEF"),
    ("#F5C46E", "#F2945B"),
    ("#F49AC1", "#B57BEF"),
    ("#7EE0D0", "#3FA6A0"),
    ("#A9B2C2", "#5C6678"),
    ("#F7B98A", "#E5793F"),
    ("#B5E37E", "#6CC287"),
]
DARK = "#1b1f2a"
LIGHT = "#F4F6FA"


def rgb(hex_):
    return tuple(int(hex_[i : i + 2], 16) for i in (1, 3, 5))


def gradient(start, end):
    """A diagonal gradient, top left to bottom right, as the mockup's."""
    a, b = rgb(start), rgb(end)
    small = Image.new("RGB", (256, 256))
    px = small.load()
    for y in range(256):
        for x in range(256):
            t = (x + y) / 510
            px[x, y] = tuple(round(a[i] + (b[i] - a[i]) * t) for i in range(3))
    return small.resize((BIG, BIG), Image.BICUBIC).convert("RGBA")


def box(cx, cy, rx, ry):
    return [(cx - rx) * UNIT, (cy - ry) * UNIT, (cx + rx) * UNIT, (cy + ry) * UNIT]


def tilted(image, cx, cy, rx, ry, degrees, fill):
    """An ellipse turned by `degrees` (clockwise on screen, as SVG's)."""
    layer = Image.new("RGBA", image.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).ellipse(box(cx, cy, rx, ry), fill=fill)
    layer = layer.rotate(-degrees, center=(cx * UNIT, cy * UNIT), resample=Image.BICUBIC)
    image.alpha_composite(layer)


def panda(start, end):
    image = gradient(start, end)
    draw = ImageDraw.Draw(image)
    for cx in (28, 72):
        draw.ellipse(box(cx, 34, 12, 12), fill=DARK)
    draw.ellipse(box(50, 60, 33, 29), fill=LIGHT)
    tilted(image, 37, 57, 8, 10.5, 25, DARK)
    tilted(image, 63, 57, 8, 10.5, -25, DARK)
    draw = ImageDraw.Draw(image)
    for cx in (38.5, 61.5):
        draw.ellipse(box(cx, 55.5, 2.6, 2.6), fill="#ffffff")
    draw.ellipse(box(50, 70, 4.5, 3.2), fill=DARK)
    # The mouth: M45 76 Q50 80 55 76, 2 wide, round ends.
    points = []
    for i in range(41):
        t = i / 40
        x = (1 - t) ** 2 * 45 + 2 * (1 - t) * t * 50 + t**2 * 55
        y = (1 - t) ** 2 * 76 + 2 * (1 - t) * t * 80 + t**2 * 76
        points.append((x * UNIT, y * UNIT))
    width = 2 * UNIT
    draw.line(points, fill=DARK, width=math.ceil(width), joint="curve")
    for x, y in (points[0], points[-1]):
        draw.ellipse([x - width / 2, y - width / 2, x + width / 2, y + width / 2], fill=DARK)
    return image.resize((SIDE, SIDE), Image.LANCZOS).convert("RGB")


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for i, (start, end) in enumerate(BACKGROUNDS, start=1):
        panda(start, end).save(out / f"panda-{i}.png", optimize=True)


if __name__ == "__main__":
    main()
