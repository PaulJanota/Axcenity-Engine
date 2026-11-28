//! Sun and spot light shadows: what the shadow quality allocates and draws,
//! and which spot lights get shadow layers.

use super::*;

#[test]
fn shadows_off_in_the_settings_draw_none_and_allocate_no_map() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut list = shadowed_wall();
    assert!(list.shadows.enabled, "the world asks for shadows");
    let mut renderer = TextureRenderer::new(&device, &queue, 64, 64);
    let shaded = middle_of(&mut renderer, &list);
    assert_eq!(renderer.scene.shadow_maps_alive(), 1);

    renderer.set_graphics(&with_shadows(ShadowQuality::Off));
    let off = middle_of(&mut renderer, &list);
    assert!(renderer.scene.shadow_map.is_none());
    assert_eq!(renderer.scene.shadow_maps_alive(), 0);

    list.items[1].casts_shadow = false;
    let lit = middle_of(&mut renderer, &list);
    assert!(lit[0] > shaded[0] + 40, "{shaded:?} vs {lit:?}");
    assert_eq!(off, lit, "no shadow is drawn");
}

#[test]
fn low_shadows_use_a_1024_map_with_two_cascades() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut list = shadowed_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 64, 64);
    let map = |renderer: &TextureRenderer| {
        let map = renderer.scene.shadow_map.as_ref().unwrap();
        (map.size, map.layers.len())
    };
    middle_of(&mut renderer, &list);
    let high = ShadowMapSize {
        size: 2048,
        cascades: 3,
    };
    assert_eq!(map(&renderer), (high, 3), "High is today's map");

    renderer.set_graphics(&with_shadows(ShadowQuality::Low));
    let shaded = middle_of(&mut renderer, &list);
    let low = ShadowMapSize {
        size: 1024,
        cascades: 2,
    };
    assert_eq!(map(&renderer), (low, 2));
    list.items[1].casts_shadow = false;
    let lit = middle_of(&mut renderer, &list);
    assert!(
        lit[0] > shaded[0] + 40,
        "Low still shadows: {shaded:?} vs {lit:?}"
    );
}

#[test]
fn a_shadow_distance_ceiling_below_the_worlds_cuts_shadows_there() {
    let world = Shadows {
        distance: 150.0,
        ..Shadows::default()
    };
    let ceiling = |distance| {
        GraphicsSettings {
            shadow_distance: Some(distance),
            ..GraphicsSettings::default()
        }
        .effective(&Capabilities::default())
    };
    let drawn = |graphics| shadows_drawn(world, &graphics).map(|(shadows, _)| shadows.distance);
    assert_eq!(drawn(ceiling(5.0)), Some(5.0));
    assert_eq!(
        drawn(ceiling(500.0)),
        Some(150.0),
        "the ceiling never raises it"
    );
    assert_eq!(drawn(ceiling(0.0)), None);
    let world_off = Shadows {
        enabled: false,
        ..world
    };
    assert!(shadows_drawn(world_off, &ceiling(500.0)).is_none());

    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut list = shadowed_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 64, 64);
    let shaded = middle_of(&mut renderer, &list);
    renderer.set_graphics(&GraphicsSettings {
        shadow_distance: Some(5.0),
        ..GraphicsSettings::default()
    });
    let cut = middle_of(&mut renderer, &list);
    list.items[1].casts_shadow = false;
    let lit = middle_of(&mut renderer, &list);
    assert!(lit[0] > shaded[0] + 40, "{shaded:?} vs {lit:?}");
    assert_eq!(cut, lit, "the wall is past the ceiling, so unshadowed");
}

