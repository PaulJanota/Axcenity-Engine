"""The outfits: garments made from the body's own surface.

Each garment starts as a copy of the body's faces where it is worn, cut
with planes at its hems (so the edges are clean), then pushed out and
draped: smoothed while never coming closer to the skin than its gap, so
cloth bridges hollows (between the shoulder blades, at the small of the
back) instead of clinging to them. Openings get a band of thickness;
folds, cuffs, pockets and soles are added after. Every garment follows
the body's frame and build shapes and its skin weights (`build.py`).
"""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

import materials
from common import SIDES, new_object, remesh, smoothstep, v

# Which bones' skin a garment is cut from.
TORSO = ("Hips", "Spine", "Chest", "UpperChest", "Neck", "LeftShoulder", "RightShoulder")
ARMS = ("LeftUpperArm", "RightUpperArm", "LeftLowerArm", "RightLowerArm")
LEGS = ("LeftUpperLeg", "RightUpperLeg", "LeftLowerLeg", "RightLowerLeg")
FEET = ("LeftFoot", "RightFoot", "LeftToes", "RightToes", "LeftLowerLeg", "RightLowerLeg")


class Garment:
    def __init__(self, name, obj, category, masks):
        self.name, self.obj, self.category, self.masks = name, obj, category, masks

    def info(self):
        return {"category": self.category, "masks": list(self.masks),
                "materials": [m.name for m in self.obj.data.materials]}


class Ctx:
    """What every garment needs: the body, its weights and joints."""

    def __init__(self, body_obj, P, normals, lm, joints, follow, weights, nbr):
        self.body, self.P, self.N, self.lm = body_obj, P, normals, lm
        self.J = {k: (h, t) for k, (h, t, _) in joints.items()}
        self.follow, self.weights, self.nbr = follow, weights, nbr
        self.bvh = follow.bvh

    def dominant(self, i):
        w = self.weights[i]
        return max(w, key=w.get) if w else None


# -- building blocks ---------------------------------------------------------------

def region(ctx, bones, extra=None):
    """A bmesh of the body faces whose vertices mostly belong to `bones`."""
    bm = bmesh.new()
    bm.from_mesh(ctx.body.data)
    bm.verts.ensure_lookup_table()
    keep = set(bones)
    drop = []
    for f in bm.faces:
        share = sum(sum(ctx.weights[vert.index].get(b, 0.0) for b in keep) for vert in f.verts) / len(f.verts)
        if share < 0.5 and not (extra and extra(f)):
            drop.append(f)
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    # The body's shape keys and materials don't come along.
    for layer in list(bm.verts.layers.shape):
        bm.verts.layers.shape.remove(layer)
    for layer in list(bm.verts.layers.deform):
        bm.verts.layers.deform.remove(layer)
    return bm


def cut(bm, point, normal):
    """Removes everything on the side `normal` points to."""
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    bmesh.ops.bisect_plane(bm, geom=geom, plane_co=point, plane_no=normal.normalized(), clear_outer=True)
    _drop_loose(bm)


def _dist_to_chain(p, chain):
    """The distance from `p` to the polyline through the points `chain`."""
    best = float("inf")
    for a, b in zip(chain, chain[1:]):
        d = b - a
        t = max(0.0, min(1.0, (p - a).dot(d) / d.length_squared))
        best = min(best, (p - (a + d * t)).length)
    return best


def cut_limb(bm, point, normal, chain, radius):
    """Like `cut`, but only on the limb along `chain` (joint positions):
    cuts and removes faces within `radius` of it, so a sleeve's hem can't
    also slice the torso the way an infinite plane does."""
    near = [f for f in bm.faces if _dist_to_chain(f.calc_center_median(), chain) < radius]
    edges = {e for f in near for e in f.edges}
    verts = {vert for f in near for vert in f.verts}
    bmesh.ops.bisect_plane(bm, geom=list(verts) + list(edges) + near, plane_co=point,
                           plane_no=normal.normalized())
    drop = [f for f in bm.faces
            if (f.calc_center_median() - point).dot(normal) > 0
            and _dist_to_chain(f.calc_center_median(), chain) < radius]
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    _drop_loose(bm)


