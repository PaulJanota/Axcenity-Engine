//! Motion vectors, TAA and motion blur (`temporal.wgsl`, and `vs_motion` and
//! `fs_motion` in shader.wgsl): the passes that keep last frame's image.
//!
//! While TAA is on or motion blur is drawn, the prepass is kept alive and a
//! motion target follows it: the camera's motion, worked out from the
//! prepass's depth, with the items that moved since last frame drawn over
//! it. Each item is known from frame to frame by [`DrawItem::id`]; [`History`]
//! keeps last frame's camera, each item's transform and where its posed
//! corners were (CO8: `posing.rs` keeps last frame's posed corners aside).
//!
//! TAA draws the scene a fraction of a pixel to one side each frame (the
//! jitter, [`History::jitter`]) and blends it into the frames before, which
//! it keeps in one of two history targets (each frame reads the other).
//! Motion blur streaks the scene by the motion. Each writes into a target of
//! its own, which is then copied over the scene's HDR texture, so what comes
//! after (depth of field, bloom, the composite pass) reads the scene as
//! before. Every target here exists only while it's used.
//!
//! Below 100% render scale, TAA upscales too (roadmap G9 stage 2,
//! `fs_taau`): its history is at the output's size, each output pixel
//! gathers the scene's jittered samples near it, and more jitter places
//! cover the bigger pixels. The result is copied into a target at the
//! output's size that bloom and the composite pass read in the scene's
//! place, so motion blur and depth of field run before it, on the scene.

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use rustc_hash::FxHashMap;

use crate::draw_list::{DrawItem, DrawList, LensEffects, Skin};
use crate::graphics::{EffectiveGraphics, MotionBlurQuality};
use crate::mesh::Vertex;

use super::gpu::{shader_module, target_bytes};
use super::lighting::{instance_sway, is_flat};
use super::lod::Draw;
use super::post::fullscreen_pass;
use super::{DEPTH_FORMAT, FAR_PLANE, HDR_FORMAT, MeshKey, NEAR_PLANE};

/// How far each pixel moved across the screen since last frame, in texture
/// coordinates.
pub(super) const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
/// This frame's share of TAA's blend: the rest is the frames before.
pub(super) const TAA_SHARE: f32 = 0.1;
/// How many places the jitter steps through before it repeats, at full
/// size. Upscaling TAA steps through more ([`jitter_phases`]).
pub(super) const JITTER_PHASES: u64 = 8;
/// The most jitter places upscaling TAA steps through (at 50% render
/// scale it uses 32).
const MAX_JITTER_PHASES: u64 = 64;
/// The longest half of a motion blur streak, as a share of the scene's
/// height (30 pixels each way at 1080).
pub(super) const MAX_MOTION_BLUR: f32 = 1.0 / 36.0;

/// What the temporal passes read. Must match `Temporal` in temporal.wgsl
/// (its first two matrices are `MotionView` in shader.wgsl).
#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
pub(super) struct TemporalUniform {
    view_proj: [[f32; 4]; 4],
    previous_view_proj: [[f32; 4]; 4],
    /// Last frame's clock and wind (`Frame::clock`), which swayed plants
    /// were bent by then.
    previous_clock: [f32; 4],
    inv_view_proj: [[f32; 4]; 4],
    /// 1 when last frame's history can be used, this frame's share of the
    /// blend, the near plane and the far plane.
    settings: [f32; 4],
    size: [f32; 4],
    /// Motion blur's share of a frame's motion, its longest half-streak in
    /// pixels, samples per pixel and tile size in pixels.
    blur: [f32; 4],
    noise: [f32; 4],
    /// Upscaling TAA: the output's width and height, and 1 over each.
    output: [f32; 4],
    /// This frame's jitter in the scene's pixels, and 1 while TAA upscales.
    jitter: [f32; 4],
    /// The view of the frame before last, without the jitter: with the
    /// other two it says how the camera's own motion changed
    /// (`camera_change` in temporal.wgsl).
    before_view_proj: [[f32; 4]; 4],
}

/// A moved item as the motion pass reads it. Must match `MotionIn` in
/// shader.wgsl.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct MotionInstance {
    model: [[f32; 4]; 4],
    previous: [[f32; 4]; 4],
    skin: [u32; 2],
    sway: [f32; 4],
    fade: f32,
}

/// The temporal passes' pipelines, uniform and per-frame buffers, which
/// don't depend on the scene's size.
pub(super) struct TemporalPasses {
    uniform: wgpu::Buffer,
    sampler: wgpu::Sampler,
    camera_layout: wgpu::BindGroupLayout,
    taa_layout: wgpu::BindGroupLayout,
    tile_layout: wgpu::BindGroupLayout,
    blur_layout: wgpu::BindGroupLayout,
    /// Group 3 of the moving items' pass: the uniform and last frame's
    /// posed corners.
    motion_layout: wgpu::BindGroupLayout,
    motion_group: wgpu::BindGroup,
    camera_motion: wgpu::RenderPipeline,
    moving: wgpu::RenderPipeline,
    taa: wgpu::RenderPipeline,
    /// TAA that upscales: drawn at the output's size.
    taau: wgpu::RenderPipeline,
    tile_max: wgpu::RenderPipeline,
    motion_blur: wgpu::RenderPipeline,
    /// This frame's moved items, and how many fit.
    instances: wgpu::Buffer,
    instance_capacity: usize,
}

