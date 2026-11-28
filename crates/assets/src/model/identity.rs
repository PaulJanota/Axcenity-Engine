//! Derived models: a character's identity baked into a model's rest mesh
//! (Character Studio overhaul CO3, decision D4,
//! `docs/development/character-studio/architecture.md` section 7).
//!
//! A character's blend shapes are of two kinds. **Identity** shapes (the
//! frame, the build, a nose's width, a clothing mask) are set when the
//! character is made and then never move; **live** shapes (expressions,
//! correctives) move every frame. Morphing identity shapes every frame in
//! every pass is waste, so a character's parts are drawn from *derived*
//! models: the base model with an [`Identity`] baked in. Its identity
//! shapes are added into the vertices and dropped from the model's blend
//! shapes; the live ones stay. Characters with the same identity share
//! their derived models.
//!
//! An identity also moves bones (the feminine frame's joints are elsewhere;
//! wider-set eyes move the eye bones). The bake decides, per layer and per
//! model, what a bone change does to the surface:
//!
//! - a model that **has one of the layer's shapes** was already moved by
//!   it: nothing more moves at rest (the body under the feminine frame);
//! - a model that **has none of them** is carried by the bone (an eyeball
//!   has no frame shape: it moves, and scales, with its eye bone), and the
//!   bake moves its rest mesh there.
//!
//! Either way the derived model's bind is the changed skeleton's, so its
//! rest mesh is right as it is (a character nothing animates is drawn
//! from it), and every pose adds the bone changes after the clips
//! ([`Model::settle`], so clips play unchanged) and turns about the new
//! joints.
//!
//! A derived model is named by its base model's `MeshId` with the
//! identity's key after a second `#` ([`derived_mesh_id`]), so everything
//! that looks a MeshPart's model up by `MeshId` gets the derived one with
//! no code of its own. `Assets` keeps the identities registered with
//! [`Assets::derive`](crate::Assets::derive) and bakes when the model is
//! first asked for; an ID whose identity this run hasn't registered (a
//! saved world, before its description is applied again) is its base
//! model.

use std::sync::Arc;

use glam::{Mat4, Vec3};

use super::{Model, MorphTarget, Primitive, PrimitiveMorph, Rig, morph, new_key};

/// What makes one character's meshes its own: layers of shape weights and
/// bone changes, each layer one cause (one parameter of the character's
/// description, usually).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Identity {
    pub layers: Vec<IdentityLayer>,
}

/// One cause's shapes and the bone changes that go with them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IdentityLayer {
    /// Blend shapes by name and how far to lean toward each. A shape named
    /// here is an identity shape even at weight 0: it's baked and dropped.
    pub shapes: Vec<(String, f32)>,
    pub bones: Vec<BoneChange>,
}

/// A bone moved or scaled by an identity.
#[derive(Clone, Debug, PartialEq)]
pub struct BoneChange {
    pub bone: String,
    /// How far its head moves, in the model file's space (glTF axes).
    pub offset: [f32; 3],
    /// Its scale, and everything on it: 1 changes nothing.
    pub scale: f32,
}

/// What an identity adds to one bone of a pose, after the clips.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoneDelta {
    /// Added to the bone's translation (in its parent's frame).
    pub translation: Vec3,
    /// Multiplies the bone's scale.
    pub scale: f32,
}

impl BoneDelta {
    pub const NONE: Self = Self {
        translation: Vec3::ZERO,
        scale: 1.0,
    };
}

impl Identity {
    /// True when it changes nothing: no layer names a shape or a bone.
    pub fn is_empty(&self) -> bool {
        self.layers
            .iter()
            .all(|layer| layer.shapes.is_empty() && layer.bones.is_empty())
    }

    /// The same for the same identity on every machine and in every run
    /// (FNV-1a over its names and numbers), so a derived model's `MeshId`
    /// means the same thing wherever the same description is applied.
    pub fn key(&self) -> u64 {
        let mut hash = Fnv::default();
        for layer in &self.layers {
            hash.number(layer.shapes.len() as u32);
            for (name, weight) in &layer.shapes {
                hash.text(name);
                hash.number(clean(*weight).to_bits());
            }
            hash.number(layer.bones.len() as u32);
            for change in &layer.bones {
                hash.text(&change.bone);
                for value in change.offset {
                    hash.number(clean(value).to_bits());
                }
                hash.number(clean(change.scale).to_bits());
            }
        }
        hash.0
    }
}

/// -0 and 0 are the same number, and anything not finite is 0.
fn clean(value: f32) -> f32 {
    if value.is_finite() && value != 0.0 {
        value
    } else {
        0.0
    }
}

struct Fnv(u64);

impl Default for Fnv {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fnv {
    fn byte(&mut self, byte: u8) {
        self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }

    fn number(&mut self, value: u32) {
        for byte in value.to_le_bytes() {
            self.byte(byte);
        }
    }

    fn text(&mut self, text: &str) {
        self.number(text.len() as u32);
        for byte in text.bytes() {
            self.byte(byte);
        }
    }
}

/// What marks the identity part of a derived model's `MeshId`.
const TAG: char = 'i';

/// The `MeshId` of `base` (a model's `MeshId`, with or without a `#` pick)
/// with the identity `key` baked in. A base that's already derived is
/// derived from its own base.
pub fn derived_mesh_id(base: &str, key: u64) -> String {
    let (base, _) = split_derived(base);
    if base.contains('#') {
        format!("{base}#{TAG}{key:016x}")
    } else {
        format!("{base}##{TAG}{key:016x}")
    }
}

/// A `MeshId`'s base model and, for a derived one, its identity's key.
/// Object names never hold a `#` (they write it as `%23`), so a second one
/// can only start a tag. A model with hidden regions (`#h`, CO9,
/// [`hidden_mesh_id`](crate::hidden_mesh_id)) is a base of its own: a
/// derived model can be baked from it (`…#h<key>#i<key>`).
pub fn split_derived(mesh_id: &str) -> (&str, Option<u64>) {
    match last_tag(mesh_id) {
        None => (mesh_id, None),
        Some((_, tag, Some(_))) if tag == crate::hidden::TAG => (mesh_id, None),
        Some((base, tag, key)) if tag == TAG => (base, key),
        // A tag that isn't a key names the base.
        Some((base, _, _)) => (base, None),
    }
}

/// A `MeshId`'s last tag past its objects: (everything before it, its
/// letter, its key when it is one).
pub(crate) fn last_tag(mesh_id: &str) -> Option<(&str, char, Option<u64>)> {
    let (file, rest) = mesh_id.split_once('#')?;
    let (head, last) = rest.rsplit_once('#')?;
    let base = if head.is_empty() {
        file
    } else {
        &mesh_id[..file.len() + 1 + head.len()]
    };
    let mut chars = last.chars();
    let tag = chars.next().unwrap_or('\0');
    let hex = chars.as_str();
    let key = (hex.len() == 16)
        .then(|| u64::from_str_radix(hex, 16).ok())
        .flatten();
    Some((base, tag, key))
}

/// A `MeshId` without any of its tags (a derived model's identity, hidden
/// regions): the file and the objects it picks.
pub fn strip_tags(mesh_id: &str) -> &str {
    let mut mesh_id = mesh_id;
    while let Some((base, _, _)) = last_tag(mesh_id) {
        mesh_id = base;
    }
    mesh_id
}

impl Rig {
    /// What `changes` add to each bone of a pose: every bone's head moved
    /// by its offset in the file's space (a bone no change names stays
    /// where it is, whatever its parent does), and its scale.
    fn deltas<'a>(&self, changes: impl Iterator<Item = &'a BoneChange>) -> Vec<BoneDelta> {
        let mut offsets = vec![Vec3::ZERO; self.bones.len()];
        let mut scales = vec![1.0_f32; self.bones.len()];
        let mut any = false;
        for change in changes {
            let Some(bone) = self.bone(&change.bone) else {
                continue;
            };
            let offset = Vec3::from(change.offset);
            if offset.is_finite() {
                offsets[bone] += offset;
            }
            if change.scale.is_finite() && change.scale > 0.0 {
                scales[bone] *= change.scale;
            }
            any = true;
        }
        if !any {
            return Vec::new();
        }
        let rest = self.rest_pose();
        let world = self.world(&rest);
        let moved: Vec<Mat4> = world
            .iter()
            .zip(&offsets)
            .map(|(matrix, offset)| Mat4::from_translation(*offset) * *matrix)
            .collect();
        self.bones
            .iter()
            .enumerate()
            .map(|(index, bone)| {
                let local = match bone.parent {
                    Some(parent) => moved[parent].inverse() * moved[index],
                    None => moved[index],
                };
                let translation = local.transform_point3(Vec3::ZERO) - bone.rest.translation;
                BoneDelta {
                    translation: if translation.is_finite() {
                        translation
                    } else {
                        Vec3::ZERO
                    },
                    scale: scales[index],
                }
            })
            .collect()
    }
}

/// Adds `deltas` (one per bone; missing ones change nothing) to `pose`.
fn settle(deltas: &[BoneDelta], pose: &mut [super::BoneTransform]) {
    for (bone, delta) in pose.iter_mut().zip(deltas) {
        bone.translation += delta.translation;
        bone.scale *= delta.scale;
    }
}