#[test]
fn switching_shadow_quality_fifty_times_leaves_one_shadow_map() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let list = shadowed_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 32, 32);
    let qualities = [
        ShadowQuality::Low,
        ShadowQuality::Medium,
        ShadowQuality::High,
        ShadowQuality::Off,
        ShadowQuality::Ultra,
    ];
    for quality in qualities.iter().cycle().take(50) {
        renderer.set_graphics(&with_shadows(*quality));
        renderer.render(&list).unwrap();
        let expected = usize::from(*quality != ShadowQuality::Off);
        assert_eq!(renderer.scene.shadow_maps_alive(), expected, "{quality:?}");
        let size = renderer.scene.shadow_map.as_ref().map(|map| map.size);
        assert_eq!(size, quality.map());
    }
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    assert_eq!(renderer.scene.shadow_maps_alive(), 1);
}

/// A wall facing the camera, lit only by a spot light to the right that
/// shines on its middle, with a small block halfway between them.
fn spot_lit_wall() -> DrawList {
    let block = |size: Vec3, position: Vec3| DrawItem {
        transform: Mat4::from_scale_rotation_translation(size, glam::Quat::IDENTITY, position),
        ..big_block()
    };
    let wall = block(Vec3::new(30.0, 30.0, 1.0), Vec3::new(0.0, 0.0, -10.0));
    let blocker = block(Vec3::splat(2.0), Vec3::new(3.0, 0.0, -5.5));
    let mut list = list_of(vec![wall, blocker]);
    list.sun.intensity = 0.0;
    let light = Vec3::new(6.0, 0.0, -1.5);
    let wall_middle = Vec3::new(0.0, 0.0, -9.5);
    list.lights
        .push(spot_light(light, wall_middle - light, 30.0, true));
    list
}

#[test]
fn a_part_in_a_shadow_casting_spot_lights_beam_shadows_the_wall_and_moves_it() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut list = spot_lit_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 64, 64);
    let middle = Vec3::new(0.0, 0.0, -9.5);
    // Where the shadow falls once the block is moved up 2 studs: twice as
    // far from the light, so 4 studs up the wall.
    let higher = Vec3::new(0.0, 4.0, -9.5);

    let shaded = pixel_at(&mut renderer, &list, middle);
    let higher_before = pixel_at(&mut renderer, &list, higher);
    list.items[1].casts_shadow = false;
    let lit = pixel_at(&mut renderer, &list, middle);
    assert!(
        lit[0] > shaded[0] + 40,
        "the block shadows the wall: {shaded:?} vs {lit:?}"
    );

    // Moved, the shadow moves with it.
    list.items[1].casts_shadow = true;
    list.items[1].transform.w_axis.y = 2.0;
    let middle_after = pixel_at(&mut renderer, &list, middle);
    let higher_after = pixel_at(&mut renderer, &list, higher);
    assert!(
        middle_after[0] > shaded[0] + 40,
        "the middle is lit again: {shaded:?} vs {middle_after:?}"
    );
    assert!(
        higher_before[0] > higher_after[0] + 40,
        "the shadow is higher up: {higher_before:?} vs {higher_after:?}"
    );

    // With `Shadows` false there's none, and no spot shadow map.
    list.items[1].transform.w_axis.y = 0.0;
    list.lights[0].cone.as_mut().unwrap().shadows = None;
    let unshadowed = pixel_at(&mut renderer, &list, middle);
    assert!(
        unshadowed[0].abs_diff(lit[0]) <= 2,
        "{unshadowed:?} vs {lit:?}"
    );
    assert!(renderer.scene.spot_shadow_map.is_none());
    assert_eq!(renderer.scene.spot_shadow_maps_alive(), 0);
}

