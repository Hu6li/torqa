# ADR 0002 — Sync-safe storage

- Status: accepted
- Date: 2026-10-02

## Context

The data directory is configurable and may live in a file-synced folder such as Nextcloud (R30).
SQLite databases inside synced folders are prone to corruption and sync conflicts, because sync
clients copy files while they are being written and cannot merge concurrent changes.

## Decision

- **Files are the source of truth**: each ride is one FIT file plus one JSON metadata file; routes
  and generated worlds are stored as files too. Files are written atomically (write temp, rename).
- A **SQLite index** (history, PRs, search) is a rebuildable cache stored **outside** the data
  directory, in the platform's local cache location. If it is missing or stale, it is rebuilt by
  scanning the data directory.
- Per-profile data lives in per-profile subdirectories to avoid write contention between users.

## Consequences

- Syncing between machines works with any file sync tool; at worst a conflict produces a duplicate
  file, never a corrupt database.
- Index rebuilds cost a scan of the data directory at startup when changes are detected.
