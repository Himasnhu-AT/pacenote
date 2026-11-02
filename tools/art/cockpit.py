# The rally car's cockpit, built procedurally (Blender): a stripped gravel-rally interior.
#   - padded multi-point roll cage, two bucket seats with harnesses
#   - flat dash: suede top, grained face, gauge cluster (textured dials, shift lights, oil and
#     water gauges), centre switch panel, co-driver's trip computer and map light
#   - suede deep-dish wheel, sequential shifter, hydraulic handbrake (each its own pivot node)
#   - door cards, floor, tunnel, pedals, footrests, fire extinguisher, mirror
# Textures (suede, carbon twill, brushed aluminium, tread plate, seat fabric, grained plastic,
# dial faces, trip LCD) are generated here with numpy: no third-party files. Ambient occlusion
# is baked (Cycles) into an "AO" vertex colour layer of the static interior.
#
# Used by build_rallycar.py (procedural body) and import_sketchfab.py (downloaded body).
# Nodes the game animates (src/art/car.rs, src/art/hands.rs): SteeringWheel, Tacho, Speedo,
# Shifter, Handbrake. Their positions must stay in step with those files.
import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from blendkit import MATS, TILE, Mesh, empty, finish, image, lin, material

R = 0.325  # tyre radius: the model origin is at hub height, the ground at z = -R
G = -R


def zg(h):
    """Height above the ground → model z."""
    return G + h


# Driver (left-hand drive): eye at (-0.37, -0.60, 1.12 above ground); see car.rs COCKPIT_EYE.
DRV_X = -0.37
# The steering wheel's centre and column axis (pointing forward and down, into the dash).
STEER_TILT = math.radians(22)
STEER_C = Vector((DRV_X, -0.17, zg(0.84)))
STEER_AX = Vector((0, math.cos(STEER_TILT), -math.sin(STEER_TILT)))
RIM = 0.175
# Seats: fore/aft trim (m) on top of their base position (seat back ~0.18 m behind the eye at
# head height, so the head rests ~0.1 m off it and the shoulders sit against it).
SEAT_DY = -0.08
# Shifter and handbrake pivots (on the console top). The game tilts the nodes about local X
# (fore/aft) and the forward axis (across); grip points are in src/art/hands.rs.
SHIFT_PIVOT = Vector((-0.10, -0.05, zg(0.58)))
HB_PIVOT = Vector((-0.03, -0.20, zg(0.58)))
# Gauges: centres (the hands test checks the tacho stays clear of the right arm).
TACHO_C = Vector((DRV_X - 0.075, 0.19, zg(0.835)))
SPEEDO_C = Vector((DRV_X + 0.075, 0.19, zg(0.835)))
DIAL_R = 0.062


class Cabin:
    """The body's cabin, heights above the ground. Defaults fit the procedural body; the
    importer measures a downloaded one."""

    floor = 0.30
    firewall_y = 0.62
    side_x = 0.80  # door cards
    door_y = (0.56, -0.72)  # front / rear edge of the door
    belt = 0.90  # side window bottom
    # Roll cage: x at the belt and at the roof rail, roof tube height, windscreen top and
    # base (y), A-pillar foot, B-pillar and main hoop (y), rear stays' foot (y, height).
    cage_x_belt = 0.75
    cage_x_roof = 0.585
    cage_roof = 1.31
    screen_top_y = -0.17
    screen_base = (0.55, 0.90)
    b_pillar_y = -0.74
    hoop_y = -1.05
    rear_foot = (-1.68, 0.70)


# ---------------------------------------------------------------- textures


def tile_noise(n, sx, sy, seed):
    """Tileable gaussian-filtered noise, zero mean, unit deviation; sx, sy blur in pixels."""
    rng = np.random.default_rng(seed)
    w = rng.standard_normal((n, n))
    fy = np.fft.fftfreq(n)[:, None]
    fx = np.fft.fftfreq(n)[None, :]
    g = np.exp(-2 * math.pi ** 2 * ((sx * fx) ** 2 + (sy * fy) ** 2))
    out = np.real(np.fft.ifft2(np.fft.fft2(w) * g))
    return (out - out.mean()) / (out.std() + 1e-9)


def normal_map(h, strength):
    dx = (np.roll(h, -1, 1) - np.roll(h, 1, 1)) * 0.5 * strength
    dy = (np.roll(h, -1, 0) - np.roll(h, 1, 0)) * 0.5 * strength
    n = np.stack([-dx, -dy, np.ones_like(h)], axis=2)
    n /= np.linalg.norm(n, axis=2, keepdims=True)
    return n * 0.5 + 0.5


def rgb(a, col):
    return a[..., None] * np.array(col)[None, None, :]


FONT = {  # 3x5 block glyphs
    "0": ["111", "101", "101", "101", "111"], "1": ["010", "110", "010", "010", "111"],
    "2": ["111", "001", "111", "100", "111"], "3": ["111", "001", "111", "001", "111"],
    "4": ["101", "101", "111", "001", "001"], "5": ["111", "100", "111", "001", "111"],
    "6": ["111", "100", "111", "101", "111"], "7": ["111", "001", "010", "010", "010"],
    "8": ["111", "101", "111", "101", "111"], "9": ["111", "101", "111", "001", "111"],
    ".": ["000", "000", "000", "000", "010"],
}


