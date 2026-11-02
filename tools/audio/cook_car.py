"""Cook the car's recorded sounds: assets_src/audio_car/freesound/*.ogg -> assets/sounds/car/*.ogg.

Every output is mono, 48 kHz, Ogg Vorbis (oggenc), loudness-normalised. Loops are made seamless
here: the tail past the loop length is crossfaded into the head, so the wrap is the recording
running on. Engine loops are cut to a whole number of firing periods (4-cylinder: rpm/30 Hz),
and a loop's two mics (engine bay, exhaust) get the same cut so the game can play them from one
position. Sources, authors and licences: assets_src/SOURCES.md.

Needs ffmpeg, oggenc (vorbis-tools) and numpy. Run from anywhere:
    python3 -I tools/audio/cook_car.py [--report]
--report prints the measured firing frequency of each engine loop (the rpm table in
src/audio/powertrain.rs comes from it) and the seam check of every loop.
"""
import os
import subprocess
import sys
import tempfile

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SRC = os.path.join(ROOT, 'assets_src', 'audio_car', 'freesound')
OUT = os.path.join(ROOT, 'assets', 'sounds', 'car')
RATE = 48000

# Engine loops: Mercedes-Benz 190E 2.3-16V on a dyno (Chippy569, CC0). (name, bay file, exhaust
# file, expected firing Hz). "on" = full load, "off" = no load (neutral).
ENGINE = [
    ('idle', '162516_off500_bay.ogg', '162515_off500_exh.ogg', 34.8),
    ('off_lo', '162519_off2500_bay.ogg', '162518_off2500_exh.ogg', 80.9),
    ('off_hi', '162522_off6500_bay.ogg', '162521_off6500_exh.ogg', 209.8),
    ('on_lo', '162510_on2500_bay.ogg', '162509_on2500_exh.ogg', 85.1),
    ('on_hi', '162513_on6500_bay.ogg', '162512_on6500_exh.ogg', 207.1),
]
ENGINE_RMS_DB = -20.0

# Other loops: (name, [(file, start s, end s), ...], high-pass Hz, low-pass Hz, RMS dB). Several
# segments are levelled, then joined with crossfades (a longer, less repetitive loop).
LOOPS = [
    # Tyres rolling slowly on a gravel road, mic close to the wheels (the engine rumble under
    # it is filtered out; the game speeds it up with the car).
    ('gravel_roll', [('577501_gravel_cu.ogg', 24.0, 32.0)], 140, None, -20.0),
    # The bodies of nine separate dirt skids.
    ('gravel_slide', [('675282_dirt_skids.ogg', a, b) for a, b in
                      [(2.2, 3.8), (9.1, 10.8), (16.3, 18.6), (22.3, 24.6), (29.0, 30.8), (35.2, 37.6),
                       (42.0, 43.9), (64.2, 67.8), (76.2, 79.8)]], 90, None, -20.0),
    # Tyre roar on a highway, from inside a car.
    ('tarmac_roll', [('817389_int_highway_tyres.ogg', 15.0, 23.0)], 35, None, -20.0),
    # Tyre squeal: a V6 saloon through hard corners, then a 4-cylinder estate's handbrake turn.
    ('tarmac_squeal', [('71738_squeal3.ogg', 6.0, 9.3), ('76804_volvo_hb1.ogg', 7.55, 8.95)], 350, None, -20.0),
    # Tyres creeping over snow and ice.
    ('snow_roll', [('71099_snow_ice_slow.ogg', 0.8, 6.8)], 90, None, -20.0),
    # Metal scraping a hard surface.
    ('scrape', [('534851_metal_scrape6.ogg', 12.0, 18.0)], 150, None, -20.0),
]
# One-shots: (name, file, start s, length s, high-pass Hz, fade-out s, peak dB).
SHOTS = [
    ('impact_heavy', '675475_metal_crunch.ogg', 0.10, 0.85, 30, 0.35, -1.0),
    ('impact_light', '151624_clank_crash.ogg', 0.0, 0.54, 40, 0.2, -3.0),
    ('snap', '164472_branch_crack.ogg', 0.04, 0.55, 80, 0.25, -3.0),
    # Three bangs from a rally car's anti-lag.
    ('pop1', '797835_antilag.ogg', 1.565, 0.17, 40, 0.08, -2.0),
    ('pop2', '797835_antilag.ogg', 2.170, 0.18, 40, 0.08, -2.0),
    ('pop3', '797835_antilag.ogg', 5.270, 0.20, 40, 0.09, -2.0),
]


