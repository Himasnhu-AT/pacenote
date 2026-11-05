#!/usr/bin/env python3
"""Cook downloaded sound effects: assets_src/audio/openverse/<pack>/* -> assets/sounds/sfx/<pack>_<n>.ogg (music: assets/sounds/music/).

Music packs (`"music": true`) stay stereo and full length (-18 LUFS, 1 s / 3 s fades).
Every other file becomes mono, 48 kHz Ogg Vorbis (like the car sounds), leading/trailing silence trimmed,
cut to the pack's max length (tools/audio/sfx_wishlist.json `max_s`, else 30 s) with a short fade,
ambience beds crossfaded into seamless loops, and loudness-normalised (EBU R128, -20 LUFS one-shots, -26 LUFS ambience beds: the game sets the
mix level). Files whose licence isn't CC0 / PDM / CC BY are refused (assets_src/SOURCES.md).

It also writes:
- assets/sounds/sfx/index.json: per pack, the cooked files with author, licence and source, so the
  game (and anyone) can see where each sound came from;
- the "Sound effects" block in assets/credits.txt (between the SFX markers): CC BY lines are
  required, CC0 ones are a courtesy;
- the "audio/openverse" section of assets_src/SOURCES.md (between the same markers).

Needs ffmpeg and oggenc (vorbis-tools). Run from anywhere:
    python3 -I tools/audio/cook_sfx.py            # cook every pack (skips up-to-date files)
    python3 -I tools/audio/cook_sfx.py --force    # re-cook everything
"""
import argparse
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, 'assets_src', 'audio', 'openverse')
OUT = os.path.join(ROOT, 'assets', 'sounds', 'sfx')
OUT_MUSIC = os.path.join(ROOT, 'assets', 'sounds', 'music')
WISHLIST = os.path.join(ROOT, 'tools', 'audio', 'sfx_wishlist.json')
CREDITS = os.path.join(ROOT, 'assets', 'credits.txt')
SOURCES = os.path.join(ROOT, 'assets_src', 'SOURCES.md')
BEGIN, END = '<!-- SFX BEGIN (tools/audio/cook_sfx.py) -->', '<!-- SFX END -->'
TXT_BEGIN, TXT_END = '# SFX BEGIN (tools/audio/cook_sfx.py)', '# SFX END'
OK = {'cc0', 'pdm', 'by'}
RATE = 48000


def wishlist():
    if not os.path.exists(WISHLIST):
        return {}
    return {p['pack']: p for p in json.load(open(WISHLIST))['packs']}


def is_bed(p):
    if p.get('music'):
        return False
    """Ambience beds (long loops) sit quieter than one-shots."""
    return p.get('length') in ('medium', 'long') or (p.get('max_s') or 0) > 20


XFADE_S = 2.0  # loop seam of an ambience bed


def cook_music(src, dst):
    """Music: stereo, full length, silence trimmed, 1 s fade in / 3 s fade out, -18 LUFS."""
    probe = subprocess.run(['ffprobe', '-v', 'error', '-show_entries', 'format=duration', '-of',
                            'csv=p=0', src], capture_output=True, text=True, check=True)
    dur = float(probe.stdout.strip() or 0)
    af = ','.join([
        'silenceremove=start_periods=1:start_threshold=-55dB:start_silence=0.05',
        'loudnorm=I=-18:TP=-1.5:LRA=11',
        'afade=t=in:d=1',
        f'afade=t=out:st={max(dur - 3.5, 0)}:d=3',
    ])
    with tempfile.TemporaryDirectory() as tmp:
        wav = os.path.join(tmp, 'x.wav')
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', src, '-ac', '2', '-ar', str(RATE),
                        '-af', af, '-c:a', 'pcm_s16le', wav], check=True)
        subprocess.run(['oggenc', '-Q', '-q', '3', '-o', dst, wav], check=True)


def cook_one(src, dst, max_s, lufs, bed):
    """One-shots: trimmed, faded out at `max_s`. Beds: cut `max_s` + XFADE_S long, then the tail
    is crossfaded (equal power) into the head, so the file loops without a seam."""
    keep = max_s + (XFADE_S if bed else 0.0)
    # silenceremove trims both ends (reverse trick); loudnorm single pass is close enough for SFX.
    af = [
        'silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.02',
        'areverse',
        'silenceremove=start_periods=1:start_threshold=-50dB:start_silence=0.05',
        'areverse',
        f'atrim=0:{keep}',
    ]
    if not bed:
        af.append(f'afade=t=out:st={max(max_s - 0.08, 0)}:d=0.08')
    af += [f'loudnorm=I={lufs}:TP=-1.5:LRA=11']
    if not bed:
        af.append('afade=t=in:d=0.004')
    # Homebrew's ffmpeg has no Vorbis encoder: decode/filter to WAV, encode with oggenc (as cook_car).
    with tempfile.TemporaryDirectory() as tmp:
        wav = os.path.join(tmp, 'x.wav')
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-i', src, '-ac', '1', '-ar', str(RATE),
                        '-af', ','.join(af), '-c:a', 'pcm_s16le', wav], check=True)
        if bed:
            loop_seam(wav)
        subprocess.run(['oggenc', '-Q', '-q', '5', '-o', dst, wav], check=True)


