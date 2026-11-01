# Builds the roadside bushes (Blender, background):
#   memgate -- blender -b --factory-startup -P tools/art/build_bushes.py -- <assets_src> <assets>
#
# Poly Haven has no dense bush, so the bushes are made like game bushes usually are: clusters of
# leaf-clump cards. The clumps are cut from a real scanned crown (`tree_small_02`, CC0) and
# rendered from the side into a 2 x 2 atlas (`assets/textures/m_shrub/color.png`, normal.png);
# `assets/models/shrub_a.pnm` / `shrub_b.pnm` (src/art/meshfile.rs format) are shells of those
# cards around a few crossed core cards, with soft outward normals so they light like a volume.
# Used by the dressing (src/art/dressing.rs) and the drive-through shrubs (src/art/props.rs).
import math
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import bpy  # noqa: E402
import numpy as np  # noqa: E402

import render_tree_cards as rt  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, ASSETS = argv[0], argv[1]
MODEL, OBJ = "tree_small_02", "tree_small_02_LOD0"
ALPHAS = {"tree_small_02_leaves": "tree_small_02_leaves_alpha_2k.jpg"}
CELL = 512
R = 0.62  # clump radius (m)


def clumps():
    s = rt.reset()
    folder = os.path.join(SRC, "models", "vegetation", MODEL)
    bpy.ops.import_scene.gltf(filepath=os.path.join(folder, f"{MODEL}_2k.gltf"))
    tree = bpy.data.objects[OBJ]
    for o in list(bpy.data.objects):
        if o != tree:
            bpy.data.objects.remove(o, do_unlink=True)
    tree.location = (0, 0, 0)
    me = tree.data
    n = len(me.polygons)
    cen = np.zeros(n * 3, np.float32)
    me.polygons.foreach_get("center", cen)
    cen = cen.reshape(-1, 3)
    mi = np.zeros(n, np.int64)
    me.polygons.foreach_get("material_index", mi)
    leaves = np.isin(mi, [k for k, m in enumerate(me.materials) if m.name in ALPHAS])
    not_trunk = ~np.isin(mi, [k for k, m in enumerate(me.materials) if "trunk" in m.name])
    tree.hide_render = True
    lc = cen[leaves]
    ax, ay = np.median(lc[:, 0]), np.median(lc[:, 1])
    z0, z1 = np.percentile(lc[:, 2], 30), np.percentile(lc[:, 2], 85)
    tmp = os.path.join(ASSETS, "textures", "m_shrub", "_tmp")
    os.makedirs(tmp, exist_ok=True)
    rng = np.random.default_rng(11)
    alb = np.zeros((2 * CELL, 2 * CELL, 4), np.float32)
    nor = np.zeros_like(alb)
    for k in range(4):
        # The densest of a few candidate spots on this side of the crown.
        a = k * math.pi / 2 + 0.4
        best = None
        for _ in range(40):
            aa = a + rng.uniform(-0.6, 0.6)
            z = rng.uniform(z0, z1)
            r = rng.uniform(0.3, 1.0)
            p = np.array([ax + math.cos(aa) * r, ay + math.sin(aa) * r, z])
            cnt = int(np.count_nonzero(leaves & (np.linalg.norm(cen - p, axis=1) < R)))
            if best is None or cnt > best[0]:
                best = (cnt, p, aa)
        _, p, aa = best
        keep = not_trunk & (np.linalg.norm(cen - p, axis=1) < R)
        o = rt.submesh(tree, keep, f"clump{k}")
        for oo in bpy.data.objects:
            if oo.name.startswith("clump"):
                oo.hide_render = oo is not o
        tilt = math.radians(12)
        d = np.array([math.cos(aa) * math.cos(tilt), math.sin(aa) * math.cos(tilt), math.sin(tilt)])
        sc = 2 * (R + 0.25)
        cam = rt.camera(s, tuple(p + d * 20), (math.pi / 2 - tilt, 0, aa + math.pi / 2), sc, CELL, CELL)
        rt.render_passes(s, [o], ALPHAS, folder, os.path.join(tmp, f"clump{k}"))
        bpy.data.objects.remove(cam, do_unlink=True)
        y, x = (k // 2) * CELL, (k % 2) * CELL
        alb[y:y + CELL, x:x + CELL] = rt.load_px(os.path.join(tmp, f"clump{k}_albedo.png"))
        nor[y:y + CELL, x:x + CELL] = rt.load_px(os.path.join(tmp, f"clump{k}_normal.png"))
    alpha = alb[..., 3]
    na = np.maximum(alpha[..., None], 1e-3)
    known = alpha > 0.5
    rgb = rt.dilate(np.clip(np.where(alpha[..., None] > 1e-3, alb[..., :3] / na, 0), 0, 1), known)
    out = os.path.join(ASSETS, "textures", "m_shrub")
    rt.save_px(os.path.join(out, "color.png"), np.dstack([rgb, alpha]))
    nn = np.where(alpha[..., None] > 1e-3, (nor[..., :3] * 2 - 1) / na, 0)
    nn = 0.55 * nn + 0.45 * np.array([0, 0, 1], np.float32)
    nn /= np.maximum(np.linalg.norm(nn, axis=-1, keepdims=True), 1e-6)
    rt.save_px(os.path.join(out, "normal.png"), np.dstack([rt.dilate(nn * 0.5 + 0.5, known), np.ones_like(alpha)]))
    for f in os.listdir(tmp):
        os.remove(os.path.join(tmp, f))
    os.rmdir(tmp)


def bush(name, seed, cards, half, height):
    """A shell of clump cards (Bevy coordinates: Y up, base at y = 0), written as PNM1."""
    rng = np.random.default_rng(seed)
    up = np.array([0.0, 1.0, 0.0])
    core = np.array([0.0, height * 0.35, 0.0])
    pos, nrm, uv, idx = [], [], [], []

    def card(c, n, size, cell):
        n = n / np.linalg.norm(n)
        u = np.cross(up, n)
        u = u / np.linalg.norm(u) if np.linalg.norm(u) > 1e-3 else np.array([1.0, 0.0, 0.0])
        v = np.cross(n, u)
        h = size / 2
        corners = [c - u * h - v * h, c + u * h - v * h, c + u * h + v * h, c - u * h + v * h]
        # Inset a little so filtering never reads the neighbouring clump.
        cx, cy = (cell % 2) * 0.5, (cell // 2) * 0.5
        u0, u1, v0, v1 = cx + 0.004, cx + 0.496, cy + 0.004, cy + 0.496
        if rng.random() < 0.5:
            u0, u1 = u1, u0  # mirrored clump: more variety from four pictures
        b = len(pos)
        for p, t in zip(corners, [(u0, v1), (u1, v1), (u1, v0), (u0, v0)]):
            out = p - core
            soft = 0.35 * n + 0.65 * out / max(np.linalg.norm(out), 1e-3) + 0.25 * up
            pos.append(p)
            nrm.append(soft / np.linalg.norm(soft))
            uv.append(t)
        idx.extend([b, b + 1, b + 2, b, b + 2, b + 3])

    # Core: crossed upright cards through the middle (fills the inside).
    for k in range(3):
        a = k * math.pi / 3 + rng.uniform(0, 0.5)
        size = height * 1.05
        # The clumps are round: sink the cards so their lower leaves reach the ground.
        card(np.array([0.0, size / 2 - 0.2, 0.0]), np.array([math.cos(a), 0.0, math.sin(a)]), size, int(rng.integers(4)))
    # Shell: cards on a half-ellipsoid, facing out (tilted at random), bigger low down.
    for _ in range(cards):
        a = rng.uniform(0, 2 * math.pi)
        e = rng.uniform(-0.1, 1.0) * math.pi / 2 * 0.95  # elevation (a few skirt cards below)
        d = np.array([math.cos(a) * math.cos(e), math.sin(e), math.sin(a) * math.cos(e)])
        c = np.array([d[0] * half * 0.62, d[1] * height * 0.68 + height * 0.12, d[2] * half * 0.62])
        size = rng.uniform(0.6, 0.9) * (1.1 - 0.3 * d[1]) * (half / 0.9)
        n = d + rng.normal(0, 0.35, 3)
        n[1] = max(n[1], -0.1)
        c[1] = max(c[1], size * 0.3 - 0.12)
        card(c, n, size, int(rng.integers(4)))
    pos, nrm, uv = np.array(pos), np.array(nrm), np.array(uv)
    path = os.path.join(ASSETS, "models", f"{name}.pnm")
    with open(path, "wb") as f:
        f.write(b"PNM1")
        f.write(struct.pack("<II", len(pos), len(idx)))
        f.write(pos.astype("<f4").tobytes())
        f.write(nrm.astype("<f4").tobytes())
        f.write(uv.astype("<f4").tobytes())
        f.write(np.array(idx, "<u4").tobytes())
    lo, hi = pos.min(0), pos.max(0)
    print(f"bush {name}: {len(idx) // 3} tris, {hi[0] - lo[0]:.2f} x {hi[1] - lo[1]:.2f} x {hi[2] - lo[2]:.2f} m")


clumps()
bush("shrub_a", 3, 32, 0.95, 1.45)
bush("shrub_b", 8, 24, 0.75, 1.15)
