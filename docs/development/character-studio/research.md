# Character Studio overhaul: research

Stage 1 of Paul's brief of 2026-11-14, "Axcenity Character Studio: Complete Overhaul, Research, Multi-Agent Development & Implementation". This file holds the evidence: what the references show (part A) and what the technology offers (part B), kept apart. The audit of what exists is [audit.md](audit.md); the proposal built on both is [implementation-plan.md](implementation-plan.md).

Earlier plans this builds on, not repeats: [default-humanoid.md](../../research/default-humanoid.md) (the skeleton contract), [character-studio.md](../../research/character-studio.md) (CS1–CS6, the stylized redesign and the workspace) and [facial-animation.md](../../research/facial-animation.md) (F0–F7, the face runtime).

**Visual references are inspiration; technical claims cite official docs, papers, source repositories or licence texts.** A claim that couldn't be checked against its source says so.

## A. Visual direction

### A.0 The reference library

Paul's reference library (kept outside the repository; collected 2026-11-14) has 189 web references and 20 images he picked, in ten folders, each with a `REFERENCES.md` that names a concrete lesson per item. His picks are the strongest signal: `FINAL` is the look to reach, `TARGET` is how the system should behave. The images are third-party (product shots, ArtStation work, Unreal and Sims screenshots), so they are described here and never copied into the repository or used as textures.

What the 20 picks have in common decides the direction:

- **Realistic, not stylized.** Every body, face and garment pick is realistic or semi-realistic: a photoreal male turnaround, an athletic female render, a dense scan-like head wireframe, Unreal strand hair, Marvelous-Designer garments. None is stylized. CS1's character (big eyes, oversized hands, sculpted hair masses, smooth untextured shells) is the opposite end, which is why [ADR-0030](../../../wiki/Architecture/Architecture-Decisions/ADR-0030-Smooth-Stylized-Humanoid-On-CC0-Base-Meshes-With-Mask-Shapes.md) has to be superseded, not tuned (see the plan's decision D1).
- **The quality is in authored detail, not geometry alone.** Muscle definition, pores, fabric weave, stitching, quilting and fold sharpness come from textures baked from high-resolution sources (normal, cavity, roughness maps) on modest meshes. A character pipeline that produces only geometry and flat colours can't reach these images whatever its polygon count.
- **Systems, not one-offs.** Half the picks are `TARGET` images of *systems*: one outfit fitted to many bodies, a modular wardrobe in colour variants, a guide-group debug view, a weight-transfer import pipeline, a tag-filtered catalogue. The overhaul is judged on those behaviours as much as on looks.

"Semi-realistic by default" is therefore defined here as: **real human proportions and anatomy (about 7.5 heads, hands and feet at life size), physically based skin, eyes, hair and fabric shading with subsurface scattering, sheen and anisotropy, clean idealized skin (micro-detail, but no scan-level blemishes by default), and garments with real construction (seams, hems, pockets, closures) and gravity folds.** Stylized remains supported as a body family and a material style, not the default.

### A.1 Anatomy and body

| Reference | What it shows | Design characteristic → engine requirement |
|---|---|---|
| Pick `male_body_turnaround_front_side_back` (FINAL) | Photoreal male base body, front, side, back | Serratus, obliques, clavicles and scapulae read through the skin; the shoulder slopes; knees and feet are believable. → A realistic base body with **anatomical landmarks in the geometry** and **muscle definition in a normal map** that a Definition slider fades in |
| Pick `female_body_closeup_and_full` (FINAL) | Athletic female, close-up and full | Hip-to-waist ratio, breast volume sitting on the pectoral, knee and ankle shape. → The feminine frame must be authored anatomy, not the masculine mesh pushed around |
| Pick `body_topology_edge_loop_bands` (TARGET) | Wireframe with loop bands at every joint, cuff and waist | Even quads; dedicated rings at joints; loop bands where garments cut. → **One canonical topology** whose joint loops also define **body regions** (for hiding under clothes and for region sliders) |
| Web 05 Elliot Packer, male vs female studies | Matched sculpts, front, side, back | The dimorphism axis: shoulder-to-hip ratio, rib cage, pelvis tilt, thigh gap → the Frame slider's target |
| Web 09–11 écorché tools (Sketchfab, Anatomy for Sculptors) | Colour-coded muscle groups, bony landmarks | Muscle-group regions for sliders; **bony landmarks where fat never accumulates** (clavicle, elbow, knee, iliac crest) → masks on the Fat sliders |
| Web 12–13 marketplace base meshes | Published budgets: 27–29k triangles for a body | LOD0 body budget ≈ 25–30k triangles |
| Web 17 "Inside Humans" (Frontiers in VR, 2021) | Bone, muscle and fat as layered volumes | **Fat and muscle as separate layers**, not one Weight slider |

### A.2 Face