#[test]
fn only_the_nearest_shadow_casting_spots_get_shadow_layers() {
    let mut list = list_of(vec![]);
    let spot = |z: f32, shadows| spot_light(Vec3::new(0.0, 0.0, z), Vec3::NEG_Z, 5.0, shadows);
    // Farthest first, so the order they're listed in doesn't decide.
    list.lights = vec![
        spot(-15.0, true),
        spot(-10.0, true),
        spot(-5.0, true),
        spot(-3.0, false),
    ];
    let planned = plan_lights(&list, 1.0, (64, 64), ShadowQuality::Medium);
    let layers: Vec<(f32, f32)> = planned
        .lights
        .iter()
        .map(|light| (light.position_range[2], light.shadow[0]))
        .collect();
    // Every light still lights; only the two nearest that cast shadows do.
    assert_eq!(
        layers,
        [(-3.0, -1.0), (-5.0, 0.0), (-10.0, 1.0), (-15.0, -1.0)]
    );
    assert_eq!(planned.spot_shadows.len(), 2);
    assert!(planned.lights[1].shadow[1] > 0.0, "texel width per stud");

    let none = plan_lights(&list, 1.0, (64, 64), ShadowQuality::Off);
    assert!(none.spot_shadows.is_empty());
    assert!(none.lights.iter().all(|light| light.shadow[0] == -1.0));
}

#[test]
fn a_spot_light_past_the_settings_limit_lights_without_a_shadow() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut list = spot_lit_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 64, 64);
    let middle = Vec3::new(0.0, 0.0, -9.5);
    let shaded = pixel_at(&mut renderer, &list, middle);
    // Two shadow-casting spots nearer the camera, pointing away from the
    // wall, take Medium's two layers.
    for x in [-1.0, 1.0] {
        list.lights.push(spot_light(
            Vec3::new(x, 0.0, -1.0),
            Vec3::new(x, 0.0, 0.0),
            3.0,
            true,
        ));
    }
    renderer.set_graphics(&with_shadows(ShadowQuality::Medium));
    let unshadowed = pixel_at(&mut renderer, &list, middle);
    assert!(
        unshadowed[0] > shaded[0] + 40,
        "still lit, without a shadow: {shaded:?} vs {unshadowed:?}"
    );
    // High has four layers, so the wall's light casts its shadow again.
    renderer.set_graphics(&with_shadows(ShadowQuality::High));
    let again = pixel_at(&mut renderer, &list, middle);
    assert!(again[0].abs_diff(shaded[0]) <= 2, "{shaded:?} vs {again:?}");
}

#[test]
fn low_shadows_allocate_no_spot_shadow_map() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let list = spot_lit_wall();
    let mut renderer = TextureRenderer::new(&device, &queue, 32, 32);
    let spot_map = |renderer: &TextureRenderer| {
        let map = renderer.scene.spot_shadow_map.as_ref()?;
        Some((map.size, map.layers.len()))
    };
    renderer.render(&list).unwrap();
    let high = crate::SpotShadowMapSize {
        size: 1024,
        lights: 4,
    };
    assert_eq!(spot_map(&renderer), Some((high, 4)));

    renderer.set_graphics(&GraphicsSettings::preset(crate::Quality::Low));
    renderer.render(&list).unwrap();
    assert_eq!(spot_map(&renderer), None);
    assert_eq!(renderer.scene.spot_shadow_maps_alive(), 0);

    for quality in ShadowQuality::ALL.iter().cycle().take(20) {
        renderer.set_graphics(&with_shadows(*quality));
        renderer.render(&list).unwrap();
        let expected = quality.spot_map();
        assert_eq!(spot_map(&renderer).map(|(size, _)| size), expected);
        assert_eq!(
            renderer.scene.spot_shadow_maps_alive(),
            usize::from(expected.is_some())
        );
    }
}

#[test]
fn a_cones_bounds_hold_its_whole_beam() {
    for angle in [10.0, 60.0, 90.0, 120.0, 179.0] {
        let (apex, direction, range) = (Vec3::new(1.0, 2.0, 3.0), Vec3::NEG_Y, 10.0);
        let (center, radius) = lighting::cone_bounds(apex, direction, angle, range);
        let half = (angle * 0.5_f32).to_radians();
        let inside = |point: Vec3| point.distance(center) <= radius + 1e-3;
        assert!(inside(apex), "{angle}: apex");
        assert!(inside(apex + direction * range), "{angle}: tip");
        for step in 0..8 {
            let turn = step as f32 * std::f32::consts::FRAC_PI_4;
            let across = Vec3::new(turn.cos(), 0.0, turn.sin());
            let rim = apex + (direction * half.cos() + across * half.sin()) * range;
            assert!(inside(rim), "{angle}: rim");
        }
        assert!(
            radius <= range * 1.0001,
            "{angle}: no bigger than its reach"
        );
    }
}