def arm_chain(J, side):
    """The arm's joints, shoulder to the end of the hand."""
    sh, el = J[f"{side}UpperArm"]
    wr = J[f"{side}LowerArm"][1]
    hand_end = J[f"{side}Hand"][1]
    return [sh, el, wr, hand_end]


def _drop_loose(bm, keep_largest=0):
    loose = [vert for vert in bm.verts if not vert.link_faces]
    bmesh.ops.delete(bm, geom=loose, context="VERTS")


def keep_islands(bm, pick):
    """Keeps the islands for which `pick(centre)` is true."""
    bm.verts.ensure_lookup_table()
    seen, drop = set(), []
    for start in bm.verts:
        if start in seen:
            continue
        stack, isl = [start], []
        while stack:
            x = stack.pop()
            if x in seen:
                continue
            seen.add(x)
            isl.append(x)
            stack.extend(e.other_vert(x) for e in x.link_edges)
        c = sum((x.co for x in isl), Vector()) / len(isl)
        if not pick(c, len(isl)):
            drop += isl
    bmesh.ops.delete(bm, geom=drop, context="VERTS")


def drape(ctx, bm, gap, iterations=14, smooth=0.5):
    """Pushes the surface `gap(p)` out from the skin and smooths it, never
    closer to the skin than its gap."""
    bm.verts.ensure_lookup_table()
    bm.normal_update()
    for vert in bm.verts:
        loc, nrm, _, _ = ctx.bvh.find_nearest(vert.co)
        vert.co = loc + nrm * gap(vert.co)
    boundary = {vert for vert in bm.verts if vert.is_boundary}
    for _ in range(iterations):
        new = {}
        for vert in bm.verts:
            if vert in boundary:
                nb = [e.other_vert(vert) for e in vert.link_edges if e.is_boundary]
            else:
                nb = [e.other_vert(vert) for e in vert.link_edges]
            if not nb:
                continue
            avg = sum((n.co for n in nb), Vector()) / len(nb)
            new[vert] = vert.co.lerp(avg, smooth)
        for vert, co in new.items():
            loc, nrm, _, _ = ctx.bvh.find_nearest(co)
            g = gap(co)
            if (co - loc).dot(nrm) < g:
                co = loc + nrm * g
            vert.co = co
    bm.normal_update()


def displace(bm, f):
    """Moves each vertex by `f(p, normal)`."""
    bm.normal_update()
    moves = [(vert, f(vert.co.copy(), vert.normal.copy())) for vert in bm.verts]
    for vert, d in moves:
        if d is not None:
            vert.co += d


def rim(bm, thickness, inside=0.012):
    """Gives each opening a thickness: the edge rolls in by `thickness` and
    runs `inside` back along the inside, so a cuff or collar reads as cloth."""
    bm.normal_update()
    edges = [e for e in bm.edges if e.is_boundary]
    if not edges:
        return
    verts = {vert for e in edges for vert in e.verts}
    normals = {vert: vert.normal.copy() for vert in verts}
    # Direction back into the garment along its surface.
    back = {}
    for vert in verts:
        inner = [e.other_vert(vert) for e in vert.link_edges if not e.is_boundary]
        d = sum(((n.co - vert.co) for n in inner), Vector())
        back[vert] = d.normalized() if d.length > 1e-9 else -normals[vert]
    ret = bmesh.ops.extrude_edge_only(bm, edges=edges)
    new_verts = [g for g in ret["geom"] if isinstance(g, bmesh.types.BMVert)]
    # Map each new vertex to its source (same position).
    src = {}
    for nv in new_verts:
        best = min(verts, key=lambda s: (s.co - nv.co).length_squared)
        src[nv] = best
    for nv in new_verts:
        s = src[nv]
        nv.co = s.co - normals[s] * thickness
    edges2 = [e for e in bm.edges if e.is_boundary]
    ret2 = bmesh.ops.extrude_edge_only(bm, edges=edges2)
    for nv in (g for g in ret2["geom"] if isinstance(g, bmesh.types.BMVert)):
        s = src[min(new_verts, key=lambda q: (q.co - nv.co).length_squared)]
        nv.co = nv.co + back[s] * inside
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])


