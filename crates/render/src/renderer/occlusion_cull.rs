//! Occlusion culling of scattered items (`occlusion_cull.wgsl`), off unless
//! asked for (`AXCENITY_OCCLUSION_CULLING=1`, or
//! [`crate::Renderer::set_occlusion_culling`]).
//!
//! The prepass draws every solid batch but the scattered ones (the painted
//! plants, stones and litter) first, in their order: models, parts and the
//! terrain. Their depth is then reduced to a pyramid whose texels hold the
//! farthest depth they cover, and every scattered instance is tested
//! against it on the GPU: one whose bounding sphere (a swaying plant's
//! reach included) lies wholly behind that depth is left out. The kept
//! instances are compacted, in their order, into a buffer of their own, and
//! the scattered batches are drawn from it with indirect draws, after the
//! rest in the prepass and in their own place in the scene pass.
//!
//! The depth is the same whatever the order (the nearest of what's drawn),
//! and a left-out instance is behind it everywhere, so in the scene pass
//! (whose depth starts from the prepass's, or which draws what hides it
//! too) every one of its pixels would have failed the depth test. The scene
//! pass draws in its usual order. What changes is the prepass's normals
//! where a scattered item and something else tie on exactly one depth
//! value: the first drawn keeps it, now the other one. Through ambient
//! occlusion and screen-space reflections that moves 17 to 2,728 pixels of
//! Overgrown Street's 2560 × 1440 stills, by up to 21 levels.
//!
//! Measured on Overgrown Street (2026-11-28, `docs/rendering/performance.md`)
//! it leaves out 2,000 to 16,000 of 50,000 plants but costs as much as it
//! saves: those plants are far and small, so they cost little to begin
//! with. That's why it's off.

use std::borrow::Cow;
use std::ops::Range;

use bytemuck::{Pod, Zeroable};
use glam::Mat4;

use super::gpu_timer::DrawnPart;
use super::types::{Batch, InstanceRaw};

/// The words of an [`InstanceRaw`], as occlusion_cull.wgsl copies them.
const INSTANCE_WORDS: u32 = 43;
const _: () = assert!(size_of::<InstanceRaw>() == INSTANCE_WORDS as usize * 4);
/// The copy's workgroups a row; must match `COPY_ROW` in occlusion_cull.wgsl.
const COPY_ROW: u32 = 4096;

/// The pyramid's format: a float a texel, the farthest depth it covers.
pub(super) const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// Must match `Cull` in occlusion_cull.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CullUniform {
    view_proj: [[f32; 4]; 4],
    size: [f32; 4],
    counts: [u32; 4],
}

/// Must match `CullBatch` in occlusion_cull.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CullBatch {
    first: u32,
    count: u32,
    out_first: u32,
    args: u32,
}

/// One batch drawn from the kept instances: which of the frame's batches,
/// where its kept instances start in [`OcclusionCull::kept`], and its slot
/// in [`OcclusionCull::args`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Culled {
    pub(super) batch: usize,
    pub(super) out_first: u32,
    pub(super) slot: u32,
}

/// What a frame culls: `batches` are the tested ones, in their order
/// among the frame's solid batches; the rest make the depth they're
/// tested against.
#[derive(Default)]
pub(super) struct CullPlan {
    pub(super) batches: Vec<Culled>,
}

impl CullPlan {
    /// The culled batch `index` is, if it's one.
    pub(super) fn of(&self, index: usize) -> Option<Culled> {
        self.batches
            .binary_search_by_key(&index, |culled| culled.batch)
            .ok()
            .map(|at| self.batches[at])
    }
}

/// Which of `solid` (the frame's solid batches, in their order) are tested:
/// the scattered ones. None if there's nothing else to test them against,
/// or nothing to test.
pub(super) fn plan(solid: &[Batch]) -> CullPlan {
    let mut out_first = 0;
    let mut batches = Vec::new();
    for (index, batch) in solid.iter().enumerate() {
        let count = batch.instances.end - batch.instances.start;
        if batch.part != DrawnPart::Scattered || count == 0 {
            continue;
        }
        batches.push(Culled {
            batch: index,
            out_first,
            slot: batches.len() as u32,
        });
        out_first += count;
    }
    let occluders = solid.iter().any(|batch| batch.part != DrawnPart::Scattered);
    if !occluders || batches.is_empty() {
        return CullPlan::default();
    }
    CullPlan { batches }
}

