//! Axcenity renderer.
//!
//! Callers describe a frame as a [`DrawList`] and hand it to
//! [`Renderer::render`]. This crate never sees Instances, scripts or windows:
//! anything that can provide a window handle (such as a `winit` window) can be
//! drawn into with [`Renderer`], and [`TextureRenderer`] draws into a texture
//! on a GPU device someone else set up (the editor's viewport).

pub mod adapters;
mod atmosphere;
mod decal;
mod draw_list;
mod fonts;
mod graphics;
mod mesh;
mod overdraw;
mod pacing;
mod renderer;
mod tone_mapping;
pub mod video_memory;
mod water;

pub use atmosphere::Atmosphere;
pub use axcenity_render_types::Camera;
pub use decal::{Decal, DecalRole, decal_quad};
pub use draw_list::{
    AmbientLight, CURVE_ENTRIES, Channel, ChannelMap, ClothShading, Clouds, ColorGrading,
    ColorLookup, Cone, DEFAULT_SNOW_THICKNESS, DepthOfField, DrawItem, DrawList, Dust, DustBoxes,
    Edges, FilmGrain, Flipbook, FlipbookFrame, FlipbookMode, Fog, GridPlane, GridTint, HairShading,
    ItemChunk, LensEffects, Light, LightProbeGrid, LightProbeVolume, MAX_DUST_BOXES,
    MAX_PALETTE_COLORS, MAX_WEATHER_BOXES, Makeup, Material, MeshSource, MorphView, NormalMap,
    Outline, ParticleCollision, ParticleCurves, ParticleEmitter, ParticleLook, PointShadows,
    PostProcessing, ProbeImage, ProbeSky, RectArea, ReflectionProbe, SKIN_MAKEUP_LAYERS,
    STAND_IN_LEVEL, SURFACE_PARAMETERS, Shading, Shadows, Shape, ShapeInOut, Skin, SkinLayers,
    SkinShading, Sky, SkyImage, SpawnKind, SpawnShape, SpotShadows, Spots, Sprite, StandIn, Sun,
    SunRays, Sway, TerrainLayer, TerrainLayers, TerrainSurface, TerrainView, TextAlign, TextShadow,
    TextSpan, TextStroke, TextureMaps, TextureSource, Toon, UiBorderSide, UiClip, UiDrawItem,
    UiImage, UiRectangle, UiShadow, UiSlice, UiText, VerticalAlign, Vignette, WEATHER_FIELD_SIZE,
    Weather, WeatherBox, WeatherField, curve_at, is_turned, turn_point, turned_bounds,
};
pub use draw_list::{
    WEATHER_PRINT_CELLS_PER_STUD, WEATHER_PRINT_LEVELS, WEATHER_PRINT_MOST, WEATHER_PRINT_SIZE,
    WeatherPrintCells, WeatherPrints,
};
pub use fonts::{BUILT_IN_FONTS, BuiltInFont, UiFont, font_families};
pub use graphics::{
    AmbientOcclusion, AntiAliasing, Capabilities, CloudQuality, DecalLimits, DepthOfFieldQuality,
    EffectiveGraphics, EnvironmentMapSize, GraphicsSettings, MotionBlurQuality, POINT_SHADOW_FACES,
    PointShadowMapSize, Quality, RAY_TRACING_DISTANCE_RANGE, RENDER_SCALE_RANGE,
    RayShadowResolution, RayTracedAmbientOcclusion, RayTracedGlobalIllumination,
    RayTracedReflections, RayTracedShadows, RealtimeGiTier, ReflectionQuality, ShadowMapSize,
    ShadowQuality, SpotShadowMapSize, TEXTURE_BUDGET_RANGE, TextureQuality, read_capabilities,
};
pub use graphics::{MESH_BUDGET_SHARE, texture_budget_for, whole_gib};
pub use overdraw::{MAX_LAYERS, OVERDRAW_BANDS, OVERDRAW_STEP, OverdrawStats, layers_of};
pub use pacing::{FramePacer, PacedFrame};
#[cfg(feature = "debug-views")]
pub use renderer::DebugView;
#[cfg(feature = "debug-views")]
pub use renderer::PATH_TRACED_MOST_PATHS;
pub use renderer::{
    BakedProbe, CpuSection, CullingBounds, DrawnPart, FramePass, LIGHT_PROBE_BAKE_SIZE, LOD_LEVELS,
    LOD_SCREEN_SIZES, LightProbeBaker, MAX_GPU_PARTICLES, PROBE_BAKE_SIZE, PartPass, PartTimes,
    PosingStats, RayTracingStats, RenderStats, Renderer, SHADOW_CASCADE_COUNT, ShaderError,
    SurfaceShader, TextureRenderer, part_label,
};
pub use tone_mapping::ToneMapping;
pub use water::{DEFAULT_WAVE_VARIATION, WATER_GRID, Water, water_surface};

/// Re-exported so callers build transforms with the same `glam` version.
pub use glam;
/// Re-exported so callers that pass in GPU devices use the same `wgpu` version.
pub use wgpu;
