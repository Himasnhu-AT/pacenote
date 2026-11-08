"""Bakes the night sky into small raw images the sky shader reads (`src/art/sky.rs`, `clouds.wesl`):

- `assets/sky/stars.bin`: the Yale Bright Star Catalogue (5th ed., ~9,100 naked-eye stars) as a
  cube-face cell lookup in equatorial coordinates (J2000). Faces stacked vertically (N x 6N,
  face order -X +X -Y +Y -Z +Z, the shader's), RGBA8 per cell: R, G = the star's position in
  the cell, B = brightness code (0 = no star; magnitude = 6.5 - B / 255 * 8), A = B-V colour
  code ((B-V + 0.4) / 2.4 * 255). The brighter star wins when two share a cell.
- `assets/sky/milkyway.bin`: the Milky Way's glow, R8, galactic equirectangular (u = longitude
  -180..180 with the centre in the middle, v = latitude +90 at the top). A model (bulge, Cygnus
  and Carina clouds, the Great Rift, mottling noise), not a photograph.
- `assets/sky/moon.bin`: NASA's LRO colour mosaic (CGI Moon Kit, public domain), RGBA8 sRGB,
  equirectangular, longitude -180..180 with the near side (0) in the middle.

Each file: b"PSKY", u32 width, u32 height, then the pixels. Standard library + ImageMagick only:
    python3 -I tools/art/bake_sky.py [assets_src] [assets]
"""
import gzip
import math
import os
import random
import struct
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "assets_src")
OUT = os.path.join(sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, "assets"), "sky")
N = 256  # cells per cube face edge (~0.35 deg)
MAG_LIMIT = 6.5


def write(name, w, h, data):
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, name), "wb") as f:
        f.write(b"PSKY" + struct.pack("<II", w, h) + bytes(data))
    print(f"{name}: {w} x {h}, {len(data) / 1024:.0f} KB")


def cube_cell(v):
    """Face, cell (x, y) and position in the cell for a unit vector (the shader's mapping)."""
    x, y, z = v
    ax, ay, az = abs(x), abs(y), abs(z)
    if ax >= ay and ax >= az:
        uv, face = (y / ax, z / ax), (1 if x > 0 else 0)
    elif ay >= az:
        uv, face = (x / ay, z / ay), (3 if y > 0 else 2)
    else:
        uv, face = (x / az, y / az), (5 if z > 0 else 4)
    gx, gy = (uv[0] * 0.5 + 0.5) * N, (uv[1] * 0.5 + 0.5) * N
    cx, cy = min(int(gx), N - 1), min(int(gy), N - 1)
    return face, cx, cy, gx - cx, gy - cy


def stars():
    path = os.path.join(SRC, "night", "bsc5", "heasarc_bsc5p.tdat.gz")
    lines = gzip.open(path, "rt", encoding="latin-1").read().splitlines()
    cols = next(l for l in lines if l.startswith("line[1]")).split("=", 1)[1].split()
    ix = {k: cols.index(k) for k in ("ra", "dec", "vmag", "bv_color")}
    data = lines[lines.index("<DATA>") + 1:]
    best = {}
    n = 0
    for row in data:
        f = row.split("|")
        if len(f) < len(cols):
            continue
        try:
            ra, dec, mag = float(f[ix["ra"]]), float(f[ix["dec"]]), float(f[ix["vmag"]])
        except ValueError:
            continue
        if mag > MAG_LIMIT:
            continue
        try:
            bv = float(f[ix["bv_color"]])
        except ValueError:
            bv = 0.6
        a, d = math.radians(ra), math.radians(dec)
        v = (math.cos(d) * math.cos(a), math.cos(d) * math.sin(a), math.sin(d))
        face, cx, cy, fx, fy = cube_cell(v)
        key = (face, cx, cy)
        if key not in best or best[key][0] > mag:
            best[key] = (mag, fx, fy, bv)
        n += 1
    px = bytearray(N * 6 * N * 4)
    for (face, cx, cy), (mag, fx, fy, bv) in best.items():
        i = ((face * N + cy) * N + cx) * 4
        # Keep the star off the cell's edge (its glow would be cut by the neighbour cell).
        px[i] = round(min(max(fx, 0.2), 0.8) * 255)
        px[i + 1] = round(min(max(fy, 0.2), 0.8) * 255)
        px[i + 2] = max(1, min(255, round((MAG_LIMIT - mag) / 8.0 * 255)))
        px[i + 3] = max(0, min(255, round((bv + 0.4) / 2.4 * 255)))
    print(f"stars: {n} to magnitude {MAG_LIMIT}, {len(best)} cells ({n - len(best)} lost to shared cells)")
    write("stars.bin", N, 6 * N, px)


