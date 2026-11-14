"""The Axcenity Humanoid skeleton fitted to the base body, the arm re-pose to
the 45 degree A-pose, and skin weights.

Joints are found from the base mesh's face sets: the loop where two sets
meet (upper arm and forearm, say) is a ring round the limb, and its
centre is the joint. The rings are vertex indices, so the same landmarks
give the joints of either frame, or of a re-posed body.
"""

import math
from collections import defaultdict

import bpy
from mathutils import Matrix, Vector

from common import (
    ARM_DOWN, FS_ANKLE, FS_EAR, FS_FACE, FS_FINGERS, FS_FOOT, FS_FOREARM, FS_JAW_SIDE,
    FS_LOWER_LIP, FS_NECK, FS_PALM, FS_PELVIS, FS_SHIN, FS_SHOULDERS, FS_THIGH, FS_TOE_BASE,
    FS_TORSO, FS_UNDER_JAW, FS_UPPER_ARM, FS_UPPER_LIP, SIDES, bm_of, centroid, face_sets,
    select_only, v,
)

FINGERS = ("Thumb", "Index", "Middle", "Ring", "Little")
SEGMENTS = ("Proximal", "Intermediate", "Distal")


class Landmarks:
    """Vertex index sets of the base body: each face set's vertices and the
    rings where two sets meet."""

    def __init__(self, body):
        sets = face_sets(body)
        bm = bm_of(body)
        self.of_set = defaultdict(set)
        for f in bm.faces:
            self.of_set[sets[f.index]].update(vert.index for vert in f.verts)
        self._border = defaultdict(set)
        for e in bm.edges:
            if len(e.link_faces) != 2:
                continue
            a, b = (sets[f.index] for f in e.link_faces)
            if a != b:
                self._border[frozenset((a, b))].update(vert.index for vert in e.verts)
        self.x = [vert.co.x for vert in bm.verts]
        bm.free()

    def ring(self, a, b, side=0):
        """Vertices on the border of sets `a` and `b` (or any of `b` if it's
        a tuple), on one side (+1 left, -1 right) or all (0)."""
        bs = b if isinstance(b, (tuple, list, set)) else (b,)
        out = set()
        for bb in bs:
            out |= self._border.get(frozenset((a, bb)), set())
        return self._side(out, side)

    def verts(self, s, side=0):
        return self._side(self.of_set[s], side)

    def _side(self, idx, side):
        if side == 0:
            return sorted(idx)
        return sorted(i for i in idx if self.x[i] * side > 0.0005)


def at(P, idx):
    return centroid(P[i] for i in idx)


