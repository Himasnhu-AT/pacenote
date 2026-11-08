"""Cooks the ACE-Step songs (assets_src/music_ace/*.wav, tools/audio/gen_music_ace.py) like the other
music (cook_sfx.cook_music: stereo, silence trimmed, 1 s / 3 s fades, -18 LUFS, Vorbis) into
assets/sounds/music/<name>.ogg.

    python3 -I tools/audio/cook_ace.py [name ...]
"""
import glob
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from cook_sfx import cook_music  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(HERE))
SRC = os.path.join(ROOT, "assets_src", "music_ace")
OUT = os.path.join(ROOT, "assets", "sounds", "music")

want = set(sys.argv[1:])
for wav in sorted(glob.glob(os.path.join(SRC, "*.wav"))):
    name = os.path.splitext(os.path.basename(wav))[0]
    if want and name not in want:
        continue
    dst = os.path.join(OUT, f"{name}.ogg")
    cook_music(wav, dst)
    print(f"cooked {name} -> {os.path.relpath(dst, ROOT)} ({os.path.getsize(dst) // 1024} KB)")
