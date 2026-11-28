//! Hardware ray tracing's scene (`docs/rendering/ray-tracing.md`): every
//! drawn mesh as a bottom-level acceleration structure (BLAS), and the
//! frame's items within the ray tracing distance as one top-level structure
//! (TLAS) that ray queries trace against.
//!
//! It exists only while something traces rays (a ray traced effect, or the
//! editor's Ray Hits view) on a GPU with wgpu's ray queries
//! ([`crate::Capabilities::ray_queries`]); otherwise nothing here is made.
//!
//! - A mesh's BLAS is built the first frame it's wanted, up to
//!   [`BLAS_TRIANGLES_PER_FRAME`] triangles a frame, so a big import builds
//!   over a few frames. Until it's built its items are left out. A caller's
//!   mesh whose [`MeshSource::version`](crate::MeshSource::version) changes
//!   is built again (wgpu has no refit, so every update is a full build).
//! - The TLAS keeps one slot per item in draw-list order and is rebuilt
//!   only when a slot changed (an item moved, came or went). wgpu uploads
//!   the instances from the lowest changed slot up, so a still scene costs
//!   nothing after its first frame.
//! - Posed (skinned or morphed) items get a BLAS of their own each frame,
//!   built from the frame's posed corners (`posing.rs`, CO8: joints,
//!   identity shapes and expressions alike), within a budget, nearest
//!   first; one past the budget is left out (its mesh's rest pose would be
//!   wrong).
//! - Swaying plants in a wind get a bent copy of their mesh each frame, a
//!   BLAS of their own (`ray_sway.rs`), within a budget, nearest first,
//!   and only within [`SWAY_DISTANCE`]: past it every plant is traced at
//!   rest, and its item isn't read for its sway at all.
//! - Small still items share a BLAS by the square of ground they stand on
//!   (`ray_groups.rs`), one TLAS instance a group.
//! - A small plant's ray traced shadow ends at [`SMALL_PLANT_SHADOW_REACH`]
//!   (thinning out over [`SMALL_PLANT_SHADOW_FADE`] before it), and rays
//!   pass through a cut-out's holes only within [`HOLES_REACH`]: further
//!   off, every hit on its cards counts (its slot has no holes), which
//!   ends a shadow ray at the first leaf it meets.

use glam::{Mat4, Vec3};
use rustc_hash::FxHashMap;

use super::clusters::{Cluster, RayKind};
use super::posing::Posing;
use super::ray_cut_outs::{CutOutRaw, CutOuts};
use super::ray_groups::{
    GROUP_TRIANGLES_PER_FRAME, GROUPED_TRIANGLES, GroupKey, MIN_GROUP, Member, StillGroups,
    fold_print, item_print,
};
use super::ray_sway::{
    BENT_TRIANGLES, BentMeshes, BundleKey, POSED_TRIANGLES, PosedKey, SEEN_SWAY_PIXELS, Sight,
    Swaying, bending, bundle_cell,
};
use super::scene::gpu_mesh_in;
use super::{Draw, GpuMesh, MeshKey, ScenePipeline, StandInKey};
use crate::draw_list::{DrawItem, Material};

/// Solid items that cast shadows.
pub(super) const MASK_SOLID: u8 = 1 << 0;
/// Items with cut-out textures (`alpha_cutoff`) that cast shadows.
pub(super) const MASK_CUT_OUT: u8 = 1 << 1;
/// See-through items and water.
pub(super) const MASK_SEE_THROUGH: u8 = 1 << 2;
/// Solid items with `casts_shadow` off.
pub(super) const MASK_NO_SHADOW: u8 = 1 << 3;
/// Cut-out items with `casts_shadow` off.
pub(super) const MASK_NO_SHADOW_CUT_OUT: u8 = 1 << 4;

/// The most triangles built into new BLASes in one frame. One mesh bigger
/// than this is still built whole, on a frame of its own. About 5 ms on an
/// RX 9070 XT (2.6 ms per million, `docs/research/hardware-ray-tracing.md`).
pub(super) const BLAS_TRIANGLES_PER_FRAME: u64 = 2_000_000;

/// A mesh's BLAS unused for this many frames is freed, as meshes are.
const FORGET_AFTER: u64 = 600;

/// Swaying items further than this from the camera (studs, to the nearest
/// point of their bounds) are traced at rest. In Overgrown Street's dense
/// views the bending budget ran out at about 26 studs anyway, and deciding
/// each further plant's sway read 48,000 items a frame.
pub(super) const SWAY_DISTANCE: f32 = 32.0;

/// A cut-out no bigger than this (the radius of its bounds, studs) is a
/// small plant: ground cover, a fern, a bush.
pub(super) const SMALL_PLANT_RADIUS: f32 = 4.0;

/// How far from the camera (studs) a small plant casts a ray traced
/// shadow, thinning out over the last [`SMALL_PLANT_SHADOW_FADE`].
pub(super) const SMALL_PLANT_SHADOW_REACH: f32 = 55.0;
pub(super) const SMALL_PLANT_SHADOW_FADE: f32 = 5.0;

/// How far from the camera (studs) rays pass through a cut-out's holes.
pub(super) const HOLES_REACH: f32 = 60.0;

/// How much of its ray traced shadow a small plant `distance` studs away
/// casts: all of it, then thinning to none at [`SMALL_PLANT_SHADOW_REACH`].
pub(super) fn small_plant_shadow(distance: f32) -> f32 {
    if !reach_on("AXCENITY_NO_SMALL_PLANT_REACH", &SMALL_PLANT_OFF) {
        return 1.0;
    }
    let start = SMALL_PLANT_SHADOW_REACH - SMALL_PLANT_SHADOW_FADE;
    (1.0 - (distance - start) / SMALL_PLANT_SHADOW_FADE).clamp(0.0, 1.0)
}

/// Measuring switches, to time the ray traced scene as it was before
/// 2026-11-27 on one build: `AXCENITY_NO_RAY_GROUPS` traces every item as
/// an instance of its own, `AXCENITY_NO_RAY_REACHES` bends swaying plants,
/// casts small plants' shadows and passes rays through holes at any
/// distance (`docs/rendering/ray-tracing.md`); `AXCENITY_NO_SWAY_DISTANCE`,
/// `AXCENITY_NO_SMALL_PLANT_REACH` and `AXCENITY_NO_HOLES_REACH` lift one
/// of the three.
fn switch_off(name: &str, cell: &'static std::sync::OnceLock<bool>) -> bool {
    *cell.get_or_init(|| std::env::var_os(name).is_some())
}

fn groups_on() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !switch_off("AXCENITY_NO_RAY_GROUPS", &OFF)
}

static SMALL_PLANT_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static SWAY_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static HOLES_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Whether the reach `name` lifts is kept: neither it nor
/// `AXCENITY_NO_RAY_REACHES` is set.
fn reach_on(name: &str, cell: &'static std::sync::OnceLock<bool>) -> bool {
    static ALL_OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !switch_off("AXCENITY_NO_RAY_REACHES", &ALL_OFF) && !switch_off(name, cell)
}

