//! Swaying plants in the ray traced scene (`docs/rendering/ray-tracing.md`):
//! a swaying item's mesh is bent on the GPU each frame by the same wind
//! `vs_main` and `vs_shadow` bend it by (`ray_sway.wgsl`, sway.wgsl's
//! `swayed_at`) and built into a BLAS, so ray traced shadows, ambient
//! occlusion, GI and the fog's shafts move with the leaves.
//!
//! - Only while the wind blows: with none, every plant is at rest and uses
//!   its mesh's shared BLAS.
//! - The nearest swaying items first, up to [`BENT_TRIANGLES`] triangles a
//!   frame; the rest keep the rest pose.
//! - wgpu has no refit, so bent copies are built afresh every frame, and a
//!   meadow bends thousands of small ones. What costs is each BLAS, each
//!   dispatch and each buffer written, not the triangles (a BLAS of its own
//!   for each of 4,000 clumps took 13 ms of the GPU's frame and 20 ms of
//!   the frame's thread), so they share: every copy is bent into one
//!   buffer where it stands in the world, a mesh's copies in one dispatch,
//!   and built as the geometries of one BLAS a [`BundleKey`].
//!
//! Posed (skinned or morphed) characters get a BLAS of their own each,
//! built straight from the frame's posed corners (CO8, `posing.rs`): their
//! joints, identity shapes and expressions as every pass draws them,
//! nearest first within [`POSED_TRIANGLES`]. A character past that budget
//! is left out of the ray traced scene (its rest pose would cast a T-shaped
//! shadow).

use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use rustc_hash::FxHashMap;

use super::gpu::{self, shader_module};
use super::{GpuMesh, MeshKey};
use crate::draw_list::DrawItem;

/// The most triangles bent and built into BLASes in one frame, nearest
/// items first: about 2 ms on an RX 9070 XT, as 4,000 small plants or as a
/// few big ones (`docs/rendering/performance.md`).
pub(super) const BENT_TRIANGLES: u64 = 400_000;

/// The most triangles of posed (skinned or morphed) items built into
/// BLASes in one frame, nearest first: about 0.8 ms on an RX 9070 XT. A
/// budget of their own, so a windy forest never takes the players' shadows.
pub(super) const POSED_TRIANGLES: u64 = 300_000;

/// What nothing has bent or posed for this many frames is freed.
const FORGET_AFTER: u64 = 120;

/// Which posed item's BLAS: the mesh, whether it's for a cut-out, and how
/// many items of that mesh were posed before it this frame.
pub(super) type PosedKey = (MeshKey, bool, u32);

/// Which BLAS of bent copies: whether they're cut-outs (their triangles
/// aren't opaque to rays), their items' TLAS mask, and whether their
/// meshes' indices are 32 bits wide (what a BLAS's geometries must share,
/// or its one TLAS instance), then which square of the ground they stand
/// on ([`bundle_cell`]).
pub(super) type BundleKey = (bool, u8, bool, [i32; 3]);

/// The square of the ground a bent item at `position`, `distance` studs
/// from the camera, is bundled by: its side (a power of two, as an
/// exponent) and where it is in the world, in sides. A square is as wide
/// as its items are far away, to the power of two below (8 studs at the
/// least), so a view's bent items share a hundred or two however far they
/// spread: few enough BLASes to build quickly, each small enough that rays
/// elsewhere pass it by (one BLAS for every bent plant built a little
/// faster and traced a quarter slower). Squares stay where they are in the
/// world, so as the camera moves an item changes bundle only as it comes
/// twice as near or goes twice as far.
pub(super) fn bundle_cell(position: glam::Vec3, distance: f32) -> [i32; 3] {
    let exponent = distance.max(1.0).log2().floor().clamp(3.0, 20.0) as i32;
    let side = 2.0_f32.powi(exponent);
    [
        exponent,
        (position.x / side).floor() as i32,
        (position.z / side).floor() as i32,
    ]
}

type GeometrySize = wgpu::BlasTriangleGeometrySizeDescriptor;

/// A swaying item to bend this frame: its mesh, where it stands and its
/// [`bending`].
pub(super) type Swaying<'a> = (MeshKey, &'a GpuMesh, Mat4, [f32; 4]);

