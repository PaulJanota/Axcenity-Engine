"""Hair styles: a cap over the scalp (so no skin shows between clumps) and
volumes of smooth, tapering clumps grown from the scalp along the style's
comb direction, kept off the head and shoulders. The last part of each
clump takes the secondary colour (`HairSecondary`), for highlights.

Every style is on the Head bone and follows the head's frame and shape
sliders (`build.py`).
"""

import math
import random

import bmesh
import bpy
from mathutils import Vector

import materials
from common import FS_EAR, FS_LID_INNER, FS_LID_OUTER, FS_NOSE, new_object, remesh, smoothstep, v


class Head:
    """The head's frame for combing: its centre, the hairline and the scalp."""

    def __init__(self, P, normals, lm, face, joints, follow):
        self.P, self.N, self.follow = P, normals, follow
        h, crown = joints["Head"][0], joints["Head"][1]
        eye = face.eyes[1]
        self.c = v(0, h.y - 0.008, h.z + (crown.z - h.z) * 0.52)
        brow_top = eye.top_z + 0.016
        ear = [P[i] for i in lm.verts(FS_EAR, 1)]
        ear_top = max(p.z for p in ear)
        ear_mid = sum((p.z for p in ear)) / len(ear)
        neck_top = h.z
        # Hairline height over the turn round the head (0 front, 180 back).
        self.line = [(0, brow_top + 0.050), (30, brow_top + 0.047), (50, brow_top + 0.032),
                     (68, eye.c.z + 0.012), (82, ear_mid - 0.004), (92, ear_top + 0.002),
                     (122, ear_top + 0.004), (150, neck_top + 0.055), (180, neck_top + 0.028)]
        skip = set(lm.verts(FS_EAR)) | set(lm.verts(FS_NOSE)) | set(lm.verts(FS_LID_OUTER)) | set(lm.verts(FS_LID_INNER))
        self.scalp = [i for i, p in enumerate(P)
                      if i not in skip and p.z > neck_top - 0.03 and p.z > self.hairline(p) and abs(p.x) < 0.2
                      and (p - self.c).length < 0.16]

    def turn(self, p):
        return math.degrees(math.atan2(abs(p.x), -(p.y - self.c.y)))

    def hairline(self, p):
        a = self.turn(p)
        pts = self.line
        for (a0, z0), (a1, z1) in zip(pts, pts[1:]):
            if a0 <= a <= a1:
                t = (a - a0) / (a1 - a0)
                return z0 + (z1 - z0) * t
        return pts[-1][1]

    def radial(self, p):
        return (p - self.c).normalized()

    def push_out(self, p, gap):
        loc, nrm, _, _ = self.follow.bvh.find_nearest(p)
        if loc is not None and (p - loc).dot(nrm) < gap:
            return loc + nrm * gap
        return p


def seeds(head, spacing, rng, where=lambda p: True):
    idx = [i for i in head.scalp if where(head.P[i])]
    rng.shuffle(idx)
    chosen = []
    for i in idx:
        p = head.P[i]
        if all((p - head.P[j]).length > spacing for j in chosen):
            chosen.append(i)
    return chosen


def grow(head, i, length, comb, steps=9, lift=0.6, gap=0.006, wave=None):
    """A clump's centre line from scalp vertex `i`: `comb(p, t)` gives the
    direction at `p`, `t` from 0 at the root to 1 at the tip."""
    p0 = head.P[i] + head.N[i] * 0.004
    pts = [p0]
    p = p0
    seg = length / steps
    for k in range(steps):
        t = (k + 1) / steps
        d = comb(p, t) + head.radial(p) * lift * (1 - t) ** 2
        d.normalize()
        q = p + d * seg
        if wave:
            q += wave(p, t)
        q = head.push_out(q, gap + 0.004 * (1 - t))
        pts.append(q)
        p = q
    return pts


