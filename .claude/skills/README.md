# Skills

Agent skills for working on ORQWA (Claude Code loads them from here when a task matches).

ORQWA's own:

- `orqwa-look` — the art direction: as realistic as possible (ADR 0020), replacing the
  faceted pastel look.
- `orqwa-art-pipeline` — making models and textures with headless Blender in the art
  container, with quick Cycles previews. Its texture baker, previews and bpy pitfalls come from
  the author's earlier game project (choppa).
- `orqwa-render-review` — rendering the world and models in the dev container for review.
- `orqwa-showcase` — re-rendering the showcase pictures of saved courses on the Mac's GPU,
  with the shot list in `shots.txt`.
- `orqwa-physics` — the ride simulation and the rider and bike motion: where they live, the
  rules (springs, hysteresis, legs that reach), tick traces before tuning. Adapted from the
  author's earlier game project (choppa).

Imported unchanged from [ra100/blender-claude-plugin](https://github.com/ra100/blender-claude-plugin)
at commit `78e9151`, MIT licence (`LICENSE` in each folder), for Blender 5.x. Their "MCP-first"
advice does not apply here; ORQWA runs Blender headless (see `orqwa-art-pipeline`):

- `blender-python-scripting`, `blender-modeling-modifiers`, `blender-animation-rigging`,
  `blender-scene-rendering`, `blender-geometry-nodes`.

To update them, copy the folders again from upstream and change the commit above.
