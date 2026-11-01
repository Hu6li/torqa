# ADR 0035 — Regional building archetypes

- Status: accepted
- Date: 2026-11-01

## Context

Climate already selects tropical houses, but temperate Japanese rides still receive alpine
chalets, Bernese farmhouses and European castles. Latitude and elevation describe climate,
not regional architecture. Saved courses must remain reproducible offline (R101).

## Decision

Select architecture per building from bundled region polygons, independently of climate.
The first set is Japan (R164), using the same pinned, ISC-licensed country-coder source
already bundled for traffic-territory detection. No provider request or course-format
change is needed. Other regions retain the existing selection.

Temperate Japanese houses use six timber-and-plaster models, with one or two storeys and
dark ceramic roofs. Rural houses at elevation stay houses instead of becoming Swiss
chalets or farmhouses. Tropical lowlands, including Okinawa and Ishigaki, retain their
existing tropical house set. Offices, apartments, public buildings and industrial buildings
keep their existing classification; this change does not turn a whole city into an old town.

Mapped castles in Japan use three tiered keep models on stone bases. Castles are still
placed only from the existing castle points of interest. Distant shells and footprints no
model fits use tiered roofs too, with the mapped irregular outline retained by the platform.
Shells simplify the authored models' swept eaves, lattice and stone batter.

## Consequences

Generator 37 offers regeneration of older kept worlds; stored format 3 stays unchanged.
Existing model names and instance buffers remain valid. Regional selection is inferred;
these are scenery archetypes, not surveyed facades or reconstructions of named castles.
China, Korea and other Asian regions need distinct models and classification in later steps.

See [regional buildings](../regional-buildings.md) for assets, references and validation.
