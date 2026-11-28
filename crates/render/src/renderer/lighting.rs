//! Frame uniforms, tiled lights, shadow cascades and draw batches.
//!
//! CPU-side planning for one frame. The `Frame`, `Light` and instance layouts
//! must stay aligned with `shader.wgsl`. Shared GPU types live in `renderer.rs`.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use glam::camera::rh;
use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::DecalRole;
use crate::draw_list::{
    Camera, ChannelMap, DrawItem, DrawList, Light, Material, PostProcessing, Shading, Shadows,
    Skin, TextureSource,
};
use crate::graphics::{AmbientOcclusion, EffectiveGraphics, ShadowMapSize, ShadowQuality};

use super::clusters::{BUILT_IN, Cluster};
use super::culling::{Bounds, CullingBounds, Frustum, ShadowView, Sphere};
use super::gpu_timer::DrawnPart;
use super::lod::{Draw, NO_CLUSTER, OPAQUE};
use super::parallel::{
    map_each, map_heavy_parts, map_indexes, map_parts, sort_unstable, worth_sharing,
};
use super::posing::PENDING;
use super::skinning::JointRaw;
use super::terrain::LastArrays;
use super::{
    Batch, CASCADE_SPLIT_BLEND, CONTACT_SHADOW_LENGTH, CONTACT_SHADOW_THICKNESS, FAR_PLANE,
    FrameUniform, InstanceRaw, LIGHT_SCALE, LIGHTS_PER_TILE, LightRaw, MATERIAL_TEXTURES,
    MAX_LIGHTS, MAX_POINT_SHADOWS, MAX_SPOT_SHADOW_DEGREES, MAX_SPOT_SHADOWS, MAX_SUN_SIZE,
    MAX_TILES, MIN_TILE_SIZE, MapsUniform, MaterialKey, MeshKey, NEAR_PLANE, POINT_SHADOW_SPREAD,
    PointShadow, PostUniform, SHADING_CLOTH, SHADING_HAIR, SHADING_SKIN, SHADING_STANDARD,
    SHADING_TOON, SHADING_UNLIT, SHADOW_CASCADES, SHADOW_CASTER_RANGE, SpotShadow, StandInKey,
    WeatherBoxRaw,
};
use crate::{HairShading, SKIN_MAKEUP_LAYERS, SURFACE_PARAMETERS, SkinLayers};

/// The camera's view (world to view space) and projection (view to clip
/// space).
pub(super) fn camera_matrices(camera: &Camera, aspect: f32) -> (Mat4, Mat4) {
    let view = rh::view::look_at_mat4(camera.position, camera.look_at, Vec3::Y);
    // wgpu uses DirectX-style clip space: depth from 0 to 1, Y up.
    let projection = rh::proj::directx::perspective(
        camera.fov_y_degrees.to_radians(),
        aspect,
        NEAR_PLANE,
        FAR_PLANE,
    );
    (view, projection)
}

pub(super) fn view_projection(list: &DrawList, aspect: f32) -> Mat4 {
    camera_view_projection(&list.camera, aspect)
}

pub(super) fn camera_view_projection(camera: &Camera, aspect: f32) -> Mat4 {
    let (view, projection) = camera_matrices(camera, aspect);
    projection * view
}

/// How strongly ambient occlusion darkens ambient light as drawn: the
/// world's strength (0 to 2), or 0 while the graphics settings have it off
/// (screen-space and ray traced alike).
pub(super) fn occlusion_drawn(list: &DrawList, graphics: &EffectiveGraphics) -> f32 {
    let strength = list.ambient.occlusion;
    let drawn =
        graphics.ambient_occlusion != AmbientOcclusion::Off || super::ray_ao::traced(graphics);
    if !drawn || !strength.is_finite() {
        return 0.0;
    }
    strength.clamp(0.0, 2.0)
}

/// The sun's shadows as drawn: the world's request, within the graphics
/// settings' ceiling. Shadows draw only if both allow them, at the smaller
/// distance, with the settings' map. `None` when they're off.
pub(super) fn shadows_drawn(
    world: Shadows,
    graphics: &EffectiveGraphics,
) -> Option<(Shadows, ShadowMapSize)> {
    let map = graphics.shadow_quality.map()?;
    let usable = world.enabled && world.distance.is_finite() && world.distance > 0.0;
    let distance = world.distance.min(graphics.shadow_distance);
    (usable && distance > 0.0).then_some((Shadows { distance, ..world }, map))
}

/// Whether contact shadows are drawn: when the world's shadows are on and
/// the graphics settings allow them, whatever the shadow maps' quality.
pub(super) fn contact_drawn(world: Shadows, graphics: &EffectiveGraphics) -> bool {
    world.enabled && graphics.contact_shadows
}

/// The world's post-processing, turned off if the graphics settings don't allow it.
pub(super) fn post_drawn(world: &PostProcessing, graphics: &EffectiveGraphics) -> PostProcessing {
    PostProcessing {
        enabled: world.enabled && graphics.post_processing,
        ..*world
    }
}

pub(super) fn frame_uniform(
    list: &DrawList,
    aspect: f32,
    graphics: &EffectiveGraphics,
) -> FrameUniform {
    let camera = &list.camera;
    let (view, projection) = camera_matrices(camera, aspect);
    let view_proj = projection * view;
    let sun_direction = list.sun.direction.normalize_or(Vec3::Y);
    let sun_radiance = linear_color(list.sun.color) * list.sun.intensity;
    let ambient = linear_color(list.ambient.color) * list.ambient.intensity;
    // A NaN or infinite wind would throw swaying plants out of sight.
    let wind = if list.wind.is_finite() {
        list.wind
    } else {
        Vec3::ZERO
    };
    let shadows = shadow_setup(
        list,
        shadows_drawn(list.shadows, graphics),
        (sun_direction, aspect),
        cascades_under_rays(list, graphics, aspect),
    );
    let sky = &list.sky;
    let environment = if sky.environment_intensity.is_finite() {
        sky.environment_intensity.max(0.0)
    } else {
        0.0
    };

    let [rain, snowfall, ..] = precipitation_uniform(&list.weather);
    let (weather_boxes, weather_box_count) = weather_boxes_uniform(list, camera.position);

    FrameUniform {
        view_proj: view_proj.to_cols_array_2d(),
        camera_position: camera.position.extend(1.0).to_array(),
        sun_direction: sun_direction.extend(0.0).to_array(),
        sun_radiance: sun_radiance.extend(0.0).to_array(),
        ambient: ambient.extend(0.0).to_array(),
        cascades: shadows.cascades.map(|cascade| cascade.to_cols_array_2d()),
        cascade_texels: shadows.texels,
        cascade_depths: shadows.depths,
        shadow: shadows.settings,
        inv_view_proj: view_proj.inverse().to_cols_array_2d(),
        sky_top: linear_color(sky.top_color).extend(environment).to_array(),
        sky_horizon: linear_color(sky.horizon_color).extend(0.0).to_array(),
        sky_ground: linear_color(sky.ground_color).extend(0.0).to_array(),
        lights: [0; 4],
        spot_shadows: [Mat4::IDENTITY.to_cols_array_2d(); MAX_SPOT_SHADOWS],
        view: view.to_cols_array_2d(),
        // The scene's size is filled in when it's drawn.
        ambient_occlusion: [occlusion_drawn(list, graphics), 0.0, 1.0, 1.0],
        // The environment maps fill these in when they're on.
        environment: [0.0; 4],
        sky_light: [[0.0; 4]; 9],
        reflection_probes: [0; 4],
        probes: Default::default(),
        point_shadows: Default::default(),
        contact: [
            f32::from(u8::from(contact_drawn(list.shadows, graphics))),
            CONTACT_SHADOW_LENGTH,
            CONTACT_SHADOW_THICKNESS,
            0.0,
        ],
        light_probes: [0; 4],
        volumes: Default::default(),
        clock: [list.time, wind.x, wind.y, wind.z],
        // The sky's look fills this in.
        night: [0.0; 4],
        // So does the fog, which takes the sky's colors.
        fog: Default::default(),
        // And the clouds, which drift with the wind.
        clouds: Default::default(),
        weather: weather_uniform(&list.weather),
        weather_bounds: [0.0; 4],
        precipitation: [rain, snowfall, weather_box_count as f32, 0.0],
        // Filled in while ray traced shadows are drawn.
        ray_shadows: [0.0; 4],
        // And while ray traced reflections are.
        ray_reflections: [0.0; 4],
        // And while TAA is on (x and y). z: how often lightning flashes
        // (`lightning` in lighting.wgsl).
        noise: [0.0, 0.0, lightning_uniform(&list.weather), 0.0],
        weather_boxes,
        // Filled in while the snow has prints.
        weather_prints: [0.0; 4],
    }
}

/// The weather boxes as `Frame::weather_boxes` holds them, and how many:
/// none while surfaces are dry and bare (the shaders skip the weather
/// then), and otherwise the [`crate::MAX_WEATHER_BOXES`] nearest `camera`
/// (inside one counting as 0 away), in the order the list has them, which
/// is the order they apply. One that can't be drawn is left out.
fn weather_boxes_uniform(
    list: &DrawList,
    camera: Vec3,
) -> ([WeatherBoxRaw; crate::MAX_WEATHER_BOXES], usize) {
    let mut raw = [WeatherBoxRaw::default(); crate::MAX_WEATHER_BOXES];
    if list.weather.is_clear() || list.weather_boxes.is_empty() {
        return (raw, 0);
    }
    // Each with its place in the list and how far outside it the camera is.
    let mut found: Vec<(usize, f32, WeatherBoxRaw)> = list
        .weather_boxes
        .iter()
        .filter_map(WeatherBoxRaw::new)
        .enumerate()
        .map(|(order, raw)| {
            let local = if camera.is_finite() {
                // `rows` is the box's matrix, a row to a vector.
                Vec3::from_array(
                    raw.rows
                        .map(|row| Vec4::from_array(row).dot(camera.extend(1.0))),
                )
            } else {
                Vec3::ZERO
            };
            let half_size = Vec3::new(raw.extent[0], raw.extent[1], raw.extent[2]);
            let outside = (local.abs() - half_size).max(Vec3::ZERO).length();
            (order, outside, raw)
        })
        .collect();
    if found.len() > crate::MAX_WEATHER_BOXES {
        // A stable sort: of two as near, the earlier is kept.
        found.sort_by(|a, b| a.1.total_cmp(&b.1));
        found.truncate(crate::MAX_WEATHER_BOXES);
        found.sort_by_key(|&(order, ..)| order);
    }
    for (slot, (_, _, weather_box)) in raw.iter_mut().zip(&found) {
        *slot = *weather_box;
    }
    (raw, found.len())
}

/// The weather as `Frame::weather` holds it: wetness and snow cover (each
/// held to 0 to 1), snow layer thickness in studs (0 to 2, defaulting to
/// [`crate::DEFAULT_SNOW_THICKNESS`] when snow > 0 and thickness <= 0), and
/// `w` filled in by `ScenePipeline` when a valid `weather_field` is bound.
fn weather_uniform(weather: &crate::Weather) -> [f32; 4] {
    let share = |value: f32| if value > 0.0 { value.min(1.0) } else { 0.0 };
    let snow = share(weather.snow);
    let thickness = if weather.snow_thickness > 0.0 && weather.snow_thickness.is_finite() {
        weather.snow_thickness.min(2.0)
    } else if snow > 0.0 {
        crate::DEFAULT_SNOW_THICKNESS
    } else {
        0.0
    };
    [share(weather.wetness), snow, thickness, 0.0]
}

/// What falls now, as `Frame::precipitation` holds it: rain and snowfall,
/// each held to 0 to 1 (a NaN counts as none).
fn precipitation_uniform(weather: &crate::Weather) -> [f32; 4] {
    let share = |value: f32| if value > 0.0 { value.min(1.0) } else { 0.0 };
    [share(weather.rain), share(weather.snowfall), 0.0, 0.0]
}

/// How often lightning flashes, as `Frame::noise.z` holds it: 0 to 1 (a NaN
/// counts as never).
fn lightning_uniform(weather: &crate::Weather) -> f32 {
    if weather.lightning > 0.0 {
        weather.lightning.min(1.0)
    } else {
        0.0
    }
}

/// The lights to draw, and which of them reach each tile of the screen.
pub(super) struct PlannedLights {
    pub(super) lights: Vec<LightRaw>,
    /// For each tile, row by row: how many lights reach it, then up to
    /// [`LIGHTS_PER_TILE`] indexes into `lights`.
    pub(super) tiles: Vec<u32>,
    pub(super) tile_size: u32,
    pub(super) tiles_across: u32,
    /// The spot lights casting shadows, by their layer of the spot shadow map.
    pub(super) spot_shadows: Vec<SpotShadow>,
    /// The point lights casting shadows, by their place in the point shadow
    /// map (six layers each).
    pub(super) point_shadows: Vec<PointShadow>,
}

