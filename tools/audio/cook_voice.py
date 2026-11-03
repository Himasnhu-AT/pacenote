"""Cook the co-driver's phrase bank: every word and phrase the co-driver can say, rendered once
with Piper (neural TTS, run here at build time only) -> assets/sounds/voice/*.ogg + bank.txt.

The game never runs a TTS: it plays lines by joining these clips (src/audio/voice.rs). The
vocabulary is what src/codriver/ can produce (calls.rs: distances, left/right, grades ("hairpin",
"square", "one".."six"), "tightens", "opens", "long", "over crest", "crest into", "don't cut",
"narrows", "off camber", "careful", "into", the finish; mod.rs: ready, splits and times, damage,
recovery); `cargo test --lib voice` checks that every line those can make is covered.

Voice: Piper `en_GB-cori-high` (Bryce Beattie, trained from scratch on public-domain LibriVox
recordings; the voice is released into the public domain). Model card and licence:
assets_src/SOURCES.md. Output: mono 24 kHz Ogg Vorbis, silence trimmed, levelled.

Run with a Python that has piper-tts and numpy (a venv; Piper is GPL, used as a tool only):
    <venv>/bin/python -I tools/audio/cook_voice.py --model assets_src/voice_piper/en_GB-cori-high.onnx
Needs ffmpeg and oggenc. `--only stem,stem` renders just those clips (new words) and leaves the
others as they are; bank.txt is always written whole.
"""
import os
import subprocess
import sys
import tempfile
import wave

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'assets', 'sounds', 'voice')
RATE = 24000
# Brisk, like a co-driver reading notes (Piper's phoneme length: 1 = the voice's own pace).
LENGTH_SCALE = 0.86
RMS_DB = -17.0
PEAK_DB = -1.5

NUM_WORDS = ['zero', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven',
             'twelve', 'thirteen', 'fourteen', 'fifteen', 'sixteen', 'seventeen', 'eighteen', 'nineteen']
TENS = ['', '', 'twenty', 'thirty', 'forty', 'fifty', 'sixty', 'seventy', 'eighty', 'ninety']


