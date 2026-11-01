# ADR 0035 — Regional building archetypes

- Status: accepted
- Date: 2026-11-01

## Context

Climate alone cannot select regional architecture. Asian and Nordic rides can receive
buildings unlike their local traditions. Saved courses must remain reproducible offline (R101).

## Decision

Select architecture per building from bundled country polygons, independently of climate.
Asia uses country-coder polygons carrying UN M49 region code 142. Nordic selection uses the
five Nordic countries plus Faroe Islands, Åland, Svalbard and Jan Mayen; it is not a broad
Northern Europe grouping. Both sets use the same pinned, ISC-licensed country-coder source
already bundled for traffic-territory detection. No provider request or course-format change
is needed.

Asian homes and mapped castles use the East Asian starter archetypes, with varied pastel,
earth, green and blue colours. Nordic homes use painted timber, pale trim, steep gables and
varied red, ochre, cream, green and blue colours. Nordic mapped castles use stone-based keeps
with four corner towers and Nordic colours. Other mapped castles retain the general European
models. Tropical lowlands retain their existing house set. Offices, apartments, public
buildings and industrial buildings keep their existing classification; this does not turn a
whole city into an old town. Castles are still placed only from existing castle points of
interest. Footprints no model fits and distant castles use shells.

## Consequences

Generator 38 offers regeneration of older kept worlds; stored format 3 stays unchanged.
Existing model names and instance buffers remain valid. Regional selection is inferred;
these are scenery archetypes, not surveys of individual buildings or replicas of named
monuments. More locally distinct architecture across both regions remains future work.

See [regional buildings](../regional-buildings.md) for assets, references and validation.
