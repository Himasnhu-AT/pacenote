# Renders tree card atlases from Poly Haven's full-size scanned trees (Blender, background):
#   memgate -- blender -b --factory-startup -P tools/art/render_tree_cards.py -- <assets_src> <assets/textures> [set ...]
#
# Each atlas (1024 x 2048) holds, from the real model:
# - left (704 px wide): the whole tree seen from the side (crossed "core" cards, far trees);
# - right column (320 px wide): three branch slices seen from above, trunk end at the bottom
#   (the near trees' branch cards).
# Outputs per set: color.png (albedo with a little baked self-occlusion, alpha cut-out),
# normal.png (card tangent space, OpenGL), frame.txt (the regions in uv and the crown base),
# optional `<set>_snow`.
# The game builds its trees from these (src/art/trees.rs); the engine cook makes the KTX2.
import math
import os
import sys

import bpy
import numpy as np

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, OUT, ONLY = argv[0], argv[1], set(argv[2:])
VEG = os.path.join(SRC, "models", "vegetation")

W, H = 1024, 2048
TREE_W = 704
BR_W, BR_H = W - TREE_W, H // 3

# set, model, object, alpha maps (material -> file), branch bands (fraction of the crown
# height, sector angle in degrees), slice half-thickness (m) and half-angle (rad), snow variant,
# options: `height` of the atlas (2048 for tall trees, 1024 for wide ones), `clusters`: the
# three small pictures are round leaf clumps seen from the side (broadleaf) instead of branch
# slices from above (conifers); `no_trunk`: the whole-tree picture leaves the trunk out (the
# game draws its own, e.g. a white birch trunk).
TREES = [
    ("card_fir", "fir_tree_01", "fir_tree_01_a_LOD0", {"fir_tree_01_twig": "fir_tree_01_twig_alpha_2k.jpg"}, [(0.22, 20), (0.42, 140), (0.62, 260)], 0.55, 0.42, True),
    ("card_birch", "tree_small_02", "tree_small_02_LOD0", {"tree_small_02_leaves": "tree_small_02_leaves_alpha_2k.jpg"}, [(0.5, 30), (0.68, 150), (0.84, 270)], 0.75, 0.6, False, {"height": 1024, "clusters": True, "no_trunk": True}),
    ("card_pine", "pine_tree_01", "pine_tree_01_a_LOD0", {"pine_tree_01_twig": "pine_tree_01_twig_alpha_2k.jpg"}, [(0.62, 30), (0.74, 150), (0.5, 270)], 0.8, 0.55, False),
]


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    s = bpy.context.scene
    s.render.engine = "CYCLES"
    s.cycles.device = "CPU"
    s.cycles.samples = 32
    s.cycles.use_denoising = False
    s.cycles.max_bounces = 0
    s.cycles.transparent_max_bounces = 64
    s.render.film_transparent = True
    s.render.image_settings.file_format = "PNG"
    s.render.image_settings.color_mode = "RGBA"
    s.render.image_settings.color_depth = "8"
    s.view_settings.view_transform = "Standard"
    s.view_settings.look = "None"
    s.display_settings.display_device = "sRGB"
    s.world = bpy.data.worlds.new("w")
    s.world.color = (0, 0, 0)
    return s


def principled(mat):
    for n in mat.node_tree.nodes:
        if n.type == "BSDF_PRINCIPLED":
            return n
    return None


def upstream_image(socket):
    """The first image texture node feeding `socket` (through mix/mapping nodes)."""
    todo = [l.from_node for l in socket.links]
    while todo:
        n = todo.pop(0)
        if n.type == "TEX_IMAGE":
            return n
        for i in n.inputs:
            todo.extend(l.from_node for l in i.links)
    return None


