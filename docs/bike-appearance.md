# Road bike appearance (R227–R228)

Both rider sizes use the same scripted modern road-bike construction, fitted to their
existing saddle, pedal and hand positions. The frame's painted tubes are welded and rounded
before export, removing the exposed intersections of the earlier separate tubes.

Paint uses a non-metallic clear coat, carbon rims/cockpit/seat post use satin reflection,
tyre tread and casing remain rough, and steel and anodized hardware use metallic reflection.
The palette retains rider and group accent colours. The bike has thinner crossed spokes,
valve stems, vented rotors, open cassette sprockets, a continuous chain with separate side
plates, an open derailleur cage and a shaped bottle in its cage. Both saddles have compact
21 cm shells, shortened from 26 cm, and matching shorter rails. No third-party asset or
texture is added.

The fork crown keeps its axle-to-crown height independently of handlebar fit. On the female
bike, the head tube rises to its minimum 80 mm length and the stem descends to the existing
bar position. This clears the tyre from the crown and down tube while preserving the wheel
hubs, rider contact points and animation fit.

## Matched visual review

The original and revised assets below use identical cameras, atmospheric sky, midday energy,
AgX tone mapping and FXAA. The rider review tool previously used the retired gradient sky
and linear tone mapping; its lighting now follows the riding view's material response.

| Original | Revised |
|---|---|
| ![Original bike](images/bike/side-before.jpg) | ![Revised bike](images/bike/side-after.jpg) |
| ![Original drivetrain](images/bike/drive-before.jpg) | ![Revised drivetrain](images/bike/drive-after.jpg) |

The [rider gallery](../art/README.md#riders) includes both rider fits seated, standing,
leaning, from the chase camera and in bike-only side, drivetrain, front, cockpit, wheel
clearance and saddle views. The rebased models retain main's current road-racing helmets.

| Female frame clearance | Male frame clearance |
|---|---|
| ![Female tyre clears the frame](images/bike/female-clearance.jpg) | ![Male tyre clears the frame](images/bike/male-clearance.jpg) |

| Female saddle | Male saddle |
|---|---|
| ![Short female saddle](images/bike/female-saddle.jpg) | ![Short male saddle](images/bike/male-saddle.jpg) |

The bundled Twann–Chasseral course at 4,200 m, built from its saved inputs:

| Chase | First person |
|---|---|
| ![Bike in the chase camera](images/bike/route-chase.jpg) | ![First-person riding view](images/bike/route-first-person.jpg) |

![Bike from beside the road](images/bike/route-side.jpg)

## Asset and rendering contract

- `art/riders/build.py` remains the source, with both generated GLBs committed. Only painted
  frame faces enter the 3 mm voxel weld and smoothing; their surface is reduced to 1,800
  triangles before rejoining the hardware. Small moving parts retain their original origins.
- The six bike nodes, skeleton, fit manifest and animation interface stay stable. The chain
  stays in the frame mesh; it adds no animated link nodes. Course generation and stored-world
  formats are unchanged.
- `RiderAvatar` styles the new `sidewall`, `carbon`, `alloy` and `bottle` materials from the
  `bike` palette alongside the existing material names. A frame still receives its rider or
  group accent; ghost transparency and body hiding use the existing presentation path.
- The complete bike is about 16k triangles, compared with about 9.8k previously. The whole
  rider and bike grows by about 12%. In the fixed side view, visible draw calls rise from 19
  to 24 for the bike alone and from 29 to 34 with a rider. These are software Vulkan counts,
  not native GPU frame-rate measurements. The 60 fps Medium target on a base M1 still needs
  measurement on that hardware.

## Reproduction

Run in the art/dev containers, with renders and checks in separate runs:

```sh
scripts/art.sh blender --background --factory-startup --python art/riders/build.py
scripts/dev.sh sh -c 'GALLERY=docs/images/models scripts/render-riders.sh'
scripts/dev.sh sh -c 'VIEWS="bike-side bike-drive bike-front bike-cockpit" scripts/render-riders.sh'
scripts/dev.sh sh -c 'VIEWS="bike-clearance bike-saddle" scripts/render-riders.sh'
scripts/dev.sh sh -c 'COURSE=core/orqwa-gd/courses/twann-chasseral.oqc SHOTS="bike-chase:4200:0 bike-first-person:4200:1 bike-road-side:4200:0:3,1.2,1" scripts/render-views.sh'
scripts/dev.sh scripts/check.sh
make wiki-check
```

The runner prints visible draw calls and primitives for each view. Review fork/tyre
clearance, frame openings, casing/tread transitions, cockpit surfaces, drivetrain continuity
and seated/standing contact points. The existing `rider_smoke.gd` checks full-turn leg reach,
standing motion, hand placement, coasting and first-person body hiding.

## Initial validation (2026-11-07, before the rebase and fit corrections)

- Repeated Blender builds produced byte-identical GLBs and fit metadata. The fit manifest is
  unchanged from the original assets.
- The initial 26 rider/bike views and three saved-course views were reviewed.
- A runtime compatibility check verified every imported material's rider/group colour and
  ghost transparency for both rider sizes.
- `scripts/check.sh` passed: Rust formatting, Clippy, workspace tests, dependency checks,
  GDScript lint/format, and all Godot smoke/render checks, including rider animation.
- `make wiki-check` passed.

## Review after rebase and fit corrections

Both models and all 34 gallery views were rebuilt from the merged scripts and visually
reviewed, retaining main's current helmet geometry. Wheel-clearance and saddle close-ups
accompany the seated and standing views. The three saved-course views were refreshed and
reviewed too. The fit manifest remains unchanged from main. Automated tests were not rerun
for these revisions.
