"""Builds the Axcenity default humanoid in Blender from Blender Studio's CC0
stylized base meshes (vendored in `source/`).

Run headless:  blender -b --factory-startup --python build.py -- --out DIR
(or with the `bpy` module: python build.py --out DIR)

The steps: load both body frames (re-posed to the 45 degree A-pose, lids
relaxed), fit the skeleton from the face sets, weight the body, give the
head its expression and identity shapes and the body its frame, build and
mask shapes, add eyes, brows, lashes and teeth, the outfits and hair
styles (each following the body's shapes), bake the skin texture, and
export `Humanoid.glb` (the default character), `HumanoidParts.glb` (every
part), `HumanoidAnimations.glb` (the clips) and
`Humanoid.customization.json` (what each slider moves). Units are metres,
Blender Z up, the character faces -Y (glTF +Z after export).
"""

import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import bpy  # noqa: E402
from mathutils import Vector  # noqa: E402

import body  # noqa: E402
import bodyshapes  # noqa: E402
import clothes  # noqa: E402
import face  # noqa: E402
import fit  # noqa: E402
import hair  # noqa: E402
import materials  # noqa: E402
import rig  # noqa: E402
from common import (  # noqa: E402
    FS_LOWER_LIP, FS_UPPER_LIP, SIDES, edge_neighbours, import_obj, mesh_from, new_object,
    shade_smooth, tri_count,
)

ARMATURE = "AxcenityHumanoid"

# The default character's parts (HumanoidParts.glb objects) and its masks.
DEFAULT_OUTFIT = ["Shirt_Tee", "Jacket_Cropped", "Pants_Cargo", "Shoes_Sneakers"]
DEFAULT_HAIR = "Hair_Swept"
FACE_PARTS = ["Eyes", "Teeth", "Brows", "Lashes"]

# The bones whose cloth gets smoothed weights (the trousers and briefs
# where they bridge the crease between the buttocks and the thighs).
HIP_BONES = ("Hips", "LeftUpperLeg", "RightUpperLeg")

# Garments worn over others, (inner, outer), cleared in this order: an
# untucked tee over the trousers' waistband, a jacket over a top.
LAYERS = [("Shirt_Tee", "Jacket_Cropped"), ("Underwear_Top", "Jacket_Cropped"),
          ("Pants_Cargo", "Shirt_Tee")]


def to_gltf(d):
    """A Blender-space vector (Z up, front -Y) in glTF space (Y up, front +Z)."""
    return [round(d[0], 5), round(d[2], 5), round(-d[1], 5)]


def vertex_weights(obj):
    names = {vg.index: vg.name for vg in obj.vertex_groups}
    return [{names[g.group]: g.weight for g in vert.groups} for vert in obj.data.vertices]


def apply_jaw(obj, jaw_w):
    jaw = obj.vertex_groups.get("Jaw") or obj.vertex_groups.new(name="Jaw")
    for i, w in enumerate(jaw_w):
        if w <= 0.0:
            continue
        for g in obj.data.vertices[i].groups:
            g.weight *= 1.0 - w
        jaw.add([i], w, "REPLACE")


def add_shapes(obj, base, shapes, defaults=None):
    """Shape keys on `obj` from {name: {vertex: delta}} over positions `base`."""
    if obj.data.shape_keys is None:
        obj.shape_key_add(name="Basis", from_mix=False)
    for name, field in shapes.items():
        if not field:
            continue
        key = obj.shape_key_add(name=name, from_mix=False)
        for i, d in field.items():
            key.data[i].co = base[i] + d
        key.value = (defaults or {}).get(name, 0.0)


def rigid_part(name, verts, faces, bone, armature, material):
    me = mesh_from(name, verts, faces)
    obj = new_object(name, me)
    shade_smooth(obj)
    group = obj.vertex_groups.new(name=bone)
    group.add(list(range(len(verts))), 1.0, "REPLACE")
    obj.data.materials.append(materials.material(material))
    return obj


def skin_to(obj, armature):
    obj.parent = armature
    mod = obj.modifiers.new("Armature", "ARMATURE")
    mod.object = armature


