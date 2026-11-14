"""The body's own blend shapes: the feminine frame, the build shapes and
the clothing masks, each a {vertex: delta} field over the body.

Masks: a clothing item names the masks it needs (`MASKS`); at weight 1 a
mask pulls the covered skin about a centimetre inside the clothes, so it
can't poke through, and feathers out where the weights of the next bone
take over, so the visible edge stays smooth. The body stays one seamless
mesh at every level of detail.
"""

import math

from mathutils import Vector

from common import (
    FS_FACE, FS_FOREARM, FS_JAW_SIDE, FS_PELVIS, FS_SHIN, FS_SHOULDERS, FS_THIGH, FS_TORSO,
    FS_UNDER_JAW, FS_UPPER_ARM, SIDES, smooth_field, smoothstep, v,
)

# Mask name -> (bones, part of the bone's length (from, to) or None).
MASKS = {
    "Mask_Torso": (("Spine", "Chest", "UpperChest", "LeftShoulder", "RightShoulder"), None),
    "Mask_Hips": (("Hips",), None),
    "Mask_UpperArms_Top": (("LeftUpperArm", "RightUpperArm"), (0.0, 0.5)),
    "Mask_UpperArms_Low": (("LeftUpperArm", "RightUpperArm"), (0.5, 1.0)),
    "Mask_Forearms": (("LeftLowerArm", "RightLowerArm"), None),
    "Mask_Thighs_Top": (("LeftUpperLeg", "RightUpperLeg"), (0.0, 0.5)),
    "Mask_Thighs_Low": (("LeftUpperLeg", "RightUpperLeg"), (0.5, 1.0)),
    "Mask_Shins_Top": (("LeftLowerLeg", "RightLowerLeg"), (0.0, 0.6)),
    "Mask_Shins_Low": (("LeftLowerLeg", "RightLowerLeg"), (0.6, 1.0)),
    "Mask_Feet": (("LeftFoot", "RightFoot", "LeftToes", "RightToes"), None),
}

MASK_DEPTH = 0.011


def bone_param(p, head, tail):
    d = tail - head
    return (p - head).dot(d) / d.length_squared


def masks(P, normals, weights, joints):
    """Mask name -> field. `weights[i]` is {bone: weight} of vertex i."""
    out = {}
    for name, (bones, part) in MASKS.items():
        field = {}
        depth = MASK_DEPTH * (0.7 if name == "Mask_Feet" else 1.0)
        for i, p in enumerate(P):
            m = 0.0
            for b in bones:
                w = weights[i].get(b, 0.0)
                if w <= 0.0:
                    continue
                if part is not None:
                    head, tail, _ = joints[b]
                    t = bone_param(p, head, tail)
                    lo, hi = part
                    edge_lo = 1.0 if lo <= 0.0 else smoothstep(lo - 0.08, lo + 0.08, t)
                    edge_hi = 1.0 if hi >= 1.0 else 1.0 - smoothstep(hi - 0.08, hi + 0.08, t)
                    w *= edge_lo * edge_hi
                m += w
            if m > 1e-3:
                field[i] = -normals[i] * depth * min(1.0, m)
        out[name] = field
    return out


def frame_feminine(M, F):
    """The feminine frame: the feminine base body's positions over the
    masculine one's (the same topology)."""
    return {i: f - m for i, (m, f) in enumerate(zip(M, F)) if (f - m).length_squared > 1e-12}


def _amount_field(P, lm, nbr, per_set, bumps, smooth=6):
    """A per-vertex amount: a value per face set, plus gaussian bumps
    (centre, radius, amount, front-only), smoothed over the mesh."""
    amount = [0.0] * len(P)
    for s, a in per_set.items():
        for i in lm.verts(s):
            amount[i] = a
    amount = smooth_field(amount, nbr, smooth)
    for i, p in enumerate(P):
        for c, r, a, facing in bumps:
            d2 = (p - c).length_squared
            if d2 > 9 * r * r:
                continue
            g = math.exp(-d2 / (2 * r * r))
            amount[i] += a * g * (facing(i) if facing else 1.0)
    return amount