def value_noise(seed, w, h, cells_x, cells_y):
    rnd = random.Random(seed)
    g = [[rnd.random() for _ in range(cells_x + 1)] for _ in range(cells_y + 1)]
    for row in g:
        row[-1] = row[0]  # wraps in longitude
    out = []
    for y in range(h):
        fy = y / h * cells_y
        y0 = int(fy)
        ty = fy - y0
        ty = ty * ty * (3 - 2 * ty)
        r = []
        for x in range(w):
            fx = x / w * cells_x
            x0 = int(fx)
            tx = fx - x0
            tx = tx * tx * (3 - 2 * tx)
            a = g[y0][x0] + (g[y0][x0 + 1] - g[y0][x0]) * tx
            b = g[y0 + 1][x0] + (g[y0 + 1][x0 + 1] - g[y0 + 1][x0]) * tx
            r.append(a + (b - a) * ty)
        out.append(r)
    return out


def milkyway():
    w, h = 1024, 512
    octaves = [value_noise(11 + k, w, h, 16 * 2**k, 8 * 2**k) for k in range(5)]
    wobble = value_noise(97, w, h, 24, 12)
    px = bytearray(w * h)
    for y in range(h):
        b = 90.0 - (y + 0.5) / h * 180.0
        for x in range(w):
            l = (x + 0.5) / w * 360.0 - 180.0
            lo = sum(o[y][x] * 0.5**k for k, o in enumerate(octaves[:2])) / 1.5
            hi = sum(o[y][x] * 0.5**k for k, o in enumerate(octaves[2:])) / 1.75
            # Thicker and brighter towards the galactic centre (the bulge in Sagittarius).
            sigma = 6.0 + 8.0 * math.exp(-(l / 40.0) ** 2)
            bright = 0.3 + 0.7 * math.exp(-(l / 30.0) ** 2) + 0.3 * math.exp(-((l - 78.0) / 14.0) ** 2) + 0.2 * math.exp(-((l + 73.0) / 18.0) ** 2)
            glow = bright * math.exp(-((b + 1.0 + 3.0 * (lo - 0.5)) / sigma) ** 2)
            # The Great Rift: a wavering dust lane north of the plane, Cygnus to Sagittarius.
            along = math.exp(-((l - 22.0) / 38.0) ** 2)
            lane = 2.0 + 5.0 * (wobble[y][x] - 0.5)
            glow *= 1.0 - 0.8 * along * math.exp(-((b - lane) / (1.6 + 2.0 * hi)) ** 2)
            # The Coalsack by the Southern Cross.
            glow *= 1.0 - 0.7 * math.exp(-(((l + 57.0) / 3.5) ** 2 + ((b + 1.0) / 2.5) ** 2))
            # Star clouds and dark lanes.
            glow *= 0.2 + 1.6 * (0.4 * lo + 0.6 * hi) ** 2.2 * 1.8
            px[y * w + x] = max(0, min(255, round(glow * 230)))
    write("milkyway.bin", w, h, px)


def moon():
    src = os.path.join(SRC, "night", "moon", "lroc_color_poles_2k.tif")
    raw = subprocess.run(["magick", src, "-resize", "512x256!", "-depth", "8", "rgba:-"], check=True, capture_output=True).stdout
    write("moon.bin", 512, 256, raw)


if __name__ == "__main__":
    stars()
    milkyway()
    moon()