def fit_joints(P, lm, eyes):
    """Bone name -> (head, tail, parent) from body positions `P` and the
    eye centres `eyes` ({1: left, -1: right})."""
    j = {}
    crown = max(p.z for p in P)
    # Centre line.
    waist = at(P, lm.ring(FS_PELVIS, FS_TORSO))
    neck_base = at(P, lm.ring(FS_NECK, FS_TORSO))
    head_sets = (FS_FACE, FS_JAW_SIDE, FS_UNDER_JAW, FS_EAR, FS_UPPER_LIP, FS_LOWER_LIP)
    neck_top = at(P, lm.ring(FS_NECK, head_sets))
    centre = [P[i] for i in lm.verts(FS_PELVIS) if abs(P[i].x) < 0.004]
    crotch_z = min(p.z for p in centre)
    thigh_l = at(P, lm.ring(FS_THIGH, FS_PELVIS, 1))
    hip_z = crotch_z + 0.075
    j["Root"] = (v(0, 0, 0), v(0, -0.12, 0), None)
    spine_y = waist.y + 0.012
    hips = v(0, spine_y, hip_z + 0.05)
    spine = v(0, spine_y, waist.z)
    neck = v(0, neck_base.y + 0.008, neck_base.z)
    j["Hips"] = (hips, spine, "Root")
    chest = spine.lerp(neck, 0.32)
    upper = spine.lerp(neck, 0.64)
    j["Spine"] = (spine, chest, "Hips")
    j["Chest"] = (chest, upper, "Spine")
    j["UpperChest"] = (upper, neck, "Chest")
    head = v(0, neck_top.y + 0.01, neck_top.z + 0.01)
    j["Neck"] = (neck, head, "UpperChest")
    j["Head"] = (head, v(0, head.y, crown), "Neck")
    # The jaw hinges in front of the ears, below them.
    ear = at(P, lm.verts(FS_EAR, 1))
    chin_line = [P[i] for i in lm.verts(FS_JAW_SIDE) if abs(P[i].x) < 0.004]
    chin = min(chin_line, key=lambda p: p.z)
    hinge = v(0, ear.y - 0.012, ear.z - 0.035)
    j["Jaw"] = (hinge, v(0, chin.y + 0.02, chin.z + 0.012), "Head")
    for side, s in SIDES:
        eye = eyes[s]
        j[f"{side}Eye"] = (eye, eye + v(0, -0.03, 0), "Head")
        elbow = at(P, lm.ring(FS_FOREARM, FS_UPPER_ARM, s))
        deltoid = at(P, lm.ring(FS_SHOULDERS, FS_UPPER_ARM, s))
        axis = (deltoid - elbow).normalized()
        shoulder = deltoid + axis * 0.32 * (deltoid - elbow).length
        clav = v(s * 0.02, neck.y, neck.z - 0.02)
        wrist = at(P, lm.ring(FS_FOREARM, FS_PALM, s))
        j[f"{side}Shoulder"] = (clav, shoulder, "UpperChest")
        j[f"{side}UpperArm"] = (shoulder, elbow, f"{side}Shoulder")
        j[f"{side}LowerArm"] = (elbow, wrist, f"{side}UpperArm")
        knuckles = {}
        for finger in FINGERS:
            p_set, i_set, d_set = FS_FINGERS[finger]
            base = at(P, lm.ring(FS_PALM, p_set, s))
            pip = at(P, lm.ring(p_set, i_set, s))
            dip = at(P, lm.ring(i_set, d_set, s))
            tip_verts = [P[i] for i in lm.verts(d_set, s)]
            direction = (dip - pip).normalized()
            tip = max(tip_verts, key=lambda p: (p - dip).dot(direction))
            tip = dip + (tip - dip) * 0.85
            if finger == "Thumb":
                # The thumb's first bone starts deep in the palm, near the wrist.
                mcp = wrist.lerp(base, 0.55)
                mcp = mcp + (base - wrist).normalized().cross(v(0, 0, 1)) * 0.0
            else:
                # The knuckle sits inside the hand, behind where the finger shows.
                along = (pip - base).normalized()
                mcp = base - along * 0.25 * (pip - base).length
            knuckles[finger] = mcp
            chain = [(mcp, pip), (pip, dip), (dip, tip)]
            parent = f"{side}Hand"
            for seg, (a, b) in zip(SEGMENTS, chain):
                bone = f"{side}{finger}{seg}"
                j[bone] = (a, b, parent)
                parent = bone
        j[f"{side}Hand"] = (wrist, knuckles["Middle"], f"{side}LowerArm")
        thigh = at(P, lm.ring(FS_THIGH, FS_PELVIS, s))
        hip = v(s * abs(thigh.x) * 1.12, thigh.y + 0.005, hip_z)
        knee = at(P, lm.ring(FS_THIGH, FS_SHIN, s))
        ankle = at(P, lm.ring(FS_SHIN, FS_ANKLE, s)).lerp(at(P, lm.ring(FS_ANKLE, FS_FOOT, s)), 0.6)
        ball = at(P, lm.ring(FS_FOOT, FS_TOE_BASE, s))
        toe_verts = [P[i] for i in lm.verts(FS_TOE_BASE, s)]
        toe_tip = min(toe_verts, key=lambda p: p.y)
        ball = v(ball.x, ball.y, 0.025)
        j[f"{side}UpperLeg"] = (hip, knee, "Hips")
        j[f"{side}LowerLeg"] = (knee, ankle, f"{side}UpperLeg")
        j[f"{side}Foot"] = (ankle, ball, f"{side}LowerLeg")
        j[f"{side}Toes"] = (ball, v(ball.x, toe_tip.y + 0.01, 0.025), f"{side}Foot")
    return j


def palm_normal(joints, side, s):
    """The palm's normal (toward the body in the A-pose)."""
    wrist = joints[f"{side}Hand"][0]
    knuckle = joints[f"{side}Hand"][1]
    index = joints[f"{side}IndexProximal"][0]
    little = joints[f"{side}LittleProximal"][0]
    along = (knuckle - wrist).normalized()
    across = (index - little).normalized()  # toward the thumb side
    return (along.cross(across) * s).normalized()


