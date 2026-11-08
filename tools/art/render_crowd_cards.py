# Renders the spectator impostor atlas from Fab's "Models of people x 10" scans (Blender, background):
#   memgate -- blender -b --factory-startup -P tools/art/render_crowd_cards.py -- <assets_src> <assets/textures> [--test]
#
# The scans are all SEATED people (one sits cross-legged on the ground). Each chair-sitter gets an
# original, plain cooler box as a seat (rally fans sit on eskies), sized to their seat height.
# Every person is rendered orthographically from 8 yaw angles at one fixed scale (MPP metres per
# pixel) into a 2048² atlas: row = person, column = angle. Outputs in <assets/textures>/card_crowd:
# color.png (base colour times a little baked AO, alpha cut-out, colour dilated under the alpha),
# normal.png (camera/card space, OpenGL, from the scans' normal maps), frame.txt (layout, heights);
# and card_crowd_mask/color.png: R = clothing (tint it per instance), G = the seat box (tint it
# separately), black elsewhere (skin, hair, background). The engine cook turns both into KTX2.
import math
import os
import sys

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
SRC, OUT = argv[0], argv[1]
TEST = "--test" in argv
BLEND = os.path.join(SRC, "fab_batch1", "blend", "blend", "untitled.blend")
TEXDIR = os.path.join(os.path.dirname(BLEND), "textures")

W = H = 2048
ANGLES = 8
CELL_W, CELL_H = 256, 204  # 8 columns x 10 rows (2040 px used, 8 px spare at the bottom)
MPP = 0.007                # metres per pixel, the same for every person
FOOT = 4                   # ground line: px above the cell's bottom edge
PREFIX = "Models of people x 10 01-"

# Scan object, extra objects merged into it, seated height (top of the scan, m), seat height in
# scan units (the scans are 10 units tall; measured from side views), seat box colour (None = sits
# on the ground), head-top height in scan units (for the clothing mask), note.
PEOPLE = [
    ("001", (), 1.25, 3.6, 0, 10.0, "woman, floral blouse, jeans, one foot up"),
    ("002", (), 1.26, 3.5, 1, 10.0, "woman, mint top, holding a mug"),
    ("003", (), 1.22, 3.8, 2, 10.0, "woman, burgundy coat, scarf, backpack"),
    ("004", (), 1.34, 3.2, 3, 10.0, "man, grey tee, leaning on his knees"),
    ("005", (), 1.26, 3.5, 1, 10.0, "woman, grey top, jeans"),
    ("006", (), 1.15, None, None, 7.4, "woman, cross-legged on the ground, stretching"),
    ("007", (), 1.28, 3.2, 0, 10.0, "woman, red suit"),
    ("008", (), 1.24, 2.9, 2, 10.0, "man, white tee, leaning back"),
    ("010", ("009",), 1.30, 3.8, 3, 10.0, "man, black sweater, phone"),
    ("011", (), 1.33, 3.5, 0, 10.0, "man, denim shirt, reading"),
]
# Cooler box colours (body, lid): plain, no logos.
# Linear base colours.
BOXES = [((0.62, 0.62, 0.60), (0.06, 0.14, 0.30)), ((0.40, 0.07, 0.05), (0.62, 0.62, 0.60)),
         ((0.06, 0.15, 0.28), (0.60, 0.60, 0.58)), ((0.16, 0.20, 0.11), (0.09, 0.10, 0.09))]
# Shallower boxes where a leg tucks back under the seat.
BOX_DEPTH = {"001": 0.24}


def scene():
    s = bpy.context.scene
    for o in list(bpy.data.objects):
        if o.type in ("CAMERA", "LIGHT"):
            bpy.data.objects.remove(o, do_unlink=True)
    s.render.engine = "CYCLES"
    s.cycles.device = "CPU"
    s.cycles.samples = 32
    s.cycles.use_denoising = False
    s.cycles.max_bounces = 0
    s.render.film_transparent = True
    s.render.resolution_percentage = 100
    s.render.image_settings.file_format = "PNG"
    s.render.image_settings.color_mode = "RGBA"
    s.render.image_settings.color_depth = "8"
    s.view_settings.view_transform = "Standard"
    s.view_settings.look = "None"
    s.display_settings.display_device = "sRGB"
    s.world = bpy.data.worlds.new("w")
    s.world.color = (0, 0, 0)
    s.render.resolution_x, s.render.resolution_y = CELL_W, CELL_H
    cd = bpy.data.cameras.new("cam")
    cd.type = "ORTHO"
    cd.ortho_scale = max(CELL_W, CELL_H) * MPP
    cd.clip_end = 100
    cam = bpy.data.objects.new("cam", cd)
    s.collection.objects.link(cam)
    s.camera = cam
    return s, cam


