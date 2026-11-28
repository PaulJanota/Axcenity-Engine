//! Occlusion culling of scattered items (`renderer/occlusion_cull.rs`):
//! the picture must be the one drawn without it, pixel for pixel, where
//! nothing ties on depth, and items wholly behind the rest must be left
//! out.

use glam::{Mat4, Vec3};

use super::card_set::{card, striped};
use super::*;
use crate::GraphicsSettings;
use crate::graphics::{AmbientOcclusion, AntiAliasing, MotionBlurQuality, Quality};

const SIZE: u32 = 160;

fn settings(anti_aliasing: AntiAliasing) -> GraphicsSettings {
    GraphicsSettings {
        anti_aliasing: Some(anti_aliasing),
        motion_blur: Some(MotionBlurQuality::Off),
        ambient_occlusion: Some(AmbientOcclusion::High),
        volumetric_fog: Some(false),
        ..GraphicsSettings::preset(Quality::High)
    }
}

/// A wall over the left of the view and a hedge of leaf cards with holes
/// over the right, near, and a field of scattered balls and leaf cards
/// far behind them both and off to the side: some hidden by the wall,
/// some seen through the hedge's holes, some in the open.
fn field() -> DrawList {
    let mut wall = item(Shape::Block, Vec3::new(-7.0, 0.0, -18.0), 1.0);
    wall.transform *= Mat4::from_scale(Vec3::new(16.0, 30.0, 1.0));
    wall.material.base_color = [0.6, 0.6, 0.65];
    let mut ground = item(Shape::Block, Vec3::new(0.0, -6.0, -60.0), 1.0);
    ground.transform *= Mat4::from_scale(Vec3::new(200.0, 1.0, 120.0));
    ground.material.base_color = [0.35, 0.3, 0.2];
    let mut items = vec![wall, ground];
    for column in 0..3 {
        let material = Material {
            base_color: [0.2, 0.6, 0.2],
            base_color_texture: Some(striped(7600 + column, 4)),
            alpha_cutoff: 0.5,
            ..Material::default()
        };
        items.push(card(
            Vec3::new(2.0 + column as f32 * 3.0, 0.0, -16.0),
            [3.2, 10.0],
            material,
        ));
    }
    for row in 0..10 {
        for column in 0..16 {
            let position = Vec3::new(
                column as f32 * 5.5 - 42.0,
                -3.0 + (row % 2) as f32 * 2.5,
                -40.0 - row as f32 * 9.0,
            );
            let mut scattered = if (row + column) % 3 == 0 {
                let material = Material {
                    base_color: [0.8, 0.7, 0.2],
                    base_color_texture: Some(striped(7700, 3)),
                    alpha_cutoff: 0.5,
                    ..Material::default()
                };
                card(position, [3.0, 3.0], material)
            } else {
                let mut ball = item(Shape::Sphere, position, 1.0);
                ball.transform *= Mat4::from_scale(Vec3::splat(2.5));
                ball.material.base_color = [0.9, 0.3 + 0.1 * (column % 5) as f32, 0.2];
                ball
            };
            scattered.cull_distance = 400.0;
            items.push(scattered);
        }
    }
    let mut list = list_of(items);
    list.sky.top_color = [0.2, 0.3, 0.6];
    list.sky.horizon_color = [0.6, 0.6, 0.7];
    list.sun.direction = Vec3::new(0.4, 0.8, 0.5).normalize();
    list.sun.intensity = 2.0;
    list.ambient.intensity = 0.4;
    list.shadows.enabled = true;
    list
}

/// `list` drawn for `frames` frames by a new renderer with `settings` and
/// occlusion culling `on`: its pixels, the scattered items it tested and
/// how many of them it kept.
fn drawn(
    (device, queue): (&wgpu::Device, &wgpu::Queue),
    list: &DrawList,
    settings: &GraphicsSettings,
    on: bool,
    frames: usize,
) -> (Vec<u8>, u32, u32) {
    let mut renderer = TextureRenderer::new(device, queue, SIZE, SIZE);
    renderer.set_graphics(settings);
    renderer.set_occlusion_culling(on);
    for _ in 0..frames {
        renderer.render(list).unwrap();
    }
    let kept = renderer.occlusion_kept().iter().sum();
    (
        renderer.read_pixels(),
        renderer.stats().occlusion_tested_items,
        kept,
    )
}

#[test]
fn occlusion_culling_leaves_out_hidden_items_and_keeps_the_picture() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let list = field();
    for anti_aliasing in [AntiAliasing::Off, AntiAliasing::Fxaa, AntiAliasing::Taa] {
        let settings = settings(anti_aliasing);
        let (all, tested, _) = drawn((&device, &queue), &list, &settings, false, 6);
        assert_eq!(tested, 0, "{anti_aliasing:?}: turned off, it still tested");
        let (culled, tested, kept) = drawn((&device, &queue), &list, &settings, true, 6);
        super::card_set::evidence(
            &format!("occlusion-culled-{anti_aliasing:?}"),
            &culled,
            SIZE,
            SIZE,
        );
        assert!(tested >= 150, "{anti_aliasing:?}: only {tested} tested");
        assert!(
            kept > 0 && kept + 10 <= tested,
            "{anti_aliasing:?}: kept {kept} of {tested}"
        );
        let differ = all.iter().zip(&culled).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "{anti_aliasing:?}: channels that differ");
    }
}

#[test]
fn under_msaa_nothing_is_tested() {
    let Some((device, queue)) = test_device() else {
        eprintln!("skipped: no graphics adapter on this machine");
        return;
    };
    let settings = settings(AntiAliasing::Msaa4);
    let (_, tested, _) = drawn((&device, &queue), &field(), &settings, true, 2);
    assert_eq!(tested, 0);
}