/// The motion target (two, so last frame's is still there to compare with)
/// and, while used, TAA's two history targets and motion blur's target,
/// with the bindings of each frame's passes (by [`History::parity`]).
pub(super) struct TemporalTargets {
    /// Kept for reading back in tests.
    #[cfg_attr(not(test), allow(dead_code))]
    motion_textures: [wgpu::Texture; 2],
    motion: [wgpu::TextureView; 2],
    camera_groups: [wgpu::BindGroup; 2],
    taa: Option<TaaTargets>,
    blur: Option<BlurTargets>,
    /// Bytes of every texture here.
    pub(super) bytes: u64,
    /// A clone of [`super::TargetsAlive::motion`].
    _alive: Arc<()>,
}

struct TaaTargets {
    /// Each frame draws into one and reads the other: at the scene's size,
    /// or while TAA upscales at the output's.
    history: [wgpu::Texture; 2],
    views: [wgpu::TextureView; 2],
    groups: [wgpu::BindGroup; 2],
    /// While TAA upscales: the output-sized target each frame's result is
    /// copied into, which bloom and the composite pass read.
    upscaled: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// A clone of [`super::TargetsAlive::taa`].
    _alive: Arc<()>,
}

struct BlurTargets {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    /// The longest streak in each tile of the screen.
    tiles: wgpu::TextureView,
    tile_groups: [wgpu::BindGroup; 2],
    groups: [wgpu::BindGroup; 2],
    /// A clone of [`super::TargetsAlive::motion_blur`].
    _alive: Arc<()>,
}

/// What the temporal passes keep from one frame to the next.
#[derive(Default)]
pub(super) struct History {
    /// Whether TAA's history target holds last frame's picture, at this
    /// size: false after the targets are remade or a frame without TAA.
    pub(super) valid: bool,
    /// Which of each pair of targets this frame draws into.
    parity: usize,
    /// Frames drawn with the motion target, for the jitter and the noise.
    frame: u64,
    /// Last frame's view, without the jitter.
    view_proj: Option<Mat4>,
    /// The view of the frame before that.
    before: Option<Mat4>,
    /// Last frame's clock and wind, as `Frame::clock` has them.
    clock: Option<[f32; 4]>,
    /// Each item's transform last frame, by its ID.
    models: FxHashMap<u64, Mat4>,
    /// Where each posed item's corners were in last frame's posed corners,
    /// by its ID and mesh (the two copies of a level-of-detail switch
    /// share an ID).
    posed: HashMap<(u64, MeshKey), u32>,
}

/// What the temporal passes need of the scene's targets.
pub(super) struct SceneViews<'a> {
    pub(super) hdr: &'a wgpu::TextureView,
    pub(super) depth: &'a wgpu::TextureView,
    /// What TAA reads: the scene, or while TAA upscales with depth of field
    /// on, depth of field's blurred copy of it (drawn before TAA then).
    pub(super) taa_source: &'a wgpu::TextureView,
}

impl TemporalPasses {
    /// `item_layouts` are the frame's, an empty one and the skin's, as the
    /// moving items' pass binds them; `previous` is last frame's posed
    /// corners (`Posing::previous`).
    pub(super) fn new(
        device: &wgpu::Device,
        item_layouts: [&wgpu::BindGroupLayout; 3],
        previous: &wgpu::Buffer,
    ) -> Self {
        let fragment = wgpu::ShaderStages::FRAGMENT;
        let uniform_entry = |visibility| wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: fragment,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let depth = || texture(1, wgpu::TextureSampleType::Depth);
        let unfiltered = |binding| {
            texture(
                binding,
                wgpu::TextureSampleType::Float { filterable: false },
            )
        };
        let layout = |label, entries: &[wgpu::BindGroupLayoutEntry]| {
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(label),
                entries,
            })
        };
        let camera_layout = layout("camera motion", &[uniform_entry(fragment), depth()]);
        let taa_layout = layout(
            "TAA",
            &[
                uniform_entry(fragment),
                depth(),
                unfiltered(2),
                texture(3, wgpu::TextureSampleType::Float { filterable: true }),
                unfiltered(4),
                unfiltered(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: fragment,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let tile_layout = layout(
            "motion blur tiles",
            &[uniform_entry(fragment), unfiltered(4)],
        );
        let blur_layout = layout(
            "motion blur",
            &[
                uniform_entry(fragment),
                depth(),
                unfiltered(2),
                unfiltered(4),
                unfiltered(7),
            ],
        );
        let motion_layout = layout(
            "moving items",
            &[
                uniform_entry(wgpu::ShaderStages::VERTEX),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        );
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("temporal"),
            size: size_of::<TemporalUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("TAA history"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::include_wgsl!("../temporal.wgsl"));
        let fullscreen =
            |label, layout: &wgpu::BindGroupLayout, entry, format: wgpu::TextureFormat| {
                let pipeline_layout =
                    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some(label),
                        bind_group_layouts: &[Some(layout)],
                        immediate_size: 0,
                    });
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(label),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        targets: &[Some(format.into())],
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let camera_motion = fullscreen(
            "camera motion",
            &camera_layout,
            "fs_camera_motion",
            MOTION_FORMAT,
        );
        let taa = fullscreen("TAA", &taa_layout, "fs_taa", HDR_FORMAT);
        let taau = fullscreen("TAA upscale", &taa_layout, "fs_taau", HDR_FORMAT);
        let tile_max = fullscreen(
            "motion blur tiles",
            &tile_layout,
            "fs_tile_max",
            MOTION_FORMAT,
        );
        let motion_blur = fullscreen("motion blur", &blur_layout, "fs_motion_blur", HDR_FORMAT);
        let moving = moving_pipeline(device, item_layouts, &motion_layout);
        let instance_capacity = 16;
        let instances = instance_buffer(device, instance_capacity);
        let motion_group = motion_group(device, &motion_layout, &uniform, previous);
        Self {
            uniform,
            sampler,
            camera_layout,
            taa_layout,
            tile_layout,
            blur_layout,
            motion_layout,
            motion_group,
            camera_motion,
            moving,
            taa,
            taau,
            tile_max,
            motion_blur,
            instances,
            instance_capacity,
        }
    }
}

/// The moving items' pipeline: each moved item's motion, drawn where the
/// prepass's depth shows it (equal or nearer, never written).
fn moving_pipeline(
    device: &wgpu::Device,
    [frame_layout, empty_layout, skin_layout]: [&wgpu::BindGroupLayout; 3],
    motion_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = shader_module(device, "moving items", super::gpu::SCENE_SHADER);
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("moving items"),
        bind_group_layouts: &[
            Some(frame_layout),
            Some(empty_layout),
            Some(skin_layout),
            Some(motion_layout),
        ],
        immediate_size: 0,
    });
    let vertex_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x3],
    };
    let instance_layout = wgpu::VertexBufferLayout {
        array_stride: size_of::<MotionInstance>() as u64,
        step_mode: wgpu::VertexStepMode::Instance,
        attributes: &wgpu::vertex_attr_array![
            2 => Float32x4, 3 => Float32x4, 4 => Float32x4, 5 => Float32x4,
            6 => Float32x4, 7 => Float32x4, 8 => Float32x4, 9 => Float32x4,
            10 => Uint32x2,
            11 => Float32x4,
            12 => Float32,
        ],
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("moving items"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_motion"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex_layout), Some(instance_layout)],
        },
        // Back faces are behind the front ones, so the depth test leaves
        // them out; double-sided items need theirs.
        primitive: wgpu::PrimitiveState {
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: Some(false),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_motion"),
            compilation_options: Default::default(),
            targets: &[Some(MOTION_FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn instance_buffer(device: &wgpu::Device, capacity: usize) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("moved items"),
        size: (capacity * size_of::<MotionInstance>()) as u64,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn motion_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    uniform: &wgpu::Buffer,
    previous: &wgpu::Buffer,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("moving items"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: previous.as_entire_binding(),
            },
        ],
    })
}