/// Picks the lights to draw (the nearest [`MAX_LIGHTS`]) and sorts them into
/// screen tiles, so each pixel only loops over the few lights that can
/// reach it instead of all of them. A light goes in every tile that its
/// sphere of reach could touch; tiles with more than [`LIGHTS_PER_TILE`]
/// keep the nearest ones to the camera. Of the spot lights that cast
/// shadows, the nearest in view get a layer of `quality`'s spot map each,
/// as many as it has, and likewise the point lights six layers each of its
/// point map; the rest light without shadows.
pub(super) fn plan_lights(
    list: &DrawList,
    aspect: f32,
    (width, height): (u32, u32),
    quality: ShadowQuality,
) -> PlannedLights {
    let camera = list.camera.position;
    let mut lights: Vec<&Light> = list
        .lights
        .iter()
        .filter(|light| {
            light.position.is_finite()
                && light.range.is_finite()
                && light.range > 0.0
                && light.brightness.is_finite()
                && light.brightness > 0.0
        })
        .collect();
    lights.sort_by(|a, b| {
        let (a, b) = (
            a.position.distance_squared(camera),
            b.position.distance_squared(camera),
        );
        a.total_cmp(&b)
    });
    lights.truncate(MAX_LIGHTS);

    let mut tile_size = MIN_TILE_SIZE;
    while (width.div_ceil(tile_size) * height.div_ceil(tile_size)) as usize > MAX_TILES {
        tile_size *= 2;
    }
    let (across, down) = (width.div_ceil(tile_size), height.div_ceil(tile_size));
    let mut planned = PlannedLights {
        lights: Vec::new(),
        tiles: Vec::new(),
        tile_size,
        tiles_across: across,
        spot_shadows: Vec::new(),
        point_shadows: Vec::new(),
    };
    if lights.is_empty() || width == 0 || height == 0 {
        return planned;
    }

    let view_proj = view_projection(list, aspect);
    let stride = LIGHTS_PER_TILE + 1;
    planned.tiles = vec![0; (across * down) as usize * stride];
    // Lights are nearest first, so the nearest that cast shadows get layers.
    let (spot_map, point_map) = (quality.spot_map(), quality.point_map());
    let spot_layers = spot_map.map_or(0, |map| (map.lights as usize).min(MAX_SPOT_SHADOWS));
    let point_lights = point_map.map_or(0, |map| (map.lights as usize).min(MAX_POINT_SHADOWS));
    for light in lights {
        let Some((first, last)) = light_tiles(view_proj, light, (width, height), tile_size) else {
            continue;
        };
        let index = planned.lights.len() as u32;
        let mut raw = light_raw(light);
        // Ray traced shadows (LE1) trace every lamp that casts shadows.
        raw.shadow[2] = super::ray_shadows::traced_start(light);
        if planned.spot_shadows.len() < spot_layers
            && let Some(map) = spot_map
            && let Some((shadow, texel)) = spot_shadow(light, map.size)
        {
            raw.shadow = [planned.spot_shadows.len() as f32, texel, raw.shadow[2], 0.0];
            planned.spot_shadows.push(shadow);
        } else if planned.point_shadows.len() < point_lights
            && let Some(map) = point_map
            && let Some((shadow, texel)) = point_shadow(light, map.size)
        {
            raw.shadow = [
                planned.point_shadows.len() as f32,
                texel,
                raw.shadow[2],
                0.0,
            ];
            planned.point_shadows.push(shadow);
        }
        planned.lights.push(raw);
        for y in first.1..=last.1 {
            for x in first.0..=last.0 {
                let tile = (y * across + x) as usize * stride;
                let count = planned.tiles[tile] as usize;
                if count < LIGHTS_PER_TILE {
                    planned.tiles[tile + 1 + count] = index;
                    planned.tiles[tile] += 1;
                }
            }
        }
    }
    planned
}

/// The first and last tile (column, row) a light's sphere of reach could
/// touch, or `None` if it's entirely out of view. Works from the sphere's
/// bounding box, which is slightly generous but never misses.
pub(super) fn light_tiles(
    view_proj: Mat4,
    light: &Light,
    (width, height): (u32, u32),
    tile_size: u32,
) -> Option<((u32, u32), (u32, u32))> {
    let (mut min, mut max) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    let mut behind = 0;
    for corner in 0..8 {
        let offset = Vec3::new(
            if corner & 1 == 0 { -1.0 } else { 1.0 },
            if corner & 2 == 0 { -1.0 } else { 1.0 },
            if corner & 4 == 0 { -1.0 } else { 1.0 },
        );
        let clip = view_proj * (light.position + offset * light.range).extend(1.0);
        if clip.w <= NEAR_PLANE * 0.5 {
            behind += 1;
            continue;
        }
        let ndc = Vec2::new(clip.x, clip.y) / clip.w;
        min = min.min(ndc);
        max = max.max(ndc);
    }
    if behind == 8 {
        return None;
    }
    // A box that reaches behind the camera could cover anywhere on screen.
    if behind > 0 {
        (min, max) = (Vec2::splat(-1.0), Vec2::splat(1.0));
    }
    if max.x < -1.0 || min.x > 1.0 || max.y < -1.0 || min.y > 1.0 {
        return None;
    }
    let to_pixels = |ndc: Vec2| {
        Vec2::new(
            (ndc.x * 0.5 + 0.5) * width as f32,
            (0.5 - ndc.y * 0.5) * height as f32,
        )
    };
    let (a, b) = (to_pixels(min), to_pixels(max));
    let tile = |pixel: f32, limit: u32| -> u32 {
        ((pixel / tile_size as f32).floor().max(0.0) as u32).min(limit.div_ceil(tile_size) - 1)
    };
    // The screen's Y grows downward, so the NDC maximum is the top.
    Some((
        (tile(a.x, width), tile(b.y, height)),
        (tile(b.x, width), tile(a.y, height)),
    ))
}

pub(super) fn light_raw(light: &Light) -> LightRaw {
    let radiance = linear_color(light.color) * light.brightness * LIGHT_SCALE;
    let mut rect = [0.0; 4];
    let (cone_edge, direction, cone_inner) = match (light.cone, light.rect) {
        (None, None) => (-2.0, Vec3::NEG_Y, 0.0),
        (None, Some(area)) => {
            let facing = area.direction.normalize_or(Vec3::NEG_Y);
            // Square to the way it faces, however `right` was given.
            let right = (area.right - facing * area.right.dot(facing))
                .normalize_or(facing.any_orthogonal_vector());
            let half_width = area.width.max(0.0) * 0.5;
            rect = (right * half_width)
                .extend(area.height.max(0.0) * 0.5)
                .to_array();
            (-3.0, facing, 0.0)
        }
        (Some(cone), _) => {
            let half = (cone.angle_degrees.clamp(1.0, 179.0) * 0.5).to_radians();
            (
                half.cos(),
                cone.direction.normalize_or(Vec3::NEG_Y),
                (half * 0.8).cos(),
            )
        }
    };
    LightRaw {
        position_range: light.position.extend(light.range).to_array(),
        radiance_cone: radiance.extend(cone_edge).to_array(),
        direction_inner: direction.extend(cone_inner).to_array(),
        shadow: [-1.0, 0.0, 0.0, 0.0],
        rect,
    }
}

/// A spot light's shadow view, if it casts shadows, with the width of one of
/// its `size`² map's texels per stud from the light. The view looks down
/// the beam with a perspective as wide as the cone (at most
/// [`MAX_SPOT_SHADOW_DEGREES`]), from the shadows' near distance out to the
/// light's range.
pub(super) fn spot_shadow(light: &Light, size: u32) -> Option<(SpotShadow, f32)> {
    let cone = light.cone?;
    let shadows = cone.shadows?;
    let direction = cone.direction.normalize_or(Vec3::NEG_Y);
    let angle = cone.angle_degrees.clamp(1.0, 179.0);
    // A little wider than the beam, so filtering at its edge stays inside.
    let fov = (angle * 1.05).min(MAX_SPOT_SHADOW_DEGREES).to_radians();
    let near = if shadows.near.is_finite() {
        shadows.near.max(0.05)
    } else {
        0.05
    };
    if near >= light.range {
        return None;
    }
    let up = if direction.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let view = rh::view::look_at_mat4(light.position, light.position + direction, up);
    let projection = rh::proj::directx::perspective(fov, 1.0, near, light.range);
    let texel = 2.0 * (fov * 0.5).tan() / size as f32;
    let shadow = SpotShadow {
        view_proj: projection * view,
        bounds: cone_bounds(light.position, direction, angle, light.range),
    };
    Some((shadow, texel))
}

/// A point light's six shadow views, if it casts shadows, with the width of
/// one of its `size`² faces' texels per stud from the light. Each face
/// looks along one way of a world axis (+X, -X, +Y, -Y, +Z, -Z), a little
/// over 90° wide ([`POINT_SHADOW_SPREAD`]), from that side's near distance
/// out to the light's range.
pub(super) fn point_shadow(light: &Light, size: u32) -> Option<(PointShadow, f32)> {
    if light.cone.is_some() {
        return None;
    }
    let shadows = light.point_shadows?;
    let fov = 2.0 * POINT_SHADOW_SPREAD.atan();
    let axes = [
        (Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::NEG_Z),
        (Vec3::Z, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_Y),
    ];
    let mut faces = [Mat4::IDENTITY; 6];
    for (face, ((direction, up), near)) in faces.iter_mut().zip(axes.into_iter().zip(shadows.near))
    {
        // Past the range nothing is lit, so that side casts nothing.
        let near = if near.is_finite() {
            near.max(0.05)
        } else {
            0.05
        };
        let near = near.min(light.range * 0.99);
        let view = rh::view::look_at_mat4(light.position, light.position + direction, up);
        let projection = rh::proj::directx::perspective(fov, 1.0, near, light.range);
        *face = projection * view;
    }
    let texel = 2.0 * POINT_SHADOW_SPREAD / size as f32;
    let shadow = PointShadow {
        faces,
        bounds: (light.position, light.range),
    };
    Some((shadow, texel))
}

/// The smallest sphere around a beam from `apex` along `direction`,
/// `angle_degrees` wide, reaching `range` studs: the part of the light's
/// sphere of reach inside the cone.
pub(super) fn cone_bounds(
    apex: Vec3,
    direction: Vec3,
    angle_degrees: f32,
    range: f32,
) -> (Vec3, f32) {
    let half = (angle_degrees * 0.5).to_radians();
    let (sin, cos) = half.sin_cos();
    if half <= std::f32::consts::FRAC_PI_4 {
        // Narrow: the sphere through the apex and the far rim.
        let radius = range / (2.0 * cos);
        (apex + direction * radius, radius)
    } else if half < std::f32::consts::FRAC_PI_2 {
        // Wide: the sphere around the rim, which holds the apex and the cap.
        (apex + direction * range * cos, range * sin)
    } else {
        (apex, range)
    }
}

/// One shadow view's casters: their instance data and how they're drawn.
#[derive(Default)]
pub(super) struct ViewCasters {
    /// Their instances, in the order of `groups` (one piece).
    pub(super) instances: Vec<Vec<InstanceRaw>>,
    /// Which casters to draw, one group per mesh and cut-out material as
    /// this view draws them, counting from the view's first instance:
    /// whole casters first, then cut-outs by material.
    pub(super) groups: Vec<(CasterGroup, Range<u32>)>,
    /// How many of `groups`, from the first, hold only casters that stay
    /// still. In a lamp's view the moving ones (posed, or swaying in the
    /// wind: `lamp_shadows::moves`) come after them, drawn every frame over
    /// the still ones kept (renderer/lamp_shadows.rs); in the sun's, every
    /// group counts.
    pub(super) still_groups: usize,
    /// How many casters were left out.
    pub(super) left_out: u32,
}