/// One swaying item's bending. Must match `BentItem` in ray_sway.wgsl: the
/// rows of its 3 by 4 object-to-world matrix, then `InstanceRaw::sway`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct ItemRaw {
    model: [[f32; 4]; 3],
    sway: [f32; 4],
}

/// One mesh's copies, bent by one dispatch. Must match `BentRun` in
/// ray_sway.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
struct RunRaw {
    /// As `Frame::clock`: the time, then the wind.
    clock: [f32; 4],
    /// How many corners a copy has, how many f32s a corner of the mesh
    /// takes, the first copy's item, and its first corner among the bent
    /// positions.
    counts: [u32; 4],
    /// How many copies, 1 if the mesh has foliage data (bound with its
    /// corners); the rest is unused.
    copies: [u32; 4],
}

/// A swaying item is bent for rays only if the wind moves it at least
/// this many pixels on screen. Under that its shadow, and what it hides
/// from the sky, land on the pixels they would at rest, and such items are
/// most of a wide view's: a meadow's clumps 200 studs away move a third of
/// a pixel.
pub(super) const SEEN_SWAY_PIXELS: f32 = 1.0;

/// How big things look to the frame's camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sight {
    /// How many pixels a stud covers one stud from the camera.
    per_stud: f32,
}

impl Sight {
    /// The sight of a camera with `view_proj` (a perspective one, as every
    /// [`crate::Camera`] is), drawn `height` pixels tall.
    pub(super) fn of(view_proj: Mat4, height: u32) -> Self {
        Self {
            per_stud: view_proj.row(1).truncate().length() * height as f32 * 0.5,
        }
    }

    /// How many pixels a length of `studs` covers at most, `distance`
    /// studs from the camera.
    pub(super) fn pixels(&self, studs: f32, distance: f32) -> f32 {
        studs * self.per_stud / distance.max(1e-3)
    }
}

/// The `InstanceRaw::sway` of `item` (without its decal role) if it moves
/// in the wind of `clock` (`Frame::clock`), with the farthest that wind
/// moves it in studs, or `None` if it's still.
pub(super) fn bending(item: &DrawItem, clock: [f32; 4]) -> Option<([f32; 4], f32)> {
    let sway = item.sway.filter(|sway| !sway.is_still())?;
    let level_wind = glam::Vec2::new(clock[1], clock[3]);
    if !level_wind.is_finite() || level_wind.length() < 1e-3 {
        return None;
    }
    Some((
        [
            sway.base,
            1.0 / sway.height,
            sway.strength,
            sway.flutter.clamp(0.0, 0.95),
        ],
        sway.reach_in(level_wind.length()),
    ))
}

/// One mesh's copies this frame, bent by one dispatch.
struct Run {
    mesh: MeshKey,
    /// The items of that mesh, in the frame's order.
    items: Vec<usize>,
    /// The first one's first corner among the bent positions; the others
    /// follow it.
    first_corner: u32,
}

/// Where a frame's swaying items are bent.
struct Runs {
    /// One a mesh, in the order the meshes first come.
    runs: Vec<Run>,
    /// Each item's first corner among the bent positions.
    first_corners: Vec<u32>,
    /// How many corners there are in all.
    corners: u32,
}

/// Where each of `meshes` (a swaying item's mesh each, with how many
/// corners it has) is bent this frame.
fn runs_of(meshes: &[(MeshKey, u32)]) -> Runs {
    let mut runs: Vec<Run> = Vec::new();
    let mut run_of: FxHashMap<MeshKey, usize> = FxHashMap::default();
    for (item, (mesh, _)) in meshes.iter().enumerate() {
        let run = *run_of.entry(*mesh).or_insert_with(|| {
            runs.push(Run {
                mesh: *mesh,
                items: Vec::new(),
                first_corner: 0,
            });
            runs.len() - 1
        });
        runs[run].items.push(item);
    }
    let mut first_corners = vec![0; meshes.len()];
    let mut corners = 0_u32;
    for run in &mut runs {
        run.first_corner = corners;
        for &item in &run.items {
            first_corners[item] = corners;
            corners += meshes[item].1;
        }
    }
    Runs {
        runs,
        first_corners,
        corners,
    }
}