def relink():
    """Point the images at blend/textures (the .blend expects ../maps)."""
    for img in bpy.data.images:
        if not img.filepath:
            continue
        p = os.path.join(TEXDIR, os.path.basename(bpy.path.abspath(img.filepath)))
        if os.path.exists(p):
            if img.packed_file:
                img.unpack(method="REMOVE")
            img.filepath = p
            img.reload()


def bake_object(name, extra):
    """A world-space copy of the scan (plus its accessories), as one mesh."""
    objs = [bpy.data.objects[PREFIX + name]] + [bpy.data.objects[PREFIX + e] for e in extra]
    bm = bmesh.new()
    mats = []
    for o in objs:
        me = o.data.copy()
        me.transform(o.matrix_world)
        # Keep material indices distinct across the merged objects.
        off = len(mats)
        mats += [s.material for s in o.material_slots]
        if off:
            for p in me.polygons:
                p.material_index += off
        bm.from_mesh(me)
        bpy.data.meshes.remove(me)
    me = bpy.data.meshes.new(f"p{name}")
    bm.to_mesh(me)
    bm.free()
    for m in mats:
        me.materials.append(m)
    ob = bpy.data.objects.new(f"p{name}", me)
    bpy.context.scene.collection.objects.link(ob)
    return ob


def box(seat, rear_y, cx, colours, tag, d=0.34):
    """A plain cooler box under the seat: rounded body, overhanging lid, side handles."""
    w = 0.52
    lid = 0.045
    bm = bmesh.new()
    body = bmesh.ops.create_cube(bm, size=1.0)["verts"]
    bmesh.ops.scale(bm, vec=(w, d, seat - lid), verts=body)
    bmesh.ops.translate(bm, vec=(0, 0, (seat - lid) / 2), verts=body)
    top = bmesh.ops.create_cube(bm, size=1.0)["verts"]
    bmesh.ops.scale(bm, vec=(w + 0.02, d + 0.02, lid), verts=top)
    bmesh.ops.translate(bm, vec=(0, 0, seat - lid / 2), verts=top)
    for sx in (-1, 1):
        hv = bmesh.ops.create_cube(bm, size=1.0)["verts"]
        bmesh.ops.scale(bm, vec=(0.03, 0.16, 0.035), verts=hv)
        bmesh.ops.translate(bm, vec=(sx * (w / 2 + 0.012), 0, seat - lid - 0.06), verts=hv)
    bmesh.ops.bevel(bm, geom=list(bm.edges), offset=0.012, segments=2, affect="EDGES")
    me = bpy.data.meshes.new(f"box{tag}")
    bm.to_mesh(me)
    bm.free()
    for i, c in enumerate(colours):
        m = bpy.data.materials.new(f"box{tag}_{i}")
        m.use_nodes = True
        bs = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
        bs.inputs["Base Color"].default_value = (*c, 1)
        me.materials.append(m)
    for p in me.polygons:
        p.material_index = 1 if p.center.z > seat - lid - 0.002 else 0
    ob = bpy.data.objects.new(f"box{tag}", me)
    bpy.context.scene.collection.objects.link(ob)
    # Rear edge just behind the buttocks, centred under the pelvis.
    ob.location = (cx, rear_y - d / 2 + 0.03, 0)
    return ob