/// Each of `views`' casters, grouped by how they're drawn: every caster of
/// one mesh and cut-out material in one instanced draw, whole casters
/// first, then cut-outs by material, so the pipeline and material change
/// as few times as they can. A shadow map keeps the nearest depth whatever
/// the order its casters are drawn in, so this draws the same map as the
/// plan's own order (the camera's distance bands first, which cut each
/// mesh's casters into a run per band: 5,776 draws in Desert Homestead's
/// cascades where grouping makes 1,695).
///
/// A cut-out casts through its holes in a lamp's view and in the sun's
/// first `cut_out_cascades` cascades, and whole past them, unless it's a
/// hair card or near the end of its shadow's reach
/// ([`CutOutCaster::every_cascade`]). A lamp's view puts the casters that
/// move every frame last (`windy`: the wind blows, so swaying ones do).
/// Each view and each share of the casters is its own job for the CPU's
/// threads.
pub(super) fn casters_in_views(
    casters: &[Vec<Caster>],
    views: &[ShadowView],
    cut_out_cascades: usize,
    windy: bool,
    by_map_size: Option<FarCascadeLods<'_>>,
) -> Vec<ViewCasters> {
    let share = worth_sharing(casters.iter().map(Vec::len).sum());
    let views: Vec<(usize, &ShadowView)> = views.iter().enumerate().collect();
    map_each(&views, share, |&(index, view)| {
        if matches!(view, ShadowView::Skipped) {
            return ViewCasters::default();
        }
        let lamp = matches!(view, ShadowView::Lamp(..));
        let near = lamp || index < cut_out_cascades;
        // The farthest cascade draws each caster at the level of detail its
        // size in that map calls for, where that's simpler than the
        // camera's and its mesh is on the GPU.
        // There, a caster drawn at `STAND_IN_LEVEL` or simpler is cast by
        // its stand-in, if it has one (`MeshSource::stand_in`).
        let lods = by_map_size.as_ref().filter(|lods| lods.view == index);
        // The mesh it's drawn with in this view and the material it's drawn
        // through (`None` whole), or `None` to leave it out. A stand-in's
        // cut-out cards are always drawn through their holes: drawn whole,
        // a few big cards would cast big squares.
        let how = |caster: &Caster| -> Option<(MeshKey, Option<u32>)> {
            let (mesh, stand_in) = match lods {
                Some(lods) => lods.mesh(caster)?,
                None => (caster.mesh, false),
            };
            let material = caster
                .cut_out
                .filter(|cut_out| stand_in || near || cut_out.every_cascade)
                .map(|cut_out| cut_out.material);
            Some((mesh, material))
        };
        // Each share's casters that reach the view, with what they're
        // sorted by: how they're drawn, then where they are among the
        // casters (`caster_order`).
        let firsts: Vec<u32> = casters
            .iter()
            .scan(0, |first, piece| {
                let this = *first;
                *first += piece.len() as u32;
                Some(this)
            })
            .collect();
        let pieces: Vec<(u32, &Vec<Caster>)> = firsts.into_iter().zip(casters).collect();
        let parts = map_each(&pieces, share, |&(first, casters)| {
            let mut kept: Vec<(u128, &Caster)> = Vec::new();
            let mut left_out = 0;
            for (at, caster) in casters.iter().enumerate() {
                let drawn = view.touches(caster.sphere).then(|| how(caster)).flatten();
                if let Some((mesh, material)) = drawn {
                    let moving = lamp && super::lamp_shadows::moves(&caster.raw, windy);
                    let key = caster_order(moving, material, mesh, first + at as u32);
                    kept.push((key, caster));
                } else {
                    left_out += 1;
                }
            }
            (kept, left_out)
        });
        let mut left_out = 0;
        let mut order = Vec::with_capacity(parts.iter().map(|(kept, _)| kept.len()).sum());
        for (kept, left) in parts {
            left_out += left;
            order.extend(kept);
        }
        // Still casters first, and `None` (whole) first among each. Every
        // caster's key is its own, so the order is the same every frame.
        order.sort_unstable_by_key(|(key, _)| *key);
        let mut instances = Vec::with_capacity(order.len());
        let mut groups: Vec<(CasterGroup, Range<u32>)> = Vec::new();
        let mut still_groups = 0;
        let mut last_kind = None;
        for (key, caster) in order {
            let copy = instances.len() as u32;
            instances.push(caster.raw);
            let kind = key >> ORDER_PLACE_BITS;
            match groups.last_mut() {
                Some((_, range)) if last_kind == Some(kind) => range.end = copy + 1,
                _ => {
                    // Kept above, so it's drawn.
                    let drawn = how(caster).expect("a kept caster is drawn");
                    groups.push((drawn, copy..copy + 1));
                    if key >> 127 == 0 {
                        still_groups = groups.len();
                    }
                }
            }
            last_kind = Some(kind);
        }
        ViewCasters {
            instances: vec![instances],
            groups,
            still_groups,
            left_out,
        }
    })
}

/// The bits of [`caster_order`]'s key below how a caster is drawn: its
/// place among the frame's casters (up to 134 million of them).
const ORDER_PLACE_BITS: u32 = 27;

/// What a shadow view sorts a caster by, as one number, so sorting
/// thousands compares little: whether it moves (last), the material it's
/// drawn through (`None`, whole, first), its mesh, then its place among
/// the frame's casters (`place`), which makes every key its own. Casters
/// with the same bits above [`ORDER_PLACE_BITS`] draw together.
fn caster_order(moving: bool, material: Option<u32>, mesh: MeshKey, place: u32) -> u128 {
    debug_assert!(place < 1 << ORDER_PLACE_BITS);
    // 33 bits: 0 for none, else one more than the material's index.
    let material = material.map_or(0, |material| u128::from(material) + 1);
    let (kind, custom) = match mesh {
        MeshKey::Block => (0u128, 0),
        MeshKey::Sphere => (1, 0),
        MeshKey::Cylinder => (2, 0),
        MeshKey::Wedge => (3, 0),
        MeshKey::CornerWedge => (4, 0),
        MeshKey::Custom(key) => (5, key),
    };
    u128::from(moving) << 127
        | material << 94
        | kind << 91
        | u128::from(custom) << ORDER_PLACE_BITS
        | u128::from(place & ((1 << ORDER_PLACE_BITS) - 1))
}

pub(super) fn post_uniform(post: &PostProcessing) -> PostUniform {
    let flag = |on: bool| if on { 1.0 } else { 0.0 };
    let exposure = if post.enabled && post.exposure.is_finite() {
        post.exposure.max(0.0)
    } else {
        1.0
    };
    PostUniform {
        values: [
            post.bloom_threshold.max(0.0),
            post.bloom_intensity.max(0.0),
            exposure,
            0.0,
        ],
        flags: [
            flag(post.enabled && post.bloom_enabled),
            flag(post.enabled),
            post.tone_mapping.index(),
            0.0,
        ],
        // No lens effects until `lens::add_lens` adds them.
        lens: [0.0, 0.0, 0.0, 1.0],
        vignette: [0.0; 4],
        vignette_color: [0.0; 4],
    }
}

/// The shadow values for the shader (see [`FrameUniform`]).
pub(super) struct ShadowSetup {
    cascades: [Mat4; SHADOW_CASCADES],
    texels: [f32; 4],
    depths: [f32; 4],
    settings: [f32; 4],
}

/// Where each of `count` shadow cascades ends, in studs from the camera
/// along the view. The ends are spread so each cascade reaches a few times
/// further than the one before (see [`CASCADE_SPLIT_BLEND`]), and the last
/// ends at `distance`. Entries past `count` are `distance` too.
pub(super) fn cascade_ends(distance: f32, count: usize) -> [f32; SHADOW_CASCADES] {
    // From 1 stud, not the near plane, or the first cascade would be tiny.
    let near = 1.0_f32.min(distance * 0.5);
    let count = count.clamp(1, SHADOW_CASCADES);
    std::array::from_fn(|index| {
        let fraction = ((index + 1) as f32 / count as f32).min(1.0);
        let even = near + (distance - near) * fraction;
        let growing = near * (distance / near).powf(fraction);
        even + (growing - even) * CASCADE_SPLIT_BLEND
    })
}

/// Where ray traced sun shadows stop being used whole and start blending
/// into the shadow maps, as a share of their reach (`sun_visibility` in
/// lighting.wgsl, `cell_sun_visibility` in volumetric_fog.wgsl).
pub(super) const RAY_SHADOW_BLEND_START: f32 = 0.85;

/// A cascade's view that covers nothing: every point lands past its far
/// depth, which every lookup of the maps skips.
const COVERS_NOTHING: Mat4 = Mat4::from_cols(
    glam::Vec4::ZERO,
    glam::Vec4::ZERO,
    glam::Vec4::ZERO,
    glam::Vec4::new(0.0, 0.0, 2.0, 1.0),
);

/// How many of the sun's nearest cascades this frame's ray traced shadows
/// make unneeded (0 without them): those whose whole slice of the view,
/// to its far corners, lies within the distance where the traced shadow
/// is used whole ([`RAY_SHADOW_BLEND_START`] of its reach). The last
/// cascade is always drawn. What still reads the maps there (light through
/// a leaf, see-through surfaces, the edges the traced mask leaves out, the
/// sky over a reflection) reads the next cascade, a coarser one.
/// `AXCENITY_NO_CASCADE_SKIP` draws them all, to compare.
pub(super) fn cascades_under_rays(
    list: &DrawList,
    graphics: &EffectiveGraphics,
    aspect: f32,
) -> usize {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *OFF.get_or_init(|| std::env::var_os("AXCENITY_NO_CASCADE_SKIP").is_some()) {
        return 0;
    }
    let (Some(rays), Some((settings, map))) = (
        super::ray_shadows::RayShadowPlan::of(list, graphics),
        shadows_drawn(list.shadows, graphics),
    ) else {
        return 0;
    };
    let cascades = map.cascades as usize;
    let distance = settings.distance.clamp(NEAR_PLANE * 4.0, FAR_PLANE);
    let camera = &list.camera;
    let tan_half = (camera.fov_y_degrees.to_radians() * 0.5).tan();
    // How much further a slice's far corner is than its far end.
    let corner = (1.0 + tan_half * tan_half * (1.0 + aspect * aspect)).sqrt();
    let whole = rays.reach * RAY_SHADOW_BLEND_START;
    cascade_ends(distance, cascades)
        .into_iter()
        .take(cascades.saturating_sub(1))
        .take_while(|end| end * corner <= whole)
        .count()
}

/// The views of the sun's `cascades` (the frame's, as many as are drawn)
/// that the frame keeps items for: not the first `under_rays`
/// ([`cascades_under_rays`]), whose views cover nothing, and as a frustum
/// would take in the whole world (a plane with no direction rejects nothing
/// wide).
pub(super) fn cascade_views(
    cascades: &[[[f32; 4]; 4]],
    under_rays: usize,
) -> impl Iterator<Item = super::culling::Frustum> + '_ {
    cascades
        .iter()
        .skip(under_rays)
        .map(|matrix| super::culling::Frustum::of(Mat4::from_cols_array_2d(matrix)))
}

/// The sun's views of the shadow cascades and the shadow values for the
/// shader. Each cascade covers one slice of the view, so the nearest
/// shadows get the most texels per stud and are the sharpest. The area of
/// each is a sphere around its slice, so turning the camera doesn't change
/// the map's scale, and its position is snapped to whole texels so shadow
/// edges don't shimmer as the camera moves. `drawn` is the shadows as
/// drawn and their map (see [`shadows_drawn`]), or `None` when they're off.
/// The first `under_rays` cascades ([`cascades_under_rays`]) cover nothing
/// and the first one drawn takes their slices too, from the camera on.
pub(super) fn shadow_setup(
    list: &DrawList,
    drawn: Option<(Shadows, ShadowMapSize)>,
    (sun_direction, aspect): (Vec3, f32),
    under_rays: usize,
) -> ShadowSetup {
    let mut setup = ShadowSetup {
        cascades: [Mat4::IDENTITY; SHADOW_CASCADES],
        texels: [1.0; 4],
        depths: [1.0; 4],
        settings: [0.0; 4],
    };
    let Some((settings, map)) = drawn else {
        return setup;
    };
    let cascades = map.cascades as usize;
    let camera = &list.camera;
    let forward = (camera.look_at - camera.position).normalize_or(Vec3::NEG_Z);
    let distance = settings.distance.clamp(NEAR_PLANE * 4.0, FAR_PLANE);
    let tan_half = (camera.fov_y_degrees.to_radians() * 0.5).tan();
    let k2 = tan_half * tan_half * (1.0 + aspect * aspect);

    // Looking from the sun toward the scene.
    let toward_sun = sun_direction;
    let up_hint = if toward_sun.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let right = (-toward_sun).cross(up_hint).normalize();
    let up = right.cross(-toward_sun);

    let mut slice_near = NEAR_PLANE;
    let ends = cascade_ends(distance, cascades);
    for (index, far) in ends.into_iter().take(cascades).enumerate() {
        if index < under_rays {
            setup.cascades[index] = COVERS_NOTHING;
            continue;
        }
        let (center, radius) = slice_sphere(slice_near, far, k2);
        let sphere_center = camera.position + forward * center;
        slice_near = far;

        let texel = 2.0 * radius / map.size as f32;
        let snap = |distance: f32| (distance / texel).round() * texel;
        let snapped_center = right * snap(sphere_center.dot(right))
            + up * snap(sphere_center.dot(up))
            + toward_sun * sphere_center.dot(toward_sun);

        let back = radius + SHADOW_CASTER_RANGE;
        let eye = snapped_center + toward_sun * back;
        let view = rh::view::look_at_mat4(eye, snapped_center, up);
        let (near, far_depth) = (0.1, back + radius);
        let projection =
            rh::proj::directx::orthographic(-radius, radius, -radius, radius, near, far_depth);
        setup.cascades[index] = projection * view;
        setup.texels[index] = texel;
        setup.depths[index] = far_depth - near;
    }
    let softness = if settings.softness.is_finite() {
        settings.softness.clamp(0.0, 1.0)
    } else {
        0.0
    };
    setup.settings = [
        1.0,
        softness * MAX_SUN_SIZE,
        cascades as f32,
        settings.distance,
    ];
    setup
}

