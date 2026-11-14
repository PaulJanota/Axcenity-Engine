# Character Studio overhaul: implementation plan

**Status: proposal awaiting Paul's approval (Stage 1 checkpoint). Nothing here is decided or built until he approves it.**

The plan for Paul's overhaul brief of 2026-11-14. It rests on [research.md](research.md) (evidence) and [audit.md](audit.md) (what exists and what's wrong), and on these wiki pages and ADRs: [Animation](../../../wiki/Engine-Systems/Animation.md), [Characters](../../../wiki/Engine-Systems/Characters.md), [Rendering](../../../wiki/Engine-Systems/Rendering.md), [Importers](../../../wiki/Asset-Pipeline/Importers.md), [ADR-0027](../../../wiki/Architecture/Architecture-Decisions/ADR-0027-Convert-New-Model-Formats-To-glTF-On-Import.md) (convert on import), [ADR-0028](../../../wiki/Architecture/Architecture-Decisions/ADR-0028-Facial-Animation-Morph-Targets-On-GPU-And-FaceControls.md) (morph targets and FaceControls), [ADR-0030](../../../wiki/Architecture/Architecture-Decisions/ADR-0030-Smooth-Stylized-Humanoid-On-CC0-Base-Meshes-With-Mask-Shapes.md) (the stylized humanoid, which D1 and D6 would supersede) and [ADR-0031](../../../wiki/Architecture/Architecture-Decisions/ADR-0031-Character-Studio-Is-A-Workspace-Of-Its-Own.md) (the workspace, kept).

It replaces stages CS3–CS6 of [character-studio.md](../../research/character-studio.md): their content (a runtime description, live customization, face and animation categories, character files, polish) is folded into the stages below. The face runtime of [facial-animation.md](../../research/facial-animation.md) (F1–F7) is reused unchanged.

## 1. In one paragraph

Keep what works: the skeleton contract, GPU morph targets, the whole face runtime, modular parts, PR #400's bone sliders and sockets, and the workspace shell. Replace the character itself with a **semi-realistic one built on the realistic CC0 bodies in the same bundle the stylized ones are vendored from**. Give the engine **one runtime character description** that the player can read, driven by a **data manifest** instead of Rust enums. Make clothing **fit any body from its geometry** instead of per-garment shape copies. Move the wardrobe **out of the binaries into a content library**. Teach the renderer **skin, eye, cloth and hair shading**. Build hair as **one groom with card, strand and mesh tiers**, cards first. The hardest part is not code but **art production**: the target look in Paul's references comes from sculpted, simulated and baked assets. This plan builds a scripted high-to-low asset pipeline and an import path for artist-made items, and is honest about where generated content will fall short (section 8).

## 2. Decisions for approval

Each decision is reversible until its stage starts. **Recommended** options are first.

