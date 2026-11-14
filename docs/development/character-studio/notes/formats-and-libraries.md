# Formats and libraries for the character creator overhaul

Research for Axcenity's character overhaul (morph-target bodies and faces, layered clothing, groom/hair, skin and fabric materials). Researched 2026-11-14. Nothing in the repository was edited.

**How it was checked:**
- **Repository:** read at branch `character-studio-cs2`, commit bd3ae493.
- **Crate metadata:** crates.io API.
- **Repo metadata:** GitHub API (`gh api`): licence, last push, latest release.
- **Specs:** the glTF spec and the extension READMEs, downloaded raw from KhronosGroup/glTF `main`.
- **Crate sources:** read in the local cargo registry for `gltf-1.4.1`, `gltf-json-1.4.1`, `meshopt-0.7.0` (which vendors meshoptimizer 1.3), `openusd-schemas-0.8.0` and `intel_tex_2-0.5.0`.
- **Unverified claims:** anything not opened is marked **unverified**.

---

## 0. Summary verdicts

| Technology | Licence | Maintained? | Rust | Verdict |
|---|---|---|---|---|
| glTF 2.0 + KHR material extensions | Spec: Khronos. `gltf` crate: MIT OR Apache-2.0 | Spec: active. `gltf` crate: last release 1.4.1 (2024-05-10); `main` is active (2026-05) but unreleased | `gltf` 1.4.1, already used | **Adopt.** It stays the runtime format. Read sheen, specular, clearcoat, ior, variants and JOINTS_1 ourselves through `extension_value` |
| VRM 1.0 | UniVRM and three-vrm: MIT. Spec repo: no LICENSE file. Each model carries the VRM Public License | Active (spec updated 2026-10, UniVRM v0.131.4 2026-10-19) | `serde_vrm`/`bevy_vrm` 0.5.0 (MIT OR Apache-2.0); `vrm-spec` 0.1.1 (Apache-2.0, pixiv) | **Adopt later.** Import-only: expressions, humanoid map and spring bones. Implement the spring-bone algorithm natively now |
| OpenUSD | TOST-1.0 (Apache-2.0 with a different trademark §6; since 2024-05-29) | v26.11.1 (2026-10-21) | Pure-Rust `openusd`/`openusd-schemas` 0.8.0 (MIT), already in the workspace, editor-only | **Adopt (convert only, ADR-0027).** Extend `convert` for UsdSkel and BasisCurves |
| MaterialX | Apache-2.0 | v1.39.5 (2026-05-22), active | None usable (placeholder crate). `shadewire` (MIT, pre-release, Oct 2026) reads MaterialX and claims WGSL output | **Reference only.** Use OpenPBR/Standard Surface as the model for new lobes |
| OpenSubdiv | TOST-1.0 (since 2025-10-15) | v3.8.0 (2026-10-23) | `opensubdiv-petite` 0.3.1 (Apache-2.0, C++ wrapper, hard build); `opensubdiv-rs` 0.7.0 (pure Rust, MIT AND TOST-1.0, 3 months old) | **Reference only.** Author LODs offline. Maybe editor-only `opensubdiv-rs` later |
| MikkTSpace | zlib-style (mikktspace.h) | C reference: last push 2024-07 | `bevy_mikktspace` 1.0.0 (Zlib AND (MIT OR Apache-2.0), 2026-02); `mikktspace` 0.3.0 (2022, stale) | **Adopt**, through the existing `meshopt` dependency (`generate_tangents` with `TangentCompatible`) or `bevy_mikktspace` |
| meshoptimizer | MIT. Crate: MIT OR Apache-2.0 | v1.3 (2026-09-25); `meshopt` 0.7.0 (2026-10-17) | Already a dependency | **Adopt more of it:** Regularize, Permissive/Protect, Priority, ErrorClamped, generateTangents, vertex cache |
| Alembic | BSD-3-Clause | 1.8.12 (2026-07-02) | None (placeholder `alembic` 0.1.0 from 2020) | **Reject for now.** Hair comes in through USD BasisCurves, converted to an engine-own groom file |
| KTX2 / Basis Universal | KTX-Software: Apache-2.0 (plus others, REUSE). Basis: Apache-2.0 | KTX v4.4.2 (2025-10); Basis v2_50 (2026-08) | `ktx2` 0.5.0 (Apache-2.0); `basisu_c_sys` 0.9.1; `basisu` 0.1.0 (pure-Rust transcoder) | **Reference only.** BC7/BC5 cooking exists. Add BC4 (already in `intel_tex_2`) for single-channel skin maps |
| Cloth / soft body / SDF / queries | rapier3d and parry3d: Apache-2.0. mesh_to_sdf: MIT OR Apache-2.0 | rapier 0.36.1 (2026-10-10, **soft bodies new in 0.36.0, 2026-09-24**); parry 0.31.1; mesh_to_sdf 0.4.0 (2024-09, dormant) | Vendored rapier has `SoftBodyBuilder::cloth`, rope, tearing and an experimental FEM. parry has point projection and BVH | **Adopt later.** Evaluate rapier soft-body cloth first. Bespoke XPBD/rod solver for hair. parry for collision queries. `mesh_to_sdf` only if baking SDFs offline |

---

## 1. glTF 2.0

- Spec: https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html (version 2.0.1, 2021-10-11). Source: https://github.com/KhronosGroup/glTF.
- Extension registry status: `extensions/README.md` on `main`, downloaded 2026-11-14.

### 1.1 Morph target rules (spec §3.7.2.2, verified verbatim in Specification.adoc)

- **Attributes:**
  - `POSITION`, `NORMAL` and `TANGENT` (VEC3, float32).
  - `TEXCOORD_n` (VEC2: float32, snorm/unorm 8/16).
  - `COLOR_n` (VEC3/VEC4: float32, snorm/unorm 8/16).
  - The spec says: "Client implementations SHOULD support at least three attributes — POSITION, NORMAL, and TANGENT — for morphing. Client implementations MAY optionally support morphed TEXCOORD_n and/or COLOR_n."
  - A target may only name attributes the primitive's `attributes` has. Omitted attributes are zero deltas.
