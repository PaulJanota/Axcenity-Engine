"""The face: eye materials, brows, lashes, teeth, jaw weights, the 41
expression shapes of the Axcenity Humanoid Facial Profile and the face
identity shapes Character Studio's sliders lean on.

Every shape is a displacement of the body's head vertices, worked out from
landmarks (the eyelid margins, the mouth corners) on the masculine frame.
Lids turn about the eyeball's centre, so they slide over it. Geometry
added to the head (brows, lashes) follows the skin under it
(`follow`), so it moves with every shape.
"""

import math

import bmesh
import bpy
from mathutils import Vector
from mathutils.bvhtree import BVHTree

from common import (
    FS_EAR, FS_FACE, FS_JAW_SIDE, FS_LID_INNER, FS_LID_OUTER, FS_LOWER_LIP, FS_NECK, FS_NOSE,
    FS_SOCKET, FS_UNDER_JAW, FS_UPPER_LIP, SIDES, centroid, mesh_from, new_object, smoothstep, v,
)

# The expression shapes, in the order the head carries them (the facial
# profile's control names; tongue controls are optional and absent).
EXPRESSIONS = [
    "Blink_Left", "Blink_Right", "Eye_Closed_Left", "Eye_Closed_Right",
    "Eye_Squint_Left", "Eye_Squint_Right", "Eye_Wide_Left", "Eye_Wide_Right",
    "Brow_Raise_Left", "Brow_Raise_Right", "Brow_Lower_Left", "Brow_Lower_Right",
    "Brow_Inward_Left", "Brow_Inward_Right",
    "Mouth_Smile_Left", "Mouth_Smile_Right", "Mouth_Frown_Left", "Mouth_Frown_Right",
    "Mouth_Open", "Mouth_Wide", "Mouth_Narrow", "Mouth_Pucker", "Mouth_Left", "Mouth_Right",
    "Upper_Lip_Raise", "Upper_Lip_Raise_Left", "Upper_Lip_Raise_Right",
    "Lower_Lip_Lower", "Lower_Lip_Lower_Left", "Lower_Lip_Lower_Right",
    "Lip_Corner_Pull_Left", "Lip_Corner_Pull_Right",
    "Cheek_Raise_Left", "Cheek_Raise_Right", "Cheek_Puff",
    "Nose_Sneer_Left", "Nose_Sneer_Right",
    "Jaw_Open", "Jaw_Forward", "Jaw_Left", "Jaw_Right",
]

HEAD_SETS = (FS_LID_OUTER, FS_LID_INNER, FS_EAR, FS_SOCKET, FS_FACE, FS_NOSE, FS_JAW_SIDE,
             FS_UPPER_LIP, FS_LOWER_LIP, FS_UNDER_JAW)


def gauss(d2, r):
    return math.exp(-d2 / (2.0 * r * r))


def ellipse(p, c, rx, ry, rz):
    """1 at `c`, smoothly 0 outside the ellipsoid of radii rx, ry, rz."""
    d = ((p.x - c.x) / rx) ** 2 + ((p.y - c.y) / ry) ** 2 + ((p.z - c.z) / rz) ** 2
    return 1.0 - smoothstep(0.0, 1.0, math.sqrt(d))


class Piecewise:
    """A function of one variable through sorted (x, y) points."""

    def __init__(self, pts):
        self.pts = sorted(pts)

    def __call__(self, x):
        pts = self.pts
        if x <= pts[0][0]:
            return pts[0][1]
        if x >= pts[-1][0]:
            return pts[-1][1]
        for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
            if x0 <= x <= x1:
                t = (x - x0) / (x1 - x0) if x1 > x0 else 0.0
                return y0 + (y1 - y0) * t
        return pts[-1][1]