#[test]
fn spot_shadows_skip_casters_outside_the_beams_bounds() {
    let near = item(Shape::Block, Vec3::new(0.0, 0.0, -3.0), 1.0);
    let far = item(Shape::Block, Vec3::new(0.0, 0.0, -40.0), 1.0);
    let glass = item(Shape::Block, Vec3::new(0.0, 0.0, -2.0), 0.5);
    let mut list = list_of(vec![near, far, glass]);
    list.items.push(DrawItem {
        casts_shadow: false,
        ..item(Shape::Sphere, Vec3::new(0.0, 0.0, -4.0), 1.0)
    });
    let DrawPlan { casters, .. } = plan(&list);
    let beam = (Vec3::new(0.0, 0.0, -5.0), 4.0);
    // A view that sees everywhere, so only the lamp's reach decides.
    let everywhere = Frustum::of(Mat4::ZERO);
    let views = lighting::casters_in_views(
        &casters,
        &[ShadowView::Lamp(beam, everywhere)],
        0,
        false,
        None,
    );
    // Only the near solid block: the far one is out of reach, the glass is
    // see-through and the sphere casts no shadow.
    assert_eq!(views[0].groups, [((MeshKey::Block, None), 0..1)]);
    assert_eq!(views[0].left_out, 1);
    let instances: Vec<_> = views[0].instances.iter().flatten().collect();
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].model[3][2], -3.0);
}

#[test]
fn a_shadow_view_draws_each_mesh_once_whatever_distance_its_casters_are_at() {
    // Blocks and spheres near the camera and far from it: the scene sorts
    // them by distance band first, so its casters alternate by mesh.
    let at = |shape, z| item(shape, Vec3::new(0.0, 0.0, z), 1.0);
    let mut list = list_of(vec![
        at(Shape::Block, -3.0),
        at(Shape::Sphere, -4.0),
        at(Shape::Block, -40.0),
        at(Shape::Sphere, -45.0),
        at(Shape::Block, -200.0),
    ]);
    // A cut-out, drawn through its holes in the near cascades only.
    list.items.push(DrawItem {
        material: Material {
            alpha_cutoff: 0.5,
            ..Material::default()
        },
        ..at(Shape::Block, -5.0)
    });
    let DrawPlan { casters, .. } = plan(&list);
    let order: Vec<MeshKey> = casters.iter().flatten().map(|caster| caster.mesh).collect();
    assert_ne!(
        order[..2],
        [MeshKey::Block, MeshKey::Block],
        "the scene's order"
    );
    let everywhere = Frustum::of(Mat4::ZERO);
    let sun = ShadowView::Sun(everywhere);
    let views = lighting::casters_in_views(&casters, &[sun, sun], 1, false, None);
    let material = casters
        .iter()
        .flatten()
        .find_map(|caster| caster.cut_out)
        .expect("a cut-out caster")
        .material;
    // Whole casters first, a draw per mesh, then the cut-out.
    let groups: Vec<_> = views[0]
        .groups
        .iter()
        .map(|(group, range)| (*group, range.len()))
        .collect();
    assert_eq!(
        groups,
        [
            ((MeshKey::Block, None), 3),
            ((MeshKey::Sphere, None), 2),
            ((MeshKey::Block, Some(material)), 1),
        ]
    );
    // Past the near cascades the cut-out casts whole, with the other blocks.
    let groups: Vec<_> = views[1]
        .groups
        .iter()
        .map(|(group, range)| (*group, range.len()))
        .collect();
    assert_eq!(
        groups,
        [((MeshKey::Block, None), 4), ((MeshKey::Sphere, None), 2)]
    );
    // Each group's instances are its own casters.
    let instances: Vec<f32> = views[1]
        .instances
        .iter()
        .flatten()
        .map(|raw| raw.model[3][2])
        .collect();
    assert_eq!(instances[4..], [-4.0, -45.0]);
}

