# Foliage, grass and seasons: audit and plan

Paul's brief of 2026-11-28: overhaul trees, terrain grass and weather for realism within the frame budget (120 fps rasterized, 90 fps ray traced), building on the existing architecture. Audit and plan written the same day against main `d67675f` (#447); Paul approved it with trees first and every recommended decision (D1–D8 below). Shared plan document: <https://claude.ai/artifact/1awsLSLHr1oHF61FhPL8TM>. Phases land one PR each from main, in this order: P0 (this file and [ADR-0053](../../wiki/Architecture/Architecture-Decisions/ADR-0053-Foliage-Data-Per-Corner-And-Grass-Generated-On-The-GPU.md)), then T1–T5, G1–G5, S1–S5.

## Summary

1. **Trees** (T): an in-engine tree generator, a clean-room take on Mtree's trunk, branch, growth and leaf functions, that makes ordinary models with their own LOD copies, extra trunk detail only at the base of LOD0, leaf clusters with baked crown normals and shade, and per-corner foliage data for layered wind and seasons.
2. **Procedural grass** (G): a terrain-only grass layer generated and culled on the GPU each frame, drawn with a handful of indirect draws as solid curved blades (no alpha test), in place of scattered grass-card models.
3. **Seasons** (S): one world Season state that shifts leaf and grass colour, thins canopies (bare branches in winter), dries grass and works with the existing snow, with per-asset profiles and automatic presets.

**The frame budget.** 120 fps is 8.3 ms and 90 fps 11.1 ms. Overgrown Street was at about 44 fps at Ultra and 23 to 28 fps with every ray traced effect on 2026-11-28 ([../rendering/performance.md](../rendering/performance.md)), so these phases can't reach the targets alone. Each phase has its own cost ceiling and a no-regression gate; Phase 2 should give time back, because scattered plants are the biggest raster cost (prepass 4.7 ms, scene 3.0 ms, cut-out depth 2.0 ms on 2026-11-27).

## Audit: how it works today

