#!/usr/bin/env python3
"""Convert raw game captures in _source-shots/*.png (1920x1080, git-ignored) into the WebP
masters the site ships from, src/assets/shots/<name>.webp. Astro then makes AVIF/WebP sizes.

  python3 -I tools/shots.py            # from web/

Captures come from the game's play.py (see README: Screenshots); name the PNG what the site
calls it (e.g. hero-cockpit.png).
"""
import os, sys
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
SRC = os.path.join(HERE, "..", "_source-shots")
OUT = os.path.join(HERE, "..", "src", "assets", "shots")
os.makedirs(OUT, exist_ok=True)
for f in sorted(os.listdir(SRC)):
    if not f.endswith(".png"):
        continue
    im = Image.open(os.path.join(SRC, f)).convert("RGB")
    dst = os.path.join(OUT, f[:-4] + ".webp")
    im.save(dst, "WEBP", quality=84, method=6)
    print(f"{f} -> {os.path.relpath(dst)} ({os.path.getsize(dst) // 1024} KB)")
