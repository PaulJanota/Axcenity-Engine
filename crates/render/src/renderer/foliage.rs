//! Plants' packed data per corner (foliage W3b, ADR-0053,
//! `docs/rendering/wind.md`).
//!
//! A mesh whose [`MeshSource::foliage`] isn't empty (a generated tree's
//! bark and leaves) keeps it on the GPU as one word a corner
//! ([`pack`]: occlusion, sway, flutter and a shade mask, 8 bits each), in
//! a buffer of its own ([`GpuMesh::foliage`]). Each frame the meshes some
//! view draws are taken up into one table ([`FoliageTable`], the frame's
//! binding [`FOLIAGE_BINDING`]), copied in once and kept while they fit,
//! and each unposed instance and caster drawn with one gets
//! `skin = [its first corner there, FOLIAGE]`: every vertex shader that
//! bends corners in the wind (scene, prepass, shadows, motion vectors)
//! reads its corners' weights there (`corner_foliage` in shader.wgsl), and
//! the ray traced scene's bent copies read the mesh's own buffer
//! (`ray_sway.rs`), so all of them bend a corner alike.
//!
//! [`MeshSource::foliage`]: crate::MeshSource::foliage

use std::ops::Range;

use rustc_hash::FxHashMap;

use super::lighting::ViewCasters;
use super::types::{Batch, GpuBytes, InstanceRaw};
use super::{GpuMesh, MeshKey};
use crate::MeshSource;

/// The frame's binding the table is at. Must match `foliage_corners` in
/// shader.wgsl.
pub(super) const FOLIAGE_BINDING: u32 = 20;

/// `InstanceRaw::skin[1]` of an item drawn at rest whose corners' foliage
/// data starts at `skin[0]` in the table. Never a posed item's (1) or one
/// waiting to be posed (`posing::PENDING`). Must match `FOLIAGE_CORNERS` in
/// shader.wgsl.
pub(super) const FOLIAGE: u32 = u32::MAX - 1;

/// The table grows in steps of this many bytes.
const STEP: u64 = 1 << 20;

/// `foliage`'s values, each clamped to 0 to 1, as one word a corner: the
/// first in the lowest 8 bits (WGSL's `unpack4x8unorm`).
pub(super) fn pack(foliage: &[[f32; 4]]) -> Vec<u32> {
    foliage
        .iter()
        .map(|values| {
            values.iter().enumerate().fold(0, |word, (i, &value)| {
                let value = if value.is_finite() { value } else { 0.0 };
                let byte = (value.clamp(0.0, 1.0) * 255.0).round() as u32;
                word | (byte << (8 * i))
            })
        })
        .collect()
}

/// `source`'s foliage data as a buffer of its own, and what it counts, or
/// `None` for a mesh without it (or with the wrong number of values).
pub(super) fn upload(
    device: &wgpu::Device,
    bytes: &GpuBytes,
    source: &dyn MeshSource,
) -> Option<(wgpu::Buffer, super::types::Counted)> {
    use wgpu::util::DeviceExt;
    let foliage = source.foliage();
    if foliage.is_empty() {
        return None;
    }
    if foliage.len() != source.positions().len() {
        log::warn!(
            "mesh #{} has foliage data for {} of its {} corners, so it sways as a whole",
            source.key(),
            foliage.len(),
            source.positions().len()
        );
        return None;
    }
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("foliage corners"),
        contents: bytemuck::cast_slice(&pack(foliage)),
        // Copied into the frame's table, and read by ray tracing's bending.
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let counted = bytes.count(buffer.size());
    Some((buffer, counted))
}

/// Where one mesh's words are in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Place {
    first: u32,
    corners: u32,
    version: u64,
}