def band(bm, near, push):
    """Thickens a band: vertices for which `near(p)` is 1 move out by `push`."""
    displace(bm, lambda p, n: n * push * near(p))


def finish(name, bm, material_of, category, masks):
    """The garment object from `bm`, with a material per face."""
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = new_object(name, me)
    for poly in obj.data.polygons:
        poly.use_smooth = True
    materials.assign(obj, [material_of(poly.center) for poly in obj.data.polygons])
    return Garment(name, obj, category, masks)


def ring_folds(axis_point, axis_dir, wavelength, amp, mask):
    """Rings of folds round a limb (bunching), as a displacement function."""
    def f(p, n):
        m = mask(p)
        if m <= 0:
            return None
        s = (p - axis_point).dot(axis_dir)
        return n * amp * m * (0.5 + 0.5 * math.sin(2 * math.pi * s / wavelength))
    return f


def limb_t(p, head, tail):
    d = tail - head
    return (p - head).dot(d) / d.length_squared


# -- garments ------------------------------------------------------------------------

def tee(ctx):
    """A crew-neck T-shirt: short sleeves to mid upper arm, hem below the waist,
    worn untucked over the trousers."""
    J = ctx.J
    bm = region(ctx, TORSO + ARMS)
    waist = J["Spine"][0]
    # The hem 12 cm under the waist joint: 6.5 cm over the trousers' top,
    # so the flanks stay covered on wide hips without a jacket.
    hem_drop = 0.12
    cut(bm, waist + v(0, 0, -hem_drop), v(0, -0.08, -1))
    neck = J["Neck"][0]
    cut(bm, neck + v(0, -0.02, -0.005), v(0, -0.45, 1))
    for side, s in SIDES:
        sh, el = J[f"{side}UpperArm"]
        # Only the arm: an infinite plane along a 45 degree arm also cuts
        # the torso, which lifted the hem at the flanks and left the waist
        # bare (docs/development/character-studio/audit.md, defect 1).
        cut_limb(bm, sh.lerp(el, 0.50), el - sh, arm_chain(J, side), 0.10)
    keep_islands(bm, lambda c, n: n > 200)

    def gap(p):
        g = 0.0055
        g += 0.006 * smoothstep(waist.z + 0.05, waist.z - 0.08, p.z)  # looser at the hem
        return g

    drape(ctx, bm, gap)
    for side, s in SIDES:
        sh, el = J[f"{side}UpperArm"]
        axis = (el - sh).normalized()
        # Sleeves: a little flare and a fold under the arm.
        displace(bm, lambda p, n, sh=sh, el=el: n * 0.004 * smoothstep(0.30, 0.5, limb_t(p, sh, el))
                 if p.x * s > abs(sh.x) * 0.9 else None)
    hem_z = waist.z - hem_drop
    band(bm, lambda p: 1.0 - smoothstep(0.0, 0.015, abs(p.z - hem_z + 0.004)) if p.z < waist.z else 0.0, 0.0015)
    rim(bm, 0.003, inside=0.014)
    return finish("Shirt_Tee", bm, lambda c: "Shirt", "Top", ["Mask_Torso", "Mask_UpperArms_Top"])


def split(bm, point, normal):
    """Adds an edge loop where the plane crosses, removing nothing (for a
    clean colour band edge)."""
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    bmesh.ops.bisect_plane(bm, geom=geom, plane_co=point, plane_no=normal.normalized())


def side_of(c, point, normal):
    return (c - point).dot(normal) > 0


