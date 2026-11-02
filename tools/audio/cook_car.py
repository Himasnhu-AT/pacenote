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

# Engine recordings: Mercedes-Benz 190E 2.3-16V on a dyno (Chippy569, CC0). (name, bay file,
# exhaust file, expected firing Hz). "on" = full load, "off" = no load (neutral). These are the
# only steady-rpm recordings of the session; its sweeps are 2-5 s transient pulls, each mic from a
# different take, so they aren't used.
ENGINE = [
    ('idle', '162516_off500_bay.ogg', '162515_off500_exh.ogg', 34.8),
    ('off_lo', '162519_off2500_bay.ogg', '162518_off2500_exh.ogg', 80.9),
    ('off_hi', '162522_off6500_bay.ogg', '162521_off6500_exh.ogg', 209.8),
    ('on_lo', '162510_on2500_bay.ogg', '162509_on2500_exh.ogg', 85.1),
    ('on_hi', '162513_on6500_bay.ogg', '162512_on6500_exh.ogg', 207.1),
]
ENGINE_RMS_DB = -20.0
# The shipped engine loops, per load: (rpm, recording name or None). A recording ships as itself
# (`engine_<load>_<rpm>_<mic>`, idle as `engine_idle_<mic>`); the points between recordings are
# re-synthesised at cook time by pitch-synchronous overlap-add (PSOLA): each firing of the
# recordings that bracket the point (weighted in log rpm) is cut out with a window of about two
# firing periods, centred on its pulse, and laid down again at the new firing period. The pulse
# shapes (the intake/exhaust resonances) stay where they were recorded, which plain resampling
# doesn't do: that moves every resonance with the pitch. Points are ~16 % apart so the game never
# resamples a loop by more than ±8 %. The game's table (src/audio/powertrain.rs) is printed by
# --report.
IDLE_RPM = 1006
ENGINE_ON = [(1450, None), (1670, None), (1925, None), (2218, None), (2556, 'on_lo'), (2963, None),
             (3434, None), (3980, None), (4613, None), (5347, None), (6208, 'on_hi'), (7200, None)]
ENGINE_OFF = [(1166, None), (1351, None), (1566, None), (1815, None), (2103, None), (2431, 'off_lo'), (2849, None),
              (3339, None), (3913, None), (4586, None), (5374, None), (6297, 'off_hi'), (7300, None)]
# Seconds per shipped loop (whole firings): memory is 192 KB per second of mono 48 kHz f32.
DERIVED_SECS = 1.3
REAL_MAX_SECS = 2.6