/// The pipelines, the pyramid and the buffers.
pub(super) struct OcclusionCull {
    /// On with `AXCENITY_OCCLUSION_CULLING=1` set when the renderer is
    /// made, or [`crate::Renderer::set_occlusion_culling`].
    pub(super) enabled: bool,
    first: wgpu::ComputePipeline,
    down: wgpu::ComputePipeline,
    test: wgpu::ComputePipeline,
    offsets: wgpu::ComputePipeline,
    copy: wgpu::ComputePipeline,
    pyramid_layout: wgpu::BindGroupLayout,
    cull_layout: wgpu::BindGroupLayout,
    /// What the first level's pass binds as the level before (it reads the
    /// depth instead): a texel no level is.
    no_level: wgpu::TextureView,
    /// The pyramid for the prepass's size.
    pyramid: Option<Pyramid>,
    uniform: wgpu::Buffer,
    batch_buffer: Grown,
    spheres: Grown,
    owners: Grown,
    ranks: Grown,
    /// The kept instances, `InstanceRaw`s: a vertex buffer for the draws.
    pub(super) kept: Grown,
    /// `DrawIndexedIndirectArgs`, one a culled batch.
    pub(super) args: Grown,
    /// How many batches the last frame tested.
    tested: usize,
}

/// The depth pyramid: a view a level and one of them all, and the
/// prepass's size it was made for (the views keep the texture alive).
struct Pyramid {
    levels: Vec<wgpu::TextureView>,
    whole: wgpu::TextureView,
    size: (u32, u32),
}

/// A buffer remade bigger when it has to be.
pub(super) struct Grown {
    pub(super) buffer: wgpu::Buffer,
    label: &'static str,
    usage: wgpu::BufferUsages,
}