def follow_shapes(obj, follow, field_names, shapes, base=None, smooth=0):
    """Shape keys on `obj` (extra geometry) following the body's `shapes`,
    each smoothed over `obj`'s edges `smooth` times (`fit.smooth_field`)."""
    base = base or [vert.co.copy() for vert in obj.data.vertices]
    binds = follow.bind(base)
    fields = {}
    for name in field_names:
        if name in shapes:
            field = face.SkinFollow.deltas(binds, shapes[name])
            fields[name] = fit.smooth_field(obj, field, smooth) if smooth else field
    add_shapes(obj, base, fields)


def copy_weights(obj, follow, body_weights, only=None):
    """Weights for extra geometry from the body under it."""
    base = [vert.co.copy() for vert in obj.data.vertices]
    binds = follow.bind(base)
    groups = {}
    for n, bd in enumerate(binds):
        if bd is None:
            continue
        (i, j, k), (wa, wb, wc) = bd
        mix = {}
        for idx, w in ((i, wa), (j, wb), (k, wc)):
            for b, bw in body_weights[idx].items():
                mix[b] = mix.get(b, 0.0) + bw * max(0.0, w)
        if only:
            mix = {b: w for b, w in mix.items() if b in only} or {only[0]: 1.0}
        total = sum(mix.values()) or 1.0
        for b, w in mix.items():
            g = groups.get(b) or obj.vertex_groups.get(b) or obj.vertex_groups.new(name=b)
            groups[b] = g
            g.add([n], w / total, "REPLACE")
    rig.limit_weights(obj)


def fit_teeth(fr, P, lm):
    """The base mesh bundle's stylized teeth, sized into the mouth: upper
    teeth on the head, lower on the jaw."""
    (teeth,) = import_obj("teeth.obj")
    me = teeth.data
    xs = [vert.co for vert in me.vertices]
    lo = Vector([min(p[k] for p in xs) for k in range(3)])
    hi = Vector([max(p[k] for p in xs) for k in range(3)])
    inside = [P[i] for i in set(lm.verts(FS_UPPER_LIP)) | set(lm.verts(FS_LOWER_LIP))]
    centre_line = [p for p in inside if abs(p.x) < 0.004]
    front = min(p.y for p in centre_line) + 0.009
    width = fr.mouth_w * 2.0 * 1.05
    scale = width / (hi.x - lo.x)
    mid_z = (lo.z + hi.z) / 2
    # The bundle's teeth face -Y like the body; front edge at `front`.
    for vert in me.vertices:
        p = vert.co
        vert.co = Vector((p.x * scale, front + (p.y - lo.y) * scale, fr.mouth.z + (p.z - mid_z) * scale))
    me.update()
    islands = _islands(me)
    names, bones = [], {}
    for isl in islands:
        zc = sum(me.vertices[i].co.z for i in isl) / len(isl)
        upper = zc > fr.mouth.z
        for i in isl:
            bones[i] = "Head" if upper else "Jaw"
    big = sorted(islands, key=len)[-2:]
    gum = set(i for isl in big for i in isl)
    for poly in me.polygons:
        names.append("Gums" if poly.vertices[0] in gum else "Teeth")
    teeth.name = "Teeth"
    for b in ("Head", "Jaw"):
        g = teeth.vertex_groups.new(name=b)
        g.add([i for i, bb in bones.items() if bb == b], 1.0, "REPLACE")
    materials.assign(teeth, names)
    shade_smooth(teeth)
    # The bundle's teeth are for sculpting (13k triangles); a fifth is plenty.
    dec = teeth.modifiers.new("Decimate", "DECIMATE")
    dec.ratio = 0.2
    bpy.context.view_layer.objects.active = teeth
    bpy.ops.object.modifier_apply(modifier="Decimate")
    return teeth, (hi - lo) * scale


def _islands(me):
    nbr = edge_neighbours(me)
    seen, out = set(), []
    for start in range(len(me.vertices)):
        if start in seen:
            continue
        stack, isl = [start], []
        while stack:
            i = stack.pop()
            if i in seen:
                continue
            seen.add(i)
            isl.append(i)
            stack.extend(nbr[i] - seen)
        out.append(isl)
    return out