/// [`SWAY_DISTANCE`] and [`HOLES_REACH`], or no limit when lifted.
fn sway_distance() -> f32 {
    if reach_on("AXCENITY_NO_SWAY_DISTANCE", &SWAY_OFF) {
        SWAY_DISTANCE
    } else {
        f32::MAX
    }
}

fn holes_reach() -> f32 {
    if reach_on("AXCENITY_NO_HOLES_REACH", &HOLES_OFF) {
        HOLES_REACH
    } else {
        f32::MAX
    }
}

/// How many items ray tracing saw in a frame, for [`crate::RenderStats`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RayTracingStats {
    /// Items in the TLAS.
    pub instances: u32,
    /// Meshes with a built BLAS.
    pub meshes: u32,
    /// Their triangles.
    pub triangles: u64,
    /// Meshes wanted but not built yet (waiting for the per-frame budget).
    pub meshes_waiting: u32,
    /// Meshes whose BLAS was built this frame (new, or a new version).
    pub meshes_built: u32,
    /// Whether the TLAS was rebuilt this frame.
    pub rebuilt: bool,
    /// Paths a pixel the editor's Path Traced view has averaged so far (0
    /// while it isn't shown).
    pub path_traced_paths: u32,
    /// Swaying items bent by the wind into BLASes of their own this frame.
    pub bent: u32,
    /// Posed (skinned) characters moved into BLASes of their own this
    /// frame.
    pub posed: u32,
    /// Posed characters in reach left out for the per-frame budget.
    pub posed_left_out: u32,
    /// Swaying items whose sway shows that were traced at rest this frame,
    /// for the per-frame budget: their shadows stand still while they move.
    pub bent_left_out: u32,
    /// How far ray traced sun shadows reach this frame, in studs (0 while
    /// they're off): the ray tracing distance or the world's
    /// `ShadowDistance`, whichever is nearer.
    pub shadow_reach: u32,
    /// Groups of small still items traced this frame (one TLAS instance
    /// each), the items in them, and the groups built this frame.
    pub groups: u32,
    pub grouped: u32,
    pub groups_built: u32,
}

struct MeshBlas {
    blas: wgpu::Blas,
    size: wgpu::BlasTriangleGeometrySizeDescriptor,
    /// The mesh's version it was made for.
    version: u64,
    built: bool,
    last_used: u64,
}

/// What a ray that hits a TLAS slot's item finds, for ray traced GI
/// (`ray_gi.wgsl`'s `gi_surfaces`, two `vec4`s a slot): its color times its
/// texture's average, with 1 after it for a cut-out (a leaf card, lit from
/// either side) and 0 otherwise, then its glow, both linear light.
pub(super) type SurfaceRaw = [[f32; 4]; 2];

/// The TLAS slots' [`SurfaceRaw`]s on the GPU, kept while ray traced GI
/// asks for them.
pub(super) struct Surfaces {
    pub(super) buffer: wgpu::Buffer,
    capacity: usize,
    written: Vec<SurfaceRaw>,
    /// Each base color texture's average color (linear), by its key.
    pub(super) texture_colors: FxHashMap<u64, [f32; 3]>,
}

/// A mesh's BLAS: the mesh, and whether it's built for a cut-out (without
/// the opaque flag, so rays can pass through its holes). A mesh drawn both
/// ways has one of each.
type BlasKey = (MeshKey, bool);

/// An item wanted in the ray traced scene: its BLAS, mesh, transform and
/// instance mask.
type Wanted<'a> = (BlasKey, &'a GpuMesh, Mat4, u8);

/// A draw within ray tracing's reach, as one thread works it out (see
/// [`RayScene::prepare`]).
struct WantedOne<'a> {
    wanted: Wanted<'a>,
    draw: &'a Draw<'a>,
    /// How far it is from the camera, to the nearest point of its bounds.
    distance: f32,
    /// How much of its shadow it casts when traced.
    shadow: f32,
    /// Whether rays pass through its holes.
    holes: bool,
    /// Whether a skin may pose it.
    posable: bool,
}

/// One slot: an item of the ray traced scene, and a row of the tables rays
/// look their hits up in (`Surfaces`, `CutOuts`, the path tracer's). A hit's
/// slot is its TLAS instance's first one plus the geometry hit.
#[derive(Clone, Copy, PartialEq)]
struct Slot {
    mesh: BlasKey,
    transform: [f32; 12],
}

/// A copy of an item's mesh made for this frame alone.
#[derive(Clone, Copy, PartialEq)]
enum Moved {
    /// A posed (skinned or morphed) item's, a BLAS of its own.
    Posed(PosedKey),
    /// A swaying item's, bent by the wind into a BLAS it shares, from this
    /// corner on among the bent positions.
    Bent(u32),
}

/// Whose BLAS a TLAS instance uses.
#[derive(Clone, Copy, PartialEq)]
enum InstanceBlas {
    /// An item's mesh's.
    Mesh(BlasKey),
    /// A posed item's own.
    Posed(PosedKey),
    /// A bundle's: every swaying item bent into it this frame, each a
    /// geometry of it and a slot of its own, already where it stands in
    /// the world.
    Bundle(BundleKey),
    /// A group's: small still items, each a geometry of it placed by its
    /// transform (`ray_groups.rs`).
    Group(GroupKey),
}

/// One TLAS instance, compared frame to frame.
#[derive(Clone, Copy, PartialEq)]
struct Instance {
    blas: InstanceBlas,
    transform: [f32; 12],
    /// Its first (or only) slot, which rays read as its custom index.
    first_slot: u32,
    mask: u8,
}

/// The rows of a 3 by 4 transform that moves nothing.
const IN_PLACE: [f32; 12] = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];

/// The frame's slots and TLAS instances.
struct Laid {
    /// Each slot's item (an index into the wanted items).
    order: Vec<usize>,
    instances: Vec<Instance>,
    /// Each bundle with its items' slots' range in `order`.
    bundles: Vec<(BundleKey, std::ops::Range<usize>)>,
    /// Each group with its items' slots' range in `order`.
    groups: Vec<(GroupKey, std::ops::Range<usize>)>,
}