impl Grown {
    fn new(device: &wgpu::Device, label: &'static str, usage: wgpu::BufferUsages) -> Self {
        Self {
            buffer: Self::make(device, label, usage, 256),
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

    fn fit(&mut self, device: &wgpu::Device, size: u64) {
        if self.buffer.size() < size {
            self.buffer = Self::make(device, self.label, self.usage, size.next_power_of_two());
        }
    }
}

impl OcclusionCull {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("occlusion culling"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("../occlusion_cull.wgsl"))),
        });
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let pyramid_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("occlusion pyramid"),
            entries: &[
                texture(0, wgpu::TextureSampleType::Depth),
                texture(1, wgpu::TextureSampleType::Float { filterable: false }),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: PYRAMID_FORMAT,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("occlusion culling"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(1, wgpu::TextureSampleType::Float { filterable: false }),
                storage(2, true),
                storage(3, true),
                storage(4, true),
                storage(5, true),
                storage(6, false),
                storage(7, false),
                storage(8, false),
            ],
        });
        let pipeline = |layout: &wgpu::BindGroupLayout, entry: &str| {
            let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(entry),
                bind_group_layouts: &[Some(layout)],
                immediate_size: 0,
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("occlusion culling"),
            size: size_of::<CullUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let read = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST;
        let no_level = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("no pyramid level"),
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PYRAMID_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            enabled: std::env::var_os("AXCENITY_OCCLUSION_CULLING").is_some_and(|on| on == "1"),
            first: pipeline(&pyramid_layout, "cs_first"),
            down: pipeline(&pyramid_layout, "cs_down"),
            test: pipeline(&cull_layout, "cs_test"),
            offsets: pipeline(&cull_layout, "cs_offsets"),
            copy: pipeline(&cull_layout, "cs_copy"),
            pyramid_layout,
            cull_layout,
            no_level,
            pyramid: None,
            uniform,
            batch_buffer: Grown::new(device, "culled batches", read),
            spheres: Grown::new(device, "culled spheres", read),
            owners: Grown::new(device, "culled batches by instance", read),
            ranks: Grown::new(device, "culled ranks", wgpu::BufferUsages::STORAGE),
            kept: Grown::new(
                device,
                "kept instances",
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::VERTEX,
            ),
            args: Grown::new(
                device,
                "culled draws",
                wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::INDIRECT
                    | wgpu::BufferUsages::COPY_DST
                    | wgpu::BufferUsages::COPY_SRC,
            ),
            tested: 0,
        }
    }

    /// How many instances the last frame's test kept of each batch it
    /// tested, read back from the GPU. Slow: for tests.
    #[cfg(test)]
    pub(super) fn kept_counts(&self, device: &wgpu::Device, queue: &wgpu::Queue) -> Vec<u32> {
        if self.tested == 0 {
            return Vec::new();
        }
        let size = (self.tested * 20) as u64;
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("culled draws read back"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.args.buffer, 0, &read, 0, size);
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |result| {
            result.expect("could not read the culled draws back");
        });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let mapped = read
            .slice(..)
            .get_mapped_range()
            .expect("the buffer is mapped");
        let words: &[u32] = bytemuck::cast_slice(&mapped);
        words.chunks(5).map(|draw| draw[1]).collect()
    }

    /// The pyramid for a prepass `size` pixels: half that size at its first
    /// level, down to one texel.
    fn pyramid_for(&mut self, device: &wgpu::Device, size: (u32, u32)) {
        if self
            .pyramid
            .as_ref()
            .is_some_and(|pyramid| pyramid.size == size)
        {
            return;
        }
        let base = ((size.0 / 2).max(1), (size.1 / 2).max(1));
        let levels = 32 - base.0.max(base.1).leading_zeros();
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("occlusion pyramid"),
            size: wgpu::Extent3d {
                width: base.0,
                height: base.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: levels,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let views = (0..levels)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let whole = texture.create_view(&wgpu::TextureViewDescriptor::default());
        self.pyramid = Some(Pyramid {
            levels: views,
            whole,
            size,
        });
    }

    /// Records the pyramid of `depth` (the prepass's so far, `size` pixels)
    /// and the test of `plan`'s batches (of `solid`, whose instances are in
    /// `instances`, each with its bounding sphere in `spheres`, by the
    /// instance's place, in the frame's shares) seen through `view_proj`. Writes each culled
    /// batch's draw (`index_counts`, in `plan`'s order) to [`Self::args`]
    /// and its kept instances to [`Self::kept`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn record(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        encoder: &mut wgpu::CommandEncoder,
        (depth, size, view_proj): (&wgpu::TextureView, (u32, u32), Mat4),
        (plan, solid, index_counts): (&CullPlan, &[Batch], &[u32]),
        instances: &wgpu::Buffer,
        spheres: &[Vec<[f32; 4]>],
    ) {
        self.pyramid_for(device, size);
        let Some(Pyramid { levels, whole, .. }) = &self.pyramid else {
            return;
        };
        // The pyramid: the first level from the depth, each next from the
        // one before.
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("occlusion pyramid"),
            timestamp_writes: None,
        });
        for (level, view) in levels.iter().enumerate() {
            // Each pass binds the depth and a level before it, and reads
            // one of them: the first level the depth, the rest the level
            // before.
            let source = if level == 0 {
                &self.no_level
            } else {
                &levels[level - 1]
            };
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("occlusion pyramid"),
                layout: &self.pyramid_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(source),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                ],
            });
            pass.set_pipeline(if level == 0 { &self.first } else { &self.down });
            pass.set_bind_group(0, &group, &[]);
            let width = ((size.0 / 2).max(1) >> level).max(1);
            let height = ((size.1 / 2).max(1) >> level).max(1);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        drop(pass);

        // The test's inputs: each culled batch, its instances' spheres in
        // the order they're kept in, and its draw with no instances yet.
        let mut batches = Vec::with_capacity(plan.batches.len());
        let mut balls: Vec<[f32; 4]> = Vec::new();
        let mut owners: Vec<u32> = Vec::new();
        // Where each share of the frame's instances starts.
        let shares: Vec<usize> = spheres
            .iter()
            .scan(0, |start, share| {
                let this = *start;
                *start += share.len();
                Some(this)
            })
            .collect();
        let mut args: Vec<u32> = Vec::with_capacity(plan.batches.len() * 5);
        for (culled, &indices) in plan.batches.iter().zip(index_counts) {
            let range: &Range<u32> = &solid[culled.batch].instances;
            batches.push(CullBatch {
                first: range.start,
                count: range.end - range.start,
                out_first: culled.out_first,
                args: culled.slot,
            });
            // The spheres are in the frame's shares, a batch maybe across
            // two: from the share it starts in on.
            let mut at = range.start as usize;
            while at < range.end as usize {
                let share = shares.partition_point(|&start| start <= at) - 1;
                let within = at - shares[share];
                let take = (spheres[share].len() - within).min(range.end as usize - at);
                balls.extend_from_slice(&spheres[share][within..within + take]);
                at += take;
            }
            let owner = batches.len() as u32 - 1;
            owners.extend(std::iter::repeat_n(
                owner,
                (range.end - range.start) as usize,
            ));
            args.extend_from_slice(&[indices, 0, 0, 0, 0]);
        }
        let kept_bytes = (balls.len() * size_of::<InstanceRaw>()) as u64;
        self.batch_buffer
            .fit(device, (batches.len() * size_of::<CullBatch>()) as u64);
        let total = balls.len() as u32;
        let groups = total.div_ceil(64);
        self.spheres.fit(device, (balls.len() * 16) as u64);
        self.owners.fit(device, u64::from(total) * 4);
        self.ranks.fit(device, u64::from(total + groups) * 4);
        self.kept.fit(device, kept_bytes.max(256));
        self.args.fit(device, (args.len() * 4) as u64);
        queue.write_buffer(&self.batch_buffer.buffer, 0, bytemuck::cast_slice(&batches));
        queue.write_buffer(&self.spheres.buffer, 0, bytemuck::cast_slice(&balls));
        queue.write_buffer(&self.owners.buffer, 0, bytemuck::cast_slice(&owners));
        queue.write_buffer(&self.args.buffer, 0, bytemuck::cast_slice(&args));
        let uniform = CullUniform {
            view_proj: view_proj.to_cols_array_2d(),
            size: [size.0 as f32, size.1 as f32, levels.len() as f32, 0.0],
            counts: [total, groups, batches.len() as u32, 0],
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        self.tested = batches.len();
        fn buffer(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
            wgpu::BindGroupEntry {
                binding,
                resource: buffer.as_entire_binding(),
            }
        }
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("occlusion culling"),
            layout: &self.cull_layout,
            entries: &[
                buffer(0, &self.uniform),
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(whole),
                },
                buffer(2, &self.batch_buffer.buffer),
                buffer(3, &self.spheres.buffer),
                buffer(4, &self.owners.buffer),
                buffer(5, instances),
                buffer(6, &self.kept.buffer),
                buffer(7, &self.args.buffer),
                buffer(8, &self.ranks.buffer),
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("occlusion culling"),
            timestamp_writes: None,
        });
        // The test, the starts, the copies: each reads what the one
        // before wrote.
        pass.set_bind_group(0, &group, &[]);
        pass.set_pipeline(&self.test);
        pass.dispatch_workgroups(groups, 1, 1);
        pass.set_pipeline(&self.offsets);
        pass.dispatch_workgroups(1, 1, 1);
        pass.set_pipeline(&self.copy);
        // A lane a word, 256 a workgroup, in rows of `COPY_ROW` (a
        // dispatch's side is at most 65,535 workgroups).
        let copy_groups = (total * INSTANCE_WORDS).div_ceil(256);
        pass.dispatch_workgroups(copy_groups.min(COPY_ROW), copy_groups.div_ceil(COPY_ROW), 1);
    }
}
