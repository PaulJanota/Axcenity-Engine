//! Lamps' shadow views kept between frames.
//!
//! A spot light's view and each face of a point light's are drawn again
//! only where what they're drawn from has changed. A lamp that stays where
//! it is, with casters that stay where they are, keeps last frame's layer
//! of its shadow map, as the sun's far cascades keep theirs while they take
//! turns. Casters that move every frame (posed, or swaying in the wind) are
//! drawn every frame, over a kept copy of the still ones: the still casters
//! are drawn alone into [`KeptLayers`] when they change, and each frame that
//! layer is copied into the map and the moving casters drawn over it. A
//! shadow map keeps the nearest depth whatever the order, so that's the
//! same map as drawing them all.
//!
//! What a view's still casters are is summed up as one number,
//! [`fingerprint`], kept with the layer ([`super::ShadowMap::drawn_from`]).
//! It holds everything the shadow shaders read (`vs_shadow`,
//! `vs_shadow_cut_out` and `fs_shadow_cut_out`), so a kept layer holds the
//! depths drawing it again would give.

use std::hash::{Hash, Hasher};

use glam::Mat4;
use rustc_hash::FxHasher;

use super::MeshKey;
use super::lighting::ViewCasters;
use super::types::InstanceRaw;

/// Whether lamps' shadow views are drawn again every frame, as they were
/// until 2026-11-26: `AXCENITY_NO_LAMP_SHADOW_KEEP` asks for it, to
/// measure what keeping them saves (`docs/rendering/lights.md`).
pub(super) fn redrawn_every_frame() -> bool {
    static ASKED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ASKED.get_or_init(|| std::env::var_os("AXCENITY_NO_LAMP_SHADOW_KEEP").is_some())
}

/// Whether a caster's shadow moves every frame: it's posed (its corners
/// are worked out each frame; `skin`'s second number is 1, or pending), or
/// it sways while the wind blows (`windy`: it moves with the clock).
pub(super) fn moves(raw: &InstanceRaw, windy: bool) -> bool {
    let posed = raw.skin[1] != 0 && raw.skin[1] != super::foliage::FOLIAGE;
    posed || (windy && raw.sway[2] > 0.0)
}

/// A lamp map's still casters, drawn alone, layer by layer (made with the
/// map's size and layers).
pub(super) struct KeptLayers {
    pub(super) texture: wgpu::Texture,
    pub(super) layers: Vec<wgpu::TextureView>,
    /// The fingerprint each layer was drawn from, `None` for one not
    /// drawn yet.
    pub(super) drawn_from: Vec<Option<u64>>,
    /// Its depth texture's size, counted with the map's.
    pub(super) bytes: u64,
}

/// What one lamp view does with its layer this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LampDraw {
    /// Draws every caster into the layer: a cascade, or a lamp's view
    /// while they're asked to be drawn every frame.
    All,
    /// Nothing changed: the layer is kept as it is.
    Kept,
    /// Draws its still casters into the layer (it has no moving ones).
    Still,
    /// Copies its kept still casters into the layer and draws the moving
    /// ones over them, first drawing the still casters into the kept layer
    /// if `still` (they changed, or weren't drawn there yet).
    Over { still: bool },
}

/// What a lamp layer's view does this frame, from its still casters'
/// fingerprint (`print`, `None` to draw everything) and whether it has
/// moving casters; `layer` and `kept` are what the layer and its kept copy
/// were last drawn from, and become this frame's.
pub(super) fn plan(
    layer: &mut Option<u64>,
    kept: &mut Option<u64>,
    print: Option<u64>,
    moving: bool,
) -> LampDraw {
    match (print, moving) {
        (None, _) => {
            *layer = None;
            LampDraw::All
        }
        (Some(print), false) if *layer == Some(print) => LampDraw::Kept,
        (Some(print), false) => {
            *layer = Some(print);
            LampDraw::Still
        }
        (Some(print), true) => {
            *layer = None;
            let still = *kept != Some(print);
            *kept = Some(print);
            LampDraw::Over { still }
        }
    }
}