/// Lays out the frame's slots and TLAS instances from `in_slots` (indices
/// into `wanted`), each one's `moved` copy and the group it's `grouped`
/// in: first the items with an instance of their own, in order, then each
/// group's items together (its geometries, in the frame's order), then
/// each bundle's bent items together (its geometries, in slot order), the
/// biggest meshes first so a bundle's BLAS seldom has to be made again for
/// a frame's different items. Bundles change every frame and go last, as
/// wgpu uploads a TLAS's instances from the first that changed.
/// `distances` is how far each wanted item is from the camera.
fn lay_out(
    in_slots: &[usize],
    moved: &[Option<Moved>],
    grouped: &[Option<GroupKey>],
    wanted: &[Wanted<'_>],
    distances: &[f32],
) -> Laid {
    let mut order = Vec::with_capacity(in_slots.len());
    let mut instances = Vec::new();
    let mut bundled: Vec<(BundleKey, u32, usize)> = Vec::new();
    // Each group's items in the frame's order, the groups as first met.
    let mut group_items: Vec<(GroupKey, Vec<usize>)> = Vec::new();
    let mut group_of: FxHashMap<GroupKey, usize> = FxHashMap::default();
    for ((&index, moved), group) in in_slots.iter().zip(moved).zip(grouped) {
        if let Some(key) = group {
            // A group's items mostly come together: the last one is asked
            // first.
            let at = match group_items.last() {
                Some((last, _)) if last == key => group_items.len() - 1,
                _ => *group_of.entry(*key).or_insert_with(|| {
                    group_items.push((*key, Vec::new()));
                    group_items.len() - 1
                }),
            };
            group_items[at].1.push(index);
            continue;
        }
        let (mesh, gpu, transform, mask) = wanted[index];
        let blas = match moved {
            Some(Moved::Bent(_)) => {
                let wide = gpu.index_format == wgpu::IndexFormat::Uint32;
                let cell = bundle_cell(transform.w_axis.truncate(), distances[index]);
                bundled.push(((mesh.1, mask, wide, cell), gpu.index_count, index));
                continue;
            }
            Some(Moved::Posed(key)) => InstanceBlas::Posed(*key),
            None => InstanceBlas::Mesh(mesh),
        };
        instances.push(Instance {
            blas,
            transform: rows_3x4(transform),
            first_slot: order.len() as u32,
            mask,
        });
        order.push(index);
    }
    let mut groups = Vec::with_capacity(group_items.len());
    for (key, items) in group_items {
        let first = order.len();
        instances.push(Instance {
            blas: InstanceBlas::Group(key),
            transform: IN_PLACE,
            first_slot: first as u32,
            mask: key.1,
        });
        order.extend(items);
        groups.push((key, first..order.len()));
    }
    // A stable sort: items of one size stay in the frame's order.
    bundled.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut bundles = Vec::new();
    for bundle in bundled.chunk_by(|a, b| a.0 == b.0) {
        let key = bundle[0].0;
        let first = order.len();
        instances.push(Instance {
            blas: InstanceBlas::Bundle(key),
            transform: IN_PLACE,
            first_slot: first as u32,
            mask: key.1,
        });
        order.extend(bundle.iter().map(|&(_, _, index)| index));
        bundles.push((key, first..order.len()));
    }
    Laid {
        order,
        instances,
        bundles,
        groups,
    }
}

/// The ray traced scene: BLASes by mesh and the TLAS.
pub(super) struct RayScene {
    blases: FxHashMap<BlasKey, MeshBlas>,
    tlas: wgpu::Tlas,
    capacity: u32,
    slots: Vec<Slot>,
    instances: Vec<Instance>,
    /// Whether the TLAS has been built with `instances`.
    built: bool,
    /// What rays hitting each slot find, while ray traced GI is on.
    pub(super) surfaces: Option<Surfaces>,
    /// Where each slot's holes are, for rays that pass through them.
    pub(super) cut_outs: CutOuts,
    /// Swaying items' bent copies, made the first time the wind blows.
    bent: Option<BentMeshes>,
    /// Small still items' groups.
    groups: StillGroups,
    pub(super) stats: RayTracingStats,
}

impl RayScene {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let capacity = 256;
        Self {
            blases: FxHashMap::default(),
            tlas: create_tlas(device, capacity),
            capacity,
            slots: Vec::new(),
            instances: Vec::new(),
            built: false,
            surfaces: None,
            cut_outs: CutOuts::new(device),
            bent: None,
            groups: StillGroups::default(),
            stats: RayTracingStats::default(),
        }
    }

    /// The TLAS, built (this frame or before) with the frame's items.
    pub(super) fn tlas(&self) -> &wgpu::Tlas {
        &self.tlas
    }

    /// Frees the BLASes of meshes no longer `kept` (gone from the GPU).
    pub(super) fn forget_meshes(&mut self, kept: impl Fn(MeshKey) -> bool) {
        self.blases.retain(|(mesh, _), _| kept(*mesh));
    }

    /// Each slot's mesh and its item's transform (the rows of a 3 by 4
    /// matrix), in slot order.
    #[cfg(feature = "debug-views")]
    pub(super) fn slots(&self) -> impl Iterator<Item = (MeshKey, [f32; 12])> + '_ {
        self.slots.iter().map(|slot| (slot.mesh.0, slot.transform))
    }

    /// How many slots are filled.
    #[cfg(feature = "debug-views")]
    pub(super) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Brings the structures up to date with this frame's `draws`, recording
    /// their builds into `encoder`: new meshes' BLASes within the budget,
    /// and the TLAS if any slot changed. `reach` is the ray tracing
    /// distance from `camera`; `mesh` finds an uploaded mesh. With
    /// `surfaces`, each slot's [`SurfaceRaw`] is kept up to date too.
    /// `clock` is the frame's time and wind (`Frame::clock`), which bends
    /// swaying items.
    pub(super) fn prepare<'a>(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        encoder: &mut wgpu::CommandEncoder,
        (frame, (camera, sight), reach, surfaces, clock): (u64, (Vec3, Sight), f32, bool, [f32; 4]),
        (draws, clusters, posing): (&'a [Draw<'a>], &[&Cluster], &Posing),
        mesh: impl Fn(MeshKey) -> Option<&'a GpuMesh> + Sync,
    ) {
        if !surfaces {
            self.surfaces = None;
        }
        // Each draw within reach, worked out on its own: shared among the
        // renderer's threads (a meadow is a hundred thousand), in order.
        let want = |draw: &'a Draw<'a>| -> Option<WantedOne<'a>> {
            let item = draw.item;
            // The copy fading out of a level-of-detail switch.
            if draw.fade > 1.0 {
                return None;
            }
            // What never changes of an item in a group comes from its
            // cluster, where it was worked out once: a frame doesn't read
            // the item of a still, unposed plant at all. The rest is
            // worked out here from the item.
            let at = draw.at as usize;
            let kept = clusters
                .get(draw.cluster as usize)
                .filter(|cluster| !cluster.skinned[at]);
            let drawn = match (draw.lod, kept) {
                (None, Some(cluster)) => cluster.mesh_keys[at],
                _ => draw.mesh(),
            };
            // From `STAND_IN_LEVEL`, its stand-in is traced in its place
            // once that's on the GPU (`MeshSource::stand_in`), and a mesh
            // another's stand-in covers is left out.
            let key = match draw.level {
                Some(level) if level >= crate::STAND_IN_LEVEL => {
                    let stand_in = match clusters.get(draw.cluster as usize) {
                        Some(cluster) => cluster.stand_ins[at],
                        None => StandInKey::of(&item.shape),
                    };
                    stand_in.mesh(level, drawn, |key| mesh(key).is_some())?
                }
                _ => drawn,
            };
            let gpu = mesh(key)?;
            let (transform, (center, radius)) = match kept {
                Some(cluster) => {
                    let transform = cluster.transforms[at];
                    (transform, gpu.bounds.sphere(transform))
                }
                None => (item.transform, item_sphere(item, gpu)),
            };
            let distance = center.distance(camera) - radius;
            if distance > reach {
                return None;
            }
            let kind = match clusters.get(draw.cluster as usize) {
                Some(cluster) => cluster.ray_kinds[at],
                None => RayKind::of(item),
            };
            // A small plant's traced shadow ends sooner than the others'.
            let shadow = if kind.cut_out && radius <= SMALL_PLANT_RADIUS {
                draw.shadow * small_plant_shadow(distance)
            } else {
                draw.shadow
            };
            // Near the end of its shadow's reach a cut-out's shadow thins
            // out ray by ray (`CutOutRaw::casting`); an item without
            // holes, whose hits a ray can't pass over, casts until half
            // of its shadow is gone. One casting none isn't in the shadow
            // rays' mask at all.
            let casts = draw.casts_shadow && shadow > 0.0 && (kind.holes || shadow >= 0.5);
            Some(WantedOne {
                wanted: ((key, kind.holes), gpu, transform, mask_of(kind, casts)),
                draw,
                distance,
                shadow,
                holes: kind.holes && distance <= holes_reach(),
                posable: kept.is_none() && item.skin.is_some(),
            })
        };
        let parts: Vec<Vec<WantedOne<'a>>> = if super::parallel::worth_sharing(draws.len()) {
            super::parallel::shared(|| {
                super::parallel::map_parts(draws, |part| part.iter().filter_map(want).collect())
            })
        } else {
            vec![draws.iter().filter_map(want).collect()]
        };
        let count: usize = parts.iter().map(Vec::len).sum();
        // The items wanted, and the meshes they need.
        let mut wanted: Vec<Wanted<'a>> = Vec::with_capacity(count);
        let mut wanted_items = Vec::with_capacity(count);
        // Each wanted item's distance, for bending the nearest first.
        let mut wanted_distances = Vec::with_capacity(count);
        // How much of its shadow each casts (`Draw::shadow`, and a small
        // plant's shorter reach when traced).
        let mut wanted_shadows = Vec::with_capacity(count);
        // Whether rays pass through each one's holes (a cut-out within
        // `HOLES_REACH`).
        let mut wanted_holes = Vec::with_capacity(count);
        // Whether each may be posed by a skin: an item of a cluster isn't
        // (`Cluster::skinned`), so its item needn't be read to find out.
        let mut wanted_posable = Vec::with_capacity(count);
        let mut wanted_surfaces: Vec<SurfaceRaw> = Vec::new();
        for one in parts.into_iter().flatten() {
            if surfaces {
                let table = self.surfaces.get_or_insert_with(|| Surfaces::new(device));
                let draw = one.draw;
                wanted_surfaces.push(match clusters.get(draw.cluster as usize) {
                    Some(cluster) => cluster.surfaces(&mut table.texture_colors)[draw.at as usize],
                    None => surface_of(&draw.item.material, &mut table.texture_colors),
                });
            }
            wanted.push(one.wanted);
            wanted_items.push(one.draw.item);
            wanted_distances.push(one.distance);
            wanted_shadows.push(one.shadow);
            wanted_holes.push(one.holes);
            wanted_posable.push(one.posable);
        }

        // New or changed meshes get a BLAS, built within the budget.
        let mut budget = BLAS_TRIANGLES_PER_FRAME;
        let mut to_build: Vec<(BlasKey, &'a GpuMesh)> = Vec::new();
        let mut waiting = 0;
        // Whether each wanted item's BLAS is built, or is being this frame:
        // found here, so the 25,000 items of a meadow look their mesh up
        // once a frame, not twice.
        let mut ready = Vec::with_capacity(wanted.len());
        for &(key, gpu, _, _) in &wanted {
            let blas = self.blases.entry(key);
            let entry = match blas {
                std::collections::hash_map::Entry::Occupied(entry)
                    if entry.get().version == gpu.version =>
                {
                    entry.into_mut()
                }
                blas => {
                    let size = size_of_mesh(gpu, key.1);
                    let fresh = MeshBlas {
                        blas: create_blas(device, &size),
                        size,
                        version: gpu.version,
                        built: false,
                        last_used: frame,
                    };
                    match blas {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            entry.insert(fresh);
                            entry.into_mut()
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(fresh),
                    }
                }
            };
            entry.last_used = frame;
            if entry.built || to_build.iter().any(|(queued, _)| *queued == key) {
                ready.push(true);
                continue;
            }
            let triangles = u64::from(gpu.index_count / 3);
            if triangles > budget && budget < BLAS_TRIANGLES_PER_FRAME {
                waiting += 1;
                ready.push(false);
                continue;
            }
            budget = budget.saturating_sub(triangles);
            to_build.push((key, gpu));
            ready.push(true);
        }
        for (key, gpu) in &to_build {
            if let Some(entry) = self.blases.get_mut(key) {
                entry.built = true;
            }
            // A cut-out mesh's texture coordinates, for its holes.
            if key.1 {
                self.cut_outs.add_mesh(device, encoder, key.0, gpu);
            }
        }

        // The frame's slots: items whose BLAS is (or is being) built.
        let in_slots: Vec<usize> = (0..wanted.len()).filter(|&index| ready[index]).collect();
        let (in_slots, moved, sways, posed_left_out) = self.bend(
            (device, queue),
            in_slots,
            (&wanted, &wanted_items, &wanted_distances, &wanted_posable),
            (clock, sight, posing),
            frame,
        );
        let grouped = self.group_still(
            (device, queue),
            (&in_slots, &moved, &sways),
            (&wanted, &wanted_distances),
            frame,
        );
        let laid = lay_out(&in_slots, &moved, &grouped, &wanted, &wanted_distances);
        let moved_of: FxHashMap<usize, Moved> = in_slots
            .iter()
            .zip(&moved)
            .filter_map(|(&index, moved)| Some((index, (*moved)?)))
            .collect();
        // Each bundle's bent copies become its BLAS's geometries, in slot
        // order.
        if let Some(meshes) = &mut self.bent {
            for (key, range) in &laid.bundles {
                let copies = laid.order[range.clone()]
                    .iter()
                    .filter_map(|index| {
                        let Some(Moved::Bent(copy)) = moved_of.get(index) else {
                            return None;
                        };
                        Some((*copy, size_of_mesh(wanted[*index].1, key.0)))
                    })
                    .collect();
                meshes.bundle(device, *key, copies, frame);
            }
        }
        let slots: Vec<Slot> = laid
            .order
            .iter()
            .map(|&index| {
                let (mesh, _, transform, _) = wanted[index];
                Slot {
                    mesh,
                    transform: rows_3x4(transform),
                }
            })
            .collect();
        let cut_outs: Vec<CutOutRaw> = laid
            .order
            .iter()
            .map(|&index| {
                let ((mesh, cut_out), gpu, _, _) = wanted[index];
                if wanted_holes[index] {
                    self.cut_outs
                        .slot((device, queue), wanted_items[index], mesh, gpu.version)
                        .casting(wanted_shadows[index])
                } else if cut_out {
                    // Past `HOLES_REACH`: every hit on its cards counts,
                    // and its shadow still thins out ray by ray.
                    CutOutRaw::default().casting(wanted_shadows[index])
                } else {
                    CutOutRaw::default()
                }
            })
            .collect();
        self.cut_outs.write(device, queue, cut_outs);
        if let Some(table) = &mut self.surfaces {
            let raw: Vec<SurfaceRaw> = laid
                .order
                .iter()
                .map(|&index| wanted_surfaces[index])
                .collect();
            table.write(device, queue, raw);
        }
        let count = slots.len() as u32;
        let instances = laid.instances;
        if instances.len() as u32 > self.capacity {
            self.capacity = (instances.len() as u32).next_power_of_two();
            self.tlas = create_tlas(device, self.capacity);
            self.instances.clear();
            self.built = false;
        }
        // A posed or bent copy is built afresh each frame, so the TLAS is
        // too, and so is a group's BLAS made again.
        let any_moved = !moved_of.is_empty();
        let changed = !self.built
            || instances != self.instances
            || !to_build.is_empty()
            || any_moved
            || self.groups.any_building();
        if changed {
            for (index, instance) in instances.iter().enumerate() {
                let blas = match (&instance.blas, &self.bent) {
                    (InstanceBlas::Mesh(key), _) => {
                        // An instance whose mesh was built again needs its
                        // new BLAS.
                        let rebuilt = to_build.iter().any(|(built, _)| built == key);
                        if self.built && !rebuilt && self.instances.get(index) == Some(instance) {
                            continue;
                        }
                        self.blases.get(key).map(|entry| &entry.blas)
                    }
                    // A copy's instance is set again: its BLAS may be new.
                    (InstanceBlas::Posed(key), Some(bent)) => bent.posed_blas(key),
                    (InstanceBlas::Bundle(key), Some(bent)) => bent.bundle_blas(key),
                    (InstanceBlas::Group(key), _) => self.groups.blas(key),
                    (InstanceBlas::Posed(_) | InstanceBlas::Bundle(_), None) => None,
                };
                self.tlas[index] = blas.map(|blas| {
                    wgpu::TlasInstance::new(
                        blas,
                        instance.transform,
                        instance.first_slot,
                        instance.mask,
                    )
                });
            }
            for index in instances.len()..self.instances.len().min(self.capacity as usize) {
                self.tlas[index] = None;
            }
        }

        // Record the builds: the swaying items' bending, the BLASes, then
        // the TLAS that uses them.
        if let Some(bent_meshes) = &self.bent {
            bent_meshes.record(encoder);
        }
        let posed_entries = in_slots.iter().zip(&moved).filter_map(|(&index, moved)| {
            let Some(Moved::Posed(key)) = moved else {
                return None;
            };
            self.bent
                .as_ref()?
                .posed_entry(key, wanted[index].1, posing.posed())
        });
        let bundle_entries = laid.bundles.iter().filter_map(|(key, range)| {
            let meshes: Vec<&GpuMesh> = laid.order[range.clone()]
                .iter()
                .map(|&index| wanted[index].1)
                .collect();
            self.bent.as_ref()?.bundle_entry(key, &meshes)
        });
        // The groups made again this frame: their items' meshes, placed.
        let group_entries = laid
            .groups
            .iter()
            .filter(|(key, _)| self.groups.building(key))
            .filter_map(|(key, range)| {
                let meshes: Vec<&GpuMesh> = laid.order[range.clone()]
                    .iter()
                    .map(|&index| wanted[index].1)
                    .collect();
                self.groups.entry(key, &meshes)
            });
        let entries: Vec<wgpu::BlasBuildEntry> = to_build
            .iter()
            .map(|(key, gpu)| {
                let entry = &self.blases[key];
                wgpu::BlasBuildEntry {
                    blas: &entry.blas,
                    geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                        wgpu::BlasTriangleGeometry {
                            size: &entry.size,
                            vertex_buffer: &gpu.vertices,
                            first_vertex: 0,
                            vertex_stride: size_of::<crate::mesh::Vertex>() as u64,
                            index_buffer: Some(&gpu.indices),
                            first_index: Some(0),
                            transform_buffer: None,
                            transform_buffer_offset: None,
                        },
                    ]),
                }
            })
            .chain(posed_entries)
            .chain(bundle_entries)
            .chain(group_entries)
            .collect();
        if changed || !entries.is_empty() {
            let tlas = changed.then_some(&self.tlas);
            encoder.build_acceleration_structures(entries.iter(), tlas);
        }
        drop(entries);
        self.slots = slots;
        self.instances = instances;
        self.built = true;

        // Meshes nothing has wanted for a while are freed.
        self.blases
            .retain(|_, entry| frame.saturating_sub(entry.last_used) < FORGET_AFTER);
        if let Some(bent) = &mut self.bent {
            bent.forget_unused(frame);
        }
        self.groups.forget_unused(frame);
        let (groups, groups_built) = self.groups.counts();
        let grouped = laid
            .groups
            .iter()
            .map(|(_, range)| range.len() as u32)
            .sum();
        let built = self.blases.values().filter(|entry| entry.built);
        self.stats = RayTracingStats {
            instances: count,
            meshes: built.clone().count() as u32,
            triangles: built
                .map(|entry| u64::from(entry.size.index_count.unwrap_or(0) / 3))
                .sum(),
            meshes_waiting: waiting,
            meshes_built: to_build.len() as u32,
            rebuilt: changed,
            path_traced_paths: 0,
            bent: self.bent.as_ref().map_or(0, BentMeshes::bent_count),
            posed: self.bent.as_ref().map_or(0, BentMeshes::posed_count),
            posed_left_out,
            bent_left_out: self.stats.bent_left_out,
            shadow_reach: 0,
            groups,
            grouped,
            groups_built,
        };
    }

    /// Builds this frame's posed items from their posed corners (`posing`)
    /// and bends its swaying ones in the wind of `clock`, nearest first
    /// within [`POSED_TRIANGLES`] and [`BENT_TRIANGLES`]; swaying ones only
    /// within [`SWAY_DISTANCE`]. Returns `in_slots` (indices into the wanted
    /// items) without the posed items past the budget, each one's bent or
    /// posed copy (`None` to keep its mesh's rest pose), whether each sways
    /// where its sway shows (bent, or left out for the budget, so its copy
    /// may come and go), and how many posed items were left out. `posable`
    /// says which wanted items may be posed; the others' items aren't read
    /// for it.
    #[allow(clippy::type_complexity)]
    fn bend(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        in_slots: Vec<usize>,
        (wanted, items, distances, posable): (&[Wanted<'_>], &[&DrawItem], &[f32], &[bool]),
        (clock, sight, posing): ([f32; 4], Sight, &Posing),
        frame: u64,
    ) -> (Vec<usize>, Vec<Option<Moved>>, Vec<bool>, u32) {
        let mut bent = vec![None; in_slots.len()];
        let mut sways = vec![false; in_slots.len()];
        self.stats.bent_left_out = 0;
        // Posed items first (a skinned mesh never also sways here), then
        // swaying ones, each nearest first.
        // Where an item's posed corners start this frame: its joints and its
        // shapes, as every pass draws it.
        let posed_mesh = |index: usize| {
            if !posable[index] {
                return None;
            }
            let ((mesh, _), _, _, _) = wanted[index];
            posing.offset(items[index].skin.as_deref()?, mesh)
        };
        let mut posing: Vec<usize> = (0..in_slots.len())
            .filter(|&slot| posed_mesh(in_slots[slot]).is_some())
            .collect();
        let mut swaying: Vec<(usize, [f32; 4])> = in_slots
            .iter()
            .enumerate()
            // Past `SWAY_DISTANCE` at rest, and the item isn't read.
            .filter(|&(_, &index)| distances[index] <= sway_distance())
            .filter(|&(_, &index)| posed_mesh(index).is_none())
            .filter_map(|(slot, &index)| {
                let (sway, moves) = bending(items[index], clock)?;
                // Too far away for its sway to show: traced at rest.
                (sight.pixels(moves, distances[index]) >= SEEN_SWAY_PIXELS).then_some((slot, sway))
            })
            .collect();
        for &(slot, _) in &swaying {
            sways[slot] = true;
        }
        if swaying.is_empty() && posing.is_empty() {
            if let Some(meshes) = &mut self.bent {
                meshes.begin();
            }
            return (in_slots, bent, sways, 0);
        }
        let nearest =
            |a: &usize, b: &usize| distances[in_slots[*a]].total_cmp(&distances[in_slots[*b]]);
        posing.sort_by(nearest);
        swaying.sort_by(|a, b| nearest(&a.0, &b.0));
        let meshes = self.bent.get_or_insert_with(|| BentMeshes::new(device));
        meshes.begin();
        let mut copies: FxHashMap<BlasKey, u32> = FxHashMap::default();
        let mut copy_of = |blas_key: BlasKey| {
            let copy = copies.entry(blas_key).or_default();
            let key = (blas_key.0, blas_key.1, *copy);
            *copy += 1;
            key
        };

        let mut budget = POSED_TRIANGLES;
        let mut left_out = vec![false; in_slots.len()];
        let mut posed_left_out = 0;
        for slot in posing {
            let index = in_slots[slot];
            let (blas_key, gpu, _, _) = wanted[index];
            let triangles = u64::from(gpu.index_count / 3);
            let Some(first) = posed_mesh(index) else {
                continue;
            };
            if triangles > budget {
                left_out[slot] = true;
                posed_left_out += 1;
                continue;
            }
            budget -= triangles;
            let key = copy_of(blas_key);
            meshes.pose(
                device,
                key,
                gpu,
                size_of_mesh(gpu, blas_key.1),
                first,
                frame,
            );
            bent[slot] = Some(Moved::Posed(key));
        }

        let mut budget = BENT_TRIANGLES;
        let mut bending: Vec<Swaying<'_>> = Vec::new();
        let mut bending_slots = Vec::new();
        for (slot, sway) in swaying {
            let ((mesh, _), gpu, transform, _) = wanted[in_slots[slot]];
            let triangles = u64::from(gpu.index_count / 3);
            if triangles > budget {
                self.stats.bent_left_out += 1;
                continue;
            }
            budget -= triangles;
            bending.push((mesh, gpu, transform, sway));
            bending_slots.push(slot);
        }
        let first_corners = meshes.bend((device, queue), clock, &bending, frame);
        for (slot, first_corner) in bending_slots.into_iter().zip(first_corners) {
            bent[slot] = Some(Moved::Bent(first_corner));
        }

        if posed_left_out == 0 {
            return (in_slots, bent, sways, 0);
        }
        let mut kept_slots = Vec::with_capacity(in_slots.len());
        let mut kept_bent = Vec::with_capacity(in_slots.len());
        let mut kept_sways = Vec::with_capacity(in_slots.len());
        for (((slot, bent), sways), out) in in_slots.into_iter().zip(bent).zip(sways).zip(left_out)
        {
            if !out {
                kept_slots.push(slot);
                kept_bent.push(bent);
                kept_sways.push(sways);
            }
        }
        (kept_slots, kept_bent, kept_sways, posed_left_out)
    }

    /// Groups this frame's small still items (`ray_groups.rs`): those of
    /// `in_slots` (indices into `wanted`, as far as `distances`) neither
    /// `moved` nor swaying where it shows (`sways`), of at most
    /// [`GROUPED_TRIANGLES`] triangles, and not see-through, by the square
    /// of ground they stand on. Returns each slot's group, or `None` for one
    /// traced as an instance of its own (not groupable, too few in its
    /// square, or its group waiting to be built).
    fn group_still(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        (in_slots, moved, sways): (&[usize], &[Option<Moved>], &[bool]),
        (wanted, distances): (&[Wanted<'_>], &[f32]),
        frame: u64,
    ) -> Vec<Option<GroupKey>> {
        let mut grouped = vec![None; in_slots.len()];
        self.groups.begin();
        if !groups_on() {
            return grouped;
        }
        // Each slot's group and fingerprint, if it's grouped at all: shared
        // among the renderer's threads.
        let slots: Vec<usize> = (0..in_slots.len()).collect();
        let key_of = |slot: &usize| -> Option<(GroupKey, u64)> {
            let slot = *slot;
            if moved[slot].is_some() || sways[slot] {
                return None;
            }
            let index = in_slots[slot];
            let ((mesh, cut_out), gpu, transform, mask) = wanted[index];
            if gpu.index_count / 3 > GROUPED_TRIANGLES || mask == MASK_SEE_THROUGH {
                return None;
            }
            let wide = gpu.index_format == wgpu::IndexFormat::Uint32;
            let cell = bundle_cell(transform.w_axis.truncate(), distances[index]);
            Some((
                (cut_out, mask, wide, cell),
                item_print(mesh, gpu, cut_out, &transform),
            ))
        };
        let keys: Vec<Option<(GroupKey, u64)>> = if super::parallel::worth_sharing(slots.len()) {
            super::parallel::shared(|| {
                super::parallel::map_parts(&slots, |part| {
                    part.iter().map(key_of).collect::<Vec<_>>()
                })
            })
            .into_iter()
            .flatten()
            .collect()
        } else {
            slots.iter().map(key_of).collect()
        };
        // Each group's slots and fingerprint, the groups as first met.
        // Draws of a square mostly come together, so the last group found
        // is asked first.
        let mut groups: Vec<(GroupKey, Vec<usize>, u64)> = Vec::new();
        let mut group_of: FxHashMap<GroupKey, usize> = FxHashMap::default();
        let mut last: Option<(GroupKey, usize)> = None;
        for (slot, found) in keys.into_iter().enumerate() {
            let Some((key, print)) = found else {
                continue;
            };
            let at = match last {
                Some((last_key, at)) if last_key == key => at,
                _ => *group_of.entry(key).or_insert_with(|| {
                    groups.push((key, Vec::new(), 0));
                    groups.len() - 1
                }),
            };
            last = Some((key, at));
            let group = &mut groups[at];
            group.1.push(slot);
            group.2 = fold_print(group.2, print);
        }
        // The nearest squares first (a square's size grows with its
        // distance), so a budget that runs out leaves the far ones waiting.
        groups.sort_unstable_by_key(|(key, _, _)| (key.3[0], *key));
        let mut budget = GROUP_TRIANGLES_PER_FRAME;
        let mut members: Vec<Member<'_>> = Vec::new();
        for (key, slots, print) in groups {
            if slots.len() < MIN_GROUP {
                continue;
            }
            let traced = self.groups.keep(&key, (slots.len(), print), frame) || {
                members.clear();
                members.extend(slots.iter().map(|&slot| {
                    let ((_, cut_out), gpu, transform, _) = wanted[in_slots[slot]];
                    (gpu, cut_out, rows_3x4(transform))
                }));
                self.groups
                    .build((device, queue), key, (&members, print), &mut budget, frame)
            };
            if traced {
                for slot in slots {
                    grouped[slot] = Some(key);
                }
            }
        }
        grouped
    }
}

impl ScenePipeline {
    /// Whether anything this frame traces rays on a device that can: a ray
    /// traced effect drawn this frame (`effects`: sun shadows, ambient
    /// occlusion) or the editor's Ray Hits or Path Traced view.
    pub(super) fn traces_rays(&self, effects: bool) -> bool {
        if !self.graphics.ray_queries {
            return false;
        }
        effects || self.ray_traced_view(false)
    }

    /// Whether some frame may trace rays with these settings: a hardware
    /// ray traced effect is on, or the editor shows a ray traced view.
    /// While so, a frame that traces nothing parks the ray traced scene
    /// rather than freeing it (ADR-0025 keeps it while an effect is on).
    pub(super) fn may_trace_rays(&self) -> bool {
        use crate::{
            RayTracedAmbientOcclusion, RayTracedGlobalIllumination, RayTracedReflections,
            RayTracedShadows,
        };
        let graphics = &self.graphics;
        graphics.ray_queries
            && (graphics.ray_traced_shadows != RayTracedShadows::Off
                || graphics.ray_traced_ambient_occlusion != RayTracedAmbientOcclusion::Off
                || graphics.ray_traced_reflections != RayTracedReflections::Off
                || (graphics.ray_traced_global_illumination != RayTracedGlobalIllumination::Off
                    && !graphics.ray_traced_gi_in_software)
                || self.ray_traced_view(false))
    }

    /// Whether the editor's Path Traced view is shown, which needs each
    /// slot's [`SurfaceRaw`].
    pub(super) fn path_traced(&self) -> bool {
        self.graphics.ray_queries && self.ray_traced_view(true)
    }

    /// Whether the editor shows the Path Traced view, or (unless
    /// `path_traced_only`) the Ray Hits view.
    #[cfg(feature = "debug-views")]
    fn ray_traced_view(&self, path_traced_only: bool) -> bool {
        use super::DebugView;
        self.debug_views.as_ref().is_some_and(|debug| {
            debug.view == DebugView::PathTraced
                || (!path_traced_only && debug.view == DebugView::RayHits)
        })
    }

    #[cfg(not(feature = "debug-views"))]
    fn ray_traced_view(&self, _path_traced_only: bool) -> bool {
        false
    }
}

/// Makes, updates, parks or frees `scene` (a [`ScenePipeline`]'s) for this
/// frame's `draws`, seen from `camera`. When nothing `traces`, it's moved
/// to `parked` while some frame still `may` trace (its meshes' BLASes are
/// what a first frame spends seconds building), and freed with `parked`
/// otherwise; the next frame that traces takes it back. With `surfaces`
/// (ray traced GI), each slot's [`SurfaceRaw`] is kept too. Returns
/// whether it recorded anything (for the GPU timer's Acceleration
/// structures pass). Takes the pipeline's fields, not the pipeline, so the
/// frame's other borrows of it can stay.
pub(super) fn prepare_scene(
    (scene, parked, may): (&mut Option<Box<RayScene>>, &mut Option<Box<RayScene>>, bool),
    pipeline: (
        (&wgpu::Device, &wgpu::Queue),
        &super::Meshes,
        &FxHashMap<u64, super::Cached<GpuMesh>>,
    ),
    encoder: &mut wgpu::CommandEncoder,
    (traces, frame, camera, reach, surfaces, clock): (
        bool,
        u64,
        (Vec3, Sight),
        f32,
        bool,
        [f32; 4],
    ),
    draws: (&[Draw], &[&Cluster], &Posing),
) -> bool {
    let ((device, queue), meshes, custom_meshes) = pipeline;
    if !traces {
        if may {
            if let Some(idle) = scene.take() {
                *parked = Some(idle);
            }
        } else {
            *scene = None;
            *parked = None;
        }
        return false;
    }
    if scene.is_none() {
        *scene = parked.take();
    }
    let scene = scene.get_or_insert_with(|| Box::new(RayScene::new(device)));
    scene.prepare(
        (device, queue),
        encoder,
        (frame, camera, reach, surfaces, clock),
        draws,
        |key| gpu_mesh_in(meshes, custom_meshes, key),
    );
    scene.stats.rebuilt || scene.stats.meshes > 0
}

impl Surfaces {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let capacity = 256;
        Self {
            buffer: surface_buffer(device, capacity),
            capacity,
            written: Vec::new(),
            texture_colors: FxHashMap::default(),
        }
    }

    /// Writes `raw` (one a slot) to the GPU if it changed, growing the
    /// buffer to fit.
    pub(super) fn write(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raw: Vec<SurfaceRaw>,
    ) {
        if raw.len() > self.capacity {
            self.capacity = raw.len().next_power_of_two();
            self.buffer = surface_buffer(device, self.capacity);
            self.written.clear();
        }
        if raw != self.written && !raw.is_empty() {
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(&raw));
        }
        self.written = raw;
    }
}

fn surface_buffer(device: &wgpu::Device, slots: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("ray traced surfaces"),
        size: (slots * size_of::<SurfaceRaw>()) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// What a ray hitting an item of `material` finds: its color (times its
/// base color texture's average, worked out once a texture into
/// `texture_colors`) and its glow, in linear light.
pub(super) fn surface_of(
    material: &Material,
    texture_colors: &mut FxHashMap<u64, [f32; 3]>,
) -> SurfaceRaw {
    let mut color = super::lighting::linear_color(material.base_color);
    if let Some(texture) = &material.base_color_texture {
        let average = *texture_colors
            .entry(texture.key())
            .or_insert_with(|| average_color(texture.rgba()));
        color *= Vec3::from(average);
    }
    let glow = super::lighting::linear_color(material.emissive);
    let cut_out = if material.alpha_cutoff > 0.0 {
        1.0
    } else {
        0.0
    };
    [
        color.extend(cut_out).to_array(),
        glow.extend(0.0).to_array(),
    ]
}

/// The average of sRGB `rgba` pixels in linear light, from at most about
/// 4,096 of them spread over the picture. Mid grey if there are none.
fn average_color(rgba: &[u8]) -> [f32; 3] {
    let pixels = rgba.len() / 4;
    if pixels == 0 {
        return [0.5; 3];
    }
    let step = (pixels / 4096).max(1);
    let mut sum = Vec3::ZERO;
    let mut count = 0.0;
    for pixel in rgba.chunks_exact(4).step_by(step) {
        let srgb = [pixel[0], pixel[1], pixel[2]].map(|c| f32::from(c) / 255.0);
        sum += super::lighting::linear_color(srgb);
        count += 1.0;
    }
    (sum / count).to_array()
}

/// Which kinds of ray see an item (the instance mask): see the `MASK_*`s.
pub(super) fn mask_of(kind: RayKind, casts_shadow: bool) -> u8 {
    if kind.see_through {
        return MASK_SEE_THROUGH;
    }
    match (casts_shadow, kind.cut_out) {
        (true, true) => MASK_CUT_OUT,
        (true, false) => MASK_SOLID,
        (false, true) => MASK_NO_SHADOW_CUT_OUT,
        (false, false) => MASK_NO_SHADOW,
    }
}

/// `transform`'s top three rows, as wgpu's TLAS instances take it.
fn rows_3x4(transform: Mat4) -> [f32; 12] {
    let rows = transform.transpose().to_cols_array();
    rows[..12].try_into().unwrap()
}

/// A BLAS's size for `gpu`: opaque, unless it's for a `cut_out`, whose
/// triangles rays that don't force opacity see as candidates to test
/// against the texture's holes.
pub(super) fn size_of_mesh(
    gpu: &GpuMesh,
    cut_out: bool,
) -> wgpu::BlasTriangleGeometrySizeDescriptor {
    wgpu::BlasTriangleGeometrySizeDescriptor {
        // `Vertex::position` comes first in each corner.
        vertex_format: wgpu::VertexFormat::Float32x3,
        vertex_count: gpu.vertex_count,
        index_format: Some(gpu.index_format),
        index_count: Some(gpu.index_count),
        flags: if cut_out {
            wgpu::AccelerationStructureGeometryFlags::empty()
        } else {
            wgpu::AccelerationStructureGeometryFlags::OPAQUE
        },
    }
}

fn create_blas(
    device: &wgpu::Device,
    size: &wgpu::BlasTriangleGeometrySizeDescriptor,
) -> wgpu::Blas {
    device.create_blas(
        &wgpu::CreateBlasDescriptor {
            label: Some("mesh BLAS"),
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        },
        wgpu::BlasGeometrySizeDescriptors::Triangles {
            descriptors: vec![size.clone()],
        },
    )
}

fn create_tlas(device: &wgpu::Device, capacity: u32) -> wgpu::Tlas {
    device.create_tlas(&wgpu::CreateTlasDescriptor {
        label: Some("ray traced scene"),
        max_instances: capacity,
        flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
        update_mode: wgpu::AccelerationStructureUpdateMode::Build,
    })
}

/// Usage a mesh's buffers need to be built into a BLAS on `device`, and
/// read (as storage) for a cut-out's texture coordinates
/// (`ray_cut_outs.rs`).
/// How far `item` (drawn with `gpu`) is from `camera` at its nearest, if
/// that's within ray tracing's `reach`. A posed item is bounded by where
/// its joints move it; an item whose bounds aren't a number is out.
pub(super) fn within_reach(
    item: &DrawItem,
    gpu: &GpuMesh,
    camera: Vec3,
    reach: f32,
) -> Option<f32> {
    let (center, radius) = item_sphere(item, gpu);
    let distance = center.distance(camera) - radius;
    (distance <= reach).then_some(distance)
}

/// The sphere around `item` (drawn with `gpu`) in the world: its middle and
/// radius. A posed item is bounded by where its joints put it.
fn item_sphere(item: &DrawItem, gpu: &GpuMesh) -> (Vec3, f32) {
    let bounds = match (&item.skin, &gpu.skin) {
        (Some(skin), Some(gpu_skin)) => gpu_skin.bounds(skin),
        _ => gpu.bounds,
    };
    bounds.sphere(item.transform)
}

pub(super) fn blas_input(device: &wgpu::Device) -> wgpu::BufferUsages {
    if device
        .features()
        .contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY)
    {
        wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE
    } else {
        wgpu::BufferUsages::empty()
    }
}

/// The editor's Ray Hits view: what the camera's rays hit in the ray traced
/// scene, each item a color of its own, its triangles alternately lighter
/// and darker, dimmer the farther away (`ray_hits.wgsl`).
#[cfg(feature = "debug-views")]
pub(super) struct RayHitsPass {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
}

/// Must match `RayView` in ray_hits.wgsl.
#[cfg(feature = "debug-views")]
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct RayViewUniform {
    inv_view_proj: [[f32; 4]; 4],
    /// The output's width and height, the ray tracing distance, unused.
    size: [f32; 4],
}

#[cfg(feature = "debug-views")]
impl RayHitsPass {
    pub(super) fn new(device: &wgpu::Device, color_format: wgpu::TextureFormat) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ray hits"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::AccelerationStructure {
                        vertex_return: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ray hits"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader =
            super::gpu::shader_module(device, "ray hits", include_str!("../ray_hits.wgsl"));
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ray hits"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_ray_hits"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_ray_hits"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: color_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ray hits"),
            size: size_of::<RayViewUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            layout,
            pipeline,
            uniform,
        }
    }

    /// Draws the view over `target`, from `view_proj`.
    pub(super) fn draw(
        &self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        (scene, view_proj, size, reach): (&RayScene, Mat4, (u32, u32), f32),
    ) {
        let uniform = RayViewUniform {
            inv_view_proj: view_proj.inverse().to_cols_array_2d(),
            size: [size.0 as f32, size.1 as f32, reach, 0.0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray hits"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: scene.tlas().as_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.uniform.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ray hits"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}