/// Which temporal targets a frame needs: TAA's history and motion blur's.
/// Either needs the motion target and the prepass.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct TemporalUse {
    pub(super) taa: bool,
    pub(super) motion_blur: bool,
}

impl TemporalUse {
    /// What `graphics` and a draw list that wants motion blur (or doesn't)
    /// use.
    pub(super) fn of(graphics: &EffectiveGraphics, wants_motion_blur: bool) -> Self {
        Self {
            taa: graphics.anti_aliasing == crate::AntiAliasing::Taa,
            motion_blur: wants_motion_blur && graphics.motion_blur != MotionBlurQuality::Off,
        }
    }

    pub(super) fn any(self) -> bool {
        self.taa || self.motion_blur
    }
}

/// Whether `lens` has motion blur that streaks anything.
pub(super) fn wants_motion_blur(lens: &LensEffects) -> bool {
    lens.motion_blur
        .is_some_and(|share| share.is_finite() && share > 0.0)
}

impl History {
    /// How many frames have been drawn with the motion target: the index
    /// the jitter and the per-frame noise step by.
    pub(super) fn frame_index(&self) -> u64 {
        self.frame
    }
}

/// How many jitter places TAA steps through for a scene `scale` times the
/// output's size: [`JITTER_PHASES`] at full size, more as each output pixel
/// gets fewer of the scene's (as FSR 2 does: 8 × the area ratio).
pub(super) fn jitter_phases(scale: f32) -> u64 {
    if !(scale.is_finite() && scale > 0.0 && scale < 1.0) {
        return JITTER_PHASES;
    }
    let phases = (JITTER_PHASES as f32 / (scale * scale)).ceil() as u64;
    phases.clamp(JITTER_PHASES, MAX_JITTER_PHASES)
}

/// Motion blur's tile size for a scene `height` pixels tall: as long as its
/// longest half-streak, so the tiles around a pixel hold every streak that
/// reaches it.
fn blur_tile(height: u32) -> u32 {
    ((height as f32 * MAX_MOTION_BLUR).ceil() as u32).clamp(4, 64)
}