def tube(bm, head, pts, width, sides=6, flat=0.45, tip_from=0.72):
    """A tapering, flattened clump along `pts`; faces after `tip_from` of the
    length are marked for the secondary colour."""
    rings = []
    n = len(pts)
    for k, p in enumerate(pts):
        t = k / (n - 1)
        tan = (pts[min(k + 1, n - 1)] - pts[max(k - 1, 0)]).normalized()
        rad = head.radial(p)
        side = tan.cross(rad).normalized()
        up = side.cross(tan).normalized()
        w = width * (1.0 - 0.7 * t ** 2.0) * (0.85 + 0.15 * math.sin(math.pi * min(1.0, t * 1.6 + 0.2)))
        w = max(w, 0.0004)
        ring = []
        for a in range(sides):
            ang = 2 * math.pi * a / sides
            ring.append(bm.verts.new(p + side * math.cos(ang) * w + up * math.sin(ang) * w * flat))
        rings.append(ring)
    tips = []
    for k in range(n - 1):
        for a in range(sides):
            b = (a + 1) % sides
            f = bm.faces.new((rings[k][a], rings[k][b], rings[k + 1][b], rings[k + 1][a]))
            if (k + 0.5) / (n - 1) > tip_from:
                tips.append(f)
    tip = bm.verts.new(pts[-1] + (pts[-1] - pts[-2]).normalized() * width * 0.25)
    for a in range(sides):
        f = bm.faces.new((rings[-1][a], rings[-1][(a + 1) % sides], tip))
        tips.append(f)
    return tips


def cap(bm, head, gap, edge_soft=0.004, subdivide=False):
    """The scalp offset by `gap`, as a shell under the clumps."""
    scalp = set(head.scalp)
    body = bmesh.new()
    me = head.follow_mesh
    body.from_mesh(me)
    body.verts.ensure_lookup_table()
    drop = [f for f in body.faces if not all(vert.index in scalp for vert in f.verts)]
    bmesh.ops.delete(body, geom=drop, context="FACES")
    loose = [vert for vert in body.verts if not vert.link_faces]
    bmesh.ops.delete(body, geom=loose, context="VERTS")
    for layer in list(body.verts.layers.shape):
        body.verts.layers.shape.remove(layer)
    for layer in list(body.verts.layers.deform):
        body.verts.layers.deform.remove(layer)
    if subdivide:
        bmesh.ops.subdivide_edges(body, edges=body.edges[:], cuts=1, use_grid_fill=True, smooth=1.0)
    body.normal_update()
    for vert in body.verts:
        edge = smoothstep(0.0, 0.012, vert.co.z - head.hairline(vert.co))
        g = gap(vert.co) if callable(gap) else gap
        vert.co += vert.normal * (edge_soft + (g - edge_soft) * edge)
    me2 = bpy.data.meshes.new("cap")
    body.to_mesh(me2)
    body.free()
    bm.from_mesh(me2)
    bpy.data.meshes.remove(me2)


# Triangles each style may have after remeshing.
BUDGET = {"Hair_Swept": 7000, "Hair_Bob": 9000, "Hair_Long": 10000, "Hair_Crop": 7000,
          "Hair_Buzz": 2500, "Hair_Ponytail": 8000}
VOXEL = {"Hair_Swept": 0.0038, "Hair_Bob": 0.0045, "Hair_Long": 0.0048, "Hair_Crop": 0.0036,
         "Hair_Ponytail": 0.0042}


def finish(name, bm, tips, voxel=None):
    """The style as one sculpted mass: its cap and clumps merged by a voxel
    remesh (so overlapping clumps read as hair, with grooves between,
    not as tiles), relaxed and decimated to the style's budget."""
    tip_set = set(tips)
    names = ["HairSecondary" if f in tip_set else "Hair" for f in bm.faces]
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = new_object(name, me)
    materials.assign(obj, names)
    if name != "Hair_Buzz":
        remesh(obj, VOXEL[name], smooth_iterations=12, smooth_factor=0.6,
               max_triangles=BUDGET.get(name), thicken=0.006)
    else:
        for poly in obj.data.polygons:
            poly.use_smooth = True
    return obj