/// A posed item's BLAS, built each frame from the frame's posed corners
/// from `first` on (in the item's own space).
struct Posed {
    first: u32,
    blas: wgpu::Blas,
    size: GeometrySize,
    /// The mesh's version it was made for.
    version: u64,
    last_used: u64,
    /// Posed this frame.
    used: bool,
}

/// One BLAS of bent copies, each a geometry of it.
struct Bundle {
    blas: wgpu::Blas,
    /// What the BLAS was made to hold, geometry by geometry: a frame's
    /// copies fit while there are no more of them and none has more
    /// corners or indices than its place has room for.
    room: Vec<GeometrySize>,
    /// This frame's copies in geometry order: each one's first corner
    /// among the bent positions and its mesh's size.
    copies: Vec<(u32, GeometrySize)>,
    last_used: u64,
}

/// What bends one mesh's copies: its corners bound with the shared buffers.
struct Group {
    group: wgpu::BindGroup,
    /// The mesh's version and the shared buffers it was made for.
    version: u64,
    buffers: u64,
    last_used: u64,
}

/// A buffer that grows to hold what it's given.
struct Grown {
    buffer: wgpu::Buffer,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Grown {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        Self {
            buffer: Self::make(device, label, usage, 1024),
            label,
            usage,
        }
    }

    fn make(
        device: &wgpu::Device,
        label: &'static str,
        usage: wgpu::BufferUsages,
        size: u64,
    ) -> wgpu::Buffer {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    }

    /// Makes room for `size` bytes. Returns whether the buffer is a new
    /// one (what it held is gone, and whatever bound it must bind it again).
    fn fit(&mut self, device: &wgpu::Device, size: u64) -> bool {
        if size <= self.buffer.size() {
            return false;
        }
        self.buffer = Self::make(device, self.label, self.usage, size.next_power_of_two());
        true
    }
}

/// The bent copies of swaying items and the BLASes of posed characters,
/// and the pass that bends swaying items.
pub(super) struct BentMeshes {
    pipeline: wgpu::ComputePipeline,
    layout: wgpu::BindGroupLayout,
    posed: FxHashMap<PosedKey, Posed>,
    bundles: FxHashMap<BundleKey, Bundle>,
    /// Every copy's bent corners, three f32s each, in the world.
    positions: Grown,
    /// Every bent item's [`ItemRaw`], a run's together.
    items: Grown,
    /// What `items` holds, so a still view writes nothing.
    items_written: Vec<ItemRaw>,
    /// Each run's [`RunRaw`], `run_stride` bytes apart.
    runs: Grown,
    run_stride: u64,
    /// Counts the shared buffers made, so a [`Group`] knows its own.
    buffers: u64,
    groups: FxHashMap<MeshKey, Group>,
    /// This frame's dispatches: a run's mesh, its place in `runs`, and how
    /// many corners it bends.
    dispatches: Vec<(MeshKey, u32, u64)>,
    /// How many items were bent this frame.
    bent: u32,
    /// Bound in a mesh's foliage data's place when it has none.
    no_foliage: wgpu::Buffer,
}

/// Whether a BLAS made with `room` can be built from `copies`.
fn fits(room: &[GeometrySize], copies: &[(u32, GeometrySize)]) -> bool {
    copies.len() <= room.len()
        && room.iter().zip(copies).all(|(room, (_, size))| {
            size.vertex_count <= room.vertex_count && size.index_count <= room.index_count
        })
}

/// The room of a new BLAS for `copies`, which the one made with `old`
/// can't hold: each place at least as big as it was and as its copy is, so
/// that as the camera moves and the copies change the BLAS is made again
/// less and less often, and a third more places than copies. If that grew
/// past twice the triangles a frame bends, only what `copies` needs.
fn room_for(old: &[GeometrySize], copies: &[(u32, GeometrySize)]) -> Vec<GeometrySize> {
    let Some((_, last)) = copies.last() else {
        return Vec::new();
    };
    let places = copies.len() + copies.len() / 3 + 1;
    let needed = |place: usize| copies.get(place).map_or(last, |(_, size)| size);
    let fresh: Vec<GeometrySize> = (0..places).map(|place| needed(place).clone()).collect();
    let grown: Vec<GeometrySize> = (0..places.max(old.len()))
        .map(|place| {
            let needed = needed(place);
            let old = old.get(place).unwrap_or(needed);
            GeometrySize {
                vertex_count: needed.vertex_count.max(old.vertex_count),
                index_count: needed.index_count.max(old.index_count),
                ..needed.clone()
            }
        })
        .collect();
    let held = |room: &[GeometrySize]| -> u64 {
        room.iter()
            .map(|size| u64::from(size.index_count.unwrap_or(0) / 3))
            .sum()
    };
    if held(&grown) > 2 * BENT_TRIANGLES.max(held(&fresh)) {
        fresh
    } else {
        grown
    }
}