def builds(P, lm, normals, nbr, joints):
    """Build_Heavy, Build_Slim and Build_Muscular."""
    J = {k: (h, t) for k, (h, t, _) in joints.items()}
    front = lambda i: smoothstep(0.1, 0.6, -normals[i].y)
    back = lambda i: smoothstep(0.1, 0.6, normals[i].y)
    waist = J["Spine"][0]
    chest = J["Chest"][1]
    heavy_bumps = [
        (waist + v(0, -0.09, 0.02), 0.09, 0.035, front),       # belly
        (waist + v(0, -0.07, -0.08), 0.07, 0.015, front),      # lower belly
        (chest + v(0, -0.06, -0.06), 0.08, 0.010, front),      # chest
        (J["Neck"][0] + v(0, -0.05, 0.13), 0.035, 0.012, None),  # under the chin
    ]
    for _, s in SIDES:
        heavy_bumps.append((waist + v(s * 0.13, 0.0, -0.02), 0.06, 0.018, None))  # flanks
        heavy_bumps.append((J["Hips"][0] + v(s * 0.1, 0.06, -0.08), 0.07, 0.014, back))  # seat
    heavy = _amount_field(P, lm, nbr, {
        FS_TORSO: 0.012, FS_PELVIS: 0.014, FS_SHOULDERS: 0.008, FS_UPPER_ARM: 0.011,
        FS_FOREARM: 0.006, FS_THIGH: 0.018, FS_SHIN: 0.009, FS_FACE: 0.003, FS_JAW_SIDE: 0.006,
        FS_UNDER_JAW: 0.008,
    }, heavy_bumps)
    slim = _amount_field(P, lm, nbr, {
        FS_TORSO: -0.008, FS_PELVIS: -0.008, FS_SHOULDERS: -0.004, FS_UPPER_ARM: -0.005,
        FS_FOREARM: -0.003, FS_THIGH: -0.009, FS_SHIN: -0.004, FS_JAW_SIDE: -0.002,
    }, [(waist + v(0, -0.08, 0.0), 0.08, -0.012, front)])
    muscle_bumps = []
    for side, s in SIDES:
        sh, el = J[f"{side}UpperArm"]
        wr = J[f"{side}LowerArm"][1]
        hip, knee = J[f"{side}UpperLeg"]
        ankle = J[f"{side}LowerLeg"][1]
        lat = v(s, 0, 0)
        muscle_bumps += [
            (sh + lat * 0.035 + v(0, 0, 0.0), 0.045, 0.014, None),           # deltoid
            (sh.lerp(el, 0.5) + v(0, -0.03, 0), 0.04, 0.012, None),           # biceps
            (sh.lerp(el, 0.5) + v(0, 0.03, 0), 0.04, 0.010, None),            # triceps
            (el.lerp(wr, 0.25), 0.04, 0.008, None),                           # forearm
            (chest + v(s * 0.075, -0.07, -0.02), 0.06, 0.016, front),         # pectoral
            (J["Neck"][0] + v(s * 0.07, 0.03, -0.01), 0.045, 0.012, None),    # trapezius
            (chest + v(s * 0.11, 0.05, -0.06), 0.07, 0.012, back),            # lats
            (hip.lerp(knee, 0.45) + v(0, -0.04, 0), 0.07, 0.014, front),      # quadriceps
            (knee.lerp(ankle, 0.3) + v(0, 0.04, 0), 0.05, 0.013, back),       # calf
            (J["Hips"][0] + v(s * 0.08, 0.08, -0.07), 0.06, 0.010, back),     # glutes
        ]
    muscle_bumps.append((waist + v(0, -0.1, 0.06), 0.06, 0.006, front))       # abdominals
    muscle = _amount_field(P, lm, nbr, {}, muscle_bumps, smooth=0)
    out = {}
    for name, amount in (("Build_Heavy", heavy), ("Build_Slim", slim), ("Build_Muscular", muscle)):
        out[name] = {i: normals[i] * a for i, a in enumerate(amount) if abs(a) > 1e-5}
    return out


def joint_offsets(joints_from, joints_to, scale=1.0):
    """Bone name -> {"offset": [x, y, z]}: how far each bone's head moves
    between two fittings (the skeleton following a body shape)."""
    out = {}
    for name, (head, _, _) in joints_from.items():
        d = (joints_to[name][0] - head) * scale
        if d.length > 1e-4:
            out[name] = {"offset": [round(d.x, 5), round(d.y, 5), round(d.z, 5)]}
    return out
