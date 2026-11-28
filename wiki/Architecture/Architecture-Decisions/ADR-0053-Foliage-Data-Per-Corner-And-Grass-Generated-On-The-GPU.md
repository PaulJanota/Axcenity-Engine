---
title: "ADR-0053: Foliage data rides per corner in a storage table, and grass is generated on the GPU"
type: adr
adr: 53
adr_status: Accepted
date: 2026-11-28
supersedes:
superseded_by:
affects: [procedural, render, scene, assets, editor]
source: [crates/render/src/renderer/types.rs, crates/render/src/shader.wgsl, crates/render/src/sway.wgsl, crates/scene/src/foliage.rs, crates/procedural/src/terrain_scatter.rs]
docs: [docs/research/foliage-grass-seasons.md, docs/rendering/wind.md, docs/rendering/weather.md, docs/terrain.md]
tags: [adr, rendering, foliage, procedural]
---

# ADR-0053: Foliage data rides per corner in a storage table, and grass is generated on the GPU

- **Status:** Accepted (Paul approved the foliage plan with its recommended decisions on 2026-11-28)
- **Date:** 2026-11-28
- **Affected systems:** procedural (tree generator), render (item shader, sway, ray tracing, a grass pass), scene (foliage), assets (models), editor (Foliage Studio)

## Context and problem

Paul's brief of 2026-11-28 asks for generated trees with layered wind and baked crown shading, procedural terrain grass in dense fields, and seasons that recolour and thin foliage, all within the frame budget (`docs/research/foliage-grass-seasons.md`). Two facts constrain how:

- The item shader already uses all 16 sampled textures and all 16 vertex attributes (`crates/render/CLAUDE.md`), so per-corner foliage data (crown normal, crown shade, branch level, wind phase, leaf rank) can't be a new vertex attribute.
- Every scattered plant is a CPU `DrawItem` of about 600 bytes, planned, culled and batched on the CPU (`crates/scene/src/foliage.rs`, `crates/render/src/renderer/clusters.rs`); Overgrown Street draws 57,000 to 67,000 in about 970 batches, and its scattered plants are the biggest raster cost (`docs/rendering/performance.md`). A grass field of blades can't take that path.

## Considered alternatives

| Option | For | Against |
|---|---|---|
| Per-corner data in a storage table read by `vertex_index`, found through the item's unused `InstanceRaw::skin` | No new attribute, no `InstanceRaw` growth; the posed-corner buffer already works this way | Three readers of `skin == [0, 0]` (snow's lift, motion vectors, the ray tracer) must keep telling posed from foliage |
| Pack data into spare bits (`tangent.w`'s magnitude, UV integer parts) | Nothing new bound | Fragile; every importer and mesher must preserve it; a few bits only |
| A second vertex buffer | Conventional | Needs attribute slots the shader doesn't have |
| Grass as scattered models (today) | Exists | CPU cost per tuft, alpha-tested cards, hundreds of batches |
| Grass generated per frame on the GPU, drawn with `draw_indexed_indirect` | No CPU work per blade; solid blades skip the cut-out paths; a few draws | A new compute pass and pipeline; motion vectors and weather to match |

## Decision

Foliage meshes carry their per-corner data in a storage table read by `vertex_index`, located through `InstanceRaw::skin` for unposed items, with a marker that keeps every existing reader of an unposed `skin` right; models without the data draw as before. Terrain grass is a `GrassLayer` inside a Terrain whose blades a compute pass generates and culls around the camera each frame and two indirect draws per pass draw; it never becomes `DrawItem`s, casts no shadow-map shadow by default and is not in the ray traced scene.

## Reasons

- Both keep within the shader's binding and attribute limits and keep `render` consuming draw data only (ADR-0003): the table is data the scene hands over, and the grass layer is terrain data drawn by the renderer.
- The grass path removes per-plant CPU work instead of making it cheaper, and solid blades avoid the alpha-tested costs measured on RADV.

## Consequences

- A new frame binding or a share of an existing storage buffer, within wgpu's default of 8 storage buffers per stage.
- Ray traced shadows and AO fall on grass (they trace from prepass pixels) but grass never shadows anything by rays; contact darkening stands in.
- Revisit if a scene needs grass in the shadow maps beyond cascade 0, or if per-corner data must survive a mesh's re-simplification (the table is per mesh and LOD copy).

## Implementation references

- Plan phases T1–T5 (trees), G1–G5 (grass), S1–S5 (seasons) in `docs/research/foliage-grass-seasons.md`.
- Tests to guard it: `crates/render/src/renderer/tests/grouped_items.rs` (a grouped foliage list draws as its items one by one), grass-only-on-terrain GPU tests.
