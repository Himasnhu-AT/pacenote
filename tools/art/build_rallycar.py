# Builds the player's car procedurally (Blender, background): "Talus", an original 5-door
# gravel-rally hatch (design sheet: docs/cars/rally_hatch.md) with livery, wheels, detachable
# body parts, a cockpit, the seated driver and three levels of detail. No downloaded parts.
#   memgate -- blender -b --factory-startup -P tools/art/build_rallycar.py -- \
#       assets/models/rallycar.glb [--preview DIR] [--no-bake] [--paint RRGGBB] [--number NN]
# The cockpit (interior, wheel, levers, gauges) is cockpit.py, the driver's body driver.py,
# mesh helpers blendkit.py.
#
# Frame (Blender): X right, Y forward (exported as glTF -Z), Z up; origin at the wheelbase's
# middle at hub height, the ground at z = -R. Wheel centres are the sim's mounts (src/vehicle.rs
# `Spec`: track 1.56 m, wheelbase 2.55 m; src/art/car.rs MODEL_HALF_TRACK/WHEELBASE).
#
# Nodes the game uses (src/art/car.rs, src/art/hands.rs, src/guide):
#   Car
#     Chassis               the sprung body
#       Body                shell, glass, trim (one mesh, several materials)
#       Interior            the cabin (AO baked into the "AO" vertex colours)
#       SteeringWheel       turns about its column (local Blender Y = glTF -Z)
#       Tacho, Speedo       gauge needles, same axis convention
#       Shifter, Handbrake  levers pivoting at their foot (hands.rs)
#       DriverBody          the seated driver (hidden in the driver's own cockpit view)
#       SeatHarness         the empty passenger seat's harness
#       LightPod            bonnet lamp pod (hidden from the inside cameras)
#       MirrorL, MirrorR, RearWing, FrontSplitter, FrontBumper, RearBumper, RearDiffuser:
#                           detachable parts (car.rs DETACHABLE), origin at their middle
#     Hub<FL|FR|RL|RR>      suspension (height from the sim)
#       Steer<FL|FR>        front knuckle (steering); carries Caliper<FL|FR>
#         Spin<..>          rolls; carries Wheel<..> (tyre, rim, disc)
#       Caliper<RL|RR>      on the rear hubs
#     CoDriverEye, CoDriverTablet   anchors for the co-driver's view
# Levels of detail: a mesh node `X` with a lower-detail twin has siblings `X_lod1` and `X_lod2`
# (same parent, same origin); car.rs gives each its VisibilityRange. Meshes without twins
# (cabin, driver, callipers) are LOD0 only: they fade out with LOD0.
# Materials are single-sided (backface culled) unless noted: the body shell is invisible from
# inside, so the cockpit sees the interior's own lining; the glass shows only from outside.
import math
import os
import sys

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cockpit  # noqa: E402
import driver  # noqa: E402
from blendkit import MATS, Mesh, empty, finish, lin, link, material  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []


def opt(name, default=None):
    return argv[argv.index(name) + 1] if name in argv else default


OUT = argv[0] if argv and not argv[0].startswith("--") else "assets/models/rallycar.glb"
PREVIEW = opt("--preview")
BAKE = "--no-bake" not in argv
NUMBER = opt("--number", "27")


def hexrgb(h):
    return np.array([int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4)])


R = cockpit.R  # tyre radius (src/art/car.rs MODEL_WHEEL_RADIUS)
G = -R  # ground z
HUB = {"FL": (-0.78, 1.275), "FR": (0.78, 1.275), "RL": (-0.78, -1.275), "RR": (0.78, -1.275)}
Y_TAIL, Y_NOSE = -1.90, 2.06
# The bumpers are the shell's lower nose and tail, split off at these stations below the flank
# crease (ZS).
FRONT_CUT, REAR_CUT = 1.74, -1.74
DRV_X = cockpit.DRV_X
# Wheel arches: squared superellipse round the hub, half length A, height B over the hub.
ARCH_A, ARCH_B, ARCH_P = 0.43, 0.445, 3.2

# Levels of detail (src/art/car.rs LOD ranges; docs/ASSETS_BRIEF.md §1): the body is built
# three times, coarser each time: station spacing (m), whether glass/door stations are kept,
# extra stations at each end, points round an arch, small trim.
LEVELS = [
    dict(step=0.05, cuts=True, ends=7, arch_n=30, minor=True),
    dict(step=0.08, cuts=True, ends=4, arch_n=16, minor=False),
    dict(step=0.40, cuts=False, ends=1, arch_n=4, minor=False),
]
D = dict(LEVELS[0])

# sRGB palette (docs/cars/rally_hatch.md).
PAINT = hexrgb(opt("--paint", "1d5c5e"))
CHALK = hexrgb("ece9df")
OCHRE = hexrgb("f2a01a")
BLACK = hexrgb("161718")
LINE = PAINT * 0.30


def zg(h):
    """Height above the ground → model z."""
    return G + h


def keys(ks, y):
    return float(np.interp(y, [k[0] for k in ks], [k[1] for k in ks]))


# ---------------------------------------------------------------- body lines (heights above ground)

ZB = [(-1.90, 0.40), (-1.80, 0.31), (-1.62, 0.25), (1.62, 0.24), (1.88, 0.28), (2.06, 0.36)]  # underside
ZS = [(-1.90, 0.55), (REAR_CUT, 0.55), (FRONT_CUT, 0.60), (2.06, 0.60)]  # flank crease / bumper top
ZT = [(-1.90, 0.99), (-1.70, 1.00), (-1.20, 0.97), (0.64, 0.925), (1.20, 0.875), (1.80, 0.83), (2.06, 0.79)]  # belt
ZR = [(-1.90, 0.995), (-1.84, 1.045), (-1.76, 1.17), (-1.66, 1.33), (-1.58, 1.425), (-1.50, 1.455), (-1.35, 1.465),
      (-0.20, 1.465), (-0.10, 1.45), (0.0, 1.40), (0.30, 1.235), (0.55, 1.03), (0.64, 0.955), (1.00, 0.93),
      (1.60, 0.885), (1.90, 0.84), (2.06, 0.80)]  # roof / bonnet centre line

# Side glass (front door, rear door, quarter) and top glass (windscreen, tailgate) spans in y.
GLASS_SIDE = [(-0.53, 0.60), (-0.62, -1.15), (-1.24, -1.50)]
GLASS_TOP = [(-0.06, 0.62), (-1.82, -1.54)]
CUTS = [0.60, -0.53, -0.62, -1.15, -1.24, -1.50, -0.06, 0.62, -1.82, -1.54, FRONT_CUT, REAR_CUT, 0.70]


