# Builds the game's rally car procedurally (Blender, background): an original compact 3-door
# gravel-rally hatchback with livery, wheels and a cockpit interior.
#   blender -b --factory-startup -P tools/art/build_rallycar.py -- assets/models/rallycar.glb [preview_dir] [--no-bake]
# The cockpit (interior, wheel, levers, gauges) is cockpit.py; mesh helpers are blendkit.py.
#
# Frame (Blender): X right, Y forward (exported as glTF -Z), Z up; origin at the wheelbase's
# hub height, the ground at z = -R. Dimensions follow src/vehicle.rs `Spec` (wheel mounts
# x ±0.76, y +1.25 front / -1.22 rear; body 1.75 x 3.9 m).
#
# Nodes the game uses (src/art/car.rs):
#   Hub<FL|FR|RL|RR>  steer + suspension pivot at the wheel centre (caliper)
#     Spin<..>        tyre, rim, disc; spins about local X
#   SteeringWheel     turns about its column axis (local Blender Y = glTF -Z)
#   Tacho, Speedo     gauge needles, same axis convention
#   Shifter, Handbrake levers, pivoting at their foot on the console (src/art/hands.rs)
#   LightPod          the bonnet lamp pod (hidden in the cockpit and bonnet views)
#   Body, Interior    static meshes
# Materials are single-sided (backface culled): the body shell is invisible from inside, so the
# cockpit sees the interior's own inner skin, and the glass only from outside.
import math
import os
import sys

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cockpit  # noqa: E402
from blendkit import MATS, Mesh, empty, finish, lin, link, material  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
OUT = argv[0] if argv else "assets/models/rallycar.glb"
PREVIEW = argv[1] if len(argv) > 1 and argv[1] != "--no-bake" else None
BAKE = "--no-bake" not in argv

R = 0.325  # tyre radius (src/art/car.rs MODEL_WHEEL_RADIUS)
G = -R  # ground z
# Wheel centres: the sim's track (1.56 m) and wheelbase (2.55 m), centred on the model origin
# (src/art/car.rs places the model at the axles' midpoint).
HUB = {"FL": (-0.78, 1.275), "FR": (0.78, 1.275), "RL": (-0.78, -1.275), "RR": (0.78, -1.275)}
Y_TAIL, Y_NOSE = -1.87, 2.03
NUMBER = "14"
# Driver (left-hand drive): eye at (-0.37, -0.60, 1.12 above ground); see car.rs COCKPIT_EYE.
DRV_X = -0.37



def zg(h):
    """Height above the ground → model z."""
    return G + h


# ---------------------------------------------------------------- livery texture
# Atlas (1024²), v up: right side v∈[0.625,1), left side v∈[0.25,0.625), top view
# (u<0.75, v<0.25), front (0.75≤u<0.875) and back (u≥0.875) below. Sides map u = Y, so both
# sides share stripe positions; only the number is drawn un-mirrored on each.

TEX = 1024
LY0, LY = -2.0, 4.1  # Y range of the side/top maps
LZ = 1.6  # height range of side/front maps
LX0, LX = -0.95, 1.9

WHITE = np.array([0.93, 0.93, 0.91])
NAVY = np.array([0.05, 0.09, 0.24])
ORANGE = np.array([1.0, 0.42, 0.04])
BLACK = np.array([0.03, 0.03, 0.035])
LINE = np.array([0.25, 0.25, 0.26])

FONT = {  # 3x5 block digits
    "0": ["111", "101", "101", "101", "111"], "1": ["010", "110", "010", "010", "111"],
    "2": ["111", "001", "111", "100", "111"], "3": ["111", "001", "111", "001", "111"],
    "4": ["101", "101", "111", "001", "001"], "5": ["111", "100", "111", "001", "111"],
    "6": ["111", "100", "111", "101", "111"], "7": ["111", "001", "010", "010", "010"],
    "8": ["111", "101", "111", "101", "111"], "9": ["111", "101", "111", "001", "111"],
}