def to_emission(mat, alpha_path, mode):
    """Rewires a glTF-imported material in place: emit its base colour (times a little ambient
    occlusion) or the camera-space normal; cut out by `alpha_path` (same UV/mapping as the
    base colour) when given. Texture transforms survive because the existing nodes are reused."""
    nt = mat.node_tree
    bsdf = principled(mat)
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL")
    base = bsdf.inputs["Base Color"].links[0].from_socket if bsdf and bsdf.inputs["Base Color"].links else None
    img = upstream_image(bsdf.inputs["Base Color"]) if base is not None else None
    emit = nt.nodes.new("ShaderNodeEmission")
    if mode == "albedo":
        ao = nt.nodes.new("ShaderNodeAmbientOcclusion")
        ao.samples = 8
        ao.inputs["Distance"].default_value = 1.4
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        if base is not None:
            nt.links.new(base, mul.inputs[6])
        else:
            mul.inputs[6].default_value = bsdf.inputs["Base Color"].default_value if bsdf else (0.3, 0.25, 0.2, 1)
        # Interior needles darker (the crown shades itself), never black: 0.3 .. 1.
        lift = nt.nodes.new("ShaderNodeMapRange")
        lift.inputs["To Min"].default_value = 0.3
        nt.links.new(ao.outputs["AO"], lift.inputs["Value"])
        nt.links.new(lift.outputs["Result"], mul.inputs[7])
        nt.links.new(mul.outputs[2], emit.inputs["Color"])
    else:
        geo = nt.nodes.new("ShaderNodeNewGeometry")
        vt = nt.nodes.new("ShaderNodeVectorTransform")
        vt.vector_type = "NORMAL"
        vt.convert_from = "WORLD"
        vt.convert_to = "CAMERA"
        nt.links.new(geo.outputs["Normal"], vt.inputs["Vector"])
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "VECTOR"
        nt.links.new(geo.outputs["Backfacing"], mix.inputs["Factor"])
        nt.links.new(vt.outputs["Vector"], mix.inputs[4])
        neg = nt.nodes.new("ShaderNodeVectorMath")
        neg.operation = "SCALE"
        neg.inputs["Scale"].default_value = -1.0
        nt.links.new(vt.outputs["Vector"], neg.inputs[0])
        nt.links.new(neg.outputs["Vector"], mix.inputs[5])
        enc = nt.nodes.new("ShaderNodeVectorMath")
        enc.operation = "MULTIPLY_ADD"
        enc.inputs[1].default_value = (0.5, 0.5, 0.5)
        enc.inputs[2].default_value = (0.5, 0.5, 0.5)
        nt.links.new(mix.outputs[1], enc.inputs[0])
        nt.links.new(enc.outputs["Vector"], emit.inputs["Color"])
    shader = emit.outputs["Emission"]
    if alpha_path:
        a = nt.nodes.new("ShaderNodeTexImage")
        a.image = bpy.data.images.load(alpha_path, check_existing=True)
        a.image.colorspace_settings.name = "Non-Color"
        if img is not None and img.inputs["Vector"].links:
            nt.links.new(img.inputs["Vector"].links[0].from_socket, a.inputs["Vector"])
        ramp = nt.nodes.new("ShaderNodeMath")
        ramp.operation = "GREATER_THAN"
        ramp.inputs[1].default_value = 0.5
        nt.links.new(a.outputs["Color"], ramp.inputs[0])
        tr = nt.nodes.new("ShaderNodeBsdfTransparent")
        mx = nt.nodes.new("ShaderNodeMixShader")
        nt.links.new(ramp.outputs["Value"], mx.inputs["Fac"])
        nt.links.new(tr.outputs["BSDF"], mx.inputs[1])
        nt.links.new(emit.outputs["Emission"], mx.inputs[2])
        shader = mx.outputs["Shader"]
    for l in list(out.inputs["Surface"].links):
        nt.links.remove(l)
    nt.links.new(shader, out.inputs["Surface"])