def jacket(ctx):
    """A cropped, open jacket: stand collar, long sleeves with ribbed cuffs,
    a ribbed hem at the waist, a light band round each upper arm."""
    J = ctx.J
    bm = region(ctx, TORSO + ARMS)
    waist = J["Spine"][0]
    hem_z = waist.z - 0.035
    cut(bm, v(0, 0, hem_z), v(0, -0.04, -1))
    neck = J["Neck"][0]
    neck_pt, neck_n = neck + v(0, -0.01, 0.025), v(0, -0.25, 1)
    cut(bm, neck_pt, neck_n)
    for side, s in SIDES:
        el, wr = J[f"{side}LowerArm"]
        cut(bm, el.lerp(wr, 0.86), wr - el)
    keep_islands(bm, lambda c, n: n > 200)
    # The open front: a plane each side, the gap widening toward the hem.
    for side, s in SIDES:
        top = v(s * 0.013, 0, neck.z)
        bottom = v(s * 0.045, 0, hem_z)
        along = (bottom - top).normalized()
        normal = along.cross(v(0, 1, 0)).normalized()
        if normal.x * s > 0:
            normal = -normal
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=top, plane_no=normal)
        drop = [f for f in bm.faces if f.calc_center_median().y < -0.01
                and f.calc_center_median().x * s >= -0.002
                and side_of(f.calc_center_median(), top, normal)]
        bmesh.ops.delete(bm, geom=drop, context="FACES")
        _drop_loose(bm)

    def gap(p):
        g = 0.015
        for side, s in SIDES:
            sh, el = J[f"{side}UpperArm"]
            wr = J[f"{side}LowerArm"][1]
            if p.x * s > abs(sh.x) * 0.85:
                t = limb_t(p, el, wr)
                g = 0.012 + 0.005 * smoothstep(0.1, 0.7, t) - 0.006 * smoothstep(0.76, 0.95, t)
        return g

    drape(ctx, bm, gap, iterations=24)
    for side, s in SIDES:
        el, wr = J[f"{side}LowerArm"]
        axis = (wr - el).normalized()
        displace(bm, ring_folds(el, axis, 0.045, 0.004,
                                lambda p, el=el, wr=wr: smoothstep(-0.15, 0.05, limb_t(p, el, wr))
                                * (1 - smoothstep(0.1, 0.25, limb_t(p, el, wr))) if p.x * s > 0 else 0.0))
        displace(bm, ring_folds(el, axis, 0.03, 0.003,
                                lambda p, el=el, wr=wr: smoothstep(0.5, 0.65, limb_t(p, el, wr))
                                * (1 - smoothstep(0.72, 0.76, limb_t(p, el, wr))) if p.x * s > 0 else 0.0))
    # Band edges as edge loops.
    planes = {}
    for side, s in SIDES:
        el, wr = J[f"{side}LowerArm"]
        cuff = (el.lerp(wr, 0.765), (wr - el).normalized())
        sh, elb = J[f"{side}UpperArm"]
        a = (sh.lerp(elb, 0.40), (elb - sh).normalized())
        b = (sh.lerp(elb, 0.50), (elb - sh).normalized())
        planes[s] = (cuff, a, b)
        for pt, n in (cuff, a, b):
            split(bm, pt, n)
    hem_band = (v(0, 0, hem_z + 0.032), v(0, -0.12, 1))
    collar = (neck_pt - neck_n.normalized() * 0.03, neck_n.normalized())
    split(bm, *hem_band)
    split(bm, *collar)
    rim(bm, 0.004, inside=0.016)

    def mat(c):
        for s, (cuff, a, b) in planes.items():
            sh = J["LeftUpperArm" if s > 0 else "RightUpperArm"][0]
            if c.x * s > 0.05:
                if side_of(c, *cuff):
                    return "JacketRib"
                if side_of(c, *a) and not side_of(c, *b) and c.x * s > abs(sh.x) + 0.035:
                    return "JacketTrim"
        if not side_of(c, *hem_band):
            return "JacketRib"
        if side_of(c, *collar) and abs(c.x) < 0.09:
            return "JacketRib"
        return "Jacket"

    band(bm, lambda p: 1.0 - smoothstep(hem_z + 0.025, hem_z + 0.035, p.z), 0.0025)
    return finish("Jacket_Cropped", bm, mat, "Outerwear",
                  ["Mask_Torso", "Mask_UpperArms_Top", "Mask_UpperArms_Low", "Mask_Forearms"])