def glyphs(u, v, text, cu, cv, height):
    """Mask of `text` in block digits centred at (cu, cv), `height` tall (u right, v up)."""
    cell = height / 5.0
    gap = 0.8 * cell
    total = len(text) * 3 * cell + (len(text) - 1) * gap
    mask = np.zeros(u.shape, bool)
    x0 = cu - total / 2
    for ch in text:
        rows = FONT[ch]
        for r, row in enumerate(rows):
            for c, bit in enumerate(row):
                if bit == "1":
                    a, b = x0 + c * cell, cv + height / 2 - (r + 1) * cell
                    mask |= (u >= a - 0.002) & (u < a + cell + 0.002) & (v >= b - 0.002) & (v < b + cell + 0.002)
        x0 += 3 * cell + gap
    return mask


def paint_side(Y, Z, mirror):
    col = np.broadcast_to(WHITE, Y.shape + (3,)).copy()
    col[Z < 0.40] = NAVY
    # A rising slash from behind the front wheel to the tail: orange band, navy pinstripe.
    zc = 0.42 + (0.95 - Y) * 0.17
    taper = np.clip((0.95 - Y) / 0.5, 0.0, 1.0)
    band = (np.abs(Z - zc) < 0.065 * taper) & (Y < 0.95)
    pin = (np.abs(Z - (zc + 0.10)) < 0.014 * taper) & (Y < 0.95)
    col[band] = ORANGE
    col[pin] = NAVY
    # Door shut lines (door from y 0.60 back to -0.66, below the belt).
    for yl in (0.60, -0.66):
        col[(np.abs(Y - yl) < 0.004) & (Z > 0.27) & (Z < 0.97)] = LINE
    col[(np.abs(Z - 0.27) < 0.004) & (Y < 0.60) & (Y > -0.66)] = LINE
    # Door number panel, number readable from each side.
    pc, pz, pw, ph = -0.12, 0.66, 0.52, 0.30
    U = -Y if mirror else Y
    pu = -pc if mirror else pc
    du, dz = np.abs(U - pu), np.abs(Z - pz)
    panel = (du < pw / 2) & (dz < ph / 2)
    border = panel & ~((du < pw / 2 - 0.012) & (dz < ph / 2 - 0.012))
    col[panel] = WHITE
    col[border] = BLACK
    col[glyphs(U, Z, NUMBER, pu, pz, 0.21)] = BLACK
    return col


def paint_top(Y, X):
    col = np.broadcast_to(WHITE, Y.shape + (3,)).copy()
    ax = np.abs(X)
    col[(ax > 0.04) & (ax < 0.17)] = NAVY
    col[(ax > 0.19) & (ax < 0.215)] = ORANGE
    # Bonnet shut lines.
    col[(np.abs(Y - 0.66) < 0.004) & (ax < 0.72)] = LINE
    col[(np.abs(ax - 0.72) < 0.004) & (Y > 0.66)] = LINE
    # Tailgate shut line under the spoiler.
    col[(np.abs(Y + 1.39) < 0.004) & (ax < 0.66)] = LINE
    return col


def paint_end(X, Z, back):
    col = np.broadcast_to(WHITE, X.shape + (3,)).copy()
    col[Z < 0.40] = NAVY
    ax = np.abs(X)
    if not back:
        col[(ax < 0.40) & (Z > 0.47) & (Z < 0.62)] = BLACK  # grille
        col[(ax < 0.52) & (Z > 0.27) & (Z < 0.37)] = BLACK  # lower intake
        col[(ax < 0.40) & (np.abs(Z - 0.545) < 0.006)] = LINE
    else:
        col[(ax < 0.24) & (Z > 0.50) & (Z < 0.62)] = np.array([0.85, 0.85, 0.80])  # plate blank
        col[(ax < 0.66) & (np.abs(Z - 0.97) < 0.004)] = LINE  # tailgate line
    return col