def decode(path):
    raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', path, '-ac', '1', '-ar', str(RATE), '-f', 'f32le', '-'],
                         capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def biquad(x, kind, hz, q=0.7071):
    w = 2 * np.pi * hz / RATE
    c, s = np.cos(w), np.sin(w)
    a = s / (2 * q)
    if kind == 'hp':
        b = [(1 + c) / 2, -(1 + c), (1 + c) / 2]
    else:
        b = [(1 - c) / 2, 1 - c, (1 - c) / 2]
    den = [1 + a, -2 * c, 1 - a]
    b = [v / den[0] for v in b]
    a1, a2 = den[1] / den[0], den[2] / den[0]
    y = np.zeros_like(x)
    x1 = x2 = y1 = y2 = 0.0
    for i, v in enumerate(x):
        o = b[0] * v + b[1] * x1 + b[2] * x2 - a1 * y1 - a2 * y2
        x2, x1, y2, y1 = x1, v, y1, o
        y[i] = o
    return y


def firing_hz(x, guess):
    """Firing frequency near `guess`: harmonic sum on a fine grid, ±6 %."""
    n = 1 << int(np.ceil(np.log2(len(x))) + 1)
    s = np.abs(np.fft.rfft(x * np.hanning(len(x)), n))
    df = RATE / n
    best = (0, guess)
    for f in np.linspace(guess * 0.94, guess * 1.06, 1200):
        k = np.arange(1, 9)
        sc = s[np.round(k * f / df).astype(int)].sum()
        if sc > best[0]:
            best = (sc, f)
    return best[1]


def make_loop(x, loop_len, xf, power):
    """Seamless loop of `loop_len` samples: x[loop_len:loop_len+xf] crossfaded into the head."""
    assert len(x) >= loop_len + xf, (len(x), loop_len, xf)
    out = x[:loop_len].copy()
    t = (np.arange(xf) + 0.5) / xf
    if power:
        win, wout = np.sin(t * np.pi / 2), np.cos(t * np.pi / 2)
    else:
        win, wout = t, 1 - t
    out[:xf] = x[:xf] * win + x[loop_len:loop_len + xf] * wout
    return out


def steadiest(x, secs, skip=0.5):
    """Start (samples) of the stretch of `secs` with the steadiest level (100 ms RMS spread),
    among stretches at least as loud as the median."""
    w = 4800
    n = len(x) // w
    db = 20 * np.log10(np.sqrt((x[:n * w].reshape(n, w) ** 2).mean(1)) + 1e-9)
    k = int(secs * RATE / w)
    s0 = int(skip * RATE / w)
    cands = []
    for i in range(s0, n - k - s0):
        seg = db[i:i + k]
        cands.append((seg.std(), -seg.mean(), i))
    med = np.median([-c[1] for c in cands])
    cands = [c for c in cands if -c[1] >= med - 1.0]
    cands.sort()
    return cands[0][2] * w


def seam(y):
    """Jump at the wrap vs the clip's typical sample-to-sample step (1 = as smooth as anywhere)."""
    d = np.abs(np.diff(y))
    return abs(y[0] - y[-1]) / (np.percentile(d, 99) + 1e-12)


def write(name, y, quality=4):
    os.makedirs(OUT, exist_ok=True)
    pcm = np.clip(np.round(y * 32767), -32768, 32767).astype('<i2')
    with tempfile.TemporaryDirectory() as tmp:
        wav = os.path.join(tmp, name + '.wav')
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 's16le', '-ar', str(RATE), '-ac', '1', '-i', '-', wav],
                       input=pcm.tobytes(), check=True)
        dst = os.path.join(OUT, name + '.ogg')
        subprocess.run(['oggenc', '-Q', '-q', str(quality), '-o', dst, wav], check=True)
    return os.path.getsize(dst)


def normalise(y, rms_db, peak_db=-1.0):
    g = 10 ** (rms_db / 20) / (np.sqrt(np.mean(y ** 2)) + 1e-12)
    pk = np.abs(y).max() * g
    if pk > 10 ** (peak_db / 20):
        g *= 10 ** (peak_db / 20) / pk
    return y * g


