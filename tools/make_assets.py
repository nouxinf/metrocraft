#!/usr/bin/env python3
"""Generate the base-map assets embedded in Transit Atlas.

Uses Natural Earth 1:110m land and lakes (public domain):
https://www.naturalearthdata.com/

Outputs:
  assets/coastlines.bin  quantized lon/lat polylines (i16 hundredths of a degree)
  assets/land.png        2048x1024 alpha mask: white = land, transparent = water

Run from the project root:  python3 tools/make_assets.py
"""

from __future__ import annotations

import io
import json
import os
import struct
import urllib.request

from PIL import Image, ImageDraw

LAND_URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_land.geojson"
LAKES_URL = "https://raw.githubusercontent.com/nvkelso/natural-earth-vector/master/geojson/ne_110m_lakes.geojson"

WIDTH, HEIGHT = 2048, 1024
SUPERSAMPLE = 2


def fetch(url: str) -> dict:
    req = urllib.request.Request(url, headers={"User-Agent": "transit-atlas-assets"})
    with urllib.request.urlopen(req, timeout=60) as response:
        return json.load(response)


def polygons(geojson: dict):
    """Yield (outer_ring, [inner_rings...]) for every polygon in a GeoJSON file."""
    for feature in geojson["features"]:
        geometry = feature.get("geometry")
        if not geometry:
            continue
        if geometry["type"] == "Polygon":
            groups = [geometry["coordinates"]]
        elif geometry["type"] == "MultiPolygon":
            groups = geometry["coordinates"]
        else:
            continue
        for group in groups:
            if not group:
                continue
            yield group[0], group[1:]


def quantize(ring):
    out = bytearray()
    for lon, lat in ring:
        x = max(-18000, min(18000, round(lon * 100)))
        y = max(-9000, min(9000, round(lat * 100)))
        out += struct.pack("<hh", int(x), int(y))
    return bytes(out)


def project(ring, scale=1):
    return [
        ((lon + 180.0) / 360.0 * WIDTH * scale, (90.0 - lat) / 180.0 * HEIGHT * scale)
        for lon, lat in ring
    ]


def main() -> None:
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    assets = os.path.join(root, "assets")
    os.makedirs(assets, exist_ok=True)

    land = fetch(LAND_URL)
    lakes = fetch(LAKES_URL)

    land_polys = list(polygons(land))
    lake_polys = list(polygons(lakes))

    # ---------------------------------------------------------------- coastlines
    rings = []
    for outer, holes in land_polys:
        rings.append(outer)
        rings.extend(holes)
    for outer, holes in lake_polys:
        rings.append(outer)
        rings.extend(holes)

    rings = [ring for ring in rings if len(ring) >= 3]
    with open(os.path.join(assets, "coastlines.bin"), "wb") as f:
        f.write(b"TATLS001")
        f.write(struct.pack("<I", len(rings)))
        for ring in rings:
            f.write(struct.pack("<I", len(ring)))
            f.write(quantize(ring))
    print(f"coastlines.bin: {len(rings)} rings, {sum(len(r) for r in rings)} points")

    # ------------------------------------------------------------------ land mask
    scale = SUPERSAMPLE
    mask = Image.new("L", (WIDTH * scale, HEIGHT * scale), 0)
    draw = ImageDraw.Draw(mask)
    for outer, holes in land_polys:
        draw.polygon(project(outer, scale), fill=255)
        for hole in holes:
            draw.polygon(project(hole, scale), fill=0)
    for outer, holes in lake_polys:
        draw.polygon(project(outer, scale), fill=0)
    mask = mask.resize((WIDTH, HEIGHT), Image.LANCZOS)

    rgba = Image.new("RGBA", (WIDTH, HEIGHT), (255, 255, 255, 0))
    rgba.putalpha(mask)
    rgba.save(os.path.join(assets, "land.png"), optimize=True)
    print(f"land.png: {os.path.getsize(os.path.join(assets, 'land.png'))} bytes")


if __name__ == "__main__":
    main()
