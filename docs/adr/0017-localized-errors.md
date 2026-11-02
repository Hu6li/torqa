# ADR 0017 — Classified failures, translated at presentation

- Status: accepted
- Date: 2026-10-15

## Context

Core errors were English strings displayed verbatim, although the rider selected German
(#261, R74). Translating interpolated strings would require matching diagnostics from Rust
libraries, servers and operating systems, and could accidentally translate filenames.

## Decision

Carry `Failure { kind, args, details }` through asynchronous jobs and application events.
Classify known errors at their operation boundary. `Kind::message()` supplies a gettext key;
Godot translates it using the rider's language before substituting untouched arguments.
Unknown causes use a translated fallback rather than appearing as the primary explanation.
Retain the original diagnostic in `details`, `Display` and logs for troubleshooting/CLI use.

Keep the existing Godot error signals as localized summaries. Add `failure_reported` and
`last_failure()` for structured presentation. A collapsed **Technical details** control keeps
the latest 20 failures available for the session, including after ride notifications fade.
Connection dialogs also expose details inline. Changing language retranslates stored keys.
**Dismiss** clears the history and hides the control until the next failure. When a platform
refuses an upload or a connection, its own reason is passed as an untranslated value after the
translated summary: it usually says what to do next (wait, tick the scope box, copy the key).

## Consequences

- UI code never parses formatted errors or decides business failure classifications.
- Third-party diagnostics may remain English inside the separate details area.
- New failure operations need a classification and translated template; placeholder values
  must never be reinterpreted as formatting instructions.
- Gettext contexts distinguish the ride action from the ride settings tab. The individually
  reviewed German wording lives directly in the catalogue, with completeness checks.

## Amendment 2026-10-29: asynchronous group and native Android failures

Group transfer, preparation and voice errors now carry classified failures through ordinary
application events, rather than a second raw-message dialog. Each occurrence has a local
monotonic revision so repeated status snapshots do not resurrect dismissed errors; a new
occurrence of the same cause still appears. Activity retry state is independent of the latest
voice/admission failure. The existing bounded session history and translated summaries apply.

Native voice errors retain their operation and CPAL cause, including worker-start failures,
timeouts and caught device-factory panics. Callback diagnostics hold at most one cause and
never microphone samples. Android package-installer errors also enter the existing update
failure path. The shared Dismiss control updates when the interface language changes.