def build(out_dir):
    from common import reset_scene
    reset_scene()
    body_obj, lm, frames = body.load()
    M, F = frames["Masculine"], frames["Feminine"]
    P = M.P
    eyes_c = M.eye_centres()
    joints = rig.fit_joints(P, lm, eyes_c)
    joints_f = rig.fit_joints(F.P, lm, F.eye_centres())
    armature = rig.build_armature(joints, ARMATURE)
    rig.auto_weights(body_obj, armature)
    rig.limit_weights(body_obj)
    body_obj.data.update()
    normals = [vert.normal.copy() for vert in body_obj.data.vertices]
    nbr = edge_neighbours(body_obj.data)
    fr = face.FaceRig(P, lm, eyes_c, M.eye_radius, normals)
    fr.jaw_w = fr.jaw_weights(nbr)
    apply_jaw(body_obj, fr.jaw_w)
    rig.limit_weights(body_obj)
    weights = vertex_weights(body_obj)

    # The body's shapes.
    expressions = fr.shapes(nbr)
    identity = fr.identity()
    body_fields = {"Frame_Feminine": bodyshapes.frame_feminine(P, F.P)}
    body_fields.update(bodyshapes.builds(P, lm, normals, nbr, joints))
    masks = bodyshapes.masks(P, normals, weights, joints)
    shapes = {}
    shapes.update(expressions)
    shapes.update({k: f for k, (f, _) in identity.items()})
    shapes.update(body_fields)
    shapes.update(masks)
    tris = face.triangles(body_obj.data)
    follow = face.SkinFollow(P, tris)

    # Materials: the skin texture (with the lips), the mouth inside.
    skin_img = materials.bake_vertex_colours(
        body_obj, materials.skin_colours(P, fr, joints, lm), "Skin", 2048, out_dir)
    materials.material("Skin", skin_img)
    inside = set(lm.verts(FS_UPPER_LIP)) | set(lm.verts(FS_LOWER_LIP))
    names = []
    for poly in body_obj.data.polygons:
        deep = all(i in inside for i in poly.vertices) and poly.center.y > fr.mouth.y + 0.014
        names.append("Mouth" if deep else "Skin")
    materials.assign(body_obj, names)
    shade_smooth(body_obj)
    add_shapes(body_obj, P, shapes)
    skin_to(body_obj, armature)

    # Eyes, brows, lashes, teeth.
    iris_img = materials.iris_texture(out_dir)
    eye_objs = []
    for side, s in SIDES:
        eye = M.eyes[s]
        materials.setup_eye(eye, eyes_c[s], M.eye_radius, iris_img)
        g = eye.vertex_groups.new(name=f"{side}Eye")
        g.add(list(range(len(eye.data.vertices))), 1.0, "REPLACE")
        shade_smooth(eye)
        eye_objs.append(eye)
    bpy.ops.object.select_all(action="DESELECT")
    for e in eye_objs:
        e.select_set(True)
    bpy.context.view_layer.objects.active = eye_objs[0]
    bpy.ops.object.join()
    eyes = bpy.context.view_layer.objects.active
    eyes.name = "Eyes"
    dec = eyes.modifiers.new("Decimate", "DECIMATE")
    dec.ratio = 0.5
    bpy.ops.object.modifier_apply(modifier="Decimate")
    skin_to(eyes, armature)

    bv, bf = face.build_brows(fr, follow.bvh)
    brows = rigid_part("Brows", bv, bf, "Head", armature, "Brow")
    follow_shapes(brows, follow, list(expressions) + list(identity) + ["Frame_Feminine"], shapes)
    copy_weights(brows, follow, weights, only=["Head"])
    skin_to(brows, armature)

    lv, lf, roots = face.build_lashes(fr, P, normals)
    lashes = rigid_part("Lashes", lv, lf, "Head", armature, "Lash")
    lash_fields = {name: face.lash_deltas(fr, P, lv, roots, shapes[name])
                   for name in list(expressions) + list(identity)}
    lash_fields.update({k: face.SkinFollow.deltas(follow.bind(lv), shapes[k]) for k in ("Frame_Feminine",)})
    add_shapes(lashes, lv, lash_fields)
    skin_to(lashes, armature)

    teeth, teeth_size = fit_teeth(fr, P, lm)
    skin_to(teeth, armature)

    # Clothes and hair, following the body's shapes.
    garments = clothes.build_all(body_obj, P, normals, lm, joints, follow, weights, nbr)
    for g in garments.values():
        follow_shapes(g.obj, follow, list(body_fields), shapes)
    # Clearance, in the rest pose and every shape: a covered layer moves with
    # the garment over it, each garment stays outside the body (with its
    # masks on), and each layer stays under the one worn over it.
    for g in garments.values():
        fit.clear_body(g.obj, P, tris, shapes, g.masks)
    layers = [(garments[i].obj, garments[o].obj) for i, o in LAYERS if i in garments and o in garments]
    for inner, outer in layers:
        fit.follow_layer(inner, outer, P, tris)
    # A covered layer now moves with its outer garment, which may not be
    # worn: clear it off the body again before tucking it under.
    for g in garments.values():
        if any(g.obj is inner for inner, _ in layers):
            fit.clear_body(g.obj, P, tris, shapes, g.masks)
    for inner, outer in layers:
        fit.clear_layer(inner, outer, P, tris, shapes)
    for g in garments.values():
        copy_weights(g.obj, follow, weights)
        # Bridging cloth bends as one piece (no folding at the hips' crease).
        fit.smooth_weights(g.obj, iterations=6, only=HIP_BONES)
        rig.limit_weights(g.obj)
        skin_to(g.obj, armature)
    styles = hair.build_all(P, normals, lm, fr, joints, follow)
    for h in styles.values():
        follow_shapes(h, follow, ["Frame_Feminine", "Face_Head_Width", "Face_Head_Length"], shapes)
        copy_weights(h, follow, weights, only=["Head"])
        skin_to(h, armature)

    # The customization data: what each shape and slider moves.
    custom = {
        "version": 1,
        "units": "metres, glTF axes (Y up, front +Z)",
        "body": {
            "Frame_Feminine": {"bones": {b: {"offset": to_gltf(o["offset"])} for b, o in
                                         bodyshapes.joint_offsets(joints, joints_f).items()},
                               "eye_scale": round(F.eye_radius / M.eye_radius, 4)},
            "Build_Heavy": {}, "Build_Slim": {}, "Build_Muscular": {},
        },
        "face": {name: {"bones": {b: ({"offset": to_gltf(c["offset"])} if "offset" in c else c)
                                  for b, c in bones.items()}}
                 for name, (_, bones) in identity.items()},
        "masks": list(masks),
        "parts": {name: g.info() for name, g in garments.items()},
        "hair": sorted(styles),
        "default": {"outfit": DEFAULT_OUTFIT, "hair": DEFAULT_HAIR},
    }
    report = {"Body": tri_count(body_obj), "Eyes": tri_count(eyes), "Brows": tri_count(brows),
              "Lashes": tri_count(lashes), "Teeth": tri_count(teeth)}
    report.update({k: tri_count(g.obj) for k, g in garments.items()})
    report.update({k: tri_count(h) for k, h in styles.items()})
    parts = {"Body": body_obj, "Eyes": eyes, "Brows": brows, "Lashes": lashes, "Teeth": teeth}
    parts.update({k: g.obj for k, g in garments.items()})
    parts.update(styles)
    return armature, parts, garments, custom, report, masks


