"""Concept images for Pacenote's original cars (Qwen-Image 2.1 Turbo, Apache-2.0, local via mflux):
the photo TripoSG turns into a shape (asset-forge/tools/triposg_run.py), then
asset-forge/tools/image3d_refine.py and tools/art/import_sketchfab.py make it a game car.

Prompts ask for plain white paint, dark glass, no badges, no plates, no text and a clean studio
3/4 view, which is what image-to-3D reads best. Fixed seeds, so re-runs give the same images.

    memgate -- ~/codes/manga-studio/.venv/bin/python tools/art/concept_cars.py [id ...] [--seeds 1,2,3]
Writes assets_src/concept_cars/<id>_<seed>.png.

    ... concept_cars.py --rear lynx_11 [dingo_21 ...]
The same car from behind (edit mode, the front image as the reference): <pick>_rear.png, the
second view tools/image3d_project.py colours the back of the mesh from.
"""
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "..", "..", "assets_src", "concept_cars")

STYLE = (
    "studio product photo of a single car, three-quarter front view from the left at eye level, "
    "whole car in frame with margin, plain light grey seamless background, soft even lighting, "
    "plain gloss white paint, dark tinted windows, black plastic trim, no logos, no badges, no "
    "emblems, no number plates, no text, no stickers, no people, sharp, realistic, original design"
)
CARS = {
    # id: (description, default seeds)
    "lynx": ("compact boxy 1980s two-door rally hatchback, original design, flared square wheel arches, "
             "four square fog lamps on the front, small roof scoop, rear spoiler on the hatch, "
             "steel mud flaps, white multi-spoke rally wheels with gravel tyres", [11, 12, 13]),
    "tarn": ("boxy early-1990s four-door family saloon prepared for gravel rally, original design, "
             "square headlamps, slightly flared wheel arches, small boot-lid spoiler, mud flaps, "
             "white five-spoke steel rally wheels with gravel tyres, raised ride height", [41, 42, 43]),
    "kite": ("small 1970s two-door rear-wheel-drive sports coupe, original design, long bonnet, short "
             "rear deck, round twin headlamps, thin chrome bumpers, slim pillars, raised slightly for "
             "gravel, black steel wheels with small hub caps", [31, 32, 33]),
}


def main():
    import mlx.core as mx
    from mflux.models.common.config import ModelConfig
    from mflux.models.common.vae.tiling_config import TilingConfig
    from mflux.models.qwen21.variants.edit.qwen_image_21_edit import QwenImage21Edit

    args = [a for a in sys.argv[1:]]
    seeds = None
    if "--seeds" in args:
        i = args.index("--seeds")
        seeds = [int(s) for s in args[i + 1].split(",")]
        del args[i:i + 2]
    rear = "--rear" in args
    if rear:
        args.remove("--rear")
    want = args or list(CARS)
    os.makedirs(OUT, exist_ok=True)
    m = QwenImage21Edit(quantize=4, model_config=ModelConfig.qwen_image_21_turbo())
    m.tiling_config = TilingConfig(vae_decode_tile_size=512)  # keeps the decode peak small
    mx.set_cache_limit(2 * 1024**3)
    if rear:
        for pick in want:
            front = os.path.join(OUT, f"{pick}.png")
            prompt = ("Show the same car as image 1 from behind: rear three-quarter view from the right, "
                      "same body shape, same plain white paint, same wheels, same dark windows, tail lamps, "
                      "rear bumper, same plain light grey studio background, whole car in frame, "
                      "no logos, no badges, no number plate, no text")
            img = m.generate_image(seed=7, prompt=prompt, width=1344, height=896, image_paths=[front], output_resolution=1024)
            path = os.path.join(OUT, f"{pick}_rear.png")
            img.save(path, export_json_metadata=False)
            print(f"CONCEPT {pick} rear -> {path}", flush=True)
            mx.clear_cache()
        return
    for cid in want:
        desc, default = CARS[cid]
        for seed in seeds or default:
            t = time.time()
            img = m.generate_image(seed=seed, prompt=f"{desc}. {STYLE}", width=1344, height=896, image_paths=[], output_resolution=1024)
            path = os.path.join(OUT, f"{cid}_{seed}.png")
            img.save(path, export_json_metadata=False)
            print(f"CONCEPT {cid} seed {seed} -> {path} ({time.time() - t:.0f}s)", flush=True)
            mx.clear_cache()


if __name__ == "__main__":
    main()
