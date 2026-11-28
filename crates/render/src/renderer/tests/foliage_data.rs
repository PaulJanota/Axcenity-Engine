//! A plant's packed data per corner (foliage W3b, ADR-0053,
//! `MeshSource::foliage`): each corner's share of the wind's lean bends it
//! in place of its height, in the scene, the shadow maps and the ray traced
//! scene alike, and its baked occlusion and shade mask shade it.

use std::sync::Arc;

use glam::Vec3;

use super::wind::{
    SIZE, mean_column, on_screen, pixel_on, pixels_per_stud, plant_and_its_shadow, plant_column,
    plant_seen_side_on,
};
use super::*;
use crate::GraphicsSettings;
use crate::graphics::{AmbientOcclusion, AntiAliasing, Quality};

/// Open, no flutter or mask, and leaning by `sway` (0 to 1).
fn leaning(sway: f32) -> [f32; 4] {
    [1.0, sway, 0.0, 0.0]
}

/// The wind tests' plant (a pole 8 studs tall, item 0 of `list`, or
/// `item`), as a box whose every corner carries `foliage`.
fn painted(list: &mut DrawList, item: usize, foliage: [f32; 4]) {
    list.items[item].shape = Shape::Mesh(Arc::new(PaintedFoliage::every_corner(
        Arc::new(UnitBox::new()),
        foliage,
    )));
}

#[test]
fn a_plants_own_sway_weights_bend_it_in_place_of_its_height() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut renderer = TextureRenderer::new(&device, &queue, SIZE, SIZE);
    let gale = Vec3::new(40.0, 0.0, 0.0);
    // How far `from`..`to` studs up the plant moves in the gale, in pixels.
    let mut shift = |foliage: Option<[f32; 4]>, from: f32, to: f32| {
        let mut columns = [Vec3::ZERO, gale].map(|wind| {
            let mut list = plant_seen_side_on(true, wind);
            if let Some(foliage) = foliage {
                painted(&mut list, 0, foliage);
            }
            plant_column(&mut renderer, &list, from, to)
        });
        columns.reverse();
        columns[0] - columns[1]
    };
    // As the box it's drawn as: the top leans, the foot stays put.
    let plain_top = shift(None, 6.0, 7.8);
    let plain_foot = shift(None, 0.2, 1.0);
    assert!(plain_top > 2.0, "the top leaned {plain_top} pixels");
    assert!(plain_foot.abs() < 0.5, "the foot moved {plain_foot}");
    // Weights of 0 hold it still in a gale, top and all.
    let stiff = shift(Some(leaning(0.0)), 6.0, 7.8);
    assert!(
        stiff.abs() < 0.3,
        "a stiff plant's top moved {stiff} pixels"
    );
    // Weights of 1 move it as far as the top would go, foot and all.
    let loose_top = shift(Some(leaning(1.0)), 6.0, 7.8);
    let loose_foot = shift(Some(leaning(1.0)), 0.2, 1.0);
    assert!(
        loose_foot > 0.7 * plain_top && (loose_top - loose_foot).abs() < 0.3 * loose_top,
        "a loose plant moved {loose_foot} at its foot, {loose_top} at its top \
         (the plain top {plain_top})"
    );
    // Half the weight leans it about half as far.
    let half = shift(Some(leaning(0.5)), 6.0, 7.8);
    assert!(
        (half - 0.5 * loose_top).abs() < 0.2 * loose_top,
        "half the weight moved it {half}, all of it {loose_top}"
    );
}

/// How far the end of the plant's shadow moves in a gale, in studs, drawn
/// by `renderer` with `foliage` on the plant.
fn shadow_shift(renderer: &mut TextureRenderer, foliage: Option<[f32; 4]>) -> f32 {
    let gale = Vec3::new(40.0, 0.0, 0.0);
    let mut column = |wind| {
        let mut list = plant_and_its_shadow(true, wind);
        if let Some(foliage) = foliage {
            painted(&mut list, 1, foliage);
        }
        // The floor in the sun, well clear of the shadow.
        let lit = pixel_on(renderer, &list, Vec3::new(8.0, 0.0, -7.0))[0];
        let rows = (
            on_screen(&list, Vec3::new(0.0, 0.0, -7.8)).y,
            on_screen(&list, Vec3::new(0.0, 0.0, -6.0)).y,
        );
        let column = mean_column(&renderer.read_pixels(), rows, |[r, ..]| {
            f32::from(lit.saturating_sub(r))
        });
        (list, column.expect("the shadow is drawn"))
    };
    let (list, still) = column(Vec3::ZERO);
    let (_, swayed) = column(gale);
    (swayed - still) / pixels_per_stud(&list, Vec3::new(0.0, 0.0, -7.0))
}

#[test]
fn a_plants_shadow_bends_by_its_own_weights() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let mut renderer = TextureRenderer::new(&device, &queue, SIZE, SIZE);
    let loose = shadow_shift(&mut renderer, Some(leaning(1.0)));
    let stiff = shadow_shift(&mut renderer, Some(leaning(0.0)));
    assert!(loose > 0.5, "a loose plant's shadow moved {loose} studs");
    assert!(
        stiff.abs() < 0.1,
        "a stiff plant's shadow moved {stiff} studs"
    );
}

