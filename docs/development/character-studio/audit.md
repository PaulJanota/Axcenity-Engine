# Character Studio overhaul: audit of what exists

Stage 1 of Paul's overhaul brief (2026-11-14). What the character systems are today, what is wrong with them, and what must change in the architecture. Audited on branch `character-studio-cs2` at `bd3ae493`, which holds CS1 (#401) and CS2 (#402); neither is merged into main yet. PR #400 (bone sliders and sockets, branch `default-humanoid-sockets`) is open against main and was read from its branch. Research: [research.md](research.md). Proposal: [implementation-plan.md](implementation-plan.md).

Evidence is marked **Measured** (a script or test produced the number), **Read** (from the source, with a path) or **Observed** (seen in a render). Renders and measurements are in [baseline/](baseline/), made by `assets/characters/humanoid/qa/` so the "after" renders of later stages use the same cameras, lights and settings.

## 1. What exists

### 1.1 Systems map

| System | Where | State | Keep? |
|---|---|---|---|
| Skeleton contract: Unity/VRM humanoid bone names, 45° A-pose, 1.75 m = 5 studs, +Z front, 4 influences | `assets/characters/humanoid/rig.py`; [default-humanoid.md](../../research/default-humanoid.md) D3–D6 | Implemented; 32 clips, retargeting, state machine and face profiles depend on it | **Keep.** Extend (twist bones) only additively |
| The default humanoid asset (CS1) | `assets/characters/humanoid/` (`build.py` and modules); `Humanoid.glb` 2.9 MB, `HumanoidParts.glb` 6.8 MB, `HumanoidAnimations.glb` 1.3 MB | Generated in Blender by script on Blender Studio's CC0 *stylized* base bodies; 6 garments, 6 hair styles, 41 expression, 19 face-identity, 4 frame/build and 10 mask shapes | **Replace the art**, keep the build approach (scripted, reviewable) and the file and shape-name contract where it still fits |
| Embedding | `crates/project/src/default_humanoid.rs:34-47` (`include_bytes!` of all three files and the customization JSON) | Every binary that links `project` carries ~11 MB of humanoid | Keep one embedded default; **move the wardrobe out** (§3.6) |
| Parts list | `HUMANOID_PARTS: [&str; 17]`, `default_humanoid.rs:57` | Hard-coded; the workspace's slots come from name prefixes (`Slot::of`, `character_workspace.rs:75`) | **Replace** with a catalogue (§3.6) |
| Modular characters (HD4) | `project::modular_character`, `humanoid_part` (`default_humanoid.rs:216-318`); `anim` followers (`animators.rs:593` `follow`, `:1731` `place_follower`) | Parts are skinned MeshParts inside the body that take its pose by bone name | **Keep**; extend to shapes (§3.2) |
| Bone sliders and sockets (HD4, PR #400, open) | `anim/src/body_shape.rs`, `anim/src/sockets.rs`, `assets` `Model::pose_shaped` | Nine factor attributes on a skinned MeshPart (`BodyHeight`, `LegLength`, `ArmLength`, `ShoulderWidth`, `WaistWidth`, `HipWidth`, `HeadSize`, `HandSize`, `FootSize`; 0.5–1.5), applied by humanoid role to the body and its parts; seven sockets with Attachments | **Keep and merge first**; the overhaul's proportions build on it |
| Morph targets on the GPU (F1–F2) | `assets` `Model::morph_targets`; `render` `skinning.rs`; [ADR-0028](../../../wiki/Architecture/Architecture-Decisions/ADR-0028-Facial-Animation-Morph-Targets-On-GPU-And-FaceControls.md) | Sparse per-corner deltas, kept by simplified LODs, morphed in the scene, shadow, prepass and motion paths; ray traced passes see the unmorphed mesh | **Keep** |
| Face runtime (F3–F7) | `anim` `face.rs`, `face_play.rs`, `face_world.rs`, `lip_sync.rs`; `net` `face_sync.rs` | 48 standard controls, profiles (axcenity, arkit, vrm, oculus), expressions (15 presets), layers, blinking, gaze, idle, lip sync, facial LOD, replication | **Keep.** New heads plug in through a profile |
| Face tab (F6) | `editor/src/character_studio.rs` and `character_studio/` | A Game Editor document tab named "Character Studio": rig mapping, expressions, lip sync on the selected character | Keep the editors; **fold them into the workspace's Face category** and rename the tab (§3.7) |
| The workspace (CS2) | `editor/src/character_workspace.rs` (state, undo, preview World, save), `character_workspace/ui.rs`, `stage.rs`; `app/character_workspace.rs`; `start/characters.rs` | From the start screen or `--character-studio`; toolbar, categories (Presets, Body, Hair, Clothing, Stage), viewport with views and lighting, snapshot undo, `*.character.json` | **Keep the shell**; rebuild its contents on the new data model |
| Character file v1 (`Look`) | `character_workspace.rs:137` | `name`, `build`, `muscle`, five slot strings; editor crate only | **Supersede** with a versioned description that the player reads (§3.1); migrate v1 |
| Customization data | `assets/characters/humanoid/Humanoid.customization.json` | Bone `offset`s per frame/face shape, `eye_scale`, garment → masks, categories, materials, default outfit | Grows into the parameter manifest (§3.3) |
| Character controller, Humanoid, ragdoll, IK, foot placement | `character`, `gameplay`, `anim` | Implemented | Keep; check that new proportions don't break foot placement and the root part's box (#400 notes the root part ignores `BodyHeight`) |

### 1.2 Tests that exist

- Workspace: `character_workspace/tests.rs` (8 tests: a look's parts and shapes, file round trip and newer files refused, presets, dressing with undo and save, every part posed with the look's shapes, every category drawn without a GPU, the views); `app/character_workspace.rs` (start screen, opening, leaving, saving).
- Face tab: `character_studio/tests.rs`, `app/character_studio.rs`; `anim/src/face_tests.rs`.
- Asset contract: `project` `default_humanoid::tests` (model box, parts on one skeleton, modular character); `player` `default_humanoid_tests.rs` (every humanoid role maps, arms at 45°, all 32 clips pair, face shapes and masks, the character walks, parts follow). PR #400 adds socket and slider tests.
- `AXCENITY_SNAPSHOTS` saves drawing tests' PNGs; `player` `every_example_renders` saves 480×270 frames of each example.

Missing: any geometry-quality test (poke-through, volume loss at joints, eyelid closure), any in-engine render of the character at a fixed camera for before/after comparison, any performance budget test for a character.

## 2. Defects in the current character

Measured by `assets/characters/humanoid/qa/measure.py` into [baseline/measurements.json](baseline/measurements.json); observed in the sheets rendered by `render_baseline.py` (Cycles, a fixed studio, seed 7). The lead reviewed the sheets and checked defect 1's cause in the source. These are Blender renders: the engine's own shading and LODs are not judged by them. [baseline/engine_workspace_cs2.png](baseline/engine_workspace_cs2.png) is a one-off capture of the CS2 workspace taken during its development; a reproducible in-engine capture is CO2.

### 2.1 Key numbers

| Item | Masculine / feminine | Reference |
|---|---|---|
| Shipped default (`Humanoid.glb`) | 10 objects, 45,641 triangles, 56 bones | |
| Parts file | 95,964 triangles; `Body` 25,000 triangles and 74 shapes | |
| Heads tall | 5.93 / 6.39 | 7.7 (Drillis and Contini 1966, via Winter, *Biomechanics and Motor Control of Human Movement*, 4th ed., fig. 4.1; not sex-specific) |
| Shoulder (bideltoid) width ÷ height | 0.254 / **0.197** | 0.259 |
| Hand ÷ height | **0.123** / 0.110 | 0.108 |
| Upper arm ÷ height | **0.155 / 0.138** | 0.186 |
| Eyeball / iris diameter | **74.7 / 28.4 mm** | about 24 / 11.7 mm (about 31 / 15 mm scaled to this oversized head) |
| Face identity shapes, largest move at 1 | nose width 2.3, lips 2.5, nose length and bridge 3.0, cheekbones 4.1, jaw width 4.3, eye spacing 5 mm | Slider ranges aren't defined yet; −1 to 1 assumed |
| Expressions, largest move | sneer 3.4, frown 7.3, smile 13 mm | |
| Build shapes, largest move | heavy 55, slim 19, muscular 13 mm | |
| Joint volume (cross-section) posed ÷ rest | elbow 135°: 0.58 (0.37); knee 135°: 0.75 (0.44); shoulder raised 150°: **0.56** (0.65); hip flexed 100°: 0.67 (0.84); wrist 70°: 0.82 (0.80); spine twist 45°: 0.97 | 1.0 is no loss |
| Fist | 447 / 348 intersecting triangle pairs in the hand | 0 at rest |
| Weights | All at most 4 influences and normalized; **911 of 1,824 face vertices above the head joint carry `Neck` weight** (up to 0.30); **upper lip `Jaw` weight mean 0.17, max 0.77**; 13–28 vertices per finger weighted to a neighbouring finger; 174 vertices carry weight from the other side's bones (max 0.17) | |
| Topology | Body closed, one island, clean. **Shoes: 698 zero-area and 147 sliver triangles, 66 islands**; trousers 120 zero-area; the jacket's 236 n-gons become 49 slivers in glTF | |

### 2.2 Defects, most severe first

| # | Defect | Severity | Evidence | Cause |
|---|---|---|---|---|
| 1 | **A hole at the waist of the shipped default.** The T-shirt's hem rises from 0.975 m at the centre to 1.109 m at the flanks (designed 0.927 m); the trousers start at 0.97 m. 22–48 mm of waist is bare in all eight frame × build combinations (up to 118 mm without the jacket, 78–144 mm with the hip flexed), and `Humanoid.glb` deletes that skin, so you see into the empty body | **Critical**, and it is in CS1 (#401), not yet merged | Measured (`clipping.waist_gap_mm`); Observed: `full_body_turnaround_masculine.png`, `details.png` (waist, tee without jacket), `build_range.png` | ✔ `clothes.py:223`: `tee()` cuts each sleeve with the infinite plane `cut()` along the 45° arm, which also slices the torso diagonally; it needs the side-limited `bmesh_cut_side` (`clothes.py:407`) |
| 2 | **Face skinning.** Turning the head bends the nose and cheeks (`Neck` weight on the face); the upper lip follows the jaw | Critical for animation | Measured (weights) | Bone heat (`rig.py` `auto_weights`) with the `Head` joint at the mouth line (z 1.496); jaw weights smoothed into the upper lip (`face.py` `jaw_weights`) |
| 3 | **Eyes and lids.** A full blink leaves 20–23% of each eyeball's front uncovered and adds 334–398 intersecting triangle pairs; at rest 160 eyeball vertices sit up to 6.9 mm inside the lids; in the feminine blink 31% of lash vertices float more than 3 mm off the skin; eyeball and iris about 2.5× real size; one thin upper lash ribbon, no lower lashes, no tear duct, dark gaps at the feminine eye corners | High | Measured; Observed: `expressions.png` and `details.png` (lashes hang as a loop under the closed lid), `face_topology.png` | Lid shapes rotate about the eyeball centre without enough travel; lashes are offset ribbons; the stylized base's eye sockets |
| 4 | **Hair.** Lumpy, melted, wet-looking masses with spiky tips; long hair drips in noodles and lies 15–30 mm *inside* the cheeks; crop and ponytail float a median 18 mm off the scalp (helmets); the buzz pokes through with `Build_Heavy` (316 of 320 scalp vertices); ears poke through bob and ponytail with the ear sliders up; hair ignores the build and ear shapes | High | Measured; Observed: `hair_styles.png`, `details.png` | `hair.py`: voxel remesh, 12 smoothing passes, decimation, no strand detail or textures; `build.py:288` gives hair only the frame and head shapes |
| 5 | **Joint deformation.** Elbow cross-section falls to 0.37, the raised shoulder pinches (0.56 volume), the wrist collapses, the fist interpenetrates; trousers tear behind the thigh in hip flexion | High | Measured; Observed: `deformation_poses.png`, `details.png` | Linear blend skinning with no twist bones or correctives; trouser cloth bridging the crotch takes the nearest skin's weights (`copy_weights`) |
| 6 | **Shoes.** A lumpy white sock on a saucer: the sole overhangs the upper by a median 6 mm (up to 25 mm); 34 toe vertices poke through the toe box (up to 14.6 mm); laces are bumps; degenerate topology | Medium-high | Measured; Observed: `details.png` | `sneakers()` lifts the whole upper 26 mm; `sole()` lofts a 40-direction hull; the remesh leaves debris |
| 7 | **Garment quality.** The jacket sits a median 24 mm off the skin and the trousers 35 mm (puffy jacket, balloon thighs, bulging pockets); the tee pokes through the jacket (7–18 vertices, up to 12.5 mm; 15 with the shoulder raised); folds are procedural sine waves; no seams, stitching or textures | Medium | Measured; Observed: turnarounds, `build_range.png` | Offset-shell construction (audit §3.9) |
| 8 | **Proportions** (inherited from the stylized base): about 6 heads tall, short upper arms, big masculine hands, very narrow feminine shoulders, low eyes under a tall cranium | Medium (by the old brief, intended; by this brief, wrong) | Measured (§2.1) | The stylized base meshes |
| 9 | **Shallow customization.** Face sliders move 2–5 mm; smile, frown and sneer barely read; slim and muscular are hard to tell from neutral under clothes | Medium | Measured; Observed: `expressions.png`, `build_range.png` | Shape magnitudes; no fat and muscle layers |
| 10 | **Mouth and teeth.** With the jaw open, 62–192 teeth vertices sit inside lip or chin flesh (up to 18 mm); the open mouth is a black hole with no tongue and only the upper corner teeth showing, like fangs; the lips interpenetrate at rest (`Face_Lips_Full` −1 removes 106 of 108 resting self-intersections) | Medium | Measured; Observed: `expressions.png`, `details.png` | Teeth sized into a stylized mouth; no tongue; lip shape |
| 11 | Open single-sided shells (brows, lashes, buzz); `Hair_Long` has 11 islands; finger weight bleed | Low | Measured | |

**Not assessed:** the engine's shading and LODs of these meshes; animation clips (only static poses); the feminine frame on its own skeleton beyond the default outfit (the feminine bone and eye offsets were emulated, as CS3 would apply them).

**Before merging #401 and #402**, defect 1 should be fixed in CS1's build (cutting the sleeves with the side-limited `bmesh_cut_side`, then a rebuild and a re-measure): it puts a visible hole in every new project's default character. Defects 2–11 are the overhaul's to fix by replacing the character (plan D1, CO4–CO6).

## 3. Architectural limitations

### 3.1 The character exists only in the editor

**Read.** `Look` (`editor/src/character_workspace.rs:137`) is the only description of a customized character, and it lives in the `editor` crate, which the player must never depend on. A game can't load a `*.character.json`, so nothing made in the workspace reaches a game: placing a character, spawning one from a script or replicating one all have no data to work from. CS3's `HumanoidDescription` (decision D6 of [character-studio.md](../../research/character-studio.md)) was planned to fix this and isn't built. PR #400 adds a second, unrelated representation (number attributes on the body MeshPart). The overhaul needs **one** description, readable by `player`, `server` and `editor`, with the attributes of #400 as one of its outputs, not a rival.

### 3.2 Shapes don't reach a character's parts

**Read.** Followers take their leader's *bone* pose by name and nothing else (`anim/src/animators.rs:614-626`, `model.pose(&follows)` with no weights). Garments, hair, brows and lashes carry copies of the body's shapes (the README: "Brows and lashes carry the same shapes"; "Every garment follows the body's frame and build shapes"), but in a game those copies stay at their file defaults. The workspace hides this: `CharacterWorkspace::poses` (`character_workspace.rs:601-634`) rewrites every posed part's weights by shape name, in the editor only. Consequences: a modular character's clothes don't follow its build in a game; with a FaceControls on the body, its separate brows and lashes wouldn't follow expressions or blinks. The default character has no FaceControls today (`project/src/default_humanoid.rs`), so this hasn't shown yet.

### 3.3 Frame and eye shapes are unusable

**Read.** `Frame_Feminine` and the eye identity shapes move vertices, and the skeleton must move with them. The bone `offset`s are in `Humanoid.customization.json` (`body.Frame_Feminine.bones`: Hips +7 cm, the hands 8 cm in, every finger), but nothing in the engine reads them; the only reader of that file takes the masks (`character_workspace.rs:307`, `garment_masks`). Leaning on `Frame_Feminine` would put the joints in the wrong places, so the workspace doesn't offer it: **masculine and feminine can't be chosen today**.

### 3.4 Sliders, slots and categories are code, not data

**Read.** `Look::shape_weights` maps two sliders to three shape names in Rust (`character_workspace.rs:205-221`); `Slot` (five slots, `:56`), `Slot::of` (name prefixes, `:75`), `part_label` and `Category` (`:329`) are Rust enums and string matches; `HUMANOID_PARTS` is a fixed array. Adding a slider, a garment, a slot (accessories, gloves, hats) or a category is a code change in two crates. The brief asks for hundreds of controls and a large wardrobe: the controls, their ranges, groups, symmetry and what each drives (shapes, bones, material values) must come from data that ships with the base model, and the panels must be drawn from it.

### 3.5 Clothing fitting doesn't scale

**Read.** A garment fits the body because the build script gave it its own copy of each body shape it follows (`clothes.py`; the README's "Every garment follows the body's frame and build shapes and its weights"). That costs garments × body shapes in file size and build time; it works only for garments built by the script, never for an imported one; and it can't follow shapes added later. Covered skin is hidden by ten coarse region masks (a 1 cm pull-in per region, all or nothing; ADR-0030), which can't express a cropped top, a sleeve length, gloves or socks, and can't stop two garment *layers* from crossing. The brief's fitting requirements (any body, both frames, layering, imported garments) need a fitting step that works on any garment from its geometry alone (see research B.4 and the plan's decision D6).

### 3.6 Content is compiled into binaries

**Read.** `include_bytes!` puts `Humanoid.glb`, `HumanoidParts.glb` (every garment and hair style in one 6.8 MB file) and `HumanoidAnimations.glb` into every binary linking `project` (`default_humanoid.rs:34-47`). A wardrobe of 40+ garments with 2K texture sets would add hundreds of MB to the editor, player and server, and one changed garment means rebuilding the parts file and the engine. Items carry no metadata the brief needs: tags, thumbnails, material variants, layer, the regions they hide, compatible bodies.

### 3.7 Two things are called Character Studio

**Read.** The workspace (CS2) opens from the start screen; in an open project, View > Character Studio and Ctrl+K open the face *tab* (F6), a different tool. While the workspace is open the Game Editor's menus are hidden (`app/chrome/menus.rs:76-79`), and from the Game Editor there is no way to the workspace or back with a character: the brief's "accessible from the main editor menu" and "integrate with the existing animation system" have no path today.

### 3.8 Tints replace textures

**Read** (CS1 notes, `docs/research/character-studio.md` CS3 row): a colour is applied through a SurfaceAppearance by `MaterialName`, and a SurfaceAppearance replaces a model's own maps. Tinting the skin loses its baked texture; tinting a garment would lose any fabric texture. Material customization (skin tone and undertone, makeup, fabric colour and pattern) needs parameters that modulate a material's own maps, not replace them.

### 3.9 The asset build has hit its ceiling

**Read and Observed.** `build.py` makes every garment by offsetting the body's own surface, cutting it with planes and smoothing it (`clothes.py`), and every hair style by voxel-remeshing clumps into one solid mass (`hair.py`). This produced a clean result for the stylized brief, but it can't make pattern construction (separate panels, seams, collars that stand, pockets with depth, closures), gravity folds, or any surface detail, because nothing is baked to textures except one skin colour map. Curls didn't survive the remesh (CS1 notes). The research (A.0) shows that the target look is mostly baked detail. The build needs a high-to-low pipeline (simulate or sculpt high, retopologize, bake normal, cavity and roughness), not more geometry rules.

### 3.10 The renderer has no character shading models

**Read** (checked by the lead against the source after a specialist's audit). The renderer is a forward PBR renderer with good scene lighting (PCSS sun cascades, tiled lamps, light probes, reflection probes, SSAO, contact shadows, TAA, optional ray-traced shadows, AO and GI), but nothing specific to people:

| Area | Today | What the overhaul needs |
|---|---|---|
| BRDF | GGX, Smith-Schlick, Schlick Fresnel, Lambert diffuse (`render/src/lighting.wgsl:577-596, 894-909`); shading models Standard, Unlit, Toon, plus a foliage translucency path (`renderer/types.rs:527-529`; dispatch `shader.wgsl:567-578`) | Skin (subsurface scattering, two specular lobes), eye (cornea lobe, iris refraction), cloth (sheen, anisotropy), hair (anisotropic, multiple scattering). None exists; no sheen, clear coat, anisotropy, transmission or SSS anywhere in `render/src` |
| Custom surfaces | A material's WGSL `surface` function returns base colour, metalness, roughness, normal, emissive and opacity (`shader.wgsl:397-428`); the lighting model is chosen after it | It can't add a lobe or change diffuse. New lighting needs new shading models in `shade`/`lit_pixel` |
| Textures | 16 of 16 sampled textures per stage used: 8 frame (shadows, SSAO, environment, sky, probes, prepass depth), 8 material (base colour, normal, metalness, roughness, occlusion, emissive, opacity, specular; `shader.wgsl:47-56`); default device limits on both the player and the editor | No room for a detail normal, SSS/thickness, a skin tone mask or a hair flow map without freeing slots. Metalness, roughness and occlusion are separate bindings although channel masks already allow one packed ORM texture: packing frees two |
| Vertex data | 16 of 16 vertex attributes used; one UV set (`wiki/Roadmap/Known-Limitations.md`); tangents averaged by area at load, not MikkTSpace, and not moved by morphs (`assets/src/model.rs`; `shader.wgsl:121-144`) | A second UV set (body binding, region IDs) and correct tangents for baked normal maps; extra per-vertex data in storage buffers, as skin and morphs already are |
| Skinning | Linear blend, 4 influences, in the vertex shader (`shader.wgsl:157-188`) | Fine with twist bones and correctives; dual quaternions optional |
| Morphs | Sparse deltas (position, normal; no tangent), evaluated in the vertex shader of **every** pass (scene, prepass, each shadow view, motion vectors twice), looping over every delta of a corner, zero weights included (`shader.wgsl:121-144`) | With ~150 body and face shapes the cost multiplies by 5–9 passes. Pose once per frame in a compute pass into a posed vertex buffer (the ray tracing path already poses skins in compute, `ray_skin.wgsl`), which also gives ray tracing the morphs it lacks |
| Transparency | Sorted by object origin, blended, no depth write; see-through items get no shadows, prepass, SSAO, contact shadows or motion vectors; cut-outs discard, alpha-to-coverage only for the depth fill under MSAA; dithering noise is fixed per pixel (no frame index) | Hair cards and lashes need cut-out shadows and a TAA-friendly alpha (per-frame dither or alpha-to-coverage); strands need their own path |
| Shadows | Position-only shadow pipeline with no material and no fragment stage (`renderer/gpu.rs:537-626`): cut-outs cast as solid | Hair cards would cast slab shadows; needs a cut-out-aware shadow pipeline. Hair self-shadowing (deep opacity) is absent |
| Post | One HDR target; no MRT, no diffuse-only or albedo buffer; the prepass normal is geometric | Screen-space SSS has no diffuse light to blur; pre-integrated skin inside `shade` needs nothing extra |
| LOD | Two simplified levels at load (0.5 and 0.25), weighting normals and UVs only, nothing locked (`assets/src/model/lod.rs:190-197`); per-item switching, so a follower may switch at a different distance from the body | Skin weights, morphs and region borders aren't protected: a face can collapse. Characters need authored or constrained LODs, switched together |
| New drawables | No generic drawable: shadows, prepass, motion and scene passes each loop over mesh batches (`renderer/scene.rs`); `DrawList::particle_emitters` is the precedent for a non-mesh drawable with a compute pass | Strand hair needs a `DrawList` field, a compute pass and an entry in each loop, keeping `render` free of Instances |

**Measured** performance is limited to 20 walking characters of ~76k triangles (RX 9070 XT: CPU draw 0.37 ms, GPU frame 0.81 ms; `docs/rendering/performance.md`). Morph cost, transparency cost and hair have never been measured.

## 4. Recommended architectural changes

Each maps to a decision in [implementation-plan.md](implementation-plan.md).

| Limitation | Change | Plan |
|---|---|---|
| §3.1 editor-only character | One versioned description, read by player, server and editor; `HumanoidDescription` and `ApplyDescription`; #400's attributes as an output | D2, CO1, CO3 |
| §3.2 shapes don't reach parts | Followers take the leader's shape weights by name; identity shapes baked per character | D4, CO3 |
| §3.3 frame and eye bones | Shape-driven bone offsets applied in `anim` after the pose, with #400's sliders | D5, CO3 |
| §3.4 sliders as code | A parameter manifest per base model; panels drawn from it | D3, CO1, CO12 |
| §3.5 fitting | Surface binding at build or import, layer push-out, region hiding in body UV space | D6, CO9 |
| §3.6 content in binaries | A content library on disk with a catalogue; one embedded default | D7, CO9 |
| §3.7 two Character Studios | Workspace reachable from the Game Editor; face tab renamed | D12, CO12 |
| §3.8 tints replace textures | Material parameters that modulate a material's own maps | D9, CO3, CO7 |
| §3.9 build ceiling | A high-to-low Blender pipeline, plus an import path for artist-made items | D8, CO4–CO6, CO10, CO11 |
| §3.10 no character shading | Skin, Eye, Cloth and Hair shading models; packed ORM; hashed alpha under TAA; cut-out shadows; compute posing | D9, CO7, CO8 |
| §2 defects 2–11 | Replace the character on the realistic CC0 base with twist bones, correctives and constrained LODs | D1, D5, CO4–CO6 |

**Not to duplicate.** These already work and the overhaul builds on them: the skeleton contract and its 32 clips; retargeting and rig roles; GPU morph targets; FaceControls, profiles, expressions, layers, blinking, gaze, lip sync, facial LOD and replication; the F6 face editors; followers; #400's bone sliders and sockets; the workspace's shell, undo and stage; the preview canvas; prefabs and the global assets library; `openusd` conversion (ADR-0027); meshopt; `intel_tex_2` texture cooking; the vendored rapier soft bodies.
