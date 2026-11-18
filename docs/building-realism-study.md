# Building detail

R256 integrates the building study into the production renderer. **Ultra** selects detailed
fitted models and adds nearby façade geometry; Low, Medium and High retain the ordinary
geometry. Scanned plaster and roof materials are shared across presets and preserve the
existing regional palette hues. All resources are bundled and work offline.

## Rendering and kept worlds

`art/buildings/build.py -- --detail` exports a second appearance of every catalogue model
under `app/assets/models/buildings/detail/`, preserving the existing names, dimensions and
placement manifest. These variants add glazing bars and main-roof drainage where appropriate.
Window panes carry normalized UVs plus stable seeds in the second UV channel; the opaque
glass shader shows homes as mostly dim rooms, a minority with drawn-back curtains, a blind
partly down or a sheer net, while retaining sky reflections. Shop windows (their glass marked by
V lifted by one) show a lit sales floor with shelves and ceiling lights, never curtains.
Tall French blocks keep their existing balcony/dormer geometry budget and use the interior
shader without extra glazing bars.

`orqwa_world::nearby_building_details` reconstructs raised frames, sills, glazing bars, glass and
trim from existing saved shell panels. Plastered/timber window grids, shopfronts, office
ribbons and individual rectangular windows are supported. Unsupported, narrow and
non-rectangular panels keep their existing appearance. Mapped shopfronts suppress the
ordinary ground-floor window detail behind them. Decorations follow each façade's orientation
and are grouped on a 64 m grid for culling.

Only Ultra derives those extra meshes, on demand around the camera. Ordinary chunk uploads
carry no extra façade buffers. The renderer requests windows within a 308 m loading margin and
refreshes after 64 m of movement, at most one chunk per frame. Additions come in 64 m cells:
a refresh names the cells already uploaded, the core derives only those newly in reach and
names the ones to let go of, a cell's width beyond the margin so moving to and fro does not
reload them; cells still in reach stay uploaded. The core reads a chunk's façade panels once
(`orqwa_world::Facades`, kept by `WorldChunks`) and forgets them when the camera leaves the
chunk's margin or the chunk is let go of. In a dense synthetic town of four-storey blocks
(container, release build) a refresh went from 1.9 million derived triangles and 43 ms on
the main thread, besides reading the whole chunk from disk again, to 0.28 million and 7 ms
on average, the first full load included. Ride preparation waits for nearby additions;
the whole course never retains close façade geometry. Fitted models switch between ordinary and
detailed appearances at **160 m**, with a **20 m** fade; shell additions use the same close
range. Only the appearance going away dithers: the one taking over is already drawn, opaque
and without a fade of its own, before it starts (from 140 m for ordinary models, and shells
from 40 m short of the model range). Two copies dithering over the same distances left
holes where neither was drawn, so buildings looked nearly transparent mid-fade. Model ranges
come from the quality preset alone; View distance changes no longer stretch models uploaded
earlier past the shells that replace them. The range remains fixed when View distance changes. Switching to a lighter preset
refreshes uploaded chunks and releases the detailed model cache. The offline graphics
benchmark uses the same quality-dependent model upload path.

The generated world, stored layout, original GLBs, model names and instance buffers stay
unchanged. Existing kept courses receive the appearance through presentation; changing
quality requires no regeneration. Winter snow conditions also reach the new trim and roof
drainage materials. These are plausible scenery details rather than surveyed façades.

## Materials and review

`art/textures/building_scans.py` converts the archived CC0 plaster and roof scans into
1024 px albedo, OpenGL normal and roughness maps. Albedo is neutralized and normalized in
linear space so wall and roof tints remain meaningful. Colour/roughness use quality-90 WebP;
normal maps are lossless. Godot imports use VRAM compression, mipmaps and normal-map settings.
See [credits](CREDITS.md) and the originals' `SOURCE.md` files for provenance and checksums.

The [initial house](images/building-realism/house-comparison.jpg) and
[town study](images/building-realism/town-comparison.jpg) preserve the design review.
Production comparisons use the same cameras and light:

| View | Medium | Ultra |
|---|---|---|
| Fitted house | [Ordinary model](images/building-integration/model-medium.jpg) | [Close model](images/building-integration/model-ultra.jpg) |
| Generated façade | [Shell](images/building-integration/facade-medium.jpg) | [Frames, glazing and trim](images/building-integration/facade-ultra.jpg) |
| Saved Bern course | [Street](images/building-integration/city-medium.jpg) | [Street](images/building-integration/city-ultra.jpg) |

The [fade before](images/building-integration/fade-before.jpg) and
[after](images/building-integration/fade-after.jpg) views stand 170 m from the house, inside the
swap between appearances; the [shop windows before](images/building-integration/shops-before.jpg)
and [after](images/building-integration/shops-after.jpg) show the façade fixture's ground-floor
shop. The saved Bern course views predate these two fixes.

The same course also has [Low](images/building-integration/city-low.jpg) and
[Winter](images/building-integration/city-winter.jpg) captures. The distant
[model](images/building-integration/model-far-ultra.jpg) and
[shell](images/building-integration/facade-far-ultra.jpg) views return to ordinary detail.
All 72 catalogue models were reviewed on both ordinary and Ultra appearances; rebuilding the
Ultra exports was byte-identical. The six-storey French block stays at 9,328 triangles.

## Native measurement (2026-11-08)

The [raw CSV](data/building-detail-m5.csv) and [metadata](data/building-detail-m5.json) record
the CI-built `b955ac1` app on an **M5 Pro**, using Metal/Forward+ and the same saved 61-chunk
Bern course for both builds. The baseline resource pack is `bcf38eb7`. Each paused chase-view
sample settles for five seconds and measures three seconds, with uncapped FPS and V-Sync off.
The viewport was 1361 × 855 logical pixels (2722 × 1710 captured pixels).

| Route position | Main Ultra median | This change Ultra median | This change FPS |
|---|---|---|---|
| 0 m | 12.07 ms | 12.88 ms | 77.6 |
| 100 m | 11.44 ms | 12.32 ms | 81.1 |
| 450 m | 14.45 ms | 14.38 ms | 69.4 |

Medium view/shadow primitive and draw-call counts match the baseline exactly. Both Medium
trials are retained in the CSV: timings varied on both builds under background workload,
so they establish no stable speedup or slowdown. Every recorded sample stayed above 60 FPS
on this machine. These short M5 samples do **not** verify the Medium/M1 target or a long ride.

The first native review exposed whole-course façade allocation. Streaming reduced retained
façade mesh triangles from **35,083,010** to **2,574,874 / 2,578,858 / 894,656** at the three
positions, releasing far geometry as the camera moved. This is a geometry count, not a VRAM
byte measurement. The full container suite and all platform CI builds passed; M1 acceptance
remains explicit work in the roadmap.

## Reproduce

Run all authoring, building and checks in the existing containers:

```sh
scripts/art.sh blender --background --factory-startup --python art/textures/building_scans.py
scripts/art.sh blender --background --factory-startup --python art/buildings/build.py -- --detail
scripts/dev.sh cargo test --manifest-path core/Cargo.toml -p orqwa-world \
  building_detail_render_fixture -- --ignored
scripts/dev.sh sh -c '
  scripts/build-gdext.sh debug &&
  godot --headless --path app --editor --import &&
  timeout 300 xvfb-run -a -s "-screen 0 1280x720x24" \
  godot --path app --rendering-driver vulkan --audio-driver Dummy \
    --resolution 1280x720 -s res://tools/render_building_details.gd
'
```

The controlled views use production assets, materials and Rust-derived shell details, with
matching cameras and light. Refresh the ordinary model gallery using `GROUPS=buildings` and
`GALLERY=docs/images/models` with `scripts/render-models.sh`; `BUILDING_DETAIL=1` selects the
nearby Ultra variant. `RENDER_FRAMES` can shorten static captures with wind disabled.
Pair the fixtures with actual course views, including a town, a regional house, distance
transitions, Low and Winter. The application smoke test covers preset gating, View distance
overrides, loaded-course refresh and original instance-data retention.
