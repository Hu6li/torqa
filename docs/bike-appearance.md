# Road bike appearance and rider fit (R227–R232)

Both rider sizes use coherent road-frame dimensions before the rider is posed. The saddle
height follows the rider's legs; the torso and arms are fitted to the frame's fixed hoods.
Both saddles have a 20 mm forward rail adjustment. The hip-fit line moves with them, and
height is solved again for the same knee extension before fitting the torso and arms.
Painted frame tubes are welded and rounded before export, and the fork is joined afterward
so its crown and lower headset remain separate parts of the steering assembly.

Paint uses a non-metallic clear coat, carbon rims/cockpit/seat post use satin reflection,
tyre tread and casing remain rough, and steel and anodized hardware use metallic reflection.
The palette retains rider and group accent colours. The bike has thinner crossed spokes,
valve stems, vented rotors, open cassette sprockets, a continuous chain with separate side
plates, an open derailleur cage and a shaped bottle in its cage. Both saddles have compact
21 cm shells, shortened from 26 cm, and matching shorter rails. No third-party asset or
texture is added.

## Frame proportions

`art/riders/bike_fit.py` uses the published
[Aethos 49/54 geometry](https://www.specialized.com/us/en/s-works-aethos-sram-red-etap-axs/p/216931)
as a dimensional reference for the unbranded small/medium frames. Each has a round headset,
a 6° rising stem, compact drops and upright shifter horns. Fork offset is perpendicular to
the steering axis. The frame's dimensions determine clearance and contact points together.

| Dimension | Small (female) | Medium (male) |
|---|---:|---:|
| Stack | 514 mm | 544 mm |
| Reach | 375 mm | 384 mm |
| Head tube | 109 mm | 137 mm |
| Stem | 80 mm | 100 mm |
| Bar width | 380 mm | 420 mm |
| Built wheelbase | 975 mm | 979 mm |

The bar has a 75 mm reach and 125 mm drop. The tyres remain 28 mm, on lighter 35 mm carbon
rims. These proportions replace the cramped cockpit and its steep descending stem. The
frame, hands, hips and hubs are exported with a consistent fit manifest; animation node and
bone names are retained. Reference photographs are not bundled.

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

## Standing posture

The standing pose moves the hips 12 cm forward of the seated fit, leaving the saddle behind
the rider. This puts the hips 6 cm farther back than the previous standing pose. Torso pitch
is now 2° more upright than seated, about 10° more upright than the previous standing pose.
Pitch remains independent of hip lift, with arm IK keeping the hood grips and a bend at
the elbows. Both arm and leg reach constrain how far back and upright the rider can stand.

Hip rise is solved over the complete crank cycle with a 5.5 cm ceiling. The per-frame reach
limit trims a small bob near the bottom of the stroke, preserving the forward shift and
torso angle. An extra 7° toe-down replaces the previous 23°: excessive foot rotation lowered
the ankle again, reducing available lift. Counter-roll still balances the rocking bike;
the existing standing spring, effort thresholds and hysteresis drive the transition.

| Previous standing pose | Revised standing pose |
|---|---|
| ![Female previous standing](images/bike/female-standing-before.jpg) | ![Female revised standing](images/bike/female-standing-after.jpg) |
| ![Male previous standing](images/bike/male-standing-before.jpg) | ![Male revised standing](images/bike/male-standing-after.jpg) |
| ![Female previous chase view](images/bike/female-standing-chase-before.jpg) | ![Female revised chase view](images/bike/female-standing-chase-after.jpg) |
| ![Male previous chase view](images/bike/male-standing-chase-before.jpg) | ![Male revised chase view](images/bike/male-standing-chase-after.jpg) |

Pose measurements at 5° crank intervals, including bike rock, give these hip movements
relative to seated. The arm and leg columns are the smallest remaining reach across both
limbs over the cycle; hood grips stay fixed to numerical precision.

| Rider | Forward | Hip rise | Body bob | Leg reach remaining | Arm reach remaining |
|---|---:|---:|---:|---:|---:|
| Female | 120 mm | 31–45 mm | 14 mm | 2.0 mm | 23 mm |
| Male | 120 mm | 37–53 mm | 16 mm | 2.0 mm | 15 mm |

The affected gallery images are refreshed and reviewed for both riders: four quarter-turn
crank positions, a halfway transition and the chase view. Matching seated views retain their
previous appearance. A course chase capture near 10 km on Twann–Chasseral's 9% climb, with
the fake trainer at 350 W and 60 rpm, also reaches full standing through `RiderMotion`.
These are visual reviews and joint measurements; no tests were run locally for this revision.

![Upright standing posture in the course chase camera](images/bike/standing-route-chase.jpg)

## Asset and rendering contract

- `art/riders/build.py` remains the source, with both generated GLBs committed. Only painted
  main-frame faces enter the 3 mm voxel weld and smoothing; their surface is reduced to 1,800
  triangles before rejoining the fork and hardware.
- The six bike nodes, skeleton and fit-manifest fields stay stable. The updated hips, hoods,
  hubs and rest pose are rebuilt together. The chain
  stays in the frame mesh; it adds no animated link nodes. Course generation and stored-world
  formats are unchanged.
- `RiderAvatar` styles the new `sidewall`, `carbon`, `alloy` and `bottle` materials from the
  `bike` palette alongside the existing material names. A frame still receives its rider or
  group accent; ghost transparency and body hiding use the existing presentation path.
- The complete bike is 18,036 triangles, compared with about 9.8k previously. The whole
  rider and bike grows by about 16%. In the fixed side view, visible draw calls rise from 19
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
scripts/dev.sh sh -c 'VIEWS="standing standing-down standing-back standing-up standing-rise standing-chase" GALLERY=docs/images/models scripts/render-riders.sh'
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

## Review of reference-sized frames

Both models and their fit metadata are rebuilt around the reference frames, retaining the
current helmet geometry. All 34 gallery views and the three saved-course views are refreshed
and visually reviewed, including the cockpit, frame clearance, saddles, seated reach and
standing poses. Automated tests were not rerun for these revisions.
