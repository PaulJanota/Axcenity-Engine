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

pub mod leaves;
pub mod mesher;
pub mod recipe;
pub mod skeleton;

#[cfg(test)]
mod tests;

use axcenity_assets::{Model, ModelError, ModelMaterial, Primitive};
use glam::Vec3;

pub use leaves::Crown;
pub use mesher::{TreeLevel, TreeMesh};
pub use recipe::{
    Bark, BranchLevel, Curve, Leaves, LodDetail, MeshDetail, Preset, TreeRecipe, Trunk,
};
pub use skeleton::{Skeleton, Stem};

/// Bump when the same recipe and variant would give a different mesh, so
/// anything kept from an older one (a cooked model) is made again.
pub const TREE_GENERATOR_VERSION: u32 = 3;

/// The sway a generated tree's bark is given ([`ModelMaterial::sway`]): a
/// built-in tree's (`scene/src/foliage.rs`).
pub const TREE_SWAY: f32 = 0.2;

/// A grown and meshed tree.
#[derive(Clone, Debug)]
pub struct Tree {
    pub skeleton: Skeleton,
    /// The tree most detailed first, then each simpler copy
    /// ([`MeshDetail::lods`]), each within its triangle budget.
    pub levels: Vec<TreeLevel>,
    /// Where leaves grow: the outer two thirds of every limb of the last
    /// level and the tips of the level before, one place per ring.
    pub leaf_anchors: Vec<LeafAnchor>,
    /// The crown's box, around every place leaves grow.
    pub crown: Crown,
    /// How many leaf clusters the most detailed copy has. Each has the
    /// same triangles, in an order spread evenly over the crown, so the
    /// first `n` clusters' triangles are a thinner crown (a season's
    /// leaf drop).
    pub leaf_clusters: usize,
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
    let mut levels = mesher::bark_levels(recipe, &skeleton);
    let leaf_anchors = leaf_anchors(&skeleton);
    let (clusters, crown) = leaves::clusters(recipe, &leaf_anchors);
    for (index, level) in levels.iter_mut().enumerate() {
        let (budget, keep) = match index {
            0 => (recipe.mesh.triangles, 1.0),
            _ => recipe
                .mesh
                .lods
                .get(index - 1)
                .map_or((0, 0.0), |lod| (lod.triangles, lod.leaves)),
        };
        let room = (budget as usize).saturating_sub(level.triangles());
        level.leaves = leaves::mesh(recipe, &clusters, &crown, keep, index == 0, room);
    }
    let leaf_clusters = levels[0].leaves.triangles()
        / (recipe.leaves.per_cluster.max(1) as usize * leaves::leaf_triangles(true));
    Tree {
        skeleton,
        levels,
        leaf_anchors,
        crown,
        leaf_clusters,
    }
}

fn leaf_anchors(skeleton: &Skeleton) -> Vec<LeafAnchor> {
    let deepest = skeleton.stems.iter().map(|s| s.level).max().unwrap_or(0);
    let mut anchors = Vec::new();
    // The finest limbs from a third of the way out, and the tips of the
    // limbs they grow on, so the crown fills in between the twigs.
    let from = |level: usize| {
        if level == deepest {
            Some(0.35)
        } else if level + 1 == deepest && level > 0 {
            Some(0.7)
        } else {
            None
        }
    };
    for stem in &skeleton.stems {
        let Some(from) = from(stem.level) else {
            continue;
        };
        let length = stem.length().max(1e-6);
        for k in 0..stem.points.len() {
            if stem.along[k] / length >= from {
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
    pub fn full(&self) -> &TreeLevel {
        &self.levels[0]
    }

    /// The tree as a model: the trunk's base (named "TrunkBase", when the
    /// recipe gives it one), the rest of its bark ("Bark") and its leaves
    /// ("Leaves", two-sided, letting light through and fluttering), each
    /// with the simpler copies as its levels of detail. All sway alike, as
    /// a tree does, so where the base meets the trunk never opens.
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
        let leaf = &recipe.leaves;
        let leaf_material = ModelMaterial {
            name: "Leaves".to_owned(),
            base_color: [leaf.color[0], leaf.color[1], leaf.color[2], 1.0],
            roughness: leaf.roughness.clamp(0.0, 1.0),
            metalness: 0.0,
            double_sided: true,
            translucency: leaf.translucency.max(0.0),
            // Below 1, the most a material may flutter (assets' MAX_FLUTTER).
            flutter: leaf.flutter.clamp(0.0, 0.95),
            sway: TREE_SWAY,
            ..ModelMaterial::default()
        };
        let primitive = |mesh: &TreeMesh, material: ModelMaterial| {
            Primitive::new(
                mesh.positions.clone(),
                mesh.normals.clone(),
                mesh.uvs.clone(),
                mesh.indices.clone(),
                material,
            )
        };
        let full = self.full();
        let has_base = !full.base.indices.is_empty();
        let has_leaves = !full.leaves.indices.is_empty();
        let parts = 1 + usize::from(has_base) + usize::from(has_leaves);
        // Every copy is fitted with the full model, so all are placed
        // alike, then handed to their primitive. A copy whose leaves were
        // all left out keeps its last ones (a primitive's copies only
        // ever get simpler).
        let mut primitives = Vec::new();
        let mut leaves = &full.leaves;
        for level in &self.levels {
            if has_base {
                primitives.push(primitive(&level.base, material("TrunkBase")));
            }
            primitives.push(primitive(&level.bark, material("Bark")));
            if has_leaves {
                if !level.leaves.indices.is_empty() {
                    leaves = &level.leaves;
                }
                primitives.push(primitive(leaves, leaf_material.clone()));
            }
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
