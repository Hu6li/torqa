# ADR 0033 — Map detail from Geofabrik region downloads

- Status: accepted (all four steps: the reader and the extractor; surfaces in prepared
  courses; the setting and the downloads; roundabouts)
- Date: 2026-10-25
- Requirements: R142, R143, R119, R126, R3, R32, R101

## Context

The OpenFreeMap tiles (ADR 0008) follow the `OpenMapTiles` schema, which reduces OpenStreetMap's
`surface` tag to `paved` or `unpaved` and leaves out `junction=roundabout`. The 1,147 zoom-14
tiles of thirteen prepared courses held no other surface value. Cobbled streets are drawn as
asphalt with lane markings (ADR 0031), and which way round a roundabout goes falls back on the
order its pieces are drawn in (R126), which is wrong where the tiles merge a ring into a longer
line or reverse it (Erlach, Bielersee course, about 20.3 km in).

The data is in OpenStreetMap. Getting it per course was weighed against each server's policy:

- **OpenStreetMap's API** is for editing: "not for read-only purposes or projects".
- **Public Overpass** lists "an app for more than just OSM mappers relying on the public
  instances as backend" as problematic; ADR 0008 measured its failures for bulk downloads.
- **An extract or Overpass server of ORQWA's own** needs a machine for the planet file
  (95.5 GB in October 2026) or an always-on database, and hosting.
