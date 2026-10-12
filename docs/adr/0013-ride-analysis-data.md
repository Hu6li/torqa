# ADR 0013 — Ride analysis data beside each ride

- Status: accepted
- Date: 2026-10-12

## Context

The Analysis tab (R60) looks at a rider's power across all their rides. The first analysis,
the power curve (R61), needs each ride's best power over every duration, and later ones need
more per ride (time in zones, #217). Computing it from the FIT files every time means decoding
every ride whenever the tab opens, which is too slow for a season of rides.

Each ride already has a JSON summary beside its FIT file (ADR 0002). That summary has a
`format` number, and a Torqa that finds a newer number treats the summary as missing and
rebuilds it from the FIT file. A rebuild loses what only the summary knows: the ride's name,
its course and its climb times. In a data directory synced between two machines with
different Torqa versions, bumping the format would make the older version erase them.

Options considered:

- **Compute from the FIT files on demand**: no stored data, but slow, and slower each week.
- **A cache outside the data directory** (the SQLite index of ADR 0002): fast, but a second
  store to keep in step and a new dependency; each machine would compute everything again.
- **Store it in the ride's JSON summary under a new format number**: simple, but older
  versions would rebuild the summaries and lose names and times.
- **Store it in the JSON summary as an optional `analysis` object with its own version.**

## Decision

- Each ride's JSON summary gets an optional **`analysis`** object holding what the analysis
  across rides keeps of the ride, with its own **`version`** (`ANALYSIS_VERSION`, now 1). The
  summary's `format` stays 2, so older versions still read the file and ignore the object.
- Later versions of the object **only add fields**, so a newer object still reads as an older
  one. An object older than the running version, or none, is computed again from the FIT file
  the next time the history is read, and saved with every other field kept.
- Facts that cannot be recomputed live beside it, outside the object: the rider's weight on
  the ride (`rider_mass_kg`, for W/kg) and whether the ride was simulated (`simulated`).
- **Version 1** holds the power curve, `power_curve_w`: the best average power in watts, to a
  tenth of a watt, over 36 durations spaced evenly on a logarithmic scale from 1 s to 8 h,
  keyed by seconds. It includes 5 s, 1, 5, 20 and 60 min, the durations whose bests are records.
- Power is put on a **1 s grid** before the curve is computed, so activities recorded at
  another rate or only on changes give the same curve: samples within a second are averaged,
  and gaps of up to 10 s are held at the power before them. A longer gap is a pause, and no
  effort spans it. Seconds without power count as 0 W, as they do for normalized power.

## Consequences

- The Analysis tab and the records read the summaries only, as the history does.
- After an update, the first read of the history goes through every older ride's FIT file
  once; the summaries then carry the analysis to other machines through the sync.
- An older Torqa that saves a summary (e.g. renaming a ride) drops fields it does not know; a
  newer one computes them again on its next read.
- Rides saved before Torqa kept the rider's weight use today's weight for W/kg.
