"""Renders the humanoid QA sheets: the same cameras, lights, backdrop,
resolution and Cycles settings every run, so a later "after" set compares
one to one with this "before" set.

    python -I render_baseline.py --out DIR [--parts HumanoidParts.glb]
                                 [--default Humanoid.glb] [--only a,b] [--samples 32]

Sheets: full_body_turnaround_masculine (the shipped Humanoid.glb),
full_body_turnaround_feminine, face_closeups, face_topology, body_clay,
build_range, hair_styles, deformation_poses, expressions, details. Every
look other than the masculine turnaround is built from the parts file:
the default outfit with the masks the customization file lists, the
default hair, Frame_Feminine with its bone offsets for the feminine frame.
"""

import argparse
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import bpy  # noqa: E402
from mathutils import Vector  # noqa: E402

import qa_scene as Q  # noqa: E402

FACE = ["Body", "Eyes", "Teeth", "Brows", "Lashes"]
FRAMES = (("masculine", 0.0), ("feminine", 1.0))
VIEWS = (("front", 0), ("three-quarter", 35), ("side", 90), ("back", 180))

# Cameras (Blender space, the character faces -Y), all 85 mm unless noted.
CAM_FULL = ((0.0, -4.72, 0.95), (0.0, 0.0, 0.92), 85)
CAM_FACE = ((0.0, -1.13, 1.62), (0.0, -0.03, 1.60), 85)
CAM_HAIR = ((0.0, -1.25, 1.64), (0.0, -0.01, 1.60), 85)
CAM_POSE = {
    "rest": ((0.0, -4.72, 0.95), (0.0, 0.0, 0.92), 85),
    "elbow_135": ((1.40, -0.90, 1.30), (0.32, -0.01, 1.19), 85),
    "knee_135": ((1.95, -0.30, 0.66), (0.09, 0.10, 0.60), 85),
    "shoulder_abd_90": ((0.35, -1.95, 1.40), (0.35, 0.0, 1.30), 85),
    "shoulder_abd_150": ((0.10, -2.05, 1.45), (0.10, 0.0, 1.38), 85),
    "hip_flex_100": ((2.6, -0.50, 0.95), (0.08, -0.32, 0.85), 85),
    "wrist_70": ((0.95, -0.95, 1.05), (0.45, 0.0, 0.92), 85),
    "spine_twist_45": ((0.0, -3.2, 1.15), (0.0, 0.0, 1.10), 85),
    "fist": ((0.95, -0.80, 1.00), (0.55, 0.0, 0.90), 85),
}
FACE_EXPRESSIONS = [
    ("neutral", {}, None),
    ("smile", {"Mouth_Smile_Left": 1, "Mouth_Smile_Right": 1}, None),
    ("frown", {"Mouth_Frown_Left": 1, "Mouth_Frown_Right": 1}, None),
    ("jaw open", {"Jaw_Open": 1}, Q.jaw_open),
    ("jaw open + smile", {"Jaw_Open": 1, "Mouth_Smile_Left": 1, "Mouth_Smile_Right": 1}, Q.jaw_open),
    ("blink", {"Blink_Left": 1, "Blink_Right": 1}, None),
    ("brow raise", {"Brow_Raise_Left": 1, "Brow_Raise_Right": 1}, None),
    ("sneer", {"Nose_Sneer_Left": 1, "Nose_Sneer_Right": 1}, None),
]
HAIRS = ["Hair_Swept", "Hair_Bob", "Hair_Long", "Hair_Crop", "Hair_Buzz", "Hair_Ponytail"]


