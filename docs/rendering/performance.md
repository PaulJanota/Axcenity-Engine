# Rendering performance

How drawing is measured, and the baseline that optimizations (culling, LOD, GPU skinning, occlusion) are compared against. Measure before optimizing: a change that claims to make drawing faster shows it here.

## What's measured

- **`RenderStats`** (`TextureRenderer::stats`, `render/src/renderer.rs`), for the last frame: draw calls, triangles and vertices (the drawn meshes' corners, every instance counted; shadow views and the prepass included); items submitted (in the draw list), drawn (by the scene pass) and culled (outside the camera's view); shadow casters drawn into and culled from the shadow views (summed over views) and sprites culled (graphics Phase 12, see Frustum culling in [pipeline.md](pipeline.md)); batches (the scene pass's instanced draws, one per mesh and material); GPU bytes by kind: textures and meshes (counted at upload and when freed), render targets (`FrameTargets`, the shadow maps and the turned-text sheet, counted when made) and buffers; GPU particles; the LOD distribution (`lod_items`: items in view whose mesh has levels of detail, by the level drawn, one cross-fading counted at the level coming in; `LOD_LEVELS` is 3); surface shaders compiled and not compiling; and the GPU's time for the frame and for each pass (`pass_times`, `pass_time(FramePass)`).
- **Per-pass GPU time** (`renderer/gpu_timer.rs`) needs `Features::TIMESTAMP_QUERY`, which the editor, the benchmarks and the player ask for where the adapter has it. The player times frames only while `Renderer::set_gpu_timing` is on: during `--bench`, and while its `fps` readout or the console's Stats tab is shown, so a game nobody is measuring pays nothing. `RenderStats::gpu_readings` counts the readings, so a reader that keeps every frame's time takes a reading once. An empty compute pass writes a timestamp before each `FramePass` (Acceleration structures, Shadows, Depth prepass, Ambient occlusion, Ray traced shadows, Volumetric fog, GPU particles, Clouds, Scene, Post-processing, UI) and one after the last, so a part made of many render passes (three cascades, the bloom chain) is timed whole. A pass the frame didn't draw (no ray tracing, shadows, ambient occlusion, ray traced shadows or volumetric fog off, no GPU particles, no UI) is `None`. Inside the Shadows pass each of the sun's cascades is stamped too (`cascade_times`, with `cascade_casters` and `cascade_triangles`); passes with no dependency between them can overlap on the GPU, so treat a cascade's time as approximate. The readings come back a frame or two later without the CPU waiting, through four readback buffers used in turn, so every frame is timed. (Until 2026-11-27 there was one: a frame was timed only once the last reading was read, which in a shown renderer, a frame ahead, is every other frame and always of the same parity. Where the far cascades take turns that read only the frames drawing cascade 3 or only those drawing cascade 4: in Overgrown Street the editor's Play read 30.3 ms of GPU where the player read 24.2 at the same view, both really 26 to 28.) **By what is drawn** (2026-11-26): the depth prepass and the scene pass are one render pass each, so within them the time is also taken per `DrawnPart` (`RenderStats::part_times`, `part_time`): `items` (models, parts, characters), `scattered items` (those with a `cull_distance`: a terrain's painted plants, stones and litter), `terrain`, and in the scene pass `cut-outs' depth`, `sky`, `decals, grid and outlines`, `see-through items` and `sprites`. A stamp is written between draws wherever the part changes (`GpuTimer::part`), which needs `Features::TIMESTAMP_QUERY_INSIDE_PASSES` as well (asked for with the other; without it the passes are timed whole). To keep that to a few dozen stamps a pass, solid items are sorted by distance band and then by part (`SortKey::part`, `drawn_part` in `renderer/lighting.rs`): items, scattered items, then the terrain under them. A part drawn in several runs (once a band) is their sum; a frame needing more than 512 stamps gives no part times. `--bench` prints them as "GPU within the prepass, the scene pass and the shadows, by what is drawn" (`"gpu_parts"` in the JSON), with each sun cascade's time after them (`Shadows: cascade 1` to `4`, nearest first; a cascade kept from an earlier frame has none that frame) and each cascade's casters among the counts. `AXCENITY_NO_PART_TIMES` leaves the stamps out, to check they cost nothing.
- **The scene pass's depth test comes before lighting** (2026-11-26; numbers and method in [benchmarks/cross-engine/homestead/results.md](../../benchmarks/cross-engine/homestead/results.md), "The plants' scene pass, profiled"). A fragment shader that can `discard` and writes depth is run before its depth is tested, so `fs_main` lit every solid pixel, hidden or not (0.7 ms of Desert Homestead's plants alone). Two things changed that, with the picture the same pixel for pixel: a solid batch of the standard material with no cutoff and nothing fading is lit by `fs_prefilled`, which has no `discard` (`PrefillPipelines::whole`, `Batch::fading`; `AXCENITY_NO_WHOLE_SOLIDS` puts `fs_main` back); and the scene's depth starts one representable step behind the prepass's in place of the far plane (`renderer/depth_start.rs`, `depth_start.wgsl`), where both passes have one sample a pixel and the prepass is drawn (`RenderStats::depth_from_prepass`; `AXCENITY_NO_DEPTH_START`, or `set_depth_from_prepass(false)`, clears it as before). The step back keeps the test "nearer than", so of two surfaces at exactly one depth the first drawn still shows; testing for "equal" against the prepass's depth was faster and showed the last, which changed 0.1 to 1.2% of a still's pixels and was left out. The terrain's position is `@invariant` for it.
- **Draw order and pipelines.** A change of pipeline between two small draws costs the GPU more than the draws (about 0.6 µs each; 0.25 to 0.5 ms of that scene's scene pass). Solid draws are sorted by pipeline before mesh within a distance band and part (`pipeline_rank` in `renderer/lighting.rs`), the default since 2026-11-26. Any change of draw order changes which surface shows at an exact depth tie (64 to 4,623 single pixels of a 1920 × 1080 still there, in far foliage): the owner looked at the pictures and took it; `AXCENITY_PIPELINE_ORDER=0` sorts by mesh first as before, to compare. For the same reason, don't split or reorder solid batches without comparing stills pixel by pixel. The shadow views' casters are another matter (a shadow map keeps the nearest depth in any order): each view draws its whole casters, then its cut-outs.
- **The frame's thread, per frame and not per item** (2026-11-26, results.md, "Shadows through the holes, and the thread"). Three things found by timing inside a section, each with the picture unchanged: a terrain's layers (its texture arrays and uniform) were made ready once for each of its pieces every frame, 1,280 times in Desert Homestead, and are now once a terrain a frame (`TerrainPass::prepare`, Uploads 0.42 to 0.21 ms); the frame's distinct materials were merged through a map of the 480-byte keys themselves, which hashed and moved each key as it grew, and are now found by a digest worked out on the threads that work out the keys (`MaterialPlaces`, 0.23 to 0.06 ms for 1,479 materials); and the materials of draws in groups are worked out in parts on the renderer's threads and joined in order (`FrameUses::of`), so a material's place is still where the draws first come to it, which the draw order follows. On 2026-11-28 the same held for grouped items: Overgrown Street's frame joins 122,604 grouped draws from 4,714 clusters, and the join worked out each cluster material's digest again every frame (26,154 of them, one thread); a `Cluster` now keeps its materials' digests (`Cluster::digests`), and the posed items' map read every draw's mesh key before asking whether it had a skin. Materials 3.62 → 1.99 ms and After the frame 1.23 → 0.41 at 2560 × 1405, the frame's thread 11 to 12% shorter in four views, the picture the same pixel for pixel. When a section costs time with nothing to do, look for work done per item that belongs to something the items share.
- **Measuring switches**, each read when the renderer is made: `AXCENITY_PLANT_PROBE=flat|carried|surface` draws scattered items' scene-pass pixels with part of the work left out (`renderer/probe.rs`; the picture is wrong while it is on), `AXCENITY_NO_CUT_OUT_SHADOWS` casts cut-outs' whole cards in the shadow maps as before 2026-11-26 ([shadows.md](shadows.md)), `AXCENITY_NO_MOTION_SHARING` works out the motion vectors' instances on one thread for every draw as before (`History::moved` now shares them among threads and keeps only the camera's, `DrawPlan::in_view`). `RenderStats` also counts `prepass_draws`, `motion_draws`, `depth_fill_draws`, `scattered_batches` and `scattered_items`, which `--bench` prints with its other counts. Three more since the same day's fourth session, for pictures as much as time: `AXCENITY_CUT_OUT_CASCADES=0` to `4` (how many of the sun's nearest cascades cut-outs cast through their holes in) and `AXCENITY_SHADOW_REACH=reach,fade` (how far a painted plant's shadow reaches and how long it thins out for, in shares of its cull distance; `0.35,0` is the rule until then), both in [shadows.md](shadows.md), and `AXCENITY_TAA_PROBE` ([temporal.md](temporal.md)). `AXCENITY_SHADOW_REACH=0.35,0` with `AXCENITY_TAA_PROBE=soft` draws the six Homestead stills of the session before pixel for pixel.
- **The editor's Profiler** (`editor/src/profiler.rs`) shows these while playing, in its **Graphics** tab (graphics Phase 37): **Frame** (CPU frame time with the peak, the viewport's CPU time to draw, GPU frame time), **GPU passes** (each pass's average time, "off" for one no recent frame drew, or one "not available on this GPU" line without timestamps), **Draws** (draw calls, batches, triangles, vertices), **Culling** (items drawn of submitted, items culled, shadow casters drawn and culled, sprites culled, GPU particles, lights), **Levels of detail** (Full detail, Level 1, Level 2), **GPU memory** by kind with a total, and **Shaders** (surface shaders compiled and not compiling, and the last 20 compile errors, which also go to the Output window). Times are averaged over the last 120 frames; counts are the newest frame's `RenderStats` as they are. Shader messages are kept across Plays, since each is reported once. The Game tab adds the **1% low** over those frames, and the Graphics tab **Limited by** (GPU-bound, CPU-bound or sync-bound, `core::bench::Bound`, where sync-bound in the editor usually means its own UI or the display).
- **The Profiler's Memory tab** (roadmap G7; `editor/src/profiler.rs`, fed every 15 frames by `Session::memory`): **Process** (the editor process, the game's counted total, its peak this Play, and the GPU's share from `RenderStats`), a chart of the counted total over the last 240 samples (about a minute at 60 fps) with a line at the whole-game budget, **Budget** (each limit set, "used of limit", in the warning color when over), **Game, by kind** (Lua, Instances, Models, Textures, Sky and probes, Sounds, Physics; `core::memory::MemoryBreakdown`), **Since Play started** (each kind's change from the first sample and its peak) and the 12 heaviest **Instances by class**. History, peaks and changes start again at each Play and freeze at Stop. The budget is the project's (Settings > This project > Memory budget; `memory_budget` in `axcenity.json`, `project/src/memory_budget.rs`: `game_mb`, `lua_mb`, `textures_mb`, each optional, none by default, left out of the manifest when unset); the first time a Play goes over a limit, Output gets one warning naming it. The player doesn't check budgets.
- **The `fps` readout** (player and editor Play, [developer-console.md](../developer-console.md)): `fps` shows frame rate, frame time, 1% low, CPU and GPU times and the bound in one line; `fps full` adds each frame step and the drawing's numbers (`axcenity_ui::render_lines`). Its cost wasn't measured on its own: the readout is a few UI quads and up to a dozen lines of text, and GPU timing is a few empty compute passes and a small readback a frame, against frames of several milliseconds.

## Benchmarks

`player/src/bench_tests.rs` builds four scenes in code and draws each at 1280 × 720 on the High preset (MSAA 4×, three 2048² cascades, ambient occlusion High). They're ignored tests:

```text
cargo test -p axcenity-player --release -- --ignored bench_ --nocapture --test-threads 1
```

Each prints its last frame's counts (items drawn, batches, draw calls, triangles, and what was culled), then the times. Each warms up for at least a second (models load, caches fill, the GPU clocks up), then draws 5 batches of 60 frames, waiting for the GPU after each (untimed) so frames don't queue. Every number printed is the fastest batch's median, so a moment of other work on the machine doesn't count. CPU steps are `build_draw_list` (and, for the characters, `Animators::update`, which poses them) and `ScenePipeline::draw` (the CPU side of `TextureRenderer::render`: planning, uploads, recording and submitting).

| Benchmark | Scene |
|---|---|
| `bench_10000_parts` | 100 × 100 grid of blocks, balls, cylinders and wedges on a floor, drawn twice: an overview with all in view, and from one edge with most out of view (culling is measured against both) |
| `bench_2000_mesh_parts` | 2,000 copies of the `pbr` sample's textured helmet (about 15,500 triangles each, drawn in five views: three cascades, the prepass and the scene) |
| `bench_terrain_with_foliage` | A 2,048-stud terrain of 1,025² samples with hills, two layers and 20,000 plants (grass, flowers, bushes, trees) |
| `bench_20_animated_characters` | 20 copies of the `animation` sample's Noodle (about 200 triangles each), each with an Animator, posed every frame |
| `bench_20_walking_characters` | 20 copies of the `character` sample's walking character (about 76,000 triangles and 8 primitives each), each with an Animator, posed every frame (added in graphics Phase 14) |
| `bench_500_bullet_holes_in_the_graphics_reference_scene` | the `graphics` sample from (12, 8, 16), drawn without decals, then with 500 bullet holes spawned by `workspace:SpawnDecal` from a grid of raycasts (its cap raised to keep them all); fails if the GPU frame with them is over 16.7 ms (added in Phase 75, see [decals.md](decals.md)) |
| `bench_build_2000_mesh_parts` (in `scene/tests/bench_mesh_parts.rs`, no GPU) | The same 2,000 helmets, timing only `build_draw_list_posed` with a `TerrainCache` kept across frames, so it runs on machines with no graphics adapter: `cargo test -p axcenity-scene --release --test bench_mesh_parts -- --ignored --nocapture` (added with I8) |
| `bench_2000_mesh_parts_behind_a_wall` | The 2,000 helmets in view but hidden behind an 80-stud wall the camera stands 40 studs from, then the same view with no helmets (added in graphics Phase 15, for occlusion culling) |
| `bench_project` | A project of your own: set `AXCENITY_BENCH_PROJECT` to its folder. Drawn from its SpawnLocation at eye height looking ahead, then from an overview 120 studs up and 250 back. Skipped when the variable isn't set. Use it to find what a slow project costs before changing it or the engine (see Planning draws below) |

## Baseline

Taken at the commit that added the benchmarks, on an AMD Radeon RX 9070 XT (Vulkan, driver 26.10.1), Windows 11. Three runs agreed within 10% on every number (the widest was `build_draw_list` for the MeshParts, 8%; GPU times within 4%). Times are milliseconds. Absolute numbers depend on the machine; compare runs on one machine.

| | 10,000 parts, overview | 10,000 parts, edge | 2,000 MeshParts | Terrain + foliage | 20 characters |
|---|---|---|---|---|---|
| Items drawn / submitted | 10,001 / 10,001 | 10,001 / 10,001 | 2,001 / 2,001 | 37,691 / 37,691 | 41 / 41 |
| Batches | 4 | 4 | 2 | 1,027 | 41 |
| Draw calls | 20 | 20 | 10 | 5,135 | 205 |
| Triangles | 13.9 M | 13.9 M | 154.5 M | 53.7 M | 4,060 |
| CPU `Animators::update` | | | | | 0.05 |
| CPU `build_draw_list` | 3.28 | 3.33 | 0.64 | 6.48 | 0.01 |
| CPU `ScenePipeline::draw` | 2.44 | 2.43 | 0.92 | 12.14 | 0.28 |
| GPU frame | 1.92 | 2.31 | 9.29 | 5.99 | 0.43 |
| GPU shadows | 0.83 | 1.20 | 4.29 | 2.90 | 0.04 |
| GPU depth prepass | 0.27 | 0.26 | 1.24 | 0.95 | 0.02 |
| GPU ambient occlusion | 0.05 | 0.04 | 0.03 | 0.04 | 0.04 |
| GPU scene | 0.70 | 0.73 | 3.68 | 2.02 | 0.26 |
| GPU post-processing | 0.07 | 0.07 | 0.04 | 0.07 | 0.07 |
| GPU memory: textures / meshes / targets / buffers (MB) | 0 / 0 / 110.5 / 4.6 | same | 106.7 / 0.9 / 110.5 / 2.7 | 11.2 / 6.5 / 110.5 / 11.4 | 0 / 0.1 / 110.5 / 2.4 |

What it says, for the phases that follow:

- **Nothing is culled.** The edge view, with most parts out of view, draws every item and costs no less (its shadows cost more). Shadows are the largest GPU pass in every heavy scene (43% to 52% of the frame), since each cascade draws every caster. Frustum culling (Phase 12) should cut both.
- **Triangles dominate the MeshPart scene** (154.5 M a frame, 9.3 ms on the GPU, under 2 ms on the CPU): the case for LOD (Phase 13).
- **The terrain scene is CPU-bound**: 1,027 batches (mostly its 1,024 chunks, each its own mesh) drawn in five views make 5,135 draw calls, and `ScenePipeline::draw` takes 12 ms, twice the GPU's time. `build_draw_list` takes another 6.5 ms for its 37,691 items. Culling chunks and plants (Phase 12) and a foliage cull distance (Phase 13) should cut both.
- **CPU posing is not a cost** at 20 characters of this rig (0.05 ms), so GPU skinning (Phase 14) isn't called for by this benchmark. Each posed character is its own mesh and batch (41 batches for 41 items), which a heavier rig or many more characters would feel first.

## Frustum culling (graphics Phase 12)

Measured at the commit that added culling, against the commit before it, both on the same machine and session as above (AMD Radeon RX 9070 XT, Vulkan), three runs each, alternating. Each cell is the range of the three runs' numbers. The GPU's clocks differed from the baseline's day (the terrain's GPU frame took 9.1 ms before culling, not 6.0), so compare within this table.

| | 10,000 parts, overview | 10,000 parts, edge | 2,000 MeshParts | Terrain + foliage | 20 characters |
|---|---|---|---|---|---|
| Items drawn / submitted, before | 10,001 / 10,001 | 10,001 / 10,001 | 2,001 / 2,001 | 37,691 / 37,691 | 41 / 41 |
| Items drawn / submitted, after | 9,960 / 10,001 | 920 / 10,001 | 1,995 / 2,001 | 27,896 / 37,691 | 41 / 41 |
| Shadow casters drawn / culled (over all views), after | 7,555 / 22,448 | 3,618 / 26,385 | 2,060 / 3,943 | 2,040 / 111,033 | 123 / 0 |
| Draw calls, before → after | 20 → 14 | 20 → 20 | 10 → 9 | 5,135 → 1,682 | 205 → 205 |
| Triangles, before → after | 13.9 M → 7.6 M | 13.9 M → 1.5 M | 154.5 M → 93.4 M | 53.7 M → 16.6 M | 4,060 → 4,060 |
| GPU frame, before | 1.92–1.95 | 2.32–2.36 | 9.07–9.13 | 9.11–9.12 | 0.43 |
| GPU frame, after | 1.38–1.39 | 0.82 | 6.43–6.49 | 2.98–3.62 | 0.43 |
| GPU shadows, before → after | 0.84 → 0.29 | 1.22 → 0.19 | 4.19 → 1.52 | 3.98 → 0.24 | 0.04 → 0.04 |
| CPU `ScenePipeline::draw`, before | 2.38–2.55 | 2.38–2.55 | 0.95–0.97 | 12.47–13.55 | 0.27 |
| CPU `ScenePipeline::draw`, after | 2.97–3.46 | 2.20–2.52 | 0.83–0.90 | 12.96–13.22 | 0.27 |

What it says:

- **The edge view, with most parts out of view, now costs a third of the GPU time** (2.3 ms to 0.8 ms): 9,081 items are culled, and the cascades draw only the casters that reach them. Its CPU time is unchanged.
- **Shadows were the biggest win everywhere.** Each cascade used to draw every caster. Now it draws only those whose spheres reach its box, which already reaches 500 studs back toward the sun, so casters out of view that shade what's in view still draw.
- **The overview costs more CPU** (about 0.5 to 0.9 ms; `build_draw_list`, which this phase didn't change, also read about 10% higher in those runs, so part of it is noise). Every cascade copies its casters' instance data after the scene's (7,555 copies), and the instance upload grows from 4.6 to 6.9 MB. Planning was made cheaper to pay for most of it: each item's mesh and material keys are worked out once, and items are sorted as indexes instead of by recomputed keys. If a scene with many casters in every cascade shows this cost, drawing each cascade from the scene's instances where a whole batch is inside would avoid the copies.
- **The terrain scene is still CPU-bound** (13 ms in `ScenePipeline::draw`), even with a third of its draw calls. The time goes to planning and recording its 37,691 items. A foliage cull distance and LOD (Phase 13) are next.
- **Characters are unchanged**: all 41 items are in view.


## Levels of detail and foliage cull distance (graphics Phase 13)

Measured at the commit that added levels of detail, against the commit before it, on the same machine as above (AMD Radeon RX 9070 XT, Vulkan), three runs each, alternating. Each cell is the range of the three runs. Compare within this table.

| | 10,000 parts, overview | 10,000 parts, edge | 2,000 MeshParts | Terrain + foliage | 20 characters |
|---|---|---|---|---|---|
| Items drawn / submitted, before | 9,960 / 10,001 | 920 / 10,001 | 1,995 / 2,001 | 27,896 / 37,691 | 41 / 41 |
| Items drawn / submitted, after | 9,960 / 10,001 | 920 / 10,001 | 1,995 / 2,001 | 1,741 / 37,691 | 41 / 41 |
| Draw calls, before → after | 14 → 14 | 20 → 20 | 9 → 9 | 1,682 → 1,680 | 205 → 205 |
| Triangles, before → after | 7.6 M → 7.6 M | 1.5 M → 1.5 M | 93.4 M → 23.3 M | 16.6 M → 1.7 M | 4,060 → 4,060 |
| GPU frame, before | 1.39–1.41 | 0.82–0.83 | 6.42 | 2.97–4.47 | 0.43 |
| GPU frame, after | 1.45–1.48 | 0.84–0.85 | 4.39–4.49 | 1.28–1.29 | 0.43–0.44 |
| GPU scene, before → after | 0.71 → 0.75–0.77 | 0.46–0.47 → 0.47 | 3.61 → 2.45–2.50 | 1.73–3.04 → 0.91–0.92 | 0.26 → 0.26 |
| CPU `ScenePipeline::draw`, before | 3.01–3.74 | 2.20–2.51 | 0.86–0.94 | 13.66–14.64 | 0.27 |
| CPU `ScenePipeline::draw`, after | 3.15–3.66 | 2.37–2.81 | 0.95–0.96 | 3.66–3.75 | 0.28–0.29 |
| GPU meshes, before → after (MB) | 0 → 0 | 0 → 0 | 0.9 → 0.3 | 6.5 → 6.5 | 0.1 → 0.1 |

What it says:

- **The helmets draw a quarter of the triangles** (93.4 M to 23.3 M) and the GPU frame drops by a third (6.4 ms to 4.4 ms). Seen from 200 studs, every copy draws its second simpler copy (a quarter of the helmet's 15,500 triangles). Only the levels drawn are uploaded, so mesh memory drops too.
- **The terrain scene is no longer CPU-bound.** Plants past their cull distance aren't planned, uploaded or drawn: 35,950 of the 37,691 items are left out (the camera is 900 studs from the middle of a 2,048-stud terrain), so `ScenePipeline::draw` takes 3.7 ms, not 14, and the GPU frame 1.3 ms, not 3 to 4.5. `build_draw_list` (6.2 ms) still builds every plant; culling them in `scene` would save most of that.
- **Characters are unchanged**: the noodle's primitives are too small to simplify (under 256 triangles each).
- **Plain parts cost about 5% more in the scene pass** (0.71 ms to 0.76 ms for the overview): each instance is 4 bytes wider (the fade) and the fragment shader checks the dither. Nothing else changed for them.

## GPU skinning (graphics Phase 14)

The baseline's Noodle is too small to show what posing costs (0.05 ms). So this phase added `bench_20_walking_characters`, 20 of the `character` sample's walking characters (about 76,000 triangles each), and measured that first. Posing on the CPU was a real cost there: each frame, `ScenePipeline::draw` skinned every posed primitive (`Posed::primitive` runs when the renderer first reads the corners), checked its 4.5 million indices, rebuilt its corners and wrote 13 MB of them to the GPU, about 5 ms of a CPU frame whose GPU work took 0.8 ms. Every posed part was also its own mesh and batch.

Measured at the commit that skins on the GPU, against the commit before it with the new benchmark, on the same machine as above (AMD Radeon RX 9070 XT, Vulkan), three runs each. Each cell is the range of the three runs. Compare within this table.

| | 10,000 parts, overview | 10,000 parts, edge | 2,000 MeshParts | Terrain + foliage | 20 characters (Noodle) | 20 walking characters |
|---|---|---|---|---|---|---|
| Batches, before → after | 4 → 4 | 4 → 4 | 2 → 2 | unchanged | 41 → 3 | 149 → 22 |
| Draw calls, before → after | 14 → 14 | 20 → 20 | 9 → 9 | unchanged | 205 → 15 | 731 → 103 |
| CPU `ScenePipeline::draw`, before | 3.18–3.43 | 2.35–2.40 | 0.94–0.96 | 3.87–3.95 | 0.29 | 4.85 |
| CPU `ScenePipeline::draw`, after | 3.22–3.34 | 2.41–2.45 | 1.01–1.02 | 4.00–4.10 | 0.18 | 0.37–0.38 |
| GPU frame, before | 1.45 | 0.84 | 4.40–4.42 | 1.28–1.29 | 0.43–0.44 | 0.79–0.86 |
| GPU frame, after | 1.46–1.47 | 0.84–0.85 | 4.53–4.56 | 1.28–1.29 | 0.43–0.44 | 0.80–0.81 |
| GPU scene, before → after | 0.76 → 0.77 | 0.47 → 0.47 | 2.46 → 2.57–2.60 | 0.91 → 0.91 | 0.26 → 0.26 | 0.48–0.50 → 0.47–0.48 |
| GPU meshes, before → after (MB) | 0 → 0 | 0 → 0 | 0.3 → 0.3 | 6.5 → 6.5 | 0.1 → 0.1 | 13.0 → 2.3 |

What it says:

- **The walking characters cost a thirteenth of the CPU time** (4.85 ms to 0.38 ms in `ScenePipeline::draw`): nothing is skinned, checked or uploaded per frame but the joints (112 bytes each). The 20 characters share one copy of each primitive, so 149 batches become 22 and mesh memory drops from 13 MB to 2.3 MB. The GPU does the blending at no measurable cost.
- **Vertex-heavy scenes pay a little for it.** Every instance is 8 bytes wider (its skin) and every vertex checks it, so the helmets' scene pass, with 23 M triangles, takes 5% longer (2.46 ms to 2.59 ms) and their CPU time 6% (the wider instances are copied for every shadow view). Plain parts and terrain are within noise. If that matters, skinned items can get pipelines of their own, so other items skip the check.

## Texture budget and occlusion culling (graphics Phase 15)

**The texture memory budget** (see [shadows.md](shadows.md)) costs nothing while textures fit: each frame checks the total once. None of the benchmarks passes even the Low preset's 256 MB (the helmets take 106.7 MB), so their numbers are unchanged. Over the budget, a texture shrinks by a GPU copy of its smaller levels, once, and only grows again (an upload) when it fits.

**Occlusion culling was not built.** The plan allowed it only if a benchmark showed overdraw-heavy scenes that frustum culling and LOD don't fix. `bench_2000_mesh_parts_behind_a_wall` measures the most it could save: everything hidden items cost, as the hidden view minus the empty one. Three runs on the machine above (AMD Radeon RX 9070 XT, Vulkan) agreed within 0.1 ms; times are milliseconds.

| | Helmets hidden behind the wall | No helmets | Hidden items cost |
|---|---|---|---|
| Items drawn / submitted | 1,840 / 2,002 | 2 / 2 | |
| Triangles | 23.2 M | 120 | |
| CPU `build_draw_list` | 0.69–0.81 | 0.00 | 0.7–0.8 |
| CPU `ScenePipeline::draw` | 1.08–1.13 | 0.16–0.17 | 0.9 |
| GPU frame | 3.52–3.53 | 0.37–0.39 | 3.1 |
| GPU shadows | 1.29 | 0.03 | 1.26 |
| GPU depth prepass | 0.64–0.65 | 0.02 | 0.63 |
| GPU scene | 1.46 | 0.24–0.26 | 1.2 |

What it says:

- **The hidden helmets aren't overdraw.** The wall is drawn first (built-in shapes sort before meshes), so their pixels fail the depth test before shading. What they cost is vertex work for 23 M triangles that LOD has already cut to a quarter, and CPU planning.
- **Culling them by a Hi-Z pyramid would save at most 1.9 ms of GPU** (the prepass and scene), not the 1.3 ms of shadows: casters behind the wall still shade what the sun's cascades cover, and camera occlusion doesn't cull shadow casters. That is from a scene built to be its best case, 2,000 15,500-triangle models packed behind one wall, and its frame still takes 3.5 ms at 1280 × 720 on High. The other benchmarks have no large occluders, so it would save nothing there.
- **It would cost more than it saves today.** The prepass exists only with ambient occlusion on (Off at Low), so Low, the preset for weak GPUs, would need a depth pass of its own. Testing last frame's bounds on the CPU needs a readback a frame or two old, which culls an item for those frames as it comes out from behind something: the popping the criterion rules out, unless items are drawn GPU-driven (compute culling and indirect draws), a rewrite of `plan_draws` and its batching.
- **When to reconsider:** a real scene (an interior, a city) whose GPU scene and prepass time is mostly hidden items, measured the same way (with and without what's hidden). Cheaper fixes come first: a cull distance on dense models (as foliage has, `DrawItem::cull_distance`) and coarser LODs.

## GPU particles (graphics Phase 36)

`bench_particles` draws one emitter keeping 1,000 to 4,000 particles alive (`Rate × Lifetime`, Lifetime 2, a cloud in front of the camera), moved and sorted on the CPU and then on the GPU, then 100,000 on the GPU. CPU steps are `Particles::update`, `build_draw_list`, `Particles::draw` and `ScenePipeline::draw`. Measured on the machine above (AMD Radeon RX 9070 XT, Vulkan). The 2,000-on-the-GPU row was taken with the threshold lowered to 2,000. Times are milliseconds.

| Particles out | 2,000 CPU | 2,000 GPU | 3,000 CPU | 3,000 GPU | 4,000 CPU | 4,000 GPU | 100,000 GPU |
|---|---|---|---|---|---|---|---|
| CPU, all steps | 0.39 | 0.35 | 0.46 | 0.26–0.29 | 0.60 | 0.34 | 0.43 |
| GPU frame | 0.50 | 0.62 | 0.53 | 0.56–0.62 | 0.55 | 0.68 | 1.81 |
| GPU particles pass | | 0.12 | | 0.10–0.11 | | 0.13 | 0.23 |
| GPU scene | 0.32 | 0.32 | 0.34 | 0.30–0.33 | 0.37 | 0.36 | 1.48 |
| CPU + GPU | 0.89 | 0.97 | 0.99 | 0.82–0.92 | 1.15 | 1.02 | 2.24 |

What it says:

- **On the CPU, particles cost about 0.1 ms of CPU time per 1,000**, mostly building, sorting and uploading their sprites in `ScenePipeline::draw`. On the GPU they cost a nearly fixed 0.10 to 0.13 ms (the sort's dispatches, whatever the count up to its 4,096 slots) and almost no CPU time.
- **The GPU wins from about 3,000 particles**, so `scene::GPU_PARTICLE_THRESHOLD` is 3,000. At 2,000 the GPU path costs more GPU time than it saves on the CPU. Weaker GPUs pay more for the fixed cost, and weaker CPUs more per particle. The CPU's cap is 4,000 per emitter, so an emitter needing more than that has to be on the GPU anyway.
- **100,000 particles take 0.23 ms to move and sort** (131,072 slots), and 1.5 ms to draw: filling overlapping squares, not moving them, is the cost there. That's the same per pixel as the CPU path would pay.
- `RenderStats::gpu_particles` counts the slots kept, and their buffers (104 bytes a slot since R17.4b; 88 before) count in the buffer bytes.

## Where the frame goes (`profile_project`)

`profile_project` (`player/src/profile_tests.rs`) measures a project from named cameras by leaving one cost out at a time and timing the frame again. Each variant changes only that run's draw list or graphics settings:

```text
AXCENITY_BENCH_PROJECT=<absolute folder> AXCENITY_SHOT_VIEWS='walker:-39.6,5.5,39.6:22,14,12' \
  AXCENITY_SHOT_SIZE=2560x1440 AXCENITY_SHOT_QUALITY=Ultra AXCENITY_SHOT_DIR=<folder for PNGs> \
  cargo test -p axcenity-player --release -- --ignored profile_project --nocapture --test-threads 1
```

| Variant | Leaves out |
|---|---|
| `no-shadows` | every shadow map (and the scene pass's shadow lookups) |
| `no-foliage-shadows` | shadows of painted foliage (items with a cull distance) |
| `no-wind`, `no-sway` | the wind (nothing bends), or every item's sway (the vertex shader skips it) |
| `no-cutout` | alpha testing: cut-out materials drawn solid (a different picture; it shows what the depth fill saves, not a saving to take) |
| `single-sided` | back faces of double-sided materials |
| `no-foliage` | painted foliage altogether |
| `no-large-meshes` | meshes of more than 100,000 triangles |
| `no-msaa`, `no-ao` | anti-aliasing, ambient occlusion |

Each variant's list is built once and drawn again and again with three frames in flight, so the GPU is never idle between frames: drawing a new list each frame left the GPU waiting on the CPU, it clocked down, and its timestamps read up to 70% slower (a first version of the tool had removals "costing" 3 to 4 ms). Variants are measured in three rounds through all of them, and each cell is the median of the rounds' medians. `AXCENITY_PROFILE_VARIANTS` picks some. After the table it reads the overdraw view back (`OverdrawStats`, below), and `AXCENITY_SHOT_DIR` saves a PNG of every variant to compare by eye.

`lod_report_project`, beside it, lists every model a project's MeshParts show or its terrains have painted with its levels of detail and their warnings (`assets::LodReport`), biggest first.

## Overdraw, as numbers (`OverdrawStats`)

`render::OverdrawStats::from_pixels` reads the overdraw view's picture back into numbers: layers per pixel over the picture and over covered pixels, the most on one pixel, the share of pixels in bands (none, 1, 2–3, 4–7, 8–15, 16–31, 32 and up, `OVERDRAW_BANDS`) and the busiest cells of an 8 × 6 grid over the screen. Each layer adds 0.02 to blue in linear light, so counts read back exactly up to `MAX_LAYERS` (50) from an 8-bit sRGB picture; 50 means "50 or more" (reported as `50+`).

**What it counts is depth complexity, not shading work.** The view draws every batch again with no depth test, back faces culled as the scene culls them, cut-out holes and dithered fades left out. So a pixel's count is how many surfaces the camera looks through there. The lit scene doesn't shade each of them: cut-out materials draw their depth first and are then lit once per pixel (`PrefillPipelines`), and an opaque surface drawn in front stops what's behind before shading (distance bands sort near first). The count is the *input* to the depth tests: the cost it predicts is vertex work and the cheap depth fill (`fs_depth_only` samples one alpha texel per layer), not full lighting per layer. Shadow maps, the prepass, MSAA samples, terrain's own passes, sprites and the sky aren't counted, and see-through items count as solid. The editor's overdraw view is the same picture; its brightest orange is 8 or more layers (red is full at 8.3 layers, green at 20, blue at 50).

## Debug views (graphics Phase 38)

Viewport > View Mode in the editor (`editor/src/viewport/view_mode.rs`) shows the viewport **Lit** (the game's frame), as one of the renderer's debug views, or as **Culling Bounds**: the lit frame with the culling overlay on (culling held where the camera was when it was picked, each item's bounds green where drawn and red where culled). The mode isn't saved between runs. The debug views (`render/src/renderer/debug_views.rs`, `debug_views.wgsl`, `TextureRenderer::set_debug_view`):

- **Unlit**: each surface's base color plus its glow. **Base Color**: the base color alone, its color map included. Both are as the color is written (sRGB), with no light, shadow, fog or post-processing.
- **Wireframe**: each triangle's edges, one pixel wide, over its surface dimmed. The edges come from each corner's place in its triangle, worked out in the shader (line drawing isn't on every backend).
- **Normals**: the way each surface faces in the world, normal maps included, x, y and z as red, green and blue from -1 (0) to 1 (255).
- **Metalness**, **Roughness**: the value, maps included, as grey from 0 (black) to 1 (white).
- **Ambient Occlusion**: the occlusion map times screen-space ambient occlusion (solid items only, and only while the Ambient Occlusion graphics setting is on), white for open.
- **Overdraw**: every surface drawn over a pixel adds the same orange light, hidden ones included, with back faces culled as the scene culls them.
- **LOD Levels**: green for an item drawn with its own mesh, then yellow, orange and red for its simpler copies (graphics Phase 13); grey for a mesh without levels.
- **Ray Hits**: what the camera's rays hit in hardware ray tracing's scene, each item its own color; plain dark on a GPU that can't ray trace ([ray-tracing.md](ray-tracing.md)). Unlike the others it traces rays rather than drawing batches again.

A debug view draws after the frame's own passes, over the finished picture and under the UI, at the output's size, with its own depth target and the camera's view (without TAA's jitter). Each batch is drawn again with its material, posed and swayed as in the scene, reading its corners from the mesh's buffers ("vertex pulling"). See-through items draw as solid. Terrain chunks show their shape and normals with the plain white material, and a material's own surface shader is left out: its maps and values show as the standard material reads them. Sprites and the sky aren't drawn.

The debug views are built only with `render`'s `debug-views` feature, which the editor turns on, and the player too since Phase 71 for its Developer Console's `wireframe` (`Renderer::set_debug_view`). With the feature, mesh buffers can also be read as storage buffers. While the lit view is shown, nothing is kept for the debug views: their pipelines, depth target and uniforms are freed when the mode goes back to Lit.

## Decals (Phase 75)

`bench_500_bullet_holes_in_the_graphics_reference_scene`, on the machine above (AMD Radeon RX 9070 XT, Vulkan), at 1280 × 720 on High. the `graphics` sample already keeps the prepass at High (ambient occlusion), so the holes add only their own pass. Times are milliseconds.

| | Without decals | 500 bullet holes |
|---|---|---|
| Decals drawn | 0 | 500 (one draw call) |
| GPU frame | 0.640 | 0.734 |
| GPU scene pass (decals are drawn in it) | 0.416 | 0.515 |
| GPU depth prepass | 0.028 | 0.028 |

So 500 projected decals cost about 0.1 ms of GPU time, well inside a 60 frames a second budget (16.7 ms). Games keep at most `MaxSpawnedDecals` (200 by default) from `workspace:SpawnDecal`, and the quality preset draws at most 512 at High.

## A dense painted terrain (`bench_project`)

The first project measured with `bench_project` was a 960 × 1,260-stud terrain with 8,339 models painted as foliage (grass, ferns, pebbles, bushes and 10-stud clumps of trees from a model pack, 20 to 550 triangles each) and 600 MeshParts (trees, boulders and a store), on the machine above (AMD Radeon RX 9070 XT, Vulkan), release build, 1280 × 720 on High. Times are milliseconds; the GPU frame was 2.8 to 3.0 ms throughout, so every number that mattered was CPU time.

| | At the spawn, before | At the spawn, after | Overview, before | Overview, after |
|---|---|---|---|---|
| Items submitted / drawn | 22,368 / 11,897 | 16,035 / 10,339 | 22,368 / 19,591 | 12,091 / 10,708 |
| CPU `build_draw_list` | 7.7 | 5.7 | 7.7 | 4.3 |
| CPU `ScenePipeline::draw` | 19.5 | 12.5 | 19.3 | 8.0 |
| GPU frame | 2.9 | 2.9 | 3.0 | 2.8 |

What it said, and what changed:

- **Painted models had no cull distance.** Built-in plants are left out past a distance by their kind, but a model painted from the Assets panel was drawn everywhere, so 8,339 of them became 17,600 draw items a frame across the whole terrain, ferns a third of a stud tall included. Now a painted model's cull distance comes from its size as placed, 120 studs per stud of its largest extent from 60 to 1,500 (`scene::foliage::model_cull_distance`, the built-in plants' rate), and `scene` skips a plant or model past it before building its items, looking each model up once per frame rather than once per instance (`docs/terrain.md`).
- **Planning cost 11 to 15 ms of the 19.5**, timed inside `plan_draws`: 2 to 3 ms working out each draw's `MaterialKey` (330 bytes of texture keys and map settings), 2 to 5 ms sorting 22,000 indexes by comparing those keys and re-deriving each item's shader key per comparison, and 6 to 7 ms in the loop, where `instance_raw` converted six sRGB channels with `powf` per item. `upload_new` worked out every `MaterialKey` a second time. Now `ScenePipeline::draw` works the keys out once per draw and hands them to both; `plan_draws` sorts by a small `SortKey` (mesh, the material's index among the frame's distinct keys, with the plain material at index 0 without a lookup, shader, sidedness, shadow) and batches by it; and `LinearColors` keeps the last base and glow colors converted, since a batch's items share them.
- **What's left is the count.** 16,000 items still reach the renderer from the spawn, which stands in a forest: clumps of trees are drawn to 1,200 studs, as their size says, and each is three to nine draw items. The next savings there are fewer primitives per painted model (one material per clump) and coarser levels of detail, which are the project's to make, or combining a terrain's painted instances of one model into one draw item each (the renderer already batches them into one instanced draw).
- **The texture budget was full for another reason.** The project's `realistic/` models embed 2048² textures (21) and three 4096² ones, 540 MB with mips of the 1,012 MB on the GPU, so High's 1,024 MB budget had shrunk one. Nothing in the frame times came from that (the budget check costs 0.2 ms and nothing was replaced), but it's the first thing to downsize in such a pack.
- **Debug builds pay more.** `scene`, `physics` and `terrain` are among the crates `[profile.dev.package]` optimizes, since the debug editor spends each frame in them (`build_draw_list` 13.4 ms unoptimized, 6.8 ms optimized, on this project). The engine's other crates stay unoptimized, so measure in release before blaming a project.

### Play and the selection box on that terrain (`bench_project_play`)

Selecting something made the editor crawl and Play was a slideshow, while neither the CPU nor the GPU looked busy: the work was on one thread (1 of 32, about 3% of the CPU in Task Manager) and the GPU had under 3 ms of work a frame. `bench_project_play` in `editor/src/play/tests.rs` times the game frame without drawing, and the selection box, on the project `AXCENITY_BENCH_PROJECT` names:

```text
cargo test -p axcenity-editor --release -- --ignored bench_project_play --nocapture --test-threads 1
```

| Release build | Before | After |
|---|---|---|
| `Session::frame`, walking (median) | 666 ms | 2.0 ms |
| Selection box, per selected part | 17 ms | 0.1 to 0.3 ms |

In a debug build the same frame is 8.5 ms and the box 0.5 to 1.4 ms.

- **Every physics query built every collider.** `candidates` (`physics/src/query.rs`) made a collider for each part in the Workspace per ray: a convex hull per MeshPart, and a copy of all 345,249 height samples for the terrain. The walking character casts many rays a frame. Now a `Reach` (the capsule the query can touch) passes by parts whose bounding ball is outside it before anything is built, and a Terrain's colliders are kept between queries by its `uid` and `revision` (`terrain_colliders`; a copy of a terrain has its own `uid`, and a reloaded model rebuilds them).
- **The selection box walked the world per ray.** Up to 12 edges of 17 rays each, every ray listing the Workspace twice and marching across the whole terrain. Now `scene::Occluders` gathers the parts and terrains once a frame, each edge tests only the parts near the line of sight to it, and terrain rays stop at the edge (`terrain::raycast::raycast_within`).

## A painted forest (`shoot_project`)

A 600-stud forest with 30,296 painted models (a project called Abandoned Farmhouse, all models in `.glb` packs with 4K embedded textures) was a slideshow: each of its 36,000 items was listed again and walked through the renderer every frame though only 5,700 were on screen. Pictures and timings come from `shoot_project` (`player/src/bench_tests.rs`), which draws named cameras and saves a PNG of each; `bake_project_probes`, beside it, bakes a project's light probe volumes without the editor:

```text
AXCENITY_BENCH_PROJECT=<folder> AXCENITY_SHOT_DIR=<folder> AXCENITY_SHOT_VIEWS='front:2.7,4.3,63:-0.6,11.5,0' \
  cargo test -p axcenity-player --release -- --ignored shoot_project --nocapture --test-threads 1
```

`AXCENITY_SHOT_SIZE` and `AXCENITY_SHOT_QUALITY` choose the frame, and `AXCENITY_SHOT_DEBUG` (`ao`, `base`, `normals`, `overdraw`) saves a debug view.

| 687 × 1024, High, release | Before | After |
|---|---|---|
| CPU `build_draw_list` | 12.2 ms | 1.2 ms |
| CPU `ScenePipeline::draw` | 29.8 ms | 13.3 ms |
| GPU frame (translucent leaves and probe light cost about 2.6 ms; the bands below save more at larger sizes) | 6.6 ms | 7.4 ms |

At 2560 × 1440 on Ultra, the GPU's frame was 40.1 ms before distance bands and is 29.0 ms now, and the CPU is 36 ms (it draws twice the items, and a fourth shadow cascade).

- **`DrawList::chunks` (`ItemChunk`)**: `scene` builds a terrain's foliage once (`foliage::PaintedFoliage`), in 48-stud squares with short-reach plants apart from trees, and hands the list `Arc`s of them. It's rebuilt only when the terrain or a painted model changes.
- **`Reach` (`renderer/culling.rs`)**: every view the frame draws (the camera, the sun's cascades and the lamps' shadow views). `LodStates::resolve` passes over a group that touches none of them, or that's past its farthest cull distance, before any per-item work; a kept group's items are checked one by one. A frozen view keeps everything.
- **`upload_new`** looks up each mesh and each set of textures once per frame, by address, rather than once per draw.
- **Real-time GI in the editor.** The editor was at 2 frames a second with Ultra and `realtime_gi` on: 434 of each 474 ms went to tracing probes (12 whole draws of a 36,000-item scene each frame, on one CPU thread, at 16² so the GPU had little to do). `light_state` saw the sky picture finish loading as the light changing, so all 1,472 probes were traced after every open, about six minutes. See Real-time GI in [sky.md](sky.md) for what a heavy scene does now.
- **Distance bands (`SortKey::band`)**: solid items are drawn nearest first in about 2 bands per doubling of the distance, then by mesh and material. Looking along a forest, every pixel had 8 or more layers of leaves, each shaded; a band drawn first hides what's behind it, by depth, before the shader runs. It's a few more batches for about a quarter less GPU time there.

## Light probes (`bench_light_probes`, roadmap R18)

`bench_light_probes` in `render/src/renderer/tests/realtime_gi.rs` measures what light probes cost ([global-illumination.md](../research/global-illumination.md), D5):

```
cargo test -p axcenity-render --release --lib -- --ignored bench_light_probes --nocapture
```

In a yard with 576 blocks at 1920 × 1080 it prints a bake per probe (16² faces, then with the sky's share kept apart, R18.3), a frame with 8 volumes of 16³ probes against none, and each preset's real-time GI tier (R18.4) with the sun moving every frame so it never rests: the frame, what tracing added and that per probe. Times are the median wall clock of a frame waited on. The cloud container only proves it runs (a software GPU in a debug build: a bake about 120 ms a probe, 145 with the sky apart). The RX 9070 XT's numbers go here when it's run on the PC.

## Sharing the frame's planning among the CPU's threads

That forest ran at 26 frames a second at 2560 × 1440 on Ultra: 40.8 ms of CPU on one thread of 32, and 31 ms of GPU. Measured with `shoot_project` from the same camera, on the machine above (AMD Radeon RX 9070 XT, Vulkan), release build. The picture is the same to within noise: 176 of 3.7 million pixels differ by more than 32 of 255. Times are milliseconds.

| 2560 × 1440, Ultra | Before | After |
|---|---|---|
| CPU `build_draw_list` | 1.2 | 1.3 |
| CPU `ScenePipeline::draw` | 39.6 | 9.5 to 10.0 |
| GPU frame | 31.0 | 13.6 to 13.9 |
| GPU shadows | 7.5 | 5.6 to 5.7 |
| GPU scene | 22.4 | 7.0 to 7.7 |

Inside `ScenePipeline::draw`, before: `LodStates::resolve` 5.2, material keys 3.4, `upload_new` 8.4, `plan_draws` 12.9, the shadow views' casters 4.1. Recording every draw took only 1.2, so it isn't threaded.

- **Threads (`renderer/parallel.rs`, rayon).** Levels of detail, material keys, sort keys, the sort, instance data and each shadow view's casters are worked out in shares of 512 items and joined in order, so a frame is the same however it was shared (`planning_comes_out_the_same_however_many_threads_share_it`). A terrain's groups outside every view are passed over before anything is shared. Each LOD state has its own lock, and states of items new this frame are added afterwards. Joining doesn't copy: `DrawPlan::instances` and `casters` stay in pieces, and `write_pieces` copies them straight into wgpu's staging memory. Under `WORTH_SHARING` (4,096) items a job stays on the calling thread: waking the others takes about 0.1 ms, which made small scenes slower. Past 16 threads there's no gain; the work now runs on the renderer's own pool of at most 12 (see The renderer's own threads, below).
- **Work per distinct thing, not per draw (`FrameUses`).** Hashing a 330-byte `MaterialKey` for every draw, twice, was most of `upload_new` and much of `plan_draws`. Now each material and mesh is looked up once. `MeshSource::lod` hands out a kept copy instead of allocating one per draw per frame.
- **Depth fill for cut-out surfaces (`PrefillPipelines`).** A shader with `discard` can't have the GPU skip what's hidden, so every leaf in front of a pixel was fully lit, then covered. Solid standard-material batches with an `alpha_cutoff` now draw their depth first in the scene pass (`fs_depth_only`, which only reads alpha), then are lit with no `discard` where depth is equal (`fs_prefilled`), so each pixel is lit once. Surface shaders and terrain are drawn as before, since they decide their own holes. `fs_depth_only` must cut out exactly what `standard_surface` does.
- **Cut-outs need no depth fill where the depth starts from the prepass's (`depth_start.wgsl`, 2026-11-28).** The prepass marks the standard material's cut-out pixels (`fs_prepass_standard`: w 0.85, a character's 0.625; a surface shader's keep 0.875, as they're lit with a depth test of their own), and the scene pass's depth start keeps those pixels' depth exactly instead of a step farther. That is the depth the cut-outs' fill used to draw there, so the fill is left out and cut-outs are lit, as before, only where their depth equals what's there (`DepthUse::Filled`, early depth test, no discard). `AXCENITY_CUT_OUT_FILL=1` draws the fill anyway, for comparing. Overgrown Street, `owner-ultra`, 2560 × 1405, pinned, three alternating runs a view against origin/main 1f083288: 37.1 → 40.5 FPS at the spawn, 37.4 → 40.8 (houseA), 37.0 → 39.9 (canopy), 27.6 → 31.1 (high); the scene pass 8.59 → 6.17 ms (spawn) and 10.60 → 6.52 (high), all of it the 2.0–4.1 ms fill, the plants' lighting unchanged. The four stills are the same pixels, and so is the thicket of `tests/depth_from_prepass.rs` (cards, hair, a surface shader with its own holes, twins, a fading plant). Under MSAA, or with no prepass, the scene pass clears its depth and fills the cut-outs as before.
- **Far cascades take turns (`ScenePipeline::schedule_cascades`).** Past `HEAVY_SHADOW_CASTERS` (12,000) casters planned, the two farthest cascades are drawn on alternate frames, keeping the matrix they were drawn with. They held 11.3 of the 14.3 million shadow triangles. A cascade is drawn anyway once the camera has moved 256 of its texels from where it was kept. With four cascades (Ultra) the farthest is drawn only one frame in four (`FARTHEST_CASCADE_TURN`, `cascade_turn`, 2026-11-28), on a frame its neighbour isn't, so no frame draws both: in Overgrown Street it was about 8 ms on the frames that drew it, 4 ms a frame on average, and covers only what is well past the middle distance, where four frames of a swaying plant's shadow don't show. `AXCENITY_FAR_CASCADE_EVERY_OTHER=1` puts it back on every other frame, for comparing: at 2560 × 1405 (`owner-ultra`, four views) cascade 4 took 4.0–4.4 ms a frame that way and 1.9–2.1 ms one frame in four, the Shadows pass 2.0–2.5 ms less and the frame 5–8% faster. Measured at the same time and left out (2026-11-28): lighting cut-outs over the started depth with no depth fill removed "cut-outs' depth" (2.0–4.1 ms) but made the scene pass dearer by more on RADV (spawn 8.6 → 11.1 ms, high 10.6 → 16.5), 5–14% slower in all; and thinning cut-out foliage by whole leaf cards changed nothing there, since every tree, plant and ivy model ships its own `_LOD1` copies (0 pixels differed).
- **Light through a leaf (`soft_sun_visibility`)** takes four filtered shadow samples, not the full 32-sample soft shadow it was computing a second time per leaf pixel (0.6 ms).

Against the commit before, small scenes are unchanged or better (10,000 parts: 3.4 to 4.0 down to 2.9; 2,000 helmets: 1.8 down to 1.2; terrain and foliage: 3.1 to 3.7 down to 2.8; two items: 0.26 both). Filtering small casters out of the far cascades was tried and removed: it left out 2 of 21,000.

**What's left is GPU work**: at Ultra, 5.7 ms of shadow vertices and 7 ms of lighting at 3.7 million pixels. High at 2560 × 1440 is 9.4 ms of GPU. Each shadow view copies its casters' 176-byte instances, of which a shadow pass reads 88, so a slimmer instance would halve that upload.

## The renderer's own threads

A frame's planning (`renderer/parallel.rs`: levels of detail, material and sort keys, the sort, instance data, each shadow view's casters) runs on the renderer's own rayon pool of at most 12 threads (`MAX_THREADS`; `AXCENITY_RENDER_THREADS` sets the number, 0 uses rayon's global pool as before). rayon's global pool has a thread per logical CPU: 32 on the 9950X3D desktop, and on Windows that's so whatever the process's affinity. Spread over 32 threads, one frame's few milliseconds of planning cost more in waking and waiting than they saved. Cooking textures (the `cook` crate, the editor's `--cook`) and building terrain texture arrays (`renderer/terrain.rs`) are long batches that do use every thread, so they stay on the global pool; the editor's background cooker has its own pool of half the cores already (`editor/src/cooking.rs`), and physics has its own (`physics/src/threads.rs`). A job called from a thread of some pool already stays there, so `planning_comes_out_the_same_however_many_threads_share_it` still compares against one thread.

Measured on that desktop with the player's `--bench`, the same release build, `AXCENITY_RENDER_THREADS=0` (the global pool) against the default, three alternating rounds each; medians of the rounds' medians, ranges in brackets, milliseconds:

| Scene | Unpinned, Render step: global → own | Unpinned frame | Pinned to the V-cache CCD, Render step | Pinned frame |
|---|---|---|---|---|
| S10k | 2.60 [2.46–2.65] → **2.14** [2.12–2.14] | 9.43 → **8.73** | 1.77 → 1.79 | 6.28 → 6.26 |
| General Store Roadside | 4.38 [4.37–4.48] → **3.85** [3.83–3.94] | 5.55 → **5.00** | 3.34 → 3.17 | 4.23 → 4.05 |
| Abandoned Farmhouse | 4.24 [4.20–4.31] → **3.73** [3.72–3.81] | 5.33 → 5.30 | 3.05 → 3.01 | 5.39 → 5.39 |
| Temperate Dry Patch, Ultra 2560 × 1440 | 6.73 [6.70–6.88] → **6.32** [6.25–6.34] | 11.17 → 11.12 | 4.91 → 4.78 | 11.16 → 11.22 |

- **Unpinned, as a game runs, every own-pool run's Render step beat every global-pool run's**: 6% to 18% less CPU time drawing. Frames that are CPU-bound get that much faster (S10k −0.7 ms, General Store −0.55 ms); GPU-bound ones (the farmhouse, the patch) keep their frame time and gain CPU headroom.
- **Pinned to 16 logical CPUs it changes little** (within noise but for General Store, −0.17 ms): the global pool's 32 threads then share 16 CPUs, which hides part of the cost. Unpinned is how players run, so that's the column that counts; pinned benchmarks (the regression check) aren't made worse.
- Unpinned frames are much slower than pinned ones on this CPU (S10k 8.7 against 6.3 ms): the scheduler moves the frame's threads between the two CCDs. That's a separate question (whether a game should prefer the V-cache CCD), not taken up here.

## MeshParts share their meshes between frames (I8)

Each MeshPart used to make a new mesh for each primitive of its model every frame, with new textures for the file's material and (since each mesh kept its own) new simpler copies for its levels of detail. 2,000 copies of one helmet handed the renderer 2,000 meshes, which it told apart by address. Now `scene/src/mesh_cache.rs` keeps, in the host's `TerrainCache` (`TerrainCache::meshes`):

- **Each model's pieces, by `MeshId`**: one mesh and one file material per primitive, made once and shared by every MeshPart showing that model. The assets are asked for the model once a frame per `MeshId`; when they hand back a different one (`Assets::reload` swapped it in), its pieces are made again. A model nothing drew this frame is let go.
- **Each animated MeshPart's `Skin`**, kept while its `Posed::version` is the same. A new pose is copied into the same buffers when nothing else holds them (`Skin::set_pose`), using the normal matrices the pose already has (`Posed::normal_matrices`) instead of inverting every joint again.

`build_draw_list` (no cache passed) still makes them each call. `bench_build_2000_mesh_parts` on the cloud container (4 vCPU Xeon at 2.1 GHz, release build, fastest batch's median of 5 × 60 frames, four runs each):

| | Before | After |
|---|---|---|
| CPU `build_draw_list`, 2,000 helmets | 1.84 to 1.90 | 0.90 to 0.99 |

The player's `bench_2000_mesh_parts` keeps a `TerrainCache` across frames too now, as the hosts do; it wasn't run for this change (the container has only a software GPU).

## Retained draw items (M4)

Every frame used to walk the Workspace and work out every part's `DrawItem` from its properties, so the draw list cost grew with the world, not with what changed. Now the host's `TerrainCache` keeps a `DrawCache` (`scene/src/retained.rs`) holding each Instance's items between frames, and an Instance's items are made again only when something they were made from changes:

- **The Instance itself**: its `revision`.
- **What it reads from other Instances**: its SurfaceAppearance and light children, a Decal's part, and the Inventory whose `Equipped` puts a carried Tool in or out of the world. Their revisions are kept with the items. No other ancestor property is read while drawing, so editing a Model or Folder invalidates nothing.
- **The tree**: a move in or out of the Workspace, or among a parent's children (`World::child_order_version`), lists the entries again in tree order and rechecks each one's children and character membership; the rest keep their items. A frame whose tree didn't change doesn't walk it.
- **Everything at once**: another World (`World::identity`; a copy keeps its revisions), another or changed `Assets` (`Assets::generation`: loads, hot reloads, material edits, `forget`, the blueprint image) or a change in the bloom glow boost.

Lights, reflection and light probes, ProjectedDecals, joints and animated MeshParts are still worked out each frame, and streaming's hidden set is checked each frame. Terrain, UI and particles are drawn as before. The output is the same `DrawList`; `render` doesn't change.

**It's on by default.** `AXCENITY_RETAINED_DRAW_LIST=off` (or `0`) builds every frame from scratch instead, and `TerrainCache::set_retained_draw_list` does it from code. One-off `build_draw_list` calls (no cache passed) always build from scratch. `scene/src/retained_tests.rs` checks, over randomized worlds and edits, that each retained frame equals a from-scratch build item by item, plus one test per way a kept item could go stale.

Cloud container (4 vCPU Xeon at 2.1 GHz, release build), times in milliseconds. The CPU benches are the fastest batch's median of 5 × 60 frames over three runs each, alternating off and on. S10k is the player's `--bench 300` under Xvfb on lavapipe, two runs each, alternating; its Render and frame numbers are software rasterization, so only the Draw list step says anything about a real machine.

| | From scratch (off) | Retained (on) |
|---|---|---|
| `bench_build_10000_parts`, all still | 6.4 to 8.0 | 1.2 to 1.5 |
| `bench_build_10000_parts`, 1% moving each frame | 6.1 to 7.1 | 1.42 to 1.45 |
| `bench_build_2000_mesh_parts` | 0.94 to 0.99 | 0.36 |
| S10k player, Draw list median | 12.9 | 4.3 to 4.4 |
| S10k player, Draw list 95th percentile | 18.0 | 5.6 to 6.1 |
| S10k player, frame median (lavapipe) | 128.9 to 129.5 | 118.9 to 120.1 |

- **What's left when nothing changes is copying**: each frame still copies every kept `DrawItem` into the new list (a few hundred bytes and some `Arc` counts each), about 1.2 ms for 10,000 parts. S10k rebuilds about 1,100 Instances a frame (its falling drops, spinners and recolored blocks) on top of that.
- **Next steps** (report section 5.2, not done yet): put still, anchored parts into spatial `ItemChunk`s so whole cells skip the copy and the renderer's LOD resolve, then remove the from-scratch path.

## Small dense patches (biome test projects)

Nine 72 × 72-stud projects, about 35,000 painted models each (pebbles, litter, grass tufts) and two trees of 0.7 to 1 million triangles. Measured with `shoot_project` at 2560 × 1440, on the machine above (AMD Radeon RX 9070 XT, Vulkan), release build. The cells are the range over five of the projects and two cameras. Times are milliseconds; two runs of one view can differ by about 1.5 ms of GPU.

| Ultra | Before | After |
|---|---|---|
| Shadow casters drawn (all views) | 66,800 to 96,500 | 38,300 to 66,600 |
| CPU `ScenePipeline::draw` | 9.3 to 10.4 | 8.0 to 8.8 |
| GPU frame | 14.3 to 20.1 | 14.4 to 19.6 |

At High, after these changes, the GPU frame is 10.0 to 15.9.

- **Shadow reach (`SHADOW_REACH`, `renderer/lod.rs`).** An item with a cull distance casts only to 35% of it. Most of the casters were pebbles and tufts drawn into every cascade. The pictures from both cameras are the same to the eye.
- **Cut-out edges (`fs_depth_coverage`).** Alpha to coverage on the depth fill (see Anti-aliasing in [shadows.md](shadows.md)) costs up to about 1 ms of GPU here, which is about what the shadow reach saves, so the GPU frame is unchanged while the edges are smooth.
- **Simpler meshes for shadows were tried and removed.** Drawing shadows two levels of detail down saved about 1 ms, added 36 to 54 MB of mesh memory and visibly changed how the trees shaded themselves and the ground.
- **Game logic isn't the cost**: `Session::frame` is 0.4 ms in `bench_project_play`. What's left is triangles. The two trees are 1.7 million triangles in up to seven passes, about half of the frame's, so a project's first saving is lighter tree models.

## Temperate Dry Patch at Ultra, 2560 × 1440 (2026-11-11)

The owner's primary benchmark: a 72 × 72-stud biome patch (see Small dense patches above) on the desktop above (Ryzen 9 9950X3D, RX 9070 XT, Vulkan), release builds, pinned to the V-cache CCD.

**The player** (`axcenity-bench-check`'s `dry-patch-ultra-1440p`: `--size 2560x1440 --quality Ultra`, vertical sync off, 400 frames from the project's demo camera): **frame median 11.3–11.5 ms (about 88 FPS), 1% low 72–75 FPS, GPU-bound**: the GPU's frame is 11.3 ms of it. Game logic is under 0.3 ms (Draw list 0.1 ms); the CPU's drawing work (`Render`) is 4.8 ms and the rest is Present wait. GPU passes: shadows 3.8 ms, depth prepass 1.6, ambient occlusion 0.2, scene 5.6, post-processing 0.1. So the 20–30% CPU and 70% GPU utilization the owner saw with vertical sync on is a GPU-bound frame waiting on the display, not idle headroom on the CPU's side.

A first run reported a frame-time standard deviation of 2.6 ms and a mean 1.5 ms below the median. The timeline (`AXCENITY_BENCH_TIMELINE`) showed one step, not jitter: the project's `views.client.lua` holds a demo camera for 8 seconds and then hands the camera to the walker, whose view costs half as much. The check now measures 400 frames, all on the demo camera.

**Where it goes** (`profile_project`, three cameras: the walker's view, under the canopy, along the ground; the offscreen renderer at the same size and preset, so 1 to 2 ms under the player's frame). GPU frame in milliseconds, then the change from leaving each cost out:

| | Walker | Canopy | Ground |
|---|---|---|---|
| GPU frame (shadows / prepass / scene) | 9.09 (3.01 / 1.02 / 4.88) | 9.18 (3.59 / 1.07 / 4.26) | 9.13 (3.46 / 1.37 / 4.03) |
| No shadows at all | −4.52 | −4.99 | −4.40 |
| No shadows from painted foliage | (−0.9 shadows pass) | −0.96 | −1.12 |
| The two oaks left out (1.7 M triangles) | −3.84 | −4.50 | −2.56 |
| Painted foliage left out | −2.26 | −1.71 | −2.86 |
| Anti-aliasing off (MSAA 4×) | −1.46 | −0.74 | −0.77 |
| Double-sided drawn one-sided | −1.28 | −0.71 | −0.89 |
| Ambient occlusion off | −0.13 | −0.11 | −0.23 |
| No wind / no sway | −0.09 / −0.14 | −0.08 / −0.08 | −0.05 / −0.01 |
| Cut-outs drawn solid | +3.06 | +4.84 | +2.49 |

Overdraw (`OverdrawStats`, layers each pixel looks through): walker 16.8 per covered pixel (13.6% of the picture at 32 or more), canopy 14.6, ground 22.5 (24.5% at 32 or more, the busiest cells along the grass at the horizon, 44 to 49 layers).

What it says:

- **It's triangles and shadows, not fragments.** The two oaks are 969k and 738k triangles at LOD0 (`lod_report_project`), drawn in up to six views (cascades, prepass, depth fill, lit). Leaving them out saves 2.6 to 4.5 ms; leaving out all 35,000 painted ground models saves 1.7 to 2.9 ms. **Most of each oak is bark**: 549k and 556k bark triangles against 163k and 182k of leaves (plus 258k of moss cards on one), and the bark material is double-sided.
- **The bright overdraw is mostly cheap.** Cut-outs are lit once per pixel already (depth fill, then lit where depth is equal): drawing the same leaves as solid cards costs 2.5 to 4.8 ms *more*, which is what the depth fill saves. What a layer still costs is the depth fill's one alpha sample and its vertices. Overdraw is highest along the grass at grazing angles, where nothing can be culled behind it.
- **Wind is free.** Every variant without it is within 0.15 ms: the sway is a few vertex instructions on the GPU, with one shared `GlobalWind` in the frame uniform and no CPU work per plant (see [wind.md](wind.md)). Nothing to optimize.
- **Ambient occlusion is cheap** (0.1–0.2 ms). Anti-aliasing costs 0.7–1.5 ms and double-sided materials 0.7–1.3 ms, but turning either off changes the picture (jagged leaf edges, missing leaf backs), so neither is a saving to take at Ultra.
- **Shadows are the largest single pass** (3.0–3.6 ms; 4.4–5.0 ms with the lookups). Painted foliage's share is about 1 ms, even though `SHADOW_REACH` already limits it to 35% of its cull distance.

### Whose overdraw it is

`profile_project` with `AXCENITY_PROFILE_OVERDRAW_GROUPS=<n>` draws the overdraw view again with only one model primitive's items at a time (named `file: material` from the models the world's MeshParts and terrains show) and lists the `n` biggest by their share of all the layers. Temperate Dry Patch, Ultra 2560 × 1440:

| Share of the layers | Demo camera | Along the ground |
|---|---|---|
| The five grass models (13,180 tufts, 28–108 triangles each) | 73.8% | 80.4% |
| Both oaks' leaves | 5.2% | 10.4% |
| Oak bark | under 1% | 2.4% |
| Shrubs, rocks, thistles and the rest | the remainder | the remainder |

So the bright bands in the overdraw view are grass seen at a low angle, 6 to 12 layers where each grass model covers anything and 40 to 50 where they all overlap along the horizon. The canopy is a small share. What a grass layer costs is its corners and the depth tests (the alpha sample of the depth fill and the prepass), not lighting; leaving all painted foliage out saved 1.7–2.9 ms of GPU, a third of it their shadows. Fewer layers there means fewer tufts far away (density by distance). That was built and measured as a graphics setting (small painted plants thinning out past 35% of their cull distance) and dropped: on all three projects the GPU frame moved by −0.12 to +0.05 ms, since a grass tuft's cull distance (about 290 studs) puts its thinning far beyond the grass that costs. The code and numbers are on the branch `claude/project-thread-92jjy2-grass`.

### Shadows on the patch, cascade by cascade

`RenderStats::cascade_times` (graphics timer stamps before each of the sun's cascades and after the last, inside the Shadows pass; shown in `fps full`, the Profiler's Graphics tab and `profile_project`) with `cascade_casters` and `cascade_triangles`. Demo camera, Ultra 2560 × 1440:

| Cascade (nearest first) | GPU | Casters | Triangles |
|---|---|---|---|
| 1 | 1.22 ms | 5,238 | 2.35 M |
| 2 | 1.53 ms | 15,062 | 3.58 M |
| 3 (every other frame) | 0.87 ms | 15,938 | 3.70 M |
| 4 (every other frame) | 0.61 ms | about the same | |

So a frame's 3.6 ms of shadows is the two near cascades (2.75 ms) plus one far one. Leaving the oaks' shadows out saves 1.9 ms; painted foliage's, 1.0 ms. High's shadows (three 2048² maps, `shadows-high`) cost 2.6–2.9 ms less than Ultra's four 4096² ones. Halving the shadow distance doesn't make them cheaper (+0.4 to +1.4 ms): the same casters land in denser cascades, so the cost follows the texels the casters cover, not the distance.

What was checked and left as it is:

- **The implementation does what it should.** Casters are culled per cascade by their spheres against each cascade's box, the far two cascades take turns past 12,000 casters, painted foliage casts only to 35% of its cull distance, cut-outs cast from their depth-only pipeline (no alpha test, so no fragment shader in the shadow pass), and a cascade is snapped to texels. Nothing is drawn twice or for nothing.
- **Big meshes cut into spatial clusters** (tried, not kept). Cutting every primitive over 65,536 triangles into pieces of at most 32,768 by where its triangles are, so each cascade and the camera cull pieces instead of whole trees, took the first cascade from 2.35 to 1.76 M triangles and the shadow pass from 3.58 to 3.37 ms, but the scene pass got 0.15–0.5 ms slower (2,427 to 2,799 draw calls; the pieces share one distance band, so they lose the sort's near-first order) and the frame didn't get faster (10.81 ms against 10.79–10.88). It would pay for one huge mesh seen partly; the oaks are seen whole.
- **Shadow-specific LOD** was tried before (Small dense patches, above) and rejected for changing how the trees shade themselves. A tree's simplified copy may move its surface up to 2% and 5% of its size, which at the near cascades' texel size (under 0.05 studs) is visible.
- **Caching cascades between frames** isn't worth it here: every plant sways, so its shadow moves every frame, and a moving camera moves the cascades. A scene with no wind and a still camera could keep them; no game benchmark is like that.
- **Lowering Ultra** (fewer or smaller maps) is the one big saving (2.7 ms) and it's a quality choice, which the High preset already offers.

The levers that remain are the scene's: the oaks are 1.7 M triangles, three quarters bark, and the bark material is double-sided. Drawing only the bark one-sided (`profile_project`'s `single-sided-match` with `AXCENITY_PROFILE_MATCH=Bark`) saves 0.22–0.48 ms of GPU across the three cameras, and the pictures match (at most 108 of 3.7 million pixels differ by more than 8 of 255, one by more than 32): the bark is closed, so its back faces are never seen. The owner approved it, so both oaks' Bark materials are one-sided in the project's `.glb` files (originals kept in a backup outside the project). The player's frame on the regression check went from 11.74 to 10.94–11.04 ms (GPU 11.47 to 10.82–10.91 ms, 1% low 70 to 76–78 FPS), and shots from the three cameras match the ones before within the numbers above. Making every double-sided material one-sided would save 0.6–1.5 ms but leaves the backs of leaves missing.

After all of this the full regression check matched every baseline within 7% (Temperate Dry Patch 11.6 ms, GPU 11.3 ms, 1% low 71.6 FPS) and `cargo nextest run --workspace` passed 3,184 of 3,184.

## Fog boxes read one at a time (Desert Homestead)

Paul's Desert Homestead (228 MeshParts, two terrains, 65,000 painted plants, an Atmosphere, Clouds and two fog volumes: a valley's haze and the house's interior air) ran at about 11 frames a second on an RX 9070 XT. Measured on 2026-11-25 on the Desktop under Fedora (radv, a 240 Hz 2560 × 1440 display), release build, the player's `--bench` at `--size 2560x1440`, main at `467da9f2` against the fix; one run each, GPU medians in milliseconds.

**Finding it.** The CPU's frame was 8 ms; the GPU's scene pass was 50 ms on Low and 110 ms on High, with 10 million triangles that the shadow maps and the depth prepass drew in 1 and 0.55 ms: so shading, not geometry, and not a quality setting. A copy of the project with one kind of thing taken out of `world.json` at a time (High, 120 frames) said which:

| Left out | Scene pass |
|---|---|
| Nothing | 110.0 |
| Clouds, the Atmosphere, Weather, PointLights, particles (each) | 109.5–112.2 |
| The terrains (and their plants) | 79.0 |
| The MeshParts | 71.8 |
| The interior volume | 64.2 |
| The valley haze volume | 63.9 |
| Both volumes, or only their FogEffects | 10.3 |

**Cause.** Each box of dust cost about 50 ms, for a ray-box intersection and two `exp`s a pixel. fog.wgsl's functions take the `Fog` as a value (it has no bindings of its own) and indexed its array of eight boxes with the loop's counter (`fog.dust[i]`). An array inside a value can't be indexed in place by a number only known as the shader runs, so each look copies the array first: all eight boxes (64 `vec4`s) to pick one, twice a box in `fogged_along`, on every pixel. (That reading comes from the cost and from what took it away; the driver's compiled shader wasn't looked at.) `weather_sample_as`, which indexed the uniform itself (`frame.fog.dust[i]`), cost nothing measurable.

**Fix.** The functions read a box through `fog_dust(i)`, which lighting.wgsl and sprite.wgsl define over their own uniforms ([fog.md](fog.md)). The pictures are the same (the render crate's 446 tests pass, the fog's among them).

| 2560 × 1440 | Scene pass before | after | GPU frame before | after |
|---|---|---|---|---|
| Low (200 frames) | 50.1 | not run | 50.5 | not run |
| Medium (200 frames) | 77.2 | not run | 78.3 | 7.3–8.9 |
| High (200 frames) | 109.2 | 10.4 | 110.3 | 12.4 |
| Ultra (the tour's first shot, 600 frames) | 134.5 (110 rising to 411 as the camera closes on the house) | 13.4 | 138.9 | 16.9 |
| Ultra, volumetric fog's own pass | 2.53 | 0.45 | | |
| Ultra, the whole 91-second tour (5,500 frames) | not run | 13.6 (5.2–16.5 by shot) | not run | 17.1 (9.5–21.2 by shot) |

**What a frame takes on the screen.** The `--bench` table's FPS line counts the frame's steps only. Under Wayland the next frame starts when the compositor says the last one was shown, so the pace is the CPU's frame plus the GPU's, rounded up to the display's refresh: timing whole runs of two lengths gave 20.6 ms a frame on High (6.7 CPU + 13.3 GPU, about 48 a second) and 23.4 ms on Ultra (about 43), where the FPS line said 136. Before the fix that is about 9 a second on High and 12 on Medium, which is what Paul saw. A run whose window is hidden gets no such signal and crawls (a 360-frame run took 255 s once): keep the window in view.

**The dusk time-lapse** (the tour's last shot runs the afternoon out in 20 seconds) held the CPU's Render step at 29–37 ms a frame: the sun moves a tenth of a degree nearly every frame, and each move works the atmosphere's 256 × 128 sky image out again, 16,000 looks through the air on the frame's thread (found by sampling the thread's stack with `eu-stack`). `sky_pixels` now shares the image's columns among the renderer's threads: Render is 10–13 ms through the lapse ([sky.md](sky.md)).

**What is left**, at 2560 × 1440:

- **The clouds' march** in the sky pass: 2.6 ms of the scene pass on High, 5.1 on Ultra (scene pass 10.4 → 7.8 and 13.4 → 8.3 without Clouds). It is stepped at full size for every pixel of sky, its noise worked out as it goes. Done on 2026-11-26: the sky's clouds are traced before the scene pass, at half size and then pixel by pixel where that isn't enough, with their shapes read from a texture ([clouds.md](clouds.md#the-traced-clouds)); 0.14 to 0.30 ms at 1920 × 1080 on Ultra, the same picture.
- **The terrains with their 65,000 plants**: 4.9 ms of the scene pass on High and 4.6 ms of the CPU's Render step (5.7 → 1.2 without them), about 10,000 items and 3,500 draw calls a frame.
- **A stall on a first sight**, 30–62 ms once on five of the tour's cuts: what is seen for the first time is uploaded in the frame that first draws it. Coming back to a view no longer stalls (next section).

## Kept within a budget (ADR-0051)

Each cut of Desert Homestead's tour stalled one frame for 30–105 ms. The renderer freed what hadn't been drawn for 10 seconds (`KEEP_UNDRAWN_FRAMES`, `KEEP_UNDRAWN_SECONDS`) and uploaded it again in the frame that next drew it, and the tour's shots are 10 seconds long; what is seen for the first time is uploaded in its frame too. Outside a streaming world the player uploads on the frame's thread. Paul chose to keep what fits a memory budget over uploading on a thread (a white or missing model after a cut is worse than one stall on a first view): the player's window and the editor's viewport now keep meshes and color textures while they fit the mesh and texture memory budgets ([models.md](models.md)).

Same machine and method as above (player `--bench 5500`, the whole tour, 2560 × 1440, one run each, `AXCENITY_BENCH_TIMELINE`); the CPU's frame at each cut, the worst of the frames around it, in milliseconds. A frame is otherwise 7 to 8 ms (10–13 through the time-lapse).

| Cut | Before (Ultra) | Kept, Ultra | Kept, High | Kept, High with a 2048 MB texture budget |
|---|---|---|---|---|
| Hero to Mailbox (first sight) | 31.3 | 30.8 | 31.1 | 30.2 |
| Mailbox to Porch (first sight) | 61.9 | 61.8 | 61.0 | 60.5 |
| Porch to Truck (first sight) | 46.3 | 38.6 | 38.1 | 38.4 |
| Truck to Windmill (first sight) | 47.3 | 30.0 | 30.1 | 29.9 |
| Windmill to Crane (first sight) | 103.0 | 57.7 | 56.8 | 57.8 |
| Crane to Dusk (back to the first view) | 105.3 | 14.3 | 12.3 | 12.8 |
| The tour's end (the fly camera, by the first view) | 75.1 | 21.3 | 22.6 | 22.2 |

- **Coming back is free of uploads**: the two cuts to a view already seen went from 105 and 75 ms to 12–14 and 21–23 ms (what is left of the second wasn't profiled; materials still leave after 10 seconds and are made again there). The three later first sights are cheaper too: what they share with earlier shots is still on the card.
- **First sights are as they were** (30–62 ms), by choice: the player would have to upload on a thread, or before the first frame, to take them away.
- **What the scene holds**, with nothing freed through the whole tour: 1,579 MiB of textures on Ultra (1,452 in 528 color textures; the rest mostly the terrains' layers and weights) and 174 MiB of meshes in 3,083 meshes (each level of detail is one). Both fit Ultra's budgets (2,048 and 1,024 MiB). On High the textures don't fit its 1,024 MiB: the least recently drawn lose their largest levels (the total stays at 1,018–1,020 MiB), so some are drawn below their full size, but the cuts cost the same: the timings match those with a 2,048 MiB budget.
- **Memory stays in use.** Nothing leaves while it fits, so what a session has seen stays on the card up to the budgets.
- **The budgets follow the card's memory** (added the same day, [settings.md](settings.md)): the runs above had the presets' fixed numbers. With a quarter of this card's 16 GB the player takes 4,096 MiB for textures and 2,048 MiB for meshes at every preset, so the scene's textures fit on High too. The whole tour again at the default settings (High): the cuts 30.4, 60.7, 38.9, 29.9, 58.4, 12.7 and 22.7 ms, the GPU's frame 13.3 ms at the median.

## Swaying plants bent together (ray tracing, Desert Homestead)

With every ray traced effect on, the editor drew Desert Homestead at 24 frames a second. Measured on 2026-11-25 on the Desktop under Fedora (radv, a 240 Hz display), release build, the player's `--bench` (300 frames) at `--size 1996x998` (the editor's viewport) with the editor's saved graphics settings through `--graphics` (Ultra, real-time GI, ray traced shadows, ambient occlusion and reflections at High, TAA), main at `a9370a3b` against the change; one run each, medians in milliseconds, not pinned.

**Finding it.** Stack samples of the running editor's frame thread (`eu-stack`) put 11 of 18 working samples in `RayScene::prepare` and in wgpu's acceleration structure builds inside `CommandEncoder::finish`. The player with the same settings said how much: 12.8 ms of a 33 ms GPU frame in the Acceleration structures pass, every frame. The wind was blowing, and each swaying plant within the 400,000-triangle budget got a BLAS of its own, built again each frame: 4,118 of them here, about 100 triangles each (the budget was written for a few big trees: "about 1 ms"). Capping how many were bent showed the cost is by the plant, not the triangle:

| Plants bent | Acceleration structures | Render step (the frame's thread) |
|---|---|---|
| 0 | 0.52 | 13.3 |
| 1 (the TLAS of 16,000 items built each frame) | 0.78 | 13.4 |
| 64 | 0.99 | 13.7 |
| 512 | 2.17 | 15.9 |
| 2,000 | 6.60 | 23.0 |
| 4,118 (the budget) | 12.8 | 33.1 |

About 3 µs of GPU time and 5 µs of the frame's thread a plant. A stand-alone probe (wgpu 30, 4,096 meshes of 100 triangles, all rebuilt in one call) confirmed that it is the BLAS that costs, and that geometries of one BLAS don't:

| Geometries a BLAS | BLASes | GPU | Record, finish and submit |
|---|---|---|---|
| 1 | 4,096 | 12.65 | 7.29 |
| 16 | 256 | 2.57 | 1.17 |
| 64 | 64 | 1.95 | 0.82 |
| 256 | 16 | 1.89 | 0.66 |
| 4,096 | 1 | 1.58 | 0.62 |
| One joined geometry | 1 | 1.49 | 0.02 |

**Fix**, in three steps ([ray-tracing.md](ray-tracing.md), "Swaying plants"):

1. **Bundles**: bent copies are geometries of shared BLASes, bent where they stand in the world, and a hit's item is its instance's first slot plus the geometry hit. Acceleration structures 12.8 → 2.5 ms; Render 33.8 → 24.0.
2. **One buffer, one dispatch a mesh.** The frame's thread was still at 23.5 ms in the renderer's draw: 8.5 preparing the ray scene (a uniform written and a bind group set for each plant), 6.2 in `finish` (4,100 dispatches), 1.5 in `submit`. With every copy bent into one buffer by one dispatch a mesh (about 60): 3.2, 3.7 and 1.2 of 14.1.
3. **By the square, and only what shows.** Rays traced slower against one BLAS a kind than against a BLAS a plant (ambient occlusion 3.8 → 5.0, ray traced shadows 5.4 → 6.8). Bundling by squares of the ground brought most of it back (16 bundles: 4.39 and 6.44; 100: 4.13 and 5.92; 280: 4.00 and 5.59, with the builds at 2.8, 3.2 and 3.6); `PREFER_FAST_TRACE` bundles traced as before but built 1.7 ms slower, no better in sum. Leaving out plants the wind moves less than a pixel bends 3,030 instead of 4,118 here (and reaches farther, to 345 studs, the budget no longer full): the GPU frame 24.5 → 23.8.

| 1996 × 998, the editor's settings | Before | After |
|---|---|---|
| Render step (the frame's thread) | 33.8 | 14.3 |
| GPU frame | 33.3 | 23.8 |
| Acceleration structures | 12.82 | 2.48 |
| Ambient occlusion (ray traced) | 3.79 | 4.15 |
| Ray traced shadows | 5.45 | 5.87 |
| Ray traced reflections | 0.37 | 0.42 |
| Scene | 7.35 | 7.37 |
| A frame as shown (whole runs of 200 and 800 frames timed) | 73.7, 75.9 and 88.7 (13 a second) | 41.7 (24 a second) |

**The picture is the same.** `shoot_project` from three of the film's cameras at 1600 × 900, Ultra with every ray traced effect and TAA: the old build against itself differs by nothing, against the new by 0.004 to 0.006 of 255 a channel on average, no pixel by more than 23.

**What is left** with ray tracing on:

- **A frame as shown is the thread's frame plus the GPU's** (15.4 + 23.8 here, 41.7 as shown), as the fog section found for the player: the next frame starts when the compositor says the last was shown. With both this long it is the largest lever left, in the editor too.
- **The GPU frame**: the scene pass 7.4, ray traced shadows 5.9, ambient occlusion 4.1, the builds 2.5.
- **The frame's thread**: 14.1 ms in the renderer's draw, of which the ray scene's 16,000 items walked each frame are 3.2 to 3.6, `finish` 3.7 and `submit` 1.2.
- **The editor itself wasn't timed** after the change, only sampled before it: these are the player's numbers at its viewport's size and settings, from the player's camera.

## The CPU and the GPU took turns (frame pacing, Desert Homestead)

Paul's report of 2026-11-25, after the section above: about 41 FPS in the viewport, 13 in the Profiler, and a card that didn't seem to be working. An audit of the whole frame followed, measured on the Desktop under Fedora (GNOME 51 on Wayland, Mesa 26.2 radv, a 240 Hz display): the release player's `--bench` at `--size 1996x998` with the editor's saved graphics settings (Ultra, real-time GI, ray traced shadows, ambient occlusion and reflections at High, TAA), the editor itself with `AXCENITY_FRAME_LOG=1` on a copy of the project, the card's sensors (`gpu_busy_percent`, the core clock and power under `hwmon`, the process's `drm-engine-gfx` time) sampled twice a second beside each run, and `eu-stack` samples of the frame's thread. Medians in milliseconds, one run each, not pinned. How to read the tools: [benchmarks.md](../benchmarks.md).

**The two frame rates.** Neither was a frame rate as shown. The viewport bar worked its number out as `1000 / max(CPU, GPU)` from the viewport's own times: what a renderer whose CPU and GPU work fully overlap could reach, leaving out the rest of the editor's frame. The Profiler's is the time between game frames in Play, which is real. Timed for the first time, the debug editor showed Desert Homestead at **26.7 frames a second** (37.5 ms between frames: 25.8 ms of CPU, then 13.2 of GPU) where the bar's formula gave 43. The bar now shows the measured rate, with the work in its tooltip, and `--bench` prints `as shown` (it had said "FPS average 68, GPU-bound" for a run shown at 24).

**The card was idle half of every frame.** With the frame shown every 40.8 ms the card was 59% busy, drawing 70 W of its 304, its core clock at 1,511 MHz; the frame's thread worked 15 of those 41 ms. The two never worked at once. Under Wayland, winit's `Window::pre_present_notify` holds the next `RedrawRequested` back until the compositor's frame callback, and the compositor can't show a frame before the GPU has finished it; the player called it each frame and egui-wgpu calls it for the editor. So each frame's CPU work started when the last frame's GPU work was done and shown. Idle for the CPU's turn, the card clocked down, which nearly doubled its own time.

| Player, 1996 × 998, the editor's settings | Waiting for the display | Started early (`FramePacer`) |
|---|---|---|
| As shown | 24.5 a second (40.8 ms) | **67.0 a second** (14.9 ms) |
| 1% low, as shown | 16.0 | 43.8 to 54.1 |
| GPU frame | 24.24 | 13.40 |
| Frame's thread (steps less waits) | 15.3 | 14.7 |
| Card busy / core clock / power | 59% / 1,511 MHz / 70 W | 89% / 3,136 MHz / 280 W |
| Ray traced shadows / ambient occlusion / reflections | 5.89 / 4.40 / 0.45 | 3.05 / 2.17 / 0.23 |
| Scene / acceleration structures / shadow maps | 7.50 / 2.47 / 1.66 | 4.22 / 1.36 / 1.09 |
| The same view in a 2560 × 1405 window (asked for 3840 × 2160, which GNOME gives as 2560 × 1405), as shown | 20.7 a second | 53.0 a second |

| Editor, Desert Homestead, its own camera and default window | Waiting | Started early |
|---|---|---|
| Debug build (what Paul runs), as shown | 26.7 | **41.0** |
| Release build, as shown | 26.7 | **46.7** |
| The whole editor frame on the CPU, debug / release | 25.8 / 21.4 | 23.9 / 21.0 |

**The fix** ([ADR-0052](../../wiki/Architecture/Architecture-Decisions/ADR-0052-A-Frame-Starts-While-The-GPU-Draws-The-Last-When-Frames-Miss-The-Display.md)): `axcenity_render::FramePacer` keeps the display's pacing while frames fit in a refresh (the lowest latency, and free then) and starts the next frame at once when they keep missing it; a renderer whose frames are shown is one frame ahead (`set_one_frame_ahead`), so starting early never queues more than one frame behind the one being drawn (in the larger window frames had queued as deep as the swapchain). The editor reaches the call through a switch in a vendored egui-wgpu (`vendor/README.md`). The picture is the same: no setting, shader or resolution changed.

**A stall twice a second.** The `1% low` stayed at 22 after that. Every 30 frames the player read the process's memory for the F9 console (the editor every 15 in Play), and on Linux the `memory-stats` crate reads `/proc/self/smaps` for it: **24 ms** for this process's 2,249 mappings, on the frame's thread, in every game. `/proc/self/statm` has the number in 0.03 ms (the crate's `always_use_statm` feature). 1% low 22 to 27 → 52 to 54, average 62 → 66.6.

**Where the frame goes now** (started early; the card's times at its full clock):

| What | Frame's thread | GPU |
|---|---|---|
| The whole frame | 14.7 | 13.1 |
| Ray tracing altogether (every effect off, same device) | 5.4 | 4.8 |
| One effect off: shadows / ambient occlusion / reflections / real-time GI | 0.7 / 0.0 / 0.8 / 0.2 | 3.05 / 2.17 / 0.23 / (probes: not timed apart) |
| Terrain foliage (16,000 items; the scene without it) | 9.7 | 4.3 |
| The 228 MeshParts | 1.5 | 1.5 |
| Clouds | 0.0 | 1.3 of the scene pass |
| Point lights, fog volumes, post effects | under 0.3 each | under 0.2 each |

The card's clock follows its load, so a run that leaves it idle part of the frame reads its passes longer: compare GPU pass times only between runs with the card busy, and trust the frame's thread and the rate as shown first.

On the frame's thread (220 `eu-stack` samples of the release player): wgpu's `CommandEncoder::finish` 29% (encoding the render passes' draws 20%, the acceleration structure builds 6%), the ray scene's `prepare_scene` 19%, `Queue::submit` 10%, waiting for the planning threads 9%, `FrameUses::of` 6%, the terrain pass 4.5%, TAA's `History::moved` 3.6%. The editor's viewport spent 20.3 ms where the player's draw list and render took 14.2: that turned out to be the view, not the editor (next section).

**What is left**, by size, with the targets they add up to (100 frames a second at these settings needs both sides at 10 ms):

| Target | From → to | Lever |
|---|---|---|
| Foliage's cost on the frame's thread | 9.7 → 4 ms | Every plant is an item walked several times a frame (culling, levels of detail, motion history, the ray scene's instance). Clusters of plants with one bound each, their instances kept between frames |
| Ray tracing's cost on the frame's thread | 5.4 → 2 ms | `RayScene::prepare` builds every table again from the draws each frame (16,000 items, several hash lookups each). Keep what didn't change; the TLAS is rebuilt each frame only because plants sway |
| `CommandEncoder::finish` | 4.3 → 1.5 ms | wgpu encodes a pass's draws when its encoder is finished, on the calling thread: give the shadow, prepass and scene passes an encoder each and finish them on the render threads |
| ~~The editor's viewport against the player's~~ | — | There is no editor-only cost: the same view costs the same in both (next section) |
| ~~Shadow maps while ray traced shadows cover their distance~~ | — | Not redundant: leaves' light from behind and the fog read them at every distance (next section) |
| Ray traced shadows and ambient occlusion | 5.2 → 3.5 ms GPU | Not started: measure rays a pixel actually traced before choosing |

**Not measured**: the editor at Paul's own viewport size (its window opened at the default size here), Play in the editor (the game's steps are 0.3 ms in the player for this project), Windows, and input latency (worked out in the ADR, not timed).

## Plants by the cluster, and the audit's second round (Desert Homestead)

Paul's six questions of 2026-11-25 on the section above, answered the same day on the same machine and settings. Two views are measured from here on: **the tour** (the project's 91-second camera sequence, which the player's `--bench` flies: about 11,000 items in view) and **the still view** (the editor's start camera, from 12, 8, 16 toward the origin, with the tour stopped: at 1996 × 998, 15,009 items drawn in 4,345 draw calls, 13.4 million triangles, and 25,193 items in the ray traced scene), the heavier of the two and the one to optimize against. Both at 1996 × 998 with the editor's settings unless a row says otherwise.

**Where the frame's thread goes, section by section.** `RenderStats::cpu_times` (`renderer/cpu_timer.rs`, `CpuSection`) times `ScenePipeline::draw` in fifteen sections on the thread that calls it. `--bench` prints them as "Drawing on the frame's thread, by section" and the editor's frame log as `drawing, ms:`, with the items drawn, the draw calls, the traced items and the viewport's size, so both hosts are read the same way.

**1. The editor has no cost of its own.** The 6 ms between the editor's viewport and the player was the view. The player's `--bench` flies the tour; the editor stood at its start camera, and its window (opened at the default size) gave a viewport of 998 × 405. With the tour stopped and the player at that camera and size, both draw exactly 17,190 items in 4,780 draw calls, and every section agrees:

| Section, ms | Editor (release) | Player, same view and size |
|---|---|---|
| Levels of detail | 0.62 | 0.62 |
| Materials | 1.40 | 1.43 |
| Culling and batches | 1.21 | 1.20 |
| Motion history | 2.69 | 2.51 |
| Ray traced scene | 5.05 | 4.85 |
| Encoding (wgpu `finish`) | 4.09 | 4.05 |
| Submit | 2.73 | 2.70 |
| All fifteen | 20.3 | 20.25 |

The rest of the editor's frame (its panels) is 0.7 ms on top in a release build and 2.7 in a debug one. `AXCENITY_START_CAMERA=x,y,z,x,y,z` starts the editor from a chosen view for such comparisons.

**2. Plants by the cluster** (`renderer/clusters.rs`). The scene already hands the renderer its painted plants in groups (`DrawList::chunks`, one shared `Arc<[DrawItem]>` a square of ground, the same from frame to frame), and the renderer already passes over whole groups no view can see. What it didn't do is remember anything about a group: every pass worked each plant out again each frame, reading its 600-byte item. A `Cluster` is one group's items with what never changes about them kept in small arrays: each item's material binding (among the group's distinct ones), its mesh, where it stands, whether a skin poses it, and what rays need of it (holes, see-through, its surface's color and glow). It is found again by the address of the group's items and holds them, so the address can't come to mean other items; a group the scene builds again gets a new cluster. Each draw says which group and item it is (`Draw::cluster`, `Draw::at`). Three more things came out of the same timings:

- **A terrain's chunks shared no work.** Each chunk's material key hashed all the terrain's layers' textures again (half a microsecond), twice a frame for each of 1,400 chunks: 1.7 ms. The key is now remembered for the layers it was worked out of (`terrain::LastArrays`).
- **The motion history built a whole scene instance** (colors turned to linear light, a matrix inverted) for every swaying plant to read its place and sway from, hashed every plant's ID in a table that only holds the list's own items, and sorted 180-byte records by mesh. It now reads the two values, looks up only what can be there, and gathers by mesh.
- **The list's own items** (1,659 here, the terrain's chunks among them) are shared among the threads again for their material keys: split off from the groups they had fallen under the size worth sharing.

| The still view, frame's thread, ms | Before | After |
|---|---|---|
| Materials | 1.37 | 1.05 |
| Uploads (the terrain pass's chunks) | 1.00 | 0.67 |
| Motion history | 2.12 | 1.11 |
| Ray traced scene | 4.58 | 4.04 |
| All fifteen sections | 18.52 | 16.45 |
| As shown | 51.0 a second | **57.0 a second** |
| The tour, as played | 67.0 a second | **70.8 a second** |
| Editor at its start camera, debug / release | 41.0 / 46.7 | **44.4 / 50.2** |

**The picture is the same**: `shoot_project` from three cameras (the still view and two of the tour's) at 1600 × 900, Ultra with every ray traced effect and TAA, before against after: two pictures identical, the third within 4 of 255 at its worst pixel, which is what two runs of the same build differ by (6). A grouped list is also tested to plan, draw and ray trace exactly as the same items listed one by one (`tests/grouped_items.rs`: the pictures equal to the last bit).

What a cluster doesn't hold yet, and what it would save, by the same timings: each item's bounds and scene instance (Culling and batches, 1.06 ms), and its place in the ray scene's tables (below).

**3. The ray traced scene, and what must be done each frame.** Its 4.6 ms split, measured with timers inside `RayScene::prepare` (25,193 items wanted, 22,876 instances):

| Part | ms before | after |
|---|---|---|
| Walking the draws: each item's mesh, reach, mask, holes and surface | 1.86 | 1.08 |
| Recording the builds (wgpu checks every TLAS instance) | 0.87 | 0.88 |
| The slot, cut-out and surface tables | 0.56 | 0.64 |
| Choosing and bending the swaying plants | 0.47 | 0.53 |
| Laying out slots and bundles | 0.29 | 0.29 |
| Setting TLAS instances, BLAS lookups, bundles | 0.51 | 0.51 |

The clusters took what was per material out of the walk. What is left is mostly not ours to keep between frames: while any plant sways, its bundle's BLAS is rebuilt, and wgpu has no refit, so the TLAS is built again each frame, and a wgpu TLAS build costs for **every instance** whether it changed or not: checked when the build is recorded (0.9 ms), tracked when the frame is submitted (in the editor at one view, Submit is 2.5 ms with ray tracing against 0.24 without) and uploaded. With one instance a still plant that is 22,876 instances. So the persistent data that would pay is fewer instances, not cached tables. Two ways, neither built: still plants bundled into BLASes by square of the ground as the bent ones are (their corners copied into world space once, by a rough estimate 40 MB of positions for this view, and a bundle to rebuild whenever one of its plants changes its level of detail), or two TLASes (still and swaying), which every ray would have to trace twice. The first wants its own measurement of trace speed and memory before it's chosen.

**4. The shadow maps are not redundant.** With ray traced shadows on, the cascades are still read every frame (`lighting.wgsl`, `volumetric_fog.wgsl`):

| Reader | Uses the traced result |
|---|---|
| Solid surfaces the prepass drew (`sun_visibility`) | Within 85% of the reach, blending into the cascades to the reach (here the reach is the shadow distance, 300 studs, so the far band always reads them) |
| Leaves and thin cloth lit from behind (`through_thin` → `soft_sun_visibility`) | Never: always the cascades |
| See-through surfaces and water | Never (not in the prepass) |
| Volumetric fog's cells | Where a cell was traced and near; the cascades past that |
| Any pixel whose depth doesn't match the traced mask (edges under MSAA, hashed hair) | Falls back to the cascades |

Desert Homestead has the first four in its views (its plants' leaves, its fog boxes), so nothing was removed. The pass could be skipped only in a frame with no translucent material, no see-through item, no fog, no MSAA and a reach 18% past the shadow distance; Desert Homestead has no such frame, and that skip isn't built.

**5. Hidden and unfocused windows.** Three things were found and fixed in the player, each by a tested rule (`player/src/window.rs` `next_frame`, `FramePacer::waits_for_display_when`):

- **The event loop spun a core** whenever it waited: between frames while waiting for the display (the frame's thread was at 96% of a core doing 15 ms of work in 41; now 35%), and for as long as a window was minimized. It now sleeps until the redraw comes.
- **A hidden game stopped, or drew for nobody.** Under Wayland a minimized window is never asked to draw: a light game stopped stepping altogether (its `--bench` never finished), and a heavy one, started early, drew at full load into a window nobody saw (256 W). Now a redraw unanswered for a quarter of a second means the window is hidden: the game steps 30 times a second without drawing (0.3% of a core, the GPU idle at 48 W) until the display asks again (a heavy scene minimized: 2% of a core and 56 W). A frame the window couldn't show (minimized on Windows, covered on macOS, where the redraw comes and the frame is skipped) spaces out the next the same way.
- **A window without focus waits for the display** whatever its frames cost: a minimized or covered window never has focus, and nothing else under Wayland says it's hidden. A visible window without focus draws at the pace it had before frame pacing.

Windows and macOS weren't run. What their sources say, and how to check a platform by hand: [benchmarks.md](../benchmarks.md#checking-frame-pacing-on-a-platform).

**6. Input latency, measured.** `AXCENITY_BENCH_LATENCY=on` times each frame from its start (when the host reads input) to the GPU finishing its picture (`renderer/latency.rs`), which is the part the engine decides; the display adds up to one refresh after, the same either way. The tour as played (vertical sync on), medians and 95th percentiles in ms:

| | As shown | Input to finished picture |
|---|---|---|
| 1996 × 998, waiting for the display (before) | 25.6 a second | 38.7 (40.7) |
| 1996 × 998, paced | 70.6 a second | **27.6** (28.4) |
| 1996 × 998, paced, no limit on frames ahead | 70.6 a second | 27.8 (29.1) |
| 2560 × 1405 window, waiting for the display | 20.9 a second | 47.1 (49.7) |
| 2560 × 1405 window, paced | 54.4 a second | **35.6** (38.9) |
| 2560 × 1405 window, paced, no limit on frames ahead | 54.4 a second | 42.1 (47.3) |
| A light scene (`samples/projects/part`), waiting / paced | 238.0 / 239.6 a second | 3.05 / 3.00 |

Pacing shortens the way from input to picture by 11 ms in both heavy cases while more than doubling the frame rate. One frame ahead is worth 6.5 ms (8.4 at the 95th percentile) where the GPU is the slower side, at the same frame rate, and changes nothing where the CPU is. A light scene is paced by the display exactly as before.

**The frame counters, again.** With ray tracing off Paul saw the viewport "lock at 60 FPS". It was: the editor holds frames to its steady frame rate while someone works (Settings > Accessibility, 60 Hz in his settings), and with ray tracing off a frame fits in that. Timed, the debug editor showed 60.0 with the setting at 60 and 65 to 67 with it off. The counter was right and didn't say why. It now reads `60 FPS (held)` when the grid sets the rate and `30 FPS (idle)` when the editor rests between frames of scenery that moves on its own, and its tooltip says what holds the rate and about how many a second the frame's work would allow (`app/viewport_bar.rs`, `Pace`). The frame log (`AXCENITY_FRAME_LOG`) is never held to the grid.

**The frame counters, a third time (2026-11-27).** Paul: the counter read `60 FPS (held)` with the editor focused and 120 with it in the background, and he wants the real rate unless he chooses a hold. The label was right; the hold was the problem. Steady frames (for a second after any input, or always with Hold the refresh rate steady) were held to `Appearance::steady_hz`, 60 by default, and every saved settings file carried that 60, so it wasn't a choice anyone made. In the background no input means no steady frames, no hold, and frames at the waiting-for-the-display pace (ADR-0052). Neither earlier fix removed the hold: the first (2026-11-22) worked the counter out from `max(cpu, gpu)` instead of measuring it, which hid the hold and read 41 where 24 were shown; the second (2026-11-26) measured again and named the hold `(held)`. Now the limit is **Frame rate limit** in Settings > Accessibility, No limit by default, saved under a new key (`frame_limit_hz`) so the old saved 60 is dropped; a limit chosen since is kept and still marked `(held)`. Frames the last one asked for at once are never marked `(idle)`, focused or not. With no limit the focused editor is paced like the player: by the display while frames fit in a refresh, at once when they don't (ADR-0052), so a light scene reads up to the display's refresh (vsync stays on).

**Against Paul's targets** (2026-11-25), Desert Homestead at 1996 × 998, release player, medians of one run each:

| Configuration | Target | The tour | The still view | What limits it |
|---|---|---|---|---|
| Standard Ultra | 120+ | 119.2 | 102.0 | The frame's thread (8.2 and 9.6 ms) |
| Advanced lighting (Ultra with real-time GI, no rays) | 90+ | 118.3 | 104.4 | The frame's thread |
| Ray traced shadows and ambient occlusion | 90+ | 74.0 | 59.3 | Both sides (13.2 and 12.4 ms on the tour) |
| Heavy ray traced scenes (everything on) | 60+ | 71.8 | 57.5 | Both sides |
| Optimized Desert Homestead (everything on) | 100 | 71.8 | 57.5 | 10 ms a side is needed; they are 13.6 and 12.6 |

Nothing was turned down to get these. What stands between the ray traced rows and their targets, by size: wgpu's per-instance TLAS cost and the ray scene's own walk (5.5 ms of the thread with ray tracing on), `finish` encoding 4,780 draw calls on one thread (4.0 ms), and on the GPU ray traced shadows (4.7 ms) and ambient occlusion (2.6).

## Encoding on several threads, and one query a ray (the audit's third round, Desert Homestead)

Paul accepted the second round's findings on 2026-11-25 and asked for five more things: keep a window without focus waiting for the display, go on looking at what ray tracing costs the GPU (shadow and ambient occlusion sampling, BVH traversal, denoising) without touching the picture, look at encoding on several threads and at fewer draw calls for the 4 ms `finish` took, consider a background rendering policy that can be set, and keep the measurements as regression benchmarks. Same project, settings and views as the sections above. Captures of three views with every ray traced effect on match the second round's: two byte for byte, the third in all but 16 of 1.44 million pixels, which is what two runs of one build differ by (17 to 21).

**Two things about the earlier numbers first.**

- **"3840 × 2160" was 2560 × 1405.** GNOME gives a window that nearly fills or overflows the screen the screen's free area, whatever was asked for (2400 × 1300 came out the same). The rows above that said 3840 × 2160 have been corrected. `--bench` now prints `drawn at` and the regression check calls out a size that differs from the baseline's.
- **GPU pass times only compare where the GPU is the limit.** The still view at 1996 × 998 is limited by the frame's thread, so the card's clock follows whatever was changed: with ray traced shadows skipped whole, the GPU's frame read 12.0 ms against 12.8, because every other pass ran slower. The GPU figures below are from the same view in a 2560 × 1405 window, where `--bench` says `GPU-bound`.

**1. Encoding on several threads** (`renderer/encoders.rs`). wgpu 30 only notes a pass's commands when they are recorded; `CommandEncoder::finish` checks each one and makes the backend's (wgpu-core's `encode_commands`). So the frame is recorded into several encoders, one after another, and they are finished on the renderer's threads together, then submitted in order in one call: the GPU does the same work in the same order. A part starts before each run of passes with many draw calls: every shadow view with 256 casters or more, the depth prepass (with motion vectors, ambient occlusion, the ray traced passes, fog and particles after it) and the scene (with post-processing and the UI). Desert Homestead's frame is six command buffers.

| The still view, frame's thread | One encoder | In parts |
|---|---|---|
| Encoding (`finish`) | 4.00 ms | **1.57 ms** |
| The parts' own times (posing and acceleration structures, three shadow views, prepass, scene) | — | 1.49, 0.35, 0.37, 0.67, 1.28, 1.34 |
| Submit | 2.25 ms | 2.39 ms |
| As shown | 57.0 a second | 64.7 |

The parts take 5.5 ms between them where one encoder took 4.0, and 1.5 ms of the frame's thread, which waits for the longest. The pictures are the same pixel for pixel (`a_frame_recorded_in_parts_is_the_picture_one_encoder_draws`, with and without ray tracing), and the GPU's timestamps still work across encoders, since wgpu resolves them at submit. `AXCENITY_FRAME_ENCODERS=one` is the way back. What bounds the step now is the first part: building the acceleration structures, where wgpu writes every TLAS instance again.

**2. Draw calls: where they come from, and why they weren't consolidated.** The still view's 4,345 draw calls: 923 in the scene pass, 919 in the prepass, 182 filling cut-outs' depth, 70 motion vectors, and 211, 388 and 568 in the three nearest shadow views (1,084 in the farthest on its turn). Of the 923 batches:

- **350 are terrain chunks**, each its own mesh and its own binding, 141,632 triangles between them (3% of the 4.3 million drawn), and each is drawn again in the prepass and the shadow views;
- the other 573 are 212 meshes with 148 sets of textures; sorting near to far in distance bands makes 923 batches of what would be 616;
- 577 batches hold one instance.

Sampled inside `finish` with one encoder (56 samples), 62% was render passes and 34% acceleration structures. Within the passes the largest single cost was binding: wgpu notes every texture of a newly bound group for each draw (`flush_bindings_helper`, about a fifth). A draw call costs about 0.9 µs. Merging the terrain's chunks would take about 1,400 draw calls off the frame, but since the passes are encoded beside one another and the acceleration structures' part is the longest, that would shorten recording (0.8 ms) by a quarter and nothing else on the frame's thread. It wasn't built: it changes how terrain is cut into chunks, for 0.2 to 0.3 ms.

**3. Ray traced shadows and ambient occlusion: one query a ray.** Both asked twice for every ray: once for solid items and once for cut-outs, because a cut-out very near the surface isn't counted (`cut_out_near`) and each query had its own nearest distance. One query finds both. A solid item's triangles are committed by the traversal as they come (its BLAS is opaque); a cut-out's come as candidates, and the shader confirms them only past that distance and where the texture isn't cut out. For ambient occlusion the nearest hit of one query is the nearer of the two answers. For shadows only whether a ray is blocked matters, except for soft shadows' edge, which is as wide as the blocker is far: there a ray that a cut-out stopped asks once more for a solid item, so the distance is the one two queries gave (`ray_shadows.wgsl`'s `first_blocker` and `blocker_distance`, `ray_ao.wgsl`'s `nearest_hit`).

| The still view at 2560 × 1405, GPU-bound, pinned (medians of two runs each) | Two queries a ray | One |
|---|---|---|
| Ray traced shadows | 6.57 ms | **4.66 ms** |
| Ambient occlusion | 3.39 ms | **2.54 ms** |
| GPU frame | 17.21 ms | **14.53 ms** |
| As shown | 57.5 a second | **68.0** |

Where the rest goes, by leaving one step out at a time at that size: the sun's rays 3.1 ms (4 a pixel), the fog's air 1.1 ms (4 rays for each of 921,600 cells), smoothing the soft shadows 0.45 ms; ambient occlusion's rays 2.3 ms (8 a pixel at half size), the screen-space pass under them 0.18 ms and its blur under 0.05 ms; acceleration structures 1.75 ms.

- **Denoising isn't where the time is.** The two smoothing passes and the blur are 0.5 ms of 7.2. The rays are. Fewer rays with their results kept from frame to frame would be the next step, and it changes how shadows and ambient occlusion look when things move, so it's Paul's to decide, not done here.
- **BVH traversal: fewer, bigger structures don't trace faster.** With every swaying plant bent into the shared bundles (16,953 of them instead of the 4,052 a frame's budget allows), the TLAS had a third of its instances and the trace passes were within 3% (ray traced shadows 4.64 → 4.50 ms, ambient occlusion 2.52 → 2.48). What fewer instances do buy is the frame's thread (below).

**4. What wgpu does for every instance, every time the scene is bound.** At submit wgpu walks every instance of a TLAS for each pass that bound it, twice (`validate_acceleration_structure_actions`, `set_acceleration_structure_dependencies`), whether or not it was rebuilt. Six passes bound it; three trace. The sun's rays and the air's now share one compute pass and one binding, and the two smoothing passes have a layout without the scene (`ray_shadows.rs`, `only_the_passes_that_trace_bind_the_ray_traced_scene`). **Submit 2.37 → 1.73 ms.** The same experiment as above (a third of the instances) took submit from 2.19 to 0.90 ms: instances are what it costs. Building every plant's bundle each frame cost 7.2 ms of GPU, so that isn't the way; still plants would need bundles built once and kept, which is memory (every plant's triangles on their own, where they share a mesh's now) and a rule for plants that change level of detail. That is the next architectural step on this side and wasn't started.

Also here: each traced item's BLAS was looked up twice a frame, now once (`RayScene::prepare`, 4.07 → 3.97 ms).

**5. A pace the player couldn't leave.** Checking a covered window (below) showed a heavy game at 1000 × 600 held at 39.9 frames a second with the keyboard's focus, where it draws 91. The player asked the display for its refresh when it made its window, and under Wayland a new window isn't on a display yet: it got no answer, took 60 Hz, and on a 240 Hz display frames 17 to 25 ms long then never counted as missing the refresh (the pacer starts frames early after eight in a row at least 1.5 refreshes long). The player now asks again every 30 shown frames, as the editor did (`window::asks_refresh_on`). **As played at 1000 × 600: 43.8 → 97.9 a second;** the tour at 1996 × 998 as played 70.8 → 77.0; a light scene 239.2 to 239.8 as before.

**6. A window's state, checked under GNOME** (`AXCENITY_BENCH_TIMELINE`'s counts file has each frame's time, focus and whether it was drawn; a second game was opened over the first for 14 seconds; the tour as played in a 1600 × 900 window, and a 1000 × 600 one for the covered row):

| The heavy game's window | Frames | The card |
|---|---|---|
| Has the focus | 86 to 94 a second, drawn | busy |
| Seen, another window has the focus | 30.5 a second, drawn, waiting for the display | 64% busy at 1,840 MHz, 82 W |
| Wholly covered by another window | 30.0 steps a second, none drawn, from 0.4 s after it was covered | none of its own |
| Minimized (second round) | 30 steps a second, none drawn | idle |
| Focus back | 91 a second within 0.1 s | busy |

**7. The cores with the large cache.** The regression check runs pinned, and pinned to the 9950X3D's eight cores with the 96 MB cache the same build was a third faster than my unpinned runs:

| Everything on, 1996 × 998 | Any core (as launched) | The V-cache cores (`taskset -c 0-7,16-23`) | The other eight (`8-15,24-31`) |
|---|---|---|---|
| Still view, as shown | 66.3 a second | **90.9** | 67.4 |
| Still view, frame's thread | 14.00 ms | 10.29 ms | 13.73 ms |
| Debug editor at its start camera | 52.0 a second | **65.8** | — |

The frame's thread walks tens of thousands of items and several hash tables each frame, and that work is bound by cache. This machine's `amd_x3d_mode` is `frequency`, which makes the kernel prefer the eight cores without the large cache, so that is where a game's frame lands. On Windows AMD's driver moves games to the V-cache cores; here it is one line as root (`echo cache > /sys/bus/platform/drivers/amd_x3d_vcache/*/amd_x3d_mode`) or GameMode's setting for it. The engine doesn't choose cores itself: a process that pins itself hands the limit to every program it starts (the editor's cooks and exports use all the cores), so that needs deciding, not slipping in. The tour is limited by the GPU and gains little (77 → 79).

**Where it stands** (1996 × 998, release player, everything measured the same way as the second round's table: one run each, as launched; then the regression check's medians of three runs pinned to the V-cache cores):

| Configuration | Target | Tour, second round → now | Still view, second round → now | Pinned: tour / still |
|---|---|---|---|---|
| Standard Ultra | 120+ | 119.2 → **132.6** | 102.0 → **114.7** | 143.7 / 179.5 |
| Advanced lighting (Ultra with real-time GI, no rays) | 90+ | 118.3 → **131.7** | 104.4 → **116.5** | 143.0 / 178.6 |
| Ray traced shadows and ambient occlusion | 90+ | 74.0 → **76.4** | 59.3 → **71.2** | 79.8 / 96.3 |
| Heavy ray traced scenes (everything on) | 60+ | 71.8 → **77.2** | 57.5 → **68.3** | 79.3 / 91.1 |
| Optimized Desert Homestead (everything on) | 100 | 71.8 → 77.2 | 57.5 → 68.3 | 79.3 / 91.1 |

The debug editor at its start camera and default window: 26.7 at the start of the audit, 44.4 after the second round, **52.0** now. Nothing was turned down. The ray traced rows are still short of 90 and 100 wherever the GPU is the limit, which the tour is: its GPU frame is 12.4 ms, of which the scene pass is 4.2 (the same without ray tracing: the clouds' march, a known cost), ambient occlusion 2.5, ray traced shadows 2.2 and acceleration structures 1.3.

**What is left, largest first.** On the frame's thread with ray tracing on (13.6 ms in the still view): the ray traced scene's walk and wgpu's per-instance TLAS work, about 7 ms across preparing, encoding and submitting, for which bundles of still plants built once are the lever; then the per-plant work of culling, materials, motion history and uploads (about 4 ms), for which each cluster's bounds and instances kept from frame to frame are. On the GPU: the rays themselves (5.4 ms of shadows and ambient occlusion at 2560 × 1405), the clouds, and building acceleration structures for swaying plants.

**Regression benchmarks.** `benchmarks/baselines.json` has the four configurations on the tour and on the still view as `homestead-tour-*` and `homestead-still-*` (the settings are `benchmarks/features/owner-*.json`; a `still:` scene is the project with no script run and no Sequence playing), recorded for this machine under Fedora as `desktop-9950x3d-fedora`, with the frame as shown, the GPU's frame, the size drawn at and what each frame drew ([benchmarks.md](../benchmarks.md#regression-check)).

## Overgrown Street with ray tracing: still groups, distances and cascades (2026-11-27)

The owner's editor ran Overgrown Street at 16 to 18 frames a second with every ray traced effect on (Ultra, TAA, real-time GI, ray traced shadows, ambient occlusion and reflections at High) in his 1996 × 703 viewport. The release player at the same settings and size was 19 frames a second, so it was the scene, not the editor: about 113,000 items within the ray tracing distance (98,000 of them under 256 triangles), each a TLAS instance, cost 14.5 ms of the ray scene's work, 6.3 ms of wgpu's submit and 3.8 ms of its encoding a frame; on the GPU ray traced shadows took 16.5 ms and the shadow maps 11.3 more. What changed ([ray-tracing.md](ray-tracing.md)): small still items grouped into BLASes by the square of ground (one instance a group), swaying plants bent only within 32 studs, the ray scene's item loop and the groups' fingerprints on the renderer's threads, the nearest cascades left out where traced shadows are used whole, and (the owner's choice) small plants' traced shadows to 55 studs and cut-outs' holes to 60.

Release player, `--bench 300` at 1996 × 703 with the owner's graphics (`ray_tracing` on), pinned to the V-cache cores, the old rules by the switches (`AXCENITY_NO_RAY_GROUPS=1 AXCENITY_NO_RAY_REACHES=1 AXCENITY_NO_CASCADE_SKIP=1`) and the new ones alternating, two rounds each (they agreed within 0.7 ms):

| | Old rules | New |
|---|---|---|
| As shown | 19.8 fps (1% low 18.6) | 22.8 fps (1% low 20.1) |
| Frame's thread | 43.5 ms | 30.1 ms |
| Ray traced scene | 14.6 ms | 8.6 ms |
| Submit / encoding | 6.3 / 3.8 ms | 1.3 / 2.3 ms |
| GPU's frame | 50.2 ms | 43.5 ms |
| Shadows (maps) | 11.5 ms | 5.3 ms |
| Ray traced shadows | 16.6 ms | 16.2 ms |

Each change alone (lifting one from the new rules, one run each, pinned): grouping is −7.5 ms of the thread and +1.1 ms of GPU (bigger BLASes trace a little slower); the sway distance −7.5 ms of the thread; the cascades −6.9 ms of GPU; the holes' distance −2.4 ms of GPU and −3.1 of the thread; the small plants' distance nothing measurable. The debug editor (`AXCENITY_FRAME_LOG`, `AXCENITY_WINDOW_SIZE=2560x1440`, pinned) went from 18.4 to 24.7 frames a second: viewport 48.4 → 31.3 ms of CPU, GPU 45.1 → 36.3 ms. Stills (`shot` with the scene's ScatterRules made, four views at 1996 × 703, 64 frames each, the same frames before and after): grouping, the sway distance, the small plants' distance and the cascades change no pixel by more than 3 levels; the holes' distance darkens what's past it (above, [ray-tracing.md](ray-tracing.md)). The ray scene's work was found by timing its phases inside `RayScene::prepare` (item loop 6.5, bending 4.8, layout and tables 4.5, TLAS and builds 3.7 ms before).

## Overgrown Street's draws and hidden items, measured (2026-11-27)

Before batching more or building GPU-driven drawing and a Hi-Z pass (the owner's brief of 2026-11-27, which set a measurement first), both were measured on Overgrown Street at the owner's 1996 × 703 Ultra, pinned to the V-cache cores.

**Where the draw calls go** (the still tool's `SHOT_COUNTS`, four views, non-RT): 6,000 to 6,800 a frame: the scene pass and the prepass about 2,000 each, the cut-outs' depth fill about 490 and the shadow views about 2,000 (cascade 4 alone 1,500). Of the scene pass's 2,000, about 970 batches draw 57,000 to 67,000 scattered plants (already instanced by mesh, material and distance band) and about 1,130 draw the 1,190 or so Parts and MeshParts, nearly one a draw. In the player the frame's thread spends about 4 ms of 20 recording, encoding and submitting them while the GPU takes 26.5 ms (non-RT) or 43 ms (RT): draw calls aren't what holds the frame.

**What hidden items cost.** `AXCENITY_OCCLUSION_PROBE` (`renderer/occlusion_probe.rs`, a measuring switch): on the frame it names it reads the prepass's depth back to the CPU once, builds its farthest-depth pyramid, and from then on leaves out of the camera's passes every draw whose bounding sphere's box is wholly behind it (shadow casters kept). For a still camera that is what a perfect occlusion test would cull, and the pictures confirm it: the four views with it on match those with it off pixel for pixel but for TAA's noise (at most 0.001% of pixels more than 8 levels apart). `--bench 300` at the spawn (one run each, the probe from frame 30, so a tenth of the frames are without it):

| | Off | Probe on (from frame 30) |
|---|---|---|
| Draws the depth hides | | 19,466 of 57,086 items |
| Draw calls | 6,514 | 5,950 |
| Non-RT: as shown | 37.2 fps | 39.2 fps |
| Non-RT: GPU prepass / scene | 7.49 / 6.53 ms | 6.84 / 5.85 ms |
| RT: as shown | 22.9 fps | 23.5 fps |
| RT: GPU prepass / scene | 7.46 / 6.01 ms | 6.84 / 5.32 ms |

Scaled to every frame, hidden items cost about 1.45 ms of the GPU's 26.5 (non-RT) and 43 ms (RT); the shadows don't change (casters are kept). The still tool's four views agree: the depth hides 14,400 to 32,700 draws, and the prepass and scene take 5 to 10% less (most behind the houses, the low view). So hidden items here are a tenth of the prepass and scene, not most of them, and a third of the hidden draws are plants that cost little each: the case [above](#texture-budget-and-occlusion-culling-graphics-phase-15) names for reconsidering occlusion culling isn't met, and GPU-driven drawing with a two-phase Hi-Z pass stays unbuilt (the owner's decision of 2026-11-27: only if the numbers met the threshold). What the frame is spent on instead: scattered plants' vertices in the prepass (4.7 ms) and their pixels in the scene (3.0 ms) and cut-outs' depth (2.0 ms), the far cascade (10.4 ms on the frames it's drawn), and with ray tracing on, traced shadows (16 ms). The probe stays as a switch, for the next scene that might.

## Overgrown Street: plants without shadows, half-resolution traced shadows, probes (2026-11-27)

The owner's decisions after the measurement above: shadow cuts only as choices (the default picture kept), probes and a refit grade for the scene, and two shader fixes. What each did, release player `--bench 300` at 1996 × 703, the owner's Ultra settings, pinned to CPUs 0-7,16-23, two alternating rounds (`before` = the engine and project before; `code` = the new engine on the old project; `after` = both new):

| | Before | Code only | After | After, RT at Half |
|---|---|---|---|---|
| Non-RT: as shown | 37.0 / 36.8 fps | 37.0 / 36.7 | 37.9 / 37.9 | |
| Non-RT: GPU frame | 26.5 ms | 26.4–26.5 | 25.9 | |
| Non-RT: shadows (cascades 1/2/3/4) | 11.5 (1.14/2.67/2.56/5.16) | 11.5 | 10.7 (0.85/2.32/2.40/5.12) | |
| Non-RT: shadow casters | 60,778 | 60,778 | 29,273 | |
| RT: as shown | 22.9 / 22.9 fps | 22.8 / 23.0 | 24.5 / 24.6 | 27.7 / 27.6 |
| RT: GPU frame | 43.3 ms | 43.1–43.3 | 40.3 | 32.8 |
| RT: ray traced shadows | 16.1 ms | 16.1 | 12.8 | 5.5 |
| Scene pass | 6.56 (non-RT) / 6.00 (RT) | same | 6.78 / 6.22 | 6.12 |

- **`CastShadow` off on five small ground-cover rules** (`ScatterRule.CastShadow`, a project choice: Nettles and weeds, Fine grass, Leaf litter, Mossy ground, Forest grass; Tall grass still casts) halves the shadow casters and takes 3.3 ms off traced shadows: fewer leaf cards for the shadow rays to pass through. Cascade 4's casters are trees, so it barely moves. Without ray tracing, contact shadows still ground those plants; with traced shadows contact shadows are skipped, so they cast nothing at all there.
- **Half-resolution traced shadows** (`ray_traced_shadow_resolution`, Half at every preset since 2026-11-28; Full at Ultra until then) traces a quarter of the rays: 12.8 → 5.5 ms.
- **Micro-shadowing and glossy leaves** cost nothing measurable (code only, the same project: the same frame rates and passes within 0.1 ms).
- **Desert Homestead, the regression check** (the same engines, its owner's Ultra and every-ray-traced-effect settings, 1996 × 998, two rounds): 174.7 / 175.0 → 175.1 / 173.6 fps and 96.8 / 97.1 → 96.8 / 96.9, every pass within 0.02 ms. Its stills change in 0.06 to 0.5% of pixels (wood grain and joints in the sun, from micro-shadowing on the parts' occlusion maps). A first version that also shaded terrain by its layers' height maps darkened its dirt layer across whole views (27% of pixels in a view from above, mean −6 levels), not only in cracks, so the terrain was left as it was.
- **Probes** (4 light probe volumes, 3,284 probes, and 4 reflection probes): the scene pass is 0.2 ms dearer (probe blending in place of the sky's diffuse light). The bake takes about 30 minutes in the debug editor.

None of this reaches the owner's targets (120 fps for standard Ultra, 90 with traced shadows and ambient occlusion); the frame stays GPU-bound by the prepass's scattered plants (4.8 ms), traced shadows and ambient occlusion with ray tracing (12.8 and 6.0), and cascade 4 (5.1 on alternate frames) without.

## Levels of detail, inspected

**How levels are picked today** (`renderer/lod.rs`, unchanged by this section): by **projected size**, the share of the view's height an item's bounding sphere covers, not by world distance or screen-space error. Below 30% an item draws its first simpler copy, below 12% its second (`LOD_SCREEN_SIZES`); it switches back once 15% past the threshold (hysteresis; the level is kept between frames by the item's ID and mesh, and for a group's items, `DrawList::chunks`, with the group by the item's place in it, `GroupStates`, since 2026-11-28: one lookup and lock a group, not a map lookup and lock an item, which were 0.55 ms of Overgrown Street's frame thread for 59,000 items), and a switch cross-fades over 30 frames by dither (12 before 2026-11-14, which read as a pop). All primitives of a model share one decision (the sphere of the part's box). Shadow views draw the level the camera picked; only the level fading in casts. Levels come from the file (`_LOD1`, `_LOD2`) or are simplified at load to half and a quarter of the triangles with meshoptimizer, a copy that saves less than a fifth on the level before being dropped ([models.md](models.md)).

**What works:** no popping (dithered fades), no flicker on a threshold (hysteresis, `a_model_sitting_on_a_threshold_keeps_its_level`), switching per model rather than per primitive, levels uploaded only when drawn, and simplified copies cached on disk.

**Limitations, measured on the patches:**

- **Leaf cards don't simplify.** Edge collapse can't merge disconnected quads, so the hazel and juniper shrubs' leaves (840–4,000 triangles) get no copies at all, while their wood does (`lod_report_project`). Grass and other small painted models are under the 256-triangle floor. Their saving comes from the cull distance and `SHADOW_REACH`, not from levels.
- **Thresholds are one size for everything.** A rock and a tree switch at the same screen size; nothing lets an asset category pick its own (`LodReport` and the inspector are written per model, so a per-category table can be added where `lod_level` reads `LOD_SCREEN_SIZES`).
- **Size, not error.** Two copies that look alike switch as early as two that don't; `LodReport`'s Almost the Same warning catches file-made copies like that.
- **Big trees stay at LOD0 in a small scene.** The patches' oaks fill a third of the view or more from anywhere on the patch, so they never switch, and their LOD2 is still 195k–260k triangles.

**The tools:**

- **LOD Levels legend** (Viewport > View Mode > LOD Levels): the viewport's corner names each color (LOD0 green, LOD1 yellow, LOD2 orange; grey has no copies), how many items in view draw each (`RenderStats::lod_items`) and the thresholds. The Overdraw view has a legend too, with a **Measure** button that reads the frame back as `OverdrawStats`.
- **LOD Inspector** (View > Panels > LOD Inspector, `lod_inspector.rs`): for the selected MeshPart, its mesh, camera distance, size on screen, the thresholds, the level it's drawn at (`TextureRenderer::lod_level_of`) and the level its size picks, then each level's triangles, vertices, share of LOD0 and estimated GPU memory, and warnings. **Preview** buttons draw that one part at LOD0, LOD1 or LOD2 in the viewport whatever its size (`TextureRenderer::preview_lod`, no fade), to compare outlines; Auto goes back, and the preview ends when the selection changes or the panel closes. Nothing in the world or the model changes.
- **`assets::LodReport`**: per primitive, its levels' triangles, vertices, share and estimated bytes (position, normal, UV and tangent per corner, 32-bit indices), and warnings, which are advice, never a rejection: no copies for a primitive of 5,000 triangles or more, a level keeping over 85% of the one before, one keeping under 10%, the simplest level still 50,000 triangles or more, and a level whose box shrinks by more than 10% on a side (its outline changes). It's what an import or a generated model can be checked with: the same report applies to any model, whatever its kind.
