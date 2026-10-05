# Skills

Agent skills for working on Torqa (Claude Code loads them from here when a task matches).

Torqa's own:

- `torqa-look` — the art direction: stylized, faceted low-poly in a soft pastel palette.
- `torqa-art-pipeline` — making models with headless Blender in the art container.
- `torqa-render-review` — rendering the world and models in the dev container for review.

Imported unchanged from [ra100/blender-claude-plugin](https://github.com/ra100/blender-claude-plugin)
at commit `78e9151`, MIT licence (`LICENSE` in each folder), for Blender 5.x. Their "MCP-first"
advice does not apply here; Torqa runs Blender headless (see `torqa-art-pipeline`):

- `blender-python-scripting`, `blender-modeling-modifiers`, `blender-animation-rigging`,
  `blender-scene-rendering`, `blender-geometry-nodes`.

To update them, copy the folders again from upstream and change the commit above.