def build_livery():
    v, u = np.mgrid[0:TEX, 0:TEX]
    u = (u + 0.5) / TEX
    v = (v + 0.5) / TEX  # row 0 = bottom (Blender's pixel order)
    img = np.zeros((TEX, TEX, 3))
    Yw = u * LY + LY0
    r = v >= 0.625
    img[r] = paint_side(Yw[r], (v[r] - 0.625) / 0.375 * LZ, False)
    left = (v >= 0.25) & (v < 0.625)
    img[left] = paint_side(Yw[left], (v[left] - 0.25) / 0.375 * LZ, True)
    top = (v < 0.25) & (u < 0.75)
    img[top] = paint_top(u[top] / 0.75 * LY + LY0, v[top] / 0.25 * LX + LX0)
    for back, lo in ((False, 0.75), (True, 0.875)):
        m = (v < 0.25) & (u >= lo) & (u < lo + 0.125)
        img[m] = paint_end((u[m] - lo) / 0.125 * LX + LX0, v[m] / 0.25 * LZ, back)
    rgba = np.concatenate([img, np.ones((TEX, TEX, 1))], axis=2).astype(np.float32)
    im = bpy.data.images.new("livery", TEX, TEX, alpha=False)
    im.pixels.foreach_set(rgba.ravel())
    im.file_format = "PNG"
    im.pack()
    return im


def uv_for(p, n):
    """Livery atlas UV of point p (model frame) on a face with normal n."""
    h = p.z - G
    if n.z < -0.55:
        return (0.76, 0.005)  # underside: navy
    if n.z > 0.55:
        return ((p.y - LY0) / LY * 0.75, (p.x - LX0) / LX * 0.25)
    if abs(n.x) >= abs(n.y):
        base = 0.625 if n.x > 0 else 0.25
        return ((p.y - LY0) / LY, base + min(max(h, 0.0), LZ - 0.01) / LZ * 0.375)
    lo = 0.75 if n.y > 0 else 0.875
    return (lo + (p.x - LX0) / LX * 0.125, min(max(h, 0.0), LZ - 0.01) / LZ * 0.25)


# ---------------------------------------------------------------- mesh building


# ---------------------------------------------------------------- body shell (loft)


def keys(ks, y):
    return float(np.interp(y, [k[0] for k in ks], [k[1] for k in ks]))


ZB = [(-1.87, 0.36), (-1.70, 0.27), (-1.50, 0.23), (1.50, 0.22), (1.80, 0.25), (2.03, 0.33)]
ZT = [(-1.87, 0.94), (-1.50, 0.97), (-0.70, 0.95), (0.62, 0.90), (1.60, 0.82), (2.03, 0.72)]
ZR = [(-1.87, 0.95), (-1.80, 1.02), (-1.62, 1.22), (-1.42, 1.385), (-1.20, 1.415), (-0.40, 1.425),
      (-0.25, 1.41), (-0.15, 1.385), (0.0, 1.31), (0.30, 1.15), (0.50, 1.02), (0.62, 0.94),
      (1.00, 0.905), (1.60, 0.855), (1.90, 0.81), (2.03, 0.76)]


def ws(y):
    w = 0.89
    if y > 1.62:
        w -= 0.21 * ((y - 1.62) / (Y_NOSE - 1.62)) ** 2
    if y < -1.50:
        w -= 0.10 * ((-1.50 - y) / (-1.50 - Y_TAIL)) ** 2
    return w


def section(y):
    """Right half of the body section at y: 11 points (x, height above ground), bottom centre
    to top centre. Segment 6 (belt → p7) is side glass, 7 the pillar/roof rail, 8–9 the top."""
    zb, zt, zr = keys(ZB, y), keys(ZT, y), keys(ZR, y)
    w = ws(y)
    wt = w - 0.025
    h = max(zr - zt, 0.0)
    wg = wt - 0.16 * min(h, 0.4) / 0.4
    zs = min(0.62, zt - 0.08)
    z7 = zt + max(h - 0.07, 0.35 * h)
    z8 = zt + max(h - 0.012, 0.7 * h)
    return [
        (0.0, zb), (w - 0.10, zb), (w - 0.02, zb + 0.06), (w, zb + 0.45 * (zs - zb)), (w, zs),
        (w - 0.008, zs + 0.5 * (zt - zs)), (wt, zt), (wg + 0.015, z7),
        (wg - 0.035 * min(h, 0.2) / 0.2 - 0.01, z8), (wg * 0.5, zr + 0.012), (0.0, zr + 0.018),
    ], h


GLASS_SIDE = [(-0.62, 0.60), (-1.30, -0.72)]
GLASS_TOP = [(-0.17, 0.60), (-1.80, -1.42)]
CUTS = [0.60, -0.17, -0.62, -0.72, -1.30, -1.42, -1.80, 0.66, -1.39]


