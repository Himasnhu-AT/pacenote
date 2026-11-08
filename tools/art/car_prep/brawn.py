# Prep for the "brawn" body (a 1970 muscle coupe, Sketchfab CC BY, see SOURCES.md): one
# merged mesh with one weathered PBR atlas (texture/New folder/ next to the FBX).
# Run by import_sketchfab.py --profile brawn. Steps: wire the atlas (base colour only; the
# metallic, roughness and normal maps are left out for size); blank the "SS" printed on a trim piece;
# split into loose parts and sort them by the atlas colour under them: red parts are body
# panels ("BodyPaint", plain Paint after --paint), see-through faces (opacity map) "Glass",
# the rest "Fittings" (the atlas). Badges and plates (modelled) go with the profile's --cut.
import os

import bpy
import numpy as np

DIR = os.path.join(os.path.dirname(os.path.abspath(opts["src"])), "texture", "New folder")


def load(name, data=False):
    im = bpy.data.images.load(os.path.join(DIR, name))
    if data:
        im.colorspace_settings.name = "Non-Color"
    return im


def pixels(im):
    w, h = im.size
    px = np.empty(w * h * 4, dtype=np.float32)
    im.pixels.foreach_get(px)
    return px.reshape(h, w, 4)  # row 0 = the bottom of the image (UV v = 0)


base = load("00_BaseColor_Merged.png")
opac = load("00_Opacity_Merged.png", True)
W, H = base.size
B = pixels(base)
# The "SS" printed on a trim piece (atlas pixels x 2715..2775, y 715..810 from the top):
# painted over with the piece's own grey.
y0, y1 = H - 810, H - 715
B[y0:y1, 2715:2775, :3] = B[H - 900, 2722, :3]
B[:, :, 3] = 1.0  # opaque (glass is its own material): written as a JPEG
base.pixels.foreach_set(B.ravel())
base.pack()
O = pixels(opac)[:, :, 0]

body = max(meshes, key=lambda o: len(o.data.polygons))
trim = body.data.materials[0]
trim.name = "Fittings"
nt = trim.node_tree
for n in [n for n in nt.nodes if n.type in ("TEX_IMAGE", "NORMAL_MAP")]:
    nt.nodes.remove(n)
bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
# Base colour only: the metallic / roughness pair would add a 5 MB PNG for little in the game.
bsdf.inputs["Metallic"].default_value = 0.0
bsdf.inputs["Roughness"].default_value = 0.6
for img, sock, data in ((base, "Base Color", False),):
    t = nt.nodes.new("ShaderNodeTexImage")
    t.image = img
    nt.links.new(t.outputs["Color"], bsdf.inputs[sock])
    if sock == "Base Color":
        nt.nodes.active = t
for sock in ("Alpha",):
    for l in list(bsdf.inputs[sock].links):
        nt.links.remove(l)
    bsdf.inputs[sock].default_value = 1.0
paint = bpy.data.materials.new("BodyPaint")
glass = bpy.data.materials.new("Glass")
for m in (paint, glass):
    m.use_nodes = True
    body.data.materials.append(m)

# Per face: the atlas under its UV centre.
me = body.data
n = len(me.polygons)
uv = np.empty(len(me.loops) * 2, dtype=np.float32)
me.uv_layers.active.data.foreach_get("uv", uv)
uv = uv.reshape(-1, 2)
start = np.empty(n, dtype=np.int64)
count = np.empty(n, dtype=np.int64)
me.polygons.foreach_get("loop_start", start)
me.polygons.foreach_get("loop_total", count)
sums = np.add.reduceat(uv, start, axis=0)
c = sums / count[:, None]
px = np.clip((c[:, 0] % 1.0) * W, 0, W - 1).astype(np.int64)
py = np.clip((c[:, 1] % 1.0) * H, 0, H - 1).astype(np.int64)
col = B[py, px, :3]
r, g, b = col[:, 0], col[:, 1], col[:, 2]
red = (r > 0.15) & (g < 0.5 * r) & (b < 0.5 * r)
see = O[py, px] < 0.5
area = np.empty(n, dtype=np.float32)
me.polygons.foreach_get("area", area)

# Loose parts (union-find over the faces' vertices).
vs = np.empty(len(me.loops), dtype=np.int64)
me.loops.foreach_get("vertex_index", vs)
parent = np.arange(len(me.vertices))


def find(a):
    while parent[a] != a:
        parent[a] = parent[parent[a]]
        a = parent[a]
    return a


face_of_loop = np.repeat(np.arange(n), count)
first = vs[start][face_of_loop]
for a, bb in zip(first, vs):
    ra, rb = find(a), find(bb)
    if ra != rb:
        parent[ra] = rb
