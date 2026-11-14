# Character Studio overhaul: body, face, skin, cloth research notes

Researched 2026-11-14 for Axcenity (Rust, wgpu 30, forward renderer + depth/normal prepass + TAA, Apache-2.0, desktop first).
Scope: research only, no repository edits.

Conventions in this file:

- **[V]** = verified by opening the primary source (page, PDF, code or licence file) during this session.
- **[S]** = only seen in a search-result summary or a secondary article; treat as likely but not verified.
- **[U]** = unverified; I could not open a source, or it is my own engineering judgement.
- "Visual inspiration" items are kept apart from "technical evidence" at the end of each section where relevant.

---

## A. MetaHuman

### A1. Creator parameterization (UE 5.6+, MetaHuman Creator inside Unreal)

- **Parametric body (new in UE 5.6, June 2025).** "Statistically driven sliders for specific body parts that you can pin and refine." Global group: Masculine/Feminine, Muscularity, Fat, Height. Region groups: Upper Torso, Lower Torso, NeckArms, Legs, Hands. "As you change a measurement, you move through a statistical space defined by real-life data." Parameters are correlated (Muscularity changes Fat unless Fat is pinned). Adjusting a slider auto-pins it; pinned measurements are drawn in the viewport. Options "Scale Ranges by Height" and "Unlock Body Ranges"; the Parameters Range Multiplier "does not affect the actual constraints in the model, just the user interface". Read-only diagnostics: Shoulder Height, Rise. [V] https://dev.epicgames.com/documentation/metahuman/metahuman-creator-body-params-tool-in-unreal-engine
- **Fixed (compatibility) bodies.** The 18 legacy body types of the web app; once applied, no parametric features. "Perform Parametric Fit" converts fixed to parametric ("an approximation and can result in some visual differences"). [V] same page; "18" is from the search summary of the same docs and the parametric-clothing page ("base 18 archetypes") [V] https://dev.epicgames.com/documentation/metahuman/getting-started-for-creating-parametric-clothing-in-metahuman
- **Face.** Three tools: *Blend* (mix up to three presets, "in local regions or globally for head and body"), *Head Transform* (broad placement of features), *Head Sculpt* ("precise, localized adjustments"); plus *From Custom Mesh* (conform an imported head) and a Teeth/Eyelashes tool. Head/body tools only work on unrigged characters ("Remove Rig"). [V] https://dev.epicgames.com/documentation/metahuman/metahuman-creator-head-and-body-tools-in-unreal-engine
- Number of facial blend regions and the statistical method (PCA or otherwise) are not documented on these pages. [U]

### A2. DNA file and RigLogic

- **DNA** = Epic's proprietary format that "encodes all information necessary to assemble and configure a full MetaHuman head, body and its rigs"; head and body are separate DNA files. Layers: Descriptor (metadata), Definition (names of controls/joints/blend shapes/animated maps/meshes, LOD mappings, joint hierarchy, bind pose), Behavior (GUI controls -> raw controls, corrective expressions, joint transforms, blend-shape weights, animated-map weights), Geometry (meshes, skin weights, blend-shape deltas). Behavior and Geometry are independent, so runtime evaluation needs only Behavior. [V] https://raw.githubusercontent.com/EpicGames/MetaHuman-DNA-Calibration/main/docs/dna.md ; [V] https://dev.epicgames.com/documentation/metahuman/metahuman-dna-rig-definition-and-rig-operation
- **RigLogic** = runtime solver: "hundreds of semantically meaningful channels" -> joint transforms, blend-shape weights (LOD0 only) and animated-map (wrinkle) multipliers. Whitepaper "Rig Logic: Runtime Evaluation of MetaHuman Face Rigs" (Epic, © 2022): "A state-of-the-art facial rig ... contains around 800 joints and more than 200 controls for expressions, complemented with 1,000+ corrective expressions"; joints split into region "joint groups" stored as dense SIMD-friendly sub-matrices (~6x faster than an earlier CSR version); eight built-in LODs where lower LODs use a strict subset of LOD0 joints (zero-cost LOD switching). [V] https://cdn2.unrealengine.com/rig-logic-whitepaper-v2-5c9f23f7e210.pdf (downloaded, 12.7 MB)
- **Licences.**
  - **OpenRigLogic** (RigLogic + DNA libraries, C++11, SWIG/Python bindings): **MIT**, "Copyright (c) 2026 Epic Games, Inc."; repo created 2026-06-05; `5.8` branch production-ready, `main` experimental. Sample assets and the Faceboard control rig are separate, on Fab under the **Fab Standard License** (not MIT). TRADEMARKS.md: MIT covers code only; you may not use "MetaHuman", "RigLogic", "OpenRigLogic" in your own product name; forks must use their own name ("[Your Software] is based on OpenRigLogic"). [V] https://github.com/EpicGames/OpenRigLogic , LICENSE and TRADEMARKS.md read directly; [V] https://dev.epicgames.com/documentation/metahuman/metahuman-devkit-in-unreal-engine ("released under an open source MIT license", integrable "outside of Unreal Engine").
  - **MetaHuman-DNA-Calibration** (DNACalib, DNAViewer): **"MetaHuman DNA Calibration License Agreement"**, a proprietary licence: may only be used to build tools that let third-party programs export/edit characters in MetaHuman format; **no redistribution**; terminable by Epic; MetaHuman characters limited to Epic's content-licence terms. Repo not updated for UE 5.6 characters (points to "MetaHuman for Maya"). **Not usable inside an Apache-2.0 engine.** [V] https://raw.githubusercontent.com/EpicGames/MetaHuman-DNA-Calibration/main/LICENSE