/// The smallest sphere around the slice of a view between `near` and `far`
/// studs along it: how far along the view its middle is, and its radius.
/// `k2` is the view's widening, `tan²(fov / 2) × (1 + aspect²)`.
fn slice_sphere(near: f32, far: f32, k2: f32) -> (f32, f32) {
    let center = (0.5 * (far + near) * (1.0 + k2)).min(far);
    let radius = ((far - center).powi(2) + far * far * k2).sqrt().max(1.0);
    (center, radius)
}

/// Everything that can stand between the sun and what the camera sees
/// within `distance` studs: the sun's view of a sphere around that much of
/// the camera's view, from as far back as a cascade's
/// ([`SHADOW_CASTER_RANGE`]). Ray traced shadows need it of a frame whose
/// shadow maps don't cover that distance: their rays leave the picture.
pub(super) fn sun_reach(list: &DrawList, distance: f32, aspect: f32) -> Mat4 {
    let camera = &list.camera;
    let forward = (camera.look_at - camera.position).normalize_or(Vec3::NEG_Z);
    let distance = distance.clamp(NEAR_PLANE * 4.0, FAR_PLANE);
    let tan_half = (camera.fov_y_degrees.to_radians() * 0.5).tan();
    let k2 = tan_half * tan_half * (1.0 + aspect * aspect);
    let (center, radius) = slice_sphere(NEAR_PLANE, distance, k2);
    let middle = camera.position + forward * center;
    let toward_sun = list.sun.direction.normalize_or(Vec3::Y);
    let up = if toward_sun.y.abs() > 0.99 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let back = radius + SHADOW_CASTER_RANGE;
    let view = rh::view::look_at_mat4(middle + toward_sun * back, middle, up);
    rh::proj::directx::orthographic(-radius, radius, -radius, radius, 0.1, back + radius) * view
}

/// What a frame's draws use, each thing once, so that the work per material
/// and per mesh follows how many there are and not how many draws share them.
pub(super) struct FrameUses<'a> {
    /// Every distinct material binding. The first is the plain one.
    pub(super) materials: Vec<MaterialKey>,
    /// Which of `materials` each draw is drawn with.
    pub(super) material_of: Vec<u32>,
    /// A draw using each of `materials`, the first one (0 for the plain
    /// material, which may have none).
    pub(super) first_draw: Vec<u32>,
    /// Every distinct mesh of a caller's the draws use, by address.
    pub(super) meshes: Vec<&'a Arc<dyn crate::MeshSource>>,
}

impl<'a> FrameUses<'a> {
    /// Works out what `draws` use. The list's own items' draws come first
    /// and are shared among the CPU's threads; the draws of items in
    /// groups follow them, and take their material from their group's
    /// cluster (`clusters`, in [`Draw::cluster`]'s order), where it was
    /// worked out once. Either way a material's place in
    /// [`FrameUses::materials`] is where the draws first come to it.
    pub(super) fn of(draws: &'a [Draw<'_>], clusters: &[&'a Cluster]) -> Self {
        let own = draws.partition_point(|draw| draw.cluster == NO_CLUSTER);
        let (own_draws, clustered) = draws.split_at(own);
        // A part's materials, each once, with the first draw of the part
        // to use it.
        // A binding's key takes a few hundred nanoseconds to work out, so
        // even the few hundred own items of a world that keeps its plants
        // in groups are worth sharing (0.6 ms on one thread for Desert
        // Homestead's 1,659).
        let parts = map_heavy_parts(own_draws, (512, 128), |draws| {
            let mut distinct: Vec<(MaterialKey, u32, u64)> = Vec::new();
            let mut places = MaterialPlaces::default();
            let mut material_of = Vec::with_capacity(draws.len());
            let mut seen_meshes: FxHashSet<*const ()> = FxHashSet::default();
            let mut meshes = Vec::new();
            let mut last_arrays = LastArrays::default();
            for (at, draw) in draws.iter().enumerate() {
                let key = material_key_with(draw.item, &mut last_arrays);
                let digest = key_digest(&key);
                let index = places
                    .find(digest, &key, &distinct, |(key, ..)| key)
                    .unwrap_or_else(|| {
                        distinct.push((key, at as u32, digest));
                        places.note(digest, distinct.len() as u32 - 1)
                    });
                material_of.push(index);
                if let Some(source) = draw.source()
                    && seen_meshes.insert(Arc::as_ptr(source).cast())
                {
                    meshes.push(source);
                }
            }
            (draws.len() as u32, distinct, material_of, meshes)
        });
        let mut uses = Self {
            materials: vec![MaterialKey::default()],
            material_of: Vec::with_capacity(draws.len()),
            first_draw: vec![0],
            meshes: Vec::new(),
        };
        // Room for every part's materials at once (a terrain's pieces are
        // a material each): growing as they come moves them all again
        // each time.
        let most: usize = parts.iter().map(|(_, distinct, ..)| distinct.len()).sum();
        uses.materials.reserve(most);
        uses.first_draw.reserve(most);
        let mut places = MaterialPlaces::default();
        places.0.reserve(most + 1);
        places.note(key_digest(&MaterialKey::default()), 0);
        let mut seen_meshes: FxHashSet<*const ()> = FxHashSet::default();
        let mut start = 0;
        for (len, distinct, material_of, meshes) in parts {
            let remap: Vec<u32> = distinct
                .into_iter()
                .map(|(key, first, digest)| uses.place(&mut places, key, digest, start + first))
                .collect();
            uses.material_of
                .extend(material_of.into_iter().map(|index| remap[index as usize]));
            uses.meshes.extend(
                meshes
                    .into_iter()
                    .filter(|source| seen_meshes.insert(Arc::as_ptr(source).cast())),
            );
            start += len;
        }
        // The draws of items in groups, shared among the threads. A part
        // lists the materials it comes to, each once as (its cluster, its
        // place in the cluster's own, the first draw of the part to use
        // it), with each draw's place in that list, and the meshes it
        // comes to. A cluster's draws follow one another, so a part keeps
        // one cluster's places at a time (a cluster met again only lists
        // its materials again, which the joining below puts right).
        let parts = map_parts(clustered, |draws| {
            let mut found: Vec<(u32, u32, u32)> = Vec::new();
            let mut material_of = Vec::with_capacity(draws.len());
            let mut meshes = Vec::new();
            let mut seen_meshes: FxHashSet<*const ()> = FxHashSet::default();
            let mut current = u32::MAX;
            let mut places: Vec<u32> = Vec::new();
            for (at, draw) in draws.iter().enumerate() {
                let cluster = clusters[draw.cluster as usize];
                if draw.cluster != current {
                    current = draw.cluster;
                    places.clear();
                    places.resize(cluster.materials.len(), u32::MAX);
                }
                let local = cluster.material_of[draw.at as usize];
                let place = &mut places[local as usize];
                if *place == u32::MAX {
                    found.push((draw.cluster, local, at as u32));
                    *place = found.len() as u32 - 1;
                }
                material_of.push(*place);
                // A level of detail's mesh takes the item's own's place.
                let source = match (draw.lod, cluster.mesh_of[draw.at as usize]) {
                    (Some(lod), _) => Some(lod),
                    (None, BUILT_IN) => None,
                    (None, local) => Some(&cluster.meshes[local as usize]),
                };
                if let Some(source) = source
                    && seen_meshes.insert(Arc::as_ptr(source).cast())
                {
                    meshes.push(source);
                }
            }
            (draws.len(), found, material_of, meshes)
        });
        // Joined in order, so a material's place in the frame's is where
        // the draws first come to it, as it would be on one thread: the
        // order of draws with one mesh follows it, and with that order
        // which of two surfaces at one depth shows. Each cluster's
        // materials are looked up in the frame's once and remembered here.
        let first_of: Vec<usize> = clusters
            .iter()
            .scan(0, |next, cluster| {
                let first = *next;
                *next += cluster.materials.len();
                Some(first)
            })
            .collect();
        let known = clusters.iter().map(|c| c.materials.len()).sum();
        let mut in_frame = vec![u32::MAX; known];
        let mut start = own;
        for (len, found, material_of, meshes) in parts {
            let remap: Vec<u32> = found
                .into_iter()
                .map(|(cluster, local, first)| {
                    let index = &mut in_frame[first_of[cluster as usize] + local as usize];
                    if *index == u32::MAX {
                        let cluster = clusters[cluster as usize];
                        let key = cluster.materials[local as usize];
                        let digest = cluster.digests[local as usize];
                        let first = (start + first as usize) as u32;
                        *index = uses.place(&mut places, key, digest, first);
                    }
                    *index
                })
                .collect();
            uses.material_of
                .extend(material_of.into_iter().map(|index| remap[index as usize]));
            uses.meshes.extend(
                meshes
                    .into_iter()
                    .filter(|source| seen_meshes.insert(Arc::as_ptr(source).cast())),
            );
            start += len;
        }
        uses
    }
}

impl<'a> FrameUses<'a> {
    /// Adds the simpler copies (`MeshSource::lod`) of meshes `draws` draw
    /// to [`FrameUses::meshes`], so they're on the GPU for a shadow view
    /// that draws a caster at another level than the camera
    /// ([`FarCascadeLods`], which draws the camera's mesh until its copy is
    /// there). A frame takes one [`LOD_TURNS`]th of the list's own items and
    /// of the clusters (`turn`), so each mesh's copies are listed, and kept
    /// as drawn, every few frames at little cost; a copy listed twice is
    /// only looked up twice.
    pub(super) fn add_lods(&mut self, draws: &'a [Draw<'_>], clusters: &[&'a Cluster], turn: u64) {
        let mine = |index: usize| index as u64 % LOD_TURNS == turn % LOD_TURNS;
        let own = draws.partition_point(|draw| draw.cluster == NO_CLUSTER);
        let own_sources = draws[..own]
            .iter()
            .enumerate()
            .filter(|&(index, _)| mine(index))
            .filter_map(|(_, draw)| match &draw.item.shape {
                crate::Shape::Mesh(source) => Some(source),
                _ => None,
            });
        let cluster_sources = clusters
            .iter()
            .enumerate()
            .filter(|&(index, _)| mine(index))
            .flat_map(|(_, cluster)| cluster.meshes.iter());
        for source in own_sources.chain(cluster_sources) {
            self.meshes
                .extend((1..=source.lod_count()).filter_map(|level| source.lod(level)));
        }
    }
}

impl<'a> FrameUses<'a> {
    /// Adds the stand-ins (`MeshSource::stand_in`) of meshes `draws` draw
    /// to [`FrameUses::meshes`], so they're on the GPU for the farthest
    /// cascade and the ray traced scene, which use the mesh drawn until
    /// its stand-in is there. A frame takes one [`LOD_TURNS`]th of the
    /// list's own items and of the clusters (`turn`), as
    /// [`FrameUses::add_lods`] does.
    pub(super) fn add_stand_ins(
        &mut self,
        draws: &'a [Draw<'_>],
        clusters: &[&'a Cluster],
        turn: u64,
    ) {
        if super::types::stand_ins_off() {
            return;
        }
        let mine = |index: usize| index as u64 % LOD_TURNS == turn % LOD_TURNS;
        let own = draws.partition_point(|draw| draw.cluster == NO_CLUSTER);
        let own_sources = draws[..own]
            .iter()
            .enumerate()
            .filter(|&(index, _)| mine(index))
            .filter_map(|(_, draw)| match &draw.item.shape {
                crate::Shape::Mesh(source) => Some(source),
                _ => None,
            });
        let cluster_sources = clusters
            .iter()
            .enumerate()
            .filter(|&(index, _)| mine(index))
            .flat_map(|(_, cluster)| cluster.meshes.iter());
        for source in own_sources.chain(cluster_sources) {
            match source.stand_in() {
                crate::StandIn::Mesh(stand_in) | crate::StandIn::Covered(stand_in) => {
                    self.meshes.push(stand_in)
                }
                crate::StandIn::Own => {}
            }
        }
    }
}

/// How many frames [`FrameUses::add_lods`] takes to list every mesh's
/// simpler copies once.
const LOD_TURNS: u64 = 8;

impl FrameUses<'_> {
    /// Where `key` (whose digest is `digest`) is among the frame's
    /// materials, added as first used by draw `first` if it's new.
    fn place(
        &mut self,
        places: &mut MaterialPlaces,
        key: MaterialKey,
        digest: u64,
        first: u32,
    ) -> u32 {
        places
            .find(digest, &key, &self.materials, |key| key)
            .unwrap_or_else(|| {
                self.materials.push(key);
                self.first_draw.push(first);
                places.note(digest, self.materials.len() as u32 - 1)
            })
    }
}

/// A material key's digest, which [`MaterialPlaces`] finds it by.
pub(super) fn key_digest(key: &MaterialKey) -> u64 {
    use std::hash::BuildHasher;
    rustc_hash::FxBuildHasher.hash_one(key)
}

/// Where each distinct material key is in a list of them, by the key's
/// digest. A key is 480 bytes: a map of the keys themselves hashes and
/// moves that much for each one it takes in, and Desert Homestead's frame
/// has 1,500 (0.23 ms on the frame's thread; 0.05 this way, the digests
/// worked out on the threads that work out the keys).
#[derive(Default)]
struct MaterialPlaces(FxHashMap<u64, u32>);