def cargo_pants(ctx):
    """Cargo joggers: a waistband, roomy thighs with side pockets, tapering
    to gathered cuffs above the ankles."""
    J = ctx.J
    bm = region(ctx, ("Hips", "Spine") + LEGS)
    waist = J["Spine"][0]
    top = waist.z - 0.055
    cut(bm, v(0, 0, top), v(0, -0.12, 1))
    for side, s in SIDES:
        knee, ankle = J[f"{side}LowerLeg"]
        bmesh_cut_side(bm, s, v(0, 0, ankle.z + 0.045), v(0, 0, -1))
    keep_islands(bm, lambda c, n: n > 200)
    crotch = v(0, J["Hips"][0].y, J["LeftUpperLeg"][0].z - 0.07)

    def gap(p):
        g = 0.010
        for side, s in SIDES:
            hip, knee = J[f"{side}UpperLeg"]
            ankle = J[f"{side}LowerLeg"][1]
            if p.x * s > 0:
                if p.z > knee.z:
                    t = limb_t(p, hip, knee)
                    g = 0.012 + 0.016 * smoothstep(0.0, 0.7, t)
                else:
                    t = limb_t(p, knee, ankle)
                    g = 0.028 - 0.006 * smoothstep(0.2, 0.7, t) - 0.016 * smoothstep(0.72, 0.93, t)
        # Cloth hangs across the crotch rather than following it.
        g += 0.022 * math.exp(-((p - crotch).length / 0.05) ** 2)
        return max(0.006, g)

    drape(ctx, bm, gap, iterations=40, smooth=0.55)
    for side, s in SIDES:
        knee, ankle = J[f"{side}LowerLeg"]
        axis = (ankle - knee).normalized()
        displace(bm, ring_folds(knee, axis, 0.035, 0.005,
                                lambda p, knee=knee, ankle=ankle: smoothstep(0.55, 0.75, limb_t(p, knee, ankle))
                                * (1 - smoothstep(0.84, 0.9, limb_t(p, knee, ankle))) if p.x * s > 0 else 0.0))
        hip = J[f"{side}UpperLeg"][0]
        displace(bm, ring_folds(hip, (knee - hip).normalized(), 0.05, 0.004,
                                lambda p, hip=hip, knee=knee: smoothstep(0.8, 0.95, limb_t(p, hip, knee))
                                * (1 - smoothstep(1.02, 1.15, limb_t(p, hip, knee))) if p.x * s > 0 else 0.0))
        mid = hip.lerp(knee, 0.55)

        def pocket(p, n, mid=mid, s=s):
            if p.x * s < abs(mid.x):
                return None
            dz, dy = (p.z - mid.z) / 0.08, (p.y - mid.y) / 0.06
            box = (1 - smoothstep(0.75, 1.0, abs(dz))) * (1 - smoothstep(0.75, 1.0, abs(dy)))
            flap = smoothstep(0.5, 0.58, dz) * (1 - smoothstep(0.92, 1.0, dz)) * (1 - smoothstep(0.8, 1.0, abs(dy)))
            lateral = smoothstep(0.3, 0.7, n.x * s)
            d = (0.011 * box + 0.003 * flap) * lateral
            return n * d if d > 0 else None

        displace(bm, pocket)
    waist_band = (v(0, 0, top - 0.035), v(0, -0.1, 1))
    split(bm, *waist_band)
    for side, s in SIDES:
        knee, ankle = J[f"{side}LowerLeg"]
        split(bm, v(0, 0, ankle.z + 0.075), v(0, 0, -1))
    band(bm, lambda p: smoothstep(top - 0.038, top - 0.03, p.z), 0.003)
    rim(bm, 0.0035, inside=0.016)
    return finish("Pants_Cargo", bm, lambda c: "Trousers", "Bottom",
                  ["Mask_Hips", "Mask_Thighs_Top", "Mask_Thighs_Low", "Mask_Shins_Top", "Mask_Shins_Low"])