/// An identity as one model wears it **live** (ADR-0041): what each pose
/// of the model's own mesh needs so that it is drawn where the model with
/// the identity baked in ([`Model::bake`]) would be, with no vertex copied.
/// Made by [`Model::live`]; small (a weight per identity shape, a matrix
/// per joint), so every character can have its own.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LiveIdentity {
    /// Each identity shape of the model (by its index in
    /// [`Model::morph_targets`]) and its weight. A shape the identity
    /// names is here even at 0, so it can't keep a weight of its own.
    weights: Vec<(usize, f32)>,
    /// What every pose adds to its bones after the clips ([`Self::settle`]).
    deltas: Vec<BoneDelta>,
    /// One matrix per joint of the model's rig, multiplied into each posed
    /// joint after the pose ([`Posed::rebind`](super::Posed::rebind)):
    /// the bind re-seated on the changed skeleton and the surface carried
    /// by the bones of the layers the model has no shape of. Empty when the
    /// identity moves no bone.
    binds: Vec<Mat4>,
}

impl LiveIdentity {
    /// True when wearing it changes nothing.
    pub fn is_empty(&self) -> bool {
        self.weights.is_empty() && self.deltas.is_empty() && self.binds.is_empty()
    }

    /// Adds the identity's bone changes to `pose` (one transform per bone
    /// of the model's rig), after the clips and before anything that reads
    /// where the bones are: [`Model::settle`] for a live identity.
    pub fn settle(&self, pose: &mut [super::BoneTransform]) {
        settle(&self.deltas, pose);
    }

    /// Whether [`settle`](Self::settle) changes a pose.
    pub fn settles(&self) -> bool {
        !self.deltas.is_empty()
    }

    /// The identity shapes' weights, by index in the model's
    /// [`Model::morph_targets`].
    pub fn weights(&self) -> &[(usize, f32)] {
        &self.weights
    }

    /// Puts the identity on `posed` (a pose of the model this was made for,
    /// its live shapes already set): its identity shapes at their weights,
    /// and each joint re-seated and carried. Call it last, after the face
    /// and the correctives have set their shapes.
    pub fn dress(&self, posed: &mut super::Posed) {
        if !self.weights.is_empty() {
            let mut weights = posed.morph_weights().to_vec();
            let mut changed = false;
            for &(index, weight) in &self.weights {
                if let Some(slot) = weights.get_mut(index)
                    && *slot != weight
                {
                    *slot = weight;
                    changed = true;
                }
            }
            if changed {
                posed.set_morph_weights(&weights);
            }
        }
        if !self.binds.is_empty() {
            posed.rebind(&self.binds);
        }
    }
}

/// What an identity does to one model, worked out once and shared by
/// [`Model::bake`] (which copies it into the vertices) and [`Model::live`]
/// (which applies it to each pose).
struct Prepared {
    /// Each of the model's shapes: how far the identity leans toward it.
    weights: Vec<f32>,
    /// Whether the identity names it (an identity shape, even at 0).
    named: Vec<bool>,
    /// The rig with each joint's bind re-seated on the changed skeleton
    /// (the model's own when no bone changes).
    rig: Option<Arc<Rig>>,
    /// What every pose adds to its bones after the clips.
    identity_pose: Vec<BoneDelta>,
    /// How each joint carries the surface of the layers this model has no
    /// shape of, in the fitted space its vertices are in; empty when
    /// nothing is carried.
    carry: Vec<Mat4>,
}

impl Model {
    /// How this model wears `identity` live (ADR-0041; see
    /// [`LiveIdentity`]): the same result as [`bake`](Self::bake), worked
    /// out on each pose instead of copied into the vertices.
    pub fn live(&self, identity: &Identity) -> LiveIdentity {
        let Prepared {
            weights,
            named,
            rig,
            identity_pose,
            carry,
        } = self.prepare(identity);
        let weights = named
            .iter()
            .enumerate()
            .filter(|(_, named)| **named)
            .map(|(index, _)| (index, weights[index]))
            .collect();
        // Each joint's matrix, as `Posed` makes it, is
        // `fit · world · inverse_bind · fit⁻¹`; the bake's is
        // `fit · world · inverse_bind' · fit⁻¹ · carry` (its bind re-seated,
        // its vertices carried first). So the bake's is the posed one times
        // `fit · inverse_bind⁻¹ · inverse_bind' · fit⁻¹ · carry`, which
        // doesn't depend on the pose: one matrix per joint.
        let mut binds = Vec::new();
        if let (Some(base), Some(seated)) = (&self.rig, &rig)
            && !identity_pose.is_empty()
        {
            let unfit = base.fit.inverse();
            binds = base
                .joints
                .iter()
                .zip(&seated.joints)
                .enumerate()
                .map(|(index, (joint, moved))| {
                    let carried = carry.get(index).copied().unwrap_or(Mat4::IDENTITY);
                    let bind = base.fit
                        * joint.inverse_bind.inverse()
                        * moved.inverse_bind
                        * unfit
                        * carried;
                    if bind.is_finite() {
                        bind
                    } else {
                        Mat4::IDENTITY
                    }
                })
                .collect();
        }
        LiveIdentity {
            weights,
            deltas: identity_pose,
            binds,
        }
    }

