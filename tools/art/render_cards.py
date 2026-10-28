# Renders foliage card textures from the Poly Haven vegetation models (Blender, background):
#   blender -b --factory-startup -P tools/art/render_cards.py -- <assets_src> <out dir>
# For each card: an orthographic side view of one model, unlit albedo with alpha (color.png)
# and the camera-space normal (normal.png: tangent space of a card facing the camera, OpenGL
# convention). Optionally a snow-dusted variant (upward-facing needles whitened).
# The game builds trees out of these cards (src/art/trees.rs); the cook turns them into KTX2.
import os
import sys

import bpy
import numpy as np

argv = sys.argv[sys.argv.index("--") + 1:]
SRC, OUT = argv[0], argv[1]
VEG = os.path.join(SRC, "models", "vegetation")

FIR = {"fir_sapling_twigs": "fir_sapling_twigs_alpha_2k.jpg"}
PINE = {"pine_sapling_small_twig": "pine_sapling_small_twig_alpha_2k.jpg"}
# name, gltf, object, alpha maps (material name -> alpha image), resolution, copies (rotated
# about the trunk and composited: a denser silhouette), snow variant (`<name>_snow`)
CARDS = [
    ("card_fir_dense", "fir_sapling", "fir_sapling_a", FIR, 1024, 4, True),
    ("card_pine_dense", "pine_sapling_small", "pine_sapling_small_a", PINE, 1024, 4, False),
    ("card_broadleaf", "island_tree_02", "island_tree_02_LOD0", {"island_tree_02_leaves": "island_tree_02_leaves_alpha_2k.jpg"}, 1024, 1, False),
]


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    s = bpy.context.scene
    s.render.engine = "CYCLES"
    s.cycles.device = "CPU"
    s.cycles.samples = 24
    s.cycles.use_denoising = False
    s.cycles.max_bounces = 0
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