# -- styles -----------------------------------------------------------------------------

def swept(head):
    """Short at the back and sides; the top in long strokes swept over to
    the right from a part on the left, the front lifted into a soft quiff."""
    rng = random.Random(11)
    bm = bmesh.new()
    cap(bm, head, lambda p: 0.008 + 0.010 * smoothstep(head.c.z - 0.01, head.c.z + 0.06, p.z))
    tips = []
    part_x = 0.032
    top_z = head.c.z + 0.035

    def comb_top(p, t, side, front):
        if front > 0.5 and t < 0.4:
            return v(-0.25, 0.55, 0.8)
        back = smoothstep(0.0, 0.05, p.y - head.c.y)
        if side > 0:
            return v(0.8 - 0.5 * back, 0.4, -0.5 - 0.4 * back)
        return v(-0.75 + 0.45 * back, 0.6, -0.3 - 0.5 * back)

    for i in seeds(head, 0.019, rng, where=lambda p: p.z > top_z):
        p = head.P[i]
        side = 1 if p.x > part_x else -1
        front = smoothstep(-0.02, -0.06, p.y - head.c.y)
        back = smoothstep(0.0, 0.05, p.y - head.c.y)
        length = ((0.06 if side > 0 else 0.10) + 0.02 * front) * (1 - 0.45 * back) + rng.uniform(-0.01, 0.01)
        width = 0.032 + rng.uniform(-0.004, 0.004)
        pts = grow(head, i, length, lambda q, t, side=side, front=front: comb_top(q, t, side, front),
                   steps=8, lift=0.1 + 0.35 * front, gap=0.012)
        tips += tube(bm, head, pts, width * 0.8, flat=0.42, tip_from=0.72)
    for i in seeds(head, 0.03, rng, where=lambda p: head.c.z - 0.06 < p.z <= top_z):
        length = 0.03 + rng.uniform(-0.004, 0.004)
        pts = grow(head, i, length, lambda q, t: v(0, 0.45, -1).normalized(), steps=4, lift=0.0, gap=0.01)
        tips += tube(bm, head, pts, 0.024, flat=0.4, tip_from=0.8)
    return finish("Hair_Swept", bm, tips)


def bob(head):
    """A wavy bob to the jaw, parted on the left, ends curling in."""
    rng = random.Random(23)
    bm = bmesh.new()
    cap(bm, head, 0.013)
    tips = []
    part_x = 0.035

    def comb(p, t, side):
        out = v(math.copysign(1, p.x) * 0.25, 0.15, 0) if abs(p.x) > 0.02 else v(0, 0.2, 0)
        d = v(0, 0, -1) + out * (1 - t)
        if p.z > head.c.z + 0.04 and t < 0.35:
            d += v(-side * 0.8, 0.2, 0.1)
        if t > 0.75:
            d += (head.c - p).normalized() * 0.6 * (t - 0.75) / 0.25 + v(0, 0, -0.2)
        return d.normalized()

    def wave(p, t):
        return v(0, 0, 0) if t < 0.2 else head.radial(p) * 0.0035 * math.sin(t * 9.0)

    for i in seeds(head, 0.023, rng):
        p = head.P[i]
        side = 1 if p.x > part_x else -1
        front = smoothstep(-0.03, -0.08, p.y - head.c.y)
        length = 0.15 + rng.uniform(-0.012, 0.012) - 0.05 * front * smoothstep(head.c.z, head.c.z + 0.06, p.z)
        width = 0.014 + rng.uniform(-0.002, 0.002)
        pts = grow(head, i, length, lambda q, t, side=side: comb(q, t, side), steps=9, lift=0.5,
                   gap=0.01, wave=wave)
        tips += tube(bm, head, pts, width * 1.7, sides=6, flat=0.4, tip_from=0.7)
    return finish("Hair_Bob", bm, tips)