class Eye:
    """One eye's landmarks: the eyeball centre and radius, and its lid
    margin as upper and lower elevation curves over the lateral offset."""

    def __init__(self, s, centre, radius, P, lm):
        self.s, self.c, self.r = s, centre, radius
        ring = lm.ring(FS_LID_OUTER, FS_LID_INNER, s)
        uts = [self.ut(P[i]) for i in ring]
        self.u_in = min(u for u, _ in uts)
        self.u_out = max(u for u, _ in uts)
        th_in = min(uts, key=lambda a: a[0])[1]
        th_out = max(uts, key=lambda a: a[0])[1]
        mid = lambda u: th_in + (th_out - th_in) * (u - self.u_in) / (self.u_out - self.u_in)
        up = [(u, t) for u, t in uts if t >= mid(u)]
        lo = [(u, t) for u, t in uts if t < mid(u)]
        up += [(self.u_in, th_in), (self.u_out, th_out)]
        lo += [(self.u_in, th_in), (self.u_out, th_out)]
        self.up = Piecewise(up)
        self.lo = Piecewise(lo)
        self.margin = ring
        self.outer = lm.ring(FS_FACE, FS_LID_OUTER, s)
        self.upper_margin = [i for i, (u, t) in zip(ring, uts) if t >= mid(u)]
        top = max((P[i] for i in ring), key=lambda p: p.z)
        self.top_z = top.z

    def ut(self, p):
        d = p - self.c
        return (d.x * self.s, math.atan2(d.z, -d.y))

    def taper(self, u):
        span = self.u_out - self.u_in
        return smoothstep(self.u_in, self.u_in + 0.2 * span, u) * (
            1.0 - smoothstep(self.u_out - 0.2 * span, self.u_out, u))

    def turn(self, p, angle):
        """`p` turned about the eye's horizontal axis, raising it by `angle`."""
        d = p - self.c
        r = math.hypot(d.y, d.z)
        th = math.atan2(d.z, -d.y) + angle
        return Vector((p.x, self.c.y - r * math.cos(th), self.c.z + r * math.sin(th)))