| Reference | Characteristic → requirement |
|---|---|
| Pick `head_wireframe_dense_facial_topology` (TARGET) | Concentric loops round the eyes and mouth, loops along the nasolabial fold and brow, density to hold wrinkles → the head's topology must allow wrinkles and folds, either in geometry (LOD0) or as **wrinkle normal maps blended by expression** |
| Pick `expression_blendshape_sheet_visemes` (TARGET) | Visemes A, O, E, U, P/B, blink, smile, frown and **pupil constrict and dilate** → expressions, visemes and pupil size as controls; per-shape thumbnails as the picker |
| Web 09–12 eye anatomy (Anatomy for Sculptors, 80 Level, Reallusion) | Separate sclera, iris, cornea, tear meniscus, caruncle; eyes angled slightly outward; parametric iris, limbus and sclera → **an eye built from parts with a refractive cornea**, iris pattern library, limbal ring and sclera veins as parameters |
| Web 13–14 XGen grooms | Brows and lashes grow in rows with clumping and asymmetry → brows and lashes as **hair cards or strands**, not painted planes |
| Web 15–16 MetaHuman head controls | Four tiers: preset blend → broad transforms → local sculpt → conform → the face editor's tiers |
| Web 17–19 ARKit 52 | The de facto interchange set → expression shapes named and mapped so ARKit capture drives them (the `arkit` face profile exists, F3) |
| Web 03 photoreal portraits | Subtle asymmetry by default; judge faces under dramatic light → an asymmetry slider and a hard-light stage preset |
| Web 20 hand-sculpted female rig | Skin compresses and wrinkles in expressions → corrective shapes or wrinkle maps per expression |

### A.3 Hair

| Reference | Characteristic → requirement |
|---|---|
| Pick `ue_strand_hair_with_subdivision` (FINAL) | Individual strands, flyaways, root-to-tip colour, multiple scattering in light-brown hair → **strand rendering with a hair shading model** at the top tier |
| Pick `groom_guide_groups_color_debug` (TARGET) | Bangs, braid and ponytail each a coloured guide group → **groom groups** with their own width, colour and physics, and a debug view that colours them |
| Pick `curly_hair_groom_color_and_clay` (FINAL) | Tight curls built from clumped helices → a **curl modifier** (radius, frequency, per-strand randomness) and volume; curly and coily styles are first-class |
| Web 01–07 Unreal groom docs | Strands at LOD0, cards below; ~5–10% guides simulated, the rest interpolated; groups; binding to the head surface → see B.1 |
| Web 11–12 Frostbite and *Dragon Age: The Veilguard* | Shipped strand hair; points per strand per asset; shadows and transparency over hair are extra work |
| Web 14–17 card hairstyles | ~22k-triangle card hairstyles; atlas layouts; sparse silhouette cards remove the "game" look; braids from cards → the **card tier must look good on its own**, since most players' GPUs and most NPCs will use it |
| Web 21 Reallusion Smart Hair | Beards and brows follow face morphs and expressions → facial hair bound to the face surface |

### A.4 Clothing

| Reference | Characteristic → requirement |
|---|---|
| Picks `zbrush_clothed_male_turnaround`, `quilted_jacket_cargo_outfit`, `denim_shirtjacket_turtleneck_cargo`, `cropped_denim_set_on_body_and_flat`, `ingame_characters_puffer_and_parka` (FINAL) | Gravity folds at elbows, compression at the waist, trouser break at the shoe; panel seams, topstitching, ribbed knit cuffs, quilting, frayed hems, edge wear; outerwear volume; fabric sheen → **pattern-simulated geometry with baked fold, seam and stitch detail**, per-fabric materials (denim, knit, leather, nylon, canvas) |
| Pick `modular_wardrobe_catalog_on_mannequins` (TARGET) | Hoodies, quarter-zips, tees, tank, jackets, cargo and jogger pants, sneakers, each in colour variants, all on one skeleton → **slots, colour and material variants per item**, one skeleton |
| Picks `metahuman_compatible_outfit_promo`, `cropped_puffer_cargo_fit_on_two_bodies`, `parametric_metahuman_wardrobe_leather` (TARGET) | One outfit fits every body of both sexes; the same garment on two bodies; resizable, two material variants; FBX, OBJ and glTF → **automatic fitting to any body**, a fitting test view, material variants, import from common formats |
| Web 03–05 MetaHuman parametric clothing | Author a few source sizes, warp from the nearest, mask the body under the garment, let users override the fit → see B.4 |
| Web 08 inZOI masking system, Web 09 Radical Heights | Garments declare body regions they hide and lower-layer regions they cover, by region IDs in a second UV channel → **region IDs for hiding and layering** |
| Web 10 Roblox layered clothing | Inner and outer cages let clothing stretch over any body and over other clothing → cage wrap as the alternative to per-size authoring |
| Web 12 Reallusion layers | Explicit layer order (under, mid, outer) with a collision push-out → layer order and push-out at fit time |
| Web 16–19 Marvelous Designer workflows | Garments from 2D patterns; fabric presets; ~1 mm skin offset for tight fits → patterns as the source of first-party garments (see B.4) |

### A.5 Footwear and accessories

No picks; web references only. Shoes need four or more material types (fabric, rubber, leather, foam) and about 5k triangles each; colour variants as texture sets on one mesh; toggleable sub-parts (gaiters, laces). Rigid accessories (hats, glasses, watches) attach to **named sockets that scale with the head or wrist**, with small user offsets; hats need **hat-safe hair variants**. Shoes go through the same fitting as clothes, because foot shape varies with the body.

### A.6 Skin and materials