impl TemporalTargets {
    /// Makes the targets `used` needs for a `width` × `height` scene,
    /// reading `scene`'s HDR color and the prepass's depth, with TAA's
    /// history at the size of `upscaled_to` while TAA upscales to it. Each
    /// set holds a clone of its counter in `alive`.
    pub(super) fn new(
        device: &wgpu::Device,
        passes: &TemporalPasses,
        ((width, height), upscaled_to): ((u32, u32), Option<(u32, u32)>),
        scene: SceneViews<'_>,
        used: TemporalUse,
        alive: &super::TargetsAlive,
    ) -> Self {
        let mut bytes = 0;
        let mut texture = |label, (width, height): (u32, u32), format| {
            let (width, height) = (width.max(1), height.max(1));
            bytes += target_bytes((width, height, 1), format, 1);
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::COPY_SRC
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let view = |texture: &wgpu::Texture| texture.create_view(&Default::default());
        let size = (width, height);
        let motion_textures = [0, 1].map(|_| texture("motion vectors", size, MOTION_FORMAT));
        let motion = [view(&motion_textures[0]), view(&motion_textures[1])];
        let upscaled_to = upscaled_to.filter(|_| used.taa);
        let history_size = upscaled_to.unwrap_or(size);
        let history = used
            .taa
            .then(|| [0, 1].map(|_| texture("TAA history", history_size, HDR_FORMAT)));
        let upscaled = upscaled_to.map(|output| {
            let texture = texture("upscaled scene (HDR)", output, HDR_FORMAT);
            let view = view(&texture);
            (texture, view)
        });
        let blur = used.motion_blur.then(|| {
            let tile = blur_tile(height);
            let tiles = (width.div_ceil(tile), height.div_ceil(tile));
            (
                texture("motion blur", size, HDR_FORMAT),
                view(&texture("motion blur tiles", tiles, MOTION_FORMAT)),
            )
        });

        // The uniform, then each texture at its binding (and for TAA the
        // history's sampler).
        let group =
            |label, layout: &wgpu::BindGroupLayout, entries: &[(u32, &wgpu::TextureView)]| {
                let mut all = vec![wgpu::BindGroupEntry {
                    binding: 0,
                    resource: passes.uniform.as_entire_binding(),
                }];
                all.extend(entries.iter().map(|&(binding, view)| wgpu::BindGroupEntry {
                    binding,
                    resource: wgpu::BindingResource::TextureView(view),
                }));
                if entries.iter().any(|&(binding, _)| binding == 3) {
                    all.push(wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&passes.sampler),
                    });
                }
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(label),
                    layout,
                    entries: &all,
                })
            };
        let camera_groups =
            [0, 1].map(|_| group("camera motion", &passes.camera_layout, &[(1, scene.depth)]));
        let taa = history.map(|history| {
            let views = [view(&history[0]), view(&history[1])];
            // Frame `parity` draws into its own history and reads the
            // other's, with its own motion and the other's.
            let groups = [0, 1].map(|parity| {
                group(
                    "TAA",
                    &passes.taa_layout,
                    &[
                        (1, scene.depth),
                        (2, scene.taa_source),
                        (3, &views[1 - parity]),
                        (4, &motion[parity]),
                        (5, &motion[1 - parity]),
                    ],
                )
            });
            TaaTargets {
                history,
                views,
                groups,
                upscaled,
                _alive: alive.taa.clone(),
            }
        });
        let blur = blur.map(|(texture, tiles)| {
            let tile_groups = [0, 1].map(|parity| {
                group(
                    "motion blur tiles",
                    &passes.tile_layout,
                    &[(4, &motion[parity])],
                )
            });
            let groups = [0, 1].map(|parity| {
                group(
                    "motion blur",
                    &passes.blur_layout,
                    &[
                        (1, scene.depth),
                        (2, scene.hdr),
                        (4, &motion[parity]),
                        (7, &tiles),
                    ],
                )
            });
            BlurTargets {
                view: view(&texture),
                texture,
                tiles,
                tile_groups,
                groups,
                _alive: alive.motion_blur.clone(),
            }
        });
        Self {
            motion_textures,
            motion,
            camera_groups,
            taa,
            blur,
            bytes,
            _alive: alive.motion.clone(),
        }
    }

    pub(super) fn has_taa(&self) -> bool {
        self.taa.is_some()
    }

    /// Whether TAA upscales: its history is at the output's size.
    pub(super) fn upscales(&self) -> bool {
        self.taa.as_ref().is_some_and(|taa| taa.upscaled.is_some())
    }

    /// While TAA upscales, the output-sized picture it makes, which bloom
    /// and the composite pass read in the scene's place.
    pub(super) fn upscaled_view(&self) -> Option<&wgpu::TextureView> {
        let (_, view) = self.taa.as_ref()?.upscaled.as_ref()?;
        Some(view)
    }

    /// The motion target the last frame drew (before `history` moved on).
    #[cfg(test)]
    pub(super) fn last_motion(&self, history: &History) -> &wgpu::Texture {
        &self.motion_textures[1 - history.parity]
    }

    /// What these targets were made for.
    pub(super) fn used(&self) -> TemporalUse {
        TemporalUse {
            taa: self.taa.is_some(),
            motion_blur: self.blur.is_some(),
        }
    }
}

impl History {
    /// Forgets everything kept: the next frame drawn with motion starts
    /// afresh, with nothing moving but the camera.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Where TAA moves this frame's scene, in its pixels: one of `phases`
    /// places of a Halton sequence ([`jitter_phases`]), each within half a
    /// pixel of the middle.
    pub(super) fn jitter(&self, phases: u64) -> Vec2 {
        let index = self.frame % phases.max(1) + 1;
        Vec2::new(halton(index, 2), halton(index, 3)) - 0.5
    }

    /// The camera's view as drawn this frame: `view_proj` moved by the
    /// jitter (of `phases` places), for a scene of `size` pixels.
    pub(super) fn jittered(
        &self,
        view_proj: Mat4,
        (width, height): (u32, u32),
        phases: u64,
    ) -> Mat4 {
        let jitter = self.jitter(phases);
        let offset = Vec3::new(
            2.0 * jitter.x / width.max(1) as f32,
            -2.0 * jitter.y / height.max(1) as f32,
            0.0,
        );
        Mat4::from_translation(offset) * view_proj
    }
}

/// The `index`th number of the Halton sequence in `base`: spread evenly
/// over 0 to 1, each new one between those before.
fn halton(mut index: u64, base: u64) -> f32 {
    let mut fraction = 1.0;
    let mut result = 0.0;
    while index > 0 {
        fraction /= base as f32;
        result += fraction * (index % base) as f32;
        index /= base;
    }
    result
}

/// The moved items of a frame, for the motion pass.
pub(super) struct Moved {
    /// By mesh, so each mesh's run is drawn at once.
    pub(super) instances: Vec<(MeshKey, MotionInstance)>,
}

