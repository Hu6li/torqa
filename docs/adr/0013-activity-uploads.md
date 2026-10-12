# ADR 0013 — Activity uploads

- Status: accepted
- Date: 2026-10-12

## Context

R29 asks for uploads to Strava, intervals.icu, Garmin Connect, TrainingPeaks and Komoot behind
one interface (R4). Torqa is open source and has no server of its own (R3). The platforms let
an app in differently. As of October 2026:

- **intervals.icu** takes a personal **API key**, sent with Basic auth, for a rider's own data.
  Its OAuth needs a client secret.
- **Strava** has OAuth only, and every token request needs the application's **client
  secret**: there is no public-client (PKCE) flow. A secret inside an open-source app is public,
  which breaks the Strava API Agreement. A new API application needs a Strava subscription and
  serves its owner alone (10 athletes after an upgrade, more only after Strava's review).
- **Garmin Connect** has paused new applications to its Connect Developer Program, and its
  Activity API reads activities rather than taking them.
- **TrainingPeaks** and **Komoot** give API access to approved commercial partners only.

## Decision

- **`ActivityUploader`** (`torqa-domain::upload`) is object-safe, so all platforms sit in one
  list (`torqa_upload::platforms()`). It has:
  - an id and a name;
  - the **fields** the rider types in to connect, with a help text and a link;
  - `connect`, which checks what was entered with the platform;
  - `upload`, which takes a FIT file, the ride's name (R50) and Torqa's id for it.

  Its operations return boxed futures. It comes with its first platform, not its second as
  the other plugin traits do: the second (Strava) is decided and planned.
- **Credentials belong to the platform.** They are a string map it fills on `connect` and gets
  back for every upload, changed in place when, say, a token is renewed. Torqa stores it per
  rider, as given, in `profiles/<id>/accounts.toml`. A new platform needs no change to storage,
  the app layer or the interface.
- **intervals.icu first**, with the rider's API key: no login, no approval, nothing to register.
  An activity intervals.icu already has (it compares files) counts as uploaded.
- **Where a ride went is kept with the ride**, in its JSON metadata (`uploads`: platform id →
  activity id and page), so it syncs with the ride (ADR 0002) and the history links to it.
- **Uploads only go out when the rider asks**, from the summary or the history.
- Garmin Connect, TrainingPeaks and Komoot are left out until they let open-source apps in.
  Until then rides reach them through the FIT export (R28) and the platform's own import.

## Consequences

- No secret of Torqa's to keep and no server.
- The accounts file holds secrets as plain text. On macOS and Linux only its owner can read it;
  it syncs with the data directory. An OS keychain would keep it off the disk but needs a new
  dependency, and the key would no longer follow the rider to another computer. Left for later.
- A Torqa from before uploads that renames an uploaded ride writes its JSON without `uploads`.
  The ride then shows as not uploaded; uploading it again links the activity the platform
  already has.
- Strava fits the same interface. Its OAuth login in the browser adds a parameter to
  `connect` when it comes.
