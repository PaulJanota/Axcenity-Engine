# Character redesign and Character Studio

Paul's brief of 2026-11-14, "Axcenity — Character Redesign and Dedicated Character Studio" (sections 1–11), with eight reference images. It replaces the faceted look of the default humanoid ([default-humanoid.md](default-humanoid.md), its decision D1) with a smooth stylized character, and moves character creation out of the Game Editor into a workspace of its own. The face (FaceControls, profiles, expressions, lip sync) stays the facial animation plan's ([facial-animation.md](facial-animation.md)); this plan uses it and doesn't fork it.

## The reference images

Described here, not copied into the repository (several are screenshots of other products or art of unclear licence).

| # | What it shows | What it's used for |
|---|---|---|
| 1 | A stylized young woman, front, relaxed A-pose: wavy shoulder-length hair with volume, big expressive eyes, freckles; a cropped navy jacket with a reflective arm band and ribbed cuffs over a grey hoodie, black cargo joggers with side pockets and gathered ankles, chunky boots, a belt | The default outfit (modern, neutral, no logo), hair volume, facial appeal |
| 2 | A "Character Studio" workspace mock-up: categories down the left (Body, Face, Hair, Clothing, Accessories, Materials, Animations), the live preview in the middle (rotate, zoom, inspect), properties on the right (Height, Build, Shoulders, Head, Arms, Legs), Reset bottom left, Save Character and Export bottom right | The workspace layout |
| 3 | A professional character creator: a content library of base characters, the viewport, a long modify panel | What not to show a beginner first: the library-of-presets idea is kept, the wall of controls isn't |
| 4 | A life-sim character creator: expression and pose pickers, background and lighting presets, outfit categories as icons over a thumbnail grid, sections as tabs along the bottom, undo and camera buttons on top | Presets, preview environments, thumbnail pickers, undo in reach |
| 5 | A caricatured elderly man in a three-piece suit, turnaround and face close-ups | How far face shapes should reach; folds and seams that make clothing believable |
| 6 | A stylized man in a knit polo and loose trousers, turnaround | Hair as volumes with clumps; fabric that reads as fabric |
| 7 | An anatomy turnaround of a slim athletic man | Landmarks for the skeleton and weights |
| 8 | Stylized base bodies, male and female, A-pose, front and back | The base body to aim for |

## Assessment of the current default (2026-11-14)

The character in `assets/characters/humanoid` at main (`411e40bf`) was built from nothing by `build.py`, by decision D1 of the default-humanoid plan ("faceted low-poly, after the reference images"). Measured from its files:

| Area | What it is | Why it looks low-poly and stiff |
|---|---|---|
| Body | Lofted superellipse rings, flat shaded, about 5,400 triangles in all | Limbs are tubes with no muscle masses; each ring is a visible facet band |
| Head | 32 segments round, 14 rows; a wedge nose, painted-flat brows, a slot mouth | Reads as a mask in close-up; no eyelid thickness, no lip volume |
| Hands | A palm loft and a tube per finger, 640 triangles | Mittens; knuckles and thumb pad missing |
| Hair | A cap over the skull with jagged fringe clumps | A helmet: no volume, no strands or clumps, one colour |
| Clothes | Rings offset from the body; jacket an open tube | No collar, seams, hems or folds; no thickness at openings |
| Materials | Flat PBR colours, no textures | No skin variation (lips, cheeks, ears), no fabric response |
| Customization | Part swaps and tints by material name | No body or face shape at all |
| Studio | A document tab in the Game Editor, needs a project and a selected character | Buried among the editor's panels; faces only |

What the engine already has, and is kept: the Unity/VRM skeleton names with roles and retargeting (AP1–AP3), glTF takes as animations (HD1), blend shapes on the GPU (facial F1–F2), FaceControls and profiles matched by shape names (F3), expressions, blinking, gaze and lip sync (F4–F5), the Character Studio face editors (F6), parts that follow a leader's skeleton by bone name (HD4), SurfaceAppearance tints by `MaterialName`, the preview canvas, prefabs and the global assets library. What's missing: per-character shape weights outside the face, bone proportions, a character file, a workspace outside a project.

## Art direction

- **Proportions.** 1.75 m, about 6.6 heads; the head a little large, hands and feet about 8% larger than real, eyes large. Recognizable anatomy (deltoids, clavicles, ribcage, calves, knuckles), smooth transitions, no cylinders.
- **Face.** Large eyes with a clear iris, pupil and catch light; eyelids with thickness; a defined brow ridge; a soft rounded nose; full lips with clear corners; a defined jaw. Skin warms at the cheeks, nose, ears and knuckles from a baked colour texture, so a tint keeps the variation.
- **Topology.** All quads from the base mesh: loops around the eyes and mouth, rings at every joint, about 25,000 triangles for the body; LOD1 at half, LOD2 at about a fifth.
- **Materials.** PBR with controlled stylization: colour carries the design; roughness separates materials (skin 0.5, cotton 0.85, denim and canvas 0.8, leather 0.45, rubber 0.9, hair 0.45); textures only where a gradient sells it (skin, hair root to tip).
- **Clothing.** Shells made from the body's own surface, so they fit every body shape: real thickness at openings; hems, cuffs, collars and seams as modelled detail; folds where cloth bunches (inside the elbows, behind the knees, at the ankles and the waist).
- **Hair.** Volumes of smooth tapering clumps over a scalp cap, never a helmet; a primary and a secondary colour.
- **Budget.** The default character at LOD0 at most 45,000 triangles; body under clothes isn't drawn.

