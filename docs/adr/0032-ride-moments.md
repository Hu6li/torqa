# ADR 0032 — One channel for the moments of a ride

- Status: accepted
- Date: 2026-10-24
- Requirements: R124, R125, R27, R42, R113

## Context

Riding showed only figures. A climb's top and the finish were short toasts ("Climb done in
14:02 — new record, 0:12 faster!") built in the ride screen from two dedicated events,
`ClimbCompleted` and `RouteCompleted`. Issue #185 asks for more things worth telling the rider:
milestones, the first ride on a course, power peaks, records, habits. A coach (#186) may later
want to speak during the ride as well. Each new kind as its own event and signal would spread
the wording across the ride screen.

## Decision

- The core decides **what happened**, the front end **how it is said**. Detection lives in
  `orqwa-app::moments` and is reported as `AppEvent::Moment(Moment)`, carrying only figures in
  SI units and indexes. `ClimbCompleted` and `RouteCompleted` are gone. A climb is a record
  only when faster than an earlier real time; simulated rides set none.
- Godot has one `moment(info)` signal. `MomentLines` holds every line in one table, two or
  more casual variants per moment and one plain line, so the wording can be refreshed without
  touching detection. Lines are written for people: full sentences, no slang that ages.
- The rider's **voice** (Casual, Plain, Off) is part of the profile, as the map style is,
  because riders sharing an installation may want different ones.
- The ride screen shows moments on a `MomentCard`, one at a time; the overlay shows the line
  in its bar. The finish is not a card: the quick summary (R113) opens over the finished ride
  at once and tells it there.
- Milestones follow the rider's units: 25 km and 1 km to go, or 25 miles and 1 mile.
- Power records are found live against the bests of the rider's history, loaded at the
  start, and told once the effort tops out rather than every second it keeps rising. A rider
  without records gets none mid-ride: every effort would be one.
- The summaries say at most one line, and only what their figures and record badges do not:
  the first ride on the course, or the rider's week. Repeating the distance or a record in a
  sentence above the same figure made the summary busy. The line looks back on the ride: the
  first ride's "enjoy the new roads" belongs to its start, not its end.
- That line is worked out from the history when a summary opens, not stored with the ride: it
  compares against the rides before it, so it needs no migration and stays right after rides
  are deleted. The casual line is picked by the ride's path, so a summary says the same each
  time.

- The chime is a presentation sound, played by Godot rather than mixed into the core's
  ambient stream, which only runs on 3D course rides (R114); moments come on every kind of
  ride. It is synthesised when the app starts, so no recording or licence is added.

## Consequences

- A future source of messages, such as a coach, reuses the card and the voice.
- With the voice off, the climb toasts riders had before are gone too; the climb panel still
  shows the time and the best.

## Amendment — 2026-11-04: power record cooldown (#482, R213)

A flat or lower rolling power average ends a peak. Small increases after that can therefore
produce repeated notifications for the same duration throughout one sustained effort.

After emitting a power record, `PowerPeaks` suppresses that duration for five minutes of
recorded riding time. Sample timestamps supply this clock, so pauses do not consume the
cooldown and processing several samples in one frame gives the same result. Each duration
has its own deadline, reset with the detector at the start of a ride.

During the cooldown the detector continues raising that duration's best, without retaining
a pending peak. Suppressed records are never queued for later: once the deadline passes,
another notification needs a new improvement beyond the highest power reached so far,
including the silent improvements. The recorded samples still supply the full power curve
and final record badges. Detection stays in Rust, so cards, overlay lines and chimes all
follow the same cooldown.