#[test]
fn a_plants_ray_traced_shadow_bends_by_its_own_weights() {
    let Some((device, queue)) = ray_test_device() else {
        eprintln!("skipped: no graphics adapter with ray queries on this machine");
        return;
    };
    let mut renderer = TextureRenderer::new(&device, &queue, SIZE, SIZE);
    renderer.set_graphics(&GraphicsSettings {
        shadow_quality: Some(crate::ShadowQuality::Off),
        ray_traced_shadows: Some(crate::RayTracedShadows::Low),
        ..GraphicsSettings::preset(Quality::Ultra)
    });
    let loose = shadow_shift(&mut renderer, Some(leaning(1.0)));
    let stiff = shadow_shift(&mut renderer, Some(leaning(0.0)));
    assert!(
        loose > 0.5,
        "a loose plant's traced shadow moved {loose} studs"
    );
    assert!(
        stiff.abs() < 0.1,
        "a stiff plant's traced shadow moved {stiff} studs"
    );
}

/// A square 4 studs across, 10 studs in front of the camera, facing it,
/// under an overcast sky with no sun, in `material`, with `foliage` on
/// every corner (or none): its middle pixel's linear colour.
fn square_lit(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    material: Material,
    foliage: Option<[f32; 4]>,
) -> Vec3 {
    let mesh: Arc<dyn MeshSource> = match foliage {
        Some(foliage) => Arc::new(PaintedFoliage::every_corner(Arc::new(Square), foliage)),
        None => Arc::new(Square),
    };
    let middle = Vec3::new(0.0, 0.0, -10.0);
    let mut list = list_of(vec![DrawItem {
        transform: Mat4::from_scale_rotation_translation(
            Vec3::splat(4.0),
            glam::Quat::IDENTITY,
            middle,
        ),
        material,
        casts_shadow: false,
        ..item(Shape::Mesh(mesh), Vec3::ZERO, 1.0)
    }]);
    list.sky = Sky {
        top_color: [0.78, 0.8, 0.8],
        horizon_color: [0.72, 0.74, 0.74],
        ground_color: [0.12, 0.13, 0.1],
        environment_intensity: 1.0,
        ..Sky::default()
    };
    list.sun.intensity = 0.0;
    list.shadows.enabled = false;
    list.camera = Camera {
        position: Vec3::ZERO,
        look_at: middle,
        fov_y_degrees: 40.0,
    };
    let mut renderer = TextureRenderer::new(device, queue, SIZE, SIZE);
    renderer.set_graphics(&GraphicsSettings {
        anti_aliasing: Some(AntiAliasing::Off),
        ambient_occlusion: Some(AmbientOcclusion::Off),
        contact_shadows: Some(false),
        ..GraphicsSettings::preset(Quality::High)
    });
    renderer.render(&list).unwrap();
    let pixels = renderer.read_pixels();
    let at = (SIZE as usize / 2) * (SIZE as usize + 1) * 4;
    // The frame is sRGB.
    let linear = |byte: u8| (f32::from(byte) / 255.0).powf(2.2);
    Vec3::new(
        linear(pixels[at]),
        linear(pixels[at + 1]),
        linear(pixels[at + 2]),
    )
}

#[test]
fn a_plants_baked_occlusion_darkens_it_and_its_mask_weathers_bark_and_tints_leaves() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let bark = Material {
        base_color: [0.3, 0.2, 0.12],
        roughness: 0.9,
        ..Material::default()
    };
    let lit = |material: &Material, foliage| square_lit(&device, &queue, material.clone(), foliage);
    // Without data, and with data that's open and unmasked, bark is lit alike.
    let plain = lit(&bark, None);
    let open = lit(&bark, Some([1.0, 0.0, 0.0, 0.0]));
    assert!(
        (plain - open).abs().max_element() < 0.01,
        "open bark {open} against plain {plain}"
    );
    // Deep in a crown, or in a crotch, it's darker.
    let deep = lit(&bark, Some([0.3, 0.0, 0.0, 0.0]));
    assert!(
        deep.y < 0.6 * open.y,
        "bark deep in the crown {deep} against open {open}"
    );
    // Weathered, it's greyer and paler.
    let weathered = lit(&bark, Some([1.0, 0.0, 0.0, 1.0]));
    let warmth = |c: Vec3| (c.x - c.z) / c.y.max(1e-4);
    assert!(
        warmth(weathered) < 0.6 * warmth(open) && weathered.y > open.y,
        "weathered bark {weathered} against fresh {open}"
    );

    let leaf = Material {
        base_color: [0.25, 0.45, 0.15],
        roughness: 0.8,
        double_sided: true,
        translucency: 0.42,
        ..Material::default()
    };
    let open = lit(&leaf, Some([1.0, 0.0, 0.0, 0.5]));
    let deep = lit(&leaf, Some([0.3, 0.0, 0.0, 0.5]));
    assert!(
        deep.y < 0.7 * open.y,
        "a leaf deep in the crown {deep} against open {open}"
    );
    // Its cluster's shade moves its tone, either way.
    let light = lit(&leaf, Some([1.0, 0.0, 0.0, 1.0]));
    let dark = lit(&leaf, Some([1.0, 0.0, 0.0, 0.0]));
    assert!(
        light.y > dark.y * 1.1,
        "a leaf's shade mask moved it from {dark} to {light}"
    );
}