impl History {
    /// What moved among `draws` since last frame: each solid item (drawn in
    /// the prepass) with an ID whose transform changed, or that's posed,
    /// and each one the wind sways (a plant's leaves move though it stands
    /// still; one without an ID or new this frame is taken to have stood
    /// where it is). `posed` says where an item's skin was posed with a
    /// mesh this frame (`Posing::offset`), and `clock` is this frame's
    /// `Frame::clock`.
    ///
    /// `in_view` says which draws the camera sees (`DrawPlan::in_view`):
    /// the prepass draws only those, and the motion pass draws only where
    /// the prepass did, so the others (a swaying plant only a shadow view
    /// reaches) are left out. `None` keeps every draw. The draws are
    /// shared among the CPU's threads; `AXCENITY_NO_MOTION_SHARING` keeps
    /// every draw on one thread, as before, for comparing the two.
    pub(super) fn moved(
        &self,
        draws: &[Draw<'_>],
        in_view: Option<&[bool]>,
        (posed, foliage): (
            impl Fn(&Skin, MeshKey) -> Option<u32> + Sync,
            impl Fn(MeshKey) -> Option<u32> + Sync,
        ),
        clock: [f32; 4],
    ) -> Moved {
        static AS_BEFORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let as_before =
            *AS_BEFORE.get_or_init(|| std::env::var_os("AXCENITY_NO_MOTION_SHARING").is_some());
        let in_view = in_view.filter(|_| !as_before);
        // Plants move only while there's wind, now or last frame, and the
        // clock moved on.
        let windy = |clock: [f32; 4]| clock[1] != 0.0 || clock[3] != 0.0;
        let swaying = self
            .clock
            .is_some_and(|before| before != clock && (windy(before) || windy(clock)));
        // One thread's share: its draws that moved, in their order.
        let part = |part: &[Draw<'_>]| {
            // Where the share starts among the draws.
            let first = (part.as_ptr() as usize - draws.as_ptr() as usize) / size_of::<Draw<'_>>();
            let mut moved = Vec::new();
            for (at, draw) in part.iter().enumerate() {
                if in_view.is_some_and(|in_view| !in_view[first + at]) {
                    continue;
                }
                if let Some(instance) = self.moved_draw(draw, (&posed, &foliage), swaying) {
                    moved.push((draw.mesh(), instance));
                }
            }
            moved
        };
        let parts = if as_before {
            vec![part(draws)]
        } else {
            super::parallel::map_parts(draws, part)
        };
        // By mesh, each mesh's in the draws' order, which is what sorting
        // them all by mesh would give: but an instance is 150 bytes, so
        // where each is (its share, its place there) is put in order and
        // each is then copied once.
        let mut by_mesh: FxHashMap<MeshKey, Vec<(u32, u32)>> = FxHashMap::default();
        let mut count = 0;
        for (share, moved) in parts.iter().enumerate() {
            count += moved.len();
            for (at, (mesh, _)) in moved.iter().enumerate() {
                by_mesh
                    .entry(*mesh)
                    .or_default()
                    .push((share as u32, at as u32));
            }
        }
        let mut meshes: Vec<MeshKey> = by_mesh.keys().copied().collect();
        meshes.sort_unstable();
        let mut instances = Vec::with_capacity(count);
        for mesh in meshes {
            let places = by_mesh.remove(&mesh).unwrap_or_default();
            instances.extend(
                places
                    .into_iter()
                    .map(|(share, at)| parts[share as usize][at as usize]),
            );
        }
        Moved { instances }
    }

    /// `draw`'s instance for the motion pass, if it moved (see
    /// [`History::moved`]).
    fn moved_draw(
        &self,
        draw: &Draw<'_>,
        (posed, foliage): (
            &impl Fn(&Skin, MeshKey) -> Option<u32>,
            &impl Fn(MeshKey) -> Option<u32>,
        ),
        swaying: bool,
    ) -> Option<MotionInstance> {
        {
            let item = draw.item;
            if !in_prepass(item) {
                return None;
            }
            let sways = swaying && item.sway.is_some_and(|sway| sway.strength > 0.0);
            // Only the list's own items with an ID were remembered.
            let remembered = (item.id != 0).then(|| self.models.get(&item.id)).flatten();
            let previous = match remembered {
                Some(previous) => previous,
                None if sways => &item.transform,
                None => return None,
            };
            let mesh = draw.mesh();
            let now = item.skin.as_deref().and_then(|skin| posed(skin, mesh));
            if *previous == item.transform && now.is_none() && !sways {
                return None;
            }
            // Of the scene's instance the pass reads only where the item
            // is and how it sways.
            if is_flat(item) {
                return None;
            }
            // `previous_corner` in shader.wgsl: 1 for posed now but not
            // last frame, else 2 more than where it was; at rest, its
            // foliage data's place, if it has any (`corner_foliage`).
            let skin = match now {
                None => foliage(mesh).map_or([0, 0], |first| [first, super::foliage::FOLIAGE]),
                Some(now) => match self.posed.get(&(item.id, mesh)) {
                    Some(&before) => [now, before + 2],
                    None => [now, 1],
                },
            };
            Some(MotionInstance {
                model: item.transform.to_cols_array_2d(),
                previous: previous.to_cols_array_2d(),
                skin,
                sway: instance_sway(item),
                fade: draw.fade,
            })
        }
    }

    /// Keeps this frame's camera (`view_proj`, without the jitter),
    /// `list`'s items and where each posed item's corners are (`posed`,
    /// by ID and mesh) for the next, and moves on to the other targets.
    /// `taa` says whether TAA's history now holds this frame.
    pub(super) fn finish(
        &mut self,
        list: &DrawList,
        (view_proj, clock): (Mat4, [f32; 4]),
        posed: HashMap<(u64, MeshKey), u32>,
        taa: bool,
    ) {
        self.before = self.view_proj.replace(view_proj);
        self.clock = Some(clock);
        self.models.clear();
        for item in list.items.iter().filter(|item| item.id != 0) {
            self.models.insert(item.id, item.transform);
        }
        self.posed = posed;
        self.valid = taa;
        self.parity = 1 - self.parity;
        self.frame += 1;
    }
}

/// Whether the prepass draws `item`: solid, not water, not terrain.
fn in_prepass(item: &DrawItem) -> bool {
    let material = &item.material;
    material.opacity >= 1.0 && material.water.is_none() && material.terrain.is_none()
}

/// A measuring switch for TAA at full size (`fs_taa`), to tell which part
/// of it a moving picture's faults come from: `AXCENITY_TAA_PROBE` is a
/// list of `clip` (history held to the mean and spread of the colors
/// around the pixel, not only their lowest and highest), `soft` (last
/// frame read as a plain blend of four pixels, as it was until 2026-11-26,
/// not through the Catmull-Rom filter), `own`
/// (each pixel's own motion, not the nearest surface's around it), `jumpy`
/// (history dropped wherever a pixel's motion differs from last frame's,
/// the camera's own changes of pace included, as it was until 2026-11-26)
/// and `share=<0 to 1>` (this frame's share of the blend). Nothing set draws as
/// always (`docs/rendering/temporal.md`).
#[derive(Clone, Copy, Default)]
struct TaaProbe {
    /// `TAA_PROBE_*` in temporal.wgsl.
    flags: u32,
    share: Option<f32>,
}

fn taa_probe() -> TaaProbe {
    static ASKED: std::sync::OnceLock<TaaProbe> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| {
        let mut probe = TaaProbe::default();
        let asked = std::env::var("AXCENITY_TAA_PROBE").unwrap_or_default();
        for part in asked.split(',').map(str::trim) {
            match part {
                "clip" => probe.flags |= 1,
                "soft" => probe.flags |= 2,
                "own" => probe.flags |= 4,
                "jumpy" => probe.flags |= 8,
                _ => {
                    if let Some(share) = part.strip_prefix("share=") {
                        probe.share = share.parse().ok().map(|s: f32| s.clamp(0.01, 1.0));
                    }
                }
            }
        }
        probe
    })
}