def bmesh_cut_side(bm, s, point, normal):
    """Like `cut`, but only on one side (x * s > 0): bisects the whole mesh
    and removes the far geometry on that side only."""
    geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
    bmesh.ops.bisect_plane(bm, geom=geom, plane_co=point, plane_no=normal)
    drop = [f for f in bm.faces if f.calc_center_median().x * s > 0
            and (f.calc_center_median() - point).dot(normal) > 0]
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    _drop_loose(bm)


def sneakers(ctx):
    """Low-top sneakers: a smooth upper over the foot (a toe box, no toes),
    a thick rubber sole, a coloured toe cap and heel tab, laces."""
    J = ctx.J
    bm = region(ctx, ("LeftFoot", "RightFoot", "LeftToes", "RightToes", "LeftLowerLeg", "RightLowerLeg"))
    for side, s in SIDES:
        ankle = J[f"{side}Foot"][0]
        bmesh_cut_side(bm, s, ankle + v(0, 0, 0.012), v(0, 0.35, 1))
    keep_islands(bm, lambda c, n: n > 80)
    sole_top = 0.026

    def gap(p):
        toes = J["LeftToes" if p.x > 0 else "RightToes"][0]
        return 0.008 + 0.016 * smoothstep(toes.y + 0.05, toes.y - 0.03, p.y) + 0.003 * smoothstep(0.05, 0.0, p.z)

    drape(ctx, bm, gap, iterations=140, smooth=0.75)
    # Lift the upper's bottom onto the sole, fading to 40% of that by the
    # ankle: lifting it all (as before) left the toe box 5 cm over the toes;
    # no lift at the collar put its thin rim on the ankle bone, where the
    # feminine frame pulled it into spikes.
    lift_top = 0.09
    for vert in bm.verts:
        z = max(0.0, vert.co.z)
        vert.co.z = z + sole_top * (1.0 - 0.6 * smoothstep(0.0, lift_top, z))
    toe_caps, heels = {}, {}
    for side, s in SIDES:
        ankle = J[f"{side}Foot"][0]
        tip = J[f"{side}Toes"][1]
        toe_caps[s] = (v(0, tip.y + 0.034, 0), v(0, -1, 0))
        heels[s] = (v(0, ankle.y + 0.03, 0), v(0, 1, 0))
        split(bm, *toe_caps[s])
        split(bm, *heels[s])
    rim(bm, 0.004, inside=0.012)

    def mat(c):
        s = 1 if c.x > 0 else -1
        if side_of(c, *toe_caps[s]) and c.z < sole_top + 0.03:
            return "ShoeAccent"
        if side_of(c, *heels[s]) and c.z > sole_top + 0.03:
            return "ShoeAccent"
        return "Shoes"

    garment = finish("Shoes_Sneakers", bm, mat, "Shoes", ["Mask_Feet"])
    obj = garment.obj
    # Give the upper a wall and relax it into one smooth piece, then add
    # the soles and laces.
    remesh(obj, 0.003, smooth_iterations=8, smooth_factor=0.5, max_triangles=2800, thicken=0.004)
    bm2 = bmesh.new()
    bm2.from_mesh(obj.data)
    accent_line = (v(0, 0, sole_top + 0.03), v(0, 0, -1))
    split(bm2, *accent_line)
    for s in (1, -1):
        split(bm2, *toe_caps[s])
        split(bm2, *heels[s])
    names = []
    for f in bm2.faces:
        c = f.calc_center_median()
        s = 1 if c.x > 0 else -1
        low = side_of(c, *accent_line)
        if (side_of(c, *toe_caps[s]) and low) or (side_of(c, *heels[s]) and not low):
            names.append("ShoeAccent")
        else:
            names.append("Shoes")
    bm2.to_mesh(obj.data)
    bm2.free()
    materials.assign(obj, names)
    for poly in obj.data.polygons:
        poly.use_smooth = True
    # The sole must enclose the foot's own footprint too: the upper's
    # outline alone left the toes poking out at the front.
    footprint = {s: [p for i, p in enumerate(ctx.P)
                     if p.z < sole_top + 0.01 and p.x * s > 0
                     and ctx.dominant(i) in (f"{side}Foot", f"{side}Toes")]
                 for side, s in SIDES}
    _join(obj, [sole(obj, s, sole_top, footprint[s]) for side, s in SIDES])
    _join(obj, [laces(J, s, obj) for side, s in SIDES])
    return garment