impl MaterialPlaces {
    /// `key`'s place among `keys`, each of which was noted here with its
    /// digest and place, if it's one of them. `key_of` reads an entry's key.
    fn find<K>(
        &self,
        digest: u64,
        key: &MaterialKey,
        keys: &[K],
        key_of: impl Fn(&K) -> &MaterialKey,
    ) -> Option<u32> {
        match self.0.get(&digest) {
            Some(&at) if key_of(&keys[at as usize]) == key => Some(at),
            // Two keys with one digest: only the first is noted, so the
            // other is looked for among them all.
            Some(_) => keys
                .iter()
                .position(|other| key_of(other) == key)
                .map(|at| at as u32),
            None => None,
        }
    }

    /// Notes that the key with `digest` is at `at`, and returns `at`.
    fn note(&mut self, digest: u64, at: u32) -> u32 {
        self.0.entry(digest).or_insert(at);
        at
    }
}

/// What the farthest cascade needs to draw casters at the level of detail
/// their size in its map calls for (`GraphicsSettings::shadow_lod`).
pub(super) struct FarCascadeLods<'a> {
    /// The cascade's index among the shadow views.
    pub(super) view: usize,
    /// How wide its map is, in studs.
    pub(super) extent: f32,
    /// Whether a mesh is on the GPU to draw with.
    pub(super) uploaded: &'a (dyn Fn(MeshKey) -> bool + Sync),
}

impl FarCascadeLods<'_> {
    /// The mesh `caster` is drawn with in this cascade, and whether it's
    /// its stand-in: the camera's, a simpler copy where its size in the map
    /// calls for one, or from `STAND_IN_LEVEL` its stand-in once that's on
    /// the GPU. `None` leaves it out (another mesh's stand-in covers it).
    fn mesh(&self, caster: &Caster) -> Option<(MeshKey, bool)> {
        let level = shadow_level(2.0 * caster.sphere.1 / self.extent);
        let drawn = self.copy(caster, level);
        let level = level.max(usize::from(caster.level));
        let mesh = caster
            .stand_in
            .mesh(level, drawn, |mesh| (self.uploaded)(mesh))?;
        Some((mesh, mesh != drawn))
    }

    /// The simplest of `caster`'s copies at or under `level` that the GPU
    /// has, or the camera's mesh when that's as simple.
    fn copy(&self, caster: &Caster, level: usize) -> MeshKey {
        if level <= usize::from(caster.level) {
            return caster.mesh;
        }
        // The simplest copy at or under that level that the GPU has.
        (usize::from(caster.level) + 1..=level)
            .rev()
            .filter_map(|level| caster.lods.get(level - 1).copied().flatten())
            .find(|&mesh| (self.uploaded)(mesh))
            .unwrap_or(caster.mesh)
    }
}

/// The level of detail an item `size` across, as a fraction of a shadow
/// map's width, is drawn with there: as the camera picks it by the share
/// of the screen's height (`lod::LOD_SCREEN_SIZES`).
pub(super) fn shadow_level(size: f32) -> usize {
    super::lod::LOD_SCREEN_SIZES
        .iter()
        .filter(|&&threshold| size < threshold)
        .count()
}

/// A solid item that casts a shadow, whether or not the camera sees it.
#[derive(Clone, Copy)]
pub(super) struct Caster {
    pub(super) mesh: MeshKey,
    /// The level of detail it's drawn with in the camera's view (0 for its
    /// own mesh), and its mesh's simpler copies (`MeshSource::lod`), for
    /// the farthest cascade to draw it at the level its size in that
    /// shadow map calls for ([`shadow_level`]).
    pub(super) level: u8,
    pub(super) lods: [Option<MeshKey>; 2],
    /// What casts in its place in the farthest cascade from
    /// `STAND_IN_LEVEL` ([`FarCascadeLods::mesh`]).
    pub(super) stand_in: StandInKey,
    /// A cut-out's material: its shadow is drawn through its texture's
    /// holes (`fs_shadow_cut_out`). `None` casts the whole mesh.
    pub(super) cut_out: Option<CutOutCaster>,
    pub(super) sphere: Sphere,
    pub(super) raw: InstanceRaw,
}

/// A caster whose shadow is drawn through its texture's holes, or with a
/// share of it left out (`InstanceRaw::emissive_fade`'s last value, which a
/// caster's copy holds [`Draw::shadow`] in).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct CutOutCaster {
    /// Its material, as its index in the frame's [`FrameUses::materials`],
    /// or [`CutOutCaster::NO_HOLES`].
    pub(super) material: u32,
    /// A hair card, or a caster near the end of its shadow's reach: drawn
    /// this way in every one of the sun's cascades, not only the nearest
    /// ([`cut_out_cascades`]).
    pub(super) every_cascade: bool,
}

impl CutOutCaster {
    /// The material of a caster with no holes of its own, drawn this way
    /// only to leave a share of its shadow out: no frame has this many
    /// materials, so the shadow pass binds the plain one.
    pub(super) const NO_HOLES: u32 = u32::MAX;
}

/// Which casters a shadow view draws together: one mesh, and for a
/// caster that view draws through its holes, its material
/// ([`CutOutCaster::material`]); `None` casts the whole mesh.
pub(super) type CasterGroup = (MeshKey, Option<u32>);

/// What a frame draws (see [`plan_draws`]).
#[derive(Default)]
pub(super) struct DrawPlan {
    /// The scene pass's instances, in pieces (each thread's share, in
    /// order). Shadow views copy theirs after these.
    pub(super) instances: Vec<Vec<InstanceRaw>>,
    /// The scene pass's batches: the items in the camera's view.
    pub(super) batches: Vec<Batch>,
    /// Every solid item that casts a shadow, in view or not, grouped by
    /// mesh: one out of view can still shadow what's in it. In pieces, as
    /// `instances` are.
    pub(super) casters: Vec<Vec<Caster>>,
    /// Items outside the camera's view.
    pub(super) culled: u32,
    /// Every drawn skin's joints, each skin's after the one before.
    pub(super) joints: Vec<JointRaw>,
    /// Where each drawn skin's joints start in `joints`, by the skin's
    /// address: items posed alike share them.
    pub(super) skins: HashMap<*const Skin, u32>,
    /// Each (skin, mesh) pair the plan's instances and casters are posed
    /// as (CO8, `posing.rs`). Until the frame knows which it poses and
    /// where, an instance or caster of one holds `[its index here,
    /// PENDING]` as its `skin`.
    pub(super) posed: Vec<PoseUse>,
    /// Items in view with levels of detail, by level (see
    /// [`RenderStats::lod_items`](crate::RenderStats::lod_items)).
    pub(super) lod_items: [u32; crate::LOD_LEVELS],
    /// Which of the frame's draws the camera sees, by the draw's index:
    /// the others are planned only as shadow casters.
    pub(super) in_view: Vec<bool>,
}

/// Puts the `draws` (`list`'s items at their levels of detail, see
/// [`LodStates::resolve`](super::lod::LodStates::resolve)) that `view` can
/// see in drawing order and groups them into batches, and lists the shadow
/// casters for the shadow views to pick from.
///
/// Solid items come first, grouped by mesh, texture and sidedness. See-through items come last,
/// farthest from the camera first, so each one blends over what's behind it.
/// They're sorted by their centers, so two that overlap can still blend in the
/// wrong order; that's the usual trade-off for this simple approach.
///
/// An item is culled when its bounding sphere is wholly outside `view`.
/// `bounds` gives each shape's box in its own space, posed by the item's
/// skin if it has one: the unit cube for the built-in shapes, its corners'
/// box for a caller's mesh. Each item's sphere and whether it was drawn go
/// in `seen`, if given.
pub(super) fn plan_draws(
    list: &DrawList,
    draws: &[Draw],
    uses: &FrameUses,
    view: &Frustum,
    bounds: impl Fn(MeshKey, Option<&Skin>) -> Bounds + Sync,
    seen: Option<&mut Vec<CullingBounds>>,
) -> DrawPlan {
    debug_assert_eq!(draws.len(), uses.material_of.len());
    // Each draw's sort key, worked out once: its mesh, its material as an
    // index among the frame's distinct materials (the plain one is 0), and
    // its pipeline. A material key is hundreds of bytes, so the sort
    // compares these instead.
    let keys = map_indexes(draws.len(), |index| {
        let draw = &draws[index];
        let material = &draw.item.material;
        let part = drawn_part(draw.item);
        let shader = shader_key(draw.item);
        let cut_out = material.alpha_cutoff > 0.0;
        // Fading changes the shader a solid item is lit by only for the
        // standard material's, with no holes (`Batch::fading`).
        let whole = !cut_out
            && shader.is_none()
            && part != DrawnPart::Terrain
            && material.opacity >= 1.0
            && material.water.is_none()
            && draw.item.decal_role != DecalRole::Decal;
        let fading = whole && draw.fade != OPAQUE;
        let key = SortKey {
            band: distance_band(draw.item.transform.w_axis.truncate(), list.camera.position),
            part,
            pipeline: pipeline_rank(shader, material.double_sided, cut_out, fading),
            mesh: draw.mesh(),
            material: uses.material_of[index],
            shader,
            double_sided: material.double_sided,
            casts_shadow: draw.casts_shadow,
            cut_out,
        };
        // Water is drawn with the see-through items, before them (see
        // renderer/water.rs), and a Part's Decal is blended by its
        // texture's alpha even when solid. Worked out here, on the threads,
        // so the split below reads no item.
        let see_through = material.opacity < 1.0
            || material.water.is_some()
            || draw.item.decal_role == DecalRole::Decal;
        let class = match (material.opacity > 0.0, see_through) {
            (false, _) => DrawClass::Unseen,
            (true, true) => DrawClass::SeeThrough,
            (true, false) => DrawClass::Solid,
        };
        (key, class)
    });
    let water = |index: u32| draws[index as usize].item.material.water.is_some();
    let mut solid: Vec<u32> = Vec::with_capacity(keys.len());
    let mut transparent: Vec<(SortKey, u32)> = Vec::new();
    for (index, &(key, class)) in keys.iter().enumerate() {
        match class {
            DrawClass::Solid => solid.push(index as u32),
            DrawClass::SeeThrough => transparent.push((key, index as u32)),
            DrawClass::Unseen => {}
        }
    }

    // By key then index, which keeps the draws' order where keys are equal.
    let opaque = sorted_draws(&solid, |index| keys[index as usize].0, uses);
    let distance = |index: u32| {
        draws[index as usize]
            .item
            .transform
            .w_axis
            .truncate()
            .distance_squared(list.camera.position)
    };
    // `total_cmp` orders floats even when one is NaN, so sorting can't panic.
    transparent.sort_by(|(_, a), (_, b)| {
        water(*b)
            .cmp(&water(*a))
            .then_with(|| distance(*b).total_cmp(&distance(*a)))
    });

    let ordered: Vec<(SortKey, u32, bool)> = opaque
        .into_iter()
        .map(|(key, index)| (key, index, false))
        .chain(
            transparent
                .into_iter()
                .map(|(key, index)| (key, index, true)),
        )
        .collect();
    let keep_seen = seen.is_some();
    // Only while the occlusion probe measures (`occlusion_probe.rs`).
    let occluders = super::occlusion_probe::image();
    let parts = map_parts(&ordered, |part| {
        plan_part(
            part,
            draws,
            uses,
            view,
            occluders.as_deref(),
            &bounds,
            keep_seen,
        )
    });
    if occluders.is_some() {
        super::occlusion_probe::count(parts.iter().map(|part| part.hidden).sum());
    }
    join_plan(parts, seen, draws.len())
}

/// Where a draw goes in [`plan_draws`]: with the solid items, with the
/// see-through ones, or nowhere (fully transparent).
#[derive(Clone, Copy, PartialEq, Eq)]
enum DrawClass {
    Solid,
    SeeThrough,
    Unseen,
}

/// The draws `indices`, each with its key (`key_of`), sorted by key and
/// then index, as `sort_unstable` would sort them. Where every key fits,
/// they are sorted as numbers ([`packed_key`]), on the threads, and the
/// keys looked up again after: sorting whole [`SortKey`]s, splitting them
/// off on one thread and copying them was 2.2 ms of Overgrown Street's
/// frame for its 124,000 draws.
fn sorted_draws(
    indices: &[u32],
    key_of: impl Fn(u32) -> SortKey + Sync,
    uses: &FrameUses,
) -> Vec<(SortKey, u32)> {
    // Each mesh by its place among the frame's meshes, in `MeshKey`'s order.
    let mut meshes: Vec<MeshKey> = [
        MeshKey::Block,
        MeshKey::Sphere,
        MeshKey::Cylinder,
        MeshKey::Wedge,
        MeshKey::CornerWedge,
    ]
    .into_iter()
    .chain(
        uses.meshes
            .iter()
            .map(|source| MeshKey::Custom(source.key())),
    )
    .collect();
    meshes.sort_unstable();
    meshes.dedup();
    let packed = map_parts(indices, |part| {
        part.iter()
            .map(|&index| packed_key(&key_of(index), index, &meshes))
            .collect::<Option<Vec<u128>>>()
    });
    match packed.into_iter().collect::<Option<Vec<Vec<u128>>>>() {
        Some(parts) => {
            let mut packed: Vec<u128> = parts.into_iter().flatten().collect();
            sort_unstable(&mut packed);
            map_parts(&packed, |part| {
                part.iter()
                    .map(|&packed| {
                        let index = packed as u32;
                        (key_of(index), index)
                    })
                    .collect::<Vec<_>>()
            })
            .into_iter()
            .flatten()
            .collect()
        }
        None => {
            let mut keyed: Vec<(SortKey, u32)> = indices
                .iter()
                .map(|&index| (key_of(index), index))
                .collect();
            sort_unstable(&mut keyed);
            keyed
        }
    }
}