class FaceRig:
    """The head's landmarks and region weights on positions `P`."""

    def __init__(self, P, lm, eye_centres, eye_radius, normals=None):
        self.P, self.lm, self.N = P, lm, normals
        n = len(P)
        self.head = [False] * n
        for s in HEAD_SETS:
            for i in lm.verts(s):
                self.head[i] = True
        self.eyes = {s: Eye(s, eye_centres[s], eye_radius, P, lm) for _, s in SIDES}
        self.lid_outer = set(lm.verts(FS_LID_OUTER)) - set(lm.ring(FS_LID_OUTER, FS_LID_INNER))
        self.lid_inner = (set(lm.verts(FS_LID_INNER)) | set(lm.verts(FS_SOCKET))
                          | set(lm.ring(FS_LID_OUTER, FS_LID_INNER)))
        self.lid_set = self.lid_outer | self.lid_inner
        # The mouth: its corners where the lips meet at the front.
        self.corner = {}
        for _, s in SIDES:
            ring = lm.ring(FS_UPPER_LIP, FS_LOWER_LIP, s)
            self.corner[s] = min((P[i] for i in ring), key=lambda p: p.y)
        k = self.corner[1]
        self.mouth = v(0, k.y - 0.004, k.z)
        self.mouth_w = k.x
        self.upper_lip = set(lm.verts(FS_UPPER_LIP))
        self.lower_lip = set(lm.verts(FS_LOWER_LIP)) | set(lm.verts(FS_JAW_SIDE))
        self.lower_inside = set(lm.verts(FS_LOWER_LIP))
        self.under_jaw = set(lm.verts(FS_UNDER_JAW))

    # -- region weights ----------------------------------------------------

    def lid_weight(self, i, eye):
        """How much vertex `i` turns with eye's lids: 1 at the lid margin
        and behind it, falling to about an eighth at the lid's crease (the
        lid unfolds there) and to nothing a centimetre beyond."""
        p = self.P[i]
        if not self.head[i] or p.x * eye.s < -0.002:
            return 0.0
        u, _ = eye.ut(p)
        crease = 0.12
        if i in self.lid_inner:
            return eye.taper(u)
        d_margin = min((p - self.P[m]).length for m in eye.margin)
        d_outer = min((p - self.P[m]).length for m in eye.outer)
        if i in self.lid_outer:
            f = d_outer / max(1e-6, d_margin + d_outer)
            return eye.taper(u) * (crease + (1.0 - crease) * f ** 1.4)
        return 0.0

    def mouth_mask(self, p, rx=0.07, rz=0.055):
        return ellipse(p, self.mouth, rx, 0.06, rz) if p.y < self.mouth.y + 0.07 else 0.0

    def corner_weight(self, p, s, r=0.017):
        return gauss((p - self.corner[s]).length_squared, r)

    def side(self, p, s):
        return smoothstep(-0.008, 0.008, p.x * s)

    def upper_lip_band(self, i):
        p = self.P[i]
        if not self.head[i] or p.z < self.mouth.z - 0.001:
            return 0.0
        if i in self.lower_inside:
            return 0.0
        return ellipse(p, self.mouth + v(0, 0, 0.006), self.mouth_w * 1.25, 0.05, 0.016)

    def lower_lip_band(self, i):
        p = self.P[i]
        if not self.head[i] or i in self.upper_lip:
            return 0.0
        if p.z > self.mouth.z + 0.001 and i not in self.lower_inside:
            return 0.0
        return ellipse(p, self.mouth - v(0, 0, 0.008), self.mouth_w * 1.25, 0.05, 0.02)

    def jaw_weights(self, nbr):
        """How much each vertex follows the Jaw bone: the lower lip, chin,
        mouth floor and under the jaw, smoothed so the cheeks stretch."""
        P = self.P
        w = [0.0] * len(P)
        keep = [False] * len(P)
        hinge_x = 0.085
        for i, p in enumerate(P):
            if not self.head[i]:
                keep[i] = True
                continue
            if i in self.lower_inside:
                w[i], keep[i] = 1.0, True
            elif i in self.upper_lip:
                w[i], keep[i] = 0.0, True
            elif i in self.lower_lip or i in self.under_jaw:
                below = 1.0 - smoothstep(self.mouth.z - 0.006, self.mouth.z + 0.002, p.z)
                w[i] = below * (1.0 - smoothstep(0.045, hinge_x, abs(p.x)))
                if i in self.under_jaw:
                    w[i] *= 1.0 - smoothstep(-0.07, -0.01, p.y)
        from common import smooth_field
        w = smooth_field(w, nbr, 16, keep)
        for i in self.lm.ring(FS_UPPER_LIP, FS_LOWER_LIP):
            w[i] = 0.5
        return w

    # -- shapes --------------------------------------------------------------

    def lids(self, s, up_frac, lo_frac, fold=1.0):
        """Deltas turning eye `s`'s lids: `up_frac` of the opening for the
        upper lid (negative closes), `lo_frac` for the lower lid."""
        eye = self.eyes[s]
        out = {}
        for i, p in enumerate(self.P):
            w = self.lid_weight(i, eye)
            if w <= 0.0:
                continue
            u, th = eye.ut(p)
            uu = max(eye.u_in, min(eye.u_out, u))
            t_up, t_lo = eye.up(uu), eye.lo(uu)
            opening = t_up - t_lo
            d = p - eye.c
            upper = (th > (t_up + t_lo) * 0.5) if d.y < 0 else (d.z > 0)
            angle = (up_frac if upper else lo_frac) * opening * w * (fold if upper else 1.0)
            if angle:
                out[i] = eye.turn(p, angle) - p
        return out

    def brow(self, s, raise_=0.0, lower=0.0, inward=0.0):
        eye = self.eyes[s]
        out = {}
        span = eye.u_out - eye.u_in
        for i, p in enumerate(self.P):
            if not self.head[i] or p.x * s < -0.004:
                continue
            u, _ = eye.ut(p)
            band = gauss((p.z - (eye.top_z + 0.016)) ** 2, 0.014)
            lat = 1.0 - smoothstep(0.0, 0.03, max(0.0, max(eye.u_in - 0.012 - u, u - eye.u_out - 0.01)))
            if p.y > eye.c.y + 0.02:
                continue
            w = band * lat
            if w < 1e-3:
                continue
            inner = 1.0 - smoothstep(eye.u_in, eye.u_in + 0.6 * span, u)
            d = v(0, 0, 0.0075 * raise_ - 0.0055 * lower)
            d += v(-s * 0.0035 * inner * inward - s * 0.0015 * lower * inner, -0.0008 * lower, -0.0025 * inward * inner)
            out[i] = d * w
        return out

    def shapes(self, nbr):
        """Expression name -> {vertex: delta}."""
        P = self.P
        S = {}
        for side, s in SIDES:
            S[f"Blink_{side}"] = self.lids(s, -0.80, 0.24)
            S[f"Eye_Closed_{side}"] = self.lids(s, -0.76, 0.27, fold=0.95)
            S[f"Eye_Squint_{side}"] = add(self.lids(s, -0.12, 0.30), self.cheek(s, 0.6))
            S[f"Eye_Wide_{side}"] = self.lids(s, 0.22, -0.07)
            S[f"Brow_Raise_{side}"] = add(self.brow(s, raise_=1.0), self.lids(s, 0.08, 0.0))
            S[f"Brow_Lower_{side}"] = add(self.brow(s, lower=1.0), self.lids(s, -0.08, 0.0))
            S[f"Brow_Inward_{side}"] = self.brow(s, inward=1.0)
            S[f"Cheek_Raise_{side}"] = add(self.cheek(s, 1.0), self.lids(s, 0.0, 0.12))
        jaw = self.jaw_w
        for side, s in SIDES:
            S[f"Mouth_Smile_{side}"] = self.field(lambda i, p, s=s: self.corner_weight(p, s, 0.019) * v(s * 0.006, 0.004, 0.0105)
                                                  + self.cheek_vec(p, s) * 1.2)
            S[f"Mouth_Frown_{side}"] = self.field(lambda i, p, s=s: self.corner_weight(p, s) * v(s * 0.002, -0.0005, -0.007))
            S[f"Lip_Corner_Pull_{side}"] = self.field(lambda i, p, s=s: self.corner_weight(p, s, 0.02) * v(s * 0.0075, 0.004, 0.003))
            S[f"Nose_Sneer_{side}"] = self.field(lambda i, p, s=s: gauss((p - v(s * 0.019, self.corner[s].y - 0.016, self.mouth.z + 0.043)).length_squared, 0.011) * v(0, 0.0005, 0.0035) * self.side(p, s))
            S[f"Upper_Lip_Raise_{side}"] = self.field(lambda i, p, s=s: self.upper_lip_band(i) * self.side(p, s) * v(0, -0.0008, 0.0045))
            S[f"Lower_Lip_Lower_{side}"] = self.field(lambda i, p, s=s: self.lower_lip_band(i) * self.side(p, s) * v(0, -0.0008, -0.005))
        S["Upper_Lip_Raise"] = add(S["Upper_Lip_Raise_Left"], S["Upper_Lip_Raise_Right"], half_centre=True)
        S["Lower_Lip_Lower"] = add(S["Lower_Lip_Lower_Left"], S["Lower_Lip_Lower_Right"], half_centre=True)
        S["Mouth_Open"] = self.field(lambda i, p: self.lower_lip_band(i) * v(0, 0, -0.007) + self.upper_lip_band(i) * v(0, 0, 0.0018))
        S["Mouth_Wide"] = self.field(lambda i, p: sum((self.corner_weight(p, s, 0.02) * v(s * 0.0075, 0.002, 0) for s in (1, -1)), Vector()))
        S["Mouth_Narrow"] = self.field(lambda i, p: sum((self.corner_weight(p, s, 0.02) * v(-s * 0.008, -0.003, 0) for s in (1, -1)), Vector()))

        def pucker(i, p):
            lips = ellipse(p, self.mouth, self.mouth_w * 1.4, 0.04, 0.025)
            return lips * v(-0.38 * p.x, -0.012, 0) + sum((self.corner_weight(p, s) * v(-s * 0.009, -0.003, 0) for s in (1, -1)), Vector())

        S["Mouth_Pucker"] = self.field(pucker)
        S["Mouth_Left"] = self.field(lambda i, p: self.mouth_mask(p) * v(0.008, 0.0012, 0))
        S["Mouth_Right"] = self.field(lambda i, p: self.mouth_mask(p) * v(-0.008, 0.0012, 0))
        S["Cheek_Puff"] = self.field(lambda i, p: sum(gauss((p - v(s * 0.05, self.mouth.y + 0.035, self.mouth.z + 0.008)).length_squared, 0.02) for s in (1, -1)) * self.N[i] * 0.008 * (0.0 if self.lower_inside.__contains__(i) or i in self.upper_lip else 1.0))
        # Jaw_Open is the Jaw bone's turn; this corrective keeps the corners
        # from tearing and rounds the lips.
        S["Jaw_Open"] = self.field(lambda i, p: sum((self.corner_weight(p, s) * v(-s * 0.003, 0.0, -0.0015) for s in (1, -1)), Vector()) + self.upper_lip_band(i) * v(0, -0.001, 0.0012))
        S["Jaw_Forward"] = self.field(lambda i, p: jaw[i] * v(0, -0.007, 0))
        S["Jaw_Left"] = self.field(lambda i, p: jaw[i] * v(0.007, 0, 0))
        S["Jaw_Right"] = self.field(lambda i, p: jaw[i] * v(-0.007, 0, 0))
        return {name: S[name] for name in EXPRESSIONS}

    def cheek_vec(self, p, s):
        c = self.corner[s] + v(s * 0.014, 0.006, 0.026)
        return gauss((p - c).length_squared, 0.016) * v(s * 0.001, -0.0015, 0.0035)

    def cheek(self, s, amount):
        return self.field(lambda i, p: self.cheek_vec(p, s) * amount * (0 if i in self.lid_set else 1))

    def field(self, f):
        out = {}
        for i, p in enumerate(self.P):
            if not self.head[i]:
                continue
            d = f(i, p)
            if d.length_squared > 1e-12:
                out[i] = Vector(d)
        return out

    # -- identity (Character Studio's face sliders) --------------------------

    def identity(self):
        """Face identity shape name -> ({vertex: delta}, {bone: change}).
        A bone change is {"offset": [x, y, z]} (metres, Blender axes) and/or
        {"scale": s} at weight 1."""
        P = self.P
        eyes = self.eyes
        out = {}

        def eye_region(i, p, s):
            e = eyes[s]
            if p.x * s < -0.003 or not self.head[i]:
                return 0.0
            if i in self.lid_set:
                return 1.0
            d = (p - e.c).length
            return 1.0 - smoothstep(e.r * 1.05, e.r * 1.05 + 0.028, d)

        out["Face_Eyes_Spacing"] = (self.field(lambda i, p: sum((eye_region(i, p, s) * v(s * 0.005, 0, 0) for s in (1, -1)), Vector())),
                                    {f"{side}Eye": {"offset": [s * 0.005, 0, 0]} for side, s in SIDES})
        out["Face_Eyes_Height"] = (self.field(lambda i, p: sum(eye_region(i, p, s) for s in (1, -1)) * v(0, 0, 0.005)),
                                   {f"{side}Eye": {"offset": [0, 0, 0.005]} for side, s in SIDES})
        out["Face_Eyes_Size"] = (self.field(lambda i, p: sum((eye_region(i, p, s) * (p - eyes[s].c) * 0.12 for s in (1, -1)), Vector())),
                                 {f"{side}Eye": {"scale": 1.12} for side, s in SIDES})

        def tilt(i, p):
            total = Vector()
            for s in (1, -1):
                e = eyes[s]
                u, _ = e.ut(p)
                lat = (u - (e.u_in + e.u_out) / 2) / (e.u_out - e.u_in)
                total += eye_region(i, p, s) * v(0, 0, 0.006 * lat)
            return total

        out["Face_Eyes_Tilt"] = (self.field(tilt), {})
        out["Face_Brows_Height"] = (add(self.brow(1, raise_=0.8), self.brow(-1, raise_=0.8)), {})
        nose_tip = max((P[i] for i in self.lm.verts(FS_NOSE)), key=lambda p: -p.y)
        nose = lambda i, p: ellipse(p, nose_tip + v(0, 0.012, 0.005), 0.03, 0.035, 0.035) if self.head[i] else 0.0
        out["Face_Nose_Width"] = (self.field(lambda i, p: nose(i, p) * v(p.x * 0.3, 0, 0)), {})
        out["Face_Nose_Length"] = (self.field(lambda i, p: nose(i, p) * v(0, -0.004, -0.004) * smoothstep(nose_tip.z + 0.02, nose_tip.z - 0.01, p.z)), {})
        out["Face_Nose_Bridge"] = (self.field(lambda i, p: (ellipse(p, nose_tip + v(0, 0.006, 0.03), 0.014, 0.03, 0.03) if self.head[i] else 0) * v(0, -0.004, 0)), {})
        out["Face_Nose_Tip"] = (self.field(lambda i, p: (ellipse(p, nose_tip, 0.018, 0.02, 0.016) if self.head[i] else 0) * v(0, -0.001, 0.004)), {})
        out["Face_Mouth_Width"] = (self.field(lambda i, p: sum((self.corner_weight(p, s, 0.02) * v(s * 0.006, 0.001, 0) for s in (1, -1)), Vector())), {})
        out["Face_Lips_Full"] = (self.field(lambda i, p: (self.upper_lip_band(i) + self.lower_lip_band(i)) * (0 if i in self.lower_inside or i in self.upper_lip else 1) * self.N[i] * 0.0028), {})
        chin = min((P[i] for i in self.lm.verts(FS_JAW_SIDE) if abs(P[i].x) < 0.004), key=lambda p: p.z)
        chin_w = lambda i, p: ellipse(p, chin + v(0, 0.01, 0.006), 0.035, 0.04, 0.03) if self.head[i] else 0.0
        out["Face_Chin_Length"] = (self.field(lambda i, p: chin_w(i, p) * v(0, 0, -0.008)), {})
        out["Face_Chin_Forward"] = (self.field(lambda i, p: chin_w(i, p) * v(0, -0.006, 0)), {})
        jaw = self.jaw_w
        out["Face_Jaw_Width"] = (self.field(lambda i, p: jaw[i] * smoothstep(0.02, 0.07, abs(p.x)) * v(math.copysign(0.008, p.x), 0, 0) if i not in self.lower_inside else v(0, 0, 0)), {})
        out["Face_Cheekbones"] = (self.field(lambda i, p: sum((gauss((p - v(s * 0.062, self.mouth.y + 0.035, self.mouth.z + 0.055)).length_squared, 0.017) * v(s * 0.004, -0.002, 0.001) for s in (1, -1)), Vector())), {})
        crown_z = max(p.z for p in P)
        out["Face_Head_Width"] = (self.field(lambda i, p: v(p.x * 0.07, 0, 0) * smoothstep(self.mouth.z + 0.02, self.mouth.z + 0.09, p.z)), {})
        out["Face_Head_Length"] = (self.field(lambda i, p: v(0, 0, 0.012) * smoothstep(eyes[1].top_z + 0.01, crown_z, p.z)), {})
        ears = set(self.lm.verts(FS_EAR))
        ear_c = {s: centroid(P[i] for i in self.lm.verts(FS_EAR, s)) for s in (1, -1)}
        out["Face_Ears_Size"] = (self.field(lambda i, p: (p - ear_c[1 if p.x > 0 else -1]) * 0.18 if i in ears else v(0, 0, 0)), {})
        out["Face_Ears_Out"] = (self.field(lambda i, p: v(math.copysign(0.006, p.x), 0.002, 0) * smoothstep(0.0, 0.02, abs(p.x) - abs(ear_c[1].x) + 0.012) if i in ears else v(0, 0, 0)), {})
        return out