# Other loops: (name, [(file, start s, end s), ...], high-pass Hz, low-pass Hz, RMS dB). Several
# segments are levelled, then joined with crossfades (a longer, less repetitive loop).
LOOPS = [
    # Tyres rolling slowly on a gravel road, mic close to the wheels (the engine rumble under
    # it is filtered out; the game speeds it up with the car).
    # The bodies of nine separate dirt skids.
    ('gravel_slide', [('675282_dirt_skids.ogg', a, b) for a, b in
                      [(2.2, 3.8), (9.1, 10.8), (16.3, 18.6), (22.3, 24.6), (29.0, 30.8), (35.2, 37.6),
                       (42.0, 43.9), (64.2, 67.8), (76.2, 79.8)]], 90, None, -20.0),
    # Tyre roar on a highway, from inside a car.
    # Tyre squeal: a V6 saloon through hard corners, then a 4-cylinder estate's handbrake turn.
    ('tarmac_squeal', [('71738_squeal3.ogg', 6.0, 9.3), ('76804_volvo_hb1.ogg', 7.55, 8.95)], 350, None, -20.0),
    # Tyres creeping over snow and ice.
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


def read_engine(name, bay_f, exh_f, guess):
    """A recording as a seamless loop of whole firings: {f, period, cycles, bay, exh} (both mics
    cut alike, each at ENGINE_RMS_DB)."""
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
    loops = {mic: normalise(make_loop(x, loop_len, xf, power=False), ENGINE_RMS_DB) for mic, x in (('bay', bay), ('exh', exh))}
    return dict(name=name, f=f, period=loop_len / cycles, cycles=cycles, **loops)


def shorten(rec, secs):
    """The same loop cut to at most `secs` of whole firings (the cut crossfaded over 40 ms)."""
    cycles = min(rec['cycles'], max(4, int(secs / rec['period'] * RATE)))
    if cycles == rec['cycles']:
        return rec
    n = int(round(cycles * rec['period']))
    xf = min(int(0.04 * RATE), len(rec['bay']) - n)
    out = dict(rec, cycles=cycles, period=n / cycles)
    for mic in ('bay', 'exh'):
        x = np.concatenate([rec[mic], rec[mic][:xf]])
        out[mic] = make_loop(x, n, xf, power=False)
    return out


def pulse_marks(rec):
    """Sample index of each firing's pulse in a recording's loop: the phase where the folded
    level peaks, then each mark nudged to its own local peak (follows small rpm wander)."""
    p, c = rec['period'], rec['cycles']
    n = len(rec['exh'])
    env = np.abs(rec['exh']) / (np.abs(rec['exh']).mean() + 1e-12) + np.abs(rec['bay']) / (np.abs(rec['bay']).mean() + 1e-12)
    k = max(1, int(0.0015 * RATE))
    env = np.convolve(np.concatenate([env[-k:], env, env[:k]]), np.ones(k) / k, 'same')[k:-k]
    steps = int(p)
    fold = np.zeros(steps)
    for j in range(c):
        idx = (np.round(j * p + np.arange(steps)).astype(int)) % n
        fold += env[idx]
    phase = int(np.argmax(fold))
    r = max(1, int(p / 5))
    marks = []
    for j in range(c):
        m = int(round(phase + j * p))
        idx = np.arange(m - r, m + r + 1) % n
        marks.append(int(idx[np.argmax(env[idx])]))
    return np.array(marks)


def psola(sources, rpm, secs):
    """A seamless loop at `rpm` from recordings: [(weight, rec, marks)]. Returns {bay, exh}
    (same length, same firing positions)."""
    f = rpm / 30.0
    pt = RATE / f
    firings = max(8, int(round(secs * f)))
    length = int(round(firings * pt))
    out = {'bay': np.zeros(length), 'exh': np.zeros(length), 'cycles': firings}
    for k in range(firings):
        centre = int(round(k * length / firings))
        for w, rec, marks in sources:
            if w <= 0:
                continue
            # Window: two periods of the shorter of the two (a raised firing keeps only what
            # fits before the next one; a lowered one keeps its whole ring and a short gap).
            half = int(round(min(rec['period'], pt)))
            win = np.hanning(2 * half + 1)
            m = marks[k % len(marks)]
            n = len(rec['bay'])
            src_idx = np.arange(m - half, m + half + 1) % n
            dst_idx = np.arange(centre - half, centre + half + 1) % length
            for mic in ('bay', 'exh'):
                np.add.at(out[mic], dst_idx, w * rec[mic][src_idx] * win)
    # Two recordings' firings don't always add up the same (partly in phase, partly not), so the
    # level of a blend wanders more than a recording's. Ride it back: the 120 ms level (circular)
    # is pulled most of the way to the loop's mean.
    for mic in ('bay', 'exh'):
        y = out[mic]
        k = int(0.12 * RATE)
        p = np.concatenate([y[-k:], y, y[:k]]) ** 2
        env = np.sqrt(np.convolve(p, np.ones(k) / k, 'same')[k:-k]) + 1e-9
        out[mic] = y * (np.sqrt(np.mean(y ** 2)) / env) ** 0.8
    return out


def bracket(points, recs, rpm):
    """[(weight, rec)] of the recordings around `rpm` in one load's list (log-rpm weights)."""
    real = [(r, recs[name]) for r, name in points if name] + [(IDLE_RPM, recs['idle'])]
    real.sort(key=lambda t: t[0])
    lo = [t for t in real if t[0] <= rpm]
    hi = [t for t in real if t[0] >= rpm]
    if not lo:
        return [(1.0, hi[0][1])]
    if not hi:
        return [(1.0, lo[-1][1])]
    (a, ra), (b, rb) = lo[-1], hi[0]
    if a == b:
        return [(1.0, ra)]
    t = np.log(rpm / a) / np.log(b / a)
    return [(1 - t, ra), (t, rb)]


def centroid(y):
    s = np.abs(np.fft.rfft(y * np.hanning(len(y)))) ** 2
    fr = np.fft.rfftfreq(len(y), 1 / RATE)
    return (s * fr).sum() / s.sum()


def cook_engine(report):
    total = 0
    recs = {name: read_engine(name, b, e, g) for name, b, e, g in ENGINE}
    marks = {name: pulse_marks(r) for name, r in recs.items()}
    table = []

    def aligned(name):
        # Rotated so a firing's pulse sits at sample 0, like the re-synthesised loops: the game
        # plays every loop from one shared firing clock, so crossfaded loops pulse together.
        r = dict(recs[name])
        m0 = int(marks[name][0])
        for mic in ('bay', 'exh'):
            r[mic] = np.roll(r[mic], -m0)
        return shorten(r, REAL_MAX_SECS)

    def ship(stem, loops, rpm, firing_guess, derived):
        nonlocal total
        for mic in ('bay', 'exh'):
            y = normalise(loops[mic], ENGINE_RMS_DB)
            size = write(f'engine_{stem}_{mic}', y)
            total += size
            if report:
                f = firing_hz(y, firing_guess)
                print(f'engine_{stem}_{mic}: {"PSOLA" if derived else "recorded"} {rpm:.0f} rpm, measured {f * 30:.0f} rpm, '
                      f'{len(y) / RATE:.2f} s, centroid {centroid(y):.0f} Hz, seam {seam(y):.2f}, {size / 1024:.0f} KB')

    idle = aligned('idle')
    ship('idle', idle, idle['f'] * 30, idle['f'], False)
    table.append(('idle', idle['f'] * 30, idle['cycles']))
    for load, points in (('on', ENGINE_ON), ('off', ENGINE_OFF)):
        for rpm, name in points:
            stem = f'{load}_{rpm}'
            if name:
                r = aligned(name)
                ship(stem, r, r['f'] * 30, r['f'], False)
                table.append((stem, r['f'] * 30, r['cycles']))
            else:
                srcs = [(w, r, marks[r['name']]) for w, r in bracket(points, recs, rpm)]
                out = psola(srcs, rpm, DERIVED_SECS)
                ship(stem, out, rpm, rpm / 30, True)
                table.append((stem, float(rpm), out['cycles']))
    if report:
        print('table for src/audio/powertrain.rs (stem, rpm, firings):')
        for stem, rpm, cycles in table:
            print(f'    ("{stem}", {rpm:.1f}, {cycles}),')
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
    if '--engine' in sys.argv:
        total = cook_engine(report)
    else:
        total = cook_engine(report) + cook_loops(report) + cook_shots(report)
    print(f'total {total / 1024:.0f} KB in {OUT}')