There is no tree or grass generator in the engine: every tree and grass tuft is an imported model (for example from the Blender Sapling pipeline in the project's Trees folder) or a built-in shape.

| System | Where | How it works |
|---|---|---|
| Built-in plants | `crates/scene/src/foliage.rs` | Grass is two crossed Wedges, a tree a Cylinder and a Ball. Culled by distance (grass 120, tree 800 studs) with a dither fade over the last tenth. |
| Painted foliage | `TerrainData::foliage`, `crates/terrain/src/brush/scatter.rs`, the editor's Scatter pane | Instances stored in the terrain (at most 32,768, `foliage.json`). Painting is already terrain-only ([../editor/scatter.md](../editor/scatter.md): it doesn't paint onto parts). |
| Scatter rules (PW1, PW2, PW5) | `crates/procedural/src/terrain_scatter.rs`, `ScatterRule` in `crates/core/src/class.rs` | Hash-grid placement per 32-sample chunk, not saved, rebuilt only for chunks whose list changed. Density, Spacing, Clumping, height, slope and layer windows, CastShadow, AlignToSlope, AvoidParts. Every result is a CPU `DrawItem` (about 600 bytes). |
| Grouping and culling | `scene` groups of 48 studs, `crates/render/src/renderer/clusters.rs`, `culling.rs` | Groups (`DrawList::chunks`) skipped whole out of view; plants instanced by mesh, material and distance band (about 970 batches for 57,000 to 67,000 plants in Overgrown Street). CPU frustum culling only: no GPU culling, no Hi-Z (the occlusion probe measured 5 to 10%), no indirect draws. |
| Levels of detail | `crates/assets/src/model/lod.rs`, `model/cards.rs`, `crates/render/src/renderer/lod.rs` | A file's `_LOD1`/`_LOD2` (at most two, `LEVELS`) or meshopt halving; chosen by screen size (30%, 12%, 5%) with hysteresis and a 30-frame dithered cross-fade, kept per group (#446). Since #445 a cut-out with both file copies gets an automatic third: half its leaf cards, grown to keep coverage. Leaf cards don't simplify by edge collapse. |
| Leaf shading | `lit_leaf`, `leaf_canopy` in `crates/render/src/shader.wgsl` | The crown is taken as the ball filling the item: normals bent 60% toward it, inside shade, foot shade, a yellow-green transmission tint, per-card tone and hue noise, ground cover lit like the ground. No per-corner crown data. |
| Wind | `swayed_at` in `sway.wgsl`, `InstanceRaw::sway` | One bend by height squared, gusts from world noise, optional flutter; the same function in the prepass, scene, shadow passes and the ray traced bent BLASes ([../rendering/wind.md](../rendering/wind.md)). No branch hierarchy. |
| Shadows | [../rendering/shadows.md](../rendering/shadows.md) | Four cascades with PCSS and contact shadows, the farthest drawn one frame in four (#438) and at the LOD its size in that map calls for (#443), per-plant `ShadowReach`, CastShadow per rule. |
| Ray tracing | `crates/render/src/renderer/ray_tracing.rs`, `ray_groups.rs`, `ray_sway.rs`, `ray_cut_outs.rs` | A BLAS per mesh at the drawn LOD, small still items grouped per ground square, swaying items bent into shared BLASes within 32 studs (400,000 triangles a frame), holes traced within 60 studs, small plants' shadows to 55 studs, no refit ([../rendering/ray-tracing.md](../rendering/ray-tracing.md)). |
| Terrain | `crates/render/src/terrain.wgsl`, `crates/scene/src/terrain.rs` | CPU chunk meshes at five levels, eight splat layers in texture arrays. Heights aren't on the GPU as a texture. |
| Weather | `crates/scene/src/weather.rs`, `weathered` in `lighting.wgsl` | Rain, snow, wetness, snow depth that lifts terrain and caps items, prints, a 64×64 field around the camera; leaves take snow by their crown ([../rendering/weather.md](../rendering/weather.md)). No seasons, colour change or leaf drop. |

Two limits shape every data change: the item shader already uses all 16 sampled textures and all 16 vertex attributes (`crates/render/CLAUDE.md`), so new per-item or per-corner data is packed into existing slots or read from a storage buffer.

## Who owns what (the speed thread)

The speed and lighting thread (Unity-level performance, 2026-11-27) owns renderer passes: impostors, cached cascades, opaque caster proxies, `lod.rs` thresholds, cascade scheduling and ray tracing budgets. This plan owns content and new systems (the tree generator, the grass layer, seasons) and only adds data those passes can use. Where a generated tree's opaque crown proxy plugs into the far cascade (`Caster::lods`, #443) is agreed between the two before either adds a proxy level. PC timing windows are booked through the project's coordinator so pinned benchmarks never overlap.

## Phase 1: trees

A tree is a recipe the engine generates into an ordinary model, so LODs, scatter rules, sway, ray tracing and cooked models take it unchanged, and each generated tree is built to a triangle budget.

**Mtree's licence.** [Mtree](https://github.com/steven-ray/modular_tree) is GPL-3.0 and the engine Apache-2.0, so none of its code is copied, ported or bundled. The generator is written from the published ideas (stacked growth functions, pipe-model radii, a manifold tube mesher) and the papers they rest on (Palubicki et al. 2009, *Self-organizing tree models for image synthesis*; the da Vinci pipe rule).

- **T1. Recipe and generator** (`crates/procedural/src/tree/`). A `.axtree` file (JSON) holds a hierarchy of generators, SpeedTree-style (Paul, 2026-11-28: built into the editor like SpeedTree in Unity; our own tool, nothing of SpeedTree's code or assets): Trunk > Branches > Twigs > Leaves, each level a node with its own properties, shown as a tree in the editor rather than a free node graph (as procedural worlds' D8). The generators are Trunk (length, radius, root flare, taper, wobble), Branches per level (count, start and end along the parent, angle, roll, gravity, randomness, radius share), Growth (apical dominance, light seeking, splits), Pipe radius (children's cross-sections add up to the parent's) and Leaves (cluster mesh, density by light, size, face-out bias). Deterministic for a seed and `TREE_GENERATOR_VERSION`. The mesher makes one continuous tube mesh: branch bases welded into the parent through a collar that ramps the radius, U once around (tiled by radius so texel density stays even), V by arc length, continuous across joins. Primitives `TrunkBase`, `Bark` and `Leaves`, with `_LOD1` and `_LOD2` copies (the loader takes two; #445 adds the third for cut-outs) and an opaque `Leaves_PROXY` crown shell.
- **T2. Detail only at the trunk base.** Up to `BaseDetailHeight` (default 8 studs) more rings and radial segments, a root flare with buttress lobes and displacement from the bark's height map; smooth above, the normal map carries the rest. Only LOD0 has it (wgpu has no tessellation stage, so it is real geometry, kept low and local). `TrunkBase` has no sway, so it never enters the bending budget and its BLAS is built once and shared. Budgets to confirm on the PC: hero tree LOD0 60k triangles, LOD1 25k, LOD2 8k, LOD3 2k, proxy 500 (Overgrown Street's imported oaks are 195k to 260k at LOD2).
- **T3. Canopy.** Leaves as clusters (twig meshes of 3 to 7 slightly curved leaves), placed where light reaches by a coarse shadow grid over the crown during growth. Each leaf corner gets a baked crown normal and crown-depth shade, which `lit_leaf` uses in place of the `leaf_canopy` ball when present. Clusters are stored in an even-spread order (Morton, bit-reversed, as #445's), which LOD3 thinning and seasonal leaf drop share.
- **T4. Layered wind.** Each corner gets a branch level and a phase: trunk bend (today's), branch sway by level, then leaf flutter; the same `swayed_at` in every pass and in `ray_sway.wgsl`.
- **T5. Editor: Foliage Studio.** An in-editor authoring tool in the manner of SpeedTree in Unity (our own): a panel beside Model Studio with the generator hierarchy on the left, each node's properties with curves (radius along length, branch count by height, leaf size by depth) on the right, and a live preview with wind, season and an LOD and triangle readout, seeds and presets (oak, maple, birch, pine, spruce, dead tree; and for smaller foliage, shrub, fern and grass clump). `.axtree` files (and `.axfoliage` for shrubs and clumps, the same format) show in the Assets panel, open in the Studio on double-click, are placed by Insert > Tree and painted with the Scatter pane and scatter rules (`trees/oak.axtree#3` for variant 3, cooked once like any model). The Studio also edits a GrassLayer's blade look (G1) with the same live preview.

**Progress.** T1 #450, T2 #451 and T3 #453 are merged (T3's renderer half, the per-corner crown data below, is T3b, not started). T5 lands in two parts: T5a (Insert > Tree, a tree's recipe, model and maps written into the project's `assets/trees/`, `docs/editor/building.md`) with a realism pass after Paul's review ("they look a bit geometric", 2026-11-28): CC0 bark and leaf maps from ambientCG (`assets/trees/textures/`), textured leaves as cut-out cards showing one leaf or spray of the picture each, and limbs that wander in smooth waves; T5b is the Foliage Studio panel.

**Realism upgrade (Paul's brief, 2026-11-28 17:50Z).** Four PRs from main, in order: W1 wood, W2 foliage volume (crossed and bent clusters, bent normals, species crowns), W3 packed vertex colour (R ambient occlusion, G limb sway, B flutter, A weathering, read by the renderer through the per-corner data of ADR-0053) and W4 LOD1 interior culling plus an LOD2 proxy for the far cascade and ray tracing; budgets unchanged (LOD0 60k, LOD1 25k, LOD2 8k). W1 (generator version 5): side limbs with four or more corners are welded onto their parent by a ring lying on its surface and a fillet ring onto the limb, flared outward (retried narrower, else a plain start from inside the parent, when the flare would fold); every tube's normals come from its own shape; the bark's U spirals along the limb (`Bark.twist`, degrees per stud) continuously from the base; a `terminal` branch level carries each limb on from its tip into the crown, so thick limbs never stop in open air; the trunk's sunk foot widens under the ground (`Trunk.root_spread`) and its root lobes reach down, so a tree on a slope shows no edge; a limb turns at most 35° at one ring; deadwood (`Trunk.broken`, `BranchLevel.broken`) snaps off into splinters round a split end grain, with snags along the limbs, and the bark carries a weathering mask by height (`Bark.weathering`, `weathering_from`; `TreeMesh::weathering`, written out in W3).

**Data.** Per-corner foliage data (crown normal, crown shade, branch level, wind phase, cluster rank) is one or two `u32` per corner in a storage buffer read by `vertex_index`, the way posed corners are; the item finds its rows through `InstanceRaw::skin`, which unposed items leave at `[0, 0]`. Three places read `[0, 0]` as unposed (snow's lift in `shader.wgsl`, the motion pass's `previous_corner`, and the ray tracer's posed or unposed BLAS choice), so the marker keeps all three right and `crates/render/src/renderer/tests/grouped_items.rs` covers it. `MapsUniform` has spare lanes (`specular.zw`, `uv_offset.w`, `toon.yzw`) for the bark blend and the season profile. Models without the data draw exactly as now.

**Cost.** No change in draw calls; fewer triangles than today's imported trees; the far cascade gets a cheap caster; `TrunkBase` is opaque and still; a few ALU per leaf pixel replace the ball guess. Gate: Desert Homestead and Overgrown Street with their old trees stay within run-to-run noise, and a scene with generated trees beats the same scene with imported ones.

## Phase 2: procedural grass

Grass is a terrain layer the GPU fills with blades each frame around the camera.

| Trick | Today | Phase 2 |
|---|---|---|
| Instancing | By mesh, material and distance band | One indirect draw per blade LOD |
| Frustum culling | CPU, per item | GPU, per 8×8-stud tile grown by blade height and sway reach |
| Distance LOD and density falloff | Cull distance with dither fade; screen-size LODs | Blades thin with distance and widen to keep cover; near 15 corners, far 7, then terrain only |
| Hi-Z occlusion | A measuring probe only | Optional (G4), kept only if it measures |
| Compute-written instance data | GPU particles (`gpu_particles.rs`) | The same pattern for blades and indirect arguments |
| Indirect draws | None | `draw_indexed_indirect` (core wgpu) |
| Wind | `swayed_at` gust field | The same field, so grass and trees move together |
| Terrain colour and normal at the root | None (heights not on the GPU) | A height texture per terrain; blades read splat weights and layer albedo at the root |
| Alpha test | Every grass model is a cut-out card | Solid blades: no `discard`, `depth_start`'s fast path, no cut-out depth fill |

- **G1. Grass layer.** A `GrassLayer` Instance inside a Terrain, covering ground by a TerrainLayer's weight, a painted density map (`grass.png`, 8-bit per sample, saved only when painted) or both, and by the ScatterMask windows. Grass lives in `TerrainData`, so it is only ever on Terrain; a part that should carry grass is turned into ground with Solid Modeling > To Terrain. The Scatter pane gets a Grass brush for the density map. Holes, water and `TerrainPath` strips clear grass as they clear scatter rules. Properties: `BladeHeight`, `BladeWidth`, `Density`, `Clumping`, `ClumpSize`, `Lean`, `Curvature`, `WindResponse`, `Stiffness`, `RootColor`, `TipColor`, `ColorVariation`, `TerrainBlend`, `Dryness`, `MaxDistance`, `CastShadow` (Off, Near), `Style` (Realistic, Stylized); presets Lawn, Meadow, Tall Grass, Dry Steppe, Stylized.
- **G2. Generation and drawing.** Each terrain uploads its heights as an R32F texture (holes in a spare channel), sculpting writing only changed rows. A compute pass ("Grass" in the profiler, before the prepass) tests each tile within `MaxDistance` against the frustum, works out its blade count from density and distance, and places blades on a world-anchored hash grid; each blade is a 16-byte record appended to a storage buffer, and the counts go into the indirect arguments. The vertex shader pulls the blade by `instance_index` (its own pipeline), builds a curved blade, rounds its normal across the width and applies wind; blades fade into the terrain's colour and normal near `MaxDistance`. Lit by the ground-cover leaf path plus root-to-tip occlusion and `weathered`. About 4 to 6 draw calls in all.
- **G3. Correctness.** Motion vectors from last frame's wind time for TAA; blades shorten under the snow's depth and vanish under water and in holes; the terrain darkens a little under dense grass (contact shadows are skipped when ray traced shadows are on).
- **G4. Optional.** Tile Hi-Z against last frame's depth, casting into cascade 0 at a quarter density, characters bending grass. Each kept only if it measures worth it.
- **G5. Moving scenes over.** A "Convert to grass layer" action on ScatterRules of small grass cards, opt-in, with a backup. Overgrown Street's Fine grass and Forest grass are the A/B test.

**Cost targets** (to confirm on the PC): under 0.1 ms of CPU a frame; +4 to 6 draw calls with hundreds of grass batches removed; at most 1.5 ms of GPU at 2560 × 1440 Ultra for a full meadow; no shadow-map casters by default; nothing added to the TLAS or BLASes (blades receive ray traced shadows and AO, which trace from prepass pixels). Gate: Overgrown Street with its grass rules converted is faster than with the cards, checked against stills first.

## Phase 3: seasons

One world season value drives colour, canopy fullness and dryness through a small table in the frame uniform.

- **S1. The season state.** A `Season` object inside Lighting beside `Weather` (the first is used; without one nothing changes): `Season` (Spring, Summer, Autumn, Winter), `Progress` (0 to 1), `TransitionTime`, `Cycle` (Off, or by days with `DaysPerSeason`, advancing when `ClockTime` wraps) and `AffectsWeather` (winter raises Weather's default `SnowCover`, autumn its `Wetness`; Weather's own values win where set). Environment presets, the sequence Environment track and `Lighting:TransitionToPreset` carry it; Luau gets `Lighting.Season` and a changed event.
- **S2. Profiles.** Each foliage material has a season profile (Deciduous, Deciduous Colourful, Evergreen Broadleaf, Conifer, Larch, Grass Temperate, Grass Dry, Flowers, None) with `SeasonStrength` and `SeasonSpread`, set in a material file's `season` block, a glTF material's `extras` (`axcenity_season`), the `.axtree` recipe, or on a ScatterRule or GrassLayer. Automatic presets at import by name and tags (oak, maple, birch, beech, aspen: Deciduous; pine, spruce, fir, juniper: Conifer; ivy, holly, laurel: Evergreen; grass, fern: Grass Temperate), shown and changeable in the Material tab.
- **S3. Rendering.** A `season` vec4 and a palette (about 9 profiles × 4 seasons × 2 colours) in the frame uniform; the profile in a spare `MapsUniform` lane. A luminance-keeping tint in `lit_leaf` and the grass shader, a per-tree autumn colour from a hash of its position, dryness as less transmission and more roughness. Leaf drop: the first fullness share of the clusters (drop order from T3, or from `cards.rs` for imported models) is drawn by index range in every pass, and a per-tree dissolve by rank smooths the last step. TerrainLayers get an optional season tint; ScatterRules and GrassLayers a fullness curve (litter in autumn, flowers in spring) through the same path, so nothing is regenerated on the CPU.
- **S4. Ray tracing.** Rays follow the coarse step: a leaf BLAS is rebuilt when its fullness step changes (8 steps a transition), inside the BLAS budget and spread over frames; bent sway copies read the same range. The fine dissolve isn't traced.
- **S5. Editor.** A Season section in Lighting's properties with a scrubber, palette editing and a preview in the Material tab.

**Cost.** No pass or draw call; a few ALU per leaf and grass pixel; fewer triangles and casters as canopies thin; BLAS rebuilds only at fullness steps, TLAS unchanged; one uniform write a frame. Gate: winter costs no more than summer.

## Decisions (Paul, 2026-11-28)

| ID | Decision | Chosen |
|---|---|---|
| D1 | Where trees are generated | In the engine (`.axtree`, Tree Studio) |
| D2 | Mtree's code | Clean-room only (GPL-3.0) |
| D3 | Grass on parts | Terrain only; To Terrain to convert a part. A `GrassSurface` flag for MeshParts later if wanted |
| D4 | Grass shadows | None in the shadow maps; occlusion and ground darkening instead |
| D5 | How seasons are driven | A Season object, by hand or cycling by days (not a calendar date and latitude) |
| D6 | Crown proxies and impostors in the renderer | The speed thread owns them; generated trees emit `Leaves_PROXY` |
| D7 | Leaf drop in ray tracing | Coarse steps only |
| D8 | Existing grass scatter rules | Opt-in convert, with a backup |
| D9 | Phase order | Trees, grass, seasons |

## Tests and benchmarks

Cloud unit tests for the generator (same seed, same mesh; triangle budget per LOD; UVs continuous across joins; drop order even) and GPU tests on lavapipe (grass only on terrain; blades gone under water, in holes and under deep snow; winter draws fewer leaf triangles; a model without foliage data draws pixel for pixel as before). Release player `--bench 300`, pinned, alternating rounds against main on the PC, on Desert Homestead, Overgrown Street and a meadow-and-forest test scene.