def relax_lids(P, lm, eye_centres, eye_radius):
    """The resting lids lowered from the base mesh's wide-open stare, so
    the upper lid just covers the top of the iris. Returns new positions."""
    rig = FaceRig(P, lm, eye_centres, eye_radius)
    out = list(P)
    for _, s in SIDES:
        for i, d in rig.lids(s, -0.13, 0.04).items():
            out[i] = out[i] + d
    return out


def add(*fields, half_centre=False):
    out = {}
    for f in fields:
        for i, d in f.items():
            out[i] = out.get(i, Vector()) + d
    return out


# -- geometry follows the skin ------------------------------------------------

class SkinFollow:
    """Moves extra geometry (brows, lashes, clothes) with a mesh's shapes:
    each point takes the delta of the nearest point on the mesh surface,
    interpolated across that triangle."""

    def __init__(self, P, tris):
        self.P = P
        self.tris = tris
        self.bvh = BVHTree.FromPolygons([tuple(p) for p in P], tris)

    def bind(self, points, max_dist=1.0):
        binds = []
        for q in points:
            hit = self.bvh.find_nearest(q, max_dist)
            if hit[0] is None:
                binds.append(None)
                continue
            loc, _, ti, _ = hit
            a, b, c = (self.P[k] for k in self.tris[ti])
            binds.append((self.tris[ti], barycentric(loc, a, b, c)))
        return binds

    @staticmethod
    def deltas(binds, field):
        """Per bound point, the interpolated delta of `field` ({vertex: delta})."""
        out = {}
        for n, bd in enumerate(binds):
            if bd is None:
                continue
            (i, j, k), (wa, wb, wc) = bd
            d = field.get(i, Vector()) * wa + field.get(j, Vector()) * wb + field.get(k, Vector()) * wc
            if d.length_squared > 1e-12:
                out[n] = d
        return out