fn buffer_entry(binding: u32, ty: wgpu::BufferBindingType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty,
            // Only the runs: each dispatch reads its own.
            has_dynamic_offset: ty == wgpu::BufferBindingType::Uniform,
            min_binding_size: None,
        },
        count: None,
    }
}

fn compute_pipeline(
    device: &wgpu::Device,
    label: &str,
    layout: &wgpu::BindGroupLayout,
    code: &'static str,
    entry: &str,
) -> wgpu::ComputePipeline {
    let shader = shader_module(device, label, code);
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(
            &device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            }),
        ),
        module: &shader,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    })
}

impl BentMeshes {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bent meshes"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, storage(true)),
                buffer_entry(2, storage(false)),
                buffer_entry(3, storage(true)),
                buffer_entry(4, storage(true)),
            ],
        });
        let pipeline = compute_pipeline(
            device,
            "bent meshes",
            &layout,
            gpu::RAY_SWAY_SHADER,
            "cs_bend",
        );
        let written = wgpu::BufferUsages::COPY_DST;
        let run_stride = u64::from(device.limits().min_uniform_buffer_offset_alignment)
            .max(size_of::<RunRaw>() as u64)
            .next_power_of_two();
        Self {
            pipeline,
            layout,
            posed: FxHashMap::default(),
            bundles: FxHashMap::default(),
            positions: Grown::new(
                device,
                "bent positions",
                wgpu::BufferUsages::STORAGE | super::ray_tracing::blas_input(device),
            ),
            items: Grown::new(device, "bent items", wgpu::BufferUsages::STORAGE | written),
            items_written: Vec::new(),
            runs: Grown::new(device, "bent runs", wgpu::BufferUsages::UNIFORM | written),
            run_stride,
            buffers: 0,
            groups: FxHashMap::default(),
            dispatches: Vec::new(),
            bent: 0,
            no_foliage: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("no foliage corners"),
                size: 16,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
        }
    }

    /// Starts a frame: nothing is bent or posed yet.
    pub(super) fn begin(&mut self) {
        for posed in self.posed.values_mut() {
            posed.used = false;
        }
        for bundle in self.bundles.values_mut() {
            bundle.copies.clear();
        }
        self.dispatches.clear();
        self.bent = 0;
    }

    /// Bends this frame's `swaying` items in the wind of `clock`
    /// (`Frame::clock`). Returns each one's first corner among the bent
    /// positions, which [`BentMeshes::bundle`] builds it from.
    pub(super) fn bend(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        clock: [f32; 4],
        swaying: &[Swaying<'_>],
        frame: u64,
    ) -> Vec<u32> {
        if swaying.is_empty() {
            return Vec::new();
        }
        let meshes: Vec<(MeshKey, u32)> = swaying
            .iter()
            .map(|(key, gpu, _, _)| (*key, gpu.vertex_count))
            .collect();
        let Runs {
            runs,
            first_corners,
            corners,
        } = runs_of(&meshes);
        self.bent = swaying.len() as u32;

        let mut items = Vec::with_capacity(swaying.len());
        let mut raw_runs = vec![0_u8; runs.len() * self.run_stride as usize];
        for (index, run) in runs.iter().enumerate() {
            let (_, gpu, _, _) = swaying[run.items[0]];
            let raw = RunRaw {
                clock,
                counts: [
                    gpu.vertex_count,
                    (size_of::<crate::mesh::Vertex>() / 4) as u32,
                    items.len() as u32,
                    run.first_corner,
                ],
                copies: [
                    run.items.len() as u32,
                    u32::from(gpu.foliage.is_some()),
                    0,
                    0,
                ],
            };
            let at = index * self.run_stride as usize;
            raw_runs[at..at + size_of::<RunRaw>()].copy_from_slice(bytemuck::bytes_of(&raw));
            items.extend(run.items.iter().map(|&item| {
                let (_, _, transform, sway) = swaying[item];
                let rows = transform.transpose().to_cols_array_2d();
                ItemRaw {
                    model: [rows[0], rows[1], rows[2]],
                    sway,
                }
            }));
        }

        // The shared buffers, and each mesh's corners bound with them.
        let positions = self.positions.fit(device, u64::from(corners) * 12);
        let grew = self
            .items
            .fit(device, (items.len() * size_of::<ItemRaw>()) as u64);
        if grew {
            self.items_written.clear();
        }
        let runs_grew = self.runs.fit(device, raw_runs.len() as u64);
        if positions || grew || runs_grew {
            self.buffers += 1;
        }
        if items != self.items_written {
            queue.write_buffer(&self.items.buffer, 0, bytemuck::cast_slice(&items));
            self.items_written = items;
        }
        // The clock moves every frame.
        queue.write_buffer(&self.runs.buffer, 0, &raw_runs);
        for (index, run) in runs.iter().enumerate() {
            let key = &run.mesh;
            let (_, gpu, _, _) = swaying[run.items[0]];
            let kept = self
                .groups
                .get(key)
                .is_some_and(|group| group.version == gpu.version && group.buffers == self.buffers);
            if !kept {
                let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("bent meshes"),
                    layout: &self.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                buffer: &self.runs.buffer,
                                offset: 0,
                                size: wgpu::BufferSize::new(size_of::<RunRaw>() as u64),
                            }),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: gpu.vertices.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: self.positions.buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: self.items.buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            // A mesh without foliage data reads none.
                            resource: gpu
                                .foliage
                                .as_ref()
                                .unwrap_or(&self.no_foliage)
                                .as_entire_binding(),
                        },
                    ],
                });
                self.groups.insert(
                    *key,
                    Group {
                        group,
                        version: gpu.version,
                        buffers: self.buffers,
                        last_used: frame,
                    },
                );
            }
            if let Some(group) = self.groups.get_mut(key) {
                group.last_used = frame;
            }
            let corners = u64::from(gpu.vertex_count) * run.items.len() as u64;
            self.dispatches
                .push((*key, (index as u64 * self.run_stride) as u32, corners));
        }
        first_corners
    }

    /// Makes this frame's bent `copies` (each one's first corner among the
    /// bent positions, from [`BentMeshes::bend`], and its mesh's size) the
    /// geometries of `key`'s BLAS, in that order, making the BLAS if it
    /// has none or one they don't fit.
    pub(super) fn bundle(
        &mut self,
        device: &wgpu::Device,
        key: BundleKey,
        copies: Vec<(u32, GeometrySize)>,
        frame: u64,
    ) {
        let old = self
            .bundles
            .get(&key)
            .map_or(&[][..], |bundle| &bundle.room);
        if !fits(old, &copies) {
            let room = room_for(old, &copies);
            let blas = device.create_blas(
                &wgpu::CreateBlasDescriptor {
                    label: Some("bent BLAS"),
                    flags: wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD,
                    update_mode: wgpu::AccelerationStructureUpdateMode::Build,
                },
                wgpu::BlasGeometrySizeDescriptors::Triangles {
                    descriptors: room.clone(),
                },
            );
            self.bundles.insert(
                key,
                Bundle {
                    blas,
                    room,
                    copies: Vec::new(),
                    last_used: frame,
                },
            );
        }
        let bundle = self.bundles.get_mut(&key).expect("made above");
        bundle.copies = copies;
        bundle.last_used = frame;
    }

    /// Builds `key`'s BLAS this frame from the frame's posed corners from
    /// `first` on (`gpu`'s, posed by `posing.rs`), making it if it has none
    /// (or one for an older version of the mesh).
    pub(super) fn pose(
        &mut self,
        device: &wgpu::Device,
        key: PosedKey,
        gpu: &GpuMesh,
        size: GeometrySize,
        first: u32,
        frame: u64,
    ) {
        let kept = self
            .posed
            .get(&key)
            .is_some_and(|posed| posed.version == gpu.version);
        if !kept {
            let blas = device.create_blas(
                &wgpu::CreateBlasDescriptor {
                    label: Some("posed BLAS"),
                    flags: wgpu::AccelerationStructureFlags::PREFER_FAST_BUILD,
                    update_mode: wgpu::AccelerationStructureUpdateMode::Build,
                },
                wgpu::BlasGeometrySizeDescriptors::Triangles {
                    descriptors: vec![size.clone()],
                },
            );
            self.posed.insert(
                key,
                Posed {
                    first,
                    blas,
                    size,
                    version: gpu.version,
                    last_used: frame,
                    used: false,
                },
            );
        }
        let posed = self.posed.get_mut(&key).expect("made above");
        posed.first = first;
        posed.last_used = frame;
        posed.used = true;
    }

    /// Records this frame's bending, before the BLASes are built (posed
    /// items were posed by the posing pass, earlier in the frame).
    pub(super) fn record(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.dispatches.is_empty() {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bent meshes"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        for (mesh, run, corners) in &self.dispatches {
            let Some(group) = self.groups.get(mesh) else {
                continue;
            };
            let groups = corners.div_ceil(64) as u32;
            pass.set_bind_group(0, &group.group, &[*run]);
            pass.dispatch_workgroups(groups.min(65_535), groups.div_ceil(65_535), 1);
        }
    }

    /// The BLAS of the posed item `key`.
    pub(super) fn posed_blas(&self, key: &PosedKey) -> Option<&wgpu::Blas> {
        Some(&self.posed.get(key)?.blas)
    }

    /// The BLAS of the bundle `key`, if it has copies this frame.
    pub(super) fn bundle_blas(&self, key: &BundleKey) -> Option<&wgpu::Blas> {
        let bundle = self.bundles.get(key)?;
        (!bundle.copies.is_empty()).then_some(&bundle.blas)
    }

    /// What builds the posed item `key`'s BLAS from the frame's `posed`
    /// corners and `gpu`'s triangles.
    pub(super) fn posed_entry<'a>(
        &'a self,
        key: &PosedKey,
        gpu: &'a GpuMesh,
        posed: &'a wgpu::Buffer,
    ) -> Option<wgpu::BlasBuildEntry<'a>> {
        let item = self.posed.get(key)?;
        Some(wgpu::BlasBuildEntry {
            blas: &item.blas,
            geometry: wgpu::BlasGeometries::TriangleGeometries(vec![wgpu::BlasTriangleGeometry {
                size: &item.size,
                // A posed corner's position is its first three words.
                vertex_buffer: posed,
                first_vertex: item.first,
                vertex_stride: super::posing::POSED_STRIDE,
                index_buffer: Some(&gpu.indices),
                first_index: Some(0),
                transform_buffer: None,
                transform_buffer_offset: None,
            }]),
        })
    }

    /// What builds the bundle `key`'s BLAS from its copies' bent positions
    /// and their meshes' triangles (`meshes`, in the copies' order).
    pub(super) fn bundle_entry<'a>(
        &'a self,
        key: &BundleKey,
        meshes: &[&'a GpuMesh],
    ) -> Option<wgpu::BlasBuildEntry<'a>> {
        let bundle = self.bundles.get(key)?;
        // A geometry left out would move the ones after it to other slots.
        if bundle.copies.is_empty() || bundle.copies.len() != meshes.len() {
            return None;
        }
        let geometries = bundle
            .copies
            .iter()
            .zip(meshes)
            .map(|((first, size), gpu)| wgpu::BlasTriangleGeometry {
                size,
                vertex_buffer: &self.positions.buffer,
                first_vertex: *first,
                vertex_stride: 12,
                index_buffer: Some(&gpu.indices),
                first_index: Some(0),
                transform_buffer: None,
                transform_buffer_offset: None,
            })
            .collect();
        Some(wgpu::BlasBuildEntry {
            blas: &bundle.blas,
            geometry: wgpu::BlasGeometries::TriangleGeometries(geometries),
        })
    }

    /// Frees what nothing has bent or posed for a while.
    pub(super) fn forget_unused(&mut self, frame: u64) {
        let recent = |last_used: u64| frame.saturating_sub(last_used) < FORGET_AFTER;
        self.posed.retain(|_, posed| recent(posed.last_used));
        self.bundles.retain(|_, bundle| recent(bundle.last_used));
        self.groups.retain(|_, group| recent(group.last_used));
    }

    /// How many swaying items were bent this frame.
    pub(super) fn bent_count(&self) -> u32 {
        self.bent
    }

    /// How many posed characters were moved into BLASes this frame.
    pub(super) fn posed_count(&self) -> u32 {
        self.posed.values().filter(|posed| posed.used).count() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_list::Sway;

    fn plant() -> DrawItem {
        DrawItem {
            shape: crate::Shape::Block,
            transform: Mat4::IDENTITY,
            material: Default::default(),
            uv_tile: 0.0,
            casts_shadow: true,
            id: 0,
            cull_distance: 0.0,
            skin: None,
            sway: Some(Sway {
                base: 2.0,
                height: 4.0,
                strength: 0.5,
                flutter: 1.5,
            }),
            decal_role: Default::default(),
        }
    }

    #[test]
    fn only_a_swaying_item_in_a_level_wind_is_bent() {
        let windy = [3.0, 10.0, 0.0, 0.0];
        assert_eq!(
            bending(&plant(), windy).map(|(sway, _)| sway),
            Some([2.0, 0.25, 0.5, 0.95]),
            "the flutter is kept under 1, as in `InstanceRaw::sway`"
        );
        assert_eq!(bending(&plant(), [3.0, 0.0, 0.0, 0.0]), None, "no wind");
        assert_eq!(
            bending(&plant(), [3.0, 0.0, 50.0, 0.0]),
            None,
            "a wind straight up bends nothing"
        );
        assert_eq!(bending(&plant(), [3.0, f32::NAN, 0.0, 0.0]), None);
        let still = DrawItem {
            sway: None,
            ..plant()
        };
        assert_eq!(bending(&still, windy), None);
    }

    #[test]
    fn a_stronger_wind_moves_a_plant_farther_up_to_its_reach() {
        let sway = plant().sway.unwrap();
        let moves = |speed: f32| bending(&plant(), [0.0, speed, 0.0, 0.0]).unwrap().1;
        assert!(moves(5.0) < moves(20.0) && moves(20.0) < moves(200.0));
        assert!((moves(1000.0) - sway.reach()).abs() < 1e-4);
        assert_eq!(sway.reach_in(0.0), 0.0);
    }

    #[test]
    fn sight_counts_the_pixels_a_length_covers_at_a_distance() {
        // 90 degrees tall, 1,000 pixels: a stud one stud away is 500,
        // wherever the camera looks and however wide the frame.
        let camera = crate::Camera {
            position: glam::Vec3::new(3.0, 20.0, -4.0),
            look_at: glam::Vec3::new(10.0, 0.0, 8.0),
            fov_y_degrees: 90.0,
        };
        for aspect in [1.0, 2.0] {
            let view_proj = super::super::lighting::camera_view_projection(&camera, aspect);
            let sight = Sight::of(view_proj, 1000);
            assert!((sight.pixels(1.0, 1.0) - 500.0).abs() < 0.01);
            assert!((sight.pixels(0.2, 100.0) - 1.0).abs() < 0.001);
            assert!(sight.pixels(0.01, 0.0) > 1000.0, "an item the camera is in");
        }
    }

    #[test]
    fn a_meshs_copies_are_bent_together_one_after_another() {
        let (a, b) = (MeshKey::Custom(7), MeshKey::Block);
        let laid = runs_of(&[(a, 10), (b, 24), (a, 10), (a, 10), (b, 24)]);
        let runs: Vec<_> = laid
            .runs
            .iter()
            .map(|run| (run.mesh, run.items.clone(), run.first_corner))
            .collect();
        assert_eq!(runs, [(a, vec![0, 2, 3], 0), (b, vec![1, 4], 30)]);
        assert_eq!(laid.first_corners, [0, 30, 10, 20, 54]);
        assert_eq!(laid.corners, 78);
        let none = runs_of(&[]);
        assert!(none.runs.is_empty() && none.first_corners.is_empty());
        assert_eq!(none.corners, 0);
    }

    fn size(vertex_count: u32, index_count: u32) -> GeometrySize {
        GeometrySize {
            vertex_format: wgpu::VertexFormat::Float32x3,
            vertex_count,
            index_format: Some(wgpu::IndexFormat::Uint32),
            index_count: Some(index_count),
            flags: wgpu::AccelerationStructureGeometryFlags::OPAQUE,
        }
    }

    fn sizes(room: &[GeometrySize]) -> Vec<(u32, u32)> {
        room.iter()
            .map(|size| (size.vertex_count, size.index_count.unwrap()))
            .collect()
    }

    #[test]
    fn a_bundles_blas_is_kept_while_the_frames_copies_fit_it() {
        let copies = vec![(0, size(40, 120)), (40, size(30, 90)), (70, size(30, 90))];
        assert!(!fits(&[], &copies), "no BLAS yet");
        let room = room_for(&[], &copies);
        assert_eq!(
            sizes(&room),
            [(40, 120), (30, 90), (30, 90), (30, 90), (30, 90)],
            "a third more places and one, each as big as the smallest copy"
        );
        assert!(fits(&room, &copies));
        // Fewer copies, smaller ones, or another of the smallest: kept.
        assert!(fits(&room, &copies[..1]));
        assert!(fits(&room, &[(0, size(8, 12)), (8, size(8, 12))]));
        let mut more = copies.clone();
        more.push((100, size(30, 90)));
        assert!(fits(&room, &more));
        assert!(fits(&room, &[]));

        // A bigger copy in a place, or too many: made again, no smaller
        // anywhere than it was.
        let bigger = vec![(0, size(40, 120)), (40, size(36, 60)), (76, size(10, 96))];
        assert!(!fits(&room, &bigger));
        assert_eq!(
            sizes(&room_for(&room, &bigger)),
            [(40, 120), (36, 90), (30, 96), (30, 96), (30, 96)]
        );
        let many = vec![(0, size(30, 90)); 6];
        assert!(!fits(&room, &many));
        let grown = room_for(&room, &many);
        assert_eq!(grown.len(), 9);
        assert_eq!(sizes(&grown)[0], (40, 120));
        assert!(fits(&grown, &many) && fits(&grown, &copies));
        assert!(room_for(&room, &[]).is_empty());
    }

    #[test]
    fn a_bundles_room_starts_over_once_it_holds_far_more_than_a_frame_bends() {
        // A BLAS that came to hold three frames' worth of triangles.
        let big = (BENT_TRIANGLES * 3) as u32;
        let old = vec![size(big, big * 3), size(30, 90)];
        let copies = vec![(0, size(40, 120)), (40, size(40, 120)), (80, size(40, 120))];
        assert!(!fits(&old, &copies));
        assert_eq!(sizes(&room_for(&old, &copies)), [(40, 120); 5]);
    }

    #[test]
    fn bent_items_are_bundled_by_squares_that_widen_with_distance() {
        let at = |x: f32, z: f32| glam::Vec3::new(x, 3.0, z);
        // Near the camera: 8-stud squares, whatever the height.
        assert_eq!(bundle_cell(at(1.0, 1.0), 0.0), [3, 0, 0]);
        assert_eq!(bundle_cell(at(7.9, -0.1), 15.9), [3, 0, -1]);
        assert_eq!(
            bundle_cell(at(9.0, 1.0), 5.0),
            bundle_cell(glam::Vec3::new(15.0, -40.0, 7.0), 12.0)
        );
        // Twice as far, twice as wide.
        assert_eq!(bundle_cell(at(9.0, 1.0), 16.0), [4, 0, 0]);
        assert_eq!(bundle_cell(at(-100.0, 300.0), 250.0), [7, -1, 2]);
        assert_eq!(bundle_cell(at(-100.0, 300.0), 256.0), [8, -1, 1]);
        // Nothing breaks far out or on a bad number.
        assert_eq!(bundle_cell(at(0.0, 0.0), f32::INFINITY)[0], 20);
        assert_eq!(bundle_cell(at(0.0, 0.0), f32::NAN)[0], 3);
    }
}
