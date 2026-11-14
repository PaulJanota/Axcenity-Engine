# Humanoid QA

Scripts that measure the default humanoid for defects and render a fixed set of comparison sheets, so a change to the character can be checked against a "before" set (the Character Studio overhaul keeps its baseline in `docs/development/character-studio/baseline/`). They read the glTF files only; they never change the asset.

| File | What it does |
|---|---|
| `qa_scene.py` | Shared helpers: import a file, set a look (frame, build, face, expression and mask shapes, the `Frame_Feminine` bone offsets and eye offsets and scales from `Humanoid.customization.json`), the fixed test poses, deformed geometry, the studio (Cycles CPU, AgX, OIDN, three area lights, grey cyclorama, seed 7) and contact sheets. |
| `measure.py` | Writes `measurements.json`: per-object stats and topology, skin weights, clothing and hair clipping for each frame and build, joint deformation in the test poses, face shapes (self-intersection, eyes and lids, teeth), shape ranges, proportions against Drillis and Contini (1966). |
| `render_baseline.py` | Renders the sheets: turnarounds, face close-ups and topology, base body, build range, hair styles, test poses, expressions, a details sheet, and `fit` (clothing fit at the extremes: the outfit, the tee alone and the underwear on both frames with the heavy build, the shoes on heavy and slim). |
| `thresholds.json` | The clipping limits `measure.py --check` holds the build to, with why each garment's limit isn't zero. |

## Running

Needs the `bpy` module (Python 3.13 for bpy 5.x) and Pillow:

```bash
python -m venv .bpy && .bpy/bin/pip install bpy pillow
.bpy/bin/python -I assets/characters/humanoid/qa/measure.py --source-ngons --out docs/development/character-studio/baseline/measurements.json
.bpy/bin/python -I assets/characters/humanoid/qa/render_baseline.py --out docs/development/character-studio/baseline
```

Measuring takes about 25 seconds (`--source-ngons` runs the build's `build()` into a temporary folder to count polygons before glTF triangulates them; leave it out to skip that). Rendering takes about a minute on a 16-core CPU. Both take `--parts` and `--default` to point at other files, `--only` to run some sections (`files,weights,clipping,face,shapes,proportions,deformation`) or sheets (`turnaround_masculine,turnaround_feminine,face_closeups,face_topology,body_clay,build_range,hair_styles,deformation_poses,expressions,details,fit`). **After changing the build**, check it: `measure.py --only clipping --out <file> --check assets/characters/humanoid/qa/thresholds.json` exits 1 and prints each limit that got worse (the waist gaps and the layer order must stay at zero). Look at `fit.png` too: a number isn't a look. For an "after" set, render into another folder with the same settings and compare sheet by sheet; don't change `STUDIO`, the cameras or the poses, or the sets stop being comparable.

## Reading the numbers

- Blender space: metres, Z up, facing -Y. Distances are millimetres unless a key ends in `_m`.
- Topology is on position-merged vertices (glTF splits vertices at UV seams). glTF stores only triangles, so n-gons are counted in the build (`source_ngons`); in the files they show up as sliver triangles.
- Clipping: a skin vertex is **covered** when a ray along its normal meets the layer within 5 cm (6 cm for hair), **poke** when instead the layer's outward face is within 15 mm under it (the skin shows through), **exposed** when that face is 15 to 30 mm under it (bare skin over an opening, such as a shoe's throat, or a layer passing through the head). `waist_gap_mm` is the height of bare skin seen between the outfit's garments from 16 directions (hits on an arm's skin don't count: a heavy build's arm reaches inside the rays' start); in `Humanoid.glb` that skin is removed, so it's a hole.
- Layering checks the build's layer order (the trousers' waistband under the untucked tee, the tee under the jacket) and counts only inner points facing away from the body: an opening's rolled-in band faces the skin and can't be seen.
- Known limits of the poke test, checked by eye in `fit.png`: in the body's midline creases (the crotch, the cleft between the buttocks) cloth bridges the fold and a ray along the skin's normal grazes it, so the briefs and trousers count points there that don't show; toe tips sit inside the sneakers' solid sole, whose open top rim lets their rays out; in `hip_flex_100` the raised thigh holds the waist rays' start points, so that pose's `waist_gap_mm` measures the thigh from inside (the renders show the waist covered).
- Poses (left side): flexion from straight, so `elbow_135` turns the forearm 135 degrees; the shoulder poses do the whole turn at the shoulder joint; `fist` curls each finger 85/100/70 degrees. `local_volume_ratio` is the signed volume of the skin within the listed radius of the joint, seen from the joint, posed over rest (rigid parts keep it at 1); `ring_area_ratio` is the area of the skin's cross-section at the joint, posed over rest.
- Feminine looks move the skeleton by the customization file's offsets and the eyes by their offset and `eye_scale`, which is what CS3 is meant to do; `face.feminine_eyes_not_moved` is the same look without that.
