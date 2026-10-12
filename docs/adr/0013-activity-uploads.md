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
- **Uploads only go out when the rider asks**, from the summary or the history (amended below:
  also automatically, per account).
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
  `connect` when it comes (see the Strava amendment).

## Amendment — 2026-10-12: automatic uploads (R60)

- Each account can be set to **upload automatically**. A ride goes up when the rider keeps it
  with **Done** on its summary: the ride is saved before the summary, where it may still be
  renamed or discarded, so saving is too early.
- **Uploads take turns**, one at a time. Each reads the account's credentials when its turn
  comes and stores renewed ones before the next starts. Platforms that renew tokens with every
  use (Strava) accept only the newest one, so two uploads of one account must not renew
  side by side.
- A ride on its way to a platform is not sent there again, and a kept ride already there is
  skipped. A failed automatic upload leaves the ride to upload from the history; it does not
  open the connect dialog by itself.

## Amendment — 2026-10-12: Strava

Options for Strava, which needs the client secret for every token request:

- **Torqa's own application, its secret built in**: against the API Agreement, and one
  athlete until Strava reviews it.
- **Torqa's own application and a token service holding the secret**: a server to run for good,
  Strava's review past 10 athletes, and every rider's tokens passing through Torqa's
  infrastructure.
- **Each rider's own Strava API application**: no secret to leak and no server, within the
  single-athlete capacity; the rider creates the application once.

Decision:

- **Strava connects through the rider's own API application**: its Client ID and Client
  Secret in the dialog, then the login in the browser, asking for `activity:write` only.
- **OAuth platforms share `torqa_upload::oauth`**: the authorization code comes back over a
  loopback redirect (RFC 8252). For the login only, Torqa listens on `127.0.0.1` at a free
  port and checks the `state`. While the browser waits, Torqa trades the code for tokens, so
  the page it then shows says how it ended. Access tokens are renewed shortly before they
  expire, in the upload's turn (Strava's refresh tokens rotate). The rider can cancel a login.
- `ActivityUploader::connect` gets the rider's **browser** (a way to open the login and the
  pages to come back to, in the rider's language), and `disconnect` withdraws a platform's
  grant (RFC 7009) as the rider disconnects or is deleted; best effort.
- Rides go up as FIT files marked as on a trainer, named after the ride; Strava processes them
  for a few seconds, and Torqa waits for the activity. One Strava already has counts as
  uploaded.
- The API stays at `www.strava.com/api/v3`; Strava's new host (`api-v3.strava.com`) only
  opens in 2027. Tokens already go in the `Authorization` header, and access is revoked with
  `/oauth/revoke`, as Strava requires from June 2027.

Consequences:

- Every rider uploading to Strava needs a Strava subscription and a few minutes to create the
  application, explained in the dialog and in [uploads.md](../uploads.md).
- Should Strava offer a public-client (PKCE) flow, Strava can move to an application of
  Torqa's with an id only; accounts connected before keep working as they are.