/// `key` and `index` as one number that sorts as they do (key, then
/// index), or `None` where a part doesn't fit or a surface shader is in it:
/// 8 bits of band, 4 of part, 31 of pipeline, 20 of the mesh's place in
/// `meshes`, 20 of material, 3 of flags and 32 of index.
fn packed_key(key: &SortKey, index: u32, meshes: &[MeshKey]) -> Option<u128> {
    if key.shader.is_some() || key.pipeline >= 1 << 31 || key.material >= 1 << 20 {
        return None;
    }
    let mesh = meshes.binary_search(&key.mesh).ok()? as u128;
    if mesh >= 1 << 20 {
        return None;
    }
    let part = key.part as u128;
    debug_assert!(part < 16);
    let flags = u128::from(key.double_sided) << 2
        | u128::from(key.casts_shadow) << 1
        | u128::from(key.cut_out);
    Some(
        u128::from(key.band) << 120
            | part << 116
            | u128::from(key.pipeline) << 85
            | mesh << 65
            | u128::from(key.material) << 45
            | flags << 42
            | u128::from(index),
    )
}

/// What makes two draws one batch.
type BatchKind = (SortKey, bool, bool, bool);

/// One (skin, mesh) pair a plan poses: the first draw (index into the
/// frame's draws) it was seen on, its mesh, and where its skin's joints
/// start in [`DrawPlan::joints`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PoseUse {
    pub(super) draw: u32,
    pub(super) mesh: MeshKey,
    pub(super) joints: u32,
}

/// A skin's joints wanted by an instance or caster of a [`PlanPart`], each
/// counted from the part's start, drawn with `mesh` by draw `draw`.
struct SkinUse<'a> {
    skin: &'a Skin,
    mesh: MeshKey,
    draw: u32,
    instance: Option<u32>,
    caster: Option<u32>,
}

/// One thread's share of [`plan_draws`], with its instances and casters
/// counted from its own start.
#[derive(Default)]
struct PlanPart<'a> {
    instances: Vec<InstanceRaw>,
    batches: Vec<(BatchKind, Batch)>,
    casters: Vec<Caster>,
    culled: u32,
    /// Of `culled`, those the occlusion probe hid.
    hidden: u32,
    lod_items: [u32; crate::LOD_LEVELS],
    seen: Vec<CullingBounds>,
    skins: Vec<SkinUse<'a>>,
    /// The draws the camera sees, by index.
    in_view: Vec<u32>,
}

/// Plans the draws `ordered` picks, in that order.
fn plan_part<'a>(
    ordered: &[(SortKey, u32, bool)],
    draws: &[Draw<'a>],
    uses: &FrameUses,
    view: &Frustum,
    occluders: Option<&super::occlusion_probe::DepthImage>,
    bounds: &(impl Fn(MeshKey, Option<&Skin>) -> Bounds + Sync),
    keep_seen: bool,
) -> PlanPart<'a> {
    let mut part = PlanPart::default();
    // Items sharing a mesh are next to each other, so its bounds are looked
    // up once per run of them (unless a skin moves them).
    let mut last_bounds: Option<(MeshKey, Bounds)> = None;
    let mut colors = LinearColors::default();
    // The last batch's key and kind, to add to it without comparing the
    // batch's material key.
    let mut last_batch: Option<BatchKind> = None;
    for &(key, at, transparent) in ordered {
        let draw = &draws[at as usize];
        let mesh = key.mesh;
        let item = draw.item;
        let skin = item.skin.as_deref();
        let shape_bounds = match (last_bounds, skin) {
            (_, Some(skin)) => bounds(mesh, Some(skin)),
            (Some((last, bounds)), None) if last == mesh => bounds,
            (_, None) => {
                let bounds = bounds(mesh, None);
                last_bounds = Some((mesh, bounds));
                bounds
            }
        };
        let (center, radius) = shape_bounds.sphere(item.transform);
        // A swaying plant reaches past where it rests.
        let sphere = (center, radius + item.sway.map_or(0.0, |sway| sway.reach()));
        let mut visible = view.touches(sphere);
        if visible && occluders.is_some_and(|depth| depth.hides(sphere)) {
            visible = false;
            part.hidden += 1;
        }
        if keep_seen {
            part.seen.push(CullingBounds {
                center: sphere.0,
                radius: sphere.1,
                drawn: visible,
            });
        }
        let caster = !transparent && draw.casts_shadow;
        if !visible {
            part.culled += 1;
            if !caster {
                continue;
            }
        }
        let Some(mut raw) = instance_raw_with(item, &mut colors) else {
            continue;
        };
        // A cut-out (no surface shader, terrain or water) casts its shadow
        // through its texture's holes: a plant's leaves and a head's
        // strands, not their cards (`cut_outs_cast_whole` has what that
        // costs, and the switch back). One whose texture is tiled by its
        // size (`uv_tile`) casts its whole mesh: `vs_shadow_cut_out` reads
        // the mesh's own coordinates, so its holes would fall at the wrong
        // scale (as `ray_cut_outs` leaves tiled items out).
        let tiled = item.uv_tile.is_finite() && item.uv_tile > 0.0;
        let hair = matches!(item.material.shading, Shading::Hair(_));
        let cut_out = (key.cut_out
            && (hair || !cut_outs_cast_whole())
            && !tiled
            && key.shader.is_none()
            && item.material.terrain.is_none()
            && item.material.water.is_none())
        .then_some(CutOutCaster {
            material: key.material,
            every_cascade: hair,
        });
        // Near the end of its shadow's reach a caster casts a share of its
        // shadow (`Draw::shadow`): the cut-out pipeline leaves the rest
        // out, dithered, whether or not it has holes of its own.
        let thinning = draw.shadow < 1.0;
        let cut_out = match cut_out {
            Some(cut_out) => Some(CutOutCaster {
                every_cascade: cut_out.every_cascade || thinning,
                ..cut_out
            }),
            None => thinning.then_some(CutOutCaster {
                material: CutOutCaster::NO_HOLES,
                every_cascade: true,
            }),
        };
        let caster_at = caster.then(|| {
            let mut raw = raw;
            raw.emissive_fade[3] = draw.shadow;
            let lods = match &item.shape {
                crate::Shape::Mesh(source) => [1, 2].map(|level| {
                    (level <= source.lod_count())
                        .then(|| source.lod(level))
                        .flatten()
                        .map(|lod| MeshKey::Custom(lod.key()))
                }),
                _ => [None; 2],
            };
            part.casters.push(Caster {
                mesh,
                level: draw.level.unwrap_or(0) as u8,
                lods,
                stand_in: StandInKey::of(&item.shape),
                cut_out,
                sphere,
                raw,
            });
            (part.casters.len() - 1) as u32
        });
        raw.emissive_fade[3] = draw.fade;
        let mut instance_at = None;
        if visible {
            // The copy fading out of a switch isn't counted.
            if let Some(level) = draw.level
                && draw.fade <= OPAQUE
            {
                part.lod_items[level] += 1;
            }
            let index = part.instances.len() as u32;
            instance_at = Some(index);
            part.instances.push(raw);
            part.in_view.push(at);
            let water = item.material.water.is_some();
            let outline = !transparent && outline_uniform(&item.material)[3] > 0.0;
            // A solid item of the standard material with no holes, of
            // which only some pixels are drawn.
            let fading = draw.fade != OPAQUE
                && !transparent
                && !water
                && !key.cut_out
                && key.shader.is_none()
                && key.part != DrawnPart::Terrain;
            let kind = (key, transparent, water, outline);
            match part.batches.last_mut() {
                Some((_, last)) if last_batch == Some(kind) => {
                    last.instances.end = index + 1;
                    last.fading |= fading;
                }
                _ => {
                    last_batch = Some(kind);
                    part.batches.push((
                        kind,
                        Batch {
                            mesh,
                            material: uses.materials[key.material as usize],
                            transparent,
                            double_sided: key.double_sided,
                            casts_shadow: key.casts_shadow,
                            shader: key.shader,
                            water,
                            outline,
                            // A surface shader or terrain decides its own
                            // holes, and water isn't drawn with depth.
                            prefilled: key.cut_out
                                && !transparent
                                && !water
                                && key.shader.is_none()
                                && item.material.terrain.is_none(),
                            fading,
                            part: key.part,
                            instances: index..index + 1,
                        },
                    ));
                }
            }
        }
        if let Some(skin) = skin {
            part.skins.push(SkinUse {
                skin,
                mesh,
                draw: at,
                instance: instance_at,
                caster: caster_at,
            });
        }
    }
    part
}

/// Puts `parts` together, in order: a batch cut in two by where a share of
/// the work ended is made whole, each skin's joints are added once, and
/// each (skin, mesh) pair is listed once.
fn join_plan(
    parts: Vec<PlanPart>,
    seen: Option<&mut Vec<CullingBounds>>,
    draws: usize,
) -> DrawPlan {
    let mut plan = DrawPlan {
        in_view: vec![false; draws],
        ..DrawPlan::default()
    };
    let mut first_instance = 0;
    let mut last_kind: Option<BatchKind> = None;
    // Where each skin's joints start, by the skin's address: items posed
    // alike share one.
    let mut skins: HashMap<*const Skin, u32> = HashMap::new();
    let mut pairs: HashMap<(*const Skin, MeshKey), u32> = HashMap::new();
    let mut seen = seen;
    for mut part in parts {
        plan.culled += part.culled;
        for draw in part.in_view {
            plan.in_view[draw as usize] = true;
        }
        for (level, count) in part.lod_items.into_iter().enumerate() {
            plan.lod_items[level] += count;
        }
        if let Some(seen) = seen.as_deref_mut() {
            seen.append(&mut part.seen);
        }
        for (kind, mut batch) in part.batches {
            batch.instances =
                batch.instances.start + first_instance..batch.instances.end + first_instance;
            match plan.batches.last_mut() {
                Some(last) if last_kind == Some(kind) => {
                    last.instances.end = batch.instances.end;
                    last.fading |= batch.fading;
                }
                _ => {
                    last_kind = Some(kind);
                    plan.batches.push(batch);
                }
            }
        }
        for used in part.skins {
            let joints = &mut plan.joints;
            let first = *skins
                .entry(std::ptr::from_ref(used.skin))
                .or_insert_with(|| {
                    let first = joints.len() as u32;
                    joints.extend(JointRaw::of(used.skin));
                    first
                });
            let posed = &mut plan.posed;
            let pair = *pairs
                .entry((std::ptr::from_ref(used.skin), used.mesh))
                .or_insert_with(|| {
                    posed.push(PoseUse {
                        draw: used.draw,
                        mesh: used.mesh,
                        joints: first,
                    });
                    posed.len() as u32 - 1
                });
            let skin = [pair, PENDING];
            if let Some(at) = used.instance {
                part.instances[at as usize].skin = skin;
            }
            if let Some(at) = used.caster {
                part.casters[at as usize].raw.skin = skin;
            }
        }
        first_instance += part.instances.len() as u32;
        plan.instances.push(part.instances);
        plan.casters.push(part.casters);
    }
    plan.skins = skins;
    plan
}

/// What a draw is sorted and batched by: draws with equal keys draw as one
/// batch. Small, so sorting thousands of them compares little.
///
/// The fields are in the order they sort by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SortKey {
    /// Drawn nearest first: what's hidden behind near items never reaches
    /// the (costly) surface shader. Equal distances batch together.
    band: u8,
    /// Within a band: items, then what's scattered over the terrain, then
    /// the terrain under them all, whose surface costs the most. It also
    /// keeps each kind's draws together, so the GPU's time can be taken by
    /// what is drawn with a stamp where the kind changes.
    part: DrawnPart,
    /// Which pipeline it is drawn with, ahead of its mesh and material
    /// ([`pipeline_rank`]); 0 while the old order is asked for.
    pipeline: u32,
    mesh: MeshKey,
    /// Its material's index among the frame's distinct material keys.
    material: u32,
    /// Its surface shader's key, or `None` for the standard material.
    shader: Option<u64>,
    double_sided: bool,
    casts_shadow: bool,
    /// Whether it has holes by its texture's alpha.
    cut_out: bool,
}