root = np.array([find(v) for v in vs[start]])
_, part = np.unique(root, return_inverse=True)
np_ = part.max() + 1
red_area = np.bincount(part, weights=area * red, minlength=np_)
all_area = np.bincount(part, weights=area, minlength=np_)
painted = (red_area / np.maximum(all_area, 1e-12)) > 0.45
mat = np.zeros(n, dtype=np.int64)
mat[painted[part]] = 1
mat[see] = 2
# Wheels: the four tyres (round, ~1 unit across, low), and every part inside a tyre's box
# (rim, drum, nuts) get "Wheel" (the atlas too): named so the converter finds them.
co = np.empty(len(me.vertices) * 3, dtype=np.float32)
me.vertices.foreach_get("co", co)
co = co.reshape(-1, 3)
pv = part[face_of_loop]
lo = np.full((np_, 3), 1e9)
hi = np.full((np_, 3), -1e9)
np.minimum.at(lo, pv, co[vs])
np.maximum.at(hi, pv, co[vs])
size, mid = hi - lo, (hi + lo) / 2
zmin = co[:, 2].min()
tyres = [i for i in range(np_) if 0.8 < size[i, 1] < 1.2 and abs(size[i, 1] - size[i, 2]) < 0.05 and size[i, 0] < 0.45 and mid[i, 2] - zmin < 0.7]
corners = {(bool(mid[i, 0] > mid[:, 0].mean()), bool(mid[i, 1] > 0)) for i in tyres}
print(f"tyre-like parts: {len(tyres)} at {len(corners)} corners")
assert len(corners) == 4, "expected tyres at four corners"
wheel = np.zeros(np_, dtype=bool)
for t in tyres:
    a, b = lo[t] - 0.03, hi[t] + 0.03
    wheel |= np.all((lo >= a) & (hi <= b), axis=1)
mat[wheel[part]] = 3
wt = trim.copy()
wt.name = "Wheel"
me.materials.append(wt)

# Badges and plates (the source's frame: nose towards -y, middle at x 0.16): every loose part
# whose middle lies in a box and which is smaller than the box's size limit is dropped.
# Exhaust tips: "Exhaust" (the atlas), so the converter prints the tip.
DROP = [
    ((0.0, 0.33), (-3.45, -3.28), (0.92, 1.08), 0.16),    # grille "SS" (two letters)
    ((-0.62, -0.39), (4.33, 4.41), (0.87, 0.97), 0.12),   # rear bumper "SS"
    ((-0.80, -0.45), (4.25, 4.36), (1.15, 1.28), 0.26),   # boot lid script
    ((0.0, 0.32), (-3.40, -3.28), (0.45, 0.75), 0.52),    # front plate
    ((0.0, 0.32), (4.26, 4.36), (0.68, 0.98), 0.52),      # rear plate
    ((-1.45, -1.30), (-1.25, -1.04), (0.88, 1.0), 0.1),   # front fender "SS", both sides
    ((1.62, 1.78), (-1.25, -1.04), (0.88, 1.0), 0.1),
]
EXHAUST = [((-0.85, -0.55), (3.75, 4.3), (0.45, 0.65)), ((0.87, 1.17), (3.75, 4.3), (0.45, 0.65))]


def inside(i, box):
    return all(box[k][0] <= mid[i, k] <= box[k][1] for k in range(3))


drop = np.array([any(inside(i, b[:3]) and size[i].max() < b[3] for b in DROP) for i in range(np_)])
exh = np.array([any(inside(i, b) and size[i].max() < 0.45 for b in EXHAUST) for i in range(np_)])
print(f"dropped {drop.sum()} badge / plate parts; {exh.sum()} exhaust parts")
mat[exh[part]] = 4
mat[drop[part]] = 5
ex = trim.copy()
ex.name = "Exhaust"
me.materials.append(ex)
me.materials.append(bpy.data.materials.new("Drop"))
me.polygons.foreach_set("material_index", mat)
me.update()
print(f"brawn prep: {np_} loose parts, {painted.sum()} painted, {wheel.sum()} wheel parts; faces: paint {(mat == 1).sum()}, glass {(mat == 2).sum()}, wheel {(mat == 3).sum()}, trim {(mat == 0).sum()}, dropped {(mat == 5).sum()}")

# One object per material, so the decimation can treat them apart.
bpy.ops.object.select_all(action="DESELECT")
body.select_set(True)
bpy.context.view_layer.objects.active = body
bpy.ops.object.mode_set(mode="EDIT")
bpy.ops.mesh.separate(type="MATERIAL")
bpy.ops.object.mode_set(mode="OBJECT")
for o in list(bpy.context.selected_objects):
    m = next(iter({o.data.materials[p.material_index].name for p in o.data.polygons}), "x")
    if m == "Drop":
        bpy.data.objects.remove(o, do_unlink=True)
        continue
    o.name = o.data.name = m + "Part"
    o.data.materials.clear()
    o.data.materials.append(bpy.data.materials[m])
    o.data.polygons.foreach_set("material_index", np.zeros(len(o.data.polygons), dtype=np.int64))