def ws(y):
    """Half width of the flank at y."""
    w = 0.87
    if y > 1.70:
        w -= 0.10 * ((y - 1.70) / (Y_NOSE - 1.70)) ** 2
    if y < -1.62:
        w -= 0.06 * ((-1.62 - y) / (-1.62 - Y_TAIL)) ** 2
    return w


def section(y):
    """Right half of the body section at y: 11 points (x, height above ground), bottom centre
    to top centre. Segments 0-3 are below the crease (bumpers at the ends), 6 (belt → p7) the
    side glass, 7 the pillar / roof rail, 8-9 the top."""
    zb, zt, zr = keys(ZB, y), keys(ZT, y), keys(ZR, y)
    zs = min(keys(ZS, y), zt - 0.10)
    w = ws(y)
    wt = w - 0.03
    h = max(zr - zt, 0.0)
    k = min(h, 0.4) / 0.4
    wg = wt - 0.15 * k
    z7 = zt + max(h - 0.075, 0.35 * h)
    z8 = zt + max(h - 0.014, 0.7 * h)
    return [
        (0.0, zb), (w - 0.10, zb), (w - 0.025, zb + 0.04), (w, zb + 0.11), (w, zs),
        (w - 0.006, zs + 0.5 * (zt - zs)), (wt, zt), (wg + 0.012, z7),
        (wg - 0.035 * min(h, 0.2) / 0.2 - 0.008, z8), (wg * 0.5, zr + 0.012), (0.0, zr + 0.018),
    ], h


def top_height(y, x):
    """Height above the ground of the body's top surface at (x, y) (bonnet, roof)."""
    pts, _ = section(y)
    top = pts[7:][::-1]  # centre → roof rail
    xs = [p[0] for p in top]
    zs = [p[1] for p in top]
    return float(np.interp(abs(x), xs, zs))


def in_any(y, spans):
    return any(min(a, b) < y < max(a, b) for a, b in spans)


def stations():
    cuts = CUTS if D["cuts"] else [FRONT_CUT, REAR_CUT, 0.62]
    ys = list(np.arange(Y_TAIL, Y_NOSE - 0.02, D["step"])) + [Y_NOSE] + cuts
    if D["ends"] > 1:
        ys += list(np.linspace(1.84, Y_NOSE, D["ends"])) + list(np.linspace(Y_TAIL, -1.74, D["ends"]))
    ys = sorted(set(round(y, 4) for y in ys))
    out = []
    for y in ys:
        if out and y - out[-1] < 0.012:
            if y in cuts:
                out[-1] = y
            continue
        out.append(y)
    return out


# ---------------------------------------------------------------- livery atlas
# 2048 x 1024, v up: right side v∈[0.625,1), left side v∈[0.25,0.625), top view (u<0.75,
# v<0.25), front (0.75≤u<0.875) and back (u≥0.875). Sides map u = y, so both sides share
# stripe positions; numbers are drawn un-mirrored on each side.

TEX_W, TEX_H = 2048, 1024
LY0, LY = -2.0, 4.2  # y range of the side/top maps
LZ = 1.6  # height range of the side/end maps
LX0, LX = -0.95, 1.9