/// The frame's table of foliage data: each mesh some view draws, copied in
/// from its own buffer the first frame it's drawn and kept while there's
/// room. When the frame's meshes don't fit, it's filled afresh with just
/// them, grown to half again what they take.
pub(super) struct FoliageTable {
    buffer: wgpu::Buffer,
    places: FxHashMap<MeshKey, Place>,
    /// Words in use.
    used: u64,
    /// Meshes to copy in when the frame is recorded, and where.
    copies: Vec<(MeshKey, u32)>,
    /// Counts the buffers made: the frame's bindings are made again when
    /// it changes.
    pub(super) generation: u64,
    /// Whether a frame's meshes have been too many to fit (warned once).
    warned: bool,
}

impl FoliageTable {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        Self {
            buffer: table_buffer(device, 64),
            places: FxHashMap::default(),
            used: 0,
            copies: Vec::new(),
            generation: 0,
            warned: false,
        }
    }

    /// The table (the frame's binding [`FOLIAGE_BINDING`]).
    pub(super) fn buffer(&self) -> &wgpu::Buffer {
        &self.buffer
    }

    /// Where `key`'s data starts this frame, if it was taken up.
    pub(super) fn first(&self, key: MeshKey) -> Option<u32> {
        self.places.get(&key).map(|place| place.first)
    }

    /// Takes up each mesh of `wanted` (with its corners and version) that
    /// isn't in yet, filling the table afresh if they don't all fit.
    fn take_up(&mut self, device: &wgpu::Device, wanted: &[(MeshKey, u32, u64)]) {
        let missing = |places: &FxHashMap<MeshKey, Place>, &(key, corners, version): &_| {
            places
                .get(&key)
                .is_none_or(|place| place.corners != corners || place.version != version)
        };
        let new: Vec<_> = wanted
            .iter()
            .filter(|want| missing(&self.places, want))
            .copied()
            .collect();
        if new.is_empty() {
            return;
        }
        let words: u64 = new.iter().map(|&(_, corners, _)| u64::from(corners)).sum();
        if (self.used + words) * 4 <= self.buffer.size() {
            for (key, corners, version) in new {
                self.place(key, corners, version);
            }
            return;
        }
        // Afresh: room for half again what this frame's meshes take.
        self.places.clear();
        self.copies.clear();
        self.used = 0;
        let all: u64 = wanted
            .iter()
            .map(|&(_, corners, _)| u64::from(corners))
            .sum();
        let most = device.limits().max_storage_buffer_binding_size;
        let size = (all * 4 * 3 / 2).div_ceil(STEP) * STEP;
        let size = size.min(most / 4 * 4).max(64);
        if size > self.buffer.size() {
            self.buffer = table_buffer(device, size);
            self.generation += 1;
        }
        for &(key, corners, version) in wanted {
            if self.places.contains_key(&key) {
                continue;
            }
            if (self.used + u64::from(corners)) * 4 > self.buffer.size() {
                if !self.warned {
                    self.warned = true;
                    log::warn!(
                        "plants' foliage data doesn't fit one binding, so some sway as a whole"
                    );
                }
                continue;
            }
            self.place(key, corners, version);
        }
    }

    fn place(&mut self, key: MeshKey, corners: u32, version: u64) {
        let first = self.used as u32;
        self.places.insert(
            key,
            Place {
                first,
                corners,
                version,
            },
        );
        self.copies.push((key, first));
        self.used += u64::from(corners);
    }

    /// Gives each unposed camera instance (by `batches`, their mesh and
    /// range) and each caster of `view_casters` drawn with a mesh that has
    /// foliage data its corners' place in the table, taking the meshes up
    /// first. `mesh` finds a mesh on the GPU. A new buffer changes
    /// [`FoliageTable::generation`] (the frame's bindings must be made
    /// again).
    pub(super) fn mark<'a>(
        &mut self,
        device: &wgpu::Device,
        (batches, instances): (&[Batch], &mut [Vec<InstanceRaw>]),
        view_casters: &mut [ViewCasters],
        mesh: impl Fn(MeshKey) -> Option<&'a GpuMesh>,
    ) {
        let with_data = |key: MeshKey| {
            mesh(key)
                .filter(|gpu| gpu.foliage.is_some())
                .map(|gpu| (key, gpu.vertex_count, gpu.version))
        };
        let mut wanted: Vec<(MeshKey, u32, u64)> = Vec::new();
        let mut seen: FxHashMap<MeshKey, bool> = FxHashMap::default();
        let mut want = |key: MeshKey| {
            *seen.entry(key).or_insert_with(|| match with_data(key) {
                Some(entry) => {
                    wanted.push(entry);
                    true
                }
                None => false,
            })
        };
        let camera: Vec<(MeshKey, Range<u32>)> = batches
            .iter()
            .filter(|batch| want(batch.mesh))
            .map(|batch| (batch.mesh, batch.instances.clone()))
            .collect();
        let casters: Vec<Vec<(MeshKey, Range<u32>)>> = view_casters
            .iter()
            .map(|view| {
                view.groups
                    .iter()
                    .filter(|((key, _), _)| want(*key))
                    .map(|((key, _), range)| (*key, range.clone()))
                    .collect()
            })
            .collect();
        if wanted.is_empty() {
            return;
        }
        self.take_up(device, &wanted);
        let give = |pieces: &mut [Vec<InstanceRaw>], runs: &[(MeshKey, Range<u32>)]| {
            for (key, range) in runs {
                if let Some(first) = self.first(*key) {
                    for_each_in(pieces, range.clone(), |raw| {
                        if raw.skin == [0, 0] {
                            raw.skin = [first, FOLIAGE];
                        }
                    });
                }
            }
        };
        give(instances, &camera);
        for (view, runs) in view_casters.iter_mut().zip(&casters) {
            give(&mut view.instances, runs);
        }
    }

    /// Records this frame's copies of newly taken up meshes into the
    /// table, before any pass reads it.
    pub(super) fn record<'a>(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        mesh: impl Fn(MeshKey) -> Option<&'a GpuMesh>,
    ) {
        for (key, first) in self.copies.drain(..) {
            let Some(source) = mesh(key).and_then(|gpu| gpu.foliage.as_ref()) else {
                continue;
            };
            let bytes = source.size();
            let at = u64::from(first) * 4;
            if at + bytes <= self.buffer.size() {
                encoder.copy_buffer_to_buffer(source, 0, &self.buffer, at, bytes);
            }
        }
    }

    /// The table's size, counted with the frame's targets in
    /// [`crate::RenderStats`].
    pub(super) fn bytes(&self) -> u64 {
        self.buffer.size()
    }
}