def _join(obj, pieces):
    bpy.ops.object.select_all(action="DESELECT")
    for p in pieces:
        p.select_set(True)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.join()


def sole(upper, s, top, footprint=()):
    """A rubber sole under one shoe: the outline of the upper's bottom, and
    of the foot's `footprint` (body points under the sole's top) with a
    4 mm margin, grown a little, lofted down to the ground with a rounded
    edge."""
    pts = [vert.co for vert in upper.data.vertices if vert.co.x * s > 0 and vert.co.z < top + 0.012]
    c = sum(pts, Vector()) / len(pts)
    # The outline is the convex hull of the upper's bottom and the foot's
    # footprint (grown by 4 mm), read along each direction from the centre:
    # picking one "far" point per direction underestimated the toes' reach
    # at the front corners, so they poked out under the upper.
    margin = 0.004
    hull = _hull_2d([(p.x, p.y) for p in pts] + [(p.x, p.y) for p in footprint])
    n_ang = 40
    ring = []
    for k in range(n_ang):
        a = 2 * math.pi * k / n_ang
        d = Vector((math.cos(a), math.sin(a), 0))
        r = _ray_to_hull((c.x, c.y), (d.x, d.y), hull) + margin
        ring.append((d, max(0.02, r)))
    verts, faces = [], []
    layers = [(top + 0.004, -0.001), (top - 0.002, 0.0025), (0.012, 0.003), (0.002, 0.0015), (0.0, -0.002)]
    for z, grow in layers:
        for d, r in ring:
            p = c + d * (r + grow)
            verts.append(Vector((p.x, p.y, z)))
    for L in range(len(layers) - 1):
        for k in range(n_ang):
            a, b = L * n_ang + k, L * n_ang + (k + 1) % n_ang
            faces.append([a, b, b + n_ang, a + n_ang] if s < 0 else [a, a + n_ang, b + n_ang, b])
    bottom_c = len(verts)
    verts.append(Vector((c.x, c.y, 0.0)))
    last = (len(layers) - 1) * n_ang
    for k in range(n_ang):
        a, b = last + k, last + (k + 1) % n_ang
        faces.append([a, bottom_c, b] if s < 0 else [b, bottom_c, a])
    me = bpy.data.meshes.new("Sole")
    me.from_pydata([tuple(p) for p in verts], [], faces)
    me.update()
    obj = new_object("Sole", me)
    for poly in obj.data.polygons:
        poly.use_smooth = True
    obj.data.materials.append(materials.material("Sole"))
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    return obj