def spell(n):
    """How a co-driver says a number: 150 = "one fifty", 200 = "two hundred", 23 = "twenty-three"."""
    if n < 20:
        return NUM_WORDS[n]
    if n < 100:
        return TENS[n // 10] + ('' if n % 10 == 0 else '-' + NUM_WORDS[n % 10])
    if n >= 1000:
        return 'one thousand'
    if n % 100 == 0:
        return NUM_WORDS[n // 100] + ' hundred'
    return NUM_WORDS[n // 100] + ' ' + spell(n % 100)


# (file stem, spoken text, keys the game looks it up by). Keys are lower case, words split on
# spaces, punctuation removed (see `normalise` in src/audio/voice.rs).
def vocabulary():
    v = []
    # Numbers: 0-59 (times, splits), distances 60..190 by 10 and 200..1000 by 50.
    nums = list(range(0, 60)) + list(range(60, 200, 10)) + list(range(200, 1001, 50))
    for n in nums:
        keys = [str(n)]
        if 1 <= n <= 6:
            keys.append(NUM_WORDS[n])  # the grades "one".."six" (1 is "one" in "tightens one")
        v.append((f'n{n}', spell(n), keys))
    words = [
        # Pace notes.
        ('left', 'left'), ('right', 'right'), ('hairpin', 'hairpin'), ('long', 'long'),
        ('over_crest', 'over crest'), ('narrows', 'narrows'), ('careful', 'careful'), ('into', 'into'),
        ('square', 'square'), ('tightens', 'tightens'), ('opens', 'opens'), ('crest_into', 'crest into'),
        ('dont_cut', "don't cut"), ('off_camber', 'off camber'),
        ('to_flying_finish', 'to flying finish'), ('flying_finish', 'flying finish'), ('flat_out', 'flat out'),
        ('ready', 'Ready.'), ('co_driver_on', 'Co-driver on.'),
        # Splits and times.
        ('split', 'split'), ('minute', 'minute'), ('minutes', 'minutes'), ('point', 'point'),
        ('seconds', 'seconds'), ('stop', 'Stop!'), ('stage_time', 'stage time'),
        # Damage.
        ('thats_it', "That's it,"), ('the_engine', 'the engine'), ('the_steering', 'the steering'),
        ('the_suspension', 'the suspension'), ('is_gone', 'is gone.'), ('out_of_the_stage', "We're out of the stage."),
        ('big_hit', 'Big hit!'), ('ouch', 'Ouch.'), ('is_badly_damaged', 'is badly damaged,'),
        ('nurse_it_home', 'nurse it home.'), ('is_damaged', 'is damaged.'), ('took_a_knock', 'took a knock.'),
        ('pulling_left', "it's pulling left."), ('pulling_right', "it's pulling right."),
        # Recovery.
        ('on_the_roof', "We're on the roof."), ('press_r_back_on_road', 'Press R to get back on the road.'),
        ('off_the_road', "We're off the road."), ('get_back_on', 'Get back on,'), ('or_press_r', 'or press R.'),
    ]
    for stem, text in words:
        v.append((stem, text, [key(text)]))
    return v


def key(text):
    keep = ''.join(c if c.isalnum() or c == "'" else ' ' for c in text.lower())
    return ' '.join(keep.split())


def render(voice, text):
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, 'x.wav')
        from piper import SynthesisConfig
        with wave.open(path, 'wb') as w:
            voice.synthesize_wav(text, w, syn_config=SynthesisConfig(length_scale=LENGTH_SCALE))
        raw = subprocess.run(['ffmpeg', '-v', 'error', '-i', path, '-ac', '1', '-ar', str(RATE),
                              '-f', 'f32le', '-'], capture_output=True, check=True).stdout
    return np.frombuffer(raw, dtype=np.float32).astype(np.float64)


def trim(x):
    """Cut the silence either side (−40 dB under the clip's loudest 10 ms), keep 15 ms before and
    40 ms after, fade 4 ms in and 20 ms out."""
    w = int(0.01 * RATE)
    n = len(x) // w
    db = 20 * np.log10(np.sqrt((x[:n * w].reshape(n, w) ** 2).mean(1)) + 1e-9)
    on = np.where(db > db.max() - 40)[0]
    # Runs of sound (gaps under 80 ms joined); a short run after the speech (a click or breath
    # the model adds after a pause) is dropped.
    runs = [[on[0], on[0]]]
    for i in on[1:]:
        if i - runs[-1][1] <= 8:
            runs[-1][1] = i
        else:
            runs.append([i, i])
    while len(runs) > 1 and runs[-1][1] - runs[-1][0] < 6:
        runs.pop()
    on = [runs[0][0], runs[-1][1]]
    a = max(0, on[0] * w - int(0.015 * RATE))
    b = min(len(x), (on[-1] + 1) * w + int(0.04 * RATE))
    y = x[a:b].copy()
    fi, fo = int(0.004 * RATE), int(0.02 * RATE)
    y[:fi] *= np.linspace(0, 1, fi)
    y[-fo:] *= np.linspace(1, 0, fo)
    return y


def level(y):
    """Speech RMS (over 10 ms windows within 30 dB of the loudest) to RMS_DB, peaks under PEAK_DB."""
    w = int(0.01 * RATE)
    n = max(1, len(y) // w)
    r = np.sqrt((y[:n * w].reshape(n, w) ** 2).mean(1)) + 1e-12
    act = r[r > r.max() * 10 ** (-30 / 20)]
    g = 10 ** (RMS_DB / 20) / np.sqrt(np.mean(act ** 2))
    pk = np.abs(y).max() * g
    if pk > 10 ** (PEAK_DB / 20):
        g *= 10 ** (PEAK_DB / 20) / pk
    return y * g


def write(stem, y):
    pcm = np.clip(np.round(y * 32767), -32768, 32767).astype('<i2')
    with tempfile.TemporaryDirectory() as tmp:
        wav = os.path.join(tmp, stem + '.wav')
        subprocess.run(['ffmpeg', '-v', 'error', '-y', '-f', 's16le', '-ar', str(RATE), '-ac', '1', '-i', '-', wav],
                       input=pcm.tobytes(), check=True)
        dst = os.path.join(OUT, stem + '.ogg')
        subprocess.run(['oggenc', '-Q', '-q', '3', '-o', dst, wav], check=True)
    return os.path.getsize(dst)


def main():
    args = sys.argv[1:]
    model = os.path.abspath(args[args.index('--model') + 1])
    import piper
    from piper import PiperVoice
    # espeak-ng refuses data paths over ~160 characters: hand it a relative one.
    data = os.path.join(os.path.dirname(piper.__file__), 'espeak-ng-data')
    os.chdir(os.path.dirname(data))
    voice = PiperVoice.load(model, espeak_data_dir='espeak-ng-data')
    os.makedirs(OUT, exist_ok=True)
    only = set(args[args.index('--only') + 1].split(',')) if '--only' in args else None
    if only is None:
        for f in os.listdir(OUT):
            if f.endswith('.ogg'):
                os.remove(os.path.join(OUT, f))
    total, secs, lines = 0, 0.0, []
    for stem, text, keys in vocabulary():
        lines.append(f'{stem}\t{"|".join(keys)}')
        if only is not None and stem not in only:
            continue
        y = level(trim(render(voice, text)))
        total += write(stem, y)
        secs += len(y) / RATE
        if '--report' in args:
            print(f'{stem:22s} {len(y) / RATE:5.2f} s  "{text}"')
    with open(os.path.join(OUT, 'bank.txt'), 'w', encoding='utf-8') as f:
        f.write('# The co-driver phrase bank: <file stem>\\t<key>|<key>... (written by tools/audio/cook_voice.py)\n')
        f.write('\n'.join(lines) + '\n')
    print(f'{len(lines)} clips, {secs:.1f} s ({secs * 48000 * 4 / 1e6:.1f} MB as 48 kHz f32, '
          f'{secs * RATE * 4 / 1e6:.1f} MB at {RATE} Hz), {total / 1024:.0f} KB in {OUT}')


if __name__ == '__main__':
    main()
