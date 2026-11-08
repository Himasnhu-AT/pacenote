"""Original Pacenote music from ACE-Step 1.5 (MIT; ../ACE-Step-1.5, MLX on Apple Silicon): the
tracks the game had no song for (more menu songs, a night set for the Late Run, a loading song).
Instrumental, fixed seeds (re-runs give the same songs). Writes raw renders to
assets_src/music_ace/<name>.wav; cook them like the other music (tools/audio/cook_sfx.py `music`
packs: stereo Vorbis, -18 LUFS, fades) into assets/sounds/music/.

Runs in ACE-Step's venv, from its folder:
    cd ../ACE-Step-1.5 && memgate --profile release -- .venv/bin/python \\
        ../pacenote/tools/audio/gen_music_ace.py [name ...]
"""
import os
import shutil
import sys
import time

ACE = os.path.abspath(os.getcwd())
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", "assets_src", "music_ace")
os.environ.setdefault("ACESTEP_LM_BACKEND", "mlx")
os.environ.setdefault("TOKENIZERS_PARALLELISM", "false")
sys.path.insert(0, ACE)

from acestep.handler import AceStepHandler  # noqa: E402
from acestep.inference import GenerationConfig, GenerationParams, generate_music  # noqa: E402
from acestep.llm_inference import LLMHandler  # noqa: E402

# name, caption, bpm, key, seconds, seed
TRACKS = [
    ("menu_music_4", "upbeat driving rock instrumental, overdriven electric guitars, punchy live drums, warm bass, sunny open road festival energy, anthemic chorus riff, no vocals", 132, "E Major", 150, 4104),
    ("menu_music_5", "electronic motorsport theme instrumental, pulsing analog synth bass, driving arpeggios, tight electronic drums, building tension and release, rally stage start energy, no vocals", 124, "A minor", 150, 5205),
    ("menu_music_6", "acoustic folk rock instrumental, banjo and acoustic guitar, foot stomps and hand claps, fiddle melody, dusty outback gravel road, upbeat and warm, no vocals", 118, "G Major", 150, 6306),
    ("night_music_1", "late night synthwave instrumental, retro analog pads, slow arpeggiated synths, gated reverb drums, neon highway at midnight, nostalgic and calm, no vocals", 96, "F minor", 150, 7101),
    ("night_music_2", "dark downtempo electronica instrumental, deep sub bass, soft glitchy percussion, airy pads, night drive through forest, moody and focused, no vocals", 88, "D minor", 150, 7202),
    ("night_music_3", "chill lofi hip hop instrumental, mellow rhodes electric piano, soft vinyl crackle, laid back drums, warm night after a race, relaxed, no vocals", 80, "C Major", 150, 7303),
    ("loading_music_4", "ambient cinematic instrumental, soft evolving pads, gentle piano motif, distant engine-like drones, anticipation before a rally stage, slow and calm, no drums, no vocals", 70, "D Major", 120, 8404),
]


def main():
    want = set(sys.argv[1:])
    os.makedirs(OUT, exist_ok=True)
    dit, lm = AceStepHandler(), LLMHandler()
    msg, ok = dit.initialize_service(project_root=ACE, config_path="acestep-v15-turbo", device="auto")
    print("[ace] dit:", ok, msg[:200], flush=True)
    msg, ok = lm.initialize(checkpoint_dir=os.path.join(ACE, "checkpoints"), lm_model_path="acestep-5Hz-lm-1.7B", backend="mlx", device="auto")
    print("[ace] lm:", ok, msg[:200], flush=True)
    for name, caption, bpm, key, secs, seed in TRACKS:
        if want and name not in want:
            continue
        t = time.time()
        params = GenerationParams(caption=caption, lyrics="[Instrumental]", instrumental=True, bpm=bpm, keyscale=key, timesignature="4", duration=secs, seed=seed, inference_steps=8, shift=3.0)
        cfg = GenerationConfig(batch_size=1, audio_format="wav", use_random_seed=False, seeds=[seed])
        tmp = os.path.join(OUT, "_tmp")
        res = generate_music(dit, lm, params, cfg, save_dir=tmp)
        if not res.success or not res.audios:
            print(f"ACE {name} FAILED {res.error}", flush=True)
            continue
        shutil.move(res.audios[0]["path"], os.path.join(OUT, f"{name}.wav"))
        print(f"ACE {name} {secs}s in {time.time() - t:.0f}s", flush=True)
    shutil.rmtree(os.path.join(OUT, "_tmp"), ignore_errors=True)


if __name__ == "__main__":
    main()
