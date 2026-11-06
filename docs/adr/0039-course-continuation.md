# ADR 0039 — Opt-in solo course continuation

- Status: accepted
- Date: 2026-11-06

## Context

R220 (#497) lets riders split a long solo course across days. Each day must remain an
independent measured activity. Starting positions in group rides already distinguish
absolute course position from distance ridden since local entry.

## Decision

- Store one JSON point per rider and stable route key under
  `profiles/<id>/continuations/<route-key>.json`, using atomic replacement. Shared course
  files and kept worlds remain unchanged. The point includes its distance, course length
  and a deterministic fingerprint of the prepared route's coordinates, elevation and
  distance samples, quantized to centimetres to tolerate platform rounding. Revalidate on
  opening/continuing; changed geometry invalidates it.
- Ask over the paused course when Finish & save is pressed. Only explicit consent changes
  an unfinished point, after the FIT save succeeds. Saving a genuine solo finish clears
  it. Discarding that activity from its quick summary restores the prior point, unless a
  deliberate reset or a synced change has superseded it.
- Use the session's entry-distance boundary for solo continuation. Begin stationary with
  empty samples, zero personal time and zero recorded distance. Initialize milestones and
  climb timing at entry. Existing record filtering excludes skipped starts and whole-course
  speed records while retaining later complete climbs and actual power efforts.
- Video playback seeks through the existing distance-to-time mapping, including manually
  aligned and unlocated video courses. Ghost timelines start where their traces pass the
  saved position. A ghost without a remaining trace is omitted.
- Workouts, group entry and simulated rides retain their separate start behavior. Eligibility
  is captured when the activity starts, so leaving a group cannot turn it into a solo
  continuation activity.

## Consequences

The gallery's In progress filter (R221) uses the same active-rider point lookup as its badge.
It combines with existing filters in Rust; toggling or clearing it changes only the query.

Continuation points sync with rider data and survive renaming or moving course files.
Prepared geometry changes can require starting over. There is no service, dependency,
course-format or world-format change. Each activity remains a separate FIT/history entry.