def principled(mat):
    return next((n for n in mat.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)


def mode_material(src, mode, head_z, is_box):
    """Emission copy of `src`: albedo (base colour x AO 0.55..1), normal (camera space, with the
    normal map), or id (R = clothing below the head line, G = box)."""
    m = src.copy()
    m.name = f"{src.name}#{mode}"
    nt = m.node_tree
    bsdf = principled(m)
    out = next(n for n in nt.nodes if n.type == "OUTPUT_MATERIAL" and n.is_active_output)
    emit = nt.nodes.new("ShaderNodeEmission")
    if mode == "albedo":
        ao = nt.nodes.new("ShaderNodeAmbientOcclusion")
        ao.samples = 8
        ao.inputs["Distance"].default_value = 0.25
        mul = nt.nodes.new("ShaderNodeMix")
        mul.data_type = "RGBA"
        mul.blend_type = "MULTIPLY"
        mul.inputs["Factor"].default_value = 1.0
        bc = bsdf.inputs["Base Color"]
        if bc.links:
            nt.links.new(bc.links[0].from_socket, mul.inputs[6])
        else:
            mul.inputs[6].default_value = bc.default_value
        lift = nt.nodes.new("ShaderNodeMapRange")
        lift.inputs["To Min"].default_value = 0.55
        nt.links.new(ao.outputs["AO"], lift.inputs["Value"])
        nt.links.new(lift.outputs["Result"], mul.inputs[7])
        nt.links.new(mul.outputs[2], emit.inputs["Color"])
    elif mode == "normal":
        nin = bsdf.inputs["Normal"]
        if nin.links:
            src_n = nin.links[0].from_socket
        else:
            src_n = nt.nodes.new("ShaderNodeNewGeometry").outputs["Normal"]
        vt = nt.nodes.new("ShaderNodeVectorTransform")
        vt.vector_type = "NORMAL"
        vt.convert_from = "WORLD"
        vt.convert_to = "CAMERA"
        nt.links.new(src_n, vt.inputs["Vector"])
        geo = nt.nodes.new("ShaderNodeNewGeometry")
        mix = nt.nodes.new("ShaderNodeMix")
        mix.data_type = "VECTOR"
        nt.links.new(geo.outputs["Backfacing"], mix.inputs["Factor"])
        # Cycles' camera space looks down +Z: flip z so a face towards the camera is +Z (OpenGL).
        flip = nt.nodes.new("ShaderNodeVectorMath")
        flip.operation = "MULTIPLY"
        flip.inputs[1].default_value = (1.0, 1.0, -1.0)
        nt.links.new(vt.outputs["Vector"], flip.inputs[0])
        vt = flip
        neg = nt.nodes.new("ShaderNodeVectorMath")
        neg.operation = "SCALE"
        neg.inputs["Scale"].default_value = -1.0
        nt.links.new(vt.outputs["Vector"], mix.inputs[4])
        nt.links.new(vt.outputs["Vector"], neg.inputs[0])
        nt.links.new(neg.outputs["Vector"], mix.inputs[5])
        enc = nt.nodes.new("ShaderNodeVectorMath")
        enc.operation = "MULTIPLY_ADD"
        enc.inputs[1].default_value = (0.5, 0.5, 0.5)
        enc.inputs[2].default_value = (0.5, 0.5, 0.5)
        nt.links.new(mix.outputs[1], enc.inputs[0])
        nt.links.new(enc.outputs["Vector"], emit.inputs["Color"])
    else:
        if is_box:
            emit.inputs["Color"].default_value = (0, 1, 0, 1)
        else:
            geo = nt.nodes.new("ShaderNodeNewGeometry")
            sep = nt.nodes.new("ShaderNodeSeparateXYZ")
            nt.links.new(geo.outputs["Position"], sep.inputs[0])
            lt = nt.nodes.new("ShaderNodeMath")
            lt.operation = "LESS_THAN"
            lt.inputs[1].default_value = head_z
            nt.links.new(sep.outputs["Z"], lt.inputs[0])
            comb = nt.nodes.new("ShaderNodeCombineColor")
            nt.links.new(lt.outputs[0], comb.inputs["Red"])
            nt.links.new(comb.outputs[0], emit.inputs["Color"])
    for l in list(out.inputs["Surface"].links):
        nt.links.remove(l)
    nt.links.new(emit.outputs["Emission"], out.inputs["Surface"])
    return m


def load_px(path):
    img = bpy.data.images.load(path)
    img.colorspace_settings.name = "Non-Color"  # raw bytes: no colour management either way
    w, h = img.size
    a = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)[::-1]  # top row first
    bpy.data.images.remove(img)
    return a


def save_px(path, a):
    h, w, _ = a.shape
    img = bpy.data.images.new("out", w, h, alpha=True)
    img.colorspace_settings.name = "Non-Color"
    img.pixels[:] = a[::-1].reshape(-1)
    img.filepath_raw = path
    img.file_format = "PNG"
    img.save()
    bpy.data.images.remove(img)