| Reference | Characteristic → requirement |
|---|---|
| Web 04–05 Separable SSS (Jimenez et al.) | The single largest win for skin → subsurface scattering in the renderer (B.5) |
| Web 01–03 MetaHuman skin tools | Tone picker, texture sets by age, freckles, accent regions; customization **baked into final textures** at the end → a layered skin material at edit time, baked for games |
| Web 06 SKAP, Web 10–11 micro-detail | Multi-frequency detail; melanin and haemoglobin maps drive tone and flush; tiling micro-detail keeps base textures small; pore variance becomes roughness at distance → a tiling detail normal and specular-AA-style roughness fade |
| Web 08 skin realism | Flat albedo; a clear-coat-like oil layer on the T-zone and lips → a second specular lobe or clear coat on skin |
| Web 14–17 Unreal shading models, Charlie sheen | The renderer needs skin, hair, cloth (sheen) and clear-coat models; matte-fuzzy vs anisotropic-shiny fabrics → shading models per B.5–B.6 |
| Web 19 Marmoset presentation | A fixed studio lighting preset to judge materials consistently → the stage's Studio preset is the QA lighting |

### A.7 Interface and presentation

| Reference | Characteristic → requirement |
|---|---|
| Pick `sims4_cas_full_body_catalog` (UI) | A big live viewport, click-on-body navigation, a row of category icons, filter chips, a dense thumbnail grid, rotate arrows under the character |
| Pick `sims4_tops_tag_filter_panel` (UI) | Tag metadata (colour, material, category, style, source) powers filters → **every wardrobe item carries tags and a thumbnail** |
| Web 01–02 MetaHuman Creator | One-click camera framings (face, body, full) and lighting environments |
| Web 03, 05 Sims sliders | Direct manipulation on the model plus numeric sliders; swatches on each item tile |
| Web 12, 15 progressive disclosure | Simple mode first, advanced behind expanders; presets so it never becomes a wall of sliders |
| Web 17 runtime character creator | Keep UI separate from the character backend, so data drives the panels |
| Animation folder 13, 15–18 | A subtle breathing idle, not a T-pose; an auto turntable; a neutral editing mode and a beauty mode; **hair and cloth must move in the idle and walk to read as real** |

### A.8 Pipeline

| Reference | Characteristic → requirement |
|---|---|
| Pick `accessory_import_skin_weight_transfer_pipeline` (TARGET) | Import → align to the base → transfer skin weights from the body → classify by item type (which drives masking and simulation) → **the garment import pipeline to build** |
| Web 04 Blender → glTF → engine | glTF with named morph targets; validate on import |
| Web 11 makehuman.js | A modifier layer (UI-facing) over raw targets → **a data-driven parameter manifest over morph targets and bone transforms** |
| Web 12 Chaos Cloth | Garments come as a render mesh plus a low-resolution simulation mesh |
| Web 18 MetaHuman conform | Validate topology and UVs on import; reject or auto-wrap |

## B. Technical findings

Gathered by three research agents from primary sources, then checked by the lead where a decision rests on it. **✔** marks a claim the lead checked directly: against this repository's source, the vendored code, the local cargo registry, or a repository's declared licence through the GitHub API (2026-11-14). Other claims cite the source the researcher read. "Unverified" means the source couldn't be opened.

### B.1 Hair and grooms

**Unreal's Groom** ([hair rendering and simulation](https://dev.epicgames.com/documentation/en-us/unreal-engine/hair-rendering-and-simulation-in-unreal-engine)) is the reference architecture. Only *guides* are simulated; each rendered strand is matched to guides and follows them; a groom has groups (`groom_group_id`), each with up to eight LODs of type Strands, Cards or Meshes. Cards are skinned to their closest guide and carry depth, coverage, tangent and root-UV/seed textures. *Binding* a groom to a skeletal mesh stores each root's closest triangle and barycentric coordinates per mesh LOD, plus about 100 RBF samples, so head shapes move the roots. Its Alembic schema (v1.5) names the attributes: `groom_guide`, `groom_group_id`, `groom_root_uv`, `groom_width`, `groom_closest_guides` and `groom_guide_weights`. Simulation (Niagara "Groom Rods" or "Groom Springs") is an XPBD loop over bend and stretch with physics-asset collision and wind. Shading follows Karis 2016: Marschner's R, TT and TRT lobes, heavily approximated, with multiple scattering described in the talk as "a giant artistic hack" based on shadow path length.

**Shipped strand renderers that need no mesh shaders:**

| Title (source) | Approach | Cost |
|---|---|---|
| *Indiana Jones and the Great Circle* (SIGGRAPH 2025 Advances) | Randomized strand order cut to a thickness and segment budget for LOD; Müller 2012 position-based simulation with a head SDF; a **compute rasterizer** into a 3-layer 32-bit visibility atlas with additive coverage; per-vertex shading with baked per-vertex self-shadowing; composited as a sorted transparent quad with motion vectors | About 1.6–2.4 ms on consoles. **The best blueprint for us** |
| *Dragon Age: The Veilguard* (EA, 2024) | A bespoke compute software rasterizer, 50k strands per character; cards on Xbox Series S and low PC settings | About 3 ms at 60 fps |
| Frostbite (SIGGRAPH 2019) | Marschner with a TT lookup texture, dual scattering with deep opacity maps; visibility buffer with MSAA | About 9 ms for long hair on PS4 |
| *Resident Evil 4* | Hardware billboards plus a compute rasterizer with an 8-layer blend; a 32-bit fallback when 64-bit atomics are missing | n/a |
| Lipp et al. 2026 (CC BY) | Software rasterizer 4.4× faster than mesh shaders; needs only compute, indirect dispatch and 64-bit atomic min | n/a |