def covered_body(body_obj, masks, needed, keep_rings=2):
    """A copy of the body without the faces the outfit fully covers (its
    masks pull them all the way in), keeping a margin of `keep_rings`
    rings under each edge of the clothes. Shape keys come along."""
    import bmesh
    from bodyshapes import MASK_DEPTH
    n = len(body_obj.data.vertices)
    cover = [0.0] * n
    for name in needed:
        depth = MASK_DEPTH * (0.7 if name == "Mask_Feet" else 1.0)
        for i, d in masks[name].items():
            cover[i] = min(1.0, cover[i] + d.length / depth)
    inside = [c > 0.98 for c in cover]
    nbr = edge_neighbours(body_obj.data)
    for _ in range(keep_rings):
        inside = [inside[i] and all(inside[j] for j in nbr[i]) for i in range(n)]
    copy = body_obj.copy()
    copy.data = body_obj.data.copy()
    bpy.context.scene.collection.objects.link(copy)
    bm = bmesh.new()
    bm.from_mesh(copy.data)
    bm.verts.ensure_lookup_table()
    drop = [f for f in bm.faces if all(inside[vert.index] for vert in f.verts)]
    bmesh.ops.delete(bm, geom=drop, context="FACES")
    loose = [vert for vert in bm.verts if not vert.link_faces]
    bmesh.ops.delete(bm, geom=loose, context="VERTS")
    bm.to_mesh(copy.data)
    bm.free()
    copy.name = "Body"
    return copy


