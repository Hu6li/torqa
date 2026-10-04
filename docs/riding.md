# Devices and riding

## Devices

Torqa remembers the trainer and heart-rate strap you used last and **reconnects them in the
background** when it starts — they appear selected in **Devices & Settings**. If one is not found
(asleep, or connected to another app), that tab says so: pedal to wake the trainer or
put on the strap, then press **Scan for devices**. Choosing another device and riding with it
makes that one the remembered device.

## During a ride

The ride screen keeps the road in view — the 3D world, or the video on video courses
([video.md](video.md)): your figures on the left, map, elevation profile, climb
and ghost panels on the right, and one **Settings** button (or key **S**).

**Settings** opens the ride settings; changes apply at once and the ride keeps going:

- **Ride**: camera (chase, first person, drone), trainer difficulty, descents ridden like flat
  roads, time of day and weather — the same options as on the course page.
- **HUD**: arrange your figures (see [hud.md](hud.md)).
- **Finish & save** ends the ride, saves it and shows its summary (name it there; see
  [history.md](history.md)).
- **Abort without saving** ends the ride after asking once; nothing is saved.

Keys: **C** switches the camera, **S** opens the settings, **M** / **.** / **,** control your
music ([audio.md](audio.md)).

On video courses the video is the view: the course page and the ride settings offer trainer
difficulty, descents and the video's **Sound** instead, and **C** does nothing.

## Simulation (fake trainer)

With the simulated trainer (*Devices & Settings*), a ride is a simulation for trying courses
out (#53):

- **Speed**: 1×, 2×, 5×, 10× or 20× in the bar at the bottom, or **+** / **−**.
- **Jump**: click the map or the elevation profile to put the rider there (the map's corner
  caption still switches between close view and whole route).
- **Free camera**: **C** past the drone view; arrow keys move, **R** / **F** rise and sink,
  **Shift** is faster, the mouse wheel sets the speed, drag with the right button to look.

A sped-up or jumped ride is saved, but counts towards no personal records.

## Graphics quality

**Devices & Settings → Graphics** sets how detailed the 3D world is drawn on this computer
(R43); it applies from the next ride:

| Preset | For | What it adds |
|---|---|---|
| Low | weaker computers | rendered at reduced resolution and sharpened (FSR), shorter view, simpler shadows and clouds, plain-coloured ground |
| Medium | MacBook with M1 (60 fps) | textured ground and asphalt, ambient occlusion, soft-edged shadows, lit clouds, haze |
| High | stronger GPUs | bounced light (SSIL), sun-sized soft shadows, light volumetric fog, 35 % more view distance |
| Ultra | fast GPUs | global illumination (SDFGI), larger shadow maps, 70 % more view distance |

If a ride stays well below 60 fps, Torqa suggests a lower preset once.