def stations():
    ys = list(np.arange(Y_TAIL, Y_NOSE - 0.02, 0.07)) + [Y_NOSE] + CUTS
    ys += list(np.linspace(1.80, Y_NOSE, 6)) + list(np.linspace(Y_TAIL, -1.70, 4))
    ys = sorted(set(round(y, 4) for y in ys))
    out = []
    for y in ys:
        if out and y - out[-1] < 0.015:
            if y in CUTS:
                out[-1] = y
            continue
        out.append(y)
    return out


def in_any(y, spans):
    return any(a < y < b for a, b in spans)


def build_shell():
    """Closed body shell: returns (Mesh, ring size, station list)."""
    ys = stations()
    rings = []
    hs = []
    for y in ys:
        pts, h = section(y)
        ring = [(x, z) for x, z in pts] + [(-x, z) for x, z in reversed(pts[1:-1])]
        rings.append([Vector((x, y, zg(z))) for x, z in ring])
        hs.append(h)

    def shrink(ring, y, f):
        cz = sum(p.z for p in ring) / len(ring)
        return [Vector((p.x * f, y, cz + (p.z - cz) * f)) for p in ring]

    n = len(rings[0])
    rings = [shrink(rings[0], Y_TAIL - 0.03, 0.93)] + rings + [shrink(rings[-1], Y_NOSE + 0.035, 0.9)]
    ys = [Y_TAIL - 0.03] + ys + [Y_NOSE + 0.035]
    hs = [hs[0]] + hs + [hs[-1]]
    M = Mesh()
    verts = [p for r in rings for p in r]
    for s in range(len(rings) - 1):
        ym = (ys[s] + ys[s + 1]) / 2
        hm = (hs[s] + hs[s + 1]) / 2
        for i in range(n):
            seg = i if i < 10 else 19 - i
            mat = "Paint"
            if seg == 6 and hm > 0.14 and in_any(ym, GLASS_SIDE):
                mat = "Glass"
            if seg in (8, 9) and in_any(ym, GLASS_TOP):
                mat = "Glass"
            i2 = (i + 1) % n
            M.f.append([s * n + i, (s + 1) * n + i, (s + 1) * n + i2, s * n + i2])
            M.m.append(mat)
    M.v = [tuple(p) for p in verts]
    # End caps (fans).
    for s, rev in ((0, True), (len(rings) - 1, False)):
        c = sum(rings[s], Vector()) / n
        ci = len(M.v)
        M.v.append(tuple(c))
        for i in range(n):
            f = [s * n + i, s * n + (i + 1) % n, ci]
            M.f.append(f[::-1] if rev else f)
            M.m.append("Paint")
    return M


