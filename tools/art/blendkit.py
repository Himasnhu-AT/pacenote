# Small Blender helpers shared by the car scripts (build_rallycar.py, cockpit.py,
# import_sketchfab.py): polygon accumulation with materials and UVs, materials, empties.
# Blender frame: X right, Y forward, Z up, metres.
import math

import bmesh
import bpy
from mathutils import Vector

MATS = {}
# Metres per texture repeat for box-projected materials (filled in by cockpit.py).
TILE = {}


def link(ob):
    bpy.context.scene.collection.objects.link(ob)


def lin(c):
    """sRGB → linear (material colours are linear)."""
    return tuple(((x + 0.055) / 1.055) ** 2.4 if x > 0.04045 else x / 12.92 for x in c)


def material(name, color, rough=0.5, metal=0.0, emit=None, alpha=None, image=None, normal=None, emit_image=None, coat=None, double=False):
    """A Principled material. `image` drives base colour, `normal` a tangent-space normal map,
    `emit_image` the emission colour (with strength emit[3] if given, else 1), `coat` a clear
    coat (weight, roughness; glTF KHR_materials_clearcoat). `double`: drawn from both sides."""
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    m.use_backface_culling = not double
    m.diffuse_color = (*color[:3], 1.0)
    nt = m.node_tree
    bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Base Color"].default_value = (*color[:3], 1.0)
    bsdf.inputs["Roughness"].default_value = rough
    bsdf.inputs["Metallic"].default_value = metal
    if emit is not None:
        bsdf.inputs["Emission Color"].default_value = (*emit[:3], 1.0)
        bsdf.inputs["Emission Strength"].default_value = emit[3]
    if coat is not None:
        bsdf.inputs["Coat Weight"].default_value = coat[0]
        bsdf.inputs["Coat Roughness"].default_value = coat[1]
    if alpha is not None:
        bsdf.inputs["Alpha"].default_value = alpha
        m.surface_render_method = "BLENDED"
    if image is not None:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = image
        nt.links.new(tex.outputs["Color"], bsdf.inputs["Base Color"])
    if normal is not None:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = normal
        normal.colorspace_settings.name = "Non-Color"
        nm = nt.nodes.new("ShaderNodeNormalMap")
        nt.links.new(tex.outputs["Color"], nm.inputs["Color"])
        nt.links.new(nm.outputs["Normal"], bsdf.inputs["Normal"])
    if emit_image is not None:
        tex = nt.nodes.new("ShaderNodeTexImage")
        tex.image = emit_image
        nt.links.new(tex.outputs["Color"], bsdf.inputs["Emission Color"])
        if emit is None:
            bsdf.inputs["Emission Strength"].default_value = 1.0
    MATS[name] = m
    return m


def image(name, rgb):
    """A packed PNG image from an (n, m, 3|4) float array (row 0 = bottom)."""
    import numpy as np
    h, w = rgb.shape[:2]
    if rgb.shape[2] == 3:
        rgb = np.concatenate([rgb, np.ones((h, w, 1))], axis=2)
    im = bpy.data.images.new(name, w, h, alpha=False)
    im.pixels.foreach_set(rgb.astype(np.float32).ravel())
    im.file_format = "PNG"
    im.pack()
    return im


def finish(ob, smooth_angle):
    me = ob.data
    if smooth_angle is None:
        for p in me.polygons:
            p.use_smooth = False
    else:
        me.shade_smooth()
        me.set_sharp_from_angle(angle=math.radians(smooth_angle))


def empty(name, loc, parent, rot=None):
    e = bpy.data.objects.new(name, None)
    link(e)
    e.parent = parent
    e.location = loc
    if rot is not None:
        e.rotation_euler = rot
    return e


def box_uv(p, n, size):
    """Box-projected UV of local point p on a face with normal n, `size` metres per repeat."""
    ax = max(range(3), key=lambda i: abs(n[i]))
    if ax == 0:
        u, v = p.y, p.z
    elif ax == 1:
        u, v = p.x, p.z
    else:
        u, v = p.x, p.y
    return (u / size, v / size)