    /// What `identity` does to this model (see [`Prepared`]).
    fn prepare(&self, identity: &Identity) -> Prepared {
        let has = |name: &str| self.morph_targets.iter().any(|target| target.name == name);
        let index_of = |name: &str| {
            self.morph_targets
                .iter()
                .position(|target| target.name == name)
        };
        // Each of this model's shapes: how far the identity leans toward
        // it, and whether it's named (an identity shape) or stays live.
        let mut weights = vec![0.0_f32; self.morph_targets.len()];
        let mut named = vec![false; self.morph_targets.len()];
        for (name, weight) in identity.layers.iter().flat_map(|layer| &layer.shapes) {
            if let Some(index) = index_of(name) {
                named[index] = true;
                if weight.is_finite() {
                    weights[index] += weight;
                }
            }
        }
        // The skeleton's part: what every pose adds (`total`), each
        // joint's new bind, and how each joint carries the surface of the
        // layers this model has no shape of (`carry`, in the fitted space
        // the vertices are in; empty when nothing is carried).
        let mut rig = self.rig.clone();
        let mut identity_pose = Vec::new();
        let mut carry: Vec<Mat4> = Vec::new();
        if let Some(base) = &self.rig {
            let all = identity.layers.iter().flat_map(|layer| &layer.bones);
            let total = base.deltas(all);
            if !total.is_empty() {
                let carried = identity
                    .layers
                    .iter()
                    .filter(|layer| !layer.shapes.iter().any(|(name, _)| has(name)))
                    .flat_map(|layer| &layer.bones);
                let carried = base.deltas(carried);
                let rest = base.world(&base.rest_pose());
                let mut settled = base.rest_pose();
                settle(&total, &mut settled);
                let settled = base.world(&settled);
                let joints = base
                    .joints
                    .iter()
                    .map(|joint| {
                        let reseat = match (settled.get(joint.bone), rest.get(joint.bone)) {
                            (Some(settled), Some(rest)) => settled.inverse() * *rest,
                            _ => Mat4::IDENTITY,
                        };
                        super::Joint {
                            bone: joint.bone,
                            inverse_bind: if reseat.is_finite() {
                                reseat * joint.inverse_bind
                            } else {
                                joint.inverse_bind
                            },
                        }
                    })
                    .collect();
                if !carried.is_empty() {
                    let mut pose = base.rest_pose();
                    settle(&carried, &mut pose);
                    let moved = base.world(&pose);
                    let unfit = base.fit.inverse();
                    carry = base
                        .joints
                        .iter()
                        .map(|joint| {
                            let matrix = match (moved.get(joint.bone), rest.get(joint.bone)) {
                                (Some(moved), Some(rest)) => {
                                    base.fit * *moved * rest.inverse() * unfit
                                }
                                _ => Mat4::IDENTITY,
                            };
                            if matrix.is_finite() {
                                matrix
                            } else {
                                Mat4::IDENTITY
                            }
                        })
                        .collect();
                }
                rig = Some(Arc::new(Rig {
                    bones: base.bones.clone(),
                    joints,
                    fit: base.fit,
                }));
                identity_pose = total;
            }
        }
        Prepared {
            weights,
            named,
            rig,
            identity_pose,
            carry,
        }
    }

    /// Adds this model's identity to `pose` (one transform per bone of its
    /// rig), after the clips and before anything that reads where the
    /// bones are. A model that isn't derived, or whose identity moves no
    /// bone, changes nothing.
    pub fn settle(&self, pose: &mut [super::BoneTransform]) {
        settle(&self.identity_pose, pose);
    }

    /// Whether [`settle`](Self::settle) changes a pose.
    pub fn settles(&self) -> bool {
        !self.identity_pose.is_empty()
    }

