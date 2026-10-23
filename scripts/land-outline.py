#!/usr/bin/env python3
"""Builds crates/hocus-settings/data/land.txt, the world map of the time
zone picker (#67).

From Natural Earth's 1:110m land polygons, which are in the public domain
(https://www.naturalearthdata.com/about/terms-of-use/):

    https://github.com/nvkelso/natural-earth-vector/blob/v5.1.2/geojson/ne_110m_land.geojson

Usage: scripts/land-outline.py ne_110m_land.geojson > crates/hocus-settings/data/land.txt

Antarctica and the far north are left out (the map shows 60° S to 84° N),
outlines are simplified (Douglas-Peucker, 0.6°) and rounded to half
degrees, and what is left smaller than a few square degrees goes. One ring
per line: longitude and latitude pairs, in half degrees, east and north
positive.
"""
import json
import sys

TOLERANCE = 0.6
SMALLEST = 3.0
SOUTH, NORTH = -60.0, 84.0


def distance(p, a, b):
    (x, y), (x1, y1), (x2, y2) = p, a, b
    dx, dy = x2 - x1, y2 - y1
    if dx == 0 and dy == 0:
        return ((x - x1) ** 2 + (y - y1) ** 2) ** 0.5
    t = max(0.0, min(1.0, ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)))
    return ((x - x1 - t * dx) ** 2 + (y - y1 - t * dy) ** 2) ** 0.5


def simplify(points, tolerance):
    if len(points) < 3:
        return points
    far, index = 0.0, 0
    for i in range(1, len(points) - 1):
        d = distance(points[i], points[0], points[-1])
        if d > far:
            far, index = d, i
    if far <= tolerance:
        return [points[0], points[-1]]
    left = simplify(points[: index + 1], tolerance)
    right = simplify(points[index:], tolerance)
    return left[:-1] + right


def area(points):
    return abs(
        sum(
            x1 * y2 - x2 * y1
            for (x1, y1), (x2, y2) in zip(points, points[1:] + points[:1])
        )
    ) / 2


def main() -> None:
    with open(sys.argv[1]) as f:
        data = json.load(f)
    rings = []
    for feature in data["features"]:
        geometry = feature["geometry"]
        polygons = (
            [geometry["coordinates"]]
            if geometry["type"] == "Polygon"
            else geometry["coordinates"]
        )
        for polygon in polygons:
            # Outer rings only: lakes are not drawn at this size.
            ring = [tuple(p) for p in polygon[0]]
            if max(lat for _, lat in ring) < SOUTH:
                continue
            ring = [(lon, max(SOUTH, min(NORTH, lat))) for lon, lat in ring]
            simple = simplify(ring, TOLERANCE)
            rounded = []
            for lon, lat in simple:
                p = (round(lon * 2), round(lat * 2))
                if not rounded or rounded[-1] != p:
                    rounded.append(p)
            if len(rounded) > 1 and rounded[0] == rounded[-1]:
                rounded.pop()
            if len(rounded) < 3 or area([(x / 2, y / 2) for x, y in rounded]) < SMALLEST:
                continue
            rings.append(rounded)
    rings.sort(key=lambda r: -area(r))
    for ring in rings:
        print(" ".join(f"{x} {y}" for x, y in ring))


if __name__ == "__main__":
    main()