class Mesh:
    """Accumulates polygons (with a material name each, optional per-vertex UVs), then becomes
    one object. Faces without explicit UVs get box-projected UVs if their material is in TILE."""

    def __init__(self):
        self.v, self.f, self.m, self.uv = [], [], [], []

    def add(self, verts, faces, mat, uvs=None):
        o = len(self.v)
        self.v += [tuple(p) for p in verts]
        self.uv += list(uvs) if uvs is not None else [None] * len(verts)
        for f in faces:
            self.f.append([o + i for i in f])
            self.m.append(mat)

    def box(self, c, s, mat, rot=None):
        """Box centred at c, size s (rotated by the 3x3 `rot` about c)."""
        from mathutils import Matrix
        rot = rot or Matrix.Identity(3)
        hx, hy, hz = s[0] / 2, s[1] / 2, s[2] / 2
        corners = [Vector((x, y, z)) for x in (-hx, hx) for y in (-hy, hy) for z in (-hz, hz)]
        vs = [Vector(c) + rot @ q for q in corners]
        faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
        self.add(vs, faces, mat)

    def cyl(self, p0, p1, r, n, mat, caps=True, r1=None):
        p0, p1 = Vector(p0), Vector(p1)
        ax = (p1 - p0).normalized()
        a = ax.orthogonal().normalized()
        b = ax.cross(a)
        r1 = r if r1 is None else r1
        vs = []
        for p, rr in ((p0, r), (p1, r1)):
            for i in range(n):
                t = 2 * math.pi * i / n
                vs.append(p + (a * math.cos(t) + b * math.sin(t)) * rr)
        faces = [(i, (i + 1) % n, n + (i + 1) % n, n + i) for i in range(n)]
        if caps:
            faces += [tuple(range(n))[::-1], tuple(range(n, 2 * n))]
        self.add(vs, faces, mat)

    def lathe(self, prof, n, mat, center=(0, 0, 0), radius_fn=None):
        """Revolve a profile [(x, r)] about the X axis through `center`."""
        c = Vector(center)
        vs = []
        for k in range(n):
            t = 2 * math.pi * k / n
            for j, (x, r) in enumerate(prof):
                rr = radius_fn(k, j, r) if radius_fn else r
                vs.append(c + Vector((x, rr * math.cos(t), rr * math.sin(t))))
        m = len(prof)
        faces = []
        for k in range(n):
            k2 = (k + 1) % n
            for j in range(m - 1):
                faces.append((k * m + j, k * m + j + 1, k2 * m + j + 1, k2 * m + j))
        self.add(vs, faces, mat)

    def sweep(self, prof, frames, mat, closed=True, caps=False):
        """Sweep a 2D profile [(a, b)] along frames [(origin, A, B)]: point = o + a*A + b*B."""
        self.loft([[o + A * a + B * b for (a, b) in prof] for (o, A, B) in frames], mat, closed, caps)

    def loft(self, rings, mat, closed=True, caps=False):
        """Skin rings of equal size. `mat` is a name, or a list with one name per ring segment."""
        m = len(rings[0])
        vs = [p for r in rings for p in r]
        o = len(self.v)
        self.v += [tuple(p) for p in vs]
        self.uv += [None] * len(vs)
        for s in range(len(rings) - 1):
            for j in range(m if closed else m - 1):
                j2 = (j + 1) % m
                self.f.append([o + s * m + j, o + s * m + j2, o + (s + 1) * m + j2, o + (s + 1) * m + j])
                self.m.append(mat if isinstance(mat, str) else mat[j])
        if caps:
            cap = mat if isinstance(mat, str) else mat[0]
            self.f.append([o + i for i in range(m)][::-1])
            self.m.append(cap)
            last = o + (len(rings) - 1) * m
            self.f.append([last + i for i in range(m)])
            self.m.append(cap)

    def torus(self, c, axis, rad, tube, n, k, mat, a0=0.0, a1=2 * math.pi):
        c, axis = Vector(c), Vector(axis).normalized()
        u = axis.orthogonal().normalized()
        w = axis.cross(u)
        full = abs(a1 - a0 - 2 * math.pi) < 1e-6
        steps = n if full else n + 1
        frames = []
        for i in range(steps):
            t = a0 + (a1 - a0) * i / n
            d = u * math.cos(t) + w * math.sin(t)
            frames.append((c + d * rad, d, axis))
        prof = [(tube * math.cos(2 * math.pi * j / k), tube * math.sin(2 * math.pi * j / k)) for j in range(k)]
        if full:
            frames.append(frames[0])
        self.sweep(prof, frames, mat)

    def strap(self, pts, width, thick, up, mat):
        """A flat ribbon through points `pts`, its face normal near `up`."""
        pts = [Vector(p) for p in pts]
        up = Vector(up)
        frames = []
        for i, p in enumerate(pts):
            t = (pts[min(i + 1, len(pts) - 1)] - pts[max(i - 1, 0)]).normalized()
            side = t.cross(up)
            if side.length < 1e-4:
                side = t.orthogonal()
            side.normalize()
            nrm = side.cross(t).normalized()
            frames.append((p, side, nrm))
        w, h = width / 2, thick / 2
        self.sweep([(-w, -h), (w, -h), (w, h), (-w, h)], frames, mat, caps=True)

    def obj(self, name, parent=None, recalc=True, smooth_angle=None):
        me = bpy.data.meshes.new(name)
        me.from_pydata(self.v, [], self.f)
        me.update()
        ob = bpy.data.objects.new(name, me)
        link(ob)
        names = list(dict.fromkeys(self.m))
        for nm in names:
            me.materials.append(MATS[nm])
        idx = {nm: i for i, nm in enumerate(names)}
        for p, nm in zip(me.polygons, self.m):
            p.material_index = idx[nm]
        # UVs (before welding, while loops still map to the vertices as given).
        if any(u is not None for u in self.uv) or any(nm in TILE for nm in names):
            uvl = me.uv_layers.new(name="UVMap").data
            for p, nm in zip(me.polygons, self.m):
                vids = [me.loops[li].vertex_index for li in p.loop_indices]
                if all(self.uv[i] is not None for i in vids):
                    for li, i in zip(p.loop_indices, vids):
                        uvl[li].uv = self.uv[i]
                elif nm in TILE:
                    for li, i in zip(p.loop_indices, vids):
                        uvl[li].uv = box_uv(me.vertices[i].co, p.normal, TILE[nm])
        bm = bmesh.new()
        bm.from_mesh(me)
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
        if recalc:
            bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        bm.to_mesh(me)
        bm.free()
        finish(ob, smooth_angle)
        if parent is not None:
            ob.parent = parent
        return ob
