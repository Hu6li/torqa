# Sample course

`twann-chasseral.oqc` is the course a new library starts with (R95, ADR 0007): **Twann –
Chasseral**, 20.4 km and 1,190 m of climbing from Lake Biel up to the Chasseral, Switzerland.
The GDExtension carries it (`include_bytes!` in `src/lib.rs`), so only the app ships it, and
writes it into the library on a first start.

It is an ordinary course file, prepared in ORQWA 0.3.0 from its `route.gpx`: the track and its
heights only, without the planner's metadata, timestamps and waypoints. It holds the terrain
and map tiles of the route: © OpenFreeMap © OpenMapTiles, data © OpenStreetMap contributors
(ODbL 1.0); terrain by Mapterhorn (CC BY 4.0) and AWS Terrain Tiles (see
[CREDITS](../../../docs/CREDITS.md)).

To prepare it again, e.g. for a new course format: take its route
(`unzip -p twann-chasseral.oqc route.gpx > twann-chasseral.gpx`), delete the sample in ORQWA,
import the GPX online as "Twann – Chasseral" and copy the new file from the library folder over
this one. `the_sample_course_rides_on_its_own_data` checks that it opens and builds its world
with an empty cache.