def long_straight(head):
    """Long straight hair past the shoulders, a centre part."""
    rng = random.Random(31)
    bm = bmesh.new()
    cap(bm, head, 0.012)
    tips = []

    def comb(p, t):
        d = v(0, 0.1, -1)
        if p.z > head.c.z + 0.03 and t < 0.25:
            d += v(math.copysign(0.9, p.x), 0.15, 0)
        if p.z < head.c.z - 0.12:
            d += v(0, 0.45, 0)  # over the shoulders, down the back
        return d.normalized()

    for i in seeds(head, 0.024, rng):
        length = 0.30 + rng.uniform(-0.02, 0.02)
        width = 0.013 + rng.uniform(-0.002, 0.002)
        pts = grow(head, i, length, comb, steps=11, lift=0.45, gap=0.01)
        tips += tube(bm, head, pts, width * 1.7, sides=6, flat=0.4, tip_from=0.75)
    return finish("Hair_Long", bm, tips)


def crop(head):
    """A short, rounded crop: a full, soft volume, a little fuller on top."""
    rng = random.Random(41)
    bumps = [(head.P[i] + head.N[i] * 0.02, rng.uniform(0.0065, 0.0085)) for i in seeds(head, 0.015, rng)]

    def gap(p):
        g = 0.014 + 0.01 * smoothstep(head.c.z - 0.03, head.c.z + 0.07, p.z)
        for c, r in bumps:
            d2 = (p - c).length_squared
            if d2 < 9 * r * r:
                g += 0.011 * math.exp(-d2 / (2 * r * r))
        return g

    bm = bmesh.new()
    cap(bm, head, gap, edge_soft=0.004, subdivide=True)
    return finish("Hair_Crop", bm, [])


def buzz(head):
    """A buzz cut: the cap alone, close to the scalp."""
    bm = bmesh.new()
    cap(bm, head, 0.0025, edge_soft=0.0012)
    return finish("Hair_Buzz", bm, [])


def ponytail(head):
    """Hair pulled back to a tie at the back of the head, and a tail."""
    rng = random.Random(53)
    bm = bmesh.new()
    cap(bm, head, 0.006)
    tips = []
    tie = head.c + v(0, 0.085, -0.005)
    tie = head.push_out(tie, 0.012)
    for i in seeds(head, 0.022, rng):
        p = head.P[i]
        to_tie = tie - p
        length = to_tie.length * 1.05
        pts = grow(head, i, length, lambda q, t: (tie - q).normalized(), steps=7, lift=0.15, gap=0.007)
        tips += tube(bm, head, pts, 0.024, flat=0.45, tip_from=2.0)
    down = v(0, 0.35, -1).normalized()
    for k in range(18):
        a = 2 * math.pi * k / 18
        start = tie + v(math.cos(a) * 0.008, 0.004, math.sin(a) * 0.008)
        pts = [start]
        p = start
        for s in range(10):
            t = (s + 1) / 10
            d = (down + v(math.cos(a) * 0.25 * t, 0.15 * (1 - t), 0)).normalized()
            p = head.push_out(p + d * 0.022, 0.01)
            pts.append(p)
        tips += tube(bm, head, pts, 0.016, flat=0.6, tip_from=0.7)
    return finish("Hair_Ponytail", bm, tips)


STYLES = {"Hair_Swept": swept, "Hair_Bob": bob, "Hair_Long": long_straight, "Hair_Crop": crop,
          "Hair_Buzz": buzz, "Hair_Ponytail": ponytail}


def build_all(P, normals, lm, face, joints, follow):
    head = Head(P, normals, lm, face, joints, follow)
    head.follow_mesh = bpy.data.objects["Body"].data
    return {name: make(head) for name, make in STYLES.items()}