- **Geofabrik's region downloads** (`download.geofabrik.de`) are free, updated daily, and cut
  into countries and, for large ones, their parts (Bavaria 856 MB of Germany's 4.9 GB). A rider
  can download the regions they ride in, once.

## Decision

- **Opt-in and manual.** Off by default; a rider turns it on in Settings → Courses and chooses
  regions from Geofabrik's tree (countries and their parts; no continents). ORQWA downloads a
  region only when asked: never on import, course preparation, opening, ride start or world
  regeneration, so R119's rule stands.
- **The download stays in the cache** (`osm/geofabrik/`), listed with its size to update or
  delete, so a later extractor reads it again without another download.
- **Own reader** (`orqwa-osm::detail`) on `prost` and `flate2`, already in use under
  `mvt-reader` and `zip`: no new crates. It declares only the fields it reads, so a pass over
  the ways skips the nodes unread, and unpacks the blobs on every core.
- **Only what the tiles lack is kept**, in `osm/detail-v1/<region>-<date of the data>/12/<x>/<y>.json`:
  - surfaces the tiles cannot tell — concrete, paving, cobbles, dirt, grass — of roads the
    tiles keep (the same class and bicycle rules), with their kind and line;
  - roundabouts (`junction=roundabout|circular`), drawn the way traffic goes round.
  A way is whole in every zoom-12 cell it passes; its id keeps it once across cells and
  regions. The date makes each path stand for one content, as course files expect (ADR 0007).
  `detail-v1` is raised when an extract's content changes.
- **Only courses prepared afterwards use it** (superseded below: library courses take it
  when opened, R144). They carry the cells they used, as they carry their tiles, and name the
  extracts in their manifest (ADR 0007, amendment of 2026-10-25). Video courses ride their
  video and use none.
- **A region is ready** when its record, `osm/geofabrik/<id>.json` (`id`, `name`, `extract`,
  `data_time`), names an extract that is there; installing a newer download of it removes the
  older extract.
- **Roads take the surfaces** of the mapped ways along them after the route is matched (ADR
  0031, amendment of 2026-10-25): route matching sees the tiles' roads unchanged.
- **Downloads** (R142): the rider's list comes from Geofabrik's `index-v1-nogeom.json`, kept in
  the cache to name regions offline; continents are only headings. A region's `-latest`
  address leads to its dated file, whose size a `HEAD` request tells before the download and
  whose name the record keeps as `source`: asking again with that name unchanged is "up to
  date". A download is written beside its final name with the dated name next to it, and a
  later attempt at the same file goes on with an HTTP range; only a whole one replaces the
  earlier download. Reading stops between blobs when asked; the earlier extract stays ready.
  One region at a time. Downloads in the cache without a ready record — put there by hand, or
  stopped while being read — are prepared when the setting is turned on and at start-up.
  Turning the setting off asks first, then deletes every region.
- **Roundabouts** (R143): before the route is matched, each piece of road along a mapped ring
  (same kind, within 4 m, aligned) is marked as going round it and turned where the tiles draw
  it against the traffic; a road the tiles join to a ring is split where the ring begins or
  ends. Route matching takes marked pieces as rings, one way as drawn, with the closing join
  of a ring drawn as one closed way and its length counted as a lap; R126's shape test still
  finds the rings the detail does not mark.
- Four steps, each a PR: the reader and extractor; surfaces in prepared courses; the setting,
  downloads and their list; roundabouts.

## Measurements

`orqwa-cli detail <download> <out>` in the dev container (18 cores):

| Region | Download | Time | Peak memory | Extract | Cells | Surfaces | Roundabout ways |
|---|---|---|---|---|---|---|---|
| Switzerland | 549 MB | 7 s | 78 MB | 36.7 MB | 998 | 83,443 | 5,406 |
| Germany | 4.87 GB | 72 s | 807 MB | 520 MB | 10,010 | 2,325,003 | 37,823 |

Of Switzerland's surfaces, 41,503 are dirt, 15,215 grass, 11,944 concrete, 9,252 paving and
5,529 cobbles; Bern's old town has 51 cobbled ways. Most are paths and tracks: a cell holds
37–52 KB on average, so a 100 km course carries a few MB of it before its file is packed.
Memory grows with the region's matching ways; Germany, among the largest downloads, stays
under 1 GB. Coordinates stored as differences would make extracts smaller if that matters.

Roundabouts on three prepared courses, checked with
`ORQWA_COURSE=<course> cargo test -p orqwa-app -- --ignored roundabouts_of_a_course`
(metres ridden against the traffic round a mapped ring, without the detail → with it):

| Course | Mapped ring pieces | Without | With |
|---|---|---|---|
| Bielersee | 29 | 9.3 m at Erlach | none |
| Bielersee–Obedüre | 44 | 9.7 m at Erlach | none |
| Alpenbrevet Gold | 19 | 30.1 m and 4.0 m near Biasca | none |

Marking rings one-way showed a matching fault both kinds of ring share: points of a file
beside a ring past its way out, matched onto it, took the line round again to reach them
(Erlach 603°, two rings near Biasca 1245° and 435°). Generator 35 leaves such points out, so
the line takes the way out the first time it passes it (R145): Erlach 240°, Biasca 147° and
50°. The check above also measures how far round each ring the route turns per pass.

The check counts a step of the route only if its middle lies on the ring too: a route that
cuts straight across a small ring, as at a mini-roundabout in Biel whose exit the tiles leave
1.4 m from the ring's nearest point, goes round none of it.

## Consequences

- A new server, `download.geofabrik.de`, is in the wiki's list of servers ORQWA contacts: only
  when the rider lists, adds or updates a region in the settings.
- Downloads take room: 0.5–5 GB for a country, beside an extract of tens to hundreds of MB.
- The data stays © OpenStreetMap contributors (ODbL 1.0); the cells a course carries share it
  under the same licence, credited as its tiles are.

## Amendment 2026-10-25 — courses opened take the detail (R144)

Riders expected a course already in their library to look right once they downloaded its
region; deleting and importing it again was the only way, and importing the same file opens
the course already there. Now a library course opened reads the extracts of the regions ready
on this computer, in place of an older extract of the same region it carries, and keeps those
it carries of regions not downloaded here (`detail::for_course`). Where that changes what it
carries, the opening brings its file up to date before the route is reported: the manifest's
`map_detail`, length, climbing and previews from the route matched with the detail, and the
cells under `data/osm/detail-v1/` in place of the old (`course::replace_data`; the rest of
the file is copied unchanged). The file's fingerprint changes with it, so its kept world is
built again on the next ride (R101) and the old one is tidied away. The route key comes from
the track as recorded, so the rider's records on the course stay its own. A course opened
from outside the library, or with no region ready, stays as it was; a video course keeps
what it carries.

## Amendment 2026-11-18 — buildings, and versions of what extracts hold

Extracts also keep what buildings are, as a point and a kind each (ADR 0041, step 5).
Changing what they hold no longer raises `detail-v1`: courses carry their cells under it, and
one opened elsewhere must still find them there. Each region's record says what its extract
holds (`content`); a region whose extract holds less than `CONTENT` is not ready, and its
download is prepared again at start-up, as one stopped while being read. Its extract's name
ends in what it holds (`-c2`), so courses carrying the extract from before take the new one
when opened (R144). The Switzerland
extract grows from 36.9 MB to 69.0 MB, and preparing DACH takes 1.8 GB at its peak, up from
0.9 GB; see ADR 0041.