def export_glb(path, armature, objects):
    bpy.ops.object.select_all(action="DESELECT")
    armature.select_set(True)
    for o in objects:
        o.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_skins=True, export_morph=True, export_morph_normal=False,
        # Sparse shape keys: a face shape moves a few hundred of the body's
        # 12,500 vertices. The engine reads Blender's empty sparse accessors
        # (shapes that don't move a primitive) as no moves.
        export_try_sparse_sk=True,
        export_animations=False, export_yup=True, export_def_bones=False,
        export_leaf_bone=False, export_tangents=False,
        export_image_format="AUTO")


def export_animations(path, armature):
    """The starter animations, one glTF animation per clip, with the
    skeleton and no meshes."""
    import animations
    animations.build_actions(armature)
    bpy.ops.object.select_all(action="DESELECT")
    armature.select_set(True)
    bpy.context.view_layer.objects.active = armature
    bpy.ops.export_scene.gltf(
        filepath=path, export_format="GLB", use_selection=True,
        export_skins=True, export_morph=False, export_animations=True,
        export_animation_mode="ACTIONS", export_force_sampling=True,
        export_optimize_animation_size=True, export_anim_slide_to_zero=True,
        export_yup=True, export_def_bones=False, export_leaf_bone=False)


def main(out_dir):
    out_dir = os.path.abspath(out_dir)
    os.makedirs(out_dir, exist_ok=True)
    armature, parts, garments, custom, report, masks = build(out_dir)
    # The default character: the body without what its outfit hides, and
    # the masks its outfit needs resting at 1 (they pull the edges in).
    body_obj = parts["Body"]
    needed = set()
    for name in DEFAULT_OUTFIT:
        needed |= set(garments[name].masks)
    body_obj.name = "BodyFull"
    trimmed = covered_body(body_obj, masks, needed)
    skin_to(trimmed, armature)
    # It isn't customized (a customized character is built from the parts
    # file), so it keeps only its face and the masks it needs.
    keep = set(face.EXPRESSIONS) | needed
    default = [trimmed]
    names = [*FACE_PARTS, *DEFAULT_OUTFIT, DEFAULT_HAIR]
    for name in names:
        parts[name].name = name + "_Full"
        copy = parts[name].copy()
        copy.data = parts[name].data.copy()
        copy.name = name
        bpy.context.scene.collection.objects.link(copy)
        default.append(copy)
    for obj in default:
        obj.data.name = obj.name
    for obj in default:
        keys = obj.data.shape_keys
        if keys is None:
            continue
        for key in list(keys.key_blocks)[1:]:
            if key.name not in keep:
                obj.shape_key_remove(key)
        if len(obj.data.shape_keys.key_blocks) == 1:
            obj.shape_key_clear()
    for key in trimmed.data.shape_keys.key_blocks:
        key.value = 1.0 if key.name in needed else 0.0
    export_glb(os.path.join(out_dir, "Humanoid.glb"), armature, default)
    report["default character"] = sum(tri_count(o) for o in default)
    for obj in default:
        bpy.data.objects.remove(obj)
    for name in names:
        parts[name].name = name
    body_obj.name = "Body"
    for obj in parts.values():
        obj.data.name = obj.name
    export_glb(os.path.join(out_dir, "HumanoidParts.glb"), armature, list(parts.values()))
    export_animations(os.path.join(out_dir, "HumanoidAnimations.glb"), armature)
    with open(os.path.join(out_dir, "Humanoid.customization.json"), "w", newline="\n") as f:
        json.dump(custom, f, indent=1, sort_keys=True)
        f.write("\n")
    for name in ("Skin.png", "Iris.png"):
        path = os.path.join(out_dir, name)
        if os.path.exists(path):
            os.remove(path)
    return report


if __name__ == "__main__":
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else sys.argv[1:]
    out = argv[argv.index("--out") + 1] if "--out" in argv else os.path.dirname(os.path.abspath(__file__))
    rep = main(out)
    total = 0
    for k in sorted(rep):
        print(f"{k:24s} {rep[k]:6d}")
