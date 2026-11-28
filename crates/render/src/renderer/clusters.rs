//! What a group of items is, worked out once: [`Clusters`].
//!
//! A draw list hands the renderer its still items in groups
//! (`DrawList::chunks`: a terrain's painted plants by square of the
//! ground, shared between frames as one `Arc<[DrawItem]>` each). Until
//! 2026-11 every pass that needed something of an item worked it out again
//! each frame, plant by plant: its material binding (a 480-byte key built
//! and hashed), what a ray finds on it. For Desert Homestead's 17,000
//! plants in view that was 1.4 ms and 1.9 ms of the frame's thread
//! (`docs/rendering/performance.md`, "Plants by the cluster").
//!
//! A [`Cluster`] is one group's items with those answers kept. It is found
//! again by the address of the group's items, and holds them itself, so
//! the address can't come to mean other items while it's kept; a group the
//! scene builds again has a new address and gets a new cluster. Nothing
//! here depends on the camera or the frame: what does (which items are
//! seen, their level of detail, the wind) is still worked out each frame.

use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;

use glam::Mat4;

use super::lighting::{key_digest, material_key_with};
use super::ray_cut_outs::has_holes;
use super::ray_tracing::{SurfaceRaw, surface_of};
use super::terrain::LastArrays;
use super::{MaterialKey, MeshKey, StandInKey};
use crate::draw_list::{DrawItem, ItemChunk};
use crate::{MeshSource, Shape};

/// A cluster nothing has drawn for this many frames is let go.
const FORGET_AFTER: u64 = 600;

/// What rays need to know of an item that its material alone says.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RayKind {
    /// A cut-out whose holes rays pass through (`ray_cut_outs::has_holes`).
    pub(super) holes: bool,
    /// See-through or water: only rays that ask for such items hit it.
    pub(super) see_through: bool,
    /// Drawn with an alpha cutoff (with `holes` unless it's see-through).
    pub(super) cut_out: bool,
}

impl RayKind {
    pub(super) fn of(item: &DrawItem) -> Self {
        let material = &item.material;
        Self {
            holes: has_holes(material),
            see_through: material.opacity < 1.0 || material.water.is_some(),
            cut_out: material.alpha_cutoff > 0.0,
        }
    }
}

/// [`Cluster::mesh_of`] of an item drawn with a built-in shape.
pub(super) const BUILT_IN: u32 = u32::MAX;

/// One group of items and what was worked out of them.
pub(super) struct Cluster {
    /// The group's items, held so their address stays theirs.
    items: Arc<[DrawItem]>,
    last_used: u64,
    /// The distinct material bindings among the items.
    pub(super) materials: Vec<MaterialKey>,
    /// Each of `materials`' digest (`key_digest`), worked out once here
    /// and not again by each frame that joins the cluster's materials in.
    pub(super) digests: Vec<u64>,
    /// Which of `materials` each item is drawn with.
    pub(super) material_of: Vec<u32>,
    /// The distinct meshes of callers' among the items' own shapes.
    pub(super) meshes: Vec<Arc<dyn MeshSource>>,
    /// Which of `meshes` each item's shape is, or [`BUILT_IN`].
    pub(super) mesh_of: Vec<u32>,
    /// Each item's own shape as a mesh key (what it's drawn with unless a
    /// level of detail takes its place).
    pub(super) mesh_keys: Vec<MeshKey>,
    /// What stands in for each item's mesh far away (`MeshSource::stand_in`).
    pub(super) stand_ins: Vec<StandInKey>,
    /// Where each item stands.
    pub(super) transforms: Vec<Mat4>,
    /// Whether each item is posed by a skin (such an item's bounds follow
    /// its joints, so passes read the item itself).
    pub(super) skinned: Vec<bool>,
    /// Each item's [`RayKind`].
    pub(super) ray_kinds: Vec<RayKind>,
    /// What a ray finds on each item, once something traced asks.
    surfaces: OnceLock<Vec<SurfaceRaw>>,
}