fn table_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("foliage table"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Calls `each` on the instances `range` holds of `pieces` (counted across
/// them, from the first).
fn for_each_in(
    pieces: &mut [Vec<InstanceRaw>],
    range: Range<u32>,
    mut each: impl FnMut(&mut InstanceRaw),
) {
    let mut start = 0_u32;
    for piece in pieces {
        let end = start + piece.len() as u32;
        let from = range.start.max(start);
        let to = range.end.min(end);
        if from < to {
            for raw in &mut piece[(from - start) as usize..(to - start) as usize] {
                each(raw);
            }
        }
        start = end;
        if start >= range.end {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foliage_values_pack_into_a_word_first_lowest() {
        let words = pack(&[[1.0, 0.0, 0.5, 2.0], [-1.0, f32::NAN, 0.25, 1.0]]);
        assert_eq!(words[0], 0xFF | (128 << 16) | (0xFF << 24));
        assert_eq!(words[1], (64 << 16) | (0xFF << 24));
    }

    #[test]
    fn a_range_across_pieces_reaches_each_of_its_instances() {
        let raw = |n: u32| InstanceRaw {
            skin: [n, 0],
            ..bytemuck::Zeroable::zeroed()
        };
        let mut pieces = vec![
            (0..3).map(raw).collect::<Vec<_>>(),
            vec![],
            (3..7).map(raw).collect(),
        ];
        let mut reached = Vec::new();
        for_each_in(&mut pieces, 2..5, |raw| reached.push(raw.skin[0]));
        assert_eq!(reached, [2, 3, 4]);
    }
}