/// Where a draw's pipeline puts it among the draws of its distance band
/// and part: draws of one pipeline get one rank, so they are drawn
/// together, whatever their mesh. A change of pipeline between two small
/// draws costs the GPU more than the draws themselves, and a change of
/// mesh or material costs little: with plants of different pipelines
/// alternating mesh by mesh, a quarter to half a millisecond of Desert
/// Homestead's scene pass (`docs/rendering/performance.md`).
///
/// It changes which of two surfaces shows where both land on exactly the
/// same depth value (far foliage, where a step of the depth buffer is a
/// stud or more): single pixels, a few hundred to a few thousand of a
/// 1920 × 1080 frame of that scene against sorting by mesh first, which
/// the owner looked at and took (2026-11-26). `AXCENITY_PIPELINE_ORDER=0`
/// puts the old order back, to compare: every draw's rank is 0 then and
/// they sort by mesh first.
///
/// Two surface shaders can share a rank (it keeps 28 bits of their key),
/// which only lets their draws mix: the key's other fields still tell
/// them apart.
fn pipeline_rank(shader: Option<u64>, double_sided: bool, cut_out: bool, fading: bool) -> u32 {
    static BY_MESH: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *BY_MESH
        .get_or_init(|| std::env::var_os("AXCENITY_PIPELINE_ORDER").is_some_and(|v| v == "0"))
    {
        return 0;
    }
    let shader = shader.map_or(0, |key| (key as u32 & 0x0fff_ffff) + 1);
    shader << 3 | u32::from(fading) << 2 | u32::from(cut_out) << 1 | u32::from(double_sided)
}

/// Whether cut-outs other than hair cards cast their whole mesh in the
/// shadow maps, as they did until 2026-11-26: `AXCENITY_NO_CUT_OUT_SHADOWS`
/// asks for it, to measure what casting through their holes costs
/// (`docs/rendering/shadows.md`).
fn cut_outs_cast_whole() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var_os("AXCENITY_NO_CUT_OUT_SHADOWS").is_some())
}

/// How many of the sun's nearest cascades a cut-out other than a hair card
/// casts through its holes in, as a renderer starts: all of them, or
/// `AXCENITY_CUT_OUT_CASCADES` (0 to 4), to measure what the far ones cost
/// and show (`docs/rendering/shadows.md`). Past them it casts its whole mesh.
pub(super) fn cut_out_cascades() -> usize {
    std::env::var("AXCENITY_CUT_OUT_CASCADES")
        .ok()
        .and_then(|asked| asked.trim().parse::<usize>().ok())
        .map_or(SHADOW_CASCADES, |asked| asked.min(SHADOW_CASCADES))
}

/// What `item` is, for the order solid items are drawn in and the GPU's
/// time by what is drawn.
fn drawn_part(item: &DrawItem) -> DrawnPart {
    if item.material.terrain.is_some() {
        DrawnPart::Terrain
    } else if item.cull_distance > 0.0 {
        DrawnPart::Scattered
    } else {
        DrawnPart::Items
    }
}

/// Which distance band a point `from` the camera is in, the nearer the
/// smaller: bands widen by half with distance, so a forest's many layers of
/// trees are drawn near to far in a handful of batches of each mesh.
fn distance_band(point: Vec3, camera: Vec3) -> u8 {
    let distance = point.distance(camera);
    if distance.is_nan() {
        return u8::MAX;
    }
    (((distance / NEAREST_BAND).max(1.0).log2() * BANDS_PER_DOUBLING) as u8).min(u8::MAX - 1)
}

/// How far the nearest distance band reaches, in studs.
const NEAREST_BAND: f32 = 12.0;
/// How many distance bands each doubling of the distance is cut into.
const BANDS_PER_DOUBLING: f32 = 1.7;

/// The key of the surface shader `item` is drawn with, or `None` for the
/// standard material's. Terrain has its own shader.
fn shader_key(item: &DrawItem) -> Option<u64> {
    let material = &item.material;
    if material.terrain.is_some() || material.water.is_some() {
        return None;
    }
    material.surface_shader.as_ref().map(|shader| shader.key())
}

/// The textures an item is drawn with, in binding order (see
/// [`MATERIAL_TEXTURES`]). Terrain draws its own layers instead.
pub(super) fn material_textures(
    item: &DrawItem,
) -> [Option<&Arc<dyn TextureSource>>; MATERIAL_TEXTURES] {
    if item.material.terrain.is_some() {
        return [None; MATERIAL_TEXTURES];
    }
    let material = &item.material;
    let maps = &material.maps;
    // Skin's makeup and marking masks (ADR-0042) take the metalness,
    // emissive and opacity maps' places: skin with layers reads none of
    // them.
    if let Some(layers) = skin_layers(material) {
        let [first, second, third] = &layers.masks;
        return [
            material.base_color_texture.as_ref(),
            maps.normal.as_ref().map(|map| &map.texture),
            Some(first),
            maps.roughness.as_ref().map(|map| &map.texture),
            skin_second_colour(material)
                .map(|(map, _)| map)
                .or(maps.occlusion.as_ref().map(|map| &map.texture)),
            Some(second),
            Some(third),
            match &material.shading {
                Shading::Skin(skin) => skin.defined_normal.as_ref(),
                _ => None,
            },
        ];
    }
    // A hair card's atlas data takes the metalness map's place (hair isn't
    // metal) and its flow map the specular map's (hair.wgsl).
    if let Shading::Hair(hair) = &material.shading
        && material.water.is_none()
    {
        return [
            material.base_color_texture.as_ref(),
            maps.normal.as_ref().map(|map| &map.texture),
            hair.data.as_ref(),
            maps.roughness.as_ref().map(|map| &map.texture),
            maps.occlusion.as_ref().map(|map| &map.texture),
            maps.emissive.as_ref(),
            maps.opacity.as_ref().map(|map| &map.texture),
            hair.flow.as_ref(),
        ];
    }
    [
        material.base_color_texture.as_ref(),
        maps.normal.as_ref().map(|map| &map.texture),
        maps.metalness.as_ref().map(|map| &map.texture),
        maps.roughness.as_ref().map(|map| &map.texture),
        // A skin's second colour map takes the occlusion map's place.
        skin_second_colour(material)
            .map(|(map, _)| map)
            .or(maps.occlusion.as_ref().map(|map| &map.texture)),
        maps.emissive.as_ref(),
        maps.opacity.as_ref().map(|map| &map.texture),
        // A toon ramp takes the specular map's place, and so does skin's
        // defined normal map (skin reads no specular map).
        match &material.shading {
            Shading::Toon(toon) if material.water.is_none() => toon.ramp.as_ref(),
            Shading::Skin(skin) if material.water.is_none() => skin.defined_normal.as_ref(),
            _ => maps.specular.as_ref(),
        },
    ]
}

/// A skin's second colour map and how much of it shows (ADR-0048: a skin
/// between two photographed ages), when it is drawn: it rides in the
/// occlusion map's place, so not on a skin with an occlusion map, and not
/// at none of it.
fn skin_second_colour(material: &Material) -> Option<(&Arc<dyn TextureSource>, f32)> {
    match &material.shading {
        Shading::Skin(skin)
            if material.water.is_none()
                && material.terrain.is_none()
                && material.maps.occlusion.is_none() =>
        {
            skin.second_colour
                .as_ref()
                .map(|(map, amount)| (map, amount.clamp(0.0, 1.0)))
                .filter(|(_, amount)| *amount > 0.0)
        }
        _ => None,
    }
}

/// A skin material's makeup and markings, when it has them (water and
/// terrain never do).
fn skin_layers(material: &Material) -> Option<&SkinLayers> {
    match &material.shading {
        Shading::Skin(skin) if material.water.is_none() && material.terrain.is_none() => {
            skin.layers.as_deref()
        }
        _ => None,
    }
}

/// Where a skin's makeup and markings ride in its uniform (skin.wgsl's
/// `skin_layered`): the surface shader's values (skin has none), the toon
/// values and the specular map's last two (skin reads neither). Returns
/// (`params`, `toon`, the specular row's z and w).
///
/// - `params[0..6]`: each makeup layer's colour in linear light (red
///   below 0: the skin's own tone) and its opacity;
/// - `params[6]`, `params[7].xy`: each layer's roughness (below 0: the
///   skin's own);
/// - `params[7].zw`: the freckles' amount and opacity;
/// - `toon`: the moles' amount and opacity, the scar's opacity, the
///   freckles' seed;
/// - specular z 1 (the masks are bound), w the moles' seed.
fn skin_layer_uniform(layers: &SkinLayers) -> ([[f32; 4]; SURFACE_PARAMETERS], [f32; 4], [f32; 2]) {
    let mut params = [[0.0; 4]; SURFACE_PARAMETERS];
    let mut roughness = [-1.0; SKIN_MAKEUP_LAYERS];
    for (index, layer) in layers.makeup.iter().enumerate() {
        let opacity = layer.opacity.clamp(0.0, 1.0);
        let colour = match layer.colour {
            Some(colour) => linear_color(colour.map(|c| c.clamp(0.0, 1.0))),
            None => Vec3::new(-1.0, 0.0, 0.0),
        };
        params[index] = colour.extend(opacity).to_array();
        roughness[index] = layer.roughness.map_or(-1.0, |r| r.clamp(0.0, 1.0));
    }
    let share = |value: f32| value.clamp(0.0, 1.0);
    let seed = |value: f32| value.rem_euclid(1.0);
    params[6] = [roughness[0], roughness[1], roughness[2], roughness[3]];
    params[7] = [
        roughness[4],
        roughness[5],
        share(layers.freckles.amount),
        share(layers.freckles.opacity),
    ];
    let toon = [
        share(layers.moles.amount),
        share(layers.moles.opacity),
        share(layers.scar),
        seed(layers.freckles.seed),
    ];
    (params, toon, [1.0, seed(layers.moles.seed)])
}

/// Where a hair material's colour values ride in its uniform (hair.wgsl's
/// `hair_colour`), in the surface shader's place:
///
/// - `params[0]`: melanin, redness, tip lightening, the dye's amount;
/// - `params[1]`: the dye's colour (linear), the root's melanin;
/// - `params[2]`: the root's length, the streaks' amount and melanin;
/// - `params[3]`: the tint (linear), applied last.
fn hair_uniform(hair: &HairShading) -> [[f32; 4]; SURFACE_PARAMETERS] {
    let mut params = [[0.0; 4]; SURFACE_PARAMETERS];
    let share = |value: f32| value.clamp(0.0, 1.0);
    let offset = |value: f32| value.clamp(-1.0, 1.0);
    params[0] = [
        offset(hair.melanin),
        offset(hair.redness),
        share(hair.tip_lightening),
        share(hair.dye_amount),
    ];
    params[1] = linear_color(hair.dye.map(share))
        .extend(offset(hair.root_melanin))
        .to_array();
    params[2] = [
        share(hair.root_length),
        share(hair.streaks),
        offset(hair.streak_melanin),
        0.0,
    ];
    params[3] = linear_color(hair.tint.map(|c| c.max(0.0)))
        .extend(0.0)
        .to_array();
    params
}

/// [`MapsUniform::shading`] and [`MapsUniform::toon`] for `material`.
/// Water draws its own way, as standard.
fn shading_uniform(material: &Material) -> ([f32; 4], [f32; 4]) {
    if material.water.is_some() {
        return ([SHADING_STANDARD, 0.0, 0.0, 0.0], [0.0; 4]);
    }
    match &material.shading {
        Shading::Standard => (
            [
                SHADING_STANDARD,
                material.translucency.clamp(0.0, 1.0),
                // shader.wgsl's `glossy_leaf`.
                if material.glossy_leaf { 1.0 } else { 0.0 },
                0.0,
            ],
            [0.0; 4],
        ),
        Shading::Unlit => ([SHADING_UNLIT, 0.0, 0.0, 0.0], [0.0; 4]),
        // hair.wgsl: which maps are bound (1 the atlas data, 2 the flow)
        // and 4 for a shell (single-sided, or marked one: `hair_shell`), the cuticle tilt's
        // scale and the secondary's strength; the colour values ride in
        // `params` (`hair_uniform`).
        Shading::Hair(hair) => (
            [
                SHADING_HAIR,
                f32::from(
                    u8::from(hair.data.is_some())
                        + 2 * u8::from(hair.flow.is_some())
                        + 4 * u8::from(!material.double_sided || hair.shell),
                ),
                hair.shift.clamp(0.0, 2.0),
                hair.secondary.clamp(0.0, 2.0),
            ],
            [0.0; 4],
        ),
        // cloth.wgsl: the sheen's roughness, the anisotropy and its turn;
        // the sheen's colour (linear) in the toon row.
        Shading::Cloth(cloth) => (
            [
                SHADING_CLOTH,
                cloth.sheen_roughness.clamp(0.0, 1.0),
                cloth.anisotropy.clamp(-1.0, 1.0),
                cloth.anisotropy_rotation,
            ],
            linear_color(cloth.sheen_colour.map(|c| c.max(0.0)))
                .extend(0.0)
                .to_array(),
        ),
        // skin.wgsl: the defined normal map's share (none without one),
        // how far light spreads, and whether that map is BC5.
        Shading::Skin(skin) => {
            let defined = skin.defined_normal.as_ref();
            (
                [
                    SHADING_SKIN,
                    defined.map_or(0.0, |_| skin.normal_blend.clamp(0.0, 1.0)),
                    skin.scatter.clamp(0.0, 4.0),
                    f32::from(defined.is_some_and(|map| !map.bc5_levels().is_empty())),
                ],
                [0.0; 4],
            )
        }
        Shading::Toon(toon) => {
            let steps = if toon.ramp.is_some() {
                0.0
            } else {
                toon.steps.max(1) as f32
            };
            (
                [
                    SHADING_TOON,
                    steps,
                    toon.softness.clamp(0.0, 1.0),
                    toon.highlight.clamp(0.0, 1.0),
                ],
                [toon.rim.max(0.0), 0.0, 0.0, 0.0],
            )
        }
    }
}