def barycentric(p, a, b, c):
    v0, v1, v2 = b - a, c - a, p - a
    d00, d01, d11 = v0.dot(v0), v0.dot(v1), v1.dot(v1)
    d20, d21 = v2.dot(v0), v2.dot(v1)
    den = d00 * d11 - d01 * d01 or 1e-12
    wb = (d11 * d20 - d01 * d21) / den
    wc = (d00 * d21 - d01 * d20) / den
    return (1.0 - wb - wc, wb, wc)


def triangles(mesh):
    mesh.calc_loop_triangles()
    return [tuple(t.vertices) for t in mesh.loop_triangles]


# -- brows and lashes -------------------------------------------------------------

def surface_point(bvh, x, z, y_from=-0.4):
    """The skin's front surface at (x, z), and its normal."""
    loc, nrm, _, _ = bvh.ray_cast(Vector((x, y_from, z)), Vector((0, 1, 0)))
    return loc, nrm


def build_brows(face, bvh):
    """Eyebrows: a thick, tapering, softly arched strip on each brow ridge,
    lying on the skin. Returns (verts, faces)."""
    verts, faces = [], []
    for _, s in SIDES:
        e = face.eyes[s]
        n_len, rows = 16, 5
        base = len(verts)
        u0, u1 = e.u_in - 0.004, e.u_out + 0.008
        for a in range(n_len + 1):
            t = a / n_len
            u = u0 + (u1 - u0) * t
            arch = 0.0055 * math.sin(math.pi * min(1.0, t * 1.25)) - 0.004 * t * t
            zc = e.top_z + 0.013 + arch
            width = 0.0032 + 0.0062 * (1.0 - t) ** 0.7
            if t < 0.06:
                width *= 0.55 + 0.45 * (t / 0.06)
            for b in range(rows):
                f = b / (rows - 1) - 0.5
                x = e.c.x + s * u
                z = zc + f * width
                loc, nrm = surface_point(bvh, x, z)
                lift = 0.0016 * (1.0 - (2 * f) ** 2) * (0.5 + 0.5 * (1 - t)) + 0.0003
                verts.append(loc + nrm * lift)
        for a in range(n_len):
            for b in range(rows - 1):
                i0 = base + a * rows + b
                q = [i0, i0 + 1, i0 + rows + 1, i0 + rows]
                faces.append(q if s > 0 else q[::-1])
    return verts, faces