def cook_engine(report):
    total = 0
    for name, bay_f, exh_f, guess in ENGINE:
        bay = biquad(decode(os.path.join(SRC, bay_f)), 'hp', 25)
        exh = biquad(decode(os.path.join(SRC, exh_f)), 'hp', 25)
        n = min(len(bay), len(exh))
        bay, exh = bay[:n], exh[:n]
        # Skip 20 ms of edge either side.
        e = int(0.02 * RATE)
        bay, exh = bay[e:n - e], exh[e:n - e]
        f = firing_hz(bay / np.abs(bay).max() + exh / np.abs(exh).max(), guess)
        period = RATE / f
        xf = int(0.06 * RATE)
        cycles = int((len(bay) - xf) / period)
        loop_len = int(round(cycles * period))
        while loop_len + xf > len(bay):
            cycles -= 1
            loop_len = int(round(cycles * period))
        for mic, x in (('bay', bay), ('exh', exh)):
            y = normalise(make_loop(x, loop_len, xf, power=False), ENGINE_RMS_DB)
            size = write(f'engine_{name}_{mic}', y)
            total += size
            if report:
                print(f'engine_{name}_{mic}: firing {f:.2f} Hz = {f * 30:.0f} rpm, {loop_len / RATE:.2f} s '
                      f'({cycles} firings), seam {seam(y):.2f}, {size / 1024:.0f} KB')
    return total


def cook_loops(report):
    total = 0
    cache = {}
    for name, segs, hp, lp, rms_db in LOOPS:
        joins = 1 if len(segs) == 1 else 0.12
        xf = int(0.25 * RATE) if len(segs) == 1 else int(joins * RATE)
        parts = []
        for f, a, b in segs:
            if f not in cache:
                cache[f] = decode(os.path.join(SRC, f))
            x = cache[f]
            # Filter with some lead-in so the filter has settled at the segment start.
            lead = int(0.1 * RATE)
            s0 = max(0, int(a * RATE) - lead)
            x = x[s0:int(b * RATE) + xf]
            if hp:
                x = biquad(x, 'hp', hp)
            if lp:
                x = biquad(x, 'lp', lp)
            x = x[int(a * RATE) - s0:]
            parts.append(x / (np.sqrt(np.mean(x ** 2)) + 1e-12))
        # Join: each part's extra `xf` tail crossfades (equal power) into the next part's head.
        t = (np.arange(xf) + 0.5) / xf
        win, wout = np.sin(t * np.pi / 2), np.cos(t * np.pi / 2)
        y = parts[0]
        for p in parts[1:]:
            y = np.concatenate([y[:-xf], y[-xf:] * wout + p[:xf] * win, p[xf:]])
        y = normalise(make_loop(y, len(y) - xf, xf, power=True), rms_db)
        size = write(name, y, quality=3)
        total += size
        if report:
            print(f'{name}: {len(segs)} part(s), {len(y) / RATE:.2f} s, seam {seam(y):.2f}, {size / 1024:.0f} KB')
    return total


def cook_shots(report):
    total = 0
    for name, f, start, secs, hp, fade, peak_db in SHOTS:
        x = decode(os.path.join(SRC, f))
        x = x[int(start * RATE):int((start + secs) * RATE)]
        if hp:
            x = biquad(x, 'hp', hp)
        # 2 ms fade in, `fade` s fade out.
        fi = int(0.002 * RATE)
        x[:fi] *= np.linspace(0, 1, fi)
        fo = min(int(fade * RATE), len(x))
        x[len(x) - fo:] *= np.linspace(1, 0, fo) ** 2
        y = x * (10 ** (peak_db / 20) / (np.abs(x).max() + 1e-12))
        size = write(name, y, quality=4)
        total += size
        if report:
            rms = 20 * np.log10(np.sqrt(np.mean(y ** 2)) + 1e-12)
            print(f'{name}: {secs:.2f} s, rms {rms:.1f} dB, {size / 1024:.0f} KB')
    return total


if __name__ == '__main__':
    report = '--report' in sys.argv
    total = cook_engine(report) + cook_loops(report) + cook_shots(report)
    print(f'total {total / 1024:.0f} KB in {OUT}')