# ---------------------------------------------------------------- assemble


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    livery = build_livery()
    material("Paint", (1, 1, 1), rough=0.32, image=livery)
    material("Glass", lin((0.05, 0.07, 0.08)), rough=0.05, metal=0.0, alpha=0.62)
    material("Black", lin((0.035, 0.035, 0.04)), rough=0.6)
    material("Accent", lin((1.0, 0.42, 0.04)), rough=0.45)
    material("Metal", lin((0.62, 0.63, 0.65)), rough=0.35, metal=1.0)
    material("Lamp", lin((0.95, 0.95, 0.9)), rough=0.1, emit=(1.0, 0.97, 0.9, 6.0))
    material("TailLamp", lin((0.55, 0.02, 0.02)), rough=0.2, emit=(1.0, 0.05, 0.03, 2.0))
    material("Tyre", lin((0.045, 0.045, 0.05)), rough=0.9)
    material("Rim", lin((0.92, 0.92, 0.92)), rough=0.35)
    material("Cage", lin((0.78, 0.79, 0.80)), rough=0.4, metal=0.6)
    material("Gauge", lin((0.02, 0.02, 0.025)), rough=0.3)
    material("Needle", lin((1.0, 0.45, 0.1)), rough=0.4, emit=(1.0, 0.45, 0.1, 3.0))
    material("Tick", lin((0.9, 0.9, 0.9)), rough=0.5, emit=(0.9, 0.9, 0.9, 1.5))

    cockpit.materials()

    root = bpy.data.objects.new("Car", None)
    link(root)

    # Body shell; wheel wells cut with booleans.
    shell = build_shell()
    body = shell.obj("Body", root, smooth_angle=38)
    inner_src = body.copy()
    inner_src.data = body.data.copy()
    for k, (x, y) in HUB.items():
        cut = Mesh()
        sx = 1 if x > 0 else -1
        cut.cyl((sx * 0.50, y, 0.045), (sx * 1.3, y, 0.045), 0.40, 40, "Black")
        c = cut.obj("cut" + k)
        mod = body.modifiers.new("arch" + k, "BOOLEAN")
        mod.operation = "DIFFERENCE"
        mod.solver = "EXACT"
        mod.material_mode = "TRANSFER"
        mod.object = c
    bpy.context.view_layer.objects.active = body
    for mod in list(body.modifiers):
        bpy.ops.object.modifier_apply(modifier=mod.name)
    for k in HUB:
        bpy.data.objects.remove(bpy.data.objects["cut" + k], do_unlink=True)
    finish(body, 38)
    uv_paint(body)

    ext = Mesh()
    add_exterior(ext)
    ext_ob = ext.obj("Trim", root, smooth_angle=35)
    uv_paint(ext_ob)

    pod = empty("LightPod", (0, 0, 0), root)
    p = Mesh()
    add_pod(p)
    p.obj("LightPodMesh", pod, smooth_angle=40)

    # Cabin lining: the shell's opaque faces round the cabin, seen from inside.
    paint = {i for i, m in enumerate(inner_src.data.materials) if m.name == "Paint"}
    # (Not round the wheel arches: they're cut out of the body, the lining would show there.)
    def in_arch(c):
        return abs(c.x) > 0.40 and any(math.hypot(c.y - y, c.z - 0.045) < 0.47 for _, y in HUB.values())

    skin = cockpit.inner_skin(inner_src, lambda f, c: f.material_index in paint and -1.78 < c.y < 0.66 and c.z - G > 0.30 and f.normal.z > -0.5 and not in_arch(c))
    bpy.data.objects.remove(inner_src, do_unlink=True)
    add_wheels(root)
    cockpit.build(root, skin, bake=BAKE)

    tris = 0
    for o in bpy.data.objects:
        if o.type == "MESH":
            o.data.calc_loop_triangles()
            tris += len(o.data.loop_triangles)
    print(f"rallycar: {tris} triangles, {len(MATS)} materials")
    if PREVIEW:
        preview(PREVIEW)
    bpy.ops.export_scene.gltf(filepath=OUT, export_format="GLB", export_yup=True, export_apply=True, use_selection=False, export_image_format="AUTO",
                              export_vertex_color="NAME", export_vertex_color_name="AO")


def uv_paint(ob):
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active.data
    paint = [i for i, m in enumerate(me.materials) if m and m.name == "Paint"]
    mw = ob.matrix_world
    for poly in me.polygons:
        n = poly.normal
        for li in poly.loop_indices:
            if poly.material_index in paint:
                p = mw @ me.vertices[me.loops[li].vertex_index].co
                uv[li].uv = uv_for(p, n)
            else:
                uv[li].uv = (0.0, 0.0)


# ---------------------------------------------------------------- exterior details