def draw_text(img, text, cx, cy, h, col):
    """Block text centred at pixel (cx, cy) (row 0 = bottom), `h` pixels tall."""
    c = h / 5.0
    width = len(text) * 4 * c - c
    x0 = cx - width / 2
    for k, ch in enumerate(text):
        rows = FONT[ch]
        for r, row in enumerate(rows):
            for q, bit in enumerate(row):
                if bit == "1":
                    xa = int(round(x0 + (k * 4 + q) * c))
                    ya = int(round(cy + h / 2 - (r + 1) * c))
                    img[max(ya, 0):ya + max(int(round(c)), 1), max(xa, 0):xa + max(int(round(c)), 1)] = col


def draw_dial(img, x0, y0, size, majors, labels, red_from, minors=True):
    """A dial face in img[y0:y0+size, x0:x0+size]: 270° sweep from lower left, ticks, labels."""
    yy, xx = np.mgrid[0:size, 0:size]
    dx = (xx + 0.5 - size / 2) / (size / 2)
    dy = (yy + 0.5 - size / 2) / (size / 2)
    rr = np.hypot(dx, dy)
    th = np.arctan2(dx, dy)  # clockwise from up
    f = (th / math.pi + 0.75) / 1.5
    face = np.zeros((size, size, 3)) + 0.012
    face[rr > 0.97] = 0.25  # chrome lip
    inside = (f >= -0.01) & (f <= 1.01)
    for i in range(majors + 1):
        fi = i / majors
        d = np.abs(f - fi) * 1.5 * math.pi * rr
        col = (0.95, 0.12, 0.08) if fi >= red_from - 1e-6 else (0.92, 0.92, 0.9)
        face[(d < 0.022) & (rr > 0.74) & (rr < 0.93)] = col
        if minors and i < majors:
            fm = (i + 0.5) / majors
            dm = np.abs(f - fm) * 1.5 * math.pi * rr
            colm = (0.95, 0.12, 0.08) if fm >= red_from else (0.8, 0.8, 0.78)
            face[(dm < 0.012) & (rr > 0.83) & (rr < 0.93)] = colm
    if red_from < 1.0:
        face[(f >= red_from) & (f <= 1.0) & (rr > 0.93) & (rr < 0.97) & inside] = (0.9, 0.1, 0.06)
    for i, lab in labels:
        t = (-0.75 + 1.5 * i / majors) * math.pi
        cx = size / 2 + math.sin(t) * 0.56 * size / 2
        cy = size / 2 + math.cos(t) * 0.56 * size / 2
        draw_text(face, lab, cx, cy, 0.15 * size / 2, (0.92, 0.92, 0.9))
    img[y0:y0 + size, x0:x0 + size] = face