    /// This model with `identity` baked in (see the module docs): its
    /// identity shapes added into its vertices and dropped from its blend
    /// shapes, its surface carried by the bones of the layers whose shapes
    /// it hasn't got, and its bind moved to the changed skeleton. Its key
    /// is new; its textures and anything else the identity doesn't touch
    /// are shared with this model.
    pub fn bake(&self, identity: &Identity) -> Model {
        let Prepared {
            weights,
            named: baked,
            rig,
            identity_pose,
            carry,
        } = self.prepare(identity);
        // The live shapes keep their order; `renumber` is each one's new
        // place.
        let mut renumber = vec![None; self.morph_targets.len()];
        let mut morph_targets: Vec<MorphTarget> = Vec::new();
        for (index, target) in self.morph_targets.iter().enumerate() {
            if !baked[index] {
                renumber[index] = u16::try_from(morph_targets.len()).ok();
                morph_targets.push(target.clone());
            }
        }
        let bake_primitive = |primitive: &Primitive, key: u64| -> Primitive {
            let mut positions = primitive.positions.clone();
            let mut normals = primitive.normals.clone();
            let identity_morphs: Vec<PrimitiveMorph> = primitive
                .morphs
                .iter()
                .filter(|morph| baked.get(usize::from(morph.target)) == Some(&true))
                .cloned()
                .collect();
            morph::apply(&identity_morphs, &weights, &mut positions, &mut normals);
            let mut morphs: Vec<PrimitiveMorph> = primitive
                .morphs
                .iter()
                .filter_map(|morph| {
                    let target = (*renumber.get(usize::from(morph.target))?)?;
                    Some(PrimitiveMorph {
                        target,
                        ..morph.clone()
                    })
                })
                .collect();
            let mut tangents = primitive.tangents.clone();
            if !carry.is_empty() {
                // Each corner's carry: its joints', by weight.
                let count = primitive.joints.len().min(primitive.weights.len());
                let carried: Vec<Option<Mat4>> = (0..positions.len())
                    .map(|corner| {
                        if corner >= count {
                            return None;
                        }
                        let mut matrix = Mat4::ZERO;
                        let mut total = 0.0;
                        for (&joint, &weight) in primitive.joints[corner]
                            .iter()
                            .zip(&primitive.weights[corner])
                        {
                            if weight > 0.0
                                && let Some(joint) = carry.get(usize::from(joint))
                            {
                                matrix += *joint * weight;
                                total += weight;
                            }
                        }
                        (total > 1e-6).then(|| matrix * (1.0 / total))
                    })
                    .collect();
                let turn =
                    |matrix: &Mat4, vector: [f32; 3]| matrix.transform_vector3(Vec3::from(vector));
                for (corner, matrix) in carried.iter().enumerate() {
                    let Some(matrix) = matrix else { continue };
                    positions[corner] = matrix
                        .transform_point3(Vec3::from(positions[corner]))
                        .to_array();
                    if let Some(normal) = normals.get_mut(corner) {
                        *normal = turn(matrix, *normal).normalize_or_zero().to_array();
                    }
                    if let Some(tangent) = tangents.get_mut(corner) {
                        let turned =
                            turn(matrix, [tangent[0], tangent[1], tangent[2]]).normalize_or_zero();
                        *tangent = [turned.x, turned.y, turned.z, tangent[3]];
                    }
                }
                // A live shape's moves turn and scale with their corners.
                for morph in &mut morphs {
                    for (index, &corner) in morph.corners.iter().enumerate() {
                        let Some(Some(matrix)) = carried.get(corner as usize) else {
                            continue;
                        };
                        if let Some(delta) = morph.positions.get_mut(index) {
                            *delta = turn(matrix, *delta).to_array();
                        }
                    }
                }
            }
            Primitive {
                key,
                positions,
                normals,
                uvs: primitive.uvs.clone(),
                tangents,
                regions: primitive.regions.clone(),
                skin_uvs: primitive.skin_uvs.clone(),
                colors: primitive.colors.clone(),
                indices: primitive.indices.clone(),
                material: primitive.material.clone(),
                joints: primitive.joints.clone(),
                weights: primitive.weights.clone(),
                morphs,
                lods: Vec::new(),
                level: primitive.level,
            }
        };
        let primitives = self
            .primitives
            .iter()
            .map(|primitive| {
                let mut made = bake_primitive(primitive, new_key());
                made.lods = primitive
                    .lods
                    .iter()
                    .map(|lod| bake_primitive(lod, new_key()))
                    .collect();
                made
            })
            .collect();

        Model {
            key: new_key(),
            size: self.size,
            center: self.center,
            primitives,
            hull_points: self.hull_points.clone(),
            collision: self.collision.clone(),
            // A stand-in has no identity: it isn't drawn.
            proxy: self.proxy.iter().map(Primitive::shared).collect(),
            warnings: Vec::new(),
            rig,
            morph_targets,
            identity_pose,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Bone, BoneTransform, Joint, ModelMaterial};
    use super::*;
    use glam::Quat;

    /// A two-bone arm along +X (`Root` at the origin, `Tip` one unit out)
    /// with one triangle weighted to `Tip`, and the shapes `Wide` (moves
    /// corner 0 by +Y) and `Blink` (moves corner 1 by +Z).
    fn arm() -> Model {
        let mut primitive = Primitive::new(
            vec![[1.0, 0.0, 0.0], [2.0, 0.0, 0.0], [1.0, 1.0, 0.0]],
            vec![[0.0, 0.0, 1.0]; 3],
            Vec::new(),
            vec![0, 1, 2],
            ModelMaterial::default(),
        );
        primitive.joints = vec![[1, 0, 0, 0]; 3];
        primitive.weights = vec![[1.0, 0.0, 0.0, 0.0]; 3];
        primitive.morphs = vec![
            PrimitiveMorph {
                target: 0,
                corners: vec![0],
                positions: vec![[0.0, 0.5, 0.0]],
                normals: vec![[0.0; 3]],
            },
            PrimitiveMorph {
                target: 1,
                corners: vec![1],
                positions: vec![[0.0, 0.0, 0.25]],
                normals: vec![[0.0; 3]],
            },
        ];
        let bone = |name: &str, parent, x| Bone {
            name: name.to_owned(),
            parent,
            rest: BoneTransform {
                translation: Vec3::new(x, 0.0, 0.0),
                ..BoneTransform::IDENTITY
            },
        };
        let rig = Rig {
            bones: vec![bone("Root", None, 0.0), bone("Tip", Some(0), 1.0)],
            joints: vec![
                Joint {
                    bone: 0,
                    inverse_bind: Mat4::IDENTITY,
                },
                Joint {
                    bone: 1,
                    inverse_bind: Mat4::from_translation(Vec3::new(-1.0, 0.0, 0.0)),
                },
            ],
            fit: Mat4::IDENTITY,
        };
        Model {
            key: new_key(),
            size: [1.0; 3],
            center: [0.0; 3],
            primitives: vec![primitive],
            hull_points: Vec::new(),
            collision: Vec::new(),
            proxy: Vec::new(),
            warnings: Vec::new(),
            rig: Some(Arc::new(rig)),
            morph_targets: vec![
                MorphTarget {
                    name: "Wide".to_owned(),
                    default_weight: 0.0,
                },
                MorphTarget {
                    name: "Blink".to_owned(),
                    default_weight: 0.0,
                },
            ],
            identity_pose: Vec::new(),
        }
    }

    fn layer(shapes: &[(&str, f32)], bones: &[(&str, [f32; 3], f32)]) -> IdentityLayer {
        IdentityLayer {
            shapes: shapes
                .iter()
                .map(|(name, weight)| ((*name).to_owned(), *weight))
                .collect(),
            bones: bones
                .iter()
                .map(|(bone, offset, scale)| BoneChange {
                    bone: (*bone).to_owned(),
                    offset: *offset,
                    scale: *scale,
                })
                .collect(),
        }
    }

    fn corners(model: &Model, pose: &[BoneTransform]) -> Vec<Vec3> {
        let posed = model.pose(pose).expect("it has a rig");
        posed
            .primitive(model, 0, 0)
            .positions
            .iter()
            .map(|corner| Vec3::from(*corner))
            .collect()
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-5
    }

    #[test]
    fn identity_shapes_are_baked_in_and_live_shapes_stay() {
        let base = arm();
        let identity = Identity {
            layers: vec![layer(&[("Wide", 0.5)], &[])],
        };
        let derived = base.bake(&identity);
        assert_ne!(derived.key(), base.key());
        assert_eq!(derived.primitives[0].positions[0], [1.0, 0.25, 0.0]);
        let names: Vec<&str> = derived
            .morph_targets
            .iter()
            .map(|target| target.name.as_str())
            .collect();
        assert_eq!(names, ["Blink"], "the baked shape is gone");
        // The live shape still moves its corner, under its new number.
        assert_eq!(derived.primitives[0].morphs.len(), 1);
        assert_eq!(derived.primitives[0].morphs[0].target, 0);
        let mut posed = derived.rest_posed();
        posed.set_morph_weights(&[1.0]);
        assert_eq!(
            posed.primitive(&derived, 0, 0).positions[1],
            [2.0, 0.0, 0.25]
        );
    }

    #[test]
    fn a_shape_named_at_zero_is_still_an_identity_shape() {
        let derived = arm().bake(&Identity {
            layers: vec![layer(&[("Wide", 0.0)], &[])],
        });
        assert_eq!(derived.primitives[0].positions[0], [1.0, 0.0, 0.0]);
        assert_eq!(derived.morph_targets.len(), 1);
    }

    #[test]
    fn a_model_with_the_layers_shape_keeps_its_surface_and_turns_about_the_new_joint() {
        let base = arm();
        // The shape's layer moves the tip joint half a unit along the arm.
        let derived = base.bake(&Identity {
            layers: vec![layer(&[("Wide", 0.0)], &[("Tip", [0.5, 0.0, 0.0], 1.0)])],
        });
        assert!(derived.settles());
        let rig = derived.rig.clone().unwrap();
        let mut rest = rig.rest_pose();
        derived.settle(&mut rest);
        assert!(close(rest[1].translation, Vec3::new(1.5, 0.0, 0.0)));
        // At rest nothing moved: the shape already put the surface there.
        let at_rest = corners(&derived, &rest);
        assert!(close(at_rest[0], Vec3::new(1.0, 0.0, 0.0)));
        assert!(close(at_rest[1], Vec3::new(2.0, 0.0, 0.0)));
        // Turned a quarter about Z, the corners go round the moved joint
        // (1.5, 0, 0), not the old one.
        rest[1].rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let turned = corners(&derived, &rest);
        assert!(close(turned[1], Vec3::new(1.5, 0.5, 0.0)), "{turned:?}");
    }

    #[test]
    fn a_model_without_the_layers_shape_is_carried_by_the_bone() {
        let base = arm();
        // `Frame` isn't one of the arm's shapes: an eyeball under a frame.
        let derived = base.bake(&Identity {
            layers: vec![layer(&[("Frame", 1.0)], &[("Tip", [0.0, 0.25, 0.0], 2.0)])],
        });
        let rig = derived.rig.clone().unwrap();
        let mut rest = rig.rest_pose();
        derived.settle(&mut rest);
        let at_rest = corners(&derived, &rest);
        // Moved up by the offset and scaled about the bone's head (1, 0, 0).
        assert!(close(at_rest[0], Vec3::new(1.0, 0.25, 0.0)), "{at_rest:?}");
        assert!(close(at_rest[1], Vec3::new(3.0, 0.25, 0.0)), "{at_rest:?}");
        // Its rest mesh is there already, for a character nothing poses,
        // and its live shape's move grew with it.
        assert_eq!(derived.primitives[0].positions[1], [3.0, 0.25, 0.0]);
        let blink = &derived.primitives[0].morphs[1];
        assert_eq!(blink.positions[0], [0.0, 0.0, 0.5]);
    }

    #[test]
    fn a_bone_no_change_names_stays_where_it_is() {
        let base = arm();
        let derived = base.bake(&Identity {
            layers: vec![layer(&[], &[("Root", [0.0, 1.0, 0.0], 1.0)])],
        });
        let rig = derived.rig.clone().unwrap();
        let mut rest = rig.rest_pose();
        derived.settle(&mut rest);
        let world = rig.world(&rest);
        assert!(close(
            world[0].transform_point3(Vec3::ZERO),
            Vec3::new(0.0, 1.0, 0.0)
        ));
        assert!(close(
            world[1].transform_point3(Vec3::ZERO),
            Vec3::new(1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn an_empty_identity_changes_nothing_but_the_key() {
        let base = arm();
        let derived = base.bake(&Identity::default());
        assert!(!derived.settles());
        assert_eq!(
            derived.primitives[0].positions,
            base.primitives[0].positions
        );
        assert_eq!(derived.morph_targets, base.morph_targets);
        assert!(Arc::ptr_eq(
            derived.rig.as_ref().unwrap(),
            base.rig.as_ref().unwrap()
        ));
    }

    /// `model` posed with `identity` worn live (ADR-0041): its own mesh,
    /// the identity's weights and joint corrections on the pose, `blink`
    /// on the live `Blink` shape.
    fn live_corners(model: &Model, identity: &Identity, pose: &[BoneTransform]) -> Vec<Vec3> {
        let live = model.live(identity);
        let mut pose = pose.to_vec();
        live.settle(&mut pose);
        let mut posed = model.pose(&pose).expect("it has a rig");
        lean(model, &mut posed, "Blink", 1.0);
        live.dress(&mut posed);
        assert_eq!(posed.model_key, model.key(), "still the model's own mesh");
        posed
            .primitive(model, 0, 0)
            .positions
            .iter()
            .map(|corner| Vec3::from(*corner))
            .collect()
    }

    /// The same, from the model with `identity` baked in.
    fn baked_corners(model: &Model, identity: &Identity, pose: &[BoneTransform]) -> Vec<Vec3> {
        let baked = model.bake(identity);
        let mut pose = pose.to_vec();
        baked.settle(&mut pose);
        let mut posed = baked.pose(&pose).expect("it has a rig");
        lean(&baked, &mut posed, "Blink", 1.0);
        posed
            .primitive(&baked, 0, 0)
            .positions
            .iter()
            .map(|corner| Vec3::from(*corner))
            .collect()
    }

    fn lean(model: &Model, posed: &mut super::super::Posed, name: &str, weight: f32) {
        let mut weights = posed.morph_weights().to_vec();
        if let Some(index) = model.morph_targets.iter().position(|t| t.name == name) {
            weights[index] = weight;
        }
        posed.set_morph_weights(&weights);
    }

    /// ADR-0041: an identity worn live (the model's own mesh, the
    /// identity's shape weights and joint corrections set on each pose)
    /// puts every corner where the bake puts it, at rest and turned: the
    /// shapes alone, a joint re-seated under a shape, a surface carried
    /// and scaled by its bone, and both layers at once.
    #[test]
    fn an_identity_worn_live_poses_the_mesh_where_its_bake_does() {
        let base = arm();
        let identities = [
            Identity {
                layers: vec![layer(&[("Wide", 0.5)], &[])],
            },
            Identity {
                layers: vec![layer(&[("Wide", 0.3)], &[("Tip", [0.5, 0.0, 0.0], 1.0)])],
            },
            Identity {
                layers: vec![layer(&[("Frame", 1.0)], &[("Tip", [0.0, 0.25, 0.0], 2.0)])],
            },
            Identity {
                layers: vec![
                    layer(&[("Wide", 0.7)], &[("Tip", [0.1, 0.0, 0.0], 1.0)]),
                    layer(&[("Frame", 1.0)], &[("Tip", [0.0, 0.2, 0.0], 1.5)]),
                ],
            },
        ];
        let rig = base.rig.clone().unwrap();
        let rest = rig.rest_pose();
        let mut turned = rest.clone();
        turned[1].rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        turned[0].rotation = Quat::from_rotation_y(0.4);
        for (index, identity) in identities.iter().enumerate() {
            for pose in [&rest, &turned] {
                let live = live_corners(&base, identity, pose);
                let baked = baked_corners(&base, identity, pose);
                for (a, b) in live.iter().zip(&baked) {
                    assert!(close(*a, *b), "identity {index}: {live:?} vs {baked:?}");
                }
            }
        }
        // Nothing to wear changes nothing.
        let none = base.live(&Identity::default());
        assert!(none.is_empty() && !none.settles());
    }

    #[test]
    fn the_key_is_the_same_for_the_same_identity_and_differs_otherwise() {
        let one = Identity {
            layers: vec![layer(&[("Wide", 0.5)], &[("Tip", [0.0, 0.1, 0.0], 1.0)])],
        };
        let same = one.clone();
        let mut other = one.clone();
        other.layers[0].shapes[0].1 = 0.6;
        assert_eq!(one.key(), same.key());
        assert_ne!(one.key(), other.key());
        // Pinned: the key is part of saved and replicated MeshIds.
        assert_eq!(Identity::default().key(), 0xcbf2_9ce4_8422_2325);
        // -0 is 0.
        let mut negative = one.clone();
        negative.layers[0].bones[0].offset[0] = -0.0;
        assert_eq!(one.key(), negative.key());
    }

    #[test]
    fn derived_mesh_ids_split_back_into_their_base_and_key() {
        let whole = derived_mesh_id("assets/a.glb", 0xab);
        assert_eq!(whole, "assets/a.glb##i00000000000000ab");
        assert_eq!(split_derived(&whole), ("assets/a.glb", Some(0xab)));
        let picked = derived_mesh_id("assets/a.glb#Rig/Body", 7);
        assert_eq!(picked, "assets/a.glb#Rig/Body#i0000000000000007");
        assert_eq!(split_derived(&picked), ("assets/a.glb#Rig/Body", Some(7)));
        assert_eq!(split_derived("assets/a.glb"), ("assets/a.glb", None));
        assert_eq!(
            split_derived("assets/a.glb#Rig/Body"),
            ("assets/a.glb#Rig/Body", None)
        );
        // Deriving a derived model starts from its base again.
        assert_eq!(
            derived_mesh_id(&picked, 9),
            "assets/a.glb#Rig/Body#i0000000000000009"
        );
        // A tag that isn't a key names the base.
        assert_eq!(split_derived("a.glb#B#nonsense"), ("a.glb#B", None));
        // A model with hidden regions is a base of its own, and a derived
        // model can be baked from it.
        let hidden = "a.glb#B#h00000000000000ff";
        assert_eq!(split_derived(hidden), (hidden, None));
        let baked = derived_mesh_id(hidden, 3);
        assert_eq!(baked, "a.glb#B#h00000000000000ff#i0000000000000003");
        assert_eq!(split_derived(&baked), (hidden, Some(3)));
        assert_eq!(strip_tags(&baked), "a.glb#B");
        assert_eq!(strip_tags("a.glb##h00000000000000ff"), "a.glb");
        assert_eq!(strip_tags("a.glb#B"), "a.glb#B");
    }
}
