# Character Studio overhaul: progress

Where the overhaul stands, newest first. The plan and its stage IDs are in [implementation-plan.md](implementation-plan.md).

| Stage | State | Notes |
|---|---|---|
| 1. Research and audit | **Done, awaiting Paul's review** (2026-11-14) | [research.md](research.md), [audit.md](audit.md), [implementation-plan.md](implementation-plan.md); baseline in [baseline/](baseline/) by `assets/characters/humanoid/qa/` |
| Before Stage 2 | Not started | Fix the waist hole in CS1 (audit §2.2 defect 1); merge #401, #402, then #400 rebased |
| 2. Architecture and contracts (CO1) | Not started | Needs the plan's decisions approved; then `architecture.md`, `agent-assignments.md`, `testing.md`, `visual-qa.md`, `dependencies.md` and the ADRs |
| 3. Foundation (CO2–CO6) | Not started | |
| 4. Parallel specialists (CO7–CO12) | Not started | |
| 5. Integration (CO13) | Not started | |
| 6. QA, optimization, second wave (CO14–CO18) | Not started | |
| 7. Final integration | Not started | |

## 2026-11-14: Stage 1

- **Agents:** the lead read the code, the reference library and the existing plans itself; five specialists ran in parallel: hair research, body/face/skin/cloth research, formats and libraries, a render-pipeline audit (read-only) and a geometry and render baseline (Blender). The lead checked every claim a decision rests on against the source, the vendored code, the cargo registry or the repositories' declared licences (marked ✔ in the research).
- **Files:** this folder (research, audit, plan, progress, the researchers' notes in `notes/`, the baseline); `assets/characters/humanoid/qa/` (measurement and render scripts); a row in `docs/README.md`; a pointer in `docs/research/character-studio.md`; a stale line fixed in `docs/research/platform-expansion.md` (rapier has soft bodies since 0.36.0); the wiki's roadmap, documentation history and home page.
- **Tests:** none run beyond the scripts' measurements (no engine code changed). `python scripts/check-wiki.py`: 0 errors. Relative links in the new docs checked.
- **Dependencies added:** none.
- **Findings that need action before the overhaul:** a hole at the waist of CS1's shipped default (unmerged #401); three open character PRs to merge in order.