impl TemporalPasses {
    /// Binds `previous` (`Posing::previous`, made again) as last frame's
    /// posed corners.
    pub(super) fn bind_previous(&mut self, device: &wgpu::Device, previous: &wgpu::Buffer) {
        self.motion_group = motion_group(device, &self.motion_layout, &self.uniform, previous);
    }

    /// Writes this frame's uniform and moved items: the camera's view
    /// `view_proj` (without the jitter) and as drawn (`drawn`, moved by
    /// `jitter` pixels), in a scene of `size` (upscaled by TAA to
    /// `upscaled_to`, if it is), with motion blur's `share` of a frame's
    /// motion (0 for none) at `quality`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        &mut self,
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        history: &History,
        (view_proj, drawn, jitter): (Mat4, Mat4, Vec2),
        (size, upscaled_to): ((u32, u32), Option<(u32, u32)>),
        (share, quality): (f32, MotionBlurQuality),
        clock: [f32; 4],
        moved: &Moved,
    ) {
        let probe = taa_probe();
        let (width, height) = (size.0.max(1) as f32, size.1.max(1) as f32);
        let output = upscaled_to.unwrap_or(size);
        let (output_width, output_height) = (output.0.max(1) as f32, output.1.max(1) as f32);
        let longest = height * MAX_MOTION_BLUR;
        let uniform = TemporalUniform {
            view_proj: view_proj.to_cols_array_2d(),
            previous_view_proj: history.view_proj.unwrap_or(view_proj).to_cols_array_2d(),
            previous_clock: history.clock.unwrap_or(clock),
            inv_view_proj: drawn.inverse().to_cols_array_2d(),
            settings: [
                f32::from(u8::from(history.valid)),
                probe.share.unwrap_or(TAA_SHARE),
                NEAR_PLANE,
                FAR_PLANE,
            ],
            size: [width, height, 1.0 / width, 1.0 / height],
            blur: [
                share,
                longest,
                quality.samples() as f32,
                blur_tile(size.1) as f32,
            ],
            noise: [(history.frame % 64) as f32, probe.flags as f32, 0.0, 0.0],
            output: [
                output_width,
                output_height,
                1.0 / output_width,
                1.0 / output_height,
            ],
            jitter: [
                jitter.x,
                jitter.y,
                f32::from(u8::from(upscaled_to.is_some())),
                0.0,
            ],
            // With no frame before last, the camera is taken to have
            // stood still until last frame.
            before_view_proj: history
                .before
                .or(history.view_proj)
                .unwrap_or(view_proj)
                .to_cols_array_2d(),
        };
        queue.write_buffer(&self.uniform, 0, bytemuck::bytes_of(&uniform));
        if moved.instances.len() > self.instance_capacity {
            self.instance_capacity = moved.instances.len().next_power_of_two();
            self.instances = instance_buffer(device, self.instance_capacity);
        }
        let instances: Vec<MotionInstance> = moved
            .instances
            .iter()
            .map(|(_, instance)| *instance)
            .collect();
        if !instances.is_empty() {
            queue.write_buffer(&self.instances, 0, bytemuck::cast_slice(&instances));
        }
    }

    /// Draws the motion target: the camera's motion everywhere, then each
    /// moved item's where the prepass's `depth` shows it. `bind` sets a
    /// mesh's vertex, index and skin bindings and gives its index count, or
    /// `None` to skip it. Returns the draws made.
    pub(super) fn draw_motion(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (targets, history): (&TemporalTargets, &History),
        (depth, frame_group, empty): (&wgpu::TextureView, &wgpu::BindGroup, &wgpu::BindGroup),
        moved: &Moved,
        mut bind: impl FnMut(&mut wgpu::RenderPass<'_>, MeshKey) -> Option<u32>,
    ) -> u32 {
        let target = &targets.motion[history.parity];
        fullscreen_pass(
            encoder,
            "camera motion",
            target,
            wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            &self.camera_motion,
            &[&targets.camera_groups[history.parity]],
        );
        if moved.instances.is_empty() {
            return 0;
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("moving items"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
                view: target,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            // Read only: the prepass's depth says what's in front.
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: None,
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&self.moving);
        pass.set_bind_group(0, frame_group, &[]);
        pass.set_bind_group(1, empty, &[]);
        pass.set_bind_group(3, &self.motion_group, &[]);
        pass.set_vertex_buffer(1, self.instances.slice(..));
        let mut draws = 0;
        let mut start = 0;
        while start < moved.instances.len() {
            let mesh = moved.instances[start].0;
            let end = start
                + moved.instances[start..]
                    .iter()
                    .take_while(|(other, _)| *other == mesh)
                    .count();
            if let Some(index_count) = bind(&mut pass, mesh) {
                pass.draw_indexed(0..index_count, 0, start as u32..end as u32);
                draws += 1;
            }
            start = end;
        }
        draws
    }

    /// TAA's pass. At full size it's copied over the scene's HDR texture
    /// `hdr`; while it upscales, into its output-sized target instead.
    pub(super) fn draw_taa(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (targets, history): (&TemporalTargets, &History),
        hdr: &wgpu::Texture,
    ) {
        let parity = history.parity;
        let Some(taa) = &targets.taa else {
            return;
        };
        let (pipeline, into, label) = match &taa.upscaled {
            Some((texture, _)) => (&self.taau, texture, "TAA upscale"),
            None => (&self.taa, hdr, "TAA"),
        };
        fullscreen_pass(
            encoder,
            label,
            &taa.views[parity],
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            pipeline,
            &[&taa.groups[parity]],
        );
        encoder.copy_texture_to_texture(
            taa.history[parity].as_image_copy(),
            into.as_image_copy(),
            into.size(),
        );
    }

    /// Motion blur's passes, copied over the scene's HDR texture `hdr`, if
    /// `targets` has its target.
    pub(super) fn draw_motion_blur(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        (targets, history): (&TemporalTargets, &History),
        hdr: &wgpu::Texture,
    ) {
        let parity = history.parity;
        if let Some(blur) = &targets.blur {
            fullscreen_pass(
                encoder,
                "motion blur tiles",
                &blur.tiles,
                wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                &self.tile_max,
                &[&blur.tile_groups[parity]],
            );
            fullscreen_pass(
                encoder,
                "motion blur",
                &blur.view,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                &self.motion_blur,
                &[&blur.groups[parity]],
            );
            encoder.copy_texture_to_texture(
                blur.texture.as_image_copy(),
                hdr.as_image_copy(),
                hdr.size(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::lighting::instance_raw;
    use super::*;
    use crate::{ItemChunk, Shape, Sway};

    /// Plants the wind sways, of three shapes mixed up, some with IDs,
    /// one moved since last frame, one squashed flat, one see-through.
    fn swaying_plants() -> (Vec<DrawItem>, Vec<DrawItem>) {
        let plant = |i: usize| {
            let shape = [Shape::Sphere, Shape::Block, Shape::Cylinder][i % 3].clone();
            let mut item = DrawItem {
                shape,
                transform: Mat4::from_translation(Vec3::new(i as f32, 0.0, -5.0)),
                material: Default::default(),
                uv_tile: 0.0,
                casts_shadow: true,
                id: if i.is_multiple_of(2) {
                    100 + i as u64
                } else {
                    0
                },
                cull_distance: 0.0,
                skin: None,
                sway: Some(Sway {
                    base: 0.5,
                    height: 2.0 + i as f32,
                    strength: 0.3,
                    flutter: 0.2,
                }),
                decal_role: Default::default(),
            };
            if i == 7 {
                item.transform = Mat4::from_scale(Vec3::new(1.0, 0.0, 1.0));
            }
            if i == 9 {
                item.material.opacity = 0.5;
            }
            if i == 11 {
                item.sway = None;
            }
            item
        };
        // The list's own items (their places are remembered by ID), and a
        // group's (never remembered: they stand still).
        ((0..6).map(plant).collect(), (6..14).map(plant).collect())
    }

    /// What `moved` gave before it was made to do less: each moved item's
    /// whole scene instance worked out, then a stable sort by mesh.
    fn moved_the_long_way(
        history: &History,
        draws: &[Draw<'_>],
        clock: [f32; 4],
    ) -> Vec<(MeshKey, MotionInstance)> {
        let windy = |clock: [f32; 4]| clock[1] != 0.0 || clock[3] != 0.0;
        let swaying = history
            .clock
            .is_some_and(|before| before != clock && (windy(before) || windy(clock)));
        let mut instances = Vec::new();
        for draw in draws {
            let item = draw.item;
            if !in_prepass(item) {
                continue;
            }
            let sways = swaying && item.sway.is_some_and(|sway| sway.strength > 0.0);
            let previous = match history.models.get(&item.id).filter(|_| item.id != 0) {
                Some(previous) => previous,
                None if sways => &item.transform,
                None => continue,
            };
            if *previous == item.transform && !sways {
                continue;
            }
            let Some(raw) = instance_raw(item) else {
                continue;
            };
            instances.push((
                draw.mesh(),
                MotionInstance {
                    model: raw.model,
                    previous: previous.to_cols_array_2d(),
                    skin: [0, 0],
                    sway: raw.sway,
                    fade: draw.fade,
                },
            ));
        }
        instances.sort_by_key(|(mesh, _)| *mesh);
        instances
    }

    #[test]
    fn what_moved_is_what_the_long_way_round_gave() {
        let (own, grouped) = swaying_plants();
        let mut list = DrawList {
            items: own,
            ..crate::renderer::tests::list_of(Vec::new())
        };
        list.chunks = vec![ItemChunk {
            center: Vec3::ZERO,
            radius: 100.0,
            cull_distance: 0.0,
            items: grouped.into(),
        }];
        let resolved = super::super::lod::LodStates::default().resolve(&list, 1, None);
        let draws = resolved.draws;
        assert_eq!(draws.len(), 14);
        let clock = |time: f32| [time, 4.5, 0.0, -2.5];
        let mut history = History::default();
        // Last frame: the same list, one own item elsewhere.
        let mut before = list.clone();
        before.items[2].transform = Mat4::from_translation(Vec3::new(-9.0, 0.0, -5.0));
        history.finish(&before, (Mat4::IDENTITY, clock(1.0)), HashMap::new(), true);

        for now in [clock(1.1), clock(1.0), [1.1, 0.0, 0.0, 0.0]] {
            let moved = history.moved(&draws, None, (|_, _| None, |_| None), now);
            let long_way = moved_the_long_way(&history, &draws, now);
            assert_eq!(moved.instances.len(), long_way.len(), "at {now:?}");
            for (got, want) in moved.instances.iter().zip(&long_way) {
                assert_eq!(got.0, want.0);
                assert_eq!(bytemuck::bytes_of(&got.1), bytemuck::bytes_of(&want.1));
            }
        }
        // Only the draws the camera sees, when told which: what the long
        // way gives of those alone.
        let in_view: Vec<bool> = (0..draws.len()).map(|at| at % 3 != 0).collect();
        let seen: Vec<_> = draws
            .iter()
            .zip(&in_view)
            .filter(|(_, seen)| **seen)
            .map(|(draw, _)| draw.clone())
            .collect();
        let moved = history.moved(&draws, Some(&in_view), (|_, _| None, |_| None), clock(1.1));
        let long_way = moved_the_long_way(&history, &seen, clock(1.1));
        assert!(!long_way.is_empty() && long_way.len() < 11);
        assert_eq!(moved.instances.len(), long_way.len());
        for (got, want) in moved.instances.iter().zip(&long_way) {
            assert_eq!(got.0, want.0);
            assert_eq!(bytemuck::bytes_of(&got.1), bytemuck::bytes_of(&want.1));
        }
        // While the wind blows every swaying solid plant moves but the
        // flat one; with no wind and the same clock, only the moved one.
        assert_eq!(
            history
                .moved(&draws, None, (|_, _| None, |_| None), clock(1.1))
                .instances
                .len(),
            11
        );
        assert_eq!(
            history
                .moved(&draws, None, (|_, _| None, |_| None), clock(1.0))
                .instances
                .len(),
            1
        );
    }

    #[test]
    fn the_jitter_steps_through_places_within_half_a_pixel() {
        let mut history = History::default();
        let mut places = Vec::new();
        for _ in 0..JITTER_PHASES {
            let jitter = history.jitter(JITTER_PHASES);
            assert!(jitter.abs().max_element() < 0.5, "{jitter}");
            places.push(jitter);
            history.frame += 1;
        }
        // Every place is different, and then they repeat.
        for (index, place) in places.iter().enumerate() {
            assert!(
                places[..index]
                    .iter()
                    .all(|other| other.distance(*place) > 0.01)
            );
        }
        assert_eq!(history.jitter(JITTER_PHASES), places[0]);
        // On average they're about the middle of the pixel.
        let mean = places.iter().copied().sum::<Vec2>() / places.len() as f32;
        assert!(mean.abs().max_element() < 0.1, "{mean}");
    }

    #[test]
    fn the_jitter_moves_the_view_by_that_share_of_a_pixel() {
        let history = History::default();
        let camera = crate::Camera {
            position: Vec3::ZERO,
            look_at: Vec3::NEG_Z,
            fov_y_degrees: 60.0,
        };
        let view_proj = super::super::lighting::camera_view_projection(&camera, 2.0);
        let point = Vec3::new(1.0, 2.0, -10.0).extend(1.0);
        let (width, height) = (200, 100);
        let pixel = |matrix: Mat4| {
            let clip = matrix * point;
            let ndc = clip.truncate().truncate() / clip.w;
            Vec2::new(
                (ndc.x + 1.0) * 0.5 * width as f32,
                (1.0 - ndc.y) * 0.5 * height as f32,
            )
        };
        let moved =
            pixel(history.jittered(view_proj, (width, height), JITTER_PHASES)) - pixel(view_proj);
        let jitter = history.jitter(JITTER_PHASES);
        assert!(moved.distance(jitter) < 1e-3, "{moved} vs {jitter}");
    }

    #[test]
    fn upscaling_steps_through_more_jitter_places_as_pixels_grow() {
        assert_eq!(jitter_phases(1.0), JITTER_PHASES);
        assert_eq!(jitter_phases(1.5), JITTER_PHASES);
        assert_eq!(jitter_phases(0.5), 32);
        assert_eq!(jitter_phases(0.75), 15);
        assert_eq!(jitter_phases(0.1), MAX_JITTER_PHASES);
        assert_eq!(jitter_phases(f32::NAN), JITTER_PHASES);
        // The places of a longer run still sit within half a pixel, and
        // cover the pixel evenly.
        let mut history = History::default();
        let mut quadrants = [0; 4];
        for _ in 0..32 {
            let jitter = history.jitter(32);
            assert!(jitter.abs().max_element() < 0.5, "{jitter}");
            quadrants[usize::from(jitter.x > 0.0) + 2 * usize::from(jitter.y > 0.0)] += 1;
            history.frame += 1;
        }
        assert!(
            quadrants.iter().all(|&n| (6..=10).contains(&n)),
            "{quadrants:?}"
        );
    }
}