impl Cluster {
    fn of(items: &Arc<[DrawItem]>, frame: u64) -> Self {
        let mut materials: Vec<MaterialKey> = Vec::new();
        let mut indexes: FxHashMap<MaterialKey, u32> = FxHashMap::default();
        let mut last_arrays = LastArrays::default();
        let material_of = items
            .iter()
            .map(|item| {
                let key = material_key_with(item, &mut last_arrays);
                *indexes.entry(key).or_insert_with(|| {
                    materials.push(key);
                    (materials.len() - 1) as u32
                })
            })
            .collect();
        let digests = materials.iter().map(key_digest).collect();
        let mut meshes: Vec<Arc<dyn MeshSource>> = Vec::new();
        let mut mesh_indexes: FxHashMap<*const (), u32> = FxHashMap::default();
        let mesh_of = items
            .iter()
            .map(|item| match &item.shape {
                Shape::Mesh(source) => *mesh_indexes
                    .entry(Arc::as_ptr(source).cast())
                    .or_insert_with(|| {
                        meshes.push(source.clone());
                        (meshes.len() - 1) as u32
                    }),
                _ => BUILT_IN,
            })
            .collect();
        Self {
            items: items.clone(),
            last_used: frame,
            materials,
            digests,
            material_of,
            meshes,
            mesh_of,
            mesh_keys: items.iter().map(|item| MeshKey::of(&item.shape)).collect(),
            stand_ins: items
                .iter()
                .map(|item| StandInKey::of(&item.shape))
                .collect(),
            transforms: items.iter().map(|item| item.transform).collect(),
            skinned: items.iter().map(|item| item.skin.is_some()).collect(),
            ray_kinds: items.iter().map(RayKind::of).collect(),
            surfaces: OnceLock::new(),
        }
    }

    /// What a ray finds on each item (`surface_of`), worked out the first
    /// time it's asked for. `texture_colors` is the ray traced scene's
    /// table of each texture's average color.
    pub(super) fn surfaces(&self, texture_colors: &mut FxHashMap<u64, [f32; 3]>) -> &[SurfaceRaw] {
        self.surfaces.get_or_init(|| {
            self.items
                .iter()
                .map(|item| surface_of(&item.material, texture_colors))
                .collect()
        })
    }
}

/// Every group the renderer has drawn lately, by the address of its items.
#[derive(Default)]
pub(super) struct Clusters {
    by_address: FxHashMap<usize, Cluster>,
}

impl Clusters {
    /// The cluster of each of `chunks`, in their order, made for those
    /// seen for the first time. Call [`Clusters::forget_unused`] now and
    /// then.
    pub(super) fn of<'c>(&'c mut self, chunks: &[&ItemChunk], frame: u64) -> Vec<&'c Cluster> {
        for chunk in chunks {
            let address = Arc::as_ptr(&chunk.items).cast::<u8>() as usize;
            self.by_address
                .entry(address)
                .or_insert_with(|| Cluster::of(&chunk.items, frame))
                .last_used = frame;
        }
        chunks
            .iter()
            .map(|chunk| {
                let address = Arc::as_ptr(&chunk.items).cast::<u8>() as usize;
                &self.by_address[&address]
            })
            .collect()
    }

    /// Lets go of clusters not drawn for [`FORGET_AFTER`] frames, and with
    /// them their hold on the items.
    pub(super) fn forget_unused(&mut self, frame: u64) {
        self.by_address
            .retain(|_, cluster| frame.saturating_sub(cluster.last_used) < FORGET_AFTER);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.by_address.len()
    }
}

#[cfg(test)]
mod tests {
    use super::super::lighting::material_key;
    use super::*;
    use crate::draw_list::Material;
    use glam::Vec3;