- **Facial rig size per LOD** (Epic's guide figures, "not exact for every MetaHuman") [V] https://dev.epicgames.com/documentation/metahuman/platform-support-and-lod-specifications-for-metahumans

  | Head | LOD0 | LOD1 | LOD2 | LOD3 | LOD4 | LOD5 | LOD6 | LOD7 |
  |---|---|---|---|---|---|---|---|---|
  | Vertices | 24000 | 12000 | 6000 | 2500 | 1300 | 560 | 270 | 130 |
  | Blend shapes | 669 | – | – | – | – | – | – | – |
  | Joints | 713 | 529 | 397 | 283 | 84 | 70 | 41 | 26 |
  | Skin influences | 12 | 12 | 12 | 8 | 8 | 8 | 4 | 4 |
  | Animated (wrinkle) maps | yes | yes | no | no | no | no | no | no |

  Body: 30,500 / 7,600 / 3,350 / 1,507 vertices for LOD0–3; correctives on LOD0–1 only; 8, 8, 8, 4 influences. Hair: strands 50k (LOD0), cards 30k verts LOD0 down to mesh hair 100 verts LOD7. Mobile best LOD = 3, max texture 2048.
- Key design point for us: the face is **mostly joints** (joint-based expressions with per-vertex displacement only at LOD0: "Only LOD 0 uses vertex displacement shapes"; "Per-vertex shape interpolation is linear"). [V] DNA rig-definition page above.

### A3. Helper/twist joints and RBF pose drivers

- Body rig (5.6+): RBF and swing/twist solvers live on the bind skeleton (driver skeleton removed); upper-arm twist blend 0.8/0.2; thigh swing blend 0.6; finger half-rotation moved to an RBF solver; corrective "surface" joints such as `clavicle_pec_r`, `spine_04_latissimus_r`, `ankle_fwd_r`, `ankle_bck_r`; RBF pose names like `upperarm_l_in_35`, `out_110`. [V] https://dev.epicgames.com/documentation/metahuman/metahuman-asset-changes
- Maya "Pose Editor" authors body secondary deformation with RBF and SwingTwist solvers, exporting to DNA. [S] https://dev.epicgames.com/documentation/metahuman/maya-pose-editor
- UE 5.6 "Unpack RBF" replaces RigLogic body deformation with PoseDriver + Control Rig (experimental, slower). [S] https://dev.epicgames.com/documentation/metahuman/metahuman-body-riglogic-unpacking-in-unreal-engine
- Neck: "The MetaHumans facial (neck) rig has one RBF solver which has three main neck joints as inputs". [S] https://dev.epicgames.com/documentation/metahuman/updating-neck-rbf-setup-tool-drivers-for-expression-editor

### A4. Eye and mouth model

- Per-LOD head meshes (from Epic's DNAViewer example `MESH_SHADER_MAPPING`): `head`, `teeth`, `saliva`, `eyeLeft`, `eyeRight`, `eyeshell`, `eyelashes`, `eyelashesShadow`, `eyeEdge`, `cartilage` (cartilage uses the eyeEdge shader). [V] https://raw.githubusercontent.com/EpicGames/MetaHuman-DNA-Calibration/main/examples/dna_viewer_build_rig_with_textures.py
- Same file lists the head texture set: color, normal, specular, occlusion, cavity, transmission, curvature, position, world-space normal, bent normal; three colour and three normal **wrinkle maps** (`cm1-3`, `wm1-3`) with ~38 regional masks (blink, brow raise, jaw open, smile, nose wrinkler, neck stretch ...) driven by RigLogic animated-map multipliers. [V] same file.
- UE MetaHumanCharacter API enum lists slots EyeLeft, EyeRight, EyeShell, LacrimalFluid, Teeth, Eyelashes, EyelashesHiLods. [S] https://dev.epicgames.com/documentation/unreal-engine/API/Plugins/MetaHumanCharacter/EMetaHumanCharacterTeethAndEyesS- (page body would not load)
- Purpose of `cartilage` (invisible, opacity 0 in UE5 per a forum question with no answer) is **unverified** [U]. My reading: `eyeshell` = eye occlusion/AO shell over the eye, `eyeEdge` = tear line / wet line, `saliva` = mouth wetness. [U]
- Materials: one skin MI per LOD; textures Basecolor, SRMF (Specular, Roughness, Metallic, **Fuzz**), Normal, Normal LOD (256²), Scatter (512²), animated delta maps (basecolor 512², normal 1K). LOD1 disables Micro Skin Detail normals; LOD2 disables animated maps and bakes AO into base colour; LOD4+ "UseSimpleShading" (Default Lit). Medium/Low quality disables SSS. Eye MIs give runtime control of sclera, cornea, iris, pupil. [V] https://dev.epicgames.com/documentation/metahuman/metahuman-materials-and-textures

### A5. 2025 licensing change and what it means for Axcenity

- June 2025 (UE 5.6): "The MetaHuman toolset is now covered by the standard Unreal Engine EULA." MetaHuman characters and animation are "non-engine products": usable in Unity, Godot etc. without the 5% royalty; above $1M/yr gross revenue from projects using MetaHumans an Unreal seat licence ($1,850/yr) is required; MetaHuman characters and clothing may be sold (Fab). Training AI on MetaHumans not allowed (as of that date). [V] https://www.cgchannel.com/2025/06/you-can-now-sell-metahumans-or-use-them-in-unity-or-godot/ (secondary press, summarises Epic's FAQ); [S] https://digitalproduction.com/2025/06/05/metahumans-graduate-ready-for-unity-godot-and-the-fab-cash-register/
- Epic's own EULA text, the MetaHuman licence page (https://www.metahuman.com/license) and the UE FAQ could **not** be opened (login redirect / HTTP 403 / 503). The Epic FAQ wording quoted in search results: assets "can be used in other video game engines without incurring any royalty obligation, provided that those assets don't use or rely on Unreal Engine code and are not UE-Only Content provided by Epic". [S]
- Fab Standard License summary: you may not "Resell or redistribute the asset for free on a standalone basis or allow others to do the same." [V summary page] https://www.fab.com/eula
- **Conclusion for an Apache-2.0 engine [judgement, U on exact EULA clauses]:**
  - Axcenity **cannot bundle** MetaHuman meshes, textures, DNA files or sample assets: they are Epic-licensed content, not relicensable under Apache-2.0, and redistribution on a standalone/extractable basis is prohibited.
  - Axcenity **can** let users import *their own* MetaHumans; the user, not Axcenity, carries the EULA obligations (seat licence over $1M, no AI training).
  - Axcenity **can** bundle/link **OpenRigLogic (MIT)** to evaluate imported DNA rigs (keep the MIT notice; do not call the feature "MetaHuman"/"RigLogic" in product naming beyond accurate attribution per TRADEMARKS.md).
  - Axcenity **must not** ship DNACalib (proprietary, no redistribution).

### A6. Outfits / parametric clothing (UE 5.6) and Chaos Cloth

- "The Outfit Asset is a new chaos cloth asset type, which is capable of containing parametric clothing." Each outfit is authored on a *source body*; for a different *target body* it "warps the clothing based on the difference between the source body and a new body"; large differences warp more, so authors ship several source-body variants and the system "automatically selects the best-matching source clothing". Render mesh (detailed, can have thickness) vs optional single-sided Sim mesh that drives it. [V] https://dev.epicgames.com/documentation/metahuman/getting-started-for-creating-parametric-clothing-in-metahuman
- In 5.6, resizable clothing does not support custom skinning, control rig or RBAN. [S] German version of the parametric-clothing terminology page.
- **Body Hidden Face Map**: paint body UVs white (show) / black (hide); "This map tells MHC which faces of the body are always hidden by the clothing, and removes them in the assembly process"; head faces cannot be hidden by it; a separate Head Hidden Face Map can "remove or shrink the head geometry out of the way of the clothing". [S] https://dev.epicgames.com/documentation/metahuman/testing-and-configuring-your-parametric-outfit-asset (search summary; page not opened)
- The exact warping algorithm (RBF, barycentric, proximity) is not documented in what I could open. [U]
- Chaos Cloth settings: "Max Distance is the maximum distance any point on the cloth can move from its animated position"; a painted value 0 means "the skinned vertex could not move" (i.e. kinematic); Backstop Distance/Radius keep cloth outside a sphere offset from the animated surface; Anim Drive springs pull cloth toward the animated pose; character collision comes from the Physics Asset. [V] https://dev.epicgames.com/documentation/en-us/unreal-engine/clothing-tool-in-unreal-engine
- Long-range attachments (tethers): "each cloth particle is connected to its closest fixed point"; Tether Scale; geodesic vs Euclidean option. [S] Epic API pages (FChaosClothAssetTetherGeneration, SimulationLongRangeAttachmentConfig).
- Solver: Dataflow bending config offers XPBD anisotropic bending (warp/weft/bias stiffness, buckling ratio) "when the solver is XPBD"; runtime classes `FXPBDAnisotropicBendingConstraints`, `FXPBDAnisotropicSpringConstraints`. [S] https://dev.epicgames.com/documentation/unreal-engine/node-reference/Dataflow/SimulationBendingConfig
- Chaos Cloth appears CPU-driven (thread tuning cvars); no official statement found. [S/U]

**Visual inspiration (not evidence):** MetaHuman Creator presets and close-up renders; the parametric-body pin/measure UX.

---

## B. Open base bodies and parametric models (licences)

| Source | What | Licence (exact) | Bundle in Apache-2.0 engine? | Status |
|---|---|---|---|---|
| Blender Studio **Human Base Meshes** | Realistic + stylized male/female bodies, heads, hands, feet, jaw, eyes; quads, UVs (UDIM), multires detail on realistic | **CC0** | **Yes** | [V] download server lists v1.0.0 (2023-11-03), v1.1.0 (2023-11-14), v1.2.0 (2024-09-04), v1.3.0 (2025-07-21), v1.4.0 (2025-09-29), **v1.4.1 (2026-01-20) = latest**; https://download.blender.org/demo/asset-bundles/human-base-meshes/ . Blender 4.0 update: realistic male body "Based on scan data for increased realism", generic head topology with example shape keys, improved eye scale on realistic assets [V] https://developer.blender.org/docs/release_notes/4.0/asset_bundles/ ; 17 meshes incl. photorealistic and stylised, CC0 [V] https://www.cgchannel.com/2023/06/download-blender-studios-free-human-base-meshes |
| **MakeHuman** core/system assets (base mesh, targets, skeleton, proxies, eyes, teeth) | Parametric human (gender/age/muscle/weight/proportions targets) | **CC0** ("All core assets are shared under Creative Commons, CC0") | **Yes** (data only) | [V] https://static.makehumancommunity.org/about/license.html |
| **MPFB2** (Blender plugin) code | Plugin | **GPL** (v3 per repo/press) | No (do not copy code) | [V] same page; GPLv3 [S] https://www.cgchannel.com/2025/03/check-out-open-source-blender-character-generation-plugin-mpfb-2 |
| MakeHuman application code | App | **AGPL** | No | [V] same page |
| Third-party MakeHuman asset packs | Clothes, hair... | mostly CC0 or **CC-BY** per pack | Per pack (CC-BY needs attribution) | [S] |
| **CharMorph** | Blender character tool (MB-Lab rewrite) | code **GPL**; characters: Antonia **CC-BY**, MB-Lab **AGPL**, Reom **CC-BY**, Vitruvian **CC0** | Vitruvian yes; Antonia/Reom with attribution; **MB-Lab no (AGPL)** | [V] https://charmorph-docs.readthedocs.io/en/main/Introduction.html |
| **Anny** (NAVER LABS Europe) | Scan-free parametric body for all ages (infant to elder), MakeHuman-derived, WHO-calibrated; 104-bone rig (163 MakeHuman rig) | code **Apache-2.0** (© 2025 NAVER Corp.); `data/mpfb2` **CC0**; `data/faceunits01` **CC0**; `data/soma` **Apache-2.0**; optional `smplx` assets **non-commercial** | **Yes**, excluding the smplx extras | [V] https://github.com/naver/anny (v0.1 2025-11-05, v0.6.2 2026-10-26); paper arXiv 2511.03589 [S] |
| **SOMA-X** (NVIDIA) | Unified body model/topology (~18,095 vertices, 77 joints) with identity backends SOMA-shape, SMPL, SMPL-X, MHR, ANNY, GarmentMeasurements | **Apache-2.0** (SMPL backends need separate SMPL licences) | Probably yes for SOMA-shape [U] | [S] https://nvlabs.github.io/SOMA-X/stable/model_card.html (release 2026-03-16) |
| **Google GNM Head v3** | Statistical head: 170 head + 3 eyeball + 80 teeth identity components; expression: 100 + 100 eye-region, 150 lower-face, 32 tongue; eyeballs, teeth, tongue; UDIM UVs; semantic samplers | **Apache-2.0** (repo + Hugging Face weights `google/gnm-v3`, `gnm_head.npz`, tag `license:apache-2.0`, no gating) | **Yes** (with NOTICE) | [V] https://github.com/google/GNM (created 2026-03-23), gnm/shape/README.md, https://huggingface.co/google/gnm-v3 ; report arXiv 2607.23687 (2026-07-26). Vertex count not stated [U] |
| **ICT FaceKit** (USC ICT) Light model | 26,719 verts, 17 parts (face, head/neck, eyes, teeth, gums/tongue, eyelashes); 100 PCA identity; 53 ARKit-named expressions | **MIT** ("Copyright (c) 2020 USC Institute for Creative Technologies"); Full model "under a different USC specific license" | Likely yes (MIT text says "Software"; README says the toolkit and Light model are MIT) | [V] https://github.com/ICT-VGL/ICT-FaceKit + LICENSE |
| **FLAME 2023 Open** | Head model | **CC-BY-4.0** plus restrictions: no pornographic use, no "fake, libelous, misleading, or defamatory content" | Bundle with attribution, but the extra use restrictions travel with it (flag) | [V] https://flame.is.tue.mpg.de/modellicense.html (Open model released Nov 2025 [S]) |
| FLAME 2017/2019/2020/2023 (non-open) | Head model | non-commercial research only; no sharing/redistribution | **No** | [V] same page |
| **SMPL-X** | Body+hands+face | non-commercial scientific research; no commercial/porn/military/surveillance use; no redistribution; commercial via Meshcapade | **No** | [V] https://smpl-x.is.tue.mpg.de/modellicense.html ; Meshcapade tiers (Micro €600/yr commercial) [S] https://meshcapade.com/assets/body-models |
| **STAR**, **SUPR** | Body models | "software copyright license for non-commercial scientific research purposes" | **No** | [S] https://github.com/ahmedosman/STAR , https://github.com/ahmedosman/SUPR |
| **3D Scan Store** free head/base mesh | Scan | "Personal single user license"; commercial requires contacting the store | **No** | [S] https://www.3dscanstore.com/blog/multi-expression-basemesh (403 on fetch) |
| **Texturing.xyz** | Skin textures/displacements | ToS: no redistribution of raw materials; may not release Materials or derivatives "under an Open Source License"; no commercial use of free downloads without permission | **No** | [V] https://texturing.xyz/pages/terms-of-service |
| Digital Emily 2 (USC ICT / Wikihuman) | Head scan | free for non-commercial use | **No** | [S] https://www.cgchannel.com/2015/06/download-digital-human-labs-digital-emily-2-model/ |
| Lee Perry-Smith "Infinite" head (three.js example) | Head scan + maps | **CC BY 3.0 Unported** (Infinite-Realities / triplegangers) | Yes, with attribution (good test asset) | [S] copies of LeePerrySmith_License.txt from three.js |

**Recommendation (B):** ship only CC0 / Apache-2.0 / MIT / attribution-only assets. Best fit:
1. Keep **Blender Human Base Meshes** but move to the **realistic** body/head in v1.4.1 (CC0) as the default "semi-realistic" base, keeping the stylized one as an alternative.
2. Use **MakeHuman/Anny CC0 targets** (gender × age × muscle × weight) as a *learning source and a data source* for body morphs, transferred onto our topology with a wrap/projection tool (data only; never copy MPFB GPL code).
3. Use **GNM Head (Apache-2.0)** as a statistical face-identity space (fit/wrap its PCA components onto our head topology offline) and/or **ICT FaceKit Light (MIT)** for identity PCA + ARKit-style expressions.
4. Do **not** bundle SMPL/SMPL-X/STAR/SUPR/old FLAME, 3D Scan Store, Texturing.xyz, Digital Emily, MetaHuman or MB-Lab assets.

---

## C. Body parameterization and skinning techniques

### Evidence

- **MakeHuman macro system (separate fat and muscle axes, multilinear corner targets).** Weight value w in [0,1] maps to three factors: `maxweight = max(0, 2w-1)`, `minweight = max(0, 1-2w)`, `averageweight = 1 - (max + min)` (same pattern for muscle); age is piecewise-linear over baby(1y)/child(10y)/young(25y)/old(90y) at 0, 0.1875, 0.5, 1. Each macro target file (e.g. `female-young-averagemuscle-maxweight`) gets weight = **product** of its factor values: `value * reduce(operator.mul, [factors[f] for f in tfactors])`. This is a multilinear interpolation over a grid of artist-sculpted "corner" shapes, which is how fat and muscle combine without one cancelling the other. Region/proportion modifiers depend on macro values. [V] https://raw.githubusercontent.com/makehumancommunity/makehuman/master/makehuman/apps/human.py and .../humanmodifier.py (`getTargetWeights`)
- **MetaHuman** uses separate Fat and Muscularity sliders inside a data-driven statistical model with correlations and model-imposed range constraints (A1). [V]
- **The Sims 4** uses separate physique deformer maps HEAVY, FIT, LEAN, BONY, PREGNANT, HIPS_WIDE/NARROW, WAIST_WIDE/NARROW plus an AVERAGE map per age (D). [V]
- **SMPL** (Loper et al., SIGGRAPH Asia 2015, TOG 34(6) 248): shape blend shapes from PCA of registered scans; **pose blend shapes are linear in the elements of joint rotation matrices** (≈207 = 9×23 shapes [S]); a learned joint regressor makes joint locations a linear function of the shaped mesh. [S] https://ps.is.mpg.de/publications/smpl-2015
- **Pose Space Deformation** (Lewis, Cordner, Fong, SIGGRAPH 2000) is the origin of RBF-interpolated corrective shapes. [U, well-known citation, not opened]
- **Dual quaternion skinning** (Kavan, Collins, Žára, O'Sullivan, TOG 27(4) 2008, "Geometric skinning with approximate dual quaternion blending"): removes LBS volume collapse / candy-wrapper, efficient on the GPU; known limitations: flipping (shortest-path), and per literature, bulging near bends and poor support for non-rigid scale. [S] https://users.cs.utah.edu/~ladislav/kavan08geometric/kavan08geometric.html
- **Optimized Centers of Rotation** (Le & Hodgins, SIGGRAPH 2016, TOG 35(4)): precompute a per-vertex centre of rotation from the rest pose and skinning weights; at runtime interpolate a rigid transform per vertex; "significantly reduces the artifacts of LBS and DQS while maintaining real-time performance and backwards compatibility". [S] https://la.disneyresearch.com/publication/skinning-with-optimized-cors/ ; paper PDF: https://la.disneyresearch.com/wp-content/uploads/Real-time-Skeletal-Skinning-with-Optimized-Centers-of-Rotation-Paper.pdf
- **Delta Mush** (Mancewicz, Derksen, Rijpkema, Wilson, DigiPro 2014): iterative smoothing that preserves rest-pose detail (Rhythm & Hues, used on all rigs since 2010). **Direct Delta Mush** (Le & Lewis, SIGGRAPH 2019, TOG 38(4), EA SEED): non-iterative, precomputed per-vertex data; variants trade storage/compute for quality; later "DDM Compression with Continuous Examples" (SIGGRAPH 2021). [S] https://www.ea.com/seed/news/siggraph2019-direct-delta-mush
- **MetaHuman** fixes LBS with helper joints: twist joints (upper-arm twist blend 0.8/0.2), swing joints (thigh 0.6), RBF-driven corrective joints (pec, latissimus, ankle) evaluated by RigLogic (A3). [V]

### Practicality on the GPU in a game runtime (judgement, [U])

| Technique | Per-frame cost | Data | Fits Axcenity now? |
|---|---|---|---|
| Identity/body morphs (sliders) | **zero per frame** if baked into a per-character vertex buffer when sliders change (compute shader or CPU) | sparse deltas | Yes; already have blend shapes |
| Multilinear corner targets (fat × muscle × gender) | baked as above | more targets to author (3×3 per gender) | Yes, best way to stop fat/muscle conflicts |
| PCA shape space (statistical) | baked | needs scan data we may not have; licence-gated datasets | Use only from open data (GNM head, Anny/MakeHuman) |
| Region sliders with smooth masks | baked | per-region masks | Yes |
| Helper/twist bones driven by swing-twist decomposition | ~LBS (a few extra bones) | none | **Yes, first fix** |
| RBF/PSD corrective blend shapes (elbow, knee, shoulder, hip, wrist) | sparse morph per frame | ~10–20 shapes per body | **Yes, second fix** |
| Dual quaternion skinning | ≈ LBS in vertex/compute shader | none | Option (per-vertex DQ/LBS blend) |
| Optimized CoR | ≈ LBS + quaternion blend | 1 float3 per vertex; must be **re-precomputed when body shape changes** (offline-ish cost) | Later; check patents first [U] |
| Delta Mush / Direct Delta Mush | DM: iterative smoothing per frame (expensive); DDM: heavy per-vertex precompute storage | large | Not for v1 |

Rules for mixing many morphs without artefacts [judgement, grounded in the sources above]:
1. Separate identity (baked once) from animation (per frame: expressions + correctives).
2. Use multiplicative corner weights (MakeHuman) or a statistical model (MetaHuman) so axes interact, instead of independent additive sliders.
3. Region masks with smooth falloff and per-slider ranges that scale with height (MetaHuman "Scale Ranges by Height").
4. Recompute normals after baking (or carry normal deltas, as Sims 4 does).
5. Move joints with the shape: a linear joint regressor (SMPL) or bone deltas (Sims 4 BOND), otherwise sliders break skinning.

---

## D. The Sims 4 Create-a-Sim as a reference

### Evidence

- GDC 2015 talk "**Innovations in The Sims 4 Character Creator**", Sri Nair (EA): asset pairing so parts in the same body area never interpenetrate; **deformation maps** and when they beat morph geometry and bone-driven modification; direct manipulation ("pulling and tugging" on the Sim); data-driven selectors/modifiers combining morph targets, bone-driven deformations and deformation maps; smart randomization. [V abstract] https://www.gdcvault.com/play/1022085/
- Resource formats, from the community s4pi library (GPL-3.0, "Copyright (C) 2016 by Sims 4 Tools Development Team") [V source read] https://github.com/s4ptacle/Sims4Tools (develop branch):
  - **DeformerMap (DMap)** `s4pi Wrappers/CASPartResource/DeformerMapResource.cs`: header with version, width, height, AgeGender, Species (v6+), **Physique** (HEAVY, FIT, LEAN, BONY, PREGNANT, HIPS_WIDE, HIPS_NARROW, WAIST_WIDE, WAIST_NARROW, IGNORE for sculpt/modifier maps, AVERAGE = "always applied for a given age"), **ShapeOrNormals** (SHAPE_DEFORMER = positional deltas; NORMALS_DEFORMER = normal deltas), min/max col/row (cropped region), **RobeChannel** (PRESENT / DROPPED / ISCOPY: a second "robe" delta set interleaved with the "skin-tight" set), v7+: per-resource quantisation range (skinTightMin/Delta, robeMin/Delta; older defaults ±0.2 for shape, ±0.75 for normals); RLE-compressed scanlines of 3 bytes (skin-tight) or 6 bytes (with robe) per texel.
  - **SimModifier (SMOD)** `SimModifierResource.cs`: age/gender, region, sub-region, link tag, a **BonePose (BOND) key**, a **DMap shape key**, a **DMap normal key**, a bone-entry list. So one slider = bone deltas + shape DMap + normal DMap.
  - **BlendGeometry (BGEO)** `MeshChunks/BGEO.cs`: per-LOD blocks (indexBase, numVerts, numDeltaVectors); per-vertex flags for position delta and/or normal delta, delta-encoded vertex indices, 16-bit packed vectors. Keyed by **vertex ID**, so it is a classic morph target that requires matching vertex IDs.
- Community practice (Sims 4 Studio forums) [S]:
  - "For CAS items the second UV map is what EA has used to allow the item to move with the body sliders"; if `uv_1` is missing "the garment will distort" when sliders move (orangemittens) [V] https://sims4studio.com/thread/4639/general-question-uv-second-map
  - Vertex colour selects the DMap channel: 00FF00 skin-tight, 3FFF00 robe-like (skirts, loose chest); garments copy `uv_1` from the body via Blender Data Transfer; "deformer maps only come in halves" (left/right mirrored UV1 layout). [S] Sims 4 Studio threads
  - TS4 MorphMaker: fingers and inner mouth share the same uv1 area, so DMap morphs cannot target either without distorting the other (resolution/space limit). [V] https://modthesims.info/d/613061

### Mechanism (synthesis)

Body and every garment carry a second UV set that maps each vertex to a point on the *body's* surface parameterisation. Each slider/physique owns low-res textures in that space holding position deltas and normal deltas, with two variants (skin-tight and robe). At runtime each vertex samples the textures at its `uv_1`, picks the variant by its vertex colour, scales by the slider weight, and adds the result to position and normal. Bone deltas (BOND) move the skeleton for proportion changes; BGEO handles cases needing per-vertex precision. Result: any garment follows any current or future body slider without per-garment morph authoring.

### Fit for Axcenity (judgement [U])

- **Good fit** for the core problem (clothes following an open-ended set of body sliders) and for user-made clothes: one projection step per garment, then all sliders work.
- Implementation in wgpu: store deltas as RGBA16F textures (no 8-bit quantisation needed), bind as a texture array, sample in the compute pass that bakes the per-character mesh (not per frame). Normal deltas as a second array.
- Weaknesses: texture resolution limits fine detail (fingers, lips, ears); UV1 islands must be unique on the body (no mirroring if asymmetric sliders are wanted); loose garments need the "robe" variant (a smoothed, inflated delta field) or a cage. Thick armour/rigid parts need rigid binding instead.
- Equivalent alternative: bake per garment at import time a closest-point barycentric binding to the body, then transfer each body morph's delta into a garment morph. Cheaper to sample, but new body sliders require re-baking every garment. The DMap-texture approach is better for an extensible creator.

**Visual inspiration (not evidence):** CAS direct manipulation ("playing with clay", push/pull on body and face) [S] https://simscommunity.info/2013/09/21/the-sims-4-nick-corea-on-create-a-sim/

---

## E. Garment fitting to varying bodies

- **Blender Surface Deform:** bind a mesh to a target's surface; later target deformation drives it; Interpolation Falloff (smoothness), Strength, vertex-group mask, Sparse Bind; target must not have edges shared by >2 faces, concave faces, doubles or faces with collinear edges; artefacts grow with distance from the target surface. [V] https://docs.blender.org/manual/en/latest/modeling/modifiers/deform/surface_deform.html
- **Unreal / MetaHuman Outfit Asset:** source-body to target-body warp, multiple source bodies, best-match selection (A6). [V]
- **Unreal Mutable (Customizable Object):** Mesh Reshape "transfer the deformation from Base Shape to Target Shape into the Base Skeletal Mesh" [V stub] https://github.com/anticto/Mutable-Documentation/wiki/Mesh-Reshape ; Remove Mesh / Clip Morph / Clip Mesh With Mesh modifiers and "Remove Unseen" to strip geometry hidden under clothing [S] https://dev.epicgames.com/documentation/en-us/unreal-engine/python-api/class/CustomizableObjectNodeModifierRemoveMesh ; forum reports of artefacts combining clip/remove modifiers with body morphs [S].
- **Marketplace "Mesh Morpher" / "Morph Tools"** UE plugins copy morph targets to clothing (Morph Tools: by vertex proximity, vertex order irrelevant). [S] https://forums.unrealengine.com/t/morph-tools-plugin/127979
- **Reallusion Character Creator:** "Conform" is a collision-based push-out: "The vertex-based conforming algorithm pushes the vertex away from the skin", with Subdivision Level (extra test points), Iteration, Margin ("the offset distance from the meshes underneath it"), Increase Size, Close-fitting, Smooth; conform the outermost layer. [V] https://manual.reallusion.com/Character-Creator-4/Content/ENU/4.0/08_Cloth/Conforming_Clothing.htm . Hiding: Auto Hide Mesh (hides inner meshes, body and clothes) and per-cloth Hide Body Mesh tool. [S] https://manual.reallusion.com/Character-Creator-4/Content/ENU/4.0/08_Cloth/Automatically_Hiding_Inner_Meshes.htm
- **CLO / Marvelous Designer:** morph-target avatar import; garment simulates while the avatar morphs over N frames; large changes can make garments fall off, so morph in stages. [S] https://support.clo3d.com/hc/en-us/articles/115000407347-Change-Pose-by-OBJ-Morphing ; EveryWear "Auto Fitting" (beta) adjusts 2D patterns and 3D garment to the avatar. [S]
- **Sims 4:** asset pairing (layer/region ownership prevents interpenetration) + DMap robe/skin-tight channels (D). [V]
- **MetaHuman:** Body Hidden Face Map (removes always-covered faces at assembly) and Head Hidden Face Map (remove *or shrink*). [S]

**Recommendation (E, judgement):**
1. Bind each garment to the body once at import (closest point + barycentric + normal offset, Surface-Deform style) to generate its `uv_1` into body space and a skin-tight/robe flag per vertex.
2. Body follows sliders through DMap-style delta textures; garments follow through the same textures (D).
3. Hiding: per-garment **hidden-region mask in body UV space** (MetaHuman-style), combined at outfit-assembly into a per-character face-visibility buffer: faces fully covered are removed (index buffer rebuild), faces near garment edges are kept but "shrunk" (our current mask shapes). Use region IDs (torso, upper arm, ...) as a coarse fallback.
4. Layering: each garment has a layer index (underwear < shirt < jacket < coat); after baking, run a small push-out pass for layer N against the baked surface of layers < N plus the body (Reallusion-style margin push along the inner surface normal, a few iterations, done once per outfit change on the GPU). Asset-pairing rules (Sims) stop impossible combinations rather than fixing them geometrically.

---

## F. Real-time skin and eyes

### Evidence

- **Separable SSS** (Jimenez, Zsolnai, Jarabo, Freude, Auzinger, Wu, von der Pahlen, Wimmer, Gutierrez; Computer Graphics Forum 2015): approximates the 2D diffuse profile with a single separable kernel, "only two 1D convolutions", post-process, "under 0.5 milliseconds per frame", 7 samples per pixel with importance sampling and per-pixel jittering; public demo/code correspond to the earlier tech report. [V] https://iryoku.com/separable-sss/
  - Reference code **licence**: BSD-2-Clause-style, "Copyright (C) 2011 Jorge Jimenez ... Diego Gutierrez", binary redistributions **must reproduce "Uses Separable SSS. Copyright (C) 2011 by Jorge Jimenez and Diego Gutierrez."** Apache-2.0 compatible if that credit goes in NOTICE/credits. [V] https://github.com/iryoku/separable-sss/blob/master/LICENSE.txt
- **Burley normalized diffusion:** Christensen & Burley, "Approximate Reflectance Profiles for Efficient Subsurface Scattering", Pixar Technical Memo #15-04, July 2015: profile = sum of two exponentials parameterised by albedo and mean free path; ~5.3% average error vs Monte Carlo; 2018 erratum (CDF inverse). [S] https://graphics.pixar.com/library/ApproxBSSRDF ; Burley, "Extending the Disney BRDF to a BSDF with Integrated Subsurface Scattering", SIGGRAPH 2015 course [U, not opened].
- **Unreal Subsurface Profile:** screen-space; Lambert + GGX lit, scattering as a post process; separable two-pass blur; ≤255 profiles; checkerboard (half pixels diffuse, half specular) or full-res packing that "requires at least a 64-bit scene color format"; **Enable Burley** "requires Temporal Anti-Aliasing to display properly"; Mean Free Path colour/distance; dual specular Roughness 0 ("softer lobe"), Roughness 1 ("tighter lobe"), Lobe Mix; transmission params; default profile ≈ Caucasian skin, visible scatter ≈ 1.2 cm; "does not work in the non-deferred (mobile) rendering mode"; profile id stored in GBuffer (metallic channel). [V] https://dev.epicgames.com/documentation/unreal-engine/subsurface-profile-shading-model-in-unreal-engine . Defaults (UE 5.0 Python API): roughness0 = 0.75, roughness1 = 1.3, lobe_mix = 0.85, IOR 1.55. [S] https://docs.unrealengine.com/5.0/en-US/PythonAPI/class/SubsurfaceProfileStruct.html
- **Godot 4 Forward+ (clustered forward, MIT)** does screen-space SSS in a forward renderer by rendering specular to a **separate specular MRT** (`COLOR_PASS_FLAG_SEPARATE_SPECULAR`, `RB_TEX_SPECULAR`) when SSS is used, running `ss_effects->sub_surface_scattering(color, depth, ...)` on the diffuse colour buffer, then merging specular. [V] https://github.com/godotengine/godot/blob/master/servers/rendering/renderer_rd/forward_clustered/render_forward_clustered.cpp — direct precedent for Axcenity.
- **Pre-integrated skin** (Penner & Borshukov, "Pre-Integrated Skin Shading", GPU Pro 2, 2011, pp. 41–55; SIGGRAPH 2011 Advances course): pre-blur the diffuse BRDF with the skin profile into a LUT indexed by N·L and curvature, plus shadow-penumbra scattering and normal-map blurring; known LUT integration-limit bug in the sample code. [S] https://www.slideshare.net/slideshow/penner-preintegrated-skin-rendering-siggraph-2011-advances-in-realtime-rendering-course/13966747
- **GPU Gems 3 ch. 14** (d'Eon & Luebke): Kelemen/Szirmay-Kalos specular, Fresnel F0 = 0.028 (IOR 1.4), white specular; six-Gaussian diffusion profile; texture-space diffusion; modified translucent shadow maps for thickness. [V] https://developer.nvidia.com/gpugems/gpugems3/part-iii-rendering/chapter-14-advanced-techniques-realistic-real-time-skin
- **Roughness/specular across the face** (Weyrich et al., "Analysis of Human Faces using a Measurement-Based Skin Reflectance Model", TOG 25(3) 2006, 149 subjects, 10 regions): highest specular scale on nose, eyebrows, eyelids, lips, forehead (sebum on nose/forehead); "the nose shows the smallest values for the roughness parameter m". Per-region numbers are in the paper's figures, not extracted. [V] https://cdfg.csail.mit.edu/pdf/journal/2006-weyrich-analysis-of-human-faces-using-a-measurement-based-skin.pdf
- **Cavity / specular occlusion and micro detail:** Jimenez & von der Pahlen deck multiplies cavity into specular reflectance [S]; MetaHuman ships cavity, occlusion, curvature, bent-normal maps and Micro Skin Detail normals (A4). [V]
- **Eyes:** Unity HDRP Eye shader: two-layer model ("the first layer describes the cornea and fluids on the surface", second = sclera + iris), cornea refraction, caustics, pupil dilation, limbal darkening, SSS. [V] https://docs.unity3d.com/Packages/com.unity.render-pipelines.high-definition@17.0/manual/understand-eyes.html . Cornea Refraction node: IOR default 1.333, iris plane offset 0.02 for the default model; Limbal Ring Size/Fade/Intensity; Iris Clamp Color. [S] https://docs.unity3d.com/Packages/com.unity.shadergraph@13.1/manual/Cornea-Refraction-Node.html . MetaHuman uses separate eye meshes + eyeshell + eyeEdge + cartilage + eyelash shadow meshes (A4). Unreal Eye shading model exposes Iris Mask and Iris Distance. [S]

### Recommendation for a forward renderer with depth/normal prepass + TAA (judgement)

1. **Phase 1 (cheap, no new targets):** pre-integrated diffuse LUT (N·L × curvature from a baked curvature map), dual-lobe GGX specular (two roughness multipliers + mix, start at 0.75/1.3/0.85), F0 ≈ 0.028, cavity-driven specular occlusion, tiled micro-detail normal with distance fade, roughness map with lower roughness/higher spec on nose and forehead.
2. **Phase 2 (quality):** Godot-style screen-space SSS: forward pass writes **diffuse lighting** and **specular lighting** to two RGBA16F targets (MRT) for SSS materials, plus an SSS mask/profile (stencil or R8: profile id and world scatter radius). Two separable depth-aware blur passes using the existing prepass depth (Burley/Christensen-Burley profile, ~7–11 importance-sampled taps with per-pixel jitter rotated each frame so TAA converges), then composite `blurred diffuse + specular` before TAA. Transmission for ears/nostrils from shadow-map thickness.
3. Required targets: existing depth (+ normal), HDR diffuse (RGBA16F), HDR specular (RGBA16F) or UE-style checkerboard packing, 8-bit stencil/mask, one ping-pong RGBA16F for the blur.
4. **Eyes:** keep separate eyeball meshes with a modelled cornea bulge; parallax-refraction of the iris in the eye shader (IOR ~1.33), limbal ring term, sclera with soft SSS-like wrap; an **eye occlusion shell** (alpha-blended multiply, AO gradient from lid edges), a **tear-line/wet-edge strip** (alpha-blended, low roughness), eyelashes + eyelash shadow card. All translucent pieces sort after opaque; TAA ghosting on thin translucent strips needs reactive masks.

---

## G. Fabric

- **KHR_materials_sheen** (ratified, © 2018–2020 Khronos): `sheenColorFactor` (default black = off), `sheenColorTexture` (sRGB), `sheenRoughnessFactor`, `sheenRoughnessTexture` (alpha); "Charlie" distribution (Conty Estevez & Kulla, SIGGRAPH 2017 "Production Friendly Microfacet Sheen BRDF"), alpha = roughness²; Charlie visibility by default, Ashikhmin-Premoze as cheaper (not energy conserving with albedo scaling); albedo-scaling layering `base * min(1 - max3(sheenColor)*E(NdotV), 1 - max3(sheenColor)*E(NdotL))` with a precomputed E LUT. [V] https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_materials_sheen/README.md
- **Filament cloth model:** Ashikhmin velvet NDF (Neubelt & Pettineo normalisation, The Order: 1886, SIGGRAPH 2013) or Charlie; Neubelt visibility instead of Estevez-Kulla shadowing for cost; no Fresnel; sheen colour default 0.04; optional subsurface colour with wrap diffuse w = 0.5; velvet = dark base + chroma in sheen; denim = base-colour chroma + default sheen; "leather, silk and satin can be recreated using the standard or anisotropic material models". [V] https://google.github.io/filament/Filament.md.html
- **KHR_materials_anisotropy** (ratified [S]): `anisotropyStrength` (default 0, × texture B), `anisotropyRotation`, texture RG = direction in tangent space. [S] https://github.com/KhronosGroup/glTF/tree/main/extensions/2.0/Khronos/KHR_materials_anisotropy
- **Per-fabric look (practice, [U]):** denim and canvas: high roughness (≈0.7–0.95), weak sheen, strong twill/weave detail normal; knit/wool: very rough, noticeable sheen, fuzz; velvet/velour: dark base, strong coloured sheen; satin/silk: low-to-mid roughness with anisotropy along the weave; leather: mid roughness (≈0.35–0.6), no sheen, cavity and grain normal, slight clearcoat on polished leather. No primary measured table found.
- **Seams, stitches, buttons (practice):** Marvelous Designer topstitches export either as 3D geometry or baked into normal maps ("Apply Normal Map to Topstitch") [S] https://support.marvelousdesigner.com/hc/en-us/articles/47358426952729 ; trim-sheet texturing as a production technique ("The Ultimate Trim: Texturing Techniques of Sunset Overdrive", Morten Olsen, Insomniac, GDC 2015) [S] https://gdcvault.com/play/1022323/ . Rule of thumb [U]: silhouette-relevant parts (buttons, collars, thick hems, zips) as geometry; stitch lines, ribbing, piping as normal-map trims on a second UV or trim sheet; prints/logos as mesh decals or a second material layer (no deferred decals in a forward renderer).

---

## H. Cloth simulation in games

- **XPBD** (Macklin, Müller, Chentanez, NVIDIA, Motion in Games 2016): compliance-based PBD; stiffness independent of iteration count and timestep; gives constraint force estimates. [S] https://matthias-research.github.io/pages/publications/XPBD.pdf
- **Small Steps** (Macklin, Storey, Lu, Terdiman, Chentanez, Jeschke, Müller, SCA 2019, DOI 10.1145/3309486.3340247): split the frame into n substeps with **one** XPBD iteration each; better than n iterations on one step; less damping and constraint error. [S] https://diglib.eg.org/handle/10.1145/3309486-3340247
- **Long Range Attachments** (Kim, Chentanez, Müller-Fischer, SCA 2012): inextensibility for game clothing via unilateral distance limits to the kinematic attachment points. US patent **US9070220** "Method of simulating clothing using long range attachments", assignee NVIDIA [S] https://patents.google.com/patent/US9070220 — check expiry/claims before copying the exact method [U].
- **Chaos Cloth** (A6): kinematic vs simulated via painted Max Distance (0 = skinned only), Backstop distance/radius, Anim Drive springs, tethers (LRA), Physics Asset capsules/spheres for collision, XPBD anisotropic constraints; CPU. [V/S as marked above]
- **Rust crates (crates.io API, 2026-11-14)** [V]:

  | Crate | Version / updated | Downloads | Licence | Notes |
  |---|---|---|---|---|
  | bevy_silk | 0.10.0 / 2025-12-08 | 12,825 | MIT | Verlet cloth, coupled to Bevy |
  | bevy_verlet | 0.10.0 / 2025-12-08 | 14,410 | MIT | Verlet, Bevy |
  | rapier-cloth-core / rapier-cloth | 0.3.0 / 2026-09-30 | 69 / 26 | MIT | "Engine-independent CPU XPBD cloth solver"; new |
  | bevy_softbody | 0.1.0 / 2026-07-14 | 33 | MIT | GPU XPBD cloth on wgpu, Bevy 0.19; single release |
  | tinyverlet | 0.1.1 / 2026-11-07 | 30 | MIT | Verlet particles/cloth |
  | oxihuman-physics | 0.2.1 / 2026-07-13 | 600 | Apache-2.0 | part of a MakeHuman-compatible Rust generator; tiny adoption |
  | havok-cloth | 0.3.1 / 2026-11-06 | 118 | **GPL-3.0-or-later** | incompatible |

  None is mature/maintained enough to depend on. **NvCloth** (C++) is under the "Nvidia Source Code License (1-Way Commercial)", not OSI; avoid. [V] https://github.com/NVIDIAGameWorks/NvCloth (NvCloth/license.txt)
- **Recommendation (judgement):** write a small in-house CPU XPBD cloth (rayon-parallel per garment) using small-steps substepping (e.g. 8–16 substeps × 1 iteration), stretch + bending (dihedral or cross-edge springs) constraints, LRA tethers to skinned "kinematic" vertices, per-vertex max-distance from the skinned pose with backstop, collision against capsules/spheres from the existing physics shapes (later SDF of the body). Simulate only skirt hems, coat tails, hoods, capes, long hair cards; everything else is skinned with RBF correctives. Run sim on a low-res proxy and drive the render mesh with a barycentric wrap (UE "sim mesh drives render mesh"). Move to a wgpu compute version only if profiles show it is needed.

---

## Things I could not verify

- The current Unreal EULA, MetaHuman licence page and UE FAQ text (403 / login redirects); licence conclusions in A5 rely on Epic's MIT/DNACalib licence files (read) plus press summaries.
- The MetaHuman outfit warping algorithm; the exact role of the `cartilage` and `eyeshell`/`eyeEdge` meshes; the Hidden Face Map docs page (search summary only).
- Whether Chaos Cloth runs only on CPU (community evidence only).
- SMPL pose-blend-shape count (207) and PCA wording (lecture slides, not the paper); STAR/SUPR licence text (README summaries only); 3D Scan Store current terms (403); Digital Emily 2 licence text.
- SOMA-X licence of the SOMA-shape identity data (model card summary only); GNM Head vertex count.
- Patent status of Optimized-CoR skinning, Delta Mush / Direct Delta Mush, and LRA (NVIDIA patent US9070220 exists; expiry unchecked).
- Fabric roughness numbers (no measured primary table found); Weyrich per-region numeric values (not extracted).
- Burley 2015 SIGGRAPH course notes and Penner's slides (not opened); the Sims 4 GDC talk content beyond the abstract; s4pi interpretation of DMap/BGEO is community reverse engineering, not EA documentation.