/// [`MapsUniform::outline`] for `material`: none for water and terrain.
fn outline_uniform(material: &Material) -> [f32; 4] {
    match material.outline {
        Some(outline)
            if outline.width > 0.0 && material.water.is_none() && material.terrain.is_none() =>
        {
            linear_color(outline.color).extend(outline.width).to_array()
        }
        _ => [0.0; 4],
    }
}

/// The material binding an item is drawn with (see [`MaterialKey`]).
#[cfg(test)]
pub(super) fn material_key(item: &DrawItem) -> MaterialKey {
    material_key_with(item, &mut LastArrays::default())
}

/// [`material_key`] for one item after another: a terrain chunk's key
/// reuses what `last` kept of the chunk before it (`terrain::LastArrays`).
pub(super) fn material_key_with(item: &DrawItem, last: &mut LastArrays) -> MaterialKey {
    if let Some(surface) = &item.material.terrain {
        return MaterialKey {
            terrain: super::terrain::terrain_key_with(surface, last),
            ..MaterialKey::default()
        };
    }
    let textures = material_textures(item).map(|source| source.map(|source| source.key()));
    // Water's values ride in the surface shader's place: water has none.
    let mut params = item.material.surface_parameters;
    if let Some(water) = &item.material.water {
        params = [[0.0; 4]; SURFACE_PARAMETERS];
        params[..3].copy_from_slice(&super::water::water_params(water));
    }
    let (shading, mut toon) = shading_uniform(&item.material);
    let outline = outline_uniform(&item.material);
    // Hair's colour values ride in the surface shader's place.
    if let Shading::Hair(hair) = &item.material.shading
        && item.material.water.is_none()
        && item.material.terrain.is_none()
    {
        params = hair_uniform(hair);
    }
    // Skin's makeup and markings ride where skin reads nothing else.
    let layered = skin_layers(&item.material).map(skin_layer_uniform);
    let mut skin_specular = [0.0; 2];
    if let Some((layer_params, layer_toon, specular)) = layered {
        params = layer_params;
        toon = layer_toon;
        skin_specular = specular;
    }
    if textures.iter().all(Option::is_none)
        && params == [[0.0; 4]; SURFACE_PARAMETERS]
        && shading == [SHADING_STANDARD, 0.0, 0.0, 0.0]
        && outline[3] == 0.0
    {
        return MaterialKey::default();
    }
    let maps = &item.material.maps;
    let mask = |map: &Option<ChannelMap>| {
        let mut mask = [0.0; 4];
        mask[map.as_ref().map_or(0, |map| map.channel as usize)] = 1.0;
        mask
    };
    let inverted = |map: &Option<ChannelMap>| f32::from(map.as_ref().is_some_and(|map| map.invert));
    let used = |used: bool| f32::from(used);
    let uniform = MapsUniform {
        metalness: mask(&maps.metalness),
        roughness: mask(&maps.roughness),
        occlusion: mask(&maps.occlusion),
        opacity: mask(&maps.opacity),
        invert: [
            inverted(&maps.metalness),
            inverted(&maps.roughness),
            inverted(&maps.occlusion),
            inverted(&maps.opacity),
        ],
        settings: [
            maps.normal.as_ref().map_or(0.0, |map| map.scale),
            if maps.normal.as_ref().is_some_and(|map| map.flip_green) {
                -1.0
            } else {
                1.0
            },
            if maps.occlusion.is_some() {
                maps.occlusion_strength.clamp(0.0, 1.0)
            } else {
                0.0
            },
            // (Skin's layers take the opacity map's place.)
            used(maps.opacity.is_some() && layered.is_none()),
        ],
        specular: [
            used(maps.specular.is_some() && shading[0] == SHADING_STANDARD),
            // A BC5 normal map has no blue: the shader works it out.
            used(
                maps.normal
                    .as_ref()
                    .is_some_and(|map| !map.texture.bc5_levels().is_empty()),
            ),
            skin_specular[0],
            skin_specular[1],
        ],
        uv_matrix: uv_matrix(&item.material),
        uv_offset: [
            item.material.uv_offset[0],
            item.material.uv_offset[1],
            // How much of a skin's second colour map shows (skin.wgsl's
            // place for it: the occlusion map's).
            skin_second_colour(&item.material).map_or(0.0, |(_, amount)| amount),
            0.0,
        ],
        shading,
        toon,
        outline,
        params,
    };
    MaterialKey {
        textures,
        maps: bytemuck::cast(uniform),
        terrain: Default::default(),
    }
}

/// The [`MapsUniform`] a key was made from. The default key's is the plain
/// material's: no maps.
pub(super) fn maps_uniform_of(key: &MaterialKey) -> MapsUniform {
    if *key == MaterialKey::default() {
        let red = [1.0, 0.0, 0.0, 0.0];
        return MapsUniform {
            metalness: red,
            roughness: red,
            occlusion: red,
            opacity: red,
            invert: [0.0; 4],
            settings: [0.0, 1.0, 0.0, 0.0],
            specular: [0.0; 4],
            uv_matrix: [1.0, 0.0, 0.0, 1.0],
            uv_offset: [0.0; 4],
            shading: [SHADING_STANDARD, 0.0, 0.0, 0.0],
            toon: [0.0; 4],
            outline: [0.0; 4],
            params: [[0.0; 4]; SURFACE_PARAMETERS],
        };
    }
    bytemuck::cast(key.maps)
}

/// The columns of the 2×2 matrix that scales, then turns, a material's
/// texture coordinates. Turning by `r` takes (u, v) to
/// (u cos r + v sin r, v cos r - u sin r): counter-clockwise in the
/// texture, whose V runs down (glTF's `KHR_texture_transform`).
pub(super) fn uv_matrix(material: &Material) -> [f32; 4] {
    let [su, sv] = material.uv_scale;
    let (sin, cos) = material.uv_rotation.sin_cos();
    [cos * su, -sin * su, sin * sv, cos * sv]
}

/// The GPU data for one item, or `None` if it can't be seen.
#[cfg(test)]
pub(super) fn instance_raw(item: &DrawItem) -> Option<InstanceRaw> {
    instance_raw_with(item, &mut LinearColors::default())
}

/// The last base and glow colors turned to linear light, kept because
/// items drawn one after another mostly share them (a batch's) and the
/// conversion takes a power per channel.
#[derive(Default)]
pub(super) struct LinearColors {
    base: Option<([f32; 3], Vec3)>,
    emissive: Option<([f32; 3], Vec3)>,
}

impl LinearColors {
    fn convert(slot: &mut Option<([f32; 3], Vec3)>, color: [f32; 3]) -> Vec3 {
        match slot {
            Some((last, linear)) if *last == color => *linear,
            _ => {
                let linear = linear_color(color);
                *slot = Some((color, linear));
                linear
            }
        }
    }
}

/// [`instance_raw`], converting colors through `colors`.
pub(super) fn instance_raw_with(item: &DrawItem, colors: &mut LinearColors) -> Option<InstanceRaw> {
    // A shape squashed flat has no inverse, and can't be seen anyway.
    if is_flat(item) {
        return None;
    }
    let model3 = Mat3::from_mat4(item.transform);
    let material = &item.material;
    let base_color = LinearColors::convert(&mut colors.base, material.base_color);
    let emissive = LinearColors::convert(&mut colors.emissive, material.emissive);
    Some(InstanceRaw {
        model: item.transform.to_cols_array_2d(),
        normal_matrix: model3.inverse().transpose().to_cols_array_2d(),
        base_color_opacity: base_color.extend(material.opacity).to_array(),
        emissive_fade: emissive.extend(1.0).to_array(),
        surface: [
            material.metalness,
            material.roughness,
            material.alpha_cutoff,
            item.uv_tile,
        ],
        skin: [0, 0],
        sway: instance_sway(item),
    })
}

/// [`InstanceRaw::sway`] of `item`: how it sways, and its decal role.
pub(super) fn instance_sway(item: &DrawItem) -> [f32; 4] {
    let mut sway = match item.sway {
        Some(sway) if !sway.is_still() => {
            // The flutter rides in the fraction of the decal role.
            [
                sway.base,
                1.0 / sway.height,
                sway.strength,
                sway.flutter.clamp(0.0, 0.95),
            ]
        }
        _ => [0.0; 4],
    };
    // `ROLE_CHARACTER` and `ROLE_DECAL` in shader.wgsl.
    sway[3] += match item.decal_role {
        DecalRole::Surface => 0.0,
        DecalRole::Character => 1.0,
        DecalRole::Decal => 2.0,
    };
    sway
}

/// Whether `item` is squashed flat (size 0 on some axis): such a shape
/// has no instance ([`instance_raw_with`]) and can't be seen.
pub(super) fn is_flat(item: &DrawItem) -> bool {
    Mat3::from_mat4(item.transform).determinant().abs() < 1e-12
}

/// Converts an sRGB color (as people pick it) to linear light (what shading
/// math needs).
pub(super) fn linear_color([r, g, b]: [f32; 3]) -> Vec3 {
    fn channel(c: f32) -> f32 {
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    Vec3::new(channel(r), channel(g), channel(b))
}

#[cfg(test)]
mod sort_tests {
    use super::*;

    struct Keyed(u64);

    impl crate::MeshSource for Keyed {
        fn key(&self) -> u64 {
            self.0
        }
        fn positions(&self) -> &[[f32; 3]] {
            &[]
        }
        fn normals(&self) -> &[[f32; 3]] {
            &[]
        }
        fn uvs(&self) -> &[[f32; 2]] {
            &[]
        }
        fn indices(&self) -> &[u32] {
            &[]
        }
    }

    /// Keys with every part varying, by a fixed pseudo-random pattern.
    fn keys(count: u32, shader: bool) -> Vec<SortKey> {
        let parts = [
            DrawnPart::Items,
            DrawnPart::Scattered,
            DrawnPart::Terrain,
            DrawnPart::SeeThrough,
        ];
        let meshes = [
            MeshKey::Block,
            MeshKey::Sphere,
            MeshKey::Custom(7),
            MeshKey::Custom(u64::MAX),
            MeshKey::Custom(1 << 40),
        ];
        (0..count)
            .map(|i| {
                let r =
                    |k: u32| (i.wrapping_mul(2_654_435_761).wrapping_add(k * 40_503) >> 7) as usize;
                SortKey {
                    band: (r(1) % 5) as u8,
                    part: parts[r(2) % parts.len()],
                    pipeline: (r(3) % 4) as u32 * 0x1000_0001,
                    mesh: meshes[r(4) % meshes.len()],
                    material: (r(5) % 9) as u32,
                    shader: (shader && i == count / 2).then_some(99),
                    double_sided: r(6) % 2 == 0,
                    casts_shadow: r(7) % 2 == 0,
                    cut_out: r(8) % 2 == 0,
                }
            })
            .collect()
    }

    fn uses_with(meshes: &[Arc<dyn crate::MeshSource>]) -> FrameUses<'_> {
        FrameUses {
            materials: Vec::new(),
            material_of: Vec::new(),
            first_draw: Vec::new(),
            meshes: meshes.iter().collect(),
        }
    }

    #[test]
    fn packed_keys_sort_draws_as_their_keys_do() {
        let sources: Vec<Arc<dyn crate::MeshSource>> = [7, u64::MAX, 1 << 40]
            .into_iter()
            .map(|key| Arc::new(Keyed(key)) as Arc<dyn crate::MeshSource>)
            .collect();
        let check = |count: u32, shader: bool, uses: &FrameUses| {
            let keys = keys(count, shader);
            // Every other draw, out of order, as the split leaves them.
            let indices: Vec<u32> = (0..count).rev().step_by(2).collect();
            let mut expected: Vec<(SortKey, u32)> = indices
                .iter()
                .map(|&index| (keys[index as usize], index))
                .collect();
            expected.sort_unstable();
            let sorted = sorted_draws(&indices, |index| keys[index as usize], uses);
            assert_eq!(sorted, expected, "{count} draws, a shader {shader}");
        };
        // Enough to be shared among the threads, and a few; with a surface
        // shader the keys themselves are sorted.
        for count in [9, 40_000] {
            for shader in [false, true] {
                check(count, shader, &uses_with(&sources));
            }
        }
        // So are they with a mesh missing from the frame's list.
        check(200, false, &uses_with(&sources[..1]));
    }
}