| # | Decision | Recommended | Alternatives and why not |
|---|---|---|---|
| D1 | Art direction | **Semi-realistic by default** (research A.0's definition) on Blender Studio's **realistic** CC0 bodies; the stylized pair stays as a second *body family*. Supersedes ADR-0030's art direction | Keep stylized default: contradicts every reference pick. MakeHuman's base mesh: dated topology (its *targets* are used, D5). MetaHuman: can't be bundled. A scan base: no shippable licence |
| D2 | The character's data | **One versioned `*.character.json` (format 2)** read by player, server and editor; a **`HumanoidDescription`** Instance (Roblox-familiar) that references or holds it; `Humanoid:ApplyDescription`. Format 1 (`Look`) migrates on load. #400's body attributes become one *output* of applying it | Attributes only: no structure for a wardrobe and materials. Editor-only (today): games can't use characters |
| D3 | What sliders do | **A parameter manifest shipped with each base model**: each parameter (id, label, group, range, default, symmetry, basic or advanced) maps to morph targets (with corner-product mixing), bone transforms and material values, with links and clamps between parameters. **The workspace draws its panels from it** | Rust enums (today): every slider is a code change |
| D4 | Runtime cost of identity | **Bake identity shapes into a per-character rest mesh** when the description changes (shared by characters with the same description); morph per frame only expressions and joint correctives; later pose once per frame in compute (D9) | Leave every shape live: the renderer re-morphs in every pass (audit §3.10) |
| D5 | Body model | One topology per body family. **Frame** (masculine ↔ feminine, a blend) × **composition** (fat and muscle as separate layers, global and per region, mixed MakeHuman-style) + **proportions** (bones, #400) + **face identity** (regions; a learned identity space as a spike). MakeHuman's CC0 targets transferred onto our topology as the anatomy source. **Twist and helper bones** (added to the skeleton, not renamed) **and 10–20 pose-driven correctives** | Script-sculpted inflation shapes: read as blobs, not anatomy. A statistical body: no shippable data |
| D6 | Clothing | Wardrobe **items** with a manifest (slot, layer, regions hidden, materials and variants, tags, thumbnail). **Fitting by surface binding** (each garment vertex bound to the body, made at build or import, rebuilt when the body changes) with **push-out between layers**; **body hiding by regions** marked in the body's UVs, replacing mask shapes (supersedes ADR-0030's masks). Rigid accessories on #400's sockets | Per-garment shape copies (today): don't scale and can't fit imported garments. Sims-style delta textures: kept as the fallback if binding fails on loose garments. Authored sizes per garment (MetaHuman): too much authoring |
| D7 | Where content lives | **A character content library on disk**, outside the binaries: base model packages, items and grooms, each a glTF plus a small manifest, listed by a catalogue. One default character stays embedded for new projects. Projects copy what they use (the existing global assets path) | `include_bytes!` (today): every item grows every binary |
| D8 | How first-party art is made | **A scripted high-to-low pipeline in Blender**, extending `build.py`: garments from 2D pattern panels sewn and simulated with Blender's cloth, decimated or retopologized, with normal, cavity and AO maps baked from the high mesh; trim sheets for stitching; procedurally baked skin detail maps; hair from Blender hair curves. Plus **an import path for artist-made items** (glTF, FBX, OBJ, USD), so better art can replace any generated item | Hand modelling: not possible in this workflow. Buying packs: licences rarely allow redistribution in an Apache-2.0 engine |
| D9 | Rendering | New shading models **Skin** (pre-integrated diffusion with two specular lobes first, screen-space SSS second), **Eye** (iris parallax, cornea lobe), **Cloth** (Charlie sheen, anisotropy), **Hair** (Karis/Tafuri lobes). To make room: metalness, roughness and occlusion become one packed texture, freeing two material slots. Read glTF's sheen, specular, clear-coat, anisotropy and variants extensions; MikkTSpace tangents; hashed alpha with per-frame noise under TAA; cut-out-aware shadows; a compute pass that morphs and skins once per frame | Custom WGSL surfaces: can't change lighting (audit §3.10). Raising device limits: possible later; packing is cheaper and portable |
| D10 | Hair | **One groom asset** (Blender hair curves → USD BasisCurves → an engine groom file, ADR-0027) with **three tiers**: **cards first** (every GPU and every NPC needs them), **strands second** (a compute pipeline after the Indiana Jones blueprint), **meshes** as the fallback. Spring bones for cards; guide simulation for strands; facial hair bound to the face. **A legal review of hair-rasterizer patents before strands ship** | Strands first: the most visible win, but the most risk and no help for crowds or low-end GPUs. TressFX or HairWorks: frozen or closed |
| D11 | Cloth simulation | Only what swings: **spring bones first** (hoods, drawstrings, skirt and coat-tail bones), then **cloth on coarse proxies** using the vendored rapier soft bodies (no new dependency) or our own XPBD | Simulating whole garments: cost and clipping for little gain |
| D12 | The interface | Character Studio **stays a workspace** (ADR-0031), **also reachable from the Game Editor** (a menu item and "Edit in Character Studio" on a selected character, with a round trip back). The Game Editor's face tab is renamed (for example "Face Rig") so only the workspace is called Character Studio; its editors also appear in the workspace's Face category | Merging into the Game Editor: the brief says not to bury it |
| D13 | How work is split | A lead (architect and integrator) and six specialists with **disjoint file ownership**, each in **its own git worktree and branch**; shared contracts land first and only the lead changes them (section 5) | All agents on one branch: conflicts and silent redesigns |

## 3. Architecture direction

Stage 2 turns this into `architecture.md` with exact schemas and signatures. The direction:

```text
 Content library (disk)                     Description (data)                     Runtime (player, server, editor)
 ─────────────────────────                  ──────────────────────                 ───────────────────────────────────────────
 base model package                         *.character.json v2                    HumanoidDescription ── ApplyDescription
   glTF body + head + eyes, skeleton          base family, frame                         │
   parameter manifest  ───────────────┐       parameter values                           ▼
   region map, sockets                │       items worn (+ variant, colours)      avatar: resolve (manifest × values)
 wardrobe items                       │       groom + hair colours                   ├─ bone transforms ──────► anim (after pose: #400 sliders,
   glTF + item manifest               ├────►  skin and makeup layers                 │                              offsets, twist bones, correctives)
   binding to the body                │       face identity values                   ├─ baked rest meshes (body, fitted items) ─► Assets (derived models,
 grooms                               │                                              │                              keyed by description hash)
   groom file + cards + mesh          │                                              ├─ material parameters ───► scene ─► render
 catalogue (tags, thumbnails) ────────┘                                              └─ parts list ────────────► MeshParts (followers)
```

- **A new crate**, working name `avatar` (the name `character` is taken by the controllers), owns the description schema, the manifests, migration from format 1, resolving a description into bone transforms, shape weights, material parameters and a parts list, fitting (binding → fitted rest positions, layer push-out) and the identity bake. It depends on `core` and `assets` only, so `player`, `server` and `editor` can all use it. It contains no rendering and no UI.
- **`anim`** keeps posing: #400's sliders, the frame's bone offsets, twist bones and correctives run after the animated pose, before skinning, so clips play unchanged. Followers take the leader's shape weights by name as well as its bones (audit §3.2).
- **`assets`** gains *derived models*: a base model plus a description hash, cached and shared, so 50 NPCs with one look share one baked mesh.
- **`render`** stays free of Instances (CLAUDE.md invariant). New shading models and passes take plain data from `scene`; strand hair arrives as a new `DrawList` field, as particle emitters do.
- **`editor`** keeps the workspace shell. Its panels become views over the manifest and the catalogue.
- **Networking** replicates the description (small data), never meshes; each machine applies it.

**Shared contracts** that must exist before specialists start (Stage 2): the description schema and its versioning; the base model manifest (parameters, regions, sockets, bone additions); the item manifest (slot, layer, regions, binding, materials, variants, tags); the groom file; the character material parameter set per shading model; the skeleton (today's names plus additive twist bones); how a part takes its leader's shapes; the catalogue format.

## 4. Stages

The brief's seven stages, with milestones of one PR each. **CO** is this overhaul's prefix. Owners are the agents of section 5; "Lead" is the architect.

### Before Stage 2: merge the open character PRs

#400 (bone sliders, sockets), #401 (CS1) and #402 (CS2) all touch the default humanoid; #402 contains #401, and #400 is based on main and changes `anim/src/animators.rs`. Merge order proposed: #401, #402, then #400 rebased onto them (the lead resolves conflicts). The overhaul starts from that merged main.

### Stage 1: research and audit (this checkpoint)

[research.md](research.md), [audit.md](audit.md), this plan; the baseline measurements and renders in [baseline/](baseline/) and their scripts in `assets/characters/humanoid/qa/`.

### Stage 2: architecture and contracts

| ID | Content | Owner | Done when |
|---|---|---|---|
| CO1 | `architecture.md`, `agent-assignments.md`, `testing.md`, `visual-qa.md`, `dependencies.md`; ADRs: semi-realistic default (supersedes ADR-0030), one runtime description, the content library, fitting by binding, character shading models, the groom asset; the `avatar` crate with the description, manifest and item schemas, validation and format 1 migration; **no behaviour change** | Lead | Schemas round-trip; a format 1 file migrates; every manifest entry names shapes and bones the base model has (tests written first and seen failing) |

### Stage 3: foundation (Agents 1 and 2 lead; Agent 6 reviews rendering implications)

| ID | Content | Owner | Done when |
|---|---|---|---|
| CO2 | **Visual QA harness**: an in-engine capture at fixed cameras, lights and settings (full body, three-quarter, side, back, face; the Studio and a hard-light preset) at 1280×720 or larger, from a description file; the Blender harness from Stage 1; geometry metrics (poke-through, joint volume, eyelid closure) as scripted checks | Lead (Agent 6 for the capture) | Same input, identical images; baseline captured on merged main |
| CO3 | **Runtime applier**: `HumanoidDescription`, `ApplyDescription`, Lua; resolve → bone transforms (with #400's attributes and the frame's offsets), shape weights to every part (followers), material parameters that modulate a material's maps instead of replacing them; derived models for the identity bake | Lead with Agent 1 | A character file applied in the player matches the workspace; parts follow the build in a game; masculine/feminine works |
| CO4 | **Realistic base body**: the CC0 realistic pair as the body family; skeleton fitted (reusing `rig.py`'s face-set rings), twist bones added, weights, A-pose; feet, hands and joint loops checked; UV layout for 2K/4K texture sets; procedurally baked skin maps (albedo by region, normal with pores, roughness, cavity); constrained LODs | Agent 1 | Geometry QA passes (no poke-through of eyes or teeth, joint volume within limits); renders reviewed against the anatomy references |
| CO5 | **Body parameters**: MakeHuman target transfer (spike first); frame blend; fat and muscle layers, global and per region (chest, shoulders, arms, abdomen, back, hips, thighs, calves); proportions; corrective shapes; the manifest entries; bony-landmark masks for fat | Agent 1 | Every manifest parameter moves geometry; extremes of every pair stay free of self-intersection; renders at the extremes reviewed |
| CO6 | **Head and face**: the realistic head on the body's topology; eyes as sclera, iris, cornea, occlusion shell and tear line; brows and lashes as cards; teeth, tongue, mouth interior; expression shapes for the 48 controls and the ARKit mapping; pupil size; identity regions (eyes, brows, nose, cheeks, jaw, chin, lips, forehead, ears) with symmetry and asymmetry; the face profile | Agent 2 | `check_face` clean; every expression and viseme renders without eyes poking through lids or teeth through lips; blink closes fully |

**Checkpoint:** a correctly modelled character exists, in both frames, before wardrobe work starts.

### Stage 4: parallel specialist work

| ID | Content | Owner | Done when |
|---|---|---|---|
| CO7 | **Character shading**: packed ORM (frees two slots); Skin (pre-integrated diffusion, two specular lobes, detail normal), Eye, Cloth (sheen, anisotropy) and Hair shading models; glTF material extensions read; MikkTSpace tangents; hashed alpha with per-frame noise under TAA; cut-out-aware shadows; GPU timings | Agent 6 | Each model measured on the reference PC; visual comparison with the Blender reference renders at the same camera; no regression in `every_example_renders` |
| CO8 | **Compute posing**: morph and skin once per frame into a posed buffer used by every pass (and by ray tracing, which then sees morphs) | Agent 6 | Frame time with 20 characters with 150 shapes at least as fast as today with 41; ray traced shadows show expressions |
| CO9 | **Clothing system**: item manifest and catalogue; binding and fitting; layer push-out; region hiding; material variants; the import flow (align to the base, transfer weights, classify the slot, compute regions and binding, make a thumbnail) | Agent 4 | Every item fits both frames and the composition extremes with poke-through under the agreed threshold; an imported third-party garment fits after import |
| CO10 | **Wardrobe, wave 1**: the high-to-low garment pipeline and about 14 items with colour and material variants: T-shirt, long-sleeve shirt, hoodie, button-up shirt, jacket, jeans, cargo pants, joggers, shorts, sneakers, boots, and on sockets a cap, glasses and a watch | Agent 4 | Each item reviewed in renders for construction (seams, hems, closures), folds and fabric response |
| CO11 | **Hair, card tier**: the groom file and converter; card generation and atlas from strands; spring bones; facial hair bound to the face; about 8 styles across straight, wavy, curly and coily, short to long; colour by melanin and redness with highlights and root-to-tip gradients | Agent 3 (shading with Agent 6) | Styles reviewed in renders and in motion (idle, walk); no slab shadows; no clipping into the head at rest |
| CO12 | **Studio interface**: panels from the manifest (basic and advanced), categories (Presets, Body, Face, Skin and Makeup, Hair, Clothing, Accessories, Expressions and Animation, Stage), wardrobe browser (thumbnails, tags, search, filters), colour and material pickers, presets and outfit presets, numeric entry, symmetry, camera framings, turntable, preview clips and expressions, click on the body to open its category; the Game Editor entry and round trip; the face tab renamed | Agent 5 | Every control changes the character; walkthrough tests (open, edit, undo, redo, save, reopen); drawing snapshots reviewed |

**Checkpoint:** each subsystem works in the integrated character, not only alone.

### Stage 5: integration

| ID | Content | Owner | Done when |
|---|---|---|---|
| CO13 | Character presets and outfit presets as data; save and load; placing a character in a project (copying its items); several characters in one scene; the description replicated over the network; export of a character as a package | Lead with Agent 5 | Changing one subsystem (body, face, hair, clothes, materials) breaks none of the others, checked by the QA harness |

### Stage 6: visual QA, optimization and the second wave

| ID | Content | Owner |
|---|---|---|
| CO14 | **Strand hair tier** (after the legal review): compute pipeline (skin, simulate guides, interpolate with curl, rasterize, composite with motion vectors), hair self-shadowing, LOD to cards | Agent 3 and Agent 6 |
| CO15 | **Cloth simulation** for skirts, coats and hoods on coarse proxies; wind | Agent 4 |
| CO16 | **Wardrobe, wave 2** to the brief's full list (polo, sweater, coat, vest, formalwear; chinos, dress pants, sweatpants, athletic shorts, workwear; running shoes, work and hiking boots, dress shoes, sandals; hats, belts, jewellery, backpacks, gloves) | Agent 4 |
| CO17 | **Skin, phase 2**: screen-space SSS; freckles, moles, scars, wrinkles; makeup layers (foundation, eyeliner, eyeshadow, lipstick, blush); wrinkle maps by expression | Agent 6 with Agent 2 |
| CO18 | **Performance**: per-character budgets measured; merging a finished character's parts for games; character LOD sets switched together; crowd test | Agent 6 |

### Stage 7: final integration

The lead reviews every change, runs the whole suite, checks the editor and player, reviews dependencies and licences, updates the docs and wiki, and writes the final report. Completion is claimed only against section 9.

## 5. Agents and ownership

| Agent | Owns (files only it changes) | Consumes |
|---|---|---|
| Lead: architect | The `avatar` crate's schemas, `Cargo.toml`, the build orchestrator (`build.py`), the manifests' format, ADRs, this folder, merging | Everything |
| 1: Modeling and anatomy | `assets/characters/humanoid/` body, rig and body-shape modules; the base model package's body; `anim` correctives and twist bones (with the lead) | The skeleton and manifest contracts |
| 2: Facial systems | The face, eye, brow and lash modules; face profiles in `anim/src/faces/`; expressions | The head topology from Agent 1 |
| 3: Hair and simulation | The hair modules and the groom converter; the groom runtime (simulation, binding); spring bones | Shading from Agent 6; the head surface from Agents 1 and 2 |
| 4: Clothing and wardrobe | The garment modules and items; fitting and import code in `avatar` (functions the lead's schemas define) | Regions and binding contracts; cloth shading from Agent 6 |
| 5: Studio UI and UX | `editor/src/character_workspace*`, `start/characters.rs`, the face tab's placement | The manifest and catalogue; never their formats |
| 6: Rendering and performance | `crates/render`, `crates/scene` material translation, benchmarks | The material parameter contract |

Rules (from the brief, made concrete): each specialist works in its **own git worktree and branch**; nobody changes a file another agent owns; a change to a shared contract goes through the lead; the lead reviews **the code and the test output**, not the summary; every hand-back lists files changed, features, interfaces, tests written and run (with output), dependencies added, open issues and integration needs.

## 6. Testing and visual QA

- **Red, then green.** Each milestone starts with tests that fail: schema round trips, manifest validity (every parameter names shapes and bones that exist), fitting thresholds, applier results in the player, UI walkthroughs. Then the code, then the suite (`cargo nextest run --workspace --no-fail-fast`), clippy and fmt.
- **Geometry checks** (Blender harness, run locally where `bpy` is installed): poke-through counts per item and body combination, volume at joints in extreme poses, eyelid closure, self-intersection at slider extremes. Thresholds agreed in Stage 2 from the Stage 1 baseline.
- **Visual QA** is mandatory: before and after images from the same description, cameras, lights and settings, in the engine (CO2) and in Blender; reviewed by the lead against the reference picks; a feature isn't accepted because it works.
- **Performance**: GPU timings per pass (the existing `FramePass` timers), the character budget below, and a crowd scene, recorded in `docs/benchmarks.md` and the wiki's benchmark history, pinned to one CCD on the reference PC.

## 7. Budgets (proposed, to be measured)

All Expected, not Measured, until Stage 6.

| | Hero (studio, close-ups) | Gameplay LOD0 | Crowd (LOD2) |
|---|---|---|---|
| Body and head | about 30k triangles | 25k | 5k |
| Clothing | up to 40k | 25k | 5k |
| Hair | cards up to 40k triangles, or strands about 30–50k | cards up to 25k | mesh or cards, 3k |
| Textures | 4K skin, 2K items | 2K and 1K | 1K, shared |
| GPU, 1440p, reference PC | under 3 ms with strands | under 1 ms | 50 characters under 3 ms |
| Draws | one per part while editing | merged when finished | merged |

## 8. Risks

| Risk | Likelihood and effect | Mitigation |
|---|---|---|
| **Generated art falls short of the references** | High. The picks are sculpted, simulated and baked by artists; scripted Blender garments and procedural skin detail can be clean and well constructed but are unlikely to match hero art | The high-to-low pipeline (D8) raises the ceiling; the import path lets artist-made items replace any generated one; quality judged by renders at each milestone; Paul decides whether artists or licensed content are in scope (question 2) |
| Licences and patents | Medium: hair rasterizer and cloth tether patents are unchecked; several attractive datasets are non-commercial | Only CC0, MIT and Apache-2.0 sources (research B.2); legal review before CO14 and before tethers in CO15 |
| Renderer limits | Medium: no free texture slots or vertex attributes; morphs cost in every pass | ORM packing first (CO7); storage buffers for extra per-vertex data; compute posing (CO8) |
| Performance | Medium: realistic characters are heavy | Budgets in section 7; identity bake (D4); tiers for hair; merging finished characters |
| Scope | High: the brief is several engine-years of work | Staged checkpoints; cards before strands; wardrobe in waves; nothing claimed done before section 9's criteria |
| Merge conflicts | Medium: three open character PRs; several agents | Merge them first; disjoint ownership; worktrees; the lead integrates |
| Blender dependency | Low: building content needs `bpy`, which CI doesn't have | Built files are committed; the build stays one command; checks that need Blender are local |
| Curly and coily hair | Medium: cards handle them poorly | Treat them as first-class in CO11's styles; strands (CO14) for the best result |

## 9. Acceptance (the brief's section 9)

| # | Criterion | Where |
|---|---|---|
| 1, 2, 3 | Sound body geometry; corrected face; masculine and feminine | CO4, CO5, CO6 |
| 4 | Customization moves geometry | CO3, CO5, CO6 |
| 5 | Convincing hair and movement | CO11, CO14 |
| 6, 7, 8, 9 | A varied wardrobe; fabric materials; fits every body; better footwear and accessories | CO7, CO9, CO10, CO16 |
| 10 | Facial expressions | CO6 (on the existing F4 runtime) |
| 11 | Presets saved and loaded | CO13 |
| 12 | Works with the animation system | CO3, CO12 (preview clips), CO13 |
| 13 | A dedicated, functional Character Studio | CO12 |
| 14 | Pipeline open to expansion | D3, D7, CO9 (import) |
| 15 | Acceptable, scalable performance | CO8, CO18 |
| 16 | Automated and visual tests | Section 6, every stage |

## 10. Questions for Paul

1. **Direction (D1):** approve semi-realistic by default on the realistic CC0 bodies, superseding ADR-0030's stylized direction? Keep the stylized bodies as a second family, or drop them?
2. **Art (D8, risk 1):** is generated content at "clean, well constructed, game-ready" acceptable for the first wardrobe and hair, with the import path for better art later? Or will there be artists, or a budget for licensed assets whose licences allow redistribution?
3. **Hair order (D10):** cards first, then strands (recommended), or strands first?
4. **Open PRs:** merge #401, #402 and #400 (rebased) before Stage 2, as proposed?
5. **Hardware:** which GPU is the minimum for the hero character's strand tier, and is the RX 9070 XT the reference for budgets?
6. **In-game creators:** should games be able to change characters at runtime (an in-game character creator, NPC variety)? The plan assumes yes for applying descriptions (the applier is in the runtime) and no for the editing UI (editor only).