def emission_material(name, base_img, alpha_img, mode):
    """mode 'albedo': emit the base colour texture; 'normal': emit the camera-space normal."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    nt = m.node_tree
    for n in list(nt.nodes):
        nt.nodes.remove(n)
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    emit = nt.nodes.new("ShaderNodeEmission")
    if mode == "albedo":
        if base_img is not None:
            t = nt.nodes.new("ShaderNodeTexImage")
            t.image = base_img
            nt.links.new(t.outputs["Color"], emit.inputs["Color"])
    else:
        geo = nt.nodes.new("ShaderNodeNewGeometry")
        vt = nt.nodes.new("ShaderNodeVectorTransform")
        vt.vector_type = "NORMAL"
        vt.convert_from = "WORLD"
        vt.convert_to = "CAMERA"
        nt.links.new(geo.outputs["Normal"], vt.inputs["Vector"])
        # Blender camera space: x right, y up, z towards the viewer (-z forward). Back faces
        # (double-sided cards) get their normal flipped towards the camera.
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
    if alpha_img is not None:
        a = nt.nodes.new("ShaderNodeTexImage")
        a.image = alpha_img
        a.image.colorspace_settings.name = "Non-Color"
        tr = nt.nodes.new("ShaderNodeBsdfTransparent")
        mx = nt.nodes.new("ShaderNodeMixShader")
        # Hard cutout like the game's alpha mask (soft edges come from the cook's mips).
        ramp = nt.nodes.new("ShaderNodeMath")
        ramp.operation = "GREATER_THAN"
        ramp.inputs[1].default_value = 0.5
        nt.links.new(a.outputs["Color"], ramp.inputs[0])
        nt.links.new(ramp.outputs["Value"], mx.inputs["Fac"])
        nt.links.new(tr.outputs["BSDF"], mx.inputs[1])
        nt.links.new(emit.outputs["Emission"], mx.inputs[2])
        shader = mx.outputs["Shader"]
    nt.links.new(shader, out.inputs["Surface"])
    return m


def base_image(mat):
    if mat is None or not mat.use_nodes:
        return None
    for n in mat.node_tree.nodes:
        if n.type == "BSDF_PRINCIPLED":
            link = n.inputs["Base Color"].links
            if link and link[0].from_node.type == "TEX_IMAGE":
                return link[0].from_node.image
    return None


def render(path):
    bpy.context.scene.render.filepath = path
    bpy.ops.render.render(write_still=True)


def card(name, model, obj_name, alphas, res, copies, snow):
    s = reset()
    folder = os.path.join(VEG, model)
    bpy.ops.import_scene.gltf(filepath=os.path.join(folder, f"{model}_2k.gltf"))
    obj = bpy.data.objects[obj_name]
    for o in list(bpy.data.objects):
        if o != obj:
            bpy.data.objects.remove(o, do_unlink=True)
    obj.location = (0, 0, 0)
    objs = [obj]
    for k in range(1, copies):
        c = obj.copy()
        c.data = obj.data
        s.collection.objects.link(c)
        c.rotation_euler = (0, 0, np.pi * k / copies + 0.3 * k)
        f = (0.92, 1.06, 0.97)[k % 3]
        c.scale = (f, f, f * (0.96 + 0.04 * k))
        objs.append(c)
    bpy.context.view_layer.update()
    # Bounds (world) → camera framing: looking along +Y at the XZ plane.
    pts = np.array([o.matrix_world @ v.co for o in objs for v in o.data.vertices])
    lo, hi = pts.min(0), pts.max(0)
    w, h = hi[0] - lo[0], hi[2] - lo[2]
    size = max(w, h) * 1.02
    cam_data = bpy.data.cameras.new("cam")
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = size
    cam = bpy.data.objects.new("cam", cam_data)
    s.collection.objects.link(cam)
    cx, cz = (lo[0] + hi[0]) / 2, (lo[2] + hi[2]) / 2
    cam.location = (cx, lo[1] - 10, cz)
    cam.rotation_euler = (np.pi / 2, 0, 0)
    s.camera = cam
    cam_data.clip_end = 100
    s.render.resolution_x = res
    s.render.resolution_y = res
    imgs = []
    for slot in obj.material_slots:
        m = slot.material
        alpha = None
        if m.name in alphas:
            alpha = bpy.data.images.load(os.path.join(folder, "textures", alphas[m.name]))
        imgs.append((m.name, base_image(m), alpha))
    out = os.path.join(OUT, name)
    os.makedirs(out, exist_ok=True)
    for mode in ("albedo", "normal"):
        for slot, (mname, b, a) in zip(obj.material_slots, imgs):
            slot.material = emission_material(f"{mname}_{mode}", b, a, mode)
        render(os.path.join(out, f"_{mode}.png"))
    # Framing for the game: the model's extent inside the square image (u0, v0, u1, v1; v down).
    with open(os.path.join(out, "frame.txt"), "w") as f:
        u0 = 0.5 + (lo[0] - cx) / size
        u1 = 0.5 + (hi[0] - cx) / size
        v0 = 0.5 - (hi[2] - cz) / size
        v1 = 0.5 - (lo[2] - cz) / size
        f.write(f"{u0:.4f} {v0:.4f} {u1:.4f} {v1:.4f} {w:.4f} {h:.4f}\n")
    finalize(out, snow)
    print("card", name, "size", w, h)


def load_px(path):
    img = bpy.data.images.load(path)
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    bpy.data.images.remove(img)
    return a


def save_px(path, a):
    h, w, _ = a.shape
    img = bpy.data.images.new("out", w, h, alpha=True)
    img.pixels[:] = a.reshape(-1)
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def dilate(rgb, alpha, steps=24):
    """Fills transparent texels with the average of their opaque neighbours (repeatedly), so
    mips and bilinear filtering never pull in the black background."""
    rgb = rgb.copy()
    known = alpha > 0.5
    for _ in range(steps):
        acc = np.zeros_like(rgb)
        cnt = np.zeros(known.shape, np.float32)
        for dy, dx in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)):
            k = np.roll(np.roll(known, dy, 0), dx, 1)
            c = np.roll(np.roll(rgb, dy, 0), dx, 1)
            acc += c * k[..., None]
            cnt += k
        grow = (~known) & (cnt > 0)
        rgb[grow] = acc[grow] / cnt[grow][:, None]
        known = known | grow
    rgb[~known] = rgb[known].mean(0) if known.any() else 0.5
    return rgb


def finalize(out, snow):
    """_albedo.png / _normal.png → color.png (RGBA, dilated) and normal.png (softened); with
    `snow`, also `<out>_snow/` with upward-facing needles dusted white."""
    alb = load_px(os.path.join(out, "_albedo.png"))
    nor = load_px(os.path.join(out, "_normal.png"))
    alpha = alb[..., 3]
    # Blender renders straight alpha with film_transparent; colour is premultiplied by
    # coverage at the edges: un-premultiply.
    rgb = np.where(alpha[..., None] > 1e-3, alb[..., :3] / np.maximum(alpha[..., None], 1e-3), 0)
    rgb = dilate(np.clip(rgb, 0, 1), alpha)
    color = np.dstack([rgb, alpha])
    save_px(os.path.join(out, "color.png"), color)
    n = nor[..., :3] * 2 - 1
    n = np.where(alpha[..., None] > 1e-3, n / np.maximum(alpha[..., None], 1e-3), 0)
    # Half way to the card normal: thin needles have wild normals; keep some relief.
    n = 0.5 * n + 0.5 * np.array([0, 0, 1], np.float32)
    n /= np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    n = dilate(n * 0.5 + 0.5, alpha)
    save_px(os.path.join(out, "normal.png"), np.dstack([n, np.ones_like(alpha)]))
    if snow:
        up = (nor[..., 1] * 2 - 1) / np.maximum(alpha, 1e-3)
        rng = np.random.default_rng(7)
        speck = rng.random(up.shape) * 0.25
        k = np.clip((up + speck - 0.42) / 0.35, 0, 1)[..., None] * 0.85
        white = np.array([0.86, 0.88, 0.92], np.float32)
        srgb = rgb * (1 - k) + white * k
        sdir = out + "_snow"
        os.makedirs(sdir, exist_ok=True)
        save_px(os.path.join(sdir, "color.png"), np.dstack([srgb, alpha]))
        save_px(os.path.join(sdir, "normal.png"), np.dstack([n, np.ones_like(alpha)]))


for c in CARDS:
    card(*c)