**Patent:** EA's US 11,551,387 B2 (in force until about 2041, as reported by the researcher) covers a tile-binned, depth-clustered hair rasterizer. Any compute rasterizer design needs a legal review before it ships. Other patents weren't searched.

**Libraries and their status:**

| | Licence | Status | Verdict |
|---|---|---|---|
| AMD TressFX | ✔ MIT | Last release v4.1.0 (2020-01-24); ✔ repository untouched since 2023-01; TressFX 5.0 (2025) only as Unreal patches behind Epic's GitHub access, licence unstated | **Reference for the simulation**: Verlet with global and local shape constraints, velocity shock propagation, length, wind and capsule passes, SDF collision (4.0). Its rendering (per-pixel linked lists or "ShortCut", Kajiya-Kay) and HLSL/DX12 sample code aren't reusable as is |
| NVIDIA HairWorks | Closed; GameWorks members only, revocable | Legacy (DX11 tessellation; DX12 not available in the last public notes) | Visual reference only |
| NVIDIA RTX Hair (Linear Swept Spheres) | RTXCR SDK, NVIDIA proprietary | Hardware support on RTX 50 only | Reject |
| Unity hair package | Unity Companion License | Unity-only | Reject |
| vkhr | MIT | Research renderer | Reference |

**Shading:** Karis 2016's R lobe plus Tafuri 2019's TT/TRT through one two-channel lookup texture, evaluated per strand vertex, suits a forward renderer. Multiple scattering, cheapest to best: shadow-depth absorption, dual scattering (Zinke 2008) with a density volume, baked visibility. Chiang 2016 (Cycles' Principled Hair) is the offline reference; Kajiya-Kay the cheapest fallback. Artists pick colour through **melanin and redness**, as in Cycles and Unreal.

**Cards:** generated offline from the same strands (Epic's experimental Hair Card Generator; Strands2Cards, SIGGRAPH Asia 2025, has no licence and needs CUDA) with an atlas of coverage, depth, root-to-tip U, seed and flow/tangent; anisotropic highlights by shifting the tangent along the normal with a flow map, two lobes. For alpha, a forward renderer with TAA fits **hashed alpha** (Wyman 2017) resolved by TAA plus an opaque core; alpha-to-coverage needs MSAA.

**Simulation, cheapest convincing:** a few hundred guides with Verlet or XPBD length constraints, TressFX-style global and local shape constraints and velocity shock propagation, capsule (and optionally head SDF) collision and wind as a force; rendered strands interpolate up to three guides. Cosserat rods (Kugelstadt 2016) only pay off for braids and twist. For cards and mesh hair, **VRM's spring bones** (VRMC_springBone: Verlet with stiffness, gravity, drag and sphere or capsule colliders) are the bone-based option.

**Authoring and interchange:** Blender 4.2+ exports hair curves to Alembic and USD BasisCurves. **No glTF extension for curves or hair exists** (issue #1258 closed in 2018). ✔ The workspace already pins the `openusd` crate (BasisCurves), so a groom converts on import to an engine-own file (ADR-0027).

**Curly and coily hair** is authored with curl generators (Blender's Curl Hair Curves; Houdini's coil modes), multiplying vertex counts; Lifted Curls (SCA 2023) simulates tight coils. The SIGGRAPH 2024 course "More Than Killmonger Locs" (Code My Crown; the Andre Walker hair-type system; locs, braids and cornrows) and GDC 2024's "Authentic Representation of Black Hair in Games" are the references for doing it respectfully; cards clip visibly on curly hair, especially light hair, so curly and coily styles need the strand tier or very careful cards.

**wgpu 30** (✔ checked in the local 30.0.1 source): compute, storage buffers, 32-bit atomics, workgroup memory and indirect dispatch are core; `SUBGROUP` and `SHADER_INT64_ATOMIC_MIN_MAX` are optional features (DX12 SM 6.6, Vulkan); `EXPERIMENTAL_MESH_SHADER` exists but is experimental. ✔ The engine requests none of them today. A compute rasterizer needs only the core set, with faster paths behind the optional ones.

### B.2 Bodies: what we may ship, and how to parameterize them

**Base meshes and shape data an Apache-2.0 engine may bundle:**

| Source | Licence | What it gives | Verdict |
|---|---|---|---|
| Blender Studio Human Base Meshes v1.4.1 (2026-01-20, latest) | CC0 1.0 | Already vendored (stylized bodies). ✔ The same bundle has **realistic** male and female bodies that share one topology (10,582 vertices each, 21,160 triangles, all quads but 20), a realistic animation head with separate iris and sclera meshes, realistic hands, feet and teeth, and a full anatomical skeleton (skull, ribcage, every bone of the hands and feet). Blender Studio says the realistic male is based on scan data. ✔ The realistic and stylized bodies do **not** share topology (12,502 vertices) | **Adopt the realistic bodies as the default base**; keep the stylized pair as a second body family |
| MakeHuman system assets (targets, skins, proxies) | CC0 (assets); code AGPL, MPFB2 code GPL ✔ (MPFB2 repository declares no single licence) | Hundreds of artist-made targets: gender, age, muscle and weight corner targets, proportions, and face features | **Adopt as data only** (never code), transferred onto our topology |
| Anny (NAVER, Nov 2025) | ✔ code Apache-2.0; its body data is MakeHuman-derived CC0 (README); its optional SMPL-X files are non-commercial | A modern packaging of MakeHuman's shape space covering all ages, with 104 bones | **Reference and data source**, never its SMPL-X files |
| Google GNM Head v3 (2026) | ✔ repository Apache-2.0; weights on Hugging Face Apache-2.0 (model card) | A head model with 170 identity, 3 eyeball, 80 teeth and 382 expression components | **Spike**: an identity space for plausible face sliders, if its topology can be wrapped onto ours |
| ICT FaceKit Light | ✔ MIT (repository last pushed 2020) | 100 identity PCA shapes and 53 ARKit-named expressions | **Spike**, as GNM |
| CharMorph | Vitruvian body CC0; Antonia and Reom CC-BY; MB-Lab base AGPL | Bodies and morphs | Reference only |
| FLAME 2023 Open | CC-BY-4.0 with bans on pornographic and defamatory use that pass on to users | Head model | Avoid: the use restrictions would bind our users |
| SMPL, SMPL-X, STAR, SUPR, older FLAME | Non-commercial, no redistribution | Statistical bodies | **Cannot bundle** |
| MetaHuman assets | Unreal EULA since UE 5.6 (June 2025): usable in other engines, but sample assets are under the Fab Standard License, which bans redistributing them alone. Epic's licence pages couldn't be opened (unverified) | The quality bar | **Cannot bundle.** Users may import their own |
| OpenRigLogic (RigLogic and DNA libraries) | ✔ MIT (2026); Epic's trademark policy bars "MetaHuman" or "RigLogic" in our product names | Evaluates MetaHuman DNA rigs | Optional importer later |
| MetaHuman DNA Calibration | Proprietary licence forbidding redistribution | DNA editing | **Cannot ship** |
| 3D Scan Store samples, Texturing.xyz, Digital Emily 2 | Personal, "not under an Open Source License", non-commercial | Scan detail and textures | **Cannot bundle** |

**How production creators parameterize bodies:**

- **MetaHuman (UE 5.6+)**: statistically driven global sliders (Masculine–Feminine, Muscularity, Fat, Height) and region groups; the model enforces its own ranges and links sliders (raising muscularity changes fat unless fat is *pinned*); measurement sliders in centimetres with clamps relative to height. Source: [Body Params tool](https://dev.epicgames.com/documentation/metahuman/metahuman-creator-body-params-tool-in-unreal-engine).
- **MakeHuman** (from its source): each macro slider splits into min/average/max factors, and each *corner* target (for example female-young-averagemuscle-maxweight) gets the product of its factors as its weight, so fat and muscle interact without cancelling. This is how a handful of macro sliders stay plausible across their whole range.
- **The Sims 4**: separate HEAVY, FIT, LEAN and BONY body-shape maps per region (see B.4).
- **Fat and muscle** are separate layers in all three, never one Weight slider.
- **Statistical (PCA) body spaces** keep random combinations human, but every open scan dataset that would train one is non-commercial: not available to us.

**Skinning artefacts and their fixes:**

| Fix | What it does | Cost | Verdict |
|---|---|---|---|
| Twist and helper bones | Spread forearm and upper-arm twist (MetaHuman blends upper-arm twist 0.8/0.2); RBF-driven helper joints at the pecs, lats and ankles | A few bones; drive them after the pose | **Adopt first** |
| Pose-driven corrective shapes | 10–20 shapes at elbows, knees, shoulders, hips and wrists, weighted from joint angles (RBF or simple cones) | Morph cost only when bent | **Adopt** |
| Dual quaternion skinning (Kavan 2008) | Fixes candy-wrapper twist; bulges at bends | About the same as linear skinning | Option, per mesh |
| Optimized centres of rotation (Le & Hodgins 2016) | Per-vertex centres; must be recomputed when the body shape changes | Precompute per character | Later; patent status unchecked |
| Delta Mush, Direct Delta Mush | Smooths skinning artefacts | Heavy per frame or heavy storage | Defer; patent status unchecked |

**A key cost rule** for the runtime: identity shapes (frame, build, proportions, face identity) change only when the character is edited, so they can be **baked into one per-character rest mesh** whenever sliders change. Per frame, only expressions and joint correctives morph. This turns "hundreds of sliders" into zero per-frame cost and suits the renderer's morph cost (audit §3.10).

### B.3 Faces and eyes

- **MetaHuman's face is mostly joints**: head LOD0 has 713 joints and 669 blend shapes with 12 influences per vertex; LOD7 has 26 joints; blend shapes and animated wrinkle maps are used only at LOD0–1 ([LOD specifications](https://dev.epicgames.com/documentation/metahuman/platform-support-and-lod-specifications-for-metahumans)). The [RigLogic whitepaper](https://cdn2.unrealengine.com/rig-logic-whitepaper-v2-5c9f23f7e210.pdf) describes about 800 joints, 200 controls and 1,000+ corrective expressions. Copying that rig is neither authorable here nor affordable at runtime; Axcenity's 48-control FaceControls (F3) with authored shapes and correctives is the right scale, with OpenRigLogic as an optional importer.
- **The face editor's tiers** (MetaHuman): blend up to three presets locally or globally, then broad transforms, then local sculpt, then conform a custom mesh.
- **Plausible faces**: MetaHuman projects sculpt edits onto a space learned from scans. An Apache-2.0 or MIT identity space (GNM, ICT FaceKit; B.2) could play that role if it wraps onto our head topology; otherwise region shapes with linked clamps.
- **Eyes** are separate meshes in every production rig: MetaHuman ships eyeLeft, eyeRight, an eye shell (occlusion), eyeEdge (tear line), eyelashes and an eyelash shadow per LOD; Unity HDRP's eye shader is two layers with cornea refraction (IOR about 1.33–1.376) and a limbal ring. ✔ The CC0 realistic head already has separate iris and sclera meshes.
- **Expressions**: ARKit's 52 shapes are the interchange set; the engine's `arkit` profile already maps them (F3). Wrinkle maps blended by expression are a LOD0-only realism step.

### B.4 Fitting clothes to any body

| Approach | How it works | Strengths | Weaknesses |
|---|---|---|---|
| Per-garment copies of body shapes (Axcenity today) | The build gives each garment its own copy of each body shape | Exact for built garments | Garments × shapes storage; imported garments can't follow; every new body slider means rebuilding every garment |
| MetaHuman Outfit Asset (UE 5.6) | Authored on a few *source bodies*; warps by the difference between the source and the target body, choosing the closest source; a coarse sim mesh drives the render mesh; a hidden-face map removes covered body faces ([parametric clothing](https://dev.epicgames.com/documentation/metahuman/getting-started-for-creating-parametric-clothing-in-metahuman)) | Large shape changes look right | Needs several authored sizes per garment |
| The Sims 4 deformer maps (community-documented, GDC 2015 talk "Innovations in The Sims 4 Character Creator") | Body shape deltas and normal deltas are stored in textures over the body's second UV set, with a skin-tight and a loose ("robe") channel; every garment carries that UV set copied from the body; body and clothes sample the same maps, so any garment follows every slider, including future ones | Any number of sliders, no per-garment cost; works for imported garments | Texture resolution limits small regions; needs unique body UVs; loose garments are approximate |
| Surface binding (Blender Surface Deform, Reallusion Conform) | Each garment vertex is bound to a body triangle (barycentric coordinates plus an offset along the normal); after the body changes, the garment is rebuilt from the binding; push-out with a margin | Exact for tight garments; works from geometry alone; cheap to bind at import | Artefacts grow with distance from the body; the body surface must be well formed |
| Cages (Roblox layered clothing) | Inner and outer cages wrap clothing over bodies and over other clothing | Works over other layers | Needs authored cages per item |
| Simulation (CLO, Marvelous Designer) | Re-drape while the avatar morphs | Best result | Offline only |

**Body hiding** is done by per-garment face maps in every product: MetaHuman's hidden-face map, Unreal Mutable's Remove Unseen, Reallusion's per-face hide, inZOI's region IDs in a second UV channel (which also say which lower *layers* a garment covers).

### B.5 Skin

| Technique | Needs | Notes |
|---|---|---|
| Pre-integrated skin (Penner 2011) | A small lookup table by N·L and curvature, inside the lighting function | No new render targets; good for a forward renderer; no light bleeding across shadow edges |
| Separable SSS (Jimenez et al. 2015) | A diffuse-only target, two 1-D blurs, under 0.5 ms, 7 jittered samples | Reference code BSD-2-style and **requires the credit line** "Uses Separable SSS. Copyright (C) 2011 by Jorge Jimenez and Diego Gutierrez." |
| Burley normalized diffusion (Christensen and Burley 2015) | As separable, with a profile that is a sum of two exponentials | Unreal's Burley mode requires TAA |
| Godot 4 Forward+ (MIT) | Writes specular to a separate target and blurs the diffuse colour buffer | **The closest precedent**: screen-space SSS in a forward renderer |

Specular: two lobes (Unreal's defaults: roughness × 0.75 and × 1.3, mix 0.85), F0 ≈ 0.028 for skin, lowest roughness on the nose and forehead (Weyrich 2006), micro-detail normals tiled over the base maps and faded to roughness at distance. Customization (tone, undertone, freckles, makeup) is layered at edit time and baked into the final textures for games (MetaHuman's approach).

### B.6 Fabric

[KHR_materials_sheen](https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_sheen) (ratified) defines the Charlie sheen (Estevez and Kulla 2017) with an albedo-scaling formula, the standard for cotton, wool and velvet; KHR_materials_anisotropy covers satin and silk. Filament's cloth recipes: velvet is a dark base colour with the colour in the sheen; denim keeps chroma in the base colour with default sheen. No measured roughness table for denim, knit, leather or canvas was found; presets will be tuned against references. Construction detail in game garments: geometry for buttons, collars and thick hems; normal-map trim sheets for stitching; mesh decals for prints and labels.

### B.7 Cloth simulation

- **XPBD** (Macklin et al. 2016) and **Small Steps** (Macklin et al. 2019): many substeps with one iteration each beat many iterations. Chaos Cloth's model: vertices painted with max distance 0 are skinned; others move within max distance, backstops, an animation drive and long-range tethers.
- **Long-range attachments** (Kim et al. 2012): NVIDIA holds US patent 9,070,220 on it; expiry unchecked. **Check before implementing tethers.**
- **Rust:** no mature cloth crate (bevy_silk, rapier-cloth-core and bevy_softbody are young; havok-cloth is GPL-3; NvCloth uses NVIDIA's own licence). ✔ But the vendored **rapier 0.36.1 already has soft bodies**: `cloth`, `cloth_tube`, `cloth_anisotropic` and `rope` builders, dihedral bending, particle attachments and mesh bindings (`vendor/rapier3d/src/dynamics/soft_body/`). No new dependency is needed to try cloth.
- Production practice simulates only what swings (skirts, coat tails, hoods, drawstrings), on a coarse proxy driving the render mesh; the rest is skinned with correctives.

### B.8 Formats and interchange

**glTF stays the runtime format.** Extension status (Khronos registry): ratified: sheen, specular, clear coat, IOR, volume, anisotropy, **variants** (fits outfit colour variants exactly), texture_basisu, mesh_quantization, draco, animation_pointer, EXT_meshopt_compression; release candidate: diffuse_transmission; **no ratified subsurface extension** (KHR_materials_subsurface closed in 2024; KHR_materials_scatter open as of 2026-11-13).

✔ What Axcenity's loader reads today (`crates/assets/src/model.rs`): triangles only; POSITION, NORMAL (smooth normals made when missing), **one** UV set, JOINTS_0/WEIGHTS_0 renormalized to 4 influences (JOINTS_1 ignored); TANGENT **not read**, recomputed by area weighting, not MikkTSpace; morph POSITION and NORMAL deltas, names from `extras.targetNames`, default weights (TANGENT deltas dropped); no COLOR_0; no KHR_materials_* extension; files that *require* quantization, meshopt, draco or basisu fail to load (`gltf` 1.4.1 with only `KHR_texture_transform` and `extras` enabled, `Cargo.toml:99`). Enabling the crate's `extensions` feature gives raw JSON for any extension. The released crate is 1.4.1 (2024-05); its main branch is active but unreleased.

| Technology | Licence | Rust support | Verdict |
|---|---|---|---|
| VRM 1.0 (VRMC_vrm, springBone, mtoon) | UniVRM and three-vrm MIT; models carry the VRM Public License | `vrm-spec`, `serde_vrm` | Later: an importer that shows each model's licence fields. Build spring bones natively first |
| OpenUSD | TOST-1.0 (Apache-2.0 with a different trademark clause) | ✔ `openusd` and `openusd-schemas` 0.8.0 already pinned (editor-only, `Cargo.toml:157-160`); have skeletons and BasisCurves | **Adopt for conversion** (ADR-0027): UsdSkel and BasisCurves → glTF and a groom file |
| Alembic | BSD-3-Clause | No usable crate | Reject for now; hair comes through USD |
| MaterialX | Apache-2.0 | Placeholder crates; its WGSL generator emits WGSL-compliant GLSL | Reference only; use OpenPBR as the model for new lobes |
| OpenSubdiv | TOST-1.0 | C++ bindings or a young pure-Rust port | Reference only: wgpu has no tessellation; author LODs offline |
| MikkTSpace | zlib-style | ✔ `meshopt` 0.7.0 (already a dependency) has `generate_tangents` (registry source `tangents.rs:113`) | **Adopt** through meshopt: baked normal maps need MikkTSpace tangents |
| meshoptimizer | MIT | Already used | Use more: regularized simplification for skinned LODs, locks on expressive regions and region borders, the morph magnitude as a simplification attribute; never `simplifyWithUpdate` on meshes with shapes |
| KTX2 / Basis Universal | Apache-2.0 | `ktx2`, `basisu` | Reference only: textures are already cooked to BC7/BC5 with `intel_tex_2` (✔ pinned); add BC4 for single-channel maps |

**Texture budget** (computed): at 4K with mips a BC7 or BC5 map is 21.3 MiB, uncompressed RGBA8 85.3 MiB; a 4K head set is about 120 MiB and a whole hero character 200–300 MiB. 4K is for the studio close-up; games default to 2K or 1K.

## C. Recommended technologies

| Area | Recommendation | Why, over the alternatives |
|---|---|---|
| Base body | Blender Studio Human Base Meshes **realistic** pair (CC0), in the same bundle `extract.py` takes the stylized bodies from | Shippable, scan-based, clean animation topology, masculine and feminine share one topology. MetaHuman and the SMPL family can't be bundled |
| Body shapes | MakeHuman CC0 targets (data only) transferred onto our topology, mixed MakeHuman-style (product-weighted corner targets), plus region shapes and #400's bone sliders | Artist-made anatomy we can't sculpt ourselves; the corner mixing keeps fat and muscle plausible across their range. PCA bodies need non-commercial data |
| Face identity | Region shapes now; spike an Apache-2.0 or MIT identity space (GNM Head, ICT FaceKit) for plausible combinations | A learned space stops broken faces; only these two have usable licences |
| Expressions | Keep FaceControls (48 controls, ARKit mapping); author shapes on the new head; wrinkle normal maps at LOD0 later | The runtime exists (F3–F7) |
| Skinning | Linear blend plus twist and helper bones and 10–20 pose-driven corrective shapes; dual quaternions optional per mesh | Cheap, fixes the visible artefacts; the heavier fixes are costly or possibly patented |
| Identity cost | Bake identity shapes into a per-character rest mesh when edited; morph only expressions and correctives per frame; then pose once per frame in compute | Hundreds of sliders at zero per-frame cost; the renderer re-morphs in every pass today |
| Clothing fit | Surface binding to the body made at import (barycentric plus offset), rebuilt when the body changes, with push-out against inner layers; body-UV hiding masks | Works for any garment, including imported ones, and any future body slider; no per-garment shape copies. Sims-style delta textures are the fallback if binding quality fails on loose garments |
| Cloth simulation | Spring bones first (hoods, drawstrings, skirts' bones); then the vendored rapier soft-body cloth (or our own XPBD) on coarse proxies for skirts, coats and capes | No new dependency; simulate only what swings |
| Skin | Phase 1 pre-integrated skin with dual-lobe specular, cavity and micro-detail normals inside the lighting function; Phase 2 screen-space separable SSS (Godot's forward-renderer approach) | Phase 1 needs no new targets; Phase 2 needs a diffuse target and a skin mask |
| Eyes | Separate sclera, iris and cornea meshes with an occlusion shell and tear line; iris refraction by parallax; a cornea lobe | The CC0 realistic head already splits iris and sclera |
| Fabric | A cloth shading model: Charlie sheen and anisotropy (glTF's sheen and anisotropy extensions), tiling weave detail normals, trim sheets for stitching | Matches the ratified glTF extensions, so imported garments carry their values |
| Hair | Three tiers from one groom: **cards** (hashed alpha with TAA, flow-map anisotropy, spring bones) first, since every GPU and NPC needs them; **strands** next (compute pipeline: skin, simulate guides, interpolate, rasterize in compute, composite with motion vectors); **mesh** fallback | The Indiana Jones blueprint needs only core wgpu compute. TressFX, HairWorks, mesh shaders and linked-list OIT are rejected (frozen, closed, experimental, unbounded memory) |
| Groom files | Authored in Blender as hair curves, exported to USD BasisCurves, converted on import to an engine groom file (guides, strands, groups, widths, root binding) | No glTF curves standard; USD already in the workspace (ADR-0027) |
| Interchange | glTF with the material extensions read (sheen, specular, clear coat, anisotropy, variants); MikkTSpace tangents through meshopt; constrained character LODs | Imported garments keep their fabric; baked normal maps match |
| Textures | BC7/BC5 (existing) plus BC4 for single-channel maps; 2K by default, 4K for the studio | Fits memory budgets |

## D. Not verified

- Epic's EULA, the MetaHuman licence page and the UE FAQ (blocked); how the MetaHuman outfit warp works in detail; what its `cartilage` and `eyeEdge` meshes do.
- Patents: EA's hair rasterizer patent (expiry and claims), NVIDIA's long-range attachment patent (expiry), and anything on optimized centres of rotation or Delta Mush. **A legal check is needed before implementing a compute hair rasterizer or cloth tethers.**
- GNM Head's vertex count and topology, and whether it wraps onto our head; the licence of SOMA-X's data; the STAR and SUPR licence texts; 3D Scan Store's current terms.
- The VRM specification's own licence; the TressFX 5.0 patches' licence; HairWorks' current status.
- Unreal's hair visibility-buffer details and its 2020 hair blog (403); the Frostbite 2020 slides; primary hair sources for Luminous, *Ghost of Tsushima* and *The Last of Us Part II*.
- Measured fabric roughness values; Weyrich's per-region skin values; the Sims 4 GDC talk beyond its abstract (the deformer-map mechanism is from community reverse engineering, not EA).
- Papers the researchers cited but didn't open: Kajiya-Kay, Marschner, d'Eon, Zinke, Chiang, Kugelstadt, Lifted Curls.
- How rapier's soft-body cloth performs on skinned garments (no benchmark run).

## E. Research notes

The researchers' full notes, with every URL, date, licence and how each claim was checked, are kept as appendices for the specialists who implement each area. They are evidence, not decisions: where they disagree with this file, this file and the lead's checks win.

- [notes/hair.md](notes/hair.md): Unreal Groom, TressFX, HairWorks, shipped strand renderers, shading, cards, simulation, authoring, wgpu feasibility, curly and coily hair
- [notes/body-face-skin-cloth.md](notes/body-face-skin-cloth.md): MetaHuman, shippable bases and licences, body parameterization and skinning, The Sims 4's deformer maps, garment fitting, skin and eyes, fabric, cloth simulation
- [notes/formats-and-libraries.md](notes/formats-and-libraries.md): glTF and its extensions (and what the loader reads), VRM, OpenUSD, MaterialX, OpenSubdiv, MikkTSpace, meshoptimizer, Alembic, KTX2 and Basis, cloth and SDF crates
