# Galaxy Tab S7 frame rate on Low

2026-11-16, Samsung Galaxy Tab S7 (SM-T870, Adreno 650, Android 13, 2560 × 1600), Godot 4.7.2
Mobile renderer, Low preset. Issue #542.

## Conditions

- Release builds of main 86f029f6, of the terrain fix (`perf/terrain-layers`) and of the terrain
  and sky fixes together, each as a side-by-side "ORQWA perf" APK whose main loop is
  `app/tools/measure_frames.gd` (settings from a file adb writes; GPU and CPU time per frame
  from the rendering server).
- Twann – Chasseral (bundled), chase camera, rider paused, at 2, 9 and 16 km. Three seconds per
  sample after settling. Each run started with the tablet cooled to ≤ 31 °C (AP sensor).
- Numbers are GPU milliseconds per frame. The tablet is GPU-bound (render CPU 2–7 ms), and
  Android paces frames to whole refreshes, so GPU time is the measure, not fps.
- Repeated with no screen stream on the tablet: every whole-frame figure below came out
  within 2.2 ms of the first run (main 101.9 / 89.8 / 115.2, terrain 92.9 / 73.3 / 102.2,
  terrain + sky 88.1 / 67.2 / 97.5).

## Whole frame

| GPU ms | 2 km | 9 km | 16 km |
|---|---:|---:|---:|
| main | 104.1 | 89.8 | 113.9 |
| terrain layers | 93.2 | 72.8 | 103.1 |
| terrain layers + sky passes | 88.0 | 67.3 | 97.1 |

## What each part costs (frame minus frame without it)

| GPU ms | main | terrain | terrain + sky |
|---|---:|---:|---:|
| terrain, 2 / 9 / 16 km | 23 / 42 / 33 | 13 / 26 / 23 | 18 / 33 / 28 |
| sky (flat colour instead), 2 / 9 / 16 km | 12 / 11 / 11 | 13 / 8 / 9 | 6 / 2 / 3 |
| sky lighting redrawn as clouds drift | ~1 | ~1 | 0 |
| trees, 2 / 16 km | 18 / 27 | 12 / 28 | 19 / 29 |
| small plants, 2 / 16 km | 6 / 8 | 3 / 7 | 7 / 8 |

The "without it" figures shift when another part gets cheaper (a tiler's costs overlap), so
read them as the size of each part, not as savings that add up.

## What is left

Terrain (~20–30 ms), trees (~20–30 ms), sun shadows (~15–20 ms in the earlier run), small
plants and the ~27 ms floor of a frame with no 3D world (interface, post-processing). Next:
foliage and shadows on Low.