def submesh(obj, keep, name):
    """A new object holding only the polygons where `keep` is true (numpy, no bmesh: the scans
    have millions of faces)."""
    me = obj.data
    npoly = len(me.polygons)
    ls = np.zeros(npoly, np.int64)
    lt = np.zeros(npoly, np.int64)
    mi = np.zeros(npoly, np.int64)
    me.polygons.foreach_get("loop_start", ls)
    me.polygons.foreach_get("loop_total", lt)
    me.polygons.foreach_get("material_index", mi)
    lv = np.zeros(len(me.loops), np.int64)
    me.loops.foreach_get("vertex_index", lv)
    co = np.zeros(len(me.vertices) * 3, np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    uv = np.zeros(len(me.loops) * 2, np.float32)
    me.uv_layers.active.data.foreach_get("uv", uv)
    uv = uv.reshape(-1, 2)
    idx = np.nonzero(keep)[0]
    loops = np.concatenate([np.arange(ls[i], ls[i] + lt[i]) for i in idx]) if len(idx) else np.zeros(0, np.int64)
    used, inv = np.unique(lv[loops], return_inverse=True)
    new = bpy.data.meshes.new(name)
    new.vertices.add(len(used))
    new.vertices.foreach_set("co", co[used].reshape(-1))
    new.loops.add(len(loops))
    new.loops.foreach_set("vertex_index", inv.astype(np.int32))
    new.polygons.add(len(idx))
    starts = np.concatenate([[0], np.cumsum(lt[idx])[:-1]]).astype(np.int32)
    new.polygons.foreach_set("loop_start", starts)
    new.polygons.foreach_set("material_index", mi[idx].astype(np.int32))
    new.uv_layers.new(name="UVMap")
    new.uv_layers[0].data.foreach_set("uv", uv[loops].reshape(-1))
    # glTF vertex colours (COLOR_0) multiply the base colour: keep them.
    for a in me.attributes:
        if a.domain == "CORNER" and a.data_type in ("BYTE_COLOR", "FLOAT_COLOR"):
            c = np.zeros(len(me.loops) * 4, np.float32)
            a.data.foreach_get("color", c)
            na = new.attributes.new(a.name, a.data_type, "CORNER")
            na.data.foreach_set("color", c.reshape(-1, 4)[loops].reshape(-1))
    for m in me.materials:
        new.materials.append(m)
    new.update()
    o = bpy.data.objects.new(name, new)
    o.matrix_world = obj.matrix_world.copy()
    bpy.context.scene.collection.objects.link(o)
    return o


def camera(s, loc, rot, scale, rx, ry):
    cd = bpy.data.cameras.new("cam")
    cd.type = "ORTHO"
    cd.ortho_scale = scale
    cd.clip_end = 200
    cam = bpy.data.objects.new("cam", cd)
    s.collection.objects.link(cam)
    cam.location = loc
    cam.rotation_euler = rot
    s.camera = cam
    s.render.resolution_x = rx
    s.render.resolution_y = ry
    return cam


def render_passes(s, objs, alphas, folder, path):
    """Renders `path`_albedo.png and `path`_normal.png of the visible objects."""
    for mode in ("albedo", "normal"):
        for o in objs:
            for k, slot in enumerate(o.material_slots):
                src = bpy.data.materials[slot.material.name.split("#")[0]]
                m = src.copy()
                m.name = f"{src.name}#{mode}#{o.name}"
                a = alphas.get(src.name)
                to_emission(m, os.path.join(folder, "textures", a) if a else None, mode)
                slot.link = "OBJECT"
                slot.material = m
        s.render.filepath = f"{path}_{mode}.png"
        bpy.ops.render.render(write_still=True)


def load_px(path):
    img = bpy.data.images.load(path)
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)[::-1]  # top row first
    bpy.data.images.remove(img)
    return a


def save_px(path, a):
    h, w, _ = a.shape
    img = bpy.data.images.new("out", w, h, alpha=True)
    img.pixels[:] = a[::-1].reshape(-1)
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def dilate(rgb, known, steps=32):
    rgb = rgb.copy()
    for _ in range(steps):
        acc = np.zeros_like(rgb)
        cnt = np.zeros(known.shape, np.float32)
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
            k = np.roll(np.roll(known, dy, 0), dx, 1)
            acc += np.roll(np.roll(rgb, dy, 0), dx, 1) * k[..., None]
            cnt += k
        grow = (~known) & (cnt > 0)
        rgb[grow] = acc[grow] / cnt[grow][:, None]
        known = known | grow
    rgb[~known] = rgb[known].mean(0) if known.any() else 0.5
    return rgb