FONT = {  # 3x5 block digits
    "0": ["111", "101", "101", "101", "111"], "1": ["010", "110", "010", "010", "111"],
    "2": ["111", "001", "111", "100", "111"], "3": ["111", "001", "111", "001", "111"],
    "4": ["101", "101", "111", "001", "001"], "5": ["111", "100", "111", "001", "111"],
    "6": ["111", "100", "111", "101", "111"], "7": ["111", "001", "001", "010", "010"],
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
        for r, row in enumerate(FONT[ch]):
            for c, bit in enumerate(row):
                if bit == "1":
                    a, b = x0 + c * cell, cv + height / 2 - (r + 1) * cell
                    mask |= (u >= a - 0.002) & (u < a + cell + 0.002) & (v >= b - 0.002) & (v < b + cell + 0.002)
        x0 += 3 * cell + gap
    return mask


def number_panel(col, U, V, cu, cv, w, h, text, digit_h):
    du, dv = np.abs(U - cu), np.abs(V - cv)
    panel = (du < w / 2) & (dv < h / 2)
    border = panel & ~((du < w / 2 - 0.014) & (dv < h / 2 - 0.014))
    col[panel] = CHALK
    col[border] = BLACK
    col[glyphs(U, V, text, cu, cv, digit_h)] = BLACK


def paint_side(Y, Z, mirror):
    col = np.broadcast_to(PAINT, Y.shape + (3,)).copy()
    zt = np.interp(Y, [k[0] for k in ZT], [k[1] for k in ZT])
    # The chalk band: from a point behind the front arch, climbing and widening to the C-pillar;
    # an ochre pinstripe rides on its top edge.
    t = np.clip((0.80 - Y) / 2.05, 0.0, 1.0)
    lo = 0.40 + 0.38 * t
    th = 0.20 * t ** 0.7
    run = (Y < 0.80) & (Y > -1.24)
    band = run & (Z > lo) & (Z < lo + th)
    pin = run & (Z > lo + th + 0.012) & (Z < lo + th + 0.012 + 0.022 * np.clip(t * 3, 0, 1))
    col[band] = CHALK
    col[pin] = OCHRE
    # Black sills and lower bumpers; black B- and C-pillars above the belt (a floating roof).
    col[Z < 0.33] = BLACK
    col[((Y > FRONT_CUT) | (Y < REAR_CUT)) & (Z < keys(ZS, 0.0) + 0.002)] = BLACK
    col[(Z > zt + 0.01) & (((Y > -0.62) & (Y < -0.53)) | ((Y > -1.24) & (Y < -1.15)) | (Y < -1.50))] = BLACK
    # Door shut lines: front door 0.62 → -0.56, rear door -0.59 → -1.18 (its foot round the arch).
    def vline(y, z0, z1):
        col[(np.abs(Y - y) < 0.0035) & (Z > z0) & (Z < z1)] = LINE

    def hline(z, y0, y1):
        col[(np.abs(Z - z) < 0.0035) & (Y > y0) & (Y < y1)] = LINE

    vline(0.62, 0.33, 0.93)
    vline(-0.56, 0.33, 0.97)
    vline(-0.59, 0.33, 0.97)
    vline(-1.18, 0.80, 0.98)
    hline(0.335, -0.59, 0.62)
    # The fuel filler (right rear quarter) and the bumper joints.
    vline(FRONT_CUT, 0.24, 0.60)
    vline(REAR_CUT, 0.25, 0.55)
    # Competition number on the front doors, readable from each side.
    U = -Y if mirror else Y
    pc = 0.03
    number_panel(col, U, Z, -pc if mirror else pc, 0.635, 0.44, 0.30, NUMBER, 0.20)
    return col


def paint_top(Y, X):
    col = np.broadcast_to(PAINT, Y.shape + (3,)).copy()
    ax = np.abs(X)
    # Chalk roof with the number for the helicopter (reads from the left side).
    roof = (Y < -0.07) & (Y > -1.52)
    col[roof] = CHALK
    col[roof & glyphs(Y, X, NUMBER, -0.85, 0.0, 0.46)] = BLACK
    # Bonnet: shut lines and two ochre pinstripes, bonnet pins at the front corners.
    col[(np.abs(Y - 0.70) < 0.0035) & (ax < 0.75)] = LINE
    col[(np.abs(ax - 0.75) < 0.0035) & (Y > 0.70)] = LINE
    col[(Y > 0.72) & (np.abs(ax - 0.17) < 0.012)] = OCHRE
    for sx in (-1, 1):
        col[np.hypot(Y - 1.93, X - sx * 0.56) < 0.025] = BLACK
    # Tailgate shut line under the wing.
    col[(np.abs(Y + 1.53) < 0.0035) & (ax < 0.62)] = LINE
    return col


def paint_end(X, Z, back):
    col = np.broadcast_to(PAINT, X.shape + (3,)).copy()
    ax = np.abs(X)
    col[Z < (0.55 if back else 0.60)] = BLACK  # bumpers
    if back:
        col[(np.abs(Z - 0.565) < 0.0035) & (ax < 0.62)] = LINE  # tailgate
        col[(np.abs(ax - 0.62) < 0.0035) & (Z > 0.565)] = LINE
    return col


def build_livery():
    v, u = np.mgrid[0:TEX_H, 0:TEX_W]
    u = (u + 0.5) / TEX_W
    v = (v + 0.5) / TEX_H  # row 0 = bottom (Blender's pixel order)
    img = np.zeros((TEX_H, TEX_W, 3))
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
    rgba = np.concatenate([img, np.ones((TEX_H, TEX_W, 1))], axis=2).astype(np.float32)
    im = bpy.data.images.new("livery", TEX_W, TEX_H, alpha=False)
    im.pixels.foreach_set(rgba.ravel())
    im.file_format = "JPEG"
    im.pack()
    return im


def uv_for(p, n):
    """Livery atlas UV of point p (model frame) on a face with normal n."""
    h = p.z - G
    if n.z < -0.55:
        return (0.76, 0.005)  # underside: black
    if n.z > 0.55:
        return ((p.y - LY0) / LY * 0.75, (p.x - LX0) / LX * 0.25)
    if abs(n.x) >= abs(n.y):
        base = 0.625 if n.x > 0 else 0.25
        return ((p.y - LY0) / LY, base + min(max(h, 0.0), LZ - 0.01) / LZ * 0.375)
    lo = 0.75 if n.y > 0 else 0.875
    return (lo + (p.x - LX0) / LX * 0.125, min(max(h, 0.0), LZ - 0.01) / LZ * 0.25)


def uv_paint(ob, mats=("Paint",)):
    me = ob.data
    if not me.uv_layers:
        me.uv_layers.new(name="UVMap")
    uv = me.uv_layers.active.data
    paint = [i for i, m in enumerate(me.materials) if m and m.name in mats]
    mw = ob.matrix_world
    for poly in me.polygons:
        if poly.material_index not in paint:
            continue
        n = (mw.to_3x3() @ poly.normal).normalized()
        for li in poly.loop_indices:
            p = mw @ me.vertices[me.loops[li].vertex_index].co
            uv[li].uv = uv_for(p, n)


# ---------------------------------------------------------------- body shell (loft)


def build_shell():
    """The body shell as three meshes: (main, front bumper, rear bumper). The ends are closed
    by horizontal strips (one per section segment), so the bumpers split off cleanly at the
    crease."""
    ys = stations()
    rings, hs = [], []
    for y in ys:
        pts, h = section(y)
        ring = list(pts) + [(-x, z) for x, z in reversed(pts[1:-1])]
        rings.append([Vector((x, y, zg(z))) for x, z in ring])
        hs.append(h)
    n = len(rings[0])  # 20: right 0..10, left 11..19 (mirrors of 9..1)
    main, front, rear = Mesh(), Mesh(), Mesh()

    def mirror(j):
        """Ring index of the left twin of right point j (0 and 10 are on the centre line)."""
        return j if j in (0, 10) else n - j

    def seg_of(i):
        return i if i < 10 else 19 - i

    for s in range(len(rings) - 1):
        ym = (ys[s] + ys[s + 1]) / 2
        hm = (hs[s] + hs[s + 1]) / 2
        for i in range(n):
            seg = seg_of(i)
            mat = "Paint"
            if seg == 6 and hm > 0.14 and in_any(ym, GLASS_SIDE):
                mat = "Glass"
            if seg in (8, 9) and in_any(ym, GLASS_TOP):
                mat = "Glass"
            i2 = (i + 1) % n
            quad = [rings[s][i], rings[s + 1][i], rings[s + 1][i2], rings[s][i2]]
            target = main
            if seg <= 3 and ym > FRONT_CUT:
                target = front
            elif seg <= 3 and ym < REAR_CUT:
                target = rear
            target.add(quad, [(0, 1, 2, 3)], mat)
    # End caps: strips between each right point and its left twin.
    for ring, nose in ((rings[0], False), (rings[-1], True)):
        for j in range(10):
            a, b, c, d = ring[j], ring[j + 1], ring[mirror(j + 1)], ring[mirror(j)]
            face = [a, b, c, d] if j not in (0, 9) else ([a, b, c] if j == 0 else [a, b, d])
            if not nose:
                face = face[::-1]
            target = main
            if j <= 3:
                target = front if nose else rear
            # Winding: outward along ±y.
            nrm = (face[1] - face[0]).cross(face[2] - face[0])
            if (nrm.y > 0) != nose:
                face = face[::-1]
            target.add(face, [tuple(range(len(face)))], "Paint")
    return main, front, rear


# ---------------------------------------------------------------- wheel arches


def arch_path(cy, n=None, drop=R - 0.27):
    """Points round a squared arch at y = cy (front end first): (point, outward normal) in the
    YZ plane (x = 0), straight down `drop` below hub height at both ends."""
    n = n or D["arch_n"]
    out = []
    a, b, p = ARCH_A, ARCH_B, ARCH_P
    out.append((Vector((0, cy + a, -drop)), Vector((0, 1, 0))))
    for k in range(n + 1):
        t = math.pi * k / n
        c, s = math.cos(t), math.sin(t)
        u = math.copysign(abs(c) ** (2 / p), c)
        v = abs(s) ** (2 / p)
        pt = Vector((0, cy + a * u, b * v))
        g = Vector((0, math.copysign(abs(u) ** (p - 1), u) / a, (abs(v) ** (p - 1)) / b))
        out.append((pt, g.normalized()))
    out.append((Vector((0, cy - a, -drop)), Vector((0, -1, 0))))
    return out


def arch_cutter(cy, sx):
    """A prism (x from 0.42 to 1.4 on side sx) with the arch's outline, reaching below the car."""
    path = arch_path(cy, drop=0.0)
    outline = [p for p, _ in path] + [Vector((0, cy - ARCH_A, -0.6)), Vector((0, cy + ARCH_A, -0.6))]
    bm = bmesh.new()
    rings = []
    for x in (0.42, 1.4):
        rings.append([bm.verts.new((sx * x, q.y, q.z)) for q in outline])
    m = len(outline)
    for i in range(m):
        bm.faces.new([rings[0][i], rings[0][(i + 1) % m], rings[1][(i + 1) % m], rings[1][i]])
    bm.faces.new(rings[0][::-1])
    bm.faces.new(rings[1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new("cutter")
    bm.to_mesh(me)
    bm.free()
    ob = bpy.data.objects.new("cutter", me)
    link(ob)
    return ob


def add_arches(M):
    """Flared squared arches, inner liners and mud flaps."""
    for k, (x, y) in HUB.items():
        sx = 1 if x > 0 else -1
        path = arch_path(y)
        w = ws(y)
        # Liner: the arch's inside surface from the inner wall to the flank; inner wall.
        rings = [[p + Vector((sx * xx, 0, 0)) for xx in (0.42, w + 0.01)] for p, _ in path]
        M.loft(rings, "Liner", closed=False)
        wall = [p + Vector((sx * 0.42, 0, 0)) for p, _ in path]
        M.add(wall, [tuple(range(len(wall)))], "Liner")
        # Flare: a box-section lip round the opening, 5 cm proud of the flank.
        prof = [(-0.008, w - 0.035), (-0.008, w + 0.032), (0.012, w + 0.05), (0.078, w + 0.05), (0.098, w + 0.025), (0.098, w - 0.035)]
        frames = [(p, nrm, Vector((sx, 0, 0))) for p, nrm in path]
        if sx < 0:
            prof = [(a, b) for a, b in reversed(prof)]
        M.sweep(prof, frames, "Plastic", caps=True)
        # Mud flap behind the wheel, an ochre stripe across its foot.
        fy = y - ARCH_A - 0.035
        M.box((sx * (w - 0.13), fy, zg(0.19)), (0.30, 0.012, 0.26), "Rubber")
        M.box((sx * (w - 0.13), fy - 0.007, zg(0.11)), (0.30, 0.004, 0.035), "Accent")


def cut_arches(body):
    cutters = []
    for k, (x, y) in HUB.items():
        c = arch_cutter(y, 1 if x > 0 else -1)
        c.name = "cut" + k
        cutters.append(c)
        mod = body.modifiers.new("arch" + k, "BOOLEAN")
        mod.operation = "DIFFERENCE"
        mod.solver = "EXACT"
        mod.material_mode = "TRANSFER"
        mod.object = c
    bpy.context.view_layer.objects.active = body
    for mod in list(body.modifiers):
        bpy.ops.object.modifier_apply(modifier=mod.name)
    for c in cutters:
        bpy.data.objects.remove(c, do_unlink=True)


# ---------------------------------------------------------------- exterior details


def on_top(y, x, lift=0.0):
    return zg(top_height(y, x) + lift)


def add_major_trim(M):
    """Trim that every level of detail keeps: arches, lamps, grille, scoop, plates, flaps."""
    add_arches(M)
    # Headlamps: square units either side of the grille in the flat nose.
    yn = Y_NOSE
    for sx in (-1, 1):
        cx = sx * 0.545
        M.box((cx, yn - 0.01, zg(0.70)), (0.34, 0.05, 0.145), "Plastic")
        M.box((cx, yn + 0.012, zg(0.70)), (0.31, 0.012, 0.118), "Chrome")
        M.box((cx, yn + 0.02, zg(0.70)), (0.29, 0.006, 0.10), "Lamp")
        # Indicator strip under the lamp.
        M.box((cx + sx * 0.06, yn + 0.004, zg(0.615)), (0.18, 0.02, 0.025), "Amber")
    # Grille: recessed black panel with horizontal bars.
    M.box((0, yn - 0.03, zg(0.70)), (0.68, 0.06, 0.15), "Grille")
    for z in (0.655, 0.685, 0.715, 0.745):
        M.box((0, yn + 0.003, zg(z)), (0.66, 0.012, 0.010), "Plastic")
    # Tail: lamps at the corners, a reversing lamp, the plate.
    yt = Y_TAIL
    for sx in (-1, 1):
        M.box((sx * 0.66, yt - 0.006, zg(0.80)), (0.20, 0.03, 0.30), "TailLamp")
        M.box((sx * 0.66, yt - 0.02, zg(0.69)), (0.20, 0.004, 0.06), "Lamp")
    M.box((0, yt - 0.006, zg(0.66)), (0.50, 0.012, 0.11), "Plate")
    # Roof scoop: a wedge rising to a forward-facing black intake.
    y0, y1 = -0.22, -0.62
    z0, z1 = on_top(y0, 0.0), on_top(y1, 0.0)
    M.add([(-0.15, y0, z0), (0.15, y0, z0), (0.16, y1, z1 - 0.005), (-0.16, y1, z1 - 0.005),
           (-0.13, y0 - 0.01, z0 + 0.085), (0.13, y0 - 0.01, z0 + 0.085)],
          [(0, 3, 2, 1), (0, 1, 5, 4), (4, 5, 2, 3), (0, 4, 3), (1, 2, 5)], "Paint")
    M.add([(-0.12, y0 + 0.002, z0 + 0.012), (0.12, y0 + 0.002, z0 + 0.012), (0.12, y0 - 0.006, z0 + 0.078), (-0.12, y0 - 0.006, z0 + 0.078)],
          [(0, 1, 2, 3)], "Grille")
    # Skid plate under the nose and engine.
    M.sweep([(-0.64, 0.0), (0.64, 0.0), (0.64, 0.012), (-0.64, 0.012)],
            [(Vector((0, y, zg(keys(ZB, y) - 0.03))), Vector((1, 0, 0)), Vector((0, 0, 1))) for y in (0.70, 1.55, 1.85, 2.0)],
            "Metal", caps=True)


def add_minor_trim(M):
    """Small trim (LOD0 only)."""
    # Wipers parked at the windscreen base.
    for x0 in (-0.58, 0.04):
        z = on_top(0.60, x0 + 0.26, 0.012)
        M.box((x0 + 0.26, 0.60, z), (0.52, 0.016, 0.012), "Plastic")
        M.cyl((x0 + 0.02, 0.62, z - 0.01), (x0 + 0.02, 0.62, z + 0.02), 0.012, 8, "Plastic")
    # Door handles.
    for sx in (-1, 1):
        for y in (0.06, -0.98):
            M.box((sx * (ws(y) + 0.008), y, zg(0.86)), (0.02, 0.13, 0.028), "Plastic")
    # Fuel filler (right rear quarter).
    M.cyl((ws(-1.42) - 0.005, -1.42, zg(0.83)), (ws(-1.42) + 0.006, -1.42, zg(0.83)), 0.055, 18, "Alu")
    # Bonnet pins, the aerial, tow loops (on the crash structure: they show through the bumpers).
    for sx in (-1, 1):
        M.cyl((sx * 0.56, 1.93, on_top(1.93, 0.56, -0.004)), (sx * 0.56, 1.93, on_top(1.93, 0.56, 0.012)), 0.018, 10, "Alu")
    M.cyl((0.30, -1.30, on_top(-1.30, 0.30)), (0.30, -1.34, on_top(-1.30, 0.30) + 0.24), 0.0035, 6, "Plastic")
    M.torus((0.40, 2.12, zg(0.33)), (1, 0, 0), 0.042, 0.009, 14, 6, "Accent")
    M.torus((-0.44, -1.97, zg(0.37)), (1, 0, 0), 0.042, 0.009, 14, 6, "Accent")
    # Exhaust, right rear, under the bumper.
    M.cyl((0.46, -1.60, zg(0.27)), (0.46, -1.99, zg(0.27)), 0.045, 14, "Metal", caps=False)
    M.add([(0.46 + 0.04 * math.cos(t), -1.97, zg(0.27) + 0.04 * math.sin(t)) for t in np.linspace(0, 2 * math.pi, 14, endpoint=False)],
          [tuple(range(14))], "Plastic")
    # Crash structure behind the bumpers (seen when one comes off): close-out panels, beams,
    # the radiator.
    for y, s in ((FRONT_CUT - 0.01, 1), (REAR_CUT + 0.01, -1)):
        zb, zs = keys(ZB, y), keys(ZS, y)
        w = ws(y) - 0.02
        M.box((0, y, zg((zb + zs) / 2)), (2 * w, 0.01, zs - zb), "Plastic")
        M.box((0, y + s * 0.10, zg(0.42)), (1.40, 0.10, 0.10), "Metal")
        for sx in (-1, 1):
            M.box((sx * 0.55, y + s * 0.05, zg(0.42)), (0.08, 0.10, 0.08), "Metal")
    M.box((0, FRONT_CUT + 0.04, zg(0.48)), (0.80, 0.05, 0.26), "Radiator")
    # Bonnet louvres either side of the centre.
    for sx in (-1, 1):
        for k in range(6):
            y = 1.20 + 0.05 * k
            x = sx * 0.36
            M.box((x, y, on_top(y, x, 0.006)), (0.26, 0.02, 0.012), "Grille")


def add_pod(M):
    y0 = 1.83
    h0 = top_height(y0, 0.0) + 0.01
    M.box((0, y0, zg(h0 + 0.10)), (1.06, 0.13, 0.17), "Plastic")
    M.box((0, y0 - 0.02, zg(h0 + 0.01)), (0.5, 0.25, 0.03), "Plastic")
    for x in (-0.39, -0.13, 0.13, 0.39):
        M.cyl((x, y0 + 0.04, zg(h0 + 0.10)), (x, y0 + 0.10, zg(h0 + 0.10)), 0.088, 14, "Plastic", caps=False)
        M.add([(x + 0.078 * math.cos(t), y0 + 0.088, zg(h0 + 0.10) + 0.078 * math.sin(t)) for t in np.linspace(0, 2 * math.pi, 14, endpoint=False)],
              [tuple(range(14))], "Lamp")


# ---------------------------------------------------------------- detachable parts


def mirror_mesh(sx):
    M = Mesh()
    y, z = 0.50, zg(1.00)
    x0 = sx * (ws(0.5) - 0.02)
    M.box((x0 + sx * 0.06, y + 0.02, z - 0.03), (0.12, 0.05, 0.03), "Plastic")
    # Housing: a tapered box, chalk cap, glass facing back.
    c = Vector((x0 + sx * 0.15, y, z))
    rings = []
    for dy, s in ((0.06, 0.82), (0.03, 1.0), (-0.04, 1.0)):
        rings.append([c + Vector((sx * a * s, dy, b * s)) for a, b in ((-0.08, -0.05), (0.09, -0.05), (0.095, 0.055), (-0.08, 0.06))])
    M.loft(rings, "Chalk", closed=True, caps=True)
    M.box(c + Vector((sx * 0.005, -0.043, 0.002)), (0.16, 0.006, 0.095), "MirrorGlass")
    return M


def wing_mesh():
    """Roof-edge wing: a cambered plane from the roof's trailing edge out over the tailgate,
    a gurney lip, end plates running down the tailgate's sides."""
    M = Mesh()
    z = on_top(-1.42, 0.0)
    sec = [(0.0, 0.004), (-0.12, 0.030), (-0.36, 0.062), (-0.385, 0.092), (-0.40, 0.092), (-0.40, 0.040), (-0.22, 0.004), (-0.08, -0.010)]
    M.sweep(sec, [(Vector((x, -1.40, z)), Vector((0, 1, 0)), Vector((0, 0, 1))) for x in (-0.69, 0.69)], "Plastic", caps=True)
    for sx in (-1, 1):
        x = sx * 0.695
        M.add([(x, -1.42, z + 0.01), (x, -1.81, z + 0.11), (x, -1.81, z - 0.10), (x, -1.62, z - 0.09)], [(0, 1, 2, 3)], "PlasticDouble")
    return M


def splitter_mesh():
    M = Mesh()
    z = zg(keys(ZB, 1.95) - 0.025)
    M.sweep([(-0.80, 0.0), (0.80, 0.0), (0.80, 0.018), (-0.80, 0.018)],
            [(Vector((0, y, z + (y - 1.80) * 0.05)), Vector((1, 0, 0)), Vector((0, 0, 1))) for y in (1.76, 2.00, 2.14)], "Plastic", caps=True)
    for sx in (-1, 1):
        M.box((sx * 0.80, 1.96, z + 0.04), (0.012, 0.36, 0.07), "Plastic")
    return M


def diffuser_mesh():
    M = Mesh()
    a, b = Vector((0, -1.56, zg(0.235))), Vector((0, -1.98, zg(0.33)))
    M.sweep([(-0.72, 0.0), (0.72, 0.0), (0.72, 0.014), (-0.72, 0.014)],
            [(a, Vector((1, 0, 0)), Vector((0, 0, 1))), (b, Vector((1, 0, 0)), Vector((0, 0, 1)))], "Plastic", caps=True)
    for x in (-0.48, -0.24, 0.0, 0.24, 0.48):
        M.add([Vector((x, a.y, a.z - 0.01)), Vector((x, b.y, b.z - 0.01)), Vector((x, b.y, b.z - 0.09)), Vector((x, a.y + 0.05, a.z - 0.03))],
              [(0, 1, 2, 3)], "PlasticDouble")
    return M


def bumper_extras(front):
    """Lamps and plates that ride on a bumper."""
    M = Mesh()
    if front:
        y = Y_NOSE
        for sx in (-1, 1):
            c = Vector((sx * 0.52, y + 0.002, zg(0.43)))
            M.cyl(c - Vector((0, 0.03, 0)), c + Vector((0, 0.01, 0)), 0.06, 12, "Plastic", caps=False)
            M.add([c + Vector((0.05 * math.cos(t), 0.012, 0.05 * math.sin(t))) for t in np.linspace(0, 2 * math.pi, 12, endpoint=False)],
                  [tuple(range(12))], "Lamp")
        M.box((0, y + 0.004, zg(0.47)), (0.46, 0.012, 0.10), "Plate")
        # Lower intake.
        M.box((0, y - 0.01, zg(0.34)), (0.80, 0.04, 0.07), "Grille")
    else:
        y = Y_TAIL
        for sx in (-1, 1):
            M.box((sx * 0.72, y - 0.004, zg(0.45)), (0.10, 0.012, 0.04), "TailLamp")
    return M


# ---------------------------------------------------------------- wheels


def wheel_mesh(right):
    """Gravel tyre, six-spoke rim and disc about the X axis; outer face towards +X (mirrored
    for the left)."""
    M = Mesh()
    w = 0.103
    prof = [(-0.093, 0.19), (-w, 0.215), (-w - 0.004, 0.262), (-w + 0.002, 0.296), (-0.087, 0.314), (-0.062, R),
            (-0.022, R), (0.022, R), (0.062, R), (0.087, 0.314), (w - 0.002, 0.296), (w + 0.004, 0.262), (w, 0.215), (0.093, 0.19)]
    n = 64

    def knobs(k, j, r):
        # Staggered tread blocks: shoulder blocks and two centre rows, offset.
        if 4 <= j <= 9:
            col = (j - 4) // 2
            if (k + 2 * col) % 4 >= 2:
                return r - 0.012
        return r

    M.lathe(prof, n, "Tyre", radius_fn=knobs)
    M.lathe([(0.075, 0.19), (-0.09, 0.19)], 36, "Rim")
    M.lathe([(0.075, 0.172), (0.083, 0.184), (0.075, 0.197)], 36, "Rim")
    for i in range(6):
        t = 2 * math.pi * i / 6
        rot = Matrix.Rotation(t, 3, "X")
        M.box(rot @ Vector((0.052, 0.0, 0.118)), (0.026, 0.036, 0.13), "Rim", rot=rot)
        # Lug nut.
        p = Matrix.Rotation(t + math.pi / 6, 3, "X") @ Vector((0.071, 0.0, 0.042))
        M.cyl(p, p + Vector((0.008, 0, 0)), 0.008, 6, "Metal")
    M.cyl((0.03, 0, 0), (0.07, 0, 0), 0.06, 18, "Rim")
    M.cyl((0.0705, 0, 0), (0.073, 0, 0), 0.022, 10, "Plastic")
    # Vented disc: two faces and a gap.
    M.cyl((-0.03, 0, 0), (-0.018, 0, 0), 0.158, 28, "Metal")
    M.cyl((-0.008, 0, 0), (0.004, 0, 0), 0.158, 28, "Metal")
    M.cyl((-0.02, 0, 0), (0.02, 0, 0), 0.07, 16, "Metal")
    if not right:
        M.v = [(-x, y, z) for x, y, z in M.v]
        M.f = [f[::-1] for f in M.f]
    return M


def wheel_far(right, n):
    """A far wheel: an n-sided tyre drum and a flat rim face."""
    M = Mesh()
    M.cyl((-0.10, 0, 0), (0.10, 0, 0), R, n, "Tyre", caps=False)
    M.cyl((0.08, 0, 0), (0.10, 0, 0), 0.20, n, "Rim", caps=True)
    if not right:
        M.v = [(-x, y, z) for x, y, z in M.v]
        M.f = [f[::-1] for f in M.f]
    return M


def add_wheels(root):
    for k, (x, y) in HUB.items():
        right = x > 0
        hub = empty(f"Hub{k}", (x, y, 0.0), root)
        carrier = empty(f"Steer{k}", (0, 0, 0), hub) if k[0] == "F" else hub
        spin = empty(f"Spin{k}", (0, 0, 0), carrier)
        w = wheel_mesh(right).obj(f"Wheel{k}", spin, recalc=False, smooth_angle=40)
        for lv, sides in ((1, 16), (2, 7)):
            twin(w, lv, wheel_far(right, sides))
        C = Mesh()
        sx = 1 if right else -1
        C.box((sx * -0.012, -0.07, 0.125), (0.065, 0.11, 0.075), "Accent", rot=Matrix.Rotation(math.radians(30), 3, "X"))
        C.box((sx * -0.012, -0.10, 0.10), (0.075, 0.03, 0.03), "Plastic", rot=Matrix.Rotation(math.radians(30), 3, "X"))
        C.obj(f"Caliper{k}", carrier, recalc=True)


# ---------------------------------------------------------------- levels of detail


def tris(ob):
    ob.data.calc_loop_triangles()
    return len(ob.data.loop_triangles)


def far_glass(ob):
    """Opaque dark glass for the far levels (no cabin behind it)."""
    for i, m in enumerate(ob.data.materials):
        if m and m.name == "Glass":
            ob.data.materials[i] = MATS["GlassFar"]


def twin(ob, lv, M):
    """`ob`'s lower-detail twin `<name>_lod<lv>` built from mesh M: same parent and origin."""
    tw = M.obj(f"{ob.name}_lod{lv}", recalc=True, smooth_angle=50) if isinstance(M, Mesh) else M
    tw.name = tw.data.name = f"{ob.name}_lod{lv}"
    tw.parent = ob.parent
    tw.location = ob.location.copy()
    tw.rotation_euler = ob.rotation_euler.copy()
    return tw


# ---------------------------------------------------------------- assemble


def materials():
    livery = build_livery()
    # A light clear coat: in Bevy a full-strength one washed the teal out to cream in the sun.
    material("Paint", (1, 1, 1), rough=0.45, image=livery, coat=(0.15, 0.15))
    material("Glass", lin((0.05, 0.07, 0.08)), rough=0.05, metal=0.0, alpha=0.62)
    material("GlassFar", lin((0.035, 0.045, 0.055)), rough=0.08, metal=0.3)
    material("Plastic", lin((0.07, 0.072, 0.075)), rough=0.62)
    material("PlasticDouble", lin((0.07, 0.072, 0.075)), rough=0.62, double=True)
    material("Liner", lin((0.035, 0.035, 0.037)), rough=0.85, double=True)
    material("Grille", lin((0.02, 0.02, 0.022)), rough=0.7)
    material("Radiator", lin((0.16, 0.16, 0.17)), rough=0.5, metal=0.6)
    material("Chalk", lin(tuple(CHALK)), rough=0.4, coat=(0.15, 0.15))
    material("Accent", lin(tuple(OCHRE)), rough=0.45)
    material("Metal", lin((0.62, 0.63, 0.65)), rough=0.35, metal=1.0)
    material("Chrome", lin((0.90, 0.90, 0.92)), rough=0.12, metal=1.0)
    material("Alu", lin((0.72, 0.73, 0.75)), rough=0.3, metal=1.0)
    material("Lamp", lin((0.95, 0.95, 0.9)), rough=0.1, emit=(1.0, 0.97, 0.9, 2.5))
    material("Amber", lin((1.0, 0.55, 0.05)), rough=0.2, emit=(1.0, 0.5, 0.05, 0.6))
    material("TailLamp", lin((0.50, 0.02, 0.02)), rough=0.15, emit=(1.0, 0.04, 0.03, 1.2))
    material("Plate", lin((0.86, 0.86, 0.83)), rough=0.5)
    material("Tyre", lin((0.045, 0.045, 0.05)), rough=0.92)
    material("Rubber", lin((0.03, 0.03, 0.032)), rough=0.9)
    material("Rim", lin(tuple(hexrgb("3a3c3f"))), rough=0.35, metal=0.6)
    material("MirrorGlass", lin((0.10, 0.11, 0.12)), rough=0.04, metal=1.0)
    material("Black", lin((0.035, 0.035, 0.04)), rough=0.6)
    material("Gauge", lin((0.02, 0.02, 0.025)), rough=0.3)
    material("Needle", lin((1.0, 0.45, 0.1)), rough=0.4, emit=(1.0, 0.45, 0.1, 3.0))
    material("Tick", lin((0.9, 0.9, 0.9)), rough=0.5, emit=(0.9, 0.9, 0.9, 1.5))
    cockpit.materials()
    driver.materials()


# Meshes drawn at every distance (no twins; src/art/car.rs leaves them without a range).
ALL_LEVELS = ("MirrorL", "MirrorR", "RearWing", "FrontSplitter", "FrontBumper", "RearBumper", "RearDiffuser", "LightPodMesh")


class Cabin(cockpit.Cabin):
    """This body's cabin for cockpit.py (heights above the ground)."""

    floor = 0.30
    firewall_y = 0.62
    side_x = 0.80
    door_y = (0.60, -0.55)
    belt = 0.93
    cage_x_belt = 0.75
    cage_x_roof = 0.60
    cage_roof = 1.36
    screen_top_y = -0.06
    screen_base = (0.60, 0.94)
    b_pillar_y = -0.74
    hoop_y = -1.05
    rear_foot = (-1.70, 0.72)


def centred(o, parent):
    """Moves `o`'s origin to its bounding box's middle (detachable parts) and parents it."""
    lo = Vector([min(v.co[i] for v in o.data.vertices) for i in range(3)])
    hi = Vector([max(v.co[i] for v in o.data.vertices) for i in range(3)])
    c = (lo + hi) / 2
    o.data.transform(Matrix.Translation(-c))
    o.location = c
    o.parent = parent
    return o


def body_level(lv):
    """The body at detail level `lv`: shell (arches cut), trim; glass opaque below level 0.
    Level 0 also returns the bumper meshes and the shell copy the cabin lining comes from."""
    D.clear()
    D.update(LEVELS[lv])
    main_m, front_m, rear_m = build_shell()
    shell = main_m.obj(f"Shell{lv}", recalc=True, smooth_angle=36)
    lining_src = None
    if lv == 0:
        lining_src = shell.copy()
        lining_src.data = shell.data.copy()
        link(lining_src)
    cut_arches(shell)
    finish(shell, 36)
    trim = Mesh()
    add_major_trim(trim)
    parts = [shell, trim.obj(f"Trim{lv}", recalc=True, smooth_angle=35)]
    for o in parts:
        uv_paint(o)
    if D["minor"]:
        minor = Mesh()
        add_minor_trim(minor)
        parts.append(minor.obj(f"TrimMinor{lv}", recalc=True, smooth_angle=35))
    body = cockpit.join(parts, "Body" if lv == 0 else f"Body_lod{lv}")
    if lv > 0:
        far_glass(body)
    return body, (front_m, rear_m), lining_src


def main():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    materials()
    root = bpy.data.objects.new("Car", None)
    link(root)
    chassis = empty("Chassis", (0, 0, 0), root)

    # The body, near to far (the far levels are the body's twins: same parent and origin).
    body, (front_m, rear_m), lining_src = body_level(0)
    body.parent = chassis
    for lv in (1, 2):
        twin(body, lv, body_level(lv)[0])
    D.clear()
    D.update(LEVELS[0])

    # Detachable parts (one level: they're small).
    for sx, name in ((-1, "MirrorL"), (1, "MirrorR")):
        centred(mirror_mesh(sx).obj(name, recalc=True, smooth_angle=40), chassis)
    centred(wing_mesh().obj("RearWing", recalc=True, smooth_angle=40), chassis)
    centred(splitter_mesh().obj("FrontSplitter", recalc=True, smooth_angle=40), chassis)
    centred(diffuser_mesh().obj("RearDiffuser", recalc=True, smooth_angle=40), chassis)
    for name, m, front in (("FrontBumper", front_m, True), ("RearBumper", rear_m, False)):
        b = m.obj(name, recalc=True, smooth_angle=36)
        uv_paint(b)
        extra = bumper_extras(front).obj(name + "Extras", recalc=True, smooth_angle=40)
        centred(cockpit.join([b, extra], name), chassis)

    pod = empty("LightPod", (0, 0, 0), chassis)
    p = Mesh()
    add_pod(p)
    p.obj("LightPodMesh", pod, smooth_angle=40)

    # Cabin lining: the shell's opaque faces round the cabin, seen from inside (not round the
    # wheel arches, which are cut out of the body).
    paint = {i for i, m in enumerate(lining_src.data.materials) if m.name == "Paint"}

    def in_arch(c):
        return abs(c.x) > 0.40 and any(abs(c.y - y) < ARCH_A + 0.06 and c.z < ARCH_B + 0.06 for _, y in HUB.values())

    skin = cockpit.inner_skin(lining_src, lambda f, c: f.material_index in paint and -1.84 < c.y < 0.66 and c.z - G > 0.30 and f.normal.z > -0.5 and not in_arch(c))
    bpy.data.objects.remove(lining_src, do_unlink=True)
    add_wheels(root)
    cockpit.build(chassis, skin, Cabin, bake=BAKE, seat_harness=True)
    # The seated driver: eye and shoulders of the arms rig (src/art/hands.rs), cushion top, feet.
    eye = Vector((DRV_X, -0.60, zg(1.12)))
    shoulders = [Vector((-0.58, -0.74, zg(0.90))), Vector((-0.16, -0.74, zg(0.90)))]
    driver.build(chassis, eye, shoulders, zg(0.445), (0.41, zg(0.41)))
    # The co-driver's eye (the driver's mirrored, seated) and tablet on the lap.
    empty("CoDriverEye", (-DRV_X, -0.60, zg(1.12)), root)
    empty("CoDriverTablet", (0.34, -0.28, 0.26), root)

    report()
    if PREVIEW:
        preview(PREVIEW)
    bpy.ops.export_scene.gltf(filepath=OUT, export_format="GLB", export_yup=True, export_apply=True, use_selection=False,
                              export_image_format="JPEG", export_jpeg_quality=88,
                              export_vertex_color="NAME", export_vertex_color_name="AO")


def report():
    groups = {"lod0 exterior": 0, "cockpit": 0, "lod1": 0, "lod2": 0, "all levels": 0}
    cabin = ("Interior", "SteeringWheelMesh", "TachoNeedle", "SpeedoNeedle", "ShifterMesh", "HandbrakeMesh", "DriverBody", "SeatHarness")
    for o in bpy.data.objects:
        if o.type != "MESH":
            continue
        n = tris(o)
        if o.name.endswith("_lod1"):
            groups["lod1"] += n
        elif o.name.endswith("_lod2"):
            groups["lod2"] += n
        elif o.name in cabin:
            groups["cockpit"] += n
        elif o.name in ALL_LEVELS:
            groups["all levels"] += n  # parts and the pod: drawn at every distance
        else:
            groups["lod0 exterior"] += n
        print(f"  {o.name:24s} {n:7d}")
    print("triangles:", groups, f"{len(MATS)} materials")
    print(f"LOD0 exterior {groups['lod0 exterior'] + groups['all levels']}, cockpit {groups['cockpit']}, "
          f"LOD1 {groups['lod1'] + groups['all levels']}, LOD2 {groups['lod2'] + groups['all levels']}")


# ---------------------------------------------------------------- preview renders


def preview(out):
    """Workbench renders: four exterior views, the driver's eye, and the far levels."""
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
        "front34": ((4.6, 5.6, 1.5), (0, 0.2, 0.15), 38, 0),
        "rear34": ((-4.6, -5.6, 1.9), (0, -0.2, 0.2), 38, 0),
        "side_left": ((-7.5, 0.1, 0.3), (0, 0.1, 0.25), 38, 0),
        "front": ((0.0, 8.0, 0.4), (0, 0, 0.25), 30, 0),
        "rear": ((0.0, -8.0, 0.6), (0, 0, 0.3), 30, 0),
        "top": ((0.01, 0.1, 9.0), (0, 0.1, 0), 40, 0),
        "cockpit": ((DRV_X, -0.60, zg(1.12)), (DRV_X, 3.0, zg(1.0)), 95, 0),
        "side_right": ((7.5, 0.1, 0.3), (0, 0.1, 0.25), 38, 0),
        "lod1": ((4.6, 5.6, 1.5), (0, 0.2, 0.15), 38, 1),
        "lod2": ((4.6, 5.6, 1.5), (0, 0.2, 0.15), 38, 2),
    }
    for name, (eye, at, fov, lv) in views.items():
        for o in bpy.data.objects:
            if o.type != "MESH":
                continue
            lvl = 1 if o.name.endswith("_lod1") else 2 if o.name.endswith("_lod2") else 0
            o.hide_render = lvl != lv
            if lv > 0 and lvl == 0 and o.name in ALL_LEVELS:
                o.hide_render = False
        cam.location = eye
        d = Vector(at) - Vector(eye)
        cam.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
        cam.data.angle = math.radians(fov)
        cam.data.clip_start = 0.02
        sc.render.filepath = f"{out}/{name}.png"
        bpy.ops.render.render(write_still=True)
    for o in bpy.data.objects:
        o.hide_render = False


if __name__ == "__main__":
    main()