def build_lashes(face, P, normals):
    """Upper lashes: a dark wing along each upper lid margin, longest
    toward the outer corner, curling up."""
    verts, faces, roots = [], [], []
    for _, s in SIDES:
        e = face.eyes[s]
        margin = sorted(e.upper_margin, key=lambda i: e.ut(P[i])[0])
        base = len(verts)
        count = len(margin)
        for a, i in enumerate(margin):
            t = a / max(1, count - 1)
            u, _ = e.ut(P[i])
            taper = e.taper(u) ** 0.5
            length = (0.0030 + 0.0028 * smoothstep(0.25, 0.85, t)) * taper + 0.0006
            radial = (P[i] - e.c).normalized()
            root = P[i] + radial * 0.0006
            fwd = Vector((0, -1, 0))
            up = Vector((0, 0, 1))
            first = (fwd * 0.75 + radial * 0.45).normalized()
            curl = (fwd * 0.35 + up * 0.85 + radial * 0.2).normalized()
            mid = root + first * length * 0.55
            tip = mid + curl * length * 0.6 + Vector((s * 0.0010 * t, 0, 0))
            for q in (root, mid, tip):
                verts.append(q)
                roots.append((i, s))
        for a in range(count - 1):
            for b in range(2):
                i0 = base + a * 3 + b
                q = [i0, i0 + 3, i0 + 4, i0 + 1]
                faces.append(q if s > 0 else q[::-1])
                faces.append(q[::-1] if s > 0 else q)
    return verts, faces, roots


def lash_deltas(face, P, verts, roots, field):
    """Each lash vertex turned about its eyeball as its root on the lid
    margin is (and moved by what's left of the root's delta)."""
    out = {}
    for n, (q, (i, s)) in enumerate(zip(verts, roots)):
        d = field.get(i)
        if d is None:
            continue
        e = face.eyes[s]
        before = e.ut(P[i])[1]
        after = e.ut(P[i] + d)[1]
        turned = e.turn(P[i], after - before)
        rest = d - (turned - P[i])
        out[n] = (e.turn(q, after - before) - q) + rest
    return out