## Decisions (taken without asking, all reversible)

- **D1 Smooth stylized, built on Blender Studio's CC0 Human Base Meshes.** Supersedes default-humanoid D1. The bundle (v1.4.1, CC0 1.0) has stylized male and female bodies with clean animation topology, face sets marking every region down to each finger joint, and matching eye and jaw meshes; the two bodies share one topology. Rejected: generating anatomy from nothing (the current approach; it can't reach this quality), MakeHuman (CC0 since 2020 but realistic and dated), Blender Studio's film characters (CC-BY named characters with production rigs), and hand modelling (not possible in this workflow; the asset contract below lets an artist replace any part later).
- **D2 Still generated by a script.** The base meshes are vendored as OBJ in `assets/characters/humanoid/source/` with their provenance (URL, version, SHA-256, licence); `build.py` fits the skeleton, re-poses the arms, weights, makes the face, hair, clothes, materials, shapes and LODs. Rebuilding stays one command, and changes stay reviewable.
- **D3 The skeleton contract doesn't change.** Bone names, rolls, the 45° A-pose, +Z front and 1.75 m = 5 studs (default-humanoid D3–D6), so the 32 clips, retargeting, the state machine, the prefab and every face profile keep working. The base bodies hang their arms about 60° below horizontal; the build re-poses them to 45° before fitting.
- **D4 One body, two frames.** The masculine body is the mesh; the feminine body (same topology) is its `Frame_Feminine` blend shape. Build shapes (`Build_Heavy`, `Build_Slim`, `Build_Muscular`) are generated. Joint positions that differ between frames are bone offsets stored per shape.
- **D5 Body masking by mask shapes.** The body stays one seamless mesh. It has a mask blend shape per region (torso, hips, upper arms top and bottom, forearms, thighs top and bottom, shins top and bottom, feet) that pulls that skin about a centimetre in, feathered where the next bone's weights take over. Each garment names the masks it needs; at weight 1 the covered skin can't poke through. Rejected (CS1): splitting the body into region objects that are hidden, because the engine simplifies each primitive for its LODs on its own, so region borders would crack at a distance, and a MeshPart would need a long pick list. The fixed default character (`Humanoid.glb`) also drops the faces its outfit hides completely.
- **D6 Appearance is a `HumanoidDescription`.** A Roblox-familiar Instance under the Humanoid: the scales (`HeightScale`, `WidthScale`, `DepthScale`, `HeadScale`, `ProportionScale`), Axcenity's own (`BodyFrame`, `Build`, `Muscle`, `ShoulderWidth`, `ArmLength`, `LegLength`, `HandScale`, `FootScale`, face shape values), the parts (hair, tops, bottoms, shoes, outerwear, accessories) and colours; `Humanoid:ApplyDescription`. Bone sliders work on any humanoid with roles; shape sliders need the named shapes and are greyed, with the reason, on models without them.
- **D7 Proportions are bone transforms, faces are shapes.** A length or size slider stretches its bone without passing the stretch to its children (their joints move instead), applied after the animated pose and before skinning, so clips play unchanged. Face identity shapes (jaw, chin, nose, eyes, mouth, brows) sit on the head beside the expression shapes; deltas add, so FaceControls keep working on any face. Moving the eyes moves the eye bones with the shape.
- **D8 Characters are `*.character.json` files.** The description's values, a name, a format version and the base model. Character Studio saves them in the global assets library (`assets/characters/`), so every project lists them; placing one copies the humanoid files into the project where missing and builds the Instances. No mesh is duplicated per character.
- **D9 Character Studio is a workspace of its own.** The start screen gets a **Create** group: Game Editor (the Projects page) and Character Studio. Character Studio needs no project: a top toolbar, categories on the left, the viewport in the middle, the contextual properties on the right. It shares `assets`, `anim`, `scene`, `render`, the preview canvas and the face editors with the Game Editor, and keeps its own undo (snapshots of the description, which is small). Model Studio and animation stay inside the Game Editor for now; the workspace switch is generic, so they can follow.
- **D10 The viewport is a stage.** Orbit, zoom, pan; front, side, back, face and full-body views; lighting presets (Studio, Daylight, Sunset, Night) and backgrounds; wireframe and skeleton overlays; animations from the humanoid's clips (Idle, Walk, Run, Jump, Wave and the emotes) with pause and a neutral pose.
- **D11 Import is checked in plain words.** An imported humanoid goes through `check_rig` and `check_face`; each problem names what's wrong and how to fix it, and the sliders it disables say why.
- **D12 Progressive disclosure.** Each category shows its everyday controls; rigging, deformation and material channels are in an Advanced fold. Presets come first, then sliders.