def _hull_2d(points):
    """The convex hull of 2D points, counter-clockwise (monotone chain)."""
    pts = sorted(set(points))
    if len(pts) < 3:
        return pts

    def turn(o, a, b):
        return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])

    lower, upper = [], []
    for p in pts:
        while len(lower) >= 2 and turn(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    for p in reversed(pts):
        while len(upper) >= 2 and turn(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    return lower[:-1] + upper[:-1]


def _ray_to_hull(o, d, hull):
    """How far a ray from `o` (inside the hull) along `d` goes to its edge."""
    best = 0.0
    for a, b in zip(hull, hull[1:] + hull[:1]):
        ex, ey = b[0] - a[0], b[1] - a[1]
        den = d[0] * ey - d[1] * ex
        if abs(den) < 1e-12:
            continue
        ax, ay = a[0] - o[0], a[1] - o[1]
        t = (ax * ey - ay * ex) / den
        u = (ax * d[1] - ay * d[0]) / den
        if t > 0.0 and -1e-9 <= u <= 1.0 + 1e-9:
            best = max(best, t)
    return best


def laces(J, s, upper):
    """Five lace bars across the instep, lying on the upper."""
    side = "Left" if s > 0 else "Right"
    ankle = J[f"{side}Foot"][0]
    ball = J[f"{side}Toes"][0]
    bvh = BVHTree.FromObject(upper, bpy.context.evaluated_depsgraph_get())
    verts, faces = [], []
    for k in range(5):
        t = 0.18 + 0.14 * k
        q = ankle.lerp(ball, t) + Vector((0, 0, 0.08))
        loc, nrm, _, _ = bvh.ray_cast(q, Vector((0, 0, -1)))
        if loc is None:
            continue
        across = Vector((1, 0, 0))
        along = nrm.cross(across).normalized()
        w, h, th = 0.016 - 0.0015 * k, 0.0035, 0.0022
        base = len(verts)
        for dx in (-w, w):
            for dy in (-h, h):
                for dz in (0.0, th):
                    verts.append(loc + across * dx + along * dy + nrm * (dz + 0.0008))
        # A small box.
        quads = [(0, 2, 6, 4), (1, 5, 7, 3), (0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6)]
        faces += [[base + i for i in q] for q in quads]
    me = bpy.data.meshes.new("Laces")
    me.from_pydata([tuple(p) for p in verts], [], faces)
    me.update()
    obj = new_object("Laces", me)
    obj.data.materials.append(materials.material("Laces"))
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces[:])
    bm.to_mesh(me)
    bm.free()
    return obj


def briefs(ctx):
    """Plain underwear briefs, close to the skin."""
    J = ctx.J
    bm = region(ctx, ("Hips", "Spine", "LeftUpperLeg", "RightUpperLeg"))
    waist = J["Spine"][0]
    cut(bm, v(0, 0, waist.z - 0.075), v(0, 0.12, 1))
    for side, s in SIDES:
        hip, knee = J[f"{side}UpperLeg"]
        bmesh_cut_side(bm, s, hip.lerp(knee, 0.18), (knee - hip) + v(0, 0, -0.25))
    keep_islands(bm, lambda c, n: n > 100)
    drape(ctx, bm, lambda p: 0.0018, iterations=4, smooth=0.3)
    rim(bm, 0.0015, inside=0.006)
    return finish("Underwear_Briefs", bm, lambda c: "Underwear", "Underwear", [])


def sports_top(ctx):
    """A plain sports top, for the feminine frame without a shirt."""
    J = ctx.J
    bm = region(ctx, TORSO)
    chest = J["Chest"][1]
    cut(bm, v(0, 0, chest.z - 0.11), v(0, 0, -1))
    neck = J["Neck"][0]
    cut(bm, neck + v(0, -0.06, -0.06), v(0, -0.6, 1))
    for side, s in SIDES:
        sh = J[f"{side}UpperArm"][0]
        bmesh_cut_side(bm, s, sh + v(-s * 0.035, 0, 0), v(s, 0, 0.35))
    keep_islands(bm, lambda c, n: n > 100)
    drape(ctx, bm, lambda p: 0.0022, iterations=10, smooth=0.5)
    rim(bm, 0.0018, inside=0.006)
    return finish("Underwear_Top", bm, lambda c: "Underwear", "Underwear", [])


def build_all(body_obj, P, normals, lm, joints, follow, weights, nbr):
    ctx = Ctx(body_obj, P, normals, lm, joints, follow, weights, nbr)
    out = {}
    for make in (tee, jacket, cargo_pants, sneakers, briefs, sports_top):
        g = make(ctx)
        out[g.name] = g
    return out
