//! Trees generated from a recipe (foliage plan T1,
//! `docs/research/foliage-grass-seasons.md`): `.axtree` files hold a
//! hierarchy of generators (trunk, branch levels, mesh detail, bark) that
//! [`generate`] grows into limbs ([`skeleton`]) and meshes as continuous
//! tubes ([`mesher`]), giving an ordinary [`Model`] every other system takes
//! as it takes an imported one.
//!
//! Written from published ideas only (stacked growth functions, the pipe
//! model of limb radii, rotation-minimizing tube frames): no code of any
//! other generator (Mtree is GPL-3.0; SpeedTree is proprietary).
//!
//! Deterministic: the same recipe and variant give the same mesh on every
//! machine ([`TREE_GENERATOR_VERSION`] changes when the output does).

pub mod mesher;
pub mod recipe;
pub mod skeleton;

#[cfg(test)]
mod tests;

use axcenity_assets::{Model, ModelError, ModelMaterial, Primitive};
use glam::Vec3;

pub use mesher::{BarkLevel, TreeMesh};
pub use recipe::{Bark, BranchLevel, Curve, LodDetail, MeshDetail, Preset, TreeRecipe, Trunk};
pub use skeleton::{Skeleton, Stem};

/// Bump when the same recipe and variant would give a different mesh, so
/// anything kept from an older one (a cooked model) is made again.
pub const TREE_GENERATOR_VERSION: u32 = 2;

/// The sway a generated tree's bark is given ([`ModelMaterial::sway`]): a
/// built-in tree's (`scene/src/foliage.rs`).
pub const TREE_SWAY: f32 = 0.2;

/// A grown and meshed tree.
#[derive(Clone, Debug)]
pub struct Tree {
    pub skeleton: Skeleton,
    /// The bark, most detailed first, then each simpler copy
    /// ([`MeshDetail::lods`]), each within its triangle budget.
    pub levels: Vec<BarkLevel>,
    /// Where leaves will grow (foliage plan T3): the outer half of every
    /// limb of the last level, one place per segment.
    pub leaf_anchors: Vec<LeafAnchor>,
}

/// A place on a limb for leaves: where, which way the limb runs, and how
/// thick it is there.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeafAnchor {
    pub position: Vec3,
    pub direction: Vec3,
    pub radius: f32,
}

/// Tree number `variant` of `recipe`.
pub fn generate(recipe: &TreeRecipe, variant: u32) -> Tree {
    let skeleton = skeleton::grow(recipe, variant);
    let levels = mesher::bark_levels(recipe, &skeleton);
    let leaf_anchors = leaf_anchors(&skeleton);
    Tree {
        skeleton,
        levels,
        leaf_anchors,
    }
}

fn leaf_anchors(skeleton: &Skeleton) -> Vec<LeafAnchor> {
    let deepest = skeleton.stems.iter().map(|s| s.level).max().unwrap_or(0);
    let mut anchors = Vec::new();
    for stem in skeleton.stems.iter().filter(|s| s.level == deepest) {
        let length = stem.length().max(1e-6);
        for k in 0..stem.points.len() {
            if stem.along[k] / length >= 0.5 {
                anchors.push(LeafAnchor {
                    position: stem.points[k],
                    direction: stem.direction(k),
                    radius: stem.radii[k],
                });
            }
        }
    }
    anchors
}

impl Tree {
    /// The most detailed bark.
    pub fn full(&self) -> &BarkLevel {
        &self.levels[0]
    }

    /// The tree as a model: the trunk's base (named "TrunkBase", when the
    /// recipe gives it one) and the rest of its bark ("Bark") as two
    /// primitives, each with the simpler copies as its levels of detail.
    /// Both sway alike, as a tree does, so where they meet never opens.
    /// Its studs are the model's: a MeshPart sized to [`Model::size`]
    /// draws it at the size it was grown.
    pub fn to_model(&self, recipe: &TreeRecipe) -> Result<Model, ModelError> {
        let bark = &recipe.bark;
        let material = |name: &str| ModelMaterial {
            name: name.to_owned(),
            base_color: [bark.color[0], bark.color[1], bark.color[2], 1.0],
            roughness: bark.roughness.clamp(0.0, 1.0),
            metalness: 0.0,
            sway: TREE_SWAY,
            ..ModelMaterial::default()
        };
        let primitive = |mesh: &TreeMesh, name: &str| {
            Primitive::new(
                mesh.positions.clone(),
                mesh.normals.clone(),
                mesh.uvs.clone(),
                mesh.indices.clone(),
                material(name),
            )
        };
        let has_base = !self.full().base.indices.is_empty();
        let parts = 1 + usize::from(has_base);
        // Every copy is fitted with the full model, so all are placed
        // alike, then handed to their primitive.
        let mut primitives = Vec::new();
        for level in &self.levels {
            if has_base {
                primitives.push(primitive(&level.base, "TrunkBase"));
            }
            primitives.push(primitive(&level.bark, "Bark"));
        }
        let mut model = Model::from_primitives(primitives)?;
        let copies: Vec<Primitive> = model.primitives.drain(parts..).collect();
        for (index, copy) in copies.into_iter().enumerate() {
            model.primitives[index % parts].lods.push(copy);
        }
        Ok(model)
    }

    /// The most detailed copy's triangles.
    pub fn triangles(&self) -> usize {
        self.full().triangles()
    }
}