def build(name, model, obj_name, alphas, bands, dz, da, snow, opts=None):
    global H, BR_H
    opts = opts or {}
    H = opts.get("height", 2048)
    BR_H = H // 3
    clusters = opts.get("clusters", False)
    s = reset()
    folder = os.path.join(VEG, model)
    bpy.ops.import_scene.gltf(filepath=os.path.join(folder, f"{model}_2k.gltf"))
    tree = bpy.data.objects[obj_name]
    for o in list(bpy.data.objects):
        if o != tree:
            bpy.data.objects.remove(o, do_unlink=True)
    tree.location = (0, 0, 0)
    bpy.context.view_layer.update()
    me = tree.data
    co = np.zeros(len(me.vertices) * 3, np.float32)
    me.vertices.foreach_get("co", co)
    co = co.reshape(-1, 3)
    lo, hi = co.min(0), co.max(0)
    # Trunk axis at the base: the mean of the lowest metre.
    base = co[co[:, 2] < lo[2] + 1.0]
    ax, ay = base[:, 0].mean(), base[:, 1].mean()
    tmp = os.path.join(OUT, name, "_tmp")
    os.makedirs(tmp, exist_ok=True)
    regions = {}

    # Per-face data for the cuts below.
    npoly = len(me.polygons)
    cen = np.zeros(npoly * 3, np.float32)
    me.polygons.foreach_get("center", cen)
    cen = cen.reshape(-1, 3)
    mi = np.zeros(npoly, np.int64)
    me.polygons.foreach_get("material_index", mi)
    trunk_mats = [k for k, m in enumerate(me.materials) if "trunk" in m.name]
    not_trunk = ~np.isin(mi, trunk_mats)

    # 1) Whole tree, side view (looking along +Y). The frame keeps the full model's extent so
    # the picture's base is the ground either way.
    half = max(abs(lo[0] - ax), abs(hi[0] - ax))
    th = hi[2] - lo[2]
    scale = max(th, 2 * half * H / TREE_W) * 1.01
    camera(s, (ax, lo[1] - 20, lo[2] + scale / 2), (math.pi / 2, 0, 0), scale, TREE_W, H)
    whole = tree
    if opts.get("no_trunk"):
        whole = submesh(tree, not_trunk, "crown_only")
        tree.hide_render = True
    render_passes(s, [whole], alphas, folder, os.path.join(tmp, "tree"))
    whole.hide_render = True
    # uv of the tree's extent inside its region (x centred on the trunk, base at the bottom).
    px = scale / H  # metres per pixel
    regions["tree"] = (0.5 * TREE_W - half / px, H - th / px, 0.5 * TREE_W + half / px, H, 2 * half, th)

    # 2) Branch slices, from above, trunk end at the bottom of each cell.
    ang = np.arctan2(cen[:, 1] - ay, cen[:, 0] - ax)
    rad = np.hypot(cen[:, 0] - ax, cen[:, 1] - ay)
    # Crown: where the twigs start.
    twig_mats = [k for k, m in enumerate(me.materials) if m.name in alphas]
    tz = cen[np.isin(mi, twig_mats), 2]
    c0, c1 = np.percentile(tz, 2), np.percentile(tz, 99)
    tree.hide_render = True
    branches = []
    for k, (frac, deg) in enumerate(bands):
        zc = c0 + (c1 - c0) * frac
        # The densest sector of the band (needle faces), searched within ±60° of `deg` so the
        # three slices come from different sides.
        band = np.isin(mi, twig_mats) & (np.abs(cen[:, 2] - zc) < dz) & (rad > 0.12)
        best = (-1, 0.0)
        for step in range(-12, 13):
            a = math.radians(deg + step * 5)
            n = int(np.count_nonzero(band & (np.abs(np.angle(np.exp(1j * (ang - a)))) < da)))
            best = max(best, (n, a))
        a = best[1]
        d = np.angle(np.exp(1j * (ang - a)))
        dirv = np.array([math.cos(a), math.sin(a)])
        if clusters:
            # A round clump around the band's median radius on that side, seen from outside.
            sel = band & (np.abs(d) < da)
            rc = float(np.median(rad[sel])) if sel.any() else 0.5
            p = np.array([ax + dirv[0] * rc, ay + dirv[1] * rc, zc])
            keep = not_trunk & (np.linalg.norm(cen - p, axis=1) < dz)
            o = submesh(tree, keep, f"branch{k}")
            side = np.array([-dirv[1], dirv[0]])
            rel = cen[keep] - p
            # Face centres understate the extent of the leaf cards: pad.
            uh = max(np.abs(rel[:, :2] @ side).max(), np.abs(rel[:, 2]).max(), 0.3) + 0.2
            sc = 2 * uh * max(1.0, BR_H / BR_W) * 1.02
            cam = camera(s, (p[0] + dirv[0] * 20, p[1] + dirv[1] * 20, p[2]), (math.pi / 2, 0, a + math.pi / 2), sc, BR_W, BR_H)
            for oo in bpy.data.objects:
                if oo.name.startswith("branch"):
                    oo.hide_render = oo is not o
            render_passes(s, [o], alphas, folder, os.path.join(tmp, f"branch{k}"))
            bpx = sc / max(BR_H, BR_W)
            cx, cy = TREE_W + BR_W / 2, k * BR_H + BR_H / 2
            regions[f"branch{k}"] = (cx - uh / bpx, cy - uh / bpx, cx + uh / bpx, cy + uh / bpx, 2 * uh, 2 * uh)
            bpy.data.objects.remove(cam, do_unlink=True)
            branches.append(o)
            continue
        keep = not_trunk & (np.abs(cen[:, 2] - zc) < dz) & (np.abs(d) < da) & (rad > 0.12)
        o = submesh(tree, keep, f"branch{k}")
        side = np.array([-dirv[1], dirv[0]])
        pts = cen[keep][:, :2] - [ax, ay]
        u = pts @ side
        v = pts @ dirv
        rmax = max(v.max(), 0.5) + 0.2
        uh = max(np.abs(u).max(), 0.3) + 0.2
        sc = max(rmax, 2 * uh * BR_H / BR_W) * 1.02
        # Camera above, looking down; image up = outwards (radial direction).
        cx, cy = ax + dirv[0] * sc / 2, ay + dirv[1] * sc / 2
        cam = camera(s, (cx, cy, hi[2] + 5), (0, 0, a - math.pi / 2), sc, BR_W, BR_H)
        for oo in bpy.data.objects:
            if oo.name.startswith("branch"):
                oo.hide_render = oo is not o
        render_passes(s, [o], alphas, folder, os.path.join(tmp, f"branch{k}"))
        bpx = sc / BR_H
        y0 = k * BR_H
        regions[f"branch{k}"] = (TREE_W + BR_W / 2 - uh / bpx, y0 + BR_H - rmax / bpx, TREE_W + BR_W / 2 + uh / bpx, y0 + BR_H, 2 * uh, rmax)
        bpy.data.objects.remove(cam, do_unlink=True)
        branches.append(o)

    # 3) Atlas.
    alb = np.zeros((H, W, 4), np.float32)
    nor = np.zeros((H, W, 4), np.float32)
    alb[:, :TREE_W] = load_px(os.path.join(tmp, "tree_albedo.png"))
    nor[:, :TREE_W] = load_px(os.path.join(tmp, "tree_normal.png"))
    up = np.zeros((H, W), np.float32)  # world-up component of the normal, for snow
    for k in range(len(bands)):
        a = load_px(os.path.join(tmp, f"branch{k}_albedo.png"))
        n = load_px(os.path.join(tmp, f"branch{k}_normal.png"))
        alb[k * BR_H:(k + 1) * BR_H, TREE_W:] = a
        nor[k * BR_H:(k + 1) * BR_H, TREE_W:] = n
    alpha = alb[..., 3]
    na = np.maximum(alpha[..., None], 1e-3)
    rgb = np.where(alpha[..., None] > 1e-3, alb[..., :3] / na, 0)
    known = alpha > 0.5
    rgb = dilate(np.clip(rgb, 0, 1), known)
    out = os.path.join(OUT, name)
    save_px(os.path.join(out, "color.png"), np.dstack([rgb, alpha]))
    n = np.where(alpha[..., None] > 1e-3, (nor[..., :3] * 2 - 1) / na, 0)
    # Side view: world up is the camera's y; top views: the camera's z.
    up[:, :TREE_W] = n[:, :TREE_W, 1]
    up[:, TREE_W:] = n[:, TREE_W:, 1 if clusters else 2]
    n = 0.55 * n + 0.45 * np.array([0, 0, 1], np.float32)
    n /= np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    n = dilate(n * 0.5 + 0.5, known)
    save_px(os.path.join(out, "normal.png"), np.dstack([n, np.ones_like(alpha)]))
    with open(os.path.join(out, "frame.txt"), "w") as f:
        f.write("# region u0 v0 u1 v1 (uv, v down) width height (m)\n")
        for key, (x0, y0, x1, y1, wm, hm) in regions.items():
            f.write(f"{key} {x0 / W:.4f} {y0 / H:.4f} {x1 / W:.4f} {y1 / H:.4f} {wm:.3f} {hm:.3f}\n")
        # Where the needles start, as a share of the tree's height (branch cards go above it).
        f.write(f"crown {(c0 - lo[2]) / th:.4f}\n")
    if snow:
        rng = np.random.default_rng(7)
        speck = rng.random(up.shape) * 0.25
        k = np.clip((up + speck - 0.55) / 0.35, 0, 1)[..., None] * 0.7
        white = np.array([0.86, 0.88, 0.92], np.float32)
        sdir = out + "_snow"
        os.makedirs(sdir, exist_ok=True)
        save_px(os.path.join(sdir, "color.png"), np.dstack([rgb * (1 - k) + white * k, alpha]))
        save_px(os.path.join(sdir, "normal.png"), np.dstack([n, np.ones_like(alpha)]))
        with open(os.path.join(out, "frame.txt")) as src, open(os.path.join(sdir, "frame.txt"), "w") as dst:
            dst.write(src.read())
    for f in os.listdir(tmp):
        os.remove(os.path.join(tmp, f))
    os.rmdir(tmp)
    print("tree card", name, {k: tuple(round(x, 3) for x in v) for k, v in regions.items()})


for t in TREES:
    if not ONLY or t[0] in ONLY:
        build(*t)
