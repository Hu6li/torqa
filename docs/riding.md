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

## The 3D world

The world is built from the map around the course (OpenStreetMap): the road you ride follows the
mapped road, other streets, tracks and paths lie beside it, and buildings stand where they are
mapped. The map rarely says what a building is, so Torqa infers it from where it stands and its
size. That gives:

- churches with a tower, and chapels with a turret on the roof;
- chalets with timber walls and deep eaves in the mountains;
- farmhouses under big roofs in the countryside;
- apartment blocks, and metal-clad halls on industrial land;
- houses and sheds everywhere else;
- shops, cafés and restaurants with a glazed front and an awning onto their street, where the map
  has one.

Mapped heights and façade colours are used where the map has them. Close to you, buildings whose
outline suits one are detailed models (made in Blender): recessed windows with shutters,
balconies with geraniums, rafters, gutters, clock towers. Further away, and for unusual outlines,
they are drawn more simply.

## Simulation (fake trainer)

With the simulated trainer (*Devices & Settings*), a ride is a simulation for trying courses
out (#53):

- **Speed**: 1×, 2×, 5×, 10× or 20× in the bar at the bottom, or **+** / **−**.
- **Jump**: click the map or the elevation profile to put the rider there (the map's corner
  caption still switches between close view and whole route).
- **Free camera**: **C** past the drone view lets the camera fly on its own (**C** again goes
  back to the chase camera); see the controls below.

A sped-up or jumped ride is saved, but counts towards no personal records.

### Free camera controls

| Keys or mouse | What it does |
|---|---|
| **↑** / **↓** / **←** / **→** | Move forward, back, left and right |
| **Shift** + **←** / **→** | Turn left and right |
| **Shift** + **↑** / **↓** | Tilt up and down |
| **R** / **F** | Rise and sink |
| **Shift** + move the mouse or trackpad, or drag with the right button | Look around |
| Mouse wheel | Fly slower or faster |

## Graphics quality

**Devices & Settings → Graphics** sets how detailed the 3D world is drawn on this computer
(R43); it applies from the next ride:

| Preset | For | What it adds |
|---|---|---|
| Low | weaker computers | rendered at reduced resolution and sharpened (FSR), shorter view, simpler shadows and clouds, plain-coloured ground, detailed buildings within 200 m |
| Medium | MacBook with M1 (60 fps) | textured ground and asphalt, grass and flowers swaying in the wind (60 m), detailed buildings within 400 m, ambient occlusion, soft-edged shadows, lit clouds, haze |
| High | stronger GPUs | bounced light (SSIL), sun-sized soft shadows, light volumetric fog, grass to 100 m with shadows, detailed buildings within 550 m, 35 % more view distance |
| Ultra | fast GPUs | global illumination (SDFGI), larger shadow maps, detailed buildings within 750 m, 70 % more view distance |

If a ride stays well below 60 fps, Torqa suggests a lower preset once.