#[test]
fn the_farthest_cascade_draws_casters_at_the_level_their_size_in_its_map_calls_for() {
    use super::level_of_detail::Detailed;
    // Two copies of a mesh with two simpler levels, both near the camera,
    // so the camera draws them at full detail.
    let near = |x: f32| DrawItem {
        shape: Shape::Mesh(Arc::new(Detailed { level: 0 })),
        ..item(Shape::Block, Vec3::new(x, 0.0, -3.0), 1.0)
    };
    let list = list_of(vec![near(-1.0), near(1.0)]);
    let DrawPlan { casters, .. } = plan(&list);
    let full = MeshKey::Custom(900);
    assert!(casters.iter().flatten().all(|caster| caster.mesh == full));
    let radius = casters[0][0].sphere.1;
    let sun = ShadowView::Sun(Frustum::of(Mat4::ZERO));
    let meshes_in = |extent: f32, uploaded: &(dyn Fn(MeshKey) -> bool + Sync)| {
        let lods = lighting::FarCascadeLods {
            view: 1,
            extent,
            uploaded,
        };
        let views = lighting::casters_in_views(&casters, &[sun, sun], 0, false, Some(lods));
        let near: Vec<MeshKey> = views[0].groups.iter().map(|((mesh, _), _)| *mesh).collect();
        let far: Vec<MeshKey> = views[1].groups.iter().map(|((mesh, _), _)| *mesh).collect();
        (near, far)
    };
    let everything: &(dyn Fn(MeshKey) -> bool + Sync) = &|_| true;
    // Half the map across: full detail, as the camera draws it.
    let (near, far) = meshes_in(4.0 * radius, everything);
    assert_eq!((near, far), (vec![full], vec![full]));
    // A fifth of it: the first simpler copy, in the farthest cascade only.
    let (near, far) = meshes_in(10.0 * radius, everything);
    assert_eq!((near, far), (vec![full], vec![MeshKey::Custom(901)]));
    // A twentieth: the simplest.
    let (_, far) = meshes_in(40.0 * radius, everything);
    assert_eq!(far, vec![MeshKey::Custom(902)]);
    // A copy not on the GPU yet isn't drawn: the next one that is, or the
    // camera's.
    let (_, far) = meshes_in(40.0 * radius, &|mesh| mesh != MeshKey::Custom(902));
    assert_eq!(far, vec![MeshKey::Custom(901)]);
    let (_, far) = meshes_in(40.0 * radius, &|mesh| mesh == full);
    assert_eq!(far, vec![full]);
    assert_eq!(lighting::shadow_level(0.5), 0);
    assert_eq!(lighting::shadow_level(0.2), 1);
    assert_eq!(lighting::shadow_level(0.05), 2);
}

/// A tree's piece with two simpler copies (the detailed square's, keys 901
/// and 902) and a stand-in (`MeshSource::stand_in`): its own (key 960), or
/// covered by that one.
struct StandingIn {
    key: u64,
    covered: bool,
}

const SQUARE_CORNERS: [[f32; 3]; 4] = [
    [-0.5, -0.5, 0.5],
    [0.5, -0.5, 0.5],
    [0.5, 0.5, 0.5],
    [-0.5, 0.5, 0.5],
];