- **TANGENT deltas** have no W: "handedness cannot be displaced".
- **Sizes:** every target accessor has the same element count as the base attributes.
- **Counts:** all primitives of a mesh that have targets MUST have the same number. Primitives without targets are treated as zero targets.
- **Weights:** one weight set per mesh. `mesh.weights` gives the defaults; otherwise 0.
- **Accessor rules:** uint32 SHOULD NOT be used for deltas. No NaN or Inf. POSITION deltas MUST have min/max.
- **Names:** not in the core spec. "Many tools add a `targetNames` array of strings to the `extras` object of the mesh". This is the de facto standard (Blender writes it).
- **Sparse accessors** (§3.6.2.3) are the spec's recommended encoding for morph targets: sorted indices and replacement values. An accessor with neither `bufferView` nor `sparse` is all zeros. Blender writes these, and the `gltf` crate's validation refuses them; Axcenity works around it, see below.
- **No limit on the number of targets** in the spec.
- **Limits engines hit:**
  - Attribute-based implementations hit the ~16 vertex-attribute limit, so they cap at 4–8 active targets (older three.js, WebGL1).
  - three.js moved to float-texture morphs in r133 (PR #22293). Exact wording **unverified**; this comes from forum and issue results.
  - Storage-buffer or texture approaches, Axcenity's included, have no fixed cap. Their cost scales with deltas × passes.
- **With KHR_mesh_quantization:** morph deltas may be byte/short (normalized) for POSITION, and normalized byte/short for NORMAL and TANGENT. They are quantized with the same transform as the base positions (verified, extension README).

### 1.2 Skins (spec §3.7.3.3 and §5.x)

- **Influence sets:**
  - `JOINTS_n`: VEC4 of uint8 or uint16. `WEIGHTS_n`: VEC4 of float32, or unorm8/unorm16.
  - Each pair carries up to 4 influences. 8 influences means `JOINTS_0/1` with `WEIGHTS_0/1`, and so on.
- **Weights:**
  - Non-negative.
  - Joint indices with non-zero weight are unique per vertex.
  - With unorm weights, the sum must be exactly 1. With float weights, the sum SHOULD be within 2e-7 × the number of non-zero weights.
- **Client support:**
  - "Client implementations MAY support only a single set of up to four weights and joints, however not supporting all weight and joint sets present in the file may have an impact on the asset's appearance."
  - Clients SHOULD support at least `TEXCOORD_0`, `TEXCOORD_1`, `COLOR_0`, and `JOINTS_0` with `WEIGHTS_0`.
- **Joint count:** no spec cap beyond the index type (uint16 gives 65,536 per skin). `inverseBindMatrices` is MAT4 float32 with the last row (0,0,0,1).
- **Tangents:** when TANGENT is absent and a normal texture exists, clients compute "default MikkTSpace" tangents (spec line 1807). The W component must be equal across a triangle.

### 1.3 Extensions (status verified against extensions/README.md and the folders on `main`)

| Extension | Status | Use for characters |
|---|---|---|
| KHR_materials_sheen | Ratified | Cloth and fabric: velvet, cotton fuzz, peach-fuzz rim on skin. Essential for a fabric material |
| KHR_materials_specular | Ratified | Dielectric F0 and tint control. Skin is about 0.028 F0 at IOR 1.4; hair; eyes |
| KHR_materials_clearcoat | Ratified | Wet lips and eyes, lacquered leather, car-paint-style accessories. Moderate |
| KHR_materials_ior | Ratified | Pairs with specular, transmission and volume. Eye cornea about 1.376 |
| KHR_materials_diffuse_transmission | **Release Candidate**, not ratified (README and the extension's own Status). May be superseded, see below | Thin translucency: ears, fabric backlight, leaves. Cheap stand-in for subsurface. Axcenity already has leaf translucency |
| KHR_materials_volume | Ratified | Thickness texture and attenuation. Gives a thickness map usable for skin SSS approximations (ears, nostrils) |
| KHR_materials_anisotropy | Ratified | Hair (Kajiya-Kay-like highlights on cards), satin, brushed metal. High value for hair cards |
| KHR_materials_variants | Ratified | Per-primitive material sets keyed by variant name: exactly "outfit colour variants" (same mesh, swap material sets). Product-configurator origin. Good fit |
| KHR_texture_basisu | Ratified | Basis-supercompressed KTX2 textures in glTF. Import-time transcode only |
| KHR_mesh_quantization | Ratified | int8/int16 positions, normals, UVs and morph deltas. Common in gltfpack output |
| EXT_meshopt_compression | Listed under **Ratified** (one of three EXT exceptions taken through Khronos ratification) | Lossless/lossy buffer compression, morph targets included. gltfpack output |
| KHR_meshopt_compression | **Release Candidate**: folder on `main` says "Release Candidate", proposal PR #2517 merged 2026-01-23. Successor to EXT_ with better attribute compression. Not yet in the README tables | Same as above |
| KHR_draco_mesh_compression | Ratified | Geometry compression (C++ decoder). Less suited to morph targets than meshopt. Skip |
| KHR_animation_pointer | Ratified | Animate any property by JSON pointer (material colours, morph weights, light values). Blush/flush materials driven by expressions; animated texture transforms (VRM-style eye UV) |
| Subsurface | **No ratified extension.** `KHR_materials_subsurface` PR #1928 was closed 2024-11-21 (README still lists it as "Initial Draft"). Replaced by `KHR_materials_volume_scatter` PR #2453 (open, updated 2026-02-09), now proposed to be replaced by **`KHR_materials_scatter` PR #2579** (open, updated 2026-11-13). That one would merge volume_scatter and diffuse_transmission, aligned with OpenPBR/Standard Surface | Skin SSS has no glTF carrier yet. Use `extras` (as Axcenity already does for wind and translucency) or Axcenity material files |

Also ratified and relevant: KHR_materials_emissive_strength (not read by Axcenity, per docs/rendering/models.md:10), KHR_materials_iridescence, KHR_materials_dispersion.

### 1.4 What the `gltf` crate parses

- **crates.io:**
  - Newest version: **1.4.1**, released 2024-05-10, licence MIT OR Apache-2.0. Axcenity already locks 1.4.1 (Cargo.lock).
  - Previous releases: 1.4.0 (2023-12), 1.3.0 (2023-08).
- **Feature flags in 1.4.1** (crates.io and local Cargo.toml):
  - Extension flags: `KHR_lights_punctual`, `KHR_materials_emissive_strength`, `KHR_materials_ior`, `KHR_materials_pbrSpecularGlossiness`, `KHR_materials_specular`, `KHR_materials_transmission`, `KHR_materials_unlit`, `KHR_materials_variants`, `KHR_materials_volume`, `KHR_texture_transform`.
  - Other flags: `allow_empty_texture`, `extensions`, `extras`, `guess_mime_type`, `import`, `names`, `utils`.
- **Not in 1.4.1:** sheen, clearcoat, anisotropy, diffuse_transmission, animation_pointer, basisu, mesh_quantization, meshopt, draco.
- **Required extensions** (`gltf-json-1.4.1/src/root.rs:160-172`):
  - `extensionsRequired` entries not in `ENABLED_EXTENSIONS` are a validation error (`Unsupported`).
  - `ENABLED_EXTENSIONS` (extensions/mod.rs:40-62) lists only lights, pbrSpecularGlossiness, unlit, texture_transform, transmission, ior and emissive_strength (plus basisu, webp and dds under `allow_empty_texture`).
  - So **specular, variants and volume are parsed when enabled but are not on the allowlist for `extensionsRequired`.** Open PR #472 is "Allow KHR_materials_specular as a required extension".
- **`extensions` feature:** gives `extension_value(name) -> Option<&serde_json::Value>` on material, mesh, accessor and other objects (`gltf-1.4.1/src/material.rs:110`, `accessor/mod.rs:155`). Any extension can be read as raw JSON without crate support, the same way Axcenity already reads `extras.targetNames`.
- **Upstream `main`** (Cargo.toml read 2026-11-14) adds unreleased features: `KHR_materials_sheen`, `KHR_materials_clearcoat`, `KHR_animation_pointer`, `EXT_texture_webp`, `allow_empty_animation_target_node`. It also merged panic fixes (PR #471, 2026-05-11).
- **Open PRs:**
  - #474: morphed TEXCOORD_n/COLOR_n.
  - #473: allowlist EXT_meshopt_compression and KHR_mesh_quantization.
  - #463: meshopt and quantization support.
  - #467 and #456: basisu (JSON only).
  - #412: texture_transform on normal and occlusion textures.
  - #422: "Rework for version 2.0".
  - The project is maintained but releases slowly.
- **Upgrade path for Axcenity:**
  - Stay on 1.4.1 and add the `extensions` feature. Parse KHR_materials_sheen, clearcoat, anisotropy, specular, variants and diffuse_transmission from raw JSON.
  - Read `JOINTS_1`/`WEIGHTS_1` with `reader.read_joints(1)` and `read_weights(1)`, which the crate already supports.
  - For quantized or meshopt glTF, either decode meshopt buffers ourselves (`meshopt::decode_vertex_buffer` and `decode_index_buffer` are already in meshopt 0.7) before validation, or convert on import (ADR-0027 style).

### 1.5 What Axcenity's glTF loader reads today (verified, with citations)

Files: `crates/assets/src/model.rs` (the reader), `model/morph.rs`, `model/gltf_takes.rs`, `model/lod.rs`, `crates/render/src/shader.wgsl`.

- **Crate and features:** `gltf = { version = "1.4", features = ["KHR_texture_transform", "extras"] }` (Cargo.toml:99). The `extensions` feature is not enabled.
- **Validation** (`read_gltf`, model.rs:903-922):
  - Runs the crate's validation but skips only the "accessor without bufferView" error, so Blender's empty sparse shape keys load.
  - Every other error is kept. So **any file with KHR_mesh_quantization, EXT_meshopt_compression, KHR_draco_mesh_compression or KHR_texture_basisu in `extensionsRequired` fails to load**: gltfpack output and many web avatars.
- **Primitive mode:** triangles only; points and lines are skipped (model.rs:1557-1560).
- **POSITION** (model.rs:1565-1569), baked by the node transform.
- **Indices:** u32, or generated; checked against bounds (model.rs:1576-1583).
- **NORMAL** (model.rs:1590-1602): smooth normals are generated when missing or of the wrong length.
- **TEXCOORD:** one set only, the set of the first texture the material uses, otherwise set 0 (model.rs:1604-1615 and 1761-1770). `TEXCOORD_1` for a second map is dropped with that map.
- **TANGENT: not read.**
  - Each primitive is pushed with `tangents: Vec::new()` (model.rs:1675).
  - Tangents are computed after fitting by Axcenity's own area-weighted algorithm (`tangents`, model.rs:2058-2104; called at 2185-2192), which is not MikkTSpace.
  - docs/rendering/models.md:23 says so: "File tangents are ignored, so seams on maps baked with MikkTSpace can show slightly."
- **COLOR_0: not read.** No `read_colors` call; docs/rendering/models.md:10 says "Not yet: vertex colors".
- **JOINTS_0/WEIGHTS_0 only** (model.rs:1617-1649):
  - `read_joints(0)` and `read_weights(0)`, kept to 4, renormalized.
  - **JOINTS_1 and WEIGHTS_1 are ignored**, so 8-influence characters lose weights.
  - The GPU skins 4 influences (shader.wgsl:170).
- **Skins** (model.rs:1523-1542): joints and inverse bind matrices. A u16 joint index across the model, so at most 65,535 joints. A skinned mesh ignores its node's transform (model.rs:1503-1509).
- **Morph targets** (model.rs:1651-1668):
  - Reads POSITION and NORMAL deltas.
  - **TANGENT deltas are discarded**: the third element of `read_morph_targets()` is `_` at model.rs:1655.
  - Sparse accessors are decoded by the crate's reader.
  - Names come from `mesh.extras.targetNames` (model.rs:1699-1709), with a `<mesh>.<n>` fallback. Default weights come from `mesh.weights` (model.rs:1710).
  - Shapes of the same name across meshes merge into one target (morph.rs:65-97, u16 cap).
  - Only corners that move are kept (`morph::sparse`, morph.rs:102). Normal deltas are computed when absent.
- **Morph weight animation:** not read. `ReadOutputs::MorphTargetWeights(_) => continue` (gltf_takes.rs:74).
- **Animations:** TRS channels; CUBICSPLINE is read at key values with tangents dropped (gltf_takes.rs:136-147).
- **Material** (model.rs:1751-1841):
  - Read: baseColor factor and texture, metallic/roughness factor and texture (G and B), normal texture and scale, occlusion (R) and strength, emissive factor and texture, alphaMode and cutoff, doubleSided.
  - `KHR_texture_transform`: the first of the base, MR or emissive transforms places all maps (model.rs:1771-1782).
  - `extras`: `axcenity_sway`, `axcenity_flutter` and `axcenity_translucency` (model.rs:349-382, applied at 1838).
  - **No KHR_materials_* extension is read.**
- **Images:** decoded through `gltf::image::Data` (PNG and JPEG via the `import` feature) into RGBA8 (`to_rgba8`, model.rs:1966). Cooked BC7/BC5 copies are used when current (model.rs:1845-1872).
- **LODs:** `meshopt::simplify_with_attributes_and_locks` with normal weight 0.5 and UV weight 1.0, no locks, `SimplifyOptions::None`, targets 50% and 25% (lod.rs:25-42, 179-215). Index-only, so morphs are remapped (`morph::remapped`, morph.rs:236).
- **GPU morph and skin** (shader.wgsl):
  - `morph_corner` (121-144) adds position and normal deltas and does not touch the tangent.
  - `pose_corner` (157-188) skins 4 influences and rotates the tangent (186).
  - The fragment shader re-orthogonalises the tangent against the morphed normal per pixel (`surface_normal`, 372-380).
  - docs/rendering/animation.md:9: ray tracing sees the unmorphed mesh.

### 1.6 Performance notes

- **Morph cost on the GPU:**
  - `morph_corner` loops over **every** delta of the corner, including zero-weight shapes (shader.wgsl:130-139).
  - It runs in every vertex pass that draws the item: scene, prepass, shadows, and the motion pass's `previous_corner`, per docs/rendering/animation.md:9.
  - A face rig with ARKit-52 plus correctives at 20–60 deltas per mouth vertex multiplies across passes and cascades.
  - **Recommendation:** a compute pre-pass that morphs and skins once per frame into a vertex buffer (as a GPU skin cache does). It would use an active-shape list that skips zero weights, and would also feed RT BLAS refits, fixing the "RT sees unmorphed mesh" limitation.
- **Sparse storage:** Axcenity's sparse per-primitive storage is already good. Per-delta memory is index + vec3 + vec3, about 28 B. Quantizing deltas to half floats (`meshopt::quantize_half`) would halve it.

---

## 2. VRM 1.0

- **Docs:** https://vrm.dev/en/ and the spec at https://github.com/vrm-c/vrm-specification (folders VRMC_vrm-1.0, VRMC_springBone-1.0, VRMC_springBone_extended_collider-1.0, VRMC_springBone_limit-1.0, VRMC_materials_mtoon-1.0, VRMC_node_constraint-1.0, VRMC_vrm_animation-1.0, VRMC_materials_hdr_emissiveMultiplier-1.0). Last commit 2026-10-02.
- **Status:** VRMC_vrm and VRMC_springBone both say "Complete".
- **Licences:**
  - The spec repo has **no LICENSE file** (GitHub API 404 for LICENSE; README is one line). Third-party claims of MIT or CC BY 4.0 are **unverified**.
  - Reference implementations: UniVRM is **MIT** (v0.131.4, 2026-10-19); pixiv/three-vrm is **MIT** (pushed 2026-10-22).
  - **Model licence:** every VRM model carries `meta.licenseUrl` = VRM Public License 1.0 (https://vrm.dev/licenses/1.0/). Settings: `avatarPermission` (default `onlyAuthor`), `commercialUsage` (default `personalNonProfit`), `allowRedistribution` (default false), `modification` (default `prohibited`), plus flags for violent, sexual, political or antisocial use (meta.md:34-46). An engine that bakes a VRM into a shipped game is redistributing it. **The importer must surface and respect these fields.**
- **Expressions** (VRMC_vrm.expressions):
  - Preset emotions: happy, angry, sad, relaxed, surprised.
  - Lip-sync visemes: aa, ih, ou, ee, oh.
  - Blink: blink, blinkLeft, blinkRight.
  - Gaze: lookUp, lookDown, lookLeft, lookRight.
  - Also `neutral`, plus custom expressions.
  - Each expression has `morphTargetBinds` (node, morph index, weight), `materialColorBinds`, `textureTransformBinds` and `isBinary`. It also has `overrideBlink`, `overrideLookAt` and `overrideMouth` (`none`/`block`/`blend`), which suppress procedural blink, gaze or lip-sync while an emotion plays.
  - This maps almost 1:1 onto Axcenity's `assets/faces/expressions/*.expression.json` and viseme maps (assets CLAUDE.md).
  - The override rules are a good design to copy for the face system.
- **Spring bones** (VRMC_springBone):
  - Per-chain joints with `stiffness`, `gravityPower`, `gravityDir`, `dragForce` and `hitRadius`.
  - Sphere and capsule colliders (plus plane and inside-shapes in `_extended_collider`).
  - Optional `center` space, and a deterministic Verlet-style algorithm with a fixed calculation order, all in the spec.
  - Cheap, robust secondary motion for hair cards, ponytails, skirts and accessories. This is what games ship before strand sim.
- **MToon:** toon shading with shade colour, rim, outline and MatCap. Axcenity already has Toon shading and outlines (docs/rendering/stylized.md), so it could be mapped approximately.
- **Rust:**
  - `serde_vrm` and `bevy_vrm` 0.5.0 (MIT OR Apache-2.0, 2026-10-13).
  - `bevy_vrm1` 0.11.0 (MIT OR Apache-2.0, 2026-10-29, VRM and VRMA, Bevy-bound).
  - `vrm-spec` 0.1.1 (Apache-2.0, pixiv/vrm-utils-rs, 2026-06): serde data structures, renderer-agnostic.
  - `vrm-runtime` 0.1.0 (2026-08, one release, 97 downloads).
  - `gltf_kun_vrm` 0.3.0.
  - Best fit: `vrm-spec` (types only) or hand-written serde, read through `gltf`'s `extension_value`.
- **Worth it for a Roblox-familiar engine?**
  - Partly. Roblox does not import VRM (**unverified**; believed so). The users Axcenity targets (Roblox-style creators) overlap with VRoid/VTuber creators.
  - .vrm is a .glb, so import cost is mostly mapping: humanoid bone map to a `*.rig.json`, expressions to `*.expression.json`, spring bones to a new component, MToon to the Toon material.
  - Risks: the per-model licence terms; VRM 0.x files (a different `VRM` extension and a different coordinate convention) are still common.
  - **Verdict: adopt later.** Implement spring bones natively now, since the Character Studio needs secondary motion for hair cards and skirts. Add the VRM 1.0 importer as a later phase.

---

## 3. OpenUSD

- **Docs:** https://openusd.org/. Source: https://github.com/PixarAnimationStudios/OpenUSD. Latest release **v26.11.1** (2026-10-21); last push 2026-11-14.
- **Licence:** **Tomorrow Open Source Technology License 1.0 (TOST-1.0)**. It "differs from the original Apache License 2.0 in ... Section 6 ('Trademarks')" (LICENSE.txt). Commit "Replace the APACHE License with the TOST License" is dated 2024-05-29. TOST is not an SPDX-listed identifier, so crates use `LicenseRef-TOST-1.0`. It is Apache-like and permissive, so compatibility is believed fine; a legal review is **unverified**.
- **Rust:**
  - Already in Axcenity: `openusd` 0.8.0 and `openusd-schemas` 0.8.0 (MIT, pure Rust, mxpv/openusd, released 2026-10-11, pushed 2026-11-14). Pinned `=0.8.0`, editor-only, used by `crates/convert`.
  - `openusd-schemas` features include `skel` (src/skel: `anim_query.rs` with blend-shape order and weights, `skinning.rs`, `decode.rs`) and `geom` (Curves, BasisCurves and NurbsCurves are documented, with the `UsdGeomBasisCurves.basis` enum at geom/mod.rs:190).
  - Axcenity's `convert` currently skips `Skeleton`, `SkelAnimation` and `BlendShape` prims (convert/src/usd.rs:22, 108-110; P7.6).
  - C++ bindings would need a USD build. Not worth it.
- **For characters:**
  - **UsdSkel:** skeleton, skinning (joint indices and weights primvars of any element size, so it is not limited to 4), blend shapes with in-betweens, and SkelAnimation.
  - **BasisCurves:** hair strands with widths, cubic or linear basis, and primvars.
  - Blender 4.2+ exports hair curves to USD as BasisCurves and imports them back. Limits: parent strands only, missing UVs reported (Blender manual 4.4 and tracker, via search).
- **Relevance given ADR-0027:** fully consistent. Keep USD as an import-time conversion. P7.6, UsdSkel to glTF skin plus morph targets, is the natural path for MetaHuman- or Houdini-style character sources. BasisCurves should convert into Axcenity's own groom sidecar (§8), not glTF, which has no curve primitive.
- **Verdict:** adopt (convert only).

## 4. MaterialX

- **Docs:** https://materialx.org/. Source: https://github.com/AcademySoftwareFoundation/MaterialX.
- **Licence:** Apache-2.0. Latest **v1.39.5** (2026-05-22), pushed 2026-11-07.
- **Code generators** (`source/`): GenGlsl (GLSL, ESSL, Vulkan GLSL, **WgslShaderGenerator**), GenMsl, GenOsl, GenMdl, GenSlang.
  - The **WGSL** generator was added 2025-06-09 (PR #2407).
  - Despite the name it emits "**WGSL-compliant GLSL**": Vulkan GLSL with separate textures and samplers (`HW_SEPARATE_SAMPLERS`), and `LightData.type` renamed. It is meant "to allow USD to render MaterialX shaders with the WebGPU implementation", cross-compiled afterwards (Tint/naga, **unverified** which).
  - So there is **no native WGSL emitter**. Output could in principle go through naga's GLSL front end, which is already in the tree through wgpu; that is **unverified** for MaterialX output.
- **Rust:**
  - Only placeholders: `materialx` and `materialx-sys` 0.0.0 (BSD-3, 2021).
  - New: **`shadewire`** 0.4.0 (MIT; vendored MaterialX Apache-2.0; OSL-derived BSD-3; created 2026-10-11, 0 stars, "Pre-release: APIs change").
  - Shadewire reads MaterialX 1.38/1.39, Blender graphs and MDL into an IR and emits WGSL, GLSL, HLSL, Metal and more.
  - Its host can accept `open_pbr_surface`, `standard_surface`, `gltf_pbr` or `UsdPreviewSurface`, or closure trees (diffuse, dielectric, conductor, sheen, subsurface…). Watch it; don't adopt yet.
- **Relevance:**
  - Axcenity's material is a fixed PBR model plus an optional WGSL `fn surface(SurfaceInput) -> Surface`, plus an editor node graph compiled to that WGSL (docs/rendering/models.md:13-15).
  - `Surface` has only base_color, metalness, roughness, normal, emissive and opacity, with no sheen, coat or SSS outputs. The shaders have no sheen, clearcoat, anisotropy or subsurface lobes (grep found none in *.wgsl).
  - MaterialX's main value is as a **reference for which outputs and lobes to add** (OpenPBR: subsurface, fuzz/sheen, coat, specular anisotropy, thin-film), so that `.mtlx` or glTF materials can later be mapped onto them.
  - Glossary or interchange import could come later through `convert`.
- **Verdict:** reference only.

## 5. OpenSubdiv

- **Docs:** https://opensubdiv.org/ (graphics.pixar.com/opensubdiv). Source: https://github.com/PixarAnimationStudios/OpenSubdiv. Latest **v3.8.0** (2026-10-23).
- **Licence:** **TOST-1.0** (LICENSE.txt). It replaced "modified Apache 2.0" in commit "Replaced Apache 2.0 License with TOST License" (2025-10-15).
- **Rust:**
  - `opensubdiv-petite` 0.3.1: Apache-2.0, 2026-05-14, 3,326 downloads. A safe wrapper over the C++ library (git submodule, CMake). Linux docs recommend clang-17 with libc++; OpenMP and CUDA caveats. Heavy build for Windows and Linux CI.
  - `opensubdiv-rs` 0.7.0: "MIT AND LicenseRef-TOST-1.0", created 2026-08-09, 8 releases, 4 stars. A pure-Rust port of the Sdc/Vtr/Far layers plus a GPU patch-table layout, `#![forbid(unsafe_code)]`, no dependencies. Young.
  - `opensubdiv` 0.1.0 is a 2020 placeholder.
- **Runtime subdivision vs offline LODs:**
  - wgpu/WebGPU has **no tessellation stage**, so runtime subdivision would be compute-based. It would run after morph and skin on every frame, with crease and UV handling. That is a big cost for little gain at game camera distances.
  - Mainstream engines ship authored or simplified LODs, and Axcenity already has meshopt LODs and `_LOD1/_LOD2`.
  - A worthwhile niche: **editor-only** subdivision in the Character Studio, to smooth a user-shaped body for close-up renders or to generate the LOD0 of a garment shell from a cage. `opensubdiv-rs` would fit if it matures.
- **Verdict:** reference only. Maybe editor-only later.

## 6. MikkTSpace

- **Source:** https://github.com/mmikk/MikkTSpace (last push 2024-07-20; no GitHub licence detection).
- **Licence:** mikktspace.h carries a **zlib-style** notice: "Permission is granted to anyone to use this software for any purpose, including commercial applications…".
- **Rust crates:**
  - `bevy_mikktspace` 1.0.0: **Zlib AND (MIT OR Apache-2.0)**, released 2026-02-17, 8.7M downloads, a safe-Rust port maintained by Bevy. Zlib is permissive and Apache-compatible.
  - `mikktspace` 0.3.0 (gltf-rs): MIT/Apache-2.0, last release 2022-02-12. Stale.
  - `mikktspace-sys` 0.1.1: MIT C wrapper, 2023.
- **New finding: meshoptimizer 1.3 has a tangent generator.**
  - meshoptimizer 1.3 (vendored by `meshopt` 0.7.0, already in Axcenity) has `meshopt_generateTangents` (stable API, not marked experimental) and the Rust wrapper `meshopt::generate_tangents`.
  - `meshopt_TangentCompatible` "Produce[s] tangents compatible with MikkTSpace (same weighting and fallbacks)". The default mode uses a different weighting with better quality on bevels.
  - It outputs per corner, so the caller must split vertices where tangents differ (UV mirror seams); the README shows how.
  - This gives MikkTSpace-compatible tangents **with no new dependency**. Exact equivalence with the reference is claimed upstream and **unverified** here.
- **Does Axcenity compute tangents?** Yes, its own way:
  - `tangents()` (model.rs:2058-2104) averages area-weighted U and V directions, Gram-Schmidts against the normal, and takes a sign.
  - It runs after fitting for primitives with a normal map (model.rs:2185-2192).
  - File TANGENT is ignored, and morph TANGENT deltas are discarded (model.rs:1655).
  - No "mikk" anywhere in the code; only docs/rendering/models.md:23 mentions it.
- **Tangents under morph targets on a normal-mapped mesh:**
  1. Bake normal maps in MikkTSpace and generate **base** tangents with MikkTSpace (or read file TANGENT when present) on the **rest** mesh, the same mesh the baker used. Otherwise baked maps show seams.
  2. Morphs change the surface. glTF allows TANGENT deltas (VEC3, no W). Most real-time engines instead apply position and normal deltas and **re-orthogonalise** the rest tangent against the morphed normal (Gram-Schmidt). Handedness (W) never changes. Axcenity already does this per pixel (shader.wgsl:372-380). Correct enough for faces.
  3. Reading TANGENT deltas is a cheap, optional accuracy gain for strong shapes such as cheek puff or jaw open. It adds 12 B per moved corner.
  4. Recomputing full MikkTSpace per frame is not viable: it is topological and CPU-bound.
  5. Vertex splits made by tangent generation must happen **before** building sparse morph corners, or the morph corner indices must be remapped (`morph::remapped` exists).
  6. Skinning must rotate tangents with the same matrix as positions. It does (shader.wgsl:186).
- **Verdict:** adopt MikkTSpace-compatible tangents, through meshopt (no new dependency) or `bevy_mikktspace`. Also honour file tangents.

## 7. meshoptimizer

- **Source:** https://github.com/zeux/meshoptimizer. MIT, **v1.3** (2026-09-25). Docs in its README.
- **Crate:** `meshopt` 0.7.0 (MIT OR Apache-2.0, 2026-10-17, gwihlidal/meshopt-rs), vendoring meshoptimizer 1.3 (`MESHOPTIMIZER_VERSION 1030`).
- **How Axcenity uses it** (grep `meshopt::`):
  - `crates/assets/src/model/lod.rs:179,202-212` calls `simplify_with_attributes_and_locks` with normals (weight 0.5) and UVs (1.0), no locks and options `None`.
  - `crates/mesh_edit/src/modifiers/reduce.rs:50` calls `simplify_decoder` for the Model Studio Reduce modifier.
  - Nothing else: no vertex-cache or fetch optimisation, no quantisation, no tangents, no codecs.
- **Features that matter for characters** (all verified in the header and README):
  - **Index-only simplification**, the current approach. LOD vertices are the original ones, so **morph targets and skin weights stay valid**; morphs are only re-indexed. Keep this for faces and bodies with shapes.
  - **Morph-aware simplification:** there is no direct morph support. Add the per-vertex maximum delta length (or a few principal deltas) as an extra attribute (up to 32 attributes), and mark expressive regions with `meshopt_SimplifyVertex_Priority`. That keeps lips, eyelids and nasolabial folds in LOD1.
  - `meshopt_SimplifyRegularize` / `RegularizeLight`: "can improve geometric quality under deformation such as skinning". Recommended for skinned LODs.
  - `meshopt_SimplifyPermissive` with `meshopt_SimplifyVertex_Protect` on UV seams: better LODs on garments with many seams.
  - `meshopt_SimplifyErrorClamped`: recommended when using attribute errors for LOD selection.
  - `meshopt_SimplifyLockBorder` / `vertex_lock`: keep garment openings (cuffs, collars) and body/garment seams from cracking between layers.
  - `meshopt_simplifyWithUpdate` moves vertices and attributes. It **invalidates morph deltas and needs bone weights renormalised**, so avoid it for morph meshes, or only for the far LOD with shapes dropped.
  - `meshopt_generateTangents`: see §6.
  - Vertex cache and fetch optimisation (`optimize_vertex_cache`, `optimize_vertex_fetch`): cheap wins at import for dense heads. Morph corner indices must be remapped if vertices are reordered.
  - Quantisation (`quantize_half`, `quantize_snorm`) and octahedral/quaternion filters: halve morph delta and tangent memory.
  - Vertex and index codecs (`encode_vertex_buffer`, `decode_*`): enable reading EXT_/KHR_meshopt_compression glTF, and compact cooked caches.
  - `clod_*` and meshlets: cluster LOD (Nanite-like). Not a fit for skinned characters yet.
  - Also from the same project, gltfpack: offline optimiser and compressor that handles morph targets and skins. Useful as a reference pipeline. The gltfpack licence is MIT as part of the repo (believed; **unverified** separately).
- **Verdict:** adopt more of it. It is already a dependency.

## 8. Alembic, and how hair should come in

- **Source:** https://github.com/alembic/alembic. Docs: http://www.alembic.io (**unverified**, not opened).
- **Licence:** BSD-3-Clause (LICENSE.txt: Lucasfilm and Sony Pictures Imageworks). Latest **1.8.12** (2026-07-02), pushed 2026-11-03.
- **Rust:** none usable.
  - `alembic` 0.1.0 is a BSD-3 placeholder from 2020.
  - `alembic-core`, `alembic-engine` and the other `alembic-*` 0.13 crates are an unrelated infrastructure tool (NetBox, Nautobot adapters).
  - The C++ library needs Imath, and optionally HDF5.
- **Hair pipelines in industry:**
  - Unreal's groom import reads **Alembic** with an informal groom attribute schema (`groom_width`, `groom_root_uv`, `groom_guide`, `groom_group_id`…). Source: Epic docs "Using Alembic for Grooms" (found via search; the attribute names are from memory, **unverified**). Maya XGen and Houdini exporters target it.
  - Blender exports hair curves to Alembic and, since 4.2, to USD BasisCurves.
- **Recommendation:**
  - Use an **engine-own groom sidecar** (cooked binary) as the runtime format. Content: strands as polylines with a fixed or variable point count; root UV on the scalp; width; a guide flag; group ID; per-strand random values; a pre-baked card LOD; and the scalp binding (triangle plus barycentrics). It would be produced by converters, as ADR-0027 says.
  - First converter: **USD BasisCurves** via `openusd`, which is already in the workspace, at zero dependency cost.
  - Alembic only when a real XGen or Houdini workflow appears. It needs an Ogawa reader in Rust (`docs/research/import-formats-and-plugins.md` already estimates a few thousand lines).
  - glTF cannot carry curves, so it isn't an option.
- **Verdict:** Alembic reject for now. USD BasisCurves adopt later. Engine-own groom sidecar adopt.

## 9. KTX2 / Basis Universal, and texture budget

- **KTX-Software:** https://github.com/KhronosGroup/KTX-Software. "Files unique to this repository generally fall under the Apache 2.0 license", other files under other licences (REUSE-compliant). **v4.4.2** (2025-10-04), pushed 2026-11-10.
- **Basis Universal:** https://github.com/BinomialLLC/basis_universal. Apache-2.0, **v2_50** (2026-08-03).
- **Rust crates:**
  - `ktx2` 0.5.0: Apache-2.0, 2026-04-19, 6.3M downloads. A parser, used widely in the wgpu/Bevy ecosystem.
  - `basis-universal` 0.3.1: MIT OR Apache-2.0, last release 2023-11, stale. Wraps the C++ encoder and transcoder.
  - `basisu_c_sys` 0.9.1: MIT OR Apache-2.0, active 2026-09, wraps Basis's C API.
  - `basisu` 0.1.0: Apache-2.0, a pure-Rust "bit-exact" transcoder, one release (2026-07).
  - `ctt` 0.6.1: MIT OR Apache-2.0 OR Zlib, 2026-10, a compressor front end.
- **Does Axcenity already compress textures?** Yes.
  - The `cook` crate uses `intel_tex_2` 0.5 (MIT/Apache-2.0, ISPC) to make BC7 (colour and data) and BC5 (normal maps) mip chains (cook/src/lib.rs:31, 126-127) in its own `AXTEX` container (assets/src/cooked.rs:38-52).
  - `texture2ddecoder` decodes on the CPU when sources are absent. wgpu `TEXTURE_COMPRESSION_BC` is used (render/src/renderer/gpu.rs:877).
  - Import presets include `max_size` (import_presets.rs:72-75).
  - No BC4, BC1 or BC6H, although `intel_tex_2` 0.5 ships `bc4.rs` and `bc6h.rs` (verified in the registry).
  - No KTX2 or Basis.
- **Relevance:** Axcenity is desktop-first, so BCn is right and KTX2/Basis adds little at runtime. The only need is **importing** glTFs with `KHR_texture_basisu`: transcode to RGBA at import, then cook. A pure-Rust `basisu` transcoder would avoid C++.
- **Verdict:** reference only. Adopt BC4 now.

**Texture budget** (4096² = 16.8 Mpx; a full mip chain adds about 1/3):

| Format | Bytes/px | 4K level 0 | 4K with mips | 2K with mips |
|---|---|---|---|---|
| RGBA8 (uncooked: Axcenity uploads `to_rgba8` output) | 4 | 64 MiB | 85.3 MiB | 21.3 MiB |
| BC7 / BC5 | 1 | 16 MiB | 21.3 MiB | 5.3 MiB |
| BC4 / BC1 | 0.5 | 8 MiB | 10.7 MiB | 2.7 MiB |

A 4K skin set for a head:
- albedo (BC7 sRGB)
- normal (BC5)
- roughness, cavity and AO packed (BC7)
- SSS, thickness or scatter mask (BC4 if available, otherwise BC7)
- two wrinkle normal maps (BC5)
- tiling detail normal at 1K (BC5)

That set is about 21.3 × 3 + 10.7 + 42.7 + 1.3 ≈ **119 MiB**, or about 130 MiB without BC4. A 4K body set adds about 75 MiB, and each garment layer with 3 × 2K–4K maps adds 16–64 MiB.

One hero at 4K is about 200–300 MiB of VRAM. Uncooked in the editor (RGBA8) the same set is about 4× that.

Implications:
- 4K for the Character Studio close-up and offline only.
- 2K head and 2K body at runtime for the local player; 1K for other avatars.
- Pack single-channel maps.
- Add BC4.
- Share tiling detail maps.
- Mip streaming or `max_size` per role.

## 10. Cloth, soft body, XPBD, rods, SDF, spatial queries

- **rapier3d 0.36.1** (Apache-2.0, https://github.com/dimforge/rapier, released 2026-10-10; vendored at `vendor/rapier3d` with one patch):
  - The **v0.36.0 changelog (2026-09-24)** added soft bodies: `SoftBodySet`, `SoftBody` and `SoftBodyBuilder` (ropes, cloth, trimeshes, volumes), simulated with rigid bodies, contacts and joints.
  - Also new: plasticity, tearing and cutting; soft-body clusters; deformable trimesh colliders (`insert_deformable`); an **experimental FEM solver** behind the `fem` feature.
  - Builder methods in the vendored source include `cloth`, `cloth_anisotropic`, `cloth_tube`, `rope`, `pinned_particles`, `bend_edges`/`dihedrals`, `self_contacts`, `skin`, `volumetric_skinned` and `tear_*`.
  - Axcenity passes `soft_bodies` through rapier calls (physics/src/sync.rs:579, 616) but exposes no soft-body Instance.
  - **Docs disagree with source:** docs/research/platform-expansion.md:80 says "Rapier has no soft bodies", which is outdated since 0.36.0.
  - Maturity: about 7 weeks old. Unknown performance for skinned-garment cloth attached to an animated body (**unverified**).
- **parry3d 0.31.1** (Apache-2.0; https://github.com/dimforge/parry): point projection with inside/outside (signed distance to TriMesh), distance, contacts, ray and shape casts, and a BVH. Enough for hair and cloth collision against body proxies; prefer capsules, as VRM and Unreal do.
- **avian3d 0.8.0** (MIT OR Apache-2.0): Bevy-ECS-bound, rigid only. Not usable.
- **XPBD cloth crates (all immature):**
  - `rapier-cloth` / `rapier-cloth-core` 0.3.0: MIT, 2026-09, 26/69 downloads, 0 stars. XPBD cloth with an optional implicit shell solver; targets rapier3d **0.34**, Axcenity is on 0.36.
  - `gizmo-physics-soft` 0.11: MIT OR Apache-2.0, XPBD cloth and ropes, no ECS required.
  - `bevy_silk` 0.10: MIT, Bevy-bound.
  - `bevy_softbody` 0.1: MIT, GPU XPBD on wgpu, Bevy.
  - `havok-cloth`: **GPL-3.0**, reject.
- **Hair and rods:** no mature Rust crate for strand, Cosserat or discrete-elastic-rod simulation. References:
  - AMD TressFX: MIT, GPUOpen, last push 2023-01. Follow-the-leader and length-constraint strand simulation, plus rendering.
  - VRM spring bones: the spec is a complete algorithm.
  - Unreal Groom/Niagara: proprietary, ideas only.
  - Practical path: guide strands or spring chains (XPBD distance plus bending) on the GPU, with interpolated render strands or cards.
- **SDF from meshes:**
  - `mesh_to_sdf` 0.4.0: MIT OR Apache-2.0, https://github.com/Azkellas/mesh_to_sdf. Last release 2024-09-17, repo last push 2025-01: dormant. Grid or point SDF with R-tree/BVH acceleration, sign by raycast (watertight) or normal.
  - Alternatives: bake with parry's `project_point` on a TriMesh (no new dependency); new and unproven `colino` and `cortenforge-mesh-sdf`.
  - Use: a body SDF baked offline per body morph basis for garment fitting and cloth/hair collision in the Character Studio. At runtime, prefer capsules.
- **Verdict:**
  - Evaluate rapier soft-body cloth for capes and skirts first: zero new dependency.
  - Spring bones (VRM-style) for hair cards and accessories.
  - A bespoke GPU XPBD guide-strand solver only if strand hair is committed.
  - parry for queries.
  - `mesh_to_sdf` optional, editor-only.

---

## Could not verify

- The VRM specification's own licence: the repo has no LICENSE file; secondary sources say MIT or CC BY 4.0.
- Whether Roblox imports VRM (believed not).
- A legal reading of TOST-1.0 compatibility with Apache-2.0 distribution. The text differs only in §6 Trademarks, per its own note.
- Which tool cross-compiles MaterialX's "WGSL-flavoured GLSL" to WGSL (Tint or naga), and whether naga's GLSL front end accepts it.
- That meshoptimizer's `TangentCompatible` output is bit-identical to reference MikkTSpace (claimed upstream).
- Unreal groom Alembic attribute names, and USD groom support in UE 5.5+. Search results only; Epic docs not opened.
- Blender's current USD hair export limits (from manual and tracker search snippets).
- three.js r133 morph-texture change wording.
- Performance of rapier 0.36 soft-body cloth on skinned garments; no benchmark run.
- gltfpack's separate licence statement.
- Alembic docs site (not opened).
