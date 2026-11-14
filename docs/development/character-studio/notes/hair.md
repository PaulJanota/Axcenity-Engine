# Hair and groom research for Axcenity Character Studio

Researched 2026-11-14. Research only; nothing in the repository was changed.
"Opened" means the page or PDF text was read in this session. "Unverified" means it was not opened or could not be confirmed.
Visual-inspiration claims are kept in their own section at the end of each question where relevant.

Local context checked (read only): workspace pins `wgpu = "30"` (lockfile `wgpu 30.0.1`, `naga 30.0.1`, `static-dxc` feature). `crates/render` requests only `TIMESTAMP_QUERY`, `TEXTURE_COMPRESSION_BC` and optional `EXPERIMENTAL_RAY_QUERY`; no subgroup or 64-bit atomic features are requested today.

---

## 1. Unreal Engine Groom

Docs opened (dev.epicgames.com, labelled "Unreal Engine 5.9 Documentation" by the fetcher):
- Interpolation: https://dev.epicgames.com/documentation/unreal-engine/groom-interpolation-in-unreal-engine
- LOD: https://dev.epicgames.com/documentation/unreal-engine/setting-up-level-of-detail-for-grooms-in-unreal-engine
- Binding: https://dev.epicgames.com/documentation/en-us/unreal-engine/setting-up-bindings-for-grooms-in-unreal-engine
- Physics: https://dev.epicgames.com/documentation/unreal-engine/enabling-physics-simulation-on-grooms-in-unreal-engine
- Strands: https://dev.epicgames.com/documentation/unreal-engine/groom-strands-in-unreal-engine
- Cards and meshes: https://dev.epicgames.com/documentation/unreal-engine/setting-up-cards-and-meshes-for-grooms-in-unreal-engine
- Materials: https://dev.epicgames.com/documentation/unreal-engine/groom-materials-in-unreal-engine
- Alembic spec: https://dev.epicgames.com/documentation/en-us/unreal-engine/using-alembic-for-grooms-in-unreal-engine
- Debugging: https://dev.epicgames.com/documentation/unreal-engine/debugging-grooms-in-unreal-engine
- The overview page (hair-rendering-and-simulation-in-unreal-engine) is only an index; its subpage links were unresolved template placeholders.

### Asset model
- Guides vs rendered strands. Guide Type: "Imported" (from the file), "Generated" (decimated from strands, "Hair to Guide Density"), or "Rigged" (rigged guides with N curves and N points/bones). Guides carry skinning and simulation motion; rendered strands follow them.
- Interpolation build: Quality Low (nearest neighbour, for short hair), Medium (curve shape matching, limited range), High (curve shape matching, unlimited, for medium/long hair). Distance metric: Parametric, Root, Index or Distance. "Randomize Guide" perturbs guide choice to break clumping; "Use Unique Guide" uses one guide per strand to cut cost.
- Deformation: per-LOD RBF interpolation (global RBF preserves the groom shape under large skeletal deformation; needs a binding and skinning), or Rigid Transform / Offset Transform / Smooth Transform / No Skinning.
- Groups: Alembic `groom_group_id`; each group has its own LOD entries and material slot.
- LODs: up to 8 entries per group, each with Screen Size, geometry type and binding type. Strands (skinning, RBF, simulation, own Curve Decimation / Vertex Decimation / Angular Threshold / Thickness Scale), Cards (skinning, RBF, simulation), Meshes (RBF only). LOD Mode Default / Auto (curve count from screen size) / Manual.
- Cards: per entry, mesh + Group Index + LOD Index; Guide type "Generated" (guides through the middle of each card band) or "Guide-Based" (each card vertex skinned to its closest guide). Textures: Depth, Coverage, Tangent, Attributes (RootUV / CoordU / Seed), Auxiliary; "Default" or "Compact" layout. Hair Card Generator is an experimental plugin that converts strands to cards (Dataflow BuildCardSettings node, Num Triangles, Random Seed, AtlasSize). https://dev.epicgames.com/documentation/en-us/unreal-engine/hair-card-generator-for-grooms-in-unreal-engine (search result, not opened).
- Material: must use the Hair shading model. Hair Attributes node outputs U, V, Length, Radius, Seed, Tangent, Root UV, BaseColor, Roughness, Depth, Coverage, AuxiliaryData, AtlasUVs, Group Index, AO, Clump ID (Depth/Coverage/Aux/AtlasUVs are cards and meshes only).