def add_exterior(M):
    for k, (x, y) in HUB.items():
        sx = 1 if x > 0 else -1
        # Arch flare: a lip swept round the upper wheel arc.
        zc = 0.045
        prof = [(0.40, 0.80), (0.40, 0.89), (0.415, 0.912), (0.45, 0.905), (0.475, 0.87), (0.475, 0.80)]
        frames = []
        a0, a1 = math.radians(-14), math.radians(194)
        for i in range(25):
            t = a0 + (a1 - a0) * i / 24
            d = Vector((0, math.cos(t), math.sin(t)))
            frames.append((Vector((0, y, zc)), d, Vector((sx, 0, 0))))
        M.sweep(prof, frames, "Black", closed=True, caps=True)
        # Mud flap behind the wheel.
        M.box((sx * 0.76, y - 0.44, zg(0.18)), (0.30, 0.012, 0.27), "Accent")
    # Skid plate under the nose and the engine.
    M.sweep([(-0.62, 0.0), (0.62, 0.0), (0.62, 0.012), (-0.62, 0.012)],
            [(Vector((0, y, zg(h))), Vector((1, 0, 0)), Vector((0, 0, 1))) for y, h in ((0.75, 0.205), (1.6, 0.205), (1.95, 0.23), (2.06, 0.31))],
            "Metal", caps=True)
    # Tow hooks: front right, rear left.
    M.torus((0.38, 2.09, zg(0.31)), (1, 0, 0), 0.04, 0.009, 12, 6, "Accent")
    M.torus((-0.42, -1.93, zg(0.36)), (1, 0, 0), 0.04, 0.009, 12, 6, "Accent")
    # Roof scoop (front opening black) and roof spoiler with end plates.
    sz = zg(keys(ZR, -0.30) + 0.012)
    M.add([(-0.11, -0.28, sz), (0.11, -0.28, sz), (0.12, -0.62, sz - 0.01), (-0.12, -0.62, sz - 0.01),
           (-0.10, -0.29, sz + 0.075), (0.10, -0.29, sz + 0.075)],
          [(0, 3, 2, 1), (0, 1, 5, 4), (4, 5, 2, 3), (0, 4, 3), (1, 2, 5)], "Paint")
    M.add([(-0.085, -0.276, sz + 0.012), (0.085, -0.276, sz + 0.012), (0.085, -0.276, sz + 0.065), (-0.085, -0.276, sz + 0.065)],
          [(0, 1, 2, 3)], "Black")
    rz = zg(1.40)
    wing = [(0.0, 0.0), (-0.24, -0.035), (-0.25, -0.02), (-0.02, 0.018)]
    M.sweep(wing, [(Vector((x, -1.36, rz + 0.06)), Vector((0, 1, 0)), Vector((0, 0, 1))) for x in (-0.66, 0.66)], "Black", caps=True)
    for sx in (-1, 1):
        M.box((sx * 0.665, -1.48, rz + 0.03), (0.012, 0.26, 0.11), "Black")
    # Headlights, tail lamps, exhaust, door mirrors.
    for sx in (-1, 1):
        M.sweep([(0.085 * math.cos(t), 0.05 * math.sin(t)) for t in np.linspace(0, 2 * math.pi, 14, endpoint=False)],
                [(Vector((sx * 0.50, y, zg(0.665))), Vector((1, 0, 0)), Vector((0, 0, 1))) for y in (2.02, 2.068)], "Lamp", caps=True)
        M.box((sx * 0.66, -1.885, zg(0.82)), (0.13, 0.06, 0.20), "TailLamp")
        # Mirror: stalk + housing, glass facing back.
        M.box((sx * 0.86, 0.50, zg(0.98)), (0.10, 0.04, 0.03), "Black")
        M.box((sx * 0.95, 0.49, zg(1.01)), (0.15, 0.08, 0.10), "Black")
        M.box((sx * 0.95, 0.448, zg(1.01)), (0.13, 0.006, 0.08), "Metal")
    M.cyl((0.46, -1.80, zg(0.27)), (0.46, -1.97, zg(0.27)), 0.042, 12, "Metal", caps=False)
    M.add([(0.46 + 0.035 * math.cos(t), -1.94, zg(0.27) + 0.035 * math.sin(t)) for t in np.linspace(0, 2 * math.pi, 12, endpoint=False)],
          [tuple(range(12))], "Black")
    # Wipers (resting at the windscreen base).
    for x0 in (-0.55, 0.05):
        M.box((x0 + 0.24, 0.585, zg(0.955)), (0.50, 0.015, 0.012), "Black")


def add_pod(M):
    y0, h0 = 1.80, keys(ZR, 1.80) + 0.01
    M.box((0, y0, zg(h0 + 0.10)), (1.06, 0.13, 0.17), "Black")
    M.box((0, y0 - 0.02, zg(h0 + 0.01)), (0.5, 0.25, 0.03), "Black")
    for x in (-0.39, -0.13, 0.13, 0.39):
        M.cyl((x, y0 + 0.04, zg(h0 + 0.10)), (x, y0 + 0.10, zg(h0 + 0.10)), 0.088, 18, "Black", caps=False)
        M.add([(x + 0.08 * math.cos(t), y0 + 0.095, zg(h0 + 0.10) + 0.08 * math.sin(t)) for t in np.linspace(0, 2 * math.pi, 18, endpoint=False)],
              [tuple(range(18))], "Lamp")