/// What each of a lamp `map`'s views does with its layer this frame (see
/// [`plan`]; `prints` and `moving` are per view, in layer order), making
/// the map's [`KeptLayers`] the first time a view has moving casters. They
/// last as long as the map.
pub(super) fn plan_layers<S>(
    device: &wgpu::Device,
    map: Option<&mut super::ShadowMap<S>>,
    prints: &[Option<u64>],
    moving: &[bool],
) -> Vec<LampDraw> {
    let Some(map) = map else {
        return Vec::new();
    };
    let over = prints
        .iter()
        .zip(moving)
        .any(|(print, moving)| print.is_some() && *moving);
    if over && map.kept.is_none() {
        let size = map.texture.size();
        let (texture, _, layers) = super::gpu::create_shadow_layers(
            device,
            "kept lamp shadows",
            (size.width, size.depth_or_array_layers),
            true,
        );
        map.kept = Some(KeptLayers {
            texture,
            drawn_from: vec![None; layers.len()],
            layers,
            bytes: map.bytes,
        });
    }
    let mut unkept = None;
    let views = prints.iter().zip(moving).enumerate();
    views
        .map(|(layer, (print, moving))| {
            let kept = match &mut map.kept {
                Some(kept) => &mut kept.drawn_from[layer],
                None => &mut unkept,
            };
            plan(&mut map.drawn_from[layer], kept, *print, *moving)
        })
        .collect()
}

/// What a lamp's shadow view's still casters are, as one number (the
/// groups before `view.still_groups`; the moving ones after are drawn
/// every frame).
///
/// It sums up the view's `matrix`, and group by group its mesh (`mesh`
/// gives an uploaded mesh's [`MeshSource::version`], `None` while it's
/// still to come) and the material it's drawn through (`material` gives a
/// cut-out's [`GpuMaterial::serial`], which a material's binding made
/// again changes), then each caster's values the shadow shaders read: its
/// place, opacity, shadow share, cutoff, pose and sway. A caster's colour
/// and the rest of its surface aren't read, so a lamp's flame changing
/// colour doesn't draw its shadows again.
///
/// [`MeshSource::version`]: crate::MeshSource::version
/// [`GpuMaterial::serial`]: super::GpuMaterial::serial
pub(super) fn fingerprint(
    matrix: Mat4,
    view: &ViewCasters,
    mesh: impl Fn(MeshKey) -> Option<u64>,
    material: impl Fn(u32) -> u64,
) -> u64 {
    let mut hasher = FxHasher::default();
    hasher.write(bytemuck::cast_slice(&matrix.to_cols_array()));
    let still = &view.groups[..view.still_groups];
    for ((key, cut_out), range) in still {
        key.hash(&mut hasher);
        mesh(*key).hash(&mut hasher);
        cut_out.map(&material).hash(&mut hasher);
        (range.start, range.end).hash(&mut hasher);
    }
    let casters = still.last().map_or(0, |(_, range)| range.end as usize);
    for raw in view.instances.iter().flatten().take(casters) {
        hasher.write(bytemuck::cast_slice(&raw.model));
        hasher.write(bytemuck::cast_slice(&[
            raw.base_color_opacity[3],
            raw.emissive_fade[3],
            raw.surface[2],
        ]));
        hasher.write(bytemuck::cast_slice(&raw.skin));
        hasher.write(bytemuck::cast_slice(&raw.sway));
    }
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_layer_is_drawn_again_only_where_its_still_casters_changed() {
        let (mut layer, mut kept) = (None, None);
        let mut step = |print, moving| plan(&mut layer, &mut kept, print, moving);
        // A new layer draws its still casters, then keeps them.
        assert_eq!(step(Some(1), false), LampDraw::Still);
        assert_eq!(step(Some(1), false), LampDraw::Kept);
        assert_eq!(step(Some(2), false), LampDraw::Still);
        // Something moving comes in: the still casters are drawn alone
        // once, and the moving ones over a copy of them every frame.
        assert_eq!(step(Some(2), true), LampDraw::Over { still: true });
        assert_eq!(step(Some(2), true), LampDraw::Over { still: false });
        assert_eq!(step(Some(3), true), LampDraw::Over { still: true });
        // It goes: the layer holds the moving casters' depth, so it's
        // drawn again once, then kept.
        assert_eq!(step(Some(3), false), LampDraw::Still);
        assert_eq!(step(Some(3), false), LampDraw::Kept);
        // Drawn every frame when asked, and drawn again after.
        assert_eq!(step(None, false), LampDraw::All);
        assert_eq!(step(Some(3), false), LampDraw::Still);
    }
}