### Binding
- Binding asset links a groom to a skeletal mesh so skin motion drives hair. Only used when the LOD Binding Type is Skinning; rigid attachment to a socket needs no binding.
- Stores per-strand root binding (debug view shows the root and "the corresponding triangle on which the root is bound"). API: `ProjectStrands` "projects strands roots onto a mesh and computes the closest triangle ID and barycentric coordinates" (API reference, via search: https://docs.unrealengine.com/4.27/en-US/API/Plugins/HairStrandsCore/FGroomBindingBuilder/index.html , not opened).
- Num Interpolation Points: RBF samples for global interpolation ("100 samples or less should be good enough").
- Optional Source Skeletal Mesh to transfer a groom authored on another mesh; assumes shared UVs. Without a binding asset, projection happens at runtime with a large GPU startup cost (search snippet of the same page).

### Rendering (partly unverified)
- Strands panel: Hair Width, Root/Tip Scale, "Voxelize" (shadows and environment occlusion from a voxel density volume), "Hair Shadow Density", "Use Hair Raytracing Geometry" (otherwise ray-traced shadows use the voxelization as proxy), "Use Stable Rasterization", "Scatter Scene Lighting".
- Debug: voxel views, `r.HairStrands.Voxelization.Virtual.*` (virtual voxel pages), `r.HairStrands.Visibility.WriteVelocityCoverageThreshold` (velocity written per hair pixel by coverage).
- Visibility: a third-party cvar index lists `r.HairStrands.Visibility.MSAA.SamplePerPixel` (2/4/8) and an experimental `r.HairStrands.Visibility.ComputeRaster` (default 0) with `.ContinuousLOD`. https://indxzero.github.io/ue544cvarwiki/articles/r.hairstrands.visibility.computeraster (search snippet; unverified against engine source).
- Epic tech blog (27 Jan 2020) https://www.unrealengine.com/en-US/tech-blog/an-early-look-at-next-generation-real-time-hair-and-fur : HTTP 403 in this session. Search snippet says light transmission uses Deep Opacity Maps combined with runtime voxelization, and shading uses a dual scattering approximation for multiple scattering. Unverified (page not opened).
- Hair BSDF: Karis 2016, "Physically Based Hair Shading in Unreal" (SIGGRAPH 2016 PBS course), slides opened: https://blog.selfshadow.com/publications/s2016-shading-course/karis/s2016_pbs_epic_hair.pdf . Marschner R/TT/TRT factorisation into longitudinal M and azimuthal N; Gaussian M (says d'Eon's energy-conserving M would be better); R azimuthal = cos(phi/2) with Fresnel and Weta's azimuth-dependent modification; TT: fitted h, IOR fixed at 1.55, constant s=0.35, logistic approximated by a Gaussian in cos(phi); TRT: "brutal" single lobe with constant h. Multiple scattering is "a giant artistic hack": a fake normal (Agni's Philosophy idea), wrapped Lambert, and absorption from light path length derived from an exponential (PCF) shadow value; Karis says it is "missing something, especially for light hair". Environment lighting: widened roughness + artificial occlusion of back paths.

### Simulation
- Niagara Solver: "Groom Rods", "Groom Springs" or "Custom Solver". Strands Size = particles per guide. Bend and stretch constraints "solved during the XPBD loop", projected after it, or both; stiffness in GPa with root-to-tip curves; damping. Collision against physics assets with static/kinetic friction, collision radius, strand viscosity for self-collision. Gravity, Air Drag, Air Velocity (wind). Sub Steps; solver calls at 24 fps; Iteration Count.
- Only guides are simulated (Alembic spec: guide curves are those used for simulation; if absent, a percentage of strands is tagged as guides on import, per search snippet).
- Whether Groom Rods is a Cosserat-rod formulation: unverified.

### Alembic groom schema v1.5 (opened)
- Curves are standard Alembic `ICurves`. Attributes: `groom_guide` (int, constant/uniform, 1 = guide), `groom_group_id` (int32), `groom_root_uv` (float[2] uniform; spherical projection if absent), `groom_width` (constant/uniform/vertex; 1 cm default), `groom_color` (float[3] vertex), `groom_id` (int32 uniform), `groom_closest_guides` (int32[3] uniform), `groom_guide_weights` (float32[3] uniform).
- Required top-level properties: `groom_version_major` = 1, `groom_version_minor` = 5; optional `groom_tool`, `groom_properties`.
- Interpolation data can be supplied externally (3 closest guides + weights per strand). Card group attribute for the generator is `groom_group_cards_id` (forum report).

---

## 2. AMD TressFX

- Repo: https://github.com/GPUOpen-Effects/TressFX . Licence: MIT (`license.txt`; file headers "Copyright (c) 2019 Advanced Micro Devices", MIT text). GitHub API: licence MIT, not archived.
- Version / maintenance: latest tag and release v4.1.0, 2020-01-24 ("Update from TressFX Sushi/4.0 to Cauldron/4.1"). Branches 3.1.1 (2016-06-15), 4.0 (2018-04-20), 4.1 and master (2020-01-24). No commits since January 2020: effectively unmaintained as a standalone library.
- TressFX 5.0: Unreal-only engine patches for UE 4.26/4.27 and UE 5.4 (branch `TressFX5-5.4.4` on GPUOpenSoftware/UnrealEngine), gated behind Epic GitHub access. UE5 post "Originally posted: August 4, 2025": https://gpuopen.com/learn/tressfx-5-0-unreal-engine-5-update/ . New in 5.x: Lumen SW/HW RT support, "deep shadow approximation", continuous LOD, IndirectDraw clear optimisation, Marschner shading, up to 16 binding bones. Licence of the 5.0 patch is not stated on that page; https://gpuopen.com/tressfx/ lists "Open source, MIT license" for TressFX generally.
- 4.1 README: Cauldron sample (DX12 + Vulkan), SM6; "a self-contained solution for hair simulation" and "a reference renderer for hair"; simulation shaders "should be self contained enough to be integrated into another codebase without substantial changes". UE 4.22 integration patch. Maya exporter (`tool/Maya/TressFX_Exporter.py`); no Blender exporter in the repo.
- Simulation pipeline (`src/TressFX/TressFXSimulation.cpp`, `src/Shaders/TressFXSimulation.hlsl`, opened): `IntegrationAndGlobalShapeConstraints` (per vertex: bone skinning of rest pose, Verlet integration, teleport reset, global shape constraint toward skinned rest) -> `CalculateStrandLevelData` (per strand) -> `VelocityShockPropagation` (per vertex; propagate root motion to avoid over-stretch on fast moves) -> `LocalShapeConstraints` (per strand, N iterations; bending/twisting relative to local frames) -> `LengthConstriantsWindAndCollision` (per vertex: wind from four directions, distance constraints, optional capsule collisions) -> `UpdateFollowHairVertices` (follow hairs copied from guide hairs with offsets). 64-thread groups, groupshared positions. SDF collision (`TressFXSDFCollision.hlsl`) with compute marching cubes and SDF generation from a dynamic mesh (4.0).
- Rendering: strands expanded to camera-facing quads in the vertex shader (`GetExpandedTressFXVert`, `expandPixels = 0.71` for AA, thin-tip ratio); coverage computed per pixel from the distance to the strand centreline. Two OIT paths: PPLL (`TressFXPPLL.hlsl`; per-pixel linked lists, keeps the nearest `KBUFFER_SIZE 8` of up to `MAX_FRAGMENTS 512`, sorted and shaded in a full-screen pass) and ShortCut (4 passes: store 3 front fragment depths + product of (1 - alpha); resolve farthest near depth; shade near fragments with early-Z, accumulate weighted colour; full-screen weighted average blended with the 1 - alpha product). Shadows: shadow map + fibre-count attenuation (`ComputeShadowAttenuation`).
- Shading in 4.1 sample: Kajiya-Kay diffuse + two shifted specular lobes approximating Marschner R and TRT (`ComputeDiffuseSpecFactors`), not a full Marschner BSDF.
- Asset format `.tfx` (`TressFXFileFormat.h`): header `{ float version; uint numHairStrands; uint numVerticesPerStrand; uint offsetVertexPosition; offsetStrandUV; offsetVertexUV; offsetStrandThickness; offsetVertexColor; uint reserved[32]; }`, fixed vertex count per strand; plus `.tfxbone` (skinning) and `.tfxmesh` (collision mesh).
- Verdict: MIT so code can be reused, but it is HLSL/C++ around a DX12/Vulkan sample framework, frozen since 2020, with Kajiya-Kay-style shading and memory-hungry PPLL. Best used as a reference (port the simulation constraints to WGSL), not as a dependency.

---

## 3. NVIDIA HairWorks (historical/visual reference only)

- Technique (NVIDIA Witcher 3 guide, opened: https://www.nvidia.com/en-us/geforce/news/the-witcher-3-wild-hunt-graphics-performance-and-tweaking-guide/): "tens of thousands of DirectX 11, tessellated hair strands"; Geralt about 30,000 hairs, up to 115,000 in close-ups; MSAA applied to HairWorks hair by default independent of game AA; hair counts scaled down with distance; cost 10-20 fps in that era. AMD reported a 64x tessellation factor (search result: https://wccftech.com/amd-announces-witcher-3-hairworks-performance-fix-catalyst-control-center-driver-coming-boost-tessellation-performance/ , not opened). Users report 8x hair MSAA default (forums, unverified).
- HairWorks 1.2 release notes (archived, opened): https://archive.docs.nvidia.com/gameworks/content/artisttools/hairworks/1_2/releasenotes.html : DX11; DX12 "NOT AVAILABLE"; guides must sit on growth-mesh vertices, one guide per vertex; alpha sorting unsupported; shadow flicker.
- Source: released in 2016 to GameWorks GitHub members under NVIDIA's GameWorks licence (press reports, e.g. https://wccftech.com/nvidia-gameworks-hairworks-github-release/ , not opened); critics noted the licence is revocable. `NVIDIAGameWorks/HairWorks` is not publicly accessible (404 to an unauthenticated query).
- Status: no explicit deprecation notice found. NVIDIA's legacy hub describes its contents as "still available but no longer in development or supported" but does not list HairWorks. NVIDIA's current strand effort is RTX Hair (question 4). Treat HairWorks as legacy and closed.
- Visual inspiration (not technical evidence): what made it read well was many thin strands per head generated from few guides, strong MSAA so strands stay thin and continuous, root-to-tip width/colour taper, and dynamic response to wind and motion.

---

## 4. Other production references

### Frostbite (EA)
- Tafuri, "Strand-based Hair Rendering in Frostbite", SIGGRAPH 2019 Advances (slides opened): https://www.advances.realtimerendering.com/s2019/hair_presentation_final.pdf . Authoring Maya/XGen (or ZBrush/Houdini) NURBS -> offline tessellated points + constraints -> runtime sim -> triangle strips. Sim: points + constraints; Eulerian grid + Lagrangian points for friction, volume preservation, aerodynamics. Single scattering: Marschner far-field, Gaussian M per lobe, R from Karis16, TT with A at h = 0 and a 2D LUT (Gaussian fit per azimuthal slice over roughness and outgoing angle), TRT Karis distribution with a roughness scale factor. Multiple scattering: Dual Scattering (Zinke 2008) with Deep Opacity Maps (4 layers) also used for shadows; Beer-Lambert density fallback. Rendering: strips widened to pixel size cause "spaghetti" look and overdraw; MSAA alone too costly; visibility buffer + MSAA rasterisation, sample deduplication (~2x), screen-space shading. PS4 900p, all strands: long hair 10k strands/240k points physics 4.7 ms + render 4.5 ms; short 15k/75k physics 0.4 ms + render 1.9 ms. "Simulating only 1/10th of all strands and interpolating" gave about 5x.
- Taillandier & Valdes, "Every Strand Counts: Physics and Rendering Behind Frostbite's Hair", Digital Dragons 2020: slides not found (unverified). Cited by Indiana Jones and RE4 talks as the basis of their software rasterisers.
- Dragon Age: The Veilguard (EA post, 12 Nov 2024, opened): https://www.ea.com/technology/news/strand-hair-dragon-age-the-veilguard . "Bespoke compute software rasterizer"; 50,000 strands per character, >100 hairstyles; max hair length raised from 63 to 255 points; decoupled sim/render tessellation; GPU sim ~2 ms, spikes to ~5 ms, disabled off-screen/distant; "hero shadows" per hair object per important light, with scalable decimation; opaque + transparent passes with a transparent-hair depth texture and stencil ("Layered" transparency); hair skips the upscaler and renders between min/max resolutions; budgets 6.5 ms @30 fps, 3 ms @60 fps for eight assets; ~128 MB fixed + 300-600 MB; Xbox Series S and low PC settings swap to Card Hair assets. Microsoft article 5 Dec 2024 (opened) adds clipping with armour/shields as the hardest sim problem.
- IP: EA patent US11551387B2 "Systems and methods for hair rasterization" (inventor Yasin Uludag, priority 2020-09-16, granted 2023-01-10, active, anticipated expiry 2041-09-10): https://patents.google.com/patent/US11551387 . Claim 1 (paraphrased): cull segments against per-tile conservative opaque depth, bin into screen tiles, cluster per tile by depth, rasterise the nearest non-empty cluster with per-pixel colour/opacity accumulation, composite tiles as a hair overlay. Not legal advice; any compute rasteriser design should be reviewed against it.

### MachineGames, Indiana Jones and the Great Circle (strongest evidence for a compute rasteriser without mesh shaders)
- Kulikov, SIGGRAPH 2025 Advances (slides opened): https://www.advances.realtimerendering.com/s2025/content/Strand%20Hair%20in%20IJGC%20-%20Final%20Slides%20(Post-Conference).pdf
- Budget ~2 ms sim + render worst case at 60 fps on Series X/S, PS5, RT desktop GPUs; strands used everywhere ("No card-based fallback for models that use strands"); artists style directly in XGen Interactive Groom.
- LOD: randomised strand order, take a prefix of the strand buffer and scale thickness [Taillandier20]; max 32k strands (low/Series S/Steam Deck) or 65k (high); per-frame cap 2000 segments per 64x64 tile and 2 million segments total; skip every other vertex for far models (shading only).
- Sim: PBD based on Müller 2012 DFTL; all strands and vertices simulated; skinned to head joint; prebuilt head SDF (0.25 MB/head); no body collision; outputs density voxel grid; optional per asset.
- Skinning to triangles: per strand root `{ idxTriangle; barycentrics (packed); triangleNormal (octahedral); triangleTangent (octahedral) }` = 16 bytes per mesh LOD (80 bytes for 5 LODs). Post-transform clip-space cache 32 MB for 2M vertices.
- Culling: occlusion against 1/16 depth, frustum, "Fedora culling"; visibility bits with one atomic per wave.
- Rasterisation: conservative software rasteriser on async compute, one lane per segment as a trapezoid; tiled visibility atlas (64x64 tiles, 2M pixels, 32 MB) with downscale when full; 3 ordered layers via chained `atomicMin` + additive coverage via `atomicAdd`; 32-bit payload = 10-bit normalised linear depth within model bounds + 22-bit segment id; reconstruct total coverage `x - 0.5 x saturate(0.5 x)`; early cull against layer 3 with non-atomic loads; loop flattening + LDS pixel request stacks cut divergence from 55% to 20%; global counting sort of segments + Morton sort in groups. Uses subgroup ops for compaction.
- Shading: per vertex (strand space), Marschner R/TT/TRT via Karis16 + Tafuri19, multiple scattering approximation, per-lobe artist scalers. Self-shadowing: rejected deep opacity maps (memory, extra raster per light); tried density-volume raymarching (needs up to 256^3) and stochastic SH probes (~6 ms Series S); shipped offline-baked ray-traced visibility per vertex in clamped L1 SH of "number of scattering events" (bias misses to -5). Hair -> skin shadows by raymarching a 48^3 density volume (6 steps, ~0.3 ms with AO); player hair shadow via a sculpted proxy mesh in shadow maps.
- Composition: one camera-facing quad per hair piece, sorted back-to-front with other transparents, writes depth and motion vectors (reuse cached post-transform positions).
- Timings (async compute off): Series S 2.39 ms, Series X 1.94 ms, PS5 1.95 ms, PS5 Pro 1.64 ms; async hides ~0.5 ms. Memory ~131 MB fixed on low spec; 10-30 MB per LOD0 asset.
- Later: NVIDIA RTX Hair (LSS) path-traced hair added on RTX 50 (press, not opened: https://80.lv/articles/nvidia-s-rtx-hair-to-make-debut-in-indiana-jones-and-the-great-circle).

### Capcom, Resident Evil 4 (RE ENGINE)
- "Resident Evil 4 Hair Discussion" (RE:2023), slides opened via docswell: https://image.docswell.com/s/CAPCOM_RandD/KVVYN3-RE2023 . Hybrid: thick segments drawn with hardware raster as camera-facing billboards; thin hair with a compute software rasteriser (froxel binning, 16x16 tile per group, Multi-Layer Alpha Blending with 8 layers in groupshared memory; 64-bit atomic path with 32-bit fallback). Lighting per strand vertex stored as vertex colour (shared by both paths), Frostbite-style model, Dual Scattering; ESM shadows; 20% of strands drawn as lines into shadow maps; 128^3 forward-scattering voxels. Ornatrix authoring, Alembic import. LOD by shuffled strands + thickening; console shows hair "with 70% off". PS5: ~3.65-4.0 ms raster, ~1.3-2.25 ms lighting for 23k strands / 267k vertices.

### Research references for compute rasterisation without mesh shaders
- Lipp, Jarabo, Wimmer, Bode, "High-Performance Real-Time Implicit Strand-Based Hair Rendering via Software Rasterization", PACMCGIT 9(4) Art. 58, July 2026, CC BY 4.0 (opened): https://www.cg.tuwien.ac.at/research/publications/2026/LIPP-2026-HAIR/LIPP-2026-HAIR-paper.pdf . Hair-mesh bundles (Yuksel 2009) generate strands on the fly; deferred software raster into a 64-bit per-pixel compressed G-buffer with `atomicMin` (24-bit depth), centre + conservative layers, post-filter reconnection; indirect-dispatch LOD; Pekelis-style model fitted to Chiang; deep opacity maps for point lights; GGX-lobe probe lookup + SH diffuse. 4.4x faster than the mesh-shader baseline (3.1 ms vs 13.6 ms, RTX 5080, 1080p, 127k strands). Requirements: compute + indirect dispatch + 64-bit atomic min (buffer fallback if no image atomics). Uses subgroup shuffles.
- Bhokare et al. 2025 (hair meshes with mesh shaders) is the baseline Lipp beats; not opened.
- Huang et al., "Real-time Level-of-detail Strand-based Rendering", EGSR 2025 / CGF 44(4) (abstract via EG digital library): https://diglib.eg.org/handle/10.1111/cgf70181 ; aggregated BCSDF, clusters replaced by thick strands by screen width, up to 13x.
- Jansson et al., "Real-Time Hybrid Hair Rendering", EGSR 2019; code https://github.com/CaffeineViking/vkhr (MIT except assets; last push 2024-02-16): Vulkan rasteriser + volume raymarcher for LOD, ADSM shadows, PPLL.
- Unity "Enemies" (Ciardi et al. 2022): package https://github.com/Unity-Technologies/com.unity.demoteam.hair , Unity Companion License (Unity-dependent projects only), last push 2025-02-11; GPU PBD-family solver with volume density/pressure, LOD, Alembic import, lines/strips/tubes and HDRP compute line renderer. Not reusable for Axcenity.

### NVIDIA RTX Hair / Linear Swept Spheres
- Blog 6 Feb 2025 (opened): https://developer.nvidia.com/blog/render-path-traced-hair-in-real-time-with-nvidia-geforce-rtx-50-series-gpus . LSS = round swept line with varying radius; hardware intersection on RTX 50 (Blackwell); OptiX software fallback, otherwise DOTS (disjoint orthogonal triangle strips) for older GPUs; ~2x faster than DOTS, ~5x less VRAM; NVAPI R570+, Vulkan `VK_NV_ray_tracing_linear_swept_spheres`.
- RTX Character Rendering SDK https://github.com/NVIDIA-RTX/RTXCR : "NVIDIA RTX SDKs LICENSE" (proprietary), active (pushed 2026-09-11). Near-field Chiang BSDF and an Omniverse far-field analytic model; path tracer; requires RTX 20+/RX 7000+/Arc.
- Relevance: path-traced reference and future RT shadows; wgpu has no curve/LSS primitive, so not a near-term target.

### Others
- Square Enix Luminous: no primary hair talk found. Karis 2016 credits the Agni's Philosophy demo for the fake-normal scattering idea. Unverified beyond that.
- Ghost of Tsushima, The Last of Us Part II: no published hair-specific technical material found (TLOU2 SIGGRAPH Asia 2020 cinematic lighting talk mentions hair, content unverified). Uncharted 4: Jiang, "The Process of Creating Volumetric-based Materials in Uncharted 4", SIGGRAPH 2016 Advances (abstract opened at https://advances.realtimerendering.com/s2016/index.html : precomputed self-shadowing for hair/fabric); 99 MB PPTX not opened.

---

## 5. Hair shading models

| Model | Source | Notes |
|---|---|---|
| Kajiya-Kay 1989 | "Rendering fur with three dimensional textures", SIGGRAPH 89 (not opened) | Tangent-based diffuse sin(T,L) + one specular lobe. Cheap; "every material in our game is PBR... except hair" (Karis). Used in TressFX 4.1 with two shifted lobes. |
| Marschner 2003 | "Light Scattering from Human Hair Fibers", SIGGRAPH 2003 (cited by Frostbite/IJGC; https://w1.graphics.cornell.edu/pubs/2003/MJC+03.pdf not opened) | R, TT, TRT lobes; M (longitudinal) x N (azimuthal); cuticle tilt shifts R/TRT. Basis of every modern real-time model. |
| Zinke 2008 | "Dual Scattering Approximation for Fast Multiple Scattering in Hair" (cited) | Global (along shadow path, needs hair count from DOM/voxels) + local scattering. Used by Frostbite 2019, RE4, Epic strands (snippet). |
| d'Eon 2011 | "An Energy-Conserving Hair Reflectance Model", EGSR (cited) | Energy-conserving M. Karis would have liked a cheap version. |
| Pekelis 2015 | Pixar data-driven model (cited) | Logistic/LUT approximations; Lipp 2026 fits it to Chiang. |
| Chiang 2016 | "A Practical and Controllable Hair and Fur Model for Production Path Tracing", CGF 35(4) | Path tracing; Blender Cycles Principled Hair "Chiang" model (manual opened: https://docs.blender.org/manual/en/latest/render/shader_nodes/shader/hair_principled.html , Cycles only; also "Huang" 2022 microfacet model; melanin/absorption/direct colour parametrisations); RTXCR near-field model. |
| Karis 2016 (Unreal) | slides opened | Real-time Marschner approximation; fake multiple scattering from shadow path length. |
| Tafuri 2019 (Frostbite) | slides opened | Improved TT (h=0 attenuation + 2D LUT) and TRT (roughness scale); dual scattering + DOM. |

Forward-renderer practicality: evaluate Karis16 R + Tafuri19 TT/TRT (one 2-channel LUT) per strand vertex or per visible sample; IJGC and RE4 shade per vertex then interpolate, which decouples shading cost from overdraw and fits a forward renderer that composites hair as a transparent layer. Multiple scattering: cheapest is Karis-style absorption from shadow depth/density; better is dual scattering with a density volume (already needed for collision/shadows) or baked per-vertex visibility (IJGC). Melanin/redness parametrisation (Chiang/Blender) is a good artist-facing colour model; base colour can be converted to absorption (RE4 does this).

---

## 6. Hair cards

- Authoring: model/groom strands, bake strand clumps into texture strips, place and sculpt cards, fix normals, tweak materials; IJGC calls the card workflow time-consuming and says card and strand versions cannot be produced automatically from each other in their pipeline. Frostbite 2019: cards cost authoring time and limit hairstyles; card creation usually involves strands anyway.
- Generation from strands: Epic Hair Card Generator (experimental, Dataflow); Strands2Cards (Tojo, Hu, Umetani, Li, SIGGRAPH Asia 2025: cluster strands into wisps, per-wisp texture in a normalised pose, shared textures for similar wisps, differentiable-render fitting of strips; code https://github.com/kenji-tojo/strns2cards with no licence file, PyTorch + CUDA); Zheng et al. 2025 "Auto Hair Card Extraction for Smooth Hair with Differentiable Rendering", TOG 44(6) (cited by Lipp 2026, not opened).
- Atlas content (Unreal): Depth, Coverage (alpha), Tangent (flow), Attributes (RootUV, CoordU = root-to-tip, Seed), Auxiliary, plus generated colour/roughness; group id and per-point colour baked by voxelising strands. Scheuermann 2004: base (stretched noise), alpha (with fully opaque regions), specular shift texture, specular noise texture.
- Alpha handling:
  - Sorted multi-pass blend (Scheuermann, GDC 2004 slides opened: https://web.engr.oregonstate.edu/~mjb/cs557/Projects/Papers/HairRendering.pdf ; SIGGRAPH 2004 sketch https://history.siggraph.org/?p=92998): pass 1 prime Z with alpha test for opaque texels, no culling; pass 2 opaque parts; pass 3 transparent back faces; pass 4 transparent front faces; relies on authoring cards roughly inside-out.
  - Alpha to coverage: needs MSAA; few opacity levels, overlapping layers do not add up (Golus: https://bgolus.medium.com/anti-aliased-alpha-test-the-esoteric-alpha-to-coverage-8b177335ae4f , search snippet).
  - Dithered/hashed alpha + TAA: Wyman & McGuire, "Hashed Alpha Testing" (I3D 2017) for stable stochastic thresholds; Enderton et al., "Stochastic Transparency" (I3D 2010). Fits Axcenity (forward + TAA + depth/normal prepass, no MSAA).
  - OIT (PPLL, weighted, MLAB): RE4 uses MLAB for strands; Veilguard layered transparency.
- Anisotropic specular on cards: Kajiya-Kay with tangent from a flow/tangent map, two highlights with shifted tangents `T' = normalize(T + shift * N)` (primary toward tip, secondary coloured toward root, sparkle noise) (Scheuermann). Physically based alternative: Karis16 on cards (Unreal cards use the same Hair shading model). Gerard 2023, "Fast Real-Time Shading for Polygonal Hair" (Eko Software, arXiv 2302.06468, search snippet): single pass, only diffuse + flow map, Marschner-based with self-shadowing baked into geometry, environment-map integration, 0.04-0.25 ms on 9k-34k-triangle curly hair.

---

## 7. Simulation

- Dynamic Follow-the-Leader: Müller, Kim, Chentanez, "Fast Simulation of Inextensible Hair and Fur", VRIPHYS 2012 (https://matthias-research.github.io/pages/publications/FTLHairFur.pdf , search snippet): inextensibility in a single iteration per frame; 47k hairs at 25 fps incl. rendering; adds numerical damping to hide FTL's mass imbalance. Shipped in Indiana Jones (all strands, head SDF).
- XPBD: Macklin, Müller, Chentanez, "XPBD: Position-Based Simulation of Compliant Constrained Dynamics", MIG 2016 (PDF opened): compliance + total Lagrange multiplier make stiffness independent of time step and iteration count. Unreal's groom solver runs bend/stretch "during the XPBD loop".
- Cosserat rods: Kugelstadt & Schömer, "Position and Orientation Based Cosserat Rods", SCA 2016 (DOI 10.2312/sca.20161234; abstract via search, full text not opened): quaternion orientations per segment, bending/torsion constraints in PBD, >10x faster than earlier position-based rods. Needed only for twist-sensitive styles (braids); more state per segment.
- TressFX constraints (code opened): Verlet + global shape (pull to skinned rest) + local shape (bend/twist relative to parent frames) + length + VSP + wind + capsule/SDF collision; render "follow" hairs from guides.
- Guide simulation + interpolation: Unreal simulates guides only; Frostbite 2019 found ~5x from simulating 1/10 of strands; IJGC simulated all strands but notes "Dynamic hair needs more work to reach 60 FPS".
- Body collision: capsules/spheres (TressFX capsules, Unreal physics asset, VRM colliders), SDF for heads (TressFX 4.0 dynamic SDF; IJGC prebaked 0.25 MB SDF).
- Grid-based hair-hair interaction (friction, volume preservation): Frostbite Eulerian/Lagrangian; Unity hair volume density/pressure; IJGC density voxels for collision and shadows.
- Cheapest convincing option for game characters: simulate a few hundred guides with Verlet/XPBD length constraints + TressFX-style global/local shape constraints + root velocity shock propagation, collide guides against the character's existing capsules (and optionally a baked head SDF), add wind as an external force, interpolate rendered strands from up to 3 guides. Run on GPU compute if guides x points exceed a few thousand, otherwise CPU is viable.
- VRMC_springBone 1.0 (spec opened: https://github.com/vrm-c/vrm-specification/blob/master/specification/VRMC_springBone-1.0/README.md): glTF 2.0 extension; springs = joint chains with `stiffness`, `gravityPower`, `gravityDir`, `dragForce`, `hitRadius`; sphere and capsule colliders in collider groups; optional `center` node. Non-normative reference: Verlet, `nextTail = currentTail + inertia + stiffness + external`, length constraint, push-out collision, set rotation from tail. Repo has no licence file (licence unverified). Good secondary-motion option for cards/meshes on bones.

---

## 8. Authoring and interchange

- Blender: Curves object (hair curves) with Geometry Nodes hair node groups (Generate, Interpolate, Clump, Curl, Frizz, Roll, Braid Hair Curves, guide maps). Blender 4.2 LTS release notes (opened: https://developer.blender.org/docs/release_notes/4.2/pipeline_assets_io/): "The hair curves object type is now supported for both import and export" for both USD and Alembic; USD export renames the active UV map to `st`. Blender manual: `UsdGeomBasisCurves` imports as Curves. Earlier ticket noted missing UVs and catmullRom vs bspline root/tip differences on USD curve export (search snippet, unverified whether fixed).
- Rust readers: `openusd` 0.8.0 (MIT, pure Rust, usda/usdc/usdz, updated 2026-10-11, https://github.com/mxpv/openusd ; `openusd-schemas` typed UsdGeom). `ogawa-rs` 0.4.0 (MIT OR Apache-2.0, Alembic Ogawa parser, last release 2023-04-14, https://github.com/Traverse-Research/ogawa-rs). Both licence-compatible with Apache-2.0.
- glTF: no curves/hair extension. KhronosGroup/glTF extension list (main branch, checked) has none; issue #1258 "Support for Curve Data" closed 2018 with the maintainer stating glTF stores render-ready data and exporters must convert; one vendor experiment (syoyo, tinygltfloader `alembic_to_gltf`). glTF 2.0 does have LINES/LINE_STRIP topologies and application-specific attributes prefixed with `_`, so an engine-specific polyline convention is possible but non-standard (no width semantics). No proposal found in issues/PRs searched for "hair", "curves", "strands".

---

## 9. wgpu 30 feasibility (from local wgpu-types/wgpu-hal/naga 30.0.1 sources)

- Core (no feature needed): compute shaders, storage buffers, `atomic<u32>/atomic<i32>` with add/min/max/and/or/xor/exchange/compareExchange in storage and workgroup memory, indirect dispatch, workgroup shared memory. Default limits: 16 KiB workgroup storage, 256 invocations per workgroup, 128 MiB storage binding, 256 MiB buffer, 8 storage buffers per stage (raise via adapter limits).
- `SHADER_INT64_ATOMIC_MIN_MAX` / `SHADER_INT64_ATOMIC_ALL_OPS`: Vulkan (`VK_KHR_shader_atomic_int64`), DX12 (SM 6.6+), Metal 2.4+. `TEXTURE_ATOMIC` (R32 image atomics) Vulkan/DX12; `TEXTURE_INT64_ATOMIC` (R64Uint min/max) Vulkan (`VK_EXT_shader_image_atomic_int64`), DX12 SM 6.6+. `SHADER_FLOAT32_ATOMIC` Vulkan only. The project already builds with `static-dxc`, which DX12 SM 6.6 needs.
- `SUBGROUP` (compute + fragment), `SUBGROUP_VERTEX`, `SUBGROUP_BARRIER`: Vulkan, DX12, Metal; native-only. Naga 30 WGSL supports `subgroupAdd/Min/Max/...`, `subgroupExclusiveAdd`, `subgroupBallot`, `subgroupBroadcast(First)`, `subgroupShuffle(Up/Down/Xor)`.
- `EXPERIMENTAL_MESH_SHADER` (+ `_MULTIVIEW`, `_POINTS`): Vulkan `VK_EXT_mesh_shader`, DX12, Metal; "Naga is only supported on vulkan. On other platforms you will have to use passthrough shaders." So mesh shaders exist but are experimental and not portable through WGSL on DX12.
- `EXPERIMENTAL_RAY_QUERY`: enabled by both Vulkan and DX12 hal backends (doc comment still says Vulkan); triangles/AABBs only, no curve/LSS primitive.
- `CONSERVATIVE_RASTERIZATION`: Vulkan only, triangles only.
- What a strand renderer needs: (a) storage buffers for strand points, guides, interpolation weights, root bindings, post-transform cache; (b) compute passes for skinning/sim/interpolate/cull/raster/shade; (c) 32-bit atomics (core) for a 3-layer visibility buffer with 10-bit depth + 22-bit id (IJGC) and atomicAdd coverage; 64-bit atomic min optional for wider payloads (Lipp 2026); (d) indirect dispatch for LOD-driven work; (e) subgroups optional for compaction and divergence tricks (IJGC, Lipp); (f) a depth prepass texture readable in compute for occlusion culling and per-pixel depth test; (g) a composite pass writing colour, depth and motion vectors before TAA. No mesh shaders required.

---

## 10. Curly and coily hair

- Representation options:
  - Geometry in the strands: author curls in DCC (Blender Curl Hair Curves: Factor, Curl Start, Radius, Frequency, Random Offset, Seed, guide map; MetaHuman Houdini "MH Groom Hairstyle Generator" has "Coil Mode", "Coil Amplitude", "Coil Frequency", "Coil Randomness", "Tuft and Curl" with "Curls Size/Frequency/Position": https://dev.epicgames.com/documentation/metahuman/mh-groom-hairstyle-generator , opened). Cost: many more points per strand ("Curly strand-based hair requires high vertex counts", SIGGRAPH 2024 course).
  - Procedural curl at interpolation time: simulate smooth guides, add a per-strand helix (radius, frequency, phase from seed) in the guide's frame when generating render strands. This is an engineering inference (Lipp 2026 and Bhokare 2025 apply styling functions to generated strands; Chang et al. 2025 "Transforming Unstructured Hair Strands into Procedural Hair Grooms" is cited by Lipp), not a documented Unreal feature. No native "curl" attribute exists in the Unreal Alembic spec v1.5 or Strands/Interpolation docs.
  - Simulation of tight coils: Shi, Wu, Parr, Darke, Kim, "Lifted Curls: A Model for Tightly Coiled Hair Simulation" (2023, DOI 10.1145/3606920, SCA 2023 Best Paper; abstract via search: curls with radii near 5 mm, robust when bends/twists occur at the discretisation scale, formulated on vertex positions).
  - Cards/meshes for coily hair: helix/tube geometry, sculpted braids/locs with anisotropic noise in height maps, cornrow partings in the scalp normal map (SIGGRAPH 2024 course artists). Cards overlap/clip and the artefacts show more on light hair.
- Published talks/resources:
  - Darke, Olander, Kim, "More Than Killmonger Locs: A Style Guide for Black Hair (in Computer Graphics)", SIGGRAPH 2024 Courses (251 pp., text opened via https://par.nsf.gov/servlets/purl/10637184 , 84 MB PDF). Includes Code My Crown (Dove, with Olander), the Andre Walker 1-4 A/B/C texture system, Blender/XGen/ZBrush/Substance workflows for twist-outs, locs, Bantu knots, cornrows, braids, and Lifted Curls.
  - Kim et al., "Countering Racial Bias in Computer Graphics Research", SIGGRAPH 2022 Talks (arXiv 2103.15163, search snippet): 41 publications showed only straight or wavy hair as evidence for "human hair".
  - Olander (Tallgran), "Authentic Representation of Black Hair in Games", GDC 2024 Advocacy (https://gdcvault.com/play/1034421/Authentic-Representation-of-Black-Hair ; schedule page 403, content unverified).
  - Open Source Afro Hair Library (A.M. Darke): free 3D models; licence not verified (secondary reports: educational and commercial use with conditions).

---

## Recommended architecture (summary)

Tier LOD0 strands: groom asset = guides + render strands (+ per-strand root UV, group, seed, clump id, width/colour), randomised strand order, up to 3 closest guides + weights per strand, per-mesh-LOD root binding (triangle, barycentrics, frame). GPU pipeline in WGSL compute: skin roots -> simulate guides (XPBD/DFTL + TressFX shape constraints, capsules + optional head SDF, wind) -> interpolate strands (+ optional procedural curl) -> post-transform cache -> cull vs depth prepass -> software raster to a tiled 3-layer 32-bit visibility atlas + additive coverage -> per-vertex Marschner (Karis16/Tafuri19) shading with baked or voxel self-shadowing -> composite quad in the transparent pass writing depth + motion vectors before TAA. Optional fast paths with `SUBGROUP` and 64-bit atomics. Hair -> skin shadows by density-volume raymarch; hair -> world shadows by proxy mesh/cards in the shadow map.
Tier cards: generated from the same strands offline (cluster -> strips -> atlas with coverage, depth, root-to-tip, seed, flow) and hand-tweakable; forward pass with hashed/dithered alpha resolved by TAA plus an opaque core; same hair BSDF using the flow-map tangent; skinned to guide-derived bones or VRM-style spring bones.
Tier meshes: today's sculpted meshes; anisotropic hair BSDF with flow maps; rigid or skinned; spring bones.

Rejected: TressFX as a dependency (frozen 2020, HLSL/C++, PPLL memory, Kajiya-Kay shading; keep as reference), HairWorks (closed, legacy, DX11 tessellation + MSAA), hardware triangle-strip strands + MSAA (overdraw, needs MSAA; Frostbite moved off it), mesh-shader hair meshes (experimental and Vulkan-only through naga; Lipp 2026 is 4x faster in software), PPLL OIT (unbounded memory), RTX Hair/LSS (Blackwell + proprietary, no wgpu primitive), Unity hair package (Unity Companion License), full Chiang path tracing (offline reference only).

Risks: EA patent US11551387B2 on tiled, depth-clustered hair rasterisation (review design); content memory (10-30 MB per LOD0 groom in IJGC); motion vectors and TAA ghosting on thin strands; curly hair vertex counts.
