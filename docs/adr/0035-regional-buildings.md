# ADR 0035 — Regional building archetypes

- Status: accepted
- Date: 2026-11-01

## Context

Climate already selects tropical houses, but Asian rides can receive alpine chalets, Bernese
farmhouses and European castles. Latitude and elevation describe climate, not regional
architecture. Saved courses must remain reproducible offline (R101).

## Decision

Select architecture per building from bundled region polygons, independently of climate.
The Asian classifier uses the country-coder polygons carrying UN M49 region code 142 from
the same pinned, ISC-licensed source already bundled for traffic-territory detection. The
starter models are own-work East Asian archetypes with Japanese architectural references;
the Asian regional set also uses varied pastel, earth, green and blue wall and roof colours.
No provider request or course-format change is needed.

Asian rural houses remain houses rather than becoming Swiss chalets or farmhouses. Tropical
lowlands retain their existing tropical house set. Offices, apartments, public buildings
and industrial buildings keep their existing classification; this does not turn a whole city
into an old town. Mapped castles in Asia use three tiered keeps on stone bases. Castles are
still placed only from the existing castle points of interest. Distant shells and footprints
no model fits use tiered roofs too, with the mapped irregular outline retained by the platform.
Shells simplify the authored models' swept eaves, lattice and stone batter.

## Consequences

Generator 37 offers regeneration of older kept worlds; stored format 3 stays unchanged.
Existing model names and instance buffers remain valid. Regional selection is inferred;
these are scenery archetypes, not surveyed facades or reconstructions of named castles.
More locally distinct regional architecture remains future work.

See [regional buildings](../regional-buildings.md) for assets, references and validation.