def loop_seam(wav):
    """Crossfade the last XFADE_S of `wav` into its start and drop that tail (in place).
    Standard library only (array), so no numpy is needed."""
    import array
    import math
    import wave
    with wave.open(wav, 'rb') as w:
        params = w.getparams()
        x = array.array('h', w.readframes(params.nframes))
    f = int(XFADE_S * RATE)
    if len(x) < 3 * f:
        return  # too short to loop: leave it as a one-shot
    n = len(x) - f
    out = x[:n]
    for i in range(f):
        t = (i / f) * (math.pi / 2)
        v = x[i] * math.sin(t) + x[n + i] * math.cos(t)
        out[i] = max(-32768, min(32767, int(round(v))))
    with wave.open(wav, 'wb') as w:
        w.setparams(params)
        w.writeframes(out.tobytes())


def replace_block(path, begin, end, body):
    text = open(path).read() if os.path.exists(path) else ''
    block = f'{begin}\n{body.rstrip()}\n{end}'
    if begin in text and end in text:
        head, rest = text.split(begin, 1)
        tail = rest.split(end, 1)[1]
        text = head + block + tail
    else:
        text = text.rstrip() + '\n\n' + block + '\n'
    open(path, 'w').write(text)


def credit_line(c):
    lic = f"{(c.get('license') or '?').upper()} {c.get('license_version') or ''}".strip()
    who = c.get('creator') or 'unknown'
    return f"\"{c.get('title')}\" by {who} ({lic}, {c.get('source')}): {c.get('landing_page')}"


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument('--force', action='store_true', help='re-cook files that are up to date')
    args = ap.parse_args()
    if not os.path.isdir(SRC):
        raise SystemExit(f'nothing to cook: {os.path.relpath(SRC, ROOT)} is missing '
                         '(run tools/audio/openverse_sounds.py --wishlist first)')
    os.makedirs(OUT, exist_ok=True)
    os.makedirs(OUT_MUSIC, exist_ok=True)
    wl = wishlist()
    index, used = {}, []
    for pack in sorted(os.listdir(SRC)):
        cpath = os.path.join(SRC, pack, 'credits.json')
        if not os.path.exists(cpath):
            continue
        p = wl.get(pack, {})
        max_s = float(p.get('max_s') or 30)
        lufs = -26 if is_bed(p) else -20
        entries = []
        for i, c in enumerate(json.load(open(cpath)), 1):
            if (c.get('license') or '').lower() not in OK:
                print(f"  refuse {pack}/{c['file']}: licence {c.get('license')} isn't shippable", file=sys.stderr)
                continue
            src = os.path.join(SRC, pack, c['file'])
            if not os.path.exists(src):
                print(f'  missing {src}', file=sys.stderr)
                continue
            name = f'{pack}_{len(entries) + 1}.ogg'
            dst = os.path.join(OUT_MUSIC if p.get('music') else OUT, name)
            if args.force or not os.path.exists(dst) or os.path.getmtime(dst) < os.path.getmtime(src):
                try:
                    if p.get('music'):
                        cook_music(src, dst)
                    else:
                        cook_one(src, dst, max_s, lufs, is_bed(p))
                    print(f'  {pack}/{c["file"]} -> {"music" if p.get("music") else "sfx"}/{name}')
                except subprocess.CalledProcessError:
                    print(f'  ffmpeg failed on {src}', file=sys.stderr)
                    continue
            entries.append({'file': name, 'title': c.get('title'), 'creator': c.get('creator'),
                            'license': c.get('license'), 'license_version': c.get('license_version'),
                            'source': c.get('source'), 'landing_page': c.get('landing_page')})
            used.append((pack, c))
        if entries:
            index[pack] = {'use': p.get('use', ''), 'bed': is_bed(p), 'music': bool(p.get('music')),
                           'files': entries}
    with open(os.path.join(OUT, 'index.json'), 'w') as f:
        json.dump(index, f, indent=2)

    by = [c for _, c in used if (c.get('license') or '').lower() == 'by']
    cc0 = [c for _, c in used if (c.get('license') or '').lower() != 'by']
    lines = ['Sound effects (from Openverse):']
    lines += [credit_line(c) + '  [CC BY: credit required]' for c in by]
    if cc0:
        lines.append('CC0 / public domain sounds, with thanks: ' +
                     ', '.join(sorted({c.get('creator') or 'unknown' for c in cc0})) + '.')
    replace_block(CREDITS, TXT_BEGIN, TXT_END, '\n'.join(lines))

    md = ['## audio/openverse', '',
          'Downloaded with `tools/audio/openverse_sounds.py` (packs in `tools/audio/sfx_wishlist.json`), '
          'cooked by `tools/audio/cook_sfx.py` into `assets/sounds/sfx/`. Only CC0, PDM and CC BY are cooked.', '']
    for pack, c in used:
        md.append(f"- `{pack}/{c['file']}`: {credit_line(c)} (downloaded {c.get('downloaded', '?')})")
    replace_block(SOURCES, BEGIN, END, '\n'.join(md))
    n = sum(len(v['files']) for v in index.values())
    print(f'{n} sound(s) in {len(index)} pack(s) -> {os.path.relpath(OUT, ROOT)}/ '
          f'({len(by)} CC BY credited in assets/credits.txt)')


if __name__ == '__main__':
    main()