class Run:
    def __init__(self, args):
        self.a = args
        self.tmp = tempfile.mkdtemp(prefix="humanoid_qa_")
        self.custom = Q.load_custom()
        self.outfit = self.custom["default"]["outfit"]
        self.hair = self.custom["default"]["hair"]

    def scene(self, path, w, h, merge=False):
        Q.reset()
        ch = Q.Character(path, merge_vertices=merge, custom=self.custom)
        cam = Q.setup_studio(w, h, self.a.samples)
        return ch, cam

    def tile(self, cam, name, cam_def):
        pos, target, lens = cam_def
        return Q.shot(cam, os.path.join(self.tmp, name + ".png"), pos, target, lens)

    def dressed(self, ch, frame=0.0, build=None, hair=None, outfit=None, expr=None, face=None):
        outfit = self.outfit if outfit is None else outfit
        hair = self.hair if hair is None else hair
        ch.show_only(FACE + list(outfit) + ([hair] if hair else []))
        ch.set_look(frame=frame, build=build, expr=expr, face=face, masks=ch.masks_for(outfit))

    def save(self, name, tiles, cols, labels, title, grey=False):
        out = os.path.join(self.a.out, name + ".png")
        Q.contact_sheet(out, tiles, cols, labels, title, grey=grey)
        print("wrote", out, os.path.getsize(out) // 1024, "KB")

    # -- sheets --------------------------------------------------------------------

    def turnaround(self, which):
        frame = 1.0 if which == "feminine" else 0.0
        if which == "masculine":
            ch, cam = self.scene(self.a.default, 320, 480)
            ch.set_look(masks=ch.masks_for(self.outfit), keep_defaults=True)
            src = "Humanoid.glb (shipped default)"
        else:
            ch, cam = self.scene(self.a.parts, 320, 480)
            self.dressed(ch, frame)
            src = "HumanoidParts.glb, Frame_Feminine 1"
        tiles, labels = [], []
        for label, deg in VIEWS:
            Q.turn_character(ch, deg)
            tiles.append(self.tile(cam, f"turn_{which}_{deg}", CAM_FULL))
            labels.append(label)
        self.save(f"full_body_turnaround_{which}", tiles, 4, labels, f"Default outfit, {which} - {src}")

    def face_closeups(self):
        ch, cam = self.scene(self.a.parts, 320, 400)
        tiles, labels = [], []
        for fname, frame in FRAMES:
            self.dressed(ch, frame)
            for label, deg in VIEWS[:2]:
                Q.turn_character(ch, deg)
                tiles.append(self.tile(cam, f"face_{fname}_{deg}", CAM_FACE))
                labels.append(f"{fname} {label}")
        self.save("face_closeups", tiles, 4, labels, "Face, default hair and outfit, real materials")

    def face_topology(self):
        ch, cam = self.scene(self.a.parts, 640, 640)
        Q.clay_parts(ch.parts.values(), Q.clay_kind)
        tiles, labels = [], []
        for fname, frame in FRAMES:
            ch.show_only(["Body", "Eyes"])
            ch.set_look(frame=frame)
            Q.turn_character(ch, 0)
            for label, deg in VIEWS[:2]:
                Q.turn_character(ch, deg)
                wire = Q.wire_overlay(ch.parts["Body"], 0.00035)
                tiles.append(self.tile(cam, f"topo_{fname}_{deg}", ((0.0, -0.97, 1.61), (0.0, -0.03, 1.60), 85)))
                bpy.data.objects.remove(wire)
                labels.append(f"{fname} {label}")
        self.save("face_topology", tiles, 2, labels, "Head topology (glTF triangles re-joined to quads), clay", grey=True)

    def body_clay(self):
        ch, cam = self.scene(self.a.parts, 320, 480)
        Q.clay_parts(ch.parts.values(), Q.clay_kind)
        tiles, labels = [], []
        for fname, frame in FRAMES:
            under = ["Underwear_Briefs"] + (["Underwear_Top"] if frame else [])
            ch.show_only(FACE + under)
            ch.set_look(frame=frame)
            for label, deg in (VIEWS[0], VIEWS[2]):
                Q.turn_character(ch, deg)
                wires = [Q.wire_overlay(ch.parts[n], 0.0018) for n in ["Body"] + under]
                tiles.append(self.tile(cam, f"body_{fname}_{deg}", CAM_FULL))
                for w in wires:
                    bpy.data.objects.remove(w)
                labels.append(f"{fname} {label}")
        self.save("body_clay", tiles, 4, labels, "Base body, underwear only, clay with wireframe", grey=True)

    def build_range(self):
        ch, cam = self.scene(self.a.parts, 320, 480)
        tiles, labels = [], []
        Q.turn_character(ch, 35)
        for fname, frame in FRAMES:
            for b in (None, "Build_Heavy", "Build_Slim", "Build_Muscular"):
                self.dressed(ch, frame, build={b: 1.0} if b else None)
                tiles.append(self.tile(cam, f"build_{fname}_{b}", CAM_FULL))
                labels.append(f"{fname} {(b or 'neutral').replace('Build_', '')}")
        self.save("build_range", tiles, 4, labels, "Builds at 1, default outfit, three-quarter")

    def hair_styles(self):
        ch, cam = self.scene(self.a.parts, 426, 400)
        tiles, labels = [], []
        Q.turn_character(ch, 35)
        for h in HAIRS:
            self.dressed(ch, 0.0, hair=h)
            tiles.append(self.tile(cam, f"hair_{h}", CAM_HAIR))
            labels.append(h)
        self.save("hair_styles", tiles, 3, labels, "Hair styles on the default (masculine) head, three-quarter")

    def deformation_poses(self):
        ch, cam = self.scene(self.a.parts, 426, 426)
        Q.clay_parts(ch.parts.values(), Q.clay_kind)
        tiles, labels = [], []
        self.dressed(ch, 0.0)
        for pose in Q.POSES:
            Q.apply_pose(ch, pose)
            tiles.append(self.tile(cam, f"pose_{pose}", CAM_POSE[pose]))
            labels.append(pose)
        ch.pose_reset()
        self.save("deformation_poses", tiles, 3, labels, "Test poses (left side), default outfit, clay", grey=True)

    def expressions(self):
        ch, cam = self.scene(self.a.parts, 320, 320)
        tiles, labels = [], []
        for fname, frame in FRAMES:
            for label, expr, pose in FACE_EXPRESSIONS:
                self.dressed(ch, frame, expr=expr)
                ch.pose_reset()
                if pose:
                    pose(ch)
                    Q.update()
                tiles.append(self.tile(cam, f"expr_{fname}_{label}", ((0.0, -0.98, 1.60), (0.0, -0.03, 1.585), 85)))
                labels.append(f"{fname[:4]} {label}")
        ch.pose_reset()
        self.save("expressions", tiles, 4, labels, "Expressions at 1 (jaw open = Jaw bone 25 deg + Jaw_Open), front")

    def details(self):
        """Close-ups of defects the measurements point at."""
        tiles, labels = [], []
        ch, cam = self.scene(self.a.default, 320, 320)
        ch.set_look(masks=ch.masks_for(self.outfit), keep_defaults=True)
        tiles.append(self.tile(cam, "d_waist", ((0.55, -1.1, 1.02), (0.0, -0.02, 0.98), 85)))
        labels.append("Humanoid.glb waist")
        tiles.append(self.tile(cam, "d_shoe", ((0.45, -0.75, 0.55), (0.10, -0.06, 0.05), 85)))
        labels.append("Humanoid.glb shoe")
        ch, cam = self.scene(self.a.parts, 320, 320)
        self.dressed(ch, 0.0, outfit=["Shirt_Tee", "Pants_Cargo", "Shoes_Sneakers"])
        tiles.append(self.tile(cam, "d_tee", ((0.9, -1.6, 1.15), (0.0, 0.0, 1.05), 85)))
        labels.append("tee without jacket")
        self.dressed(ch, 0.0, hair="Hair_Long")
        Q.turn_character(ch, 90)
        tiles.append(self.tile(cam, "d_long", ((0.0, -1.3, 1.55), (0.0, -0.01, 1.52), 85)))
        labels.append("Hair_Long side")
        Q.turn_character(ch, 0)
        self.dressed(ch, 0.0, expr={"Blink_Left": 1, "Blink_Right": 1})
        tiles.append(self.tile(cam, "d_blink", ((0.10, -0.40, 1.60), (0.035, -0.08, 1.59), 85)))
        labels.append("blink, left eye")
        self.dressed(ch, 0.0, expr={"Jaw_Open": 1})
        Q.jaw_open(ch)
        Q.update()
        Q.turn_character(ch, 30)
        tiles.append(self.tile(cam, "d_jaw", ((0.0, -0.45, 1.50), (0.0, -0.03, 1.49), 85)))
        labels.append("jaw open, three-quarter")
        ch.pose_reset()
        Q.turn_character(ch, 0)
        self.dressed(ch, 0.0, build={"Build_Heavy": 1.0}, hair="Hair_Buzz")
        Q.turn_character(ch, 35)
        tiles.append(self.tile(cam, "d_buzz", ((0.0, -1.2, 1.64), (0.0, -0.01, 1.61), 85)))
        labels.append("Hair_Buzz + Build_Heavy")
        Q.turn_character(ch, 0)
        self.dressed(ch, 0.0, hair="Hair_Bob", face={"Face_Ears_Out": 1, "Face_Ears_Size": 1})
        Q.turn_character(ch, 60)
        tiles.append(self.tile(cam, "d_bob_ears", ((0.0, -1.2, 1.62), (0.0, -0.01, 1.59), 85)))
        labels.append("Hair_Bob + Ears_Out/Size 1")
        Q.turn_character(ch, 0)
        self.dressed(ch, 0.0, hair="Hair_Long")
        tiles.append(self.tile(cam, "d_long_front", ((0.0, -1.3, 1.57), (0.0, -0.01, 1.54), 85)))
        labels.append("Hair_Long front")
        self.dressed(ch, 0.0)
        tiles.append(self.tile(cam, "d_eye", ((0.10, -0.40, 1.60), (0.035, -0.08, 1.59), 85)))
        labels.append("masculine left eye")
        self.dressed(ch, 1.0)
        tiles.append(self.tile(cam, "d_eye_f", ((0.12, -0.40, 1.60), (0.047, -0.085, 1.588), 85)))
        labels.append("feminine left eye")
        self.dressed(ch, 0.0)
        Q.apply_pose(ch, "hip_flex_100")
        tiles.append(self.tile(cam, "d_hip", ((1.0, 1.3, 0.75), (0.09, 0.05, 0.80), 85)))
        labels.append("hip_flex_100 from behind")
        ch.pose_reset()
        self.save("details", tiles, 4, labels, "Details the measurements point at")


SHEETS = ["turnaround_masculine", "turnaround_feminine", "face_closeups", "face_topology", "body_clay",
          "build_range", "hair_styles", "deformation_poses", "expressions", "details"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--parts", default=os.path.join(Q.ASSET_DIR, "HumanoidParts.glb"))
    ap.add_argument("--default", default=os.path.join(Q.ASSET_DIR, "Humanoid.glb"))
    ap.add_argument("--out", required=True)
    ap.add_argument("--only", default="")
    ap.add_argument("--samples", type=int, default=Q.STUDIO["samples"])
    a = ap.parse_args(sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:])
    os.makedirs(a.out, exist_ok=True)
    r = Run(a)
    only = [s for s in a.only.split(",") if s] or SHEETS
    for s in only:
        if s.startswith("turnaround_"):
            r.turnaround(s.split("_", 1)[1])
        else:
            getattr(r, s)()
    shutil.rmtree(r.tmp, ignore_errors=True)


if __name__ == "__main__":
    main()