    fn plant(color: [f32; 3], cutoff: f32, opacity: f32) -> DrawItem {
        DrawItem {
            shape: Shape::Block,
            transform: Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0)),
            material: Material {
                base_color: color,
                alpha_cutoff: cutoff,
                opacity,
                ..Material::default()
            },
            uv_tile: 0.0,
            casts_shadow: true,
            id: 0,
            cull_distance: 0.0,
            skin: None,
            sway: None,
            decal_role: Default::default(),
        }
    }

    fn chunk(items: Vec<DrawItem>) -> ItemChunk {
        ItemChunk {
            center: Vec3::ZERO,
            radius: 10.0,
            cull_distance: 0.0,
            items: items.into(),
        }
    }

    #[test]
    fn a_cluster_says_of_each_item_what_the_item_would() {
        let items = vec![
            plant([1.0, 0.0, 0.0], 0.0, 1.0),
            plant([0.0, 1.0, 0.0], 0.5, 1.0),
            plant([0.0, 0.0, 1.0], 0.5, 0.4),
        ];
        let chunk = chunk(items.clone());
        let mut clusters = Clusters::default();
        let found = clusters.of(&[&chunk], 1);
        let cluster = found[0];
        for (index, item) in items.iter().enumerate() {
            let local = cluster.material_of[index] as usize;
            assert_eq!(cluster.materials[local], material_key(item));
            assert_eq!(cluster.digests[local], key_digest(&material_key(item)));
            assert_eq!(cluster.ray_kinds[index], RayKind::of(item));
            assert_eq!(cluster.mesh_keys[index], MeshKey::of(&item.shape));
            assert_eq!(cluster.stand_ins[index], StandInKey::of(&item.shape));
            assert_eq!(cluster.transforms[index], item.transform);
            assert_eq!(cluster.skinned[index], item.skin.is_some());
            assert_eq!(cluster.mesh_of[index], BUILT_IN, "blocks are built in");
        }
        assert_eq!(
            cluster.ray_kinds[1],
            RayKind {
                holes: true,
                see_through: false,
                cut_out: true
            }
        );
        assert!(cluster.ray_kinds[2].see_through);
        assert!(!cluster.ray_kinds[2].holes, "see-through, not a cut-out");
        let mut colors = FxHashMap::default();
        let surfaces = cluster.surfaces(&mut colors).to_vec();
        for (item, surface) in items.iter().zip(&surfaces) {
            assert_eq!(*surface, surface_of(&item.material, &mut colors));
        }
    }

    #[test]
    fn items_that_share_a_material_binding_share_it_in_the_cluster() {
        // Plain materials differ only in what each instance carries.
        let chunk = chunk(vec![
            plant([1.0, 0.0, 0.0], 0.0, 1.0),
            plant([0.0, 1.0, 0.0], 0.0, 1.0),
        ]);
        let mut clusters = Clusters::default();
        let cluster = clusters.of(&[&chunk], 1)[0];
        assert_eq!(cluster.materials.len(), 1);
        assert_eq!(cluster.material_of, [0, 0]);
    }

    #[test]
    fn a_group_is_found_again_by_its_items_and_a_rebuilt_one_is_new() {
        let first = chunk(vec![plant([1.0; 3], 0.0, 1.0)]);
        // The same items, shared: the scene hands the same `Arc` each frame.
        let again = ItemChunk {
            items: first.items.clone(),
            ..chunk(Vec::new())
        };
        let rebuilt = chunk(vec![plant([1.0; 3], 0.0, 1.0)]);
        let mut clusters = Clusters::default();
        clusters.of(&[&first], 1);
        clusters.of(&[&again], 2);
        assert_eq!(clusters.len(), 1);
        clusters.of(&[&rebuilt], 3);
        assert_eq!(clusters.len(), 2);
    }

    #[test]
    fn a_cluster_keeps_its_items_until_it_is_forgotten() {
        let mut clusters = Clusters::default();
        let weak = {
            let chunk = chunk(vec![plant([1.0; 3], 0.0, 1.0)]);
            clusters.of(&[&chunk], 1);
            Arc::downgrade(&chunk.items)
        };
        // The scene let go of the group; the cluster still holds its
        // items, so no other group can turn up at their address.
        assert!(weak.upgrade().is_some());
        clusters.forget_unused(1 + FORGET_AFTER - 1);
        assert_eq!(clusters.len(), 1);
        clusters.forget_unused(1 + FORGET_AFTER);
        assert_eq!(clusters.len(), 0);
        assert!(weak.upgrade().is_none());
    }
}