# ---------------------------------------------------------------- wheels


def wheel_mesh(right):
    """Tyre, rim and disc about the X axis; outer face towards +X (mirrored for left)."""
    M = Mesh()
    w = 0.103
    prof = [(-0.093, 0.19), (-w, 0.215), (-w - 0.004, 0.262), (-w + 0.002, 0.296), (-0.085, 0.316), (-0.055, R),
            (-0.018, R), (0.018, R), (0.055, R), (0.085, 0.316), (w - 0.002, 0.296), (w + 0.004, 0.262), (w, 0.215), (0.093, 0.19)]
    n = 56

    def knobs(k, j, r):
        if 4 <= j <= 9:
            col = min(max(j - 4, 0), 4) // 2
            if (k + 2 * col) % 4 >= 2:
                return r - 0.011
        return r

    M.lathe(prof, n, "Tyre", radius_fn=knobs)
    # Rim: barrel (inside), outer lip, six spokes, hub; brake disc.
    M.lathe([(0.075, 0.19), (-0.09, 0.19)], 32, "Rim")
    M.lathe([(0.075, 0.172), (0.082, 0.183), (0.075, 0.196)], 32, "Rim")
    for i in range(6):
        t = 2 * math.pi * i / 6
        rot = Matrix.Rotation(t, 3, "X")
        M.box(rot @ Vector((0.05, 0.0, 0.118)), (0.028, 0.038, 0.13), "Rim", rot=rot)
    M.cyl((0.03, 0, 0), (0.07, 0, 0), 0.058, 16, "Rim")
    M.cyl((0.0705, 0, 0), (0.072, 0, 0), 0.02, 8, "Black")
    M.cyl((-0.02, 0, 0), (0.0, 0, 0), 0.155, 24, "Metal")
    if not right:
        M.v = [(-x, y, z) for x, y, z in M.v]
        M.f = [f[::-1] for f in M.f]
    return M


def add_wheels(root):
    for k, (x, y) in HUB.items():
        right = x > 0
        hub = empty(f"Hub{k}", (x, y, 0.0), root)
        spin = empty(f"Spin{k}", (0, 0, 0), hub)
        wheel_mesh(right).obj(f"Wheel{k}", spin, recalc=False, smooth_angle=40)
        C = Mesh()
        sx = 1 if right else -1
        C.box((sx * -0.01, -0.07, 0.12), (0.06, 0.10, 0.07), "Accent", rot=Matrix.Rotation(math.radians(30), 3, "X"))
        C.obj(f"Caliper{k}", hub, recalc=True)


# ---------------------------------------------------------------- preview renders


def preview(out, eye=Vector()):
    """Workbench renders of the car (`eye`: the driver rig's offset for the cockpit view)."""
    import os
    os.makedirs(out, exist_ok=True)
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "STUDIO"
    sc.display.shading.color_type = "TEXTURE"
    sc.display.shading.show_backface_culling = True
    sc.render.resolution_x, sc.render.resolution_y = 1000, 640
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    link(cam)
    sc.camera = cam
    views = {
        "front34": ((4.5, 5.5, 1.6), (0, 0.2, 0.2), 40),
        "rear34": ((-4.5, -5.5, 1.8), (0, -0.2, 0.2), 40),
        "side_left": ((-7.5, 0.1, 0.3), (0, 0.1, 0.25), 40),
        "top": ((0.01, 0.1, 9.0), (0, 0.1, 0), 40),
        "cockpit": (Vector((DRV_X, -0.60, zg(1.12))) + eye, Vector((DRV_X, 3.0, zg(1.0))) + eye, 95),
    }
    for name, (eye, at, lens_fov) in views.items():
        cam.location = eye
        d = Vector(at) - Vector(eye)
        cam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
        cam.data.angle = math.radians(lens_fov)
        cam.data.clip_start = 0.02
        sc.render.filepath = f"{out}/{name}.png"
        bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    main()