def build_armature(joints, name="AxcenityHumanoid"):
    """An armature from `joints`, with the rolls the clips are keyed for:
    +X swings a limb forward and bends an elbow, finger curl is -X."""
    arm = bpy.data.armatures.new(name)
    obj = bpy.data.objects.new(name, arm)
    bpy.context.scene.collection.objects.link(obj)
    select_only([obj])
    bpy.ops.object.mode_set(mode="EDIT")
    for bone_name, (head, tail, _) in joints.items():
        eb = arm.edit_bones.new(bone_name)
        eb.head = head
        eb.tail = tail
        eb.use_deform = bone_name != "Root"
    for bone_name, (_, _, parent) in joints.items():
        eb = arm.edit_bones[bone_name]
        if parent:
            eb.parent = arm.edit_bones[parent]
        d = (eb.tail - eb.head).normalized()
        if any(f in bone_name for f in FINGERS) and bone_name[4:] not in ("Hand",):
            side = "Left" if bone_name.startswith("Left") else "Right"
            s = 1 if side == "Left" else -1
            eb.align_roll(-palm_normal(joints, side, s))
        elif abs(d.y) > 0.7:
            eb.align_roll(Vector((0, 0, 1)))
        else:
            eb.align_roll(Vector((0, -1, 0)))
    bpy.ops.object.mode_set(mode="OBJECT")
    obj.select_set(False)
    return obj


def auto_weights(body, armature, skip=("Root", "Jaw", "LeftEye", "RightEye")):
    """Bone-heat weights from the armature onto `body` (skipping bones
    that get their weights by hand)."""
    for b in armature.data.bones:
        b.use_deform = b.name not in skip
    select_only([body, armature], active=armature)
    bpy.ops.object.parent_set(type="ARMATURE_AUTO")
    for b in armature.data.bones:
        b.use_deform = b.name != "Root"


def repose_arms(body, P, lm, eyes):
    """Turns the base body's arms (about 60 degrees below horizontal) up to
    the 45 degree A-pose, straightening the elbow's droop. Returns the new
    positions; `body` is left as it was."""
    joints = fit_joints(P, lm, eyes)
    arm = build_armature(joints, "Repose")
    work = body.copy()
    work.data = body.data.copy()
    bpy.context.scene.collection.objects.link(work)
    for vg in list(work.vertex_groups):
        work.vertex_groups.remove(vg)
    for vert, p in zip(work.data.vertices, P):
        vert.co = p
    work.data.update()
    auto_weights(work, arm)
    select_only([arm])
    bpy.ops.object.mode_set(mode="POSE")
    for side, s in SIDES:
        for bone in ("UpperArm", "LowerArm"):
            pb = arm.pose.bones[f"{side}{bone}"]
            bpy.context.view_layer.update()
            m = pb.matrix.copy()
            head = m.to_translation()
            d = (pb.tail - pb.head).normalized()
            r = math.sqrt(max(0.0, 1.0 - d.y * d.y))
            target = Vector((s * math.cos(ARM_DOWN) * r, d.y, -math.sin(ARM_DOWN) * r)).normalized()
            rot = d.rotation_difference(target).to_matrix().to_4x4()
            pb.matrix = Matrix.Translation(head) @ rot @ Matrix.Translation(-head) @ m
    bpy.context.view_layer.update()
    bpy.ops.object.mode_set(mode="OBJECT")
    deps = bpy.context.evaluated_depsgraph_get()
    evaluated = work.evaluated_get(deps)
    out = [vert.co.copy() for vert in evaluated.data.vertices]
    bpy.data.objects.remove(work)
    bpy.data.objects.remove(arm)
    return out


def limit_weights(obj, keep=4):
    """At most `keep` influences per vertex, normalized (the engine reads 4)."""
    groups = {vg.index: vg for vg in obj.vertex_groups}
    for vert in obj.data.vertices:
        ws = sorted(((g.weight, g.group) for g in vert.groups if g.group in groups), reverse=True)
        drop = ws[keep:]
        kept = [w for w in ws[:keep] if w[0] > 0.002]
        total = sum(w for w, _ in kept) or 1.0
        for _, gi in drop:
            groups[gi].remove([vert.index])
        for w, gi in ws[:keep]:
            if w <= 0.002:
                groups[gi].remove([vert.index])
            else:
                groups[gi].add([vert.index], w / total, "REPLACE")
