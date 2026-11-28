# Tree textures

The bark and leaf maps Insert > Tree's recipes use (foliage plan T5, `docs/research/foliage-grass-seasons.md`). They're CC0 1.0 Universal sets from ambientCG (see `LICENSE.txt`), resized and cut down from their 4K JPG downloads. Credit isn't required, but appreciated.

| File | Set | Changes |
|---|---|---|
| `bark_oak_color.jpg`, `bark_oak_normal.jpg` | Bark001 (https://ambientcg.com/a/Bark001) | 1024 × 2048, Color and NormalGL |
| `bark_pine_color.jpg`, `bark_pine_normal.jpg` | Bark014 (https://ambientcg.com/a/Bark014) | 1024 × 1024, Color and NormalGL |
| `bark_birch_color.jpg`, `bark_birch_normal.jpg` | Bark013 (https://ambientcg.com/a/Bark013) | 1024 × 1024, NormalGL; the Color map made grey and pale, for a birch |
| `leaves_broad.png`, `leaves_broad_normal.jpg` | LeafSet024 (https://ambientcg.com/a/LeafSet024) | 1024 × 1024; the Opacity map is the PNG's alpha. A 3 × 3 grid of leaves, stems down |
| `leaves_conifer.png`, `leaves_conifer_normal.jpg` | LeafSet019 (https://ambientcg.com/a/LeafSet019) | 1024 × 1024; Opacity as alpha. Three sprays, stems at the right (`textures::CONIFER_SPRAYS`) |

The editor embeds them (`crates/editor/src/app/trees.rs`) and writes them to a project's `assets/trees/textures/` beside the first tree's recipe; the recipe's `texture` and `normal_map` name them relative to its own folder, so a recipe can point at a project's own maps instead.
