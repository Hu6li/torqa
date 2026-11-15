---
name: orqwa-showcase
description: Re-render ORQWA's showcase pictures (docs/images/showcase — the gallery, README and wiki home page) on the Mac's GPU from the rider's saved courses, add or change showcase shots, and review and publish them. Use when asked to refresh, extend or re-run the showcase or "the nice screenshots", after a visible change to riders, scenery, seasons or the HUD that the showcase shows, or when preparing website pictures.
---

# Showcase pictures

The showcase is ORQWA at its best: saved real-world courses at the **Ultra** preset, rendered
on a real GPU in a 3200 × 1800 window, exported as 1600 × 900 JPEGs to `docs/images/showcase/`.
They appear in [docs/screenshots.md#showcase](../../../docs/screenshots.md#showcase), the
README (`fuji-lakeside-evening`, `alpenbrevet-furka-group-climb`) and the wiki home page
(`saimaa-ridge-group-evening`). App and route screenshots are a different job, done in the dev
container (`orqwa-render-review`).

Every shot is listed in [`shots.txt`](shots.txt): `<course> <name:distance:camera[:right,up,back][:key=value…]>`.
The name is the published file's. **Keep `shots.txt` the source of truth**: when a shot is
reframed, added or dropped, change it there, not only on the command line.

## What it needs (macOS)

This is the exception to "containers only": GPU renders run natively (CLAUDE.md, ADR 0021).

1. **The portable Godot editor** of the version the project uses (`godot --version` in the dev
   container), e.g. into the scratchpad:
   `curl -sSLO https://github.com/godotengine/godot/releases/download/<version>-stable/Godot_v<version>-stable_macos.universal.zip`,
   unzip, `xattr -dr com.apple.quarantine Godot.app`.
2. **The ORQWA extension of the checked-out commit** in `app/bin/liborqwa_gd.dylib`. Docker on
   macOS cannot build it; take it from the **macOS app** artifact of the latest CI run on
   `main` (`gh run list --branch main --workflow CI`, then
   `gh run download <id> -n ORQWA-macos-arm64.zip -D <scratch>` and copy
   `ORQWA.app/Contents/Frameworks/liborqwa_gd.dylib`). A release's dylib is only right while
   no GDScript calls a newer Rust function: an old one fails with a script error mid-ride.
   `app/bin/` is ignored by Git.
3. **The saved courses** in `~/Library/Application Support/ORQWA/courses/` (`COURSES=` for
   another folder): Saimaa Cycling Tour, Fuji, Gorges de la Nesque, Alpenbrevet Gold, Wada
   Ōtarumi Pass, Grosse Scheidegg, Lago del Narèt. Their map, terrain and kept worlds are
   reused from `~/Library/Caches/ORQWA`; a course without a kept world is built first, which
   takes minutes.
4. Import once after a checkout: `"$GODOT" --headless --path app --import`.

Do not run GPU renders while `scripts/check.sh` or container renders are running.

## Rendering

```sh
GODOT=<scratch>/Godot.app/Contents/MacOS/Godot OUT_DIR=<scratch>/showcase \
    .claude/skills/orqwa-showcase/render.sh                       # all shots, ~2 min per course
ONLY="saimaa-ridge-winter fuji-lake-evening" GODOT=… .claude/skills/orqwa-showcase/render.sh
```

`render.sh` runs `scripts/render-showcase.sh` once per course (a separate `HOME` keeps your
own profile and settings out), keeps the full-size PNGs in `OUT_DIR` (default
`screenshots/showcase/`) and writes the JPEGs straight into `docs/images/showcase/`.
Check `git status` afterwards: every published picture is replaced, so look at all of them.

Shot fields (see `app/tools/render_views.gd`): camera `0` chase, `1` first person, `2` drone;
`right,up,back` puts the camera aside (negative `back` is in front of the rider, e.g.
`-5,3,-20` for a group coming towards the camera); `time=Morning|Midday|Evening`,
`weather=Clear|Cloudy|Hazy|Rain`, `season=Spring|Summer|Autumn|Winter` (Autumn and Winter
dress the riders in winter kit, R245), `rider=female|male`, `group=<1–7 friends>`, `hud=0` (the
world alone), `fov=`, `elevation=gradient` (the profile coloured by gradient, R241–R242) and
`map=dark`. Each shot rides the last 1.5 km at threshold power, so the HUD shows riding
figures.

## Review

Open every picture at full size (the PNG) before publishing; judge it with `orqwa-look`.
Drop or reframe shots with:

- a camera inside terrain, a wall or a tree (free cameras aside are the usual culprits; try
  a few `right,up,back` framings);
- notices, moment cards or `--` in the HUD;
- nameplates over faces, friends clipping into each other or the rider;
- a season or time that no longer matches the name.

## Publishing

Commit the JPEGs with the change that made them necessary, or as `docs:` on their own.
New pictures go into `docs/screenshots.md#showcase` under their course; keep the attribution
line there (OpenStreetMap ODbL, Mapterhorn CC BY 4.0, AWS Terrain Tiles). Link the review
images in the PR by commit (`raw.githubusercontent.com/ORQWA/ORQWA/<commit>/docs/images/showcase/…`).
Keep the folder small: 1600 px JPEGs, no PNGs in the repository.
