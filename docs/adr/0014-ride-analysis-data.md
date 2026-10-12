# ADR 0014 — Ride analysis data beside each ride

- Status: accepted
- Date: 2026-10-12

## Context

The Analysis tab (R61) looks at a rider's power across all their rides. The first analysis,
the power curve (R62), needs each ride's best power over every duration, and later ones need
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

## Amendment — 2026-10-12: histograms and days (R64)

- **Version 2** of the `analysis` object adds `power_histogram_s` and `heart_rate_histogram_s`:
  the seconds the ride spent at each whole watt and each whole beat per minute, counted like
  the summary's time in zones (each interval between samples for the value at its end). The
  weekly time in zones is counted from them with the rider's zones of today, so it matches the
  summary's and follows a changed FTP or zones without reading the FIT files. Rides analysed by
  version 1 are analysed again once.
- Fitness, fatigue and form and the weekly totals count rides by **local day**. Rust has no
  time-zone data here and no dependency is added for it: the interface passes its current
  offset from UTC, so a ride near midnight on the other side of a change to or from summer
  time may land on the day beside it.

## Amendment — 2026-10-12: the FTP estimate (R65)

- The Analysis tab offers an FTP estimated from the **best efforts of the last 90 days**, the
  window intervals.icu uses, and never applies it by itself.
- It is the **critical power of Morton's three-parameter model**, `P(t) = CP + W′ / (t − k)`,
  fitted to the best efforts of 2 to 20 minutes: for each `k` from −0.5 s to −300 s in half
  seconds, `CP` and `W′` follow from a linear least-squares fit, and the `k` with the smallest
  error is kept. No library is needed for it. As in GoldenCheetah, critical power stands for
  FTP.
- Only efforts that **look maximal** are fitted: those on the upper hull of work over time.
  A best 10 minutes from rides that never went hard for 10 minutes lies under the line between
  harder shorter and longer efforts; fitted, such efforts pulled a test rider's estimate from
  250 W down to 212 W. At least three maximal efforts are needed.
- The fit is only taken if critical power lies between 50 and 600 W, `W′` between 3 and 40 kJ
  and critical power below the best 20 minutes; otherwise, or with fewer efforts, the estimate
  is 95 % of the best 20 minutes (the 20-minute test's rule). Without 20 minutes there is none.