## Stages (one PR each, in merge order)

| ID | Stage | Content |
|---|---|---|
| CS1 | Art direction and the new humanoid (done) | This document; `build.py` rewritten on the CC0 base meshes: the skeleton fitted from face sets, arms re-posed to 45°, skin weights, the 41 face shapes, eyes with iris and pupil, eyebrows and lashes, teeth, the frame, build, identity and mask shapes, the default outfit (T-shirt, cropped jacket, cargo joggers, sneakers) and underwear, six hair styles, a baked skin texture; the engine's default humanoid and its tests on the new files (*as built:* the base mesh's mouth already has its inside, so the jaw opens it with no cut; no tongue (the profile's tongue controls are optional); hair is sculpted masses made by a voxel remesh of clumps, and curls didn't survive it, so the sixth style is a short crop; LODs are the engine's, made at load, which keep blend shapes; `Humanoid.glb` keeps only its face shapes and the masks its outfit needs, and the frame, build and identity shapes are in `HumanoidParts.glb`) |
| CS2 | The workspace (done) | The start screen's Create group; the Character Studio workspace with its toolbar, category list, viewport (orbit, zoom, pan, views, lighting and backgrounds) and properties panel, on its own preview World; Back to the start screen (*as built:* a Character Studio page after Projects in the start screen's navigation, with New Character, presets and saved characters, and `--character-studio`; the Game Editor's menus hidden while it's open; categories Presets, Body (build and muscle), Hair, Clothing and Stage, so the hair, clothing and build parts of CS3 came early, shown in the preview only; a first `*.character.json` (`Look`) with Save and open, which CS5 extends; the frame, proportions, face, materials, Face and Animation categories are CS3 and CS4) |
| CS3 | Live customization | `HumanoidDescription`; body sliders (shapes and bone transforms in `anim`), face sliders, hair, clothing and accessory swaps with body masking, colours and material presets, undo and redo, reset |
| CS4 | Face and animation | The Face category (the F6 rig, expressions and lip sync editors on the workspace's character), expression presets, blink and jaw, animation preview, wireframe and skeleton overlays |
| CS5 | Character files | New, open, save, duplicate, presets; `*.character.json` in the global library; placing a character in the Game Editor; importing a humanoid model with checks; export as a package |
| CS6 | Polish and validation | A new developer's walkthrough (launch, create, customize, preview, save, use in a game), user documentation, tests for the regressions found |

## CS1 as built

| | Before | After |
|---|---|---|
| Default character | About 5,400 triangles, flat shaded | About 45,700 triangles (body 25,000 less the skin under the outfit), smooth shaded |
| Files | 1.7 + 2.7 + 1.3 MB | 2.9 + 6.8 + 1.3 MB (`Humanoid.glb`, `HumanoidParts.glb`, `HumanoidAnimations.glb`); shape keys are most of it |
| Body | Lofted rings | Blender Studio's stylized base body; masculine and feminine frames on one topology |
| Face | A mask: wedge nose, painted brows, sphere eyes | Sculpted face, eyes with a textured iris, brows and lashes, teeth; 41 expression shapes, 19 identity shapes |
| Clothes | Offset rings | Draped shells with hems, cuffs, collar, rib bands, folds, pockets, a welded sneaker with a sole and laces |
| Hair | Four helmets | Six sculpted styles (swept, bob, long, crop, buzz, ponytail) |
| Skin | Flat colour | A baked 2048 px texture: lips, cheeks, nose, ears, knuckles, elbows, knees |

Checked by the existing tests on the new files (`project`: the model box, the parts file's objects on one skeleton, a modular character; `player`: every humanoid role maps with no problems, the arms at 45°, all 32 clips pair with the skeleton, the 41 face shapes and the masks resting at 1, the default character walks animated, a part follows the body's pose) and by renders and a player screenshot in the PR.

## Acceptance (Paul's section 11)

| # | Criterion | Where |
|---|---|---|
| 1, 2, 3 | Improved stylized art, not a low-poly NPC, appealing anatomy, hair, face and clothes | CS1 (renders in the PR) |
| 4, 5 | Character Studio from the start screen, a focused workspace | CS2 |
| 6, 7, 8 | Body and face customization; parts swapped independently; immediate updates | CS3 |
| 9, 10 | Facial animation on customized faces; animation preview | CS4 |
| 11 | Saved and reused across projects | CS5 |
| 12 | Approachable | CS2–CS6 (D12), checked in CS6 |
| 13 | Game Editor workflows still work | Every stage (the workspace suite) |
| 14 | Tests | Every stage |