def dilate(rgb, known, steps=24):
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


def skin(rgb):
    """Skin-like pixels (warm, mid saturation): kept out of the clothing mask."""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    mx = rgb.max(-1)
    mn = rgb.min(-1)
    sat = (mx - mn) / np.maximum(mx, 1e-4)
    return (r > g) & (g > b) & (r - g > 0.04) & (r - b > 0.1) & (r > 0.3) & (sat > 0.15) & (sat < 0.62) & (g > 0.18)


def main():
    bpy.ops.wm.open_mainfile(filepath=BLEND)
    relink()
    s, cam = scene()
    scans = [o for o in bpy.data.objects if o.type == "MESH"]
    for o in scans:
        o.hide_render = True
    out = os.path.join(OUT, "card_crowd")
    tmp = os.path.join(out, "_tmp")
    os.makedirs(tmp, exist_ok=True)
    rows = PEOPLE[:2] if TEST else PEOPLE
    alb = np.zeros((H, W, 4), np.float32)
    nor = np.zeros((H, W, 4), np.float32)
    ids = np.zeros((H, W, 4), np.float32)
    info = []
    for r, (name, extra, height, seat_u, box_c, head_u, note) in enumerate(rows):
        p = bake_object(name, extra)
        me = p.data
        co = np.zeros(len(me.vertices) * 3, np.float32)
        me.vertices.foreach_get("co", co)
        co = co.reshape(-1, 3)
        lo, hi = co.min(0), co.max(0)
        k = height / (hi[2] - lo[2])
        # Metres, feet on z = 0, footprint centred later.
        me.transform(Matrix.Translation((0, 0, 0)) @ Matrix.Diagonal((k, k, k, 1)) @ Matrix.Translation((-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
        me.update()
        co = np.zeros(len(me.vertices) * 3, np.float32)
        me.vertices.foreach_get("co", co)
        co = co.reshape(-1, 3)
        objs = [p]
        seat = None
        if seat_u is not None:
            seat = seat_u * k
            band = co[(co[:, 2] > seat) & (co[:, 2] < seat + 0.12)]
            # The rear of the band (the scans face -Y) is the buttocks.
            rear = float(np.percentile(band[:, 1], 98))
            cx = float(np.median(band[band[:, 1] > rear - 0.15][:, 0]))
            objs.append(box(seat, rear, cx, BOXES[box_c], name, BOX_DEPTH.get(name, 0.34)))
        # Pivot: the centre of the footprint of everything drawn.
        pts = [co]
        for o in objs[1:]:
            bb = np.array([o.matrix_world @ Vector(c) for c in o.bound_box])
            pts.append(bb)
        allp = np.concatenate(pts)
        pv = (allp[:, :2].min(0) + allp[:, :2].max(0)) / 2
        reach = float(np.hypot(allp[:, 0] - pv[0], allp[:, 1] - pv[1]).max())
        top = float(allp[:, 2].max())
        head_z = head_u * k - 0.24
        mats = {}
        for mode in ("albedo", "normal", "id"):
            for o in objs:
                is_box = o.name.startswith("box")
                for i, slot in enumerate(o.data.materials):
                    src = slot if "#" not in slot.name else bpy.data.materials[slot.name.split("#")[0]]
                    key = (src.name, mode)
                    if key not in mats:
                        mats[key] = mode_material(src, mode, head_z, is_box)
                    o.data.materials[i] = mats[key]
                o.hide_render = False
            for a in range(ANGLES):
                yaw = math.radians(a * 360 / ANGLES)
                # Camera azimuth `yaw` counter-clockwise (from above) from the person's front (-Y).
                d = Vector((math.sin(yaw), -math.cos(yaw), 0))
                zc = (CELL_H / 2 - FOOT) * MPP
                cam.location = (pv[0] + d.x * 20, pv[1] + d.y * 20, zc)
                cam.rotation_euler = (math.pi / 2, 0, yaw)
                s.cycles.samples = 8 if mode == "id" else 32
                # Data passes are written raw; the albedo goes out as sRGB.
                s.view_settings.view_transform = "Standard" if mode == "albedo" else "Raw"
                f = os.path.join(tmp, f"{name}_{mode}_{a}.png")
                s.render.filepath = f
                bpy.ops.render.render(write_still=True)
                px = load_px(f)
                y0, x0 = r * CELL_H, a * CELL_W
                {"albedo": alb, "normal": nor, "id": ids}[mode][y0:y0 + CELL_H, x0:x0 + CELL_W] = px
                os.remove(f)
        for o in objs:
            o.hide_render = True
        info.append((r, name, height, seat, top, reach, note))
        print(f"crowd {name}: height {height} m, seat {seat}, top {top:.2f}, reach {reach:.2f} m")
        if reach / MPP > CELL_W / 2 - 2 or top / MPP > CELL_H - FOOT - 2:
            print(f"WARNING crowd {name}: does not fit its cell")

    alpha = alb[..., 3]
    na = np.maximum(alpha[..., None], 1e-3)
    known = alpha > 0.5
    rgb = np.where(alpha[..., None] > 1e-3, alb[..., :3] / na, 0)
    rgb = dilate(np.clip(rgb, 0, 1), known)
    save_px(os.path.join(out, "color.png"), np.dstack([rgb, alpha]))
    n = np.where(alpha[..., None] > 1e-3, (nor[..., :3] * 2 - 1) / na, 0)
    n[~known] = (0, 0, 1)
    n /= np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    n = dilate(n * 0.5 + 0.5, known)
    save_px(os.path.join(out, "normal.png"), np.dstack([n, np.ones_like(alpha)]))
    idc = np.where(alpha[..., None] > 1e-3, ids[..., :3] / na, 0)
    body = (idc[..., 0] > 0.5) & known
    cloth = body & ~skin(rgb)  # rgb is sRGB-encoded
    # Majority vote over 5x5 inside the body: patterned fabric (florals) loses its skin-coloured
    # speckles; faces, hands and arms (bigger patches) stay out.
    c = cloth.astype(np.float32)
    acc = np.zeros_like(c)
    for dy in range(-2, 3):
        for dx in range(-2, 3):
            acc += np.roll(np.roll(c, dy, 0), dx, 1)
    cloth = body & (acc > 12.5)
    boxm = (idc[..., 1] > 0.5) & known
    mask = np.zeros((H, W, 4), np.float32)
    mask[..., 0] = cloth
    mask[..., 1] = boxm
    mask[..., 3] = 1
    os.makedirs(out + "_mask", exist_ok=True)
    save_px(os.path.join(out + "_mask", "color.png"), mask)
    os.rmdir(tmp)

    with open(os.path.join(out, "frame.txt"), "w") as f:
        f.write(f"# Spectator impostor atlas (tools/art/render_crowd_cards.py), {W}x{H}.\n")
        f.write(f"atlas {W} {H}\n")
        f.write(f"cell {CELL_W} {CELL_H}\n")
        f.write(f"grid {ANGLES} {len(PEOPLE)}\n")
        f.write(f"mpp {MPP}\n")
        f.write(f"foot {FOOT}\n")
        f.write("# Row r = person r, column a = view angle a*45 deg: the camera sits a*45 deg counter-clockwise\n")
        f.write("# (seen from above) from the person's front, so a=0 is the front, a=2 their left side, a=4 the back.\n")
        f.write("# Cell (a, r) covers uv u0 = a*cell_w/atlas_w, v0 = r*cell_h/atlas_h (v down), size cell/atlas.\n")
        f.write("# In every cell the ground line is `foot` px above the cell's bottom edge and the footprint\n")
        f.write("# centre (rotation pivot) is the cell's horizontal centre; 1 px = mpp metres in all cells, so a\n")
        f.write(f"# quad of {CELL_W * MPP:.3f} x {CELL_H * MPP:.3f} m, bottom edge {FOOT * MPP:.3f} m below the ground, draws it to scale.\n")
        f.write("# Mask (card_crowd_mask/color): R = clothing (tint), G = seat box (tint), black = skin, hair, background.\n")
        f.write("# person row scan height_m seat_m (- = on the ground) top_m reach_m note\n")
        for r, name, height, seat, top, reach, note in info:
            f.write(f"person {r} {name} {height:.2f} {'-' if seat is None else f'{seat:.3f}'} {top:.3f} {reach:.3f} {note}\n")
    print("crowd atlas written", out)


main()