impl MeshSource for StandingIn {
    fn key(&self) -> u64 {
        self.key
    }
    fn positions(&self) -> &[[f32; 3]] {
        &SQUARE_CORNERS
    }
    fn normals(&self) -> &[[f32; 3]] {
        &[[0.0, 0.0, 1.0]; 4]
    }
    fn uvs(&self) -> &[[f32; 2]] {
        &[]
    }
    fn indices(&self) -> &[u32] {
        &[0, 1, 2, 0, 2, 3]
    }
    fn lod_count(&self) -> usize {
        if self.key == 960 { 0 } else { 2 }
    }
    fn lod(&self, level: usize) -> Option<&Arc<dyn MeshSource>> {
        use super::level_of_detail::Detailed;
        static COPIES: std::sync::OnceLock<[Arc<dyn MeshSource>; 2]> = std::sync::OnceLock::new();
        let copies = COPIES.get_or_init(|| {
            [1, 2].map(|level| Arc::new(Detailed { level }) as Arc<dyn MeshSource>)
        });
        copies
            .get(level.checked_sub(1)?)
            .filter(|_| self.key != 960)
    }
    fn stand_in(&self) -> crate::StandIn<'_> {
        static STAND_IN: std::sync::OnceLock<Arc<dyn MeshSource>> = std::sync::OnceLock::new();
        if self.key == 960 {
            return crate::StandIn::Own;
        }
        let stand_in = STAND_IN.get_or_init(|| {
            Arc::new(StandingIn {
                key: 960,
                covered: false,
            })
        });
        if self.covered {
            crate::StandIn::Covered(stand_in)
        } else {
            crate::StandIn::Mesh(stand_in)
        }
    }
}

#[test]
fn the_farthest_cascade_casts_a_far_items_stand_in_and_leaves_out_what_it_covers() {
    // A tree's two pieces near the camera, so it draws them at full detail:
    // one with a stand-in, one the stand-in covers. The first is a cut-out.
    let piece = |covered: bool, x: f32| DrawItem {
        shape: Shape::Mesh(Arc::new(StandingIn {
            key: if covered { 951 } else { 950 },
            covered,
        })),
        material: Material {
            alpha_cutoff: if covered { 0.0 } else { 0.5 },
            ..Material::default()
        },
        ..item(Shape::Block, Vec3::new(x, 0.0, -3.0), 1.0)
    };
    let list = list_of(vec![piece(false, -1.0), piece(true, 1.0)]);
    let DrawPlan { casters, .. } = plan(&list);
    let radius = casters[0][0].sphere.1;
    let sun = ShadowView::Sun(Frustum::of(Mat4::ZERO));
    let groups_in = |extent: f32, uploaded: &(dyn Fn(MeshKey) -> bool + Sync)| {
        let lods = lighting::FarCascadeLods {
            view: 1,
            extent,
            uploaded,
        };
        let views = lighting::casters_in_views(&casters, &[sun, sun], 1, false, Some(lods));
        let groups = |at: usize| -> Vec<(MeshKey, bool)> {
            views[at]
                .groups
                .iter()
                .map(|((mesh, material), _)| (*mesh, material.is_some()))
                .collect()
        };
        (groups(0), groups(1))
    };
    let everything: &(dyn Fn(MeshKey) -> bool + Sync) = &|_| true;
    // The nearer cascade draws both as the camera does, the cut-out
    // through its holes.
    let (near, far) = groups_in(10.0 * radius, everything);
    let (own, covered) = (MeshKey::Custom(950), MeshKey::Custom(951));
    let mut sorted = near.clone();
    sorted.sort();
    assert_eq!(sorted, [(own, true), (covered, false)]);
    // At the first simpler level the stand-in isn't used yet: past the
    // near cascades the cut-out casts whole, with the other piece.
    assert_eq!(far, [(MeshKey::Custom(901), false)]);
    // From `STAND_IN_LEVEL`, the stand-in casts in the first piece's place,
    // through its holes, and the covered piece is left out.
    assert_eq!(lighting::shadow_level(2.0 / 40.0), crate::STAND_IN_LEVEL);
    let (_, far) = groups_in(40.0 * radius, everything);
    assert_eq!(far, [(MeshKey::Custom(960), true)]);
    // Until the stand-in is on the GPU, both cast their simplest copy.
    let (_, far) = groups_in(40.0 * radius, &|mesh| mesh != MeshKey::Custom(960));
    assert_eq!(far, [(MeshKey::Custom(902), false)]);
}