def textures():
    T = {}
    # Suede (wheel rim, dash top, binnacle): fine nap.
    h = tile_noise(256, 1.0, 1.0, 1)
    tone = tile_noise(256, 14, 14, 2)
    T["suede"] = image("suede", rgb(0.05 + 0.006 * tone, (1, 1, 1)))
    T["suede_n"] = image("suede_n", normal_map(h, 0.35))
    # Carbon fibre, 2x2 twill: 16 tows across the tile.
    n, tow = 512, 32
    yy, xx = np.mgrid[0:n, 0:n]
    i, j = yy // tow, xx // tow
    horiz = ((j - i) % 4) < 2
    across = np.where(horiz, (yy % tow) / tow, (xx % tow) / tow)
    bump = np.sin(math.pi * across)
    fibre = np.where(horiz, tile_noise(n, 12, 0.6, 3), tile_noise(n, 0.6, 12, 4))
    shade = np.where(horiz, 0.030, 0.050) + 0.022 * bump + 0.004 * fibre
    T["carbon"] = image("carbon", rgb(shade, (0.9, 0.92, 1.0)))
    T["carbon_n"] = image("carbon_n", normal_map(bump * 2.0 + 0.05 * fibre, 1.6))
    # Brushed aluminium: streaks along U.
    st = tile_noise(256, 24, 0.5, 5)
    T["alu"] = image("alu", rgb(0.60 + 0.035 * st, (1, 1, 1)))
    T["alu_n"] = image("alu_n", normal_map(st, 0.25))
    # Tread plate: raised lugs alternating ±45° in a 32 px grid.
    n, c = 256, 32
    yy, xx = np.mgrid[0:n, 0:n]
    u = (xx % c) / c - 0.5
    v = (yy % c) / c - 0.5
    sgn = np.where(((xx // c + yy // c) % 2) == 0, 1.0, -1.0)
    a = (u + sgn * v) / math.sqrt(2)
    b = (u - sgn * v) / math.sqrt(2)
    lug = np.clip(1 - (np.maximum(np.abs(a) / 0.33, np.abs(b) / 0.08)) ** 4, 0, 1)
    T["tread"] = image("tread", rgb(0.55 + 0.08 * lug + 0.02 * tile_noise(n, 3, 3, 6), (1, 1, 1)))
    T["tread_n"] = image("tread_n", normal_map(lug * 3.0, 1.2))
    # Seat fabric: tight basket weave, dark blue.
    n = 256
    yy, xx = np.mgrid[0:n, 0:n]
    w = np.sin(xx / n * 2 * math.pi * 32) * np.sin(yy / n * 2 * math.pi * 32)
    fab = 0.85 + 0.12 * w + 0.05 * tile_noise(n, 2, 2, 7)
    T["fabric"] = image("fabric", rgb(fab, lin((0.10, 0.14, 0.30))))
    T["fabric_n"] = image("fabric_n", normal_map(w, 0.8))
    # Grained plastic (dash face, console).
    g = tile_noise(256, 1.4, 1.4, 8)
    T["grain"] = image("grain", rgb(0.032 + 0.003 * g, (1, 1, 1)))
    T["grain_n"] = image("grain_n", normal_map(g, 0.5))
    # Dials: tacho, speedo (top row), oil, water (bottom left).
    d = np.zeros((512, 512, 3))
    draw_dial(d, 0, 256, 256, 16, [(2 * k, str(k)) for k in range(9)], 14 / 16)
    draw_dial(d, 256, 256, 256, 20, [(4 * k, str(40 * k)) for k in range(6)], 2.0)
    draw_dial(d, 0, 0, 128, 4, [], 0.75, minors=False)
    draw_dial(d, 128, 0, 128, 4, [], 0.75, minors=False)
    T["dials"] = image("dials", d)
    # Trip computer LCD: distance and split.
    lcd = np.zeros((128, 256, 3)) + np.array((0.035, 0.018, 0.0))
    draw_text(lcd, "12.47", 128, 86, 46, (1.0, 0.55, 0.12))
    draw_text(lcd, "0.38", 150, 30, 30, (1.0, 0.55, 0.12))
    T["lcd"] = image("lcd", lcd)
    return T


def materials():
    """Defines the cockpit's materials (and the shared basics if the caller hasn't)."""
    basics = {
        "Black": dict(color=lin((0.035, 0.035, 0.04)), rough=0.6),
        "Accent": dict(color=lin((1.0, 0.42, 0.04)), rough=0.45),
        "Metal": dict(color=lin((0.62, 0.63, 0.65)), rough=0.35, metal=1.0),
        "Cage": dict(color=lin((0.80, 0.81, 0.82)), rough=0.4, metal=0.3),
        "Gauge": dict(color=lin((0.02, 0.02, 0.025)), rough=0.3),
        "Needle": dict(color=lin((1.0, 0.45, 0.1)), rough=0.4, emit=(1.0, 0.45, 0.1, 3.0)),
        "Tick": dict(color=lin((0.9, 0.9, 0.9)), rough=0.5, emit=(0.9, 0.9, 0.9, 1.5)),
    }
    for k, v in basics.items():
        if k not in MATS:
            material(k, **v)
    T = textures()
    material("CabinPaint", lin((0.62, 0.63, 0.64)), rough=0.55)
    material("Suede", (1, 1, 1), rough=0.95, image=T["suede"], normal=T["suede_n"])
    material("Carbon", (1, 1, 1), rough=0.28, image=T["carbon"], normal=T["carbon_n"])
    material("Alu", (1, 1, 1), rough=0.38, metal=1.0, image=T["alu"], normal=T["alu_n"])
    material("Tread", (1, 1, 1), rough=0.45, metal=1.0, image=T["tread"], normal=T["tread_n"])
    material("SeatFabric", (1, 1, 1), rough=0.92, image=T["fabric"], normal=T["fabric_n"])
    material("SeatShell", lin((0.03, 0.03, 0.035)), rough=0.35)
    material("DashPlastic", (1, 1, 1), rough=0.7, image=T["grain"], normal=T["grain_n"])
    material("Foam", (1, 1, 1), rough=0.95, image=T["suede"], normal=T["suede_n"])
    material("Harness", lin((0.78, 0.10, 0.06)), rough=0.8)
    material("Rubber", lin((0.03, 0.03, 0.03)), rough=0.9)
    material("Red", lin((0.70, 0.04, 0.03)), rough=0.35)
    material("Mirror", lin((0.9, 0.9, 0.92)), rough=0.04, metal=1.0)
    material("Dial", (1, 1, 1), rough=0.25, image=T["dials"], emit_image=T["dials"], emit=(1, 1, 1, 0.5))
    material("Lcd", lin((0.02, 0.02, 0.02)), rough=0.2, emit_image=T["lcd"], emit=(1, 1, 1, 1.4))
    material("LedG", lin((0.1, 0.9, 0.2)), rough=0.3, emit=(0.1, 1.0, 0.2, 0.8))
    material("LedA", lin((1.0, 0.6, 0.05)), rough=0.3, emit=(1.0, 0.6, 0.05, 0.8))
    material("LedR", lin((1.0, 0.08, 0.05)), rough=0.3, emit=(1.0, 0.08, 0.05, 0.8))
    TILE.update({"Suede": 0.10, "Carbon": 0.064, "Alu": 0.15, "Tread": 0.12, "SeatFabric": 0.04, "DashPlastic": 0.08, "Foam": 0.10})


# ---------------------------------------------------------------- static interior


def inner_skin(src, keep):
    """The body's cabin faces, moved 3.5 cm in along their normals and flipped, as a lining
    seen from inside (the body is single-sided). `keep(face, centre)` picks the faces."""
    me = src.data
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.transform(src.matrix_world)
    bm.normal_update()
    for v in bm.verts:
        v.co -= v.normal * 0.035
    gone = [f for f in bm.faces if not keep(f, f.calc_center_median())]
    bmesh.ops.delete(bm, geom=gone, context="FACES")
    bmesh.ops.reverse_faces(bm, faces=bm.faces)
    M = Mesh()
    bm.verts.index_update()
    M.v = [tuple(v.co) for v in bm.verts]
    M.uv = [None] * len(M.v)
    for f in bm.faces:
        M.f.append([v.index for v in f.verts])
        M.m.append("CabinPaint")
    bm.free()
    return M


def dash(M, D, cab):
    # Profile in (y, height), swept across the car: suede top, grained face.
    prof = [(0.62, 0.905), (0.42, 0.985), (0.24, 0.975), (0.18, 0.94), (0.20, 0.74), (0.30, 0.56), (0.64, 0.50)]
    mats = ["Suede", "Suede", "Suede", "DashPlastic", "DashPlastic", "DashPlastic", "DashPlastic"]
    xs = np.linspace(-0.80, 0.80, 17)
    M.loft([[Vector((x, y, zg(h))) for y, h in prof] for x in xs], mats, caps=True)
    # Driver's binnacle hood and gauge panel.
    hood = [(DRV_X + 0.21 * math.cos(t), zg(0.90) + 0.12 * math.sin(t)) for t in np.linspace(0, math.pi, 15)]
    rings = []
    for y in (0.13, 0.24, 0.34):
        rings.append([Vector((x, y, z)) for x, z in hood] + [Vector((DRV_X + (x - DRV_X) * 0.9, y, zg(0.90) + (z - zg(0.90)) * 0.9)) for x, z in reversed(hood)])
    M.loft(rings, "Suede", closed=True, caps=True)
    M.box((DRV_X, 0.205, zg(0.87)), (0.40, 0.02, 0.27), "Gauge")
    # Dial faces (textured), chrome bezels, needles are nodes (add_nodes).
    for c, (u0, v0, s) in ((TACHO_C, (0.0, 0.5, 0.5)), (SPEEDO_C, (0.5, 0.5, 0.5))):
        disc(D, c, DIAL_R, (u0 + s / 2, v0 + s / 2), s / 2 * 0.985, "Dial")
        M.torus(c + Vector((0, -0.006, 0)), (0, 1, 0), DIAL_R + 0.003, 0.0045, 32, 6, "Alu")
    for dx, (u0, v0) in ((-0.168, (0.0, 0.0)), (0.168, (0.25, 0.0))):
        c = Vector((DRV_X + dx, 0.19, zg(0.80)))
        disc(D, c, 0.027, (u0 + 0.125, v0 + 0.125), 0.124, "Dial")
        M.torus(c + Vector((0, -0.005, 0)), (0, 1, 0), 0.029, 0.003, 20, 5, "Alu")
        a = math.radians(-35 if dx < 0 else 20)
        tip = c + Vector((math.sin(a) * 0.022, -0.008, math.cos(a) * 0.022))
        D.add([c + Vector((-0.0015, -0.008, 0)), c + Vector((0.0015, -0.008, 0)), tip], [(0, 1, 2)], "Needle")
    # Shift lights above the dials: 3 green, 2 amber, 2 red.
    for k, m in enumerate(["LedG"] * 3 + ["LedA"] * 2 + ["LedR"] * 2):
        x = DRV_X - 0.075 + 0.025 * k
        M.cyl((x, 0.193, zg(0.935)), (x, 0.186, zg(0.935)), 0.0065, 10, m)
    # Steering column with its shroud.
    a, b = STEER_C + STEER_AX * 0.13, STEER_C + STEER_AX * 0.36
    M.cyl(a, b, 0.028, 12, "Black")
    M.box(STEER_C + STEER_AX * 0.27, (0.10, 0.20, 0.09), "DashPlastic", rot=Matrix.Rotation(-STEER_TILT, 3, "X"))


def disc(M, c, r, uvc, uvr, mat, n=28):
    """A flat round face at c facing the driver (−Y), UV-mapped round uvc. Flat faces go in
    the decal mesh (no normal recalculation: their winding sets their facing)."""
    ts = np.linspace(0, 2 * math.pi, n, endpoint=False)
    vs = [c + Vector((r * math.cos(t), -0.004, r * math.sin(t))) for t in ts]
    uvs = [(uvc[0] + uvr * math.cos(t), uvc[1] + uvr * math.sin(t)) for t in ts]
    M.add(vs, [tuple(range(n))], mat, uvs=uvs)


def quad(M, c, rot, w, h, mat, uv=((0, 0), (1, 0), (1, 1), (0, 1))):
    """A flat w×h face centred at c facing local −Y (after `rot`)."""
    vs = [c + rot @ Vector(p) for p in ((-w / 2, 0, -h / 2), (w / 2, 0, -h / 2), (w / 2, 0, h / 2), (-w / 2, 0, h / 2))]
    M.add(vs, [(0, 1, 2, 3)], mat, uvs=list(uv))


def console(M, cab):
    # Transmission tunnel and the console on it.
    hump = [(-0.17, 0.0), (-0.15, 0.13), (-0.09, 0.18), (0.09, 0.18), (0.15, 0.13), (0.17, 0.0)]
    M.loft([[Vector((-0.02 + a, y, zg(cab.floor + b))) for a, b in hump] for y in np.linspace(-1.0, cab.firewall_y, 9)], "CabinPaint", caps=True)
    M.box((-0.02, 0.0, zg(0.44)), (0.22, 0.50, 0.28), "DashPlastic")
    # Switch panel: two rows of toggles (two under red guards), rotaries, the kill switch.
    rot = Matrix.Rotation(math.radians(-15), 3, "X")
    pc = Vector((-0.02, 0.19, zg(0.72)))
    M.box(pc, (0.30, 0.03, 0.18), "Carbon", rot=rot)

    def at(u, v, d=0.0):
        return pc + rot @ Vector((u, -0.015 - d, v))

    for k in range(6):
        u = -0.11 + 0.044 * k
        toggle(M, at, u, 0.040, rot, guard=k < 2)
    for k in range(3):
        toggle(M, at, -0.11 + 0.044 * k, -0.025, rot)
    for u in (0.022, 0.066):
        M.cyl(at(u, -0.025), at(u, -0.025, 0.018), 0.011, 14, "Black")
        M.box(at(u, -0.017, 0.019), (0.003, 0.002, 0.008), "Accent", rot=rot)
    M.cyl(at(0.11, -0.025), at(0.11, -0.025, 0.014), 0.017, 16, "Red")
    M.cyl(at(0.11, -0.025), at(0.11, -0.025, 0.004), 0.022, 16, "Accent")
    # Sequential shifter housing and the hydraulic handbrake's master cylinder and reservoir.
    M.box(SHIFT_PIVOT + Vector((0, 0.0, -0.005)), (0.075, 0.13, 0.03), "Alu")
    M.cyl(SHIFT_PIVOT - Vector((0, 0, 0.005)), SHIFT_PIVOT + Vector((0, 0, 0.04)), 0.032, 12, "Rubber", r1=0.014)
    M.box(HB_PIVOT + Vector((0, 0.0, 0.0)), (0.05, 0.08, 0.035), "Alu")
    M.cyl(HB_PIVOT + Vector((0, 0.03, 0.012)), HB_PIVOT + Vector((0, 0.13, 0.012)), 0.014, 10, "Alu")
    res = HB_PIVOT + Vector((0.055, 0.06, 0.0))
    M.cyl(res, res + Vector((0, 0, 0.065)), 0.018, 12, "Accent")
    M.cyl(res + Vector((0, 0, 0.065)), res + Vector((0, 0, 0.08)), 0.02, 12, "Black")


def toggle(M, at, u, v, rot, guard=False):
    M.box(at(u, v, 0.002), (0.016, 0.006, 0.016), "Black", rot=rot)
    M.cyl(at(u, v, 0.004), at(u, v + 0.008, 0.026), 0.0032, 6, "Metal")
    M.box(at(u, v + 0.022, 0.0), (0.02, 0.002, 0.006), "Tick", rot=rot)
    if guard:
        M.box(at(u, v + 0.004, 0.014), (0.022, 0.004, 0.026), "Red", rot=rot @ Matrix.Rotation(math.radians(-35), 3, "X"))


def footwell(M, cab):
    fl = cab.floor
    # Floor and firewall (boxes, so AO and normals behave).
    M.box((0, -0.55, zg(fl - 0.01)), (1.62, 2.40, 0.02), "CabinPaint")
    M.box((0, cab.firewall_y + 0.01, zg(0.58)), (1.62, 0.02, 0.56), "CabinPaint")  # under the cowl
    # Driver's pedals (drilled alloy pads on black arms) and dead pedal.
    for x, top in ((DRV_X - 0.13, 0.43), (DRV_X - 0.01, 0.43), (DRV_X + 0.11, 0.40)):
        pad = Vector((x, 0.43, zg(top)))
        M.cyl(pad + Vector((0, 0.02, 0.02)), Vector((x, 0.56, zg(0.66))), 0.009, 6, "Black")
        M.box(pad, (0.06, 0.012, 0.085), "Alu", rot=Matrix.Rotation(math.radians(25), 3, "X"))
    M.box((DRV_X - 0.25, 0.42, zg(0.38)), (0.08, 0.012, 0.16), "Tread", rot=Matrix.Rotation(math.radians(40), 3, "X"))
    # Co-driver's footrest: an inclined tread plate with a heel bar.
    M.box((-DRV_X, 0.47, zg(0.39)), (0.40, 0.012, 0.24), "Tread", rot=Matrix.Rotation(math.radians(48), 3, "X"))
    M.cyl((-DRV_X - 0.20, 0.36, zg(0.33)), (-DRV_X + 0.20, 0.36, zg(0.33)), 0.012, 8, "Alu")
    # Fire extinguisher against the tunnel, co-driver's side.
    e0, e1 = Vector((0.17, -0.62, zg(0.37))), Vector((0.17, -0.30, zg(0.37)))
    M.cyl(e0, e1, 0.045, 16, "Red")
    M.cyl(e1, e1 + Vector((0, 0.03, 0)), 0.03, 12, "Black")
    M.cyl(e1 + Vector((0, 0.03, 0.0)), e1 + Vector((0, 0.07, 0.03)), 0.008, 6, "Black")
    for y in (-0.56, -0.38):
        M.torus((0.17, y, zg(0.37)), (0, 1, 0), 0.048, 0.006, 16, 4, "Alu")


def doors(M, cab):
    for sx in (-1, 1):
        x = sx * cab.side_x
        y0, y1 = cab.door_y
        h0, h1 = cab.floor + 0.06, cab.belt - 0.02
        M.box((x, (y0 + y1) / 2, zg((h0 + h1) / 2)), (0.012, y0 - y1, h1 - h0), "Carbon")
        # Pull strap and the release lever.
        M.strap([(x - sx * 0.008, 0.10, zg(0.74)), (x - sx * 0.035, -0.05, zg(0.71)), (x - sx * 0.008, -0.20, zg(0.74))], 0.03, 0.004, (sx, 0, 0), "Accent")
        M.box((x - sx * 0.012, 0.32, zg(0.80)), (0.012, 0.09, 0.02), "Alu")


def bucket(M, x, cab, H=None):
    """A fibreglass bucket seat: back and cushion shells (black outside, fabric inside); its
    harness goes in `H` if given (else in `M`)."""
    rec = math.radians(14)
    up = Vector((0, -math.sin(rec), math.cos(rec)))
    fwd = Vector((0, math.cos(rec), math.sin(rec)))
    base = Vector((x, -0.63 + SEAT_DY, zg(0.43)))
    L = 0.88
    keys = [(0.0, 0.25, 0.16), (0.3, 0.245, 0.12), (0.55, 0.262, 0.11), (0.66, 0.235, 0.07), (0.72, 0.20, 0.07),
            (0.80, 0.175, 0.12), (0.92, 0.175, 0.13), (1.0, 0.16, 0.09)]
    mats = ["SeatShell"] * 7 + ["SeatFabric"] * 6 + ["SeatShell"]
    rings = []
    for s in np.linspace(0, 1, 17):
        w = float(np.interp(s, [k[0] for k in keys], [k[1] for k in keys]))
        d = float(np.interp(s, [k[0] for k in keys], [k[2] for k in keys]))
        o = base + up * (L * s)
        rings.append([o + Vector((a, 0, 0)) + fwd * b for a, b in u_profile(w, d, 0.03)])
    M.loft(rings, mats, caps=True)
    # Cushion: the same U, lying down, bolsters lower towards the front.
    rings = []
    for y, hb, hh in ((-0.80, 0.15, 0.43), (-0.70, 0.13, 0.42), (-0.58, 0.11, 0.42), (-0.46, 0.09, 0.42), (-0.38, 0.07, 0.41)):
        o = Vector((x, y + 0.17 + SEAT_DY, zg(hh - 0.02)))
        rings.append([o + Vector((a, 0, b)) for a, b in u_profile(0.25, hb, 0.03)])
    M.loft(rings, mats, caps=True)
    # Pads, side mounts, floor rails.
    M.box((x, -0.42 + SEAT_DY, zg(0.425)), (0.36, 0.40, 0.05), "SeatFabric")
    M.box(base + up * (L * 0.35) + fwd * 0.005, (0.36, 0.04, 0.50), "SeatFabric", rot=Matrix.Rotation(-rec, 3, "X"))
    for sx in (-1, 1):
        M.box((x + sx * 0.265, -0.45 + SEAT_DY, zg(0.40)), (0.01, 0.30, 0.10), "Alu")
        M.box((x + sx * 0.18, -0.41 + SEAT_DY, zg(cab.floor + 0.03)), (0.03, 0.52, 0.06), "Metal")
    harness(H if H is not None else M, x, base, up, fwd, L, cab)


def u_profile(w, d, t):
    """Closed U section (across, depth): outer skin then inner (fabric) skin."""
    outer = [(-w, d), (-w, 0.0), (-0.6 * w, -0.045), (0.0, -0.055), (0.6 * w, -0.045), (w, 0.0), (w, d)]
    inner = [(w - t, d), (w - t, 0.005), (0.55 * w, -0.018), (0.0, -0.026), (-0.55 * w, -0.018), (-w + t, 0.005), (-w + t, d)]
    return outer + inner


def harness(M, x, base, up, fwd, L, cab):
    """Six-point harness: shoulder straps from the harness bar, lap belts, crotch strap."""
    buckle = Vector((x, -0.38 + SEAT_DY, zg(0.50)))
    for sx in (-1, 1):
        pts = [Vector((x + sx * 0.09, cab.hoop_y, zg(1.00))),
               base + up * (L * 0.70) + fwd * 0.01 + Vector((sx * 0.08, 0, 0)),
               base + up * (L * 0.45) + fwd * 0.035 + Vector((sx * 0.09, 0, 0)),
               base + up * (L * 0.15) + fwd * 0.06 + Vector((sx * 0.07, 0, 0)),
               buckle + Vector((sx * 0.03, -0.05, 0.01))]
        M.strap(pts, 0.05, 0.004, fwd, "Harness")
        M.strap([Vector((x + sx * 0.24, -0.55 + SEAT_DY, zg(0.50))), Vector((x + sx * 0.14, -0.45 + SEAT_DY, zg(0.475))), buckle + Vector((sx * 0.035, 0, 0))], 0.05, 0.004, (0, 0, 1), "Harness")
    M.strap([Vector((x, -0.23 + SEAT_DY, zg(0.44))), Vector((x, -0.30 + SEAT_DY, zg(0.47))), buckle + Vector((0, 0.03, 0))], 0.045, 0.004, (0, 0, 1), "Harness")
    M.cyl(buckle - Vector((0, 0, 0.006)), buckle + Vector((0, 0, 0.008)), 0.035, 18, "Alu")
    M.cyl(buckle + Vector((0, 0, 0.008)), buckle + Vector((0, 0, 0.012)), 0.018, 14, "Black")


def cage(M, cab):
    """Roll cage tubes (both sides mirrored), padded where heads and knees are."""
    r = 0.023
    xb, xr, roof = cab.cage_x_belt, cab.cage_x_roof, cab.cage_roof
    sy, (ay, ah), by, hy = cab.screen_top_y, cab.screen_base, cab.b_pillar_y, cab.hoop_y
    fy, fh = cab.rear_foot
    side = [
        # Main hoop leg, B-pillar hoop, roof rail, A-pillar leg, door bars (X), rear stay.
        [(xb - 0.06, hy, 0.82), (xb - 0.06, hy, 1.00), (xr - 0.005, hy, roof)],
        [(xb - 0.05, by, cab.floor + 0.01), (xb - 0.06, by, 1.00), (xr, by, roof)],
        [(xr - 0.005, hy, roof), (xr, sy, roof)],
        [(xb - 0.01, ay, cab.floor + 0.02), (xb + 0.01, ay, ah), (xr, sy, roof)],
        [(xb, ay - 0.05, 0.45), (xb - 0.04, by, 0.95)],
        [(xb, ay - 0.05, 0.85), (xb - 0.04, by, 0.45)],
        [(xr - 0.005, hy, roof), (xr + 0.055, fy, fh)],
    ]
    pads = {1: (0.1, 0.9), 2: (0.04, 0.96), 3: (0.1, 0.92), 4: (0.12, 0.88), 5: (0.12, 0.88)}
    lines = [([(s * x, y, h) for x, y, h in ln], pads.get(k)) for s in (-1, 1) for k, ln in enumerate(side)]
    lines += [
        ([(-(xr - 0.005), hy, roof), (xr - 0.005, hy, roof)], None),
        ([(-xr, sy, roof), (xr, sy, roof)], (0.06, 0.94)),
        ([(xb - 0.06, hy, 0.84), (-(xr - 0.005), hy, roof)], None),
        ([(-(xb - 0.06), hy, 1.00), (xb - 0.06, hy, 1.00)], None),  # harness bar
        ([(-(xr - 0.005), hy, roof), (xr, sy, roof)], None),  # roof diagonal
    ]
    for ln, pad in lines:
        for a, b in zip(ln, ln[1:]):
            pa, pb = Vector((a[0], a[1], zg(a[2]))), Vector((b[0], b[1], zg(b[2])))
            d = (pb - pa).normalized() * r * 0.8
            M.cyl(pa - d, pb + d, r, 12, "Cage")
            if pad is not None and (len(ln) == 2 or b == ln[-1]):
                t0, t1 = pad
                M.cyl(pa.lerp(pb, t0), pa.lerp(pb, t1), 0.032, 12, "Foam")
                for t in (t0, t1):
                    p = pa.lerp(pb, t)
                    M.cyl(p - d * 0.4, p + d * 0.4, 0.0335, 12, "Black")


def codriver(M, D, cab):
    # Trip computer on the dash in front of the co-driver, tilted up towards them.
    rot = Matrix.Rotation(math.radians(-22), 3, "X")
    c = Vector((0.33, 0.16, zg(0.90)))
    M.box(c, (0.18, 0.05, 0.09), "Black", rot=rot)
    quad(D, c + rot @ Vector((0, -0.0255, 0.012)), rot, 0.15, 0.052, "Lcd")
    for k in range(4):
        M.box(c + rot @ Vector((-0.06 + 0.04 * k, -0.027, -0.03)), (0.022, 0.006, 0.012), "Tick" if k else "Accent", rot=rot)
    M.box(c + Vector((0, 0.04, -0.045)), (0.10, 0.06, 0.012), "Alu")
    # Map light on a gooseneck.
    pts = [Vector((0.16, 0.30, zg(0.975))), Vector((0.16, 0.27, zg(1.01))), Vector((0.18, 0.22, zg(1.025))), Vector((0.20, 0.18, zg(1.02)))]
    for a, b in zip(pts, pts[1:]):
        M.cyl(a, b, 0.006, 6, "Black")
    M.cyl(pts[-1], pts[-1] + Vector((0.01, -0.03, -0.02)), 0.016, 12, "Black", r1=0.02)
    # Rear-view mirror under the windscreen header.
    m = Vector((0.0, cab.screen_top_y + 0.05, zg(cab.cage_roof - 0.055)))
    M.cyl(m + Vector((0, 0, 0.04)), m + Vector((0, 0, 0.015)), 0.005, 6, "Black")
    M.box(m, (0.17, 0.025, 0.048), "Black")
    quad(D, m + Vector((0, -0.013, 0)), Matrix.Identity(3), 0.158, 0.038, "Mirror")


# ---------------------------------------------------------------- moving parts


def add_nodes(root, parts=("wheel", "dials", "levers"), offset=Vector()):
    """The animated nodes: steering wheel, needles, levers (`parts` picks which), all moved by
    `offset` (a body whose seat sits elsewhere: src/art/car.rs and hands.rs move the same)."""
    if "wheel" in parts:
        add_wheel(root, offset)
    if "dials" in parts:
        add_dials(root, offset)
    if "levers" in parts:
        add_levers(root, offset)


def add_wheel(root, offset=Vector()):
    # Steering wheel: a suede deep-dish three-spoke, built about its own local Y axis (rim in
    # local XZ), tilted with the column. The rim stays at RIM (the hands grip it there).
    sw = empty("SteeringWheel", STEER_C + offset, root, rot=(-STEER_TILT, 0, 0))
    W = Mesh()
    W.torus((0, 0, 0), (0, 1, 0), RIM, 0.019, 48, 10, "Suede")
    W.torus((0, 0, 0), (0, 1, 0), RIM, 0.0195, 4, 10, "Accent", a0=math.radians(86), a1=math.radians(94))
    dish = 0.055
    for t in (0.0, math.pi, -math.pi / 2):  # spokes: left, right, down
        d = Vector((math.cos(t), 0, math.sin(t)))
        a, b = d * (RIM - 0.012), d * 0.045 + Vector((0, dish, 0))
        mid, ln = (a + b) / 2, (b - a).length
        q = (b - a).normalized()
        side = q.cross(Vector((0, 1, 0)) if abs(q.y) < 0.9 else Vector((1, 0, 0))).normalized()
        nrm = side.cross(q)
        basis = Matrix((side, q, nrm)).transposed()
        W.box(mid, (0.034, ln, 0.006), "Alu", rot=basis)
        for k in (0.35, 0.65):
            W.cyl(a.lerp(b, k) - nrm * 0.004, a.lerp(b, k) + nrm * 0.004, 0.008, 10, "Black")
    W.cyl((0, dish - 0.01, 0), (0, dish + 0.05, 0), 0.042, 18, "Black")
    W.cyl((0, dish - 0.016, 0), (0, dish - 0.01, 0), 0.034, 18, "Black")
    W.torus((0, dish - 0.016, 0), (0, 1, 0), 0.034, 0.003, 18, 4, "Accent")
    W.cyl((0, dish + 0.05, 0), (0, dish + 0.085, 0), 0.03, 14, "Alu")
    W.obj("SteeringWheelMesh", sw, recalc=True, smooth_angle=50)


def add_dials(root, offset=Vector()):
    # Needle nodes at the dial centres.
    for name, c in (("Tacho", TACHO_C), ("Speedo", SPEEDO_C)):
        node = empty(name, c + offset, root)
        Nd = Mesh()
        Nd.add([(-0.003, -0.009, -0.008), (0.003, -0.009, -0.008), (0.0015, -0.009, 0.050), (-0.0015, -0.009, 0.050)], [(0, 1, 2, 3)], "Needle")
        Nd.cyl((0, -0.006, 0), (0, -0.011, 0), 0.008, 10, "Black")
        Nd.obj(name + "Needle", node, recalc=False)


def add_levers(root, offset=Vector()):
    # Sequential shifter (leaning back towards the driver): shaft, reverse-lockout collar, knob.
    S = Mesh()
    top = Vector((-0.03, -0.05, 0.28))
    S.cyl((0, 0, 0), top, 0.011, 10, "Metal")
    S.cyl(top * 0.68, top * 0.74, 0.016, 12, "Accent")
    S.cyl(top, top + Vector((0, 0, 0.055)), 0.021, 14, "Black")
    S.cyl(top + Vector((0, 0, 0.055)), top + Vector((0, 0, 0.065)), 0.021, 14, "Black", r1=0.012)
    S.obj("ShifterMesh", empty("Shifter", SHIFT_PIVOT + offset, root), recalc=True, smooth_angle=40)
    # Hydraulic handbrake: short and low (top below the wheel hub), leaning back.
    H = Mesh()
    hb = Vector((-0.01, -0.04, 0.25)).normalized()
    H.cyl((0, 0, 0), hb * 0.18, 0.011, 10, "Metal")
    H.cyl(hb * 0.18, hb * 0.253, 0.016, 12, "Accent")
    H.cyl(hb * 0.253, hb * 0.258, 0.012, 12, "Black")
    H.obj("HandbrakeMesh", empty("Handbrake", HB_PIVOT + offset, root), recalc=True, smooth_angle=40)


# ---------------------------------------------------------------- assemble


def join(objs, name):
    ctx = bpy.context
    for o in ctx.view_layer.objects:
        if o is not None:
            o.select_set(False)
    for o in objs:
        o.select_set(True)
    ctx.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    ob = ctx.view_layer.objects.active
    ob.name = name
    ob.data.name = name
    return ob


def bake_ao(ob, samples=48):
    """Densifies `ob` and bakes contact ambient occlusion (30 cm) into its "AO" vertex colours."""
    me = ob.data
    bm = bmesh.new()
    bm.from_mesh(me)
    for _ in range(3):
        long = [e for e in bm.edges if e.calc_length() > 0.12]
        if not long:
            break
        bmesh.ops.subdivide_edges(bm, edges=long, cuts=1, use_grid_fill=True)
    bm.to_mesh(me)
    bm.free()
    finish(ob, 35)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"
    sc.cycles.device = "CPU"
    sc.cycles.samples = samples
    if sc.world is None:
        sc.world = bpy.data.worlds.new("World")
    sc.world.light_settings.distance = 0.30
    attr = me.color_attributes.new("AO", "BYTE_COLOR", "CORNER")
    me.color_attributes.active_color = attr
    for o in bpy.context.view_layer.objects:
        if o is not None:
            o.select_set(False)
    ob.select_set(True)
    bpy.context.view_layer.objects.active = ob
    bpy.ops.object.bake(type="AO", target="VERTEX_COLORS")
    c = np.zeros(len(attr.data) * 4, dtype=np.float32)
    attr.data.foreach_get("color", c)
    c = c.reshape(-1, 4)
    c[:, :3] = 0.30 + 0.70 * c[:, :3]
    c[:, 3] = 1.0
    attr.data.foreach_set("color", c.ravel())


def build(root, skin=None, cab=Cabin, bake=True, seat_harness=False):
    """The cockpit under `root`: a static "Interior" (with `skin`, the cabin lining, if given)
    and the moving nodes. With `seat_harness`, the passenger seat's harness is its own mesh
    "SeatHarness" under `root`. Call materials() first."""
    M, D = Mesh(), Mesh()
    H = Mesh() if seat_harness else None
    dash(M, D, cab)
    console(M, cab)
    footwell(M, cab)
    doors(M, cab)
    for x in (DRV_X, -DRV_X):
        bucket(M, x, cab, H if x > 0 else None)
    cage(M, cab)
    codriver(M, D, cab)
    parts = [M.obj("InteriorParts", recalc=True, smooth_angle=35), D.obj("InteriorDecals", recalc=False)]
    if skin is not None:
        parts.append(skin.obj("CabinLining", recalc=False, smooth_angle=35))
    ob = join(parts, "Interior")
    ob.name = "Interior"
    ob.parent = root
    if bake:
        bake_ao(ob)
    if H is not None:
        H.obj("SeatHarness", root, recalc=True, smooth_angle=40)
    add_nodes(root)
    return ob
