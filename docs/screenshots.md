# Screenshots

Current app screens and generated scenery, captured from the application with a simulated
trainer and sample ride history. Open an image for its full size. The phone-remote browser
screens remain in the [remote guide](https://github.com/ORQWA/ORQWA/wiki/Devices-and-riding#remote).

## Showcase

Saved courses rendered on a GPU at the Ultra preset, in different seasons, weather and times of
day, alone and in group rides (`scripts/render-showcase.sh`, see
[Refreshing screenshots](development.md#refreshing-screenshots)). Map data © OpenStreetMap
contributors (ODbL); terrain from Mapterhorn (CC BY 4.0) and AWS Terrain Tiles. Full-size
pictures are rendered locally rather than kept in the repository.
Riders wear winter kit in autumn and winter. Some shots show the dark ride map, with the elevation
profile plain or coloured by gradient, as chosen in the rider's profile.

### Saimaa

| | |
|---|---|
| ![A group on the Saimaa lake ridge at sunset](images/showcase/saimaa-ridge-group-evening.jpg) | ![Riding the Saimaa ridge in winter](images/showcase/saimaa-ridge-winter-group.jpg) |
| ![The Saimaa ridge in the evening, with the HUD](images/showcase/saimaa-ridge-evening.jpg) | ![Rain on the Saimaa ridge in a group ride](images/showcase/saimaa-ridge-rain-group.jpg) |
| ![The Saimaa ridge in winter from above](images/showcase/saimaa-ridge-winter.jpg) | ![The Saimaa ridge from the side](images/showcase/saimaa-ridge-side.jpg) |

### Fuji

| | |
|---|---|
| ![Cherry blossom along a lake below Mount Fuji](images/showcase/fuji-lake-spring-group.jpg) | ![Evening sun over the lake below Mount Fuji](images/showcase/fuji-lake-evening.jpg) |
| ![Spring by the lake, with the HUD](images/showcase/fuji-lake-spring.jpg) | ![Friends riding the lake road in spring](images/showcase/fuji-lake-spring-riders.jpg) |
| ![The lakeside path below Mount Fuji at evening, with the HUD](images/showcase/fuji-lakeside-evening.jpg) | |

### Gorges de la Nesque

| | |
|---|---|
| ![A group above the Nesque gorge in autumn evening light](images/showcase/nesque-gorge-autumn-group.jpg) | ![Above the Nesque gorge, with the HUD](images/showcase/nesque-gorge-autumn.jpg) |

### Alpenbrevet

| | |
|---|---|
| ![A group climbing the Furka in evening light](images/showcase/alpenbrevet-furka-group.jpg) | ![The Furka from above](images/showcase/alpenbrevet-furka-drone.jpg) |
| ![Nufenen hairpins in winter](images/showcase/alpenbrevet-nufenen-winter.jpg) | ![Nufenen on a hazy morning, with the HUD](images/showcase/alpenbrevet-nufenen-morning.jpg) |
| ![The Oberalpsee and its railway in the evening](images/showcase/alpenbrevet-oberalpsee-evening.jpg) |  |
| ![Climbing the Furka with friends, with the HUD and riders list](images/showcase/alpenbrevet-furka-group-climb.jpg) | ![The same climb with gradient-coloured elevation and the dark map](images/showcase/alpenbrevet-furka-gradient.jpg) |
| ![The same climb with the dark map](images/showcase/alpenbrevet-furka-dark.jpg) | |

### Wada Ōtarumi

| | |
|---|---|
| ![A group in an autumn hairpin](images/showcase/wada-hairpin-autumn-group.jpg) | ![Autumn forest and a view at evening](images/showcase/wada-autumn-evening.jpg) |
| ![Rain on an autumn mountain road](images/showcase/wada-autumn-rain.jpg) | ![An autumn evening climb, with the HUD](images/showcase/wada-autumn-chase.jpg) |
| ![Descending through autumn woods with friends and the dark map](images/showcase/wada-autumn-dark.jpg) | ![The same descent with gradient-coloured elevation](images/showcase/wada-autumn-gradient.jpg) |

### Grosse Scheidegg

| | |
|---|---|
| ![A group on the alpine meadows at evening](images/showcase/grosse-scheidegg-group-evening.jpg) | ![Snowy forest below the Wetterhorn](images/showcase/grosse-scheidegg-winter.jpg) |
| ![Meadow hairpins in the morning](images/showcase/grosse-scheidegg-meadow.jpg) | |
| ![A winter descent with the dark map](images/showcase/grosse-scheidegg-winter-dark.jpg) | ![The same descent with gradient-coloured elevation](images/showcase/grosse-scheidegg-winter-gradient.jpg) |

## Riding and training

| Riding | Training |
|---|---|
| ![Chase camera](images/app/ride.jpg) | ![Heart-rate workout](images/workouts/heart-rate-hold.png) |
| ![First-person camera](images/app/ride-first-person.jpg) | ![Structured workout](images/workouts/structured-ride.png) |
| ![Overview camera](images/app/ride-overview.jpg) | ![Planned workouts](images/workouts/planned.png) |
| ![Video course](images/readme/video-course.jpg) | ![Workout editor](images/workouts/editor.png) |
| ![A moment of the ride: a climb record on its card](images/app/ride-moment.png) | ![A friend's group connection dropped, on the same card in red](images/app/ride-group-away.png) |
| ![Group riders, map markers and gaps](images/groups/riders.jpg) | ![Assisted group ride summary](images/groups/assisted-summary.png) |

Video footage: “Le petit Menglonnais” by [Fred Van Gestel](https://cycling.vangestel.online/indoor/videos/france/le-petit-menglonnais/),
[CC BY-NC-SA 4.0](https://creativecommons.org/licenses/by-nc-sa/4.0/). That screenshot has the
same licence; the footage is used only in documentation.

## Group rider names

Compact charcoal nameplates match the HUD's panel styling, with semibold white names and a
small rider-colour edge and downward pointer (R217–R219). Their size and gap above the helmet
stay steady as riders approach. Long names shorten with an ellipsis; overlapping badges favor
the nearer rider.

| Close rider and long name | Group from above |
|---|---|
| ![Nameplate close to the first-person camera](images/groups/rider-names-close.jpg) | ![Compact group nameplates from the drone](images/groups/rider-names-drone.jpg) |

![Nameplates in the minimum window size](images/groups/rider-names-small.jpg)

## Group connection ping

Friends' ping appears beside their names in the roster. During a ride, it stays inside the
Group Rides dialog (R235).

| Normal window | Minimum window |
|---|---|
| ![Friends' ping in the group dialog](images/groups/window.jpg) | ![Ping at 1024 × 640](images/groups/ping-small.jpg) |

## Courses, setup and analysis

Solo 3D and video courses can be continued across days. These captures use the implemented
controls with an illustrative saved position (R220, #497).

| Saved course position | Continue another day? |
|---|---|
| ![Saved position on the map and elevation profile](images/continuation/course-detail-en.png) | ![Blue Yes and red No before leaving the course](images/continuation/save-point-en.jpg) |

| Compact course page in German | Compact save question in German |
|---|---|
| ![Equal-sized translated course actions](images/continuation/course-detail-compact-de.png) | ![Equal-sized translated question actions](images/continuation/save-point-compact-de.jpg) |

![Small In progress badge on the course map](images/continuation/courses-en.png)

Use **Filters → Progress → In progress** to find the active rider's saved courses.

| In progress filter | German at 1024 × 640 |
|---|---|
| ![Progress beside the existing course filters](images/app/courses-filters.png) | ![Compact progress filter](images/continuation/courses-filters-compact-de.png) |

Use **Type → World** for routes from world files. Their cards carry a **World** pill beside
the name and show the countries supplied by the author. Square covers keep rounded corners.

| World filter | German |
|---|---|
| ![World filter, rounded cover and country](images/worlds/library-filter-en.png) | ![Welt filter and country](images/worlds/library-filter-de.png) |

![Group ride activity previews and invitations](images/groups/lobby-en.png)

![A friend sees what the organizer is choosing](images/groups/preview-en.png)

The readiness action stays beside the rider's name, with a prompt in the existing subtitle.

| English | German |
|---|---|
| ![Mark ready beside your name](images/groups/readiness-en.png) | ![Bereit melden in the minimum window size](images/groups/readiness-de-small.png) |

The group countdown locks choices while keeping the course preview in place. The organizer's
Cancel action occupies the Start button's place and returns the connected group to the lobby.

| Ready to start | Counting down |
|---|---|
| ![Group ready to start](images/groups/countdown-ready-en.png) | ![Course preview during countdown](images/groups/countdown-en.png) |

Fallback organizers are chosen automatically before starting; the friend is asked immediately
in the lobby. The organizer can choose someone else.

| Organizer's choice | Friend's consent |
|---|---|
| ![Automatic fallback awaiting consent](images/groups/fallback-auto-lobby-en.png) | ![Consent before starting](images/groups/fallback-consent-en.png) |
| ![Solo lobby explanation in German](images/groups/fallback-alone-de-small.png) | ![Consent in a small German window](images/groups/fallback-consent-de-small.png) |

During a ride, the organizer can end the course for everyone from Group Rides.

| English | German |
|---|---|
| ![In-ride organizer controls](images/groups/organizer-window-en.png) | ![In-ride organizer controls in a small window](images/groups/organizer-window-de-small.png) |

| Before riding | After riding |
|---|---|
| | ![Quick finish summary](images/app/summary-quick.png) |
| ![Course library](images/app/courses.png) | ![Ride summary](images/app/summary.png) |
| ![Course detail](images/app/course.png) | ![Ride history](images/app/history.png) |
| ![Connected devices](images/issues/devices-readings.png) | ![Power curve and profile](images/app/analysis.png) |
| ![Rider profile](images/app/profile.png) | ![Fitness and fatigue](images/app/analysis-fitness.png) |
| ![HUD editor](images/app/hud-editor.jpg) | ![Weekly training](images/app/analysis-weekly.png) |
| ![Graphics settings](images/app/settings-graphics.png) | ![Progress over time](images/app/analysis-progress.png) |

| Calories in the HUD | Optional calorie inputs |
|---|---|
| ![Calories and Work](images/calories/hud-en.png) | ![Age advances each New Year](images/calories/age-en.png) |

| Full HUD in a small window | German |
|---|---|
| ![Both HUD columns scrolled at 1024 × 640](images/calories/hud-full-small-en.png) | ![Both HUD columns in German](images/calories/hud-full-small-de.png) |

## Cities and woodland

The same woodland view on Low and Ultra graphics.
Change quality in [Settings](https://github.com/ORQWA/ORQWA/wiki/Settings#graphics-and-full-screen).

| Low | Ultra |
|---|---|
| ![Fewer nearby trees](images/views/forest-low.jpg) | ![Dense woods drawn farther](images/views/forest-ultra.jpg) |

| City streets | Woodland |
|---|---|
| ![City paving and sidewalks](images/views/city-street.jpg) | ![Wooded hairpin](images/views/hairpin-chase.jpg) |
| ![City overview](images/views/city-overview.jpg) | ![Forest floor and ferns](images/views/forest-floor.jpg) |
| ![City bench](images/views/city-benches.jpg) | ![Hairpin overview](images/views/hairpin-drone.jpg) |

## Standard route views

The fixture routes exercise villages, climbs, junctions, bridges, tunnels and weather. Their
fixed camera positions make it possible to compare future changes. These are the current
renders; historical before/after images remain with their original reviews.

| Villages and roads | Lake and junction views |
|---|---|
| ![Village](images/views/village-chase.jpg) | ![Lakeside road](images/views/lake-chase.jpg) |
| ![Climb](images/views/climb-chase.jpg) | ![Lake overview](images/views/lake-drone.jpg) |
| ![Evening village](images/views/village-evening.jpg) | ![Rain](images/views/lake-rain.jpg) |
| ![Morning climb](images/views/climb-morning-drone.jpg) | ![Morning lake](images/views/lake-morning.jpg) |
| ![Morning valley](images/views/valley-morning.jpg) | ![Lakeside village](images/views/lakeside-village.jpg) |
| ![Shops](images/views/shops-side.jpg) | ![Building block](images/views/block-chase.jpg) |
| ![Junction](images/views/junction-chase.jpg) | ![Junction overview](images/views/junction-drone.jpg) |
| ![Railway tunnel](images/views/railway-tunnel-drone.jpg) | |

| Bern | Oberalp |
|---|---|
| ![River](images/views/river-drone.jpg) | ![Road tunnel](images/views/oberalp-tunnel-chase.jpg) |
| ![Old town](images/views/oldtown-drone.jpg) | ![Railway](images/views/oberalp-railway-chase.jpg) |
| ![Bridge](images/views/bridge-chase.jpg) | ![Mountain hairpins](images/views/oberalp-hairpins-drone.jpg) |
| ![Roundabout](images/views/roundabout-drone.jpg) | ![Alpine lake](images/views/oberalp-lake-drone.jpg) |
| ![Bridge from the side](images/views/bridge-side.jpg) | ![Tunnel portal](images/views/oberalp-portal-drone.jpg) |
| | ![Mountain village](images/views/oberalp-village-chase.jpg) |

Every building, plant, roadside prop and rider also has a refreshed image in the
[model galleries](../art/README.md#gallery). See [Development](development.md#refreshing-screenshots)
for the capture and review workflow, and [Scenery reviews](scenery-reviews.md) for the before
and after images of scenery fixes.
