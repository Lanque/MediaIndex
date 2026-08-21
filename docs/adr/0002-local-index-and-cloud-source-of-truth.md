# ADR 0002: SQLite is local index; PostgreSQL is cloud source of truth

- Status: accepted
- Date: 2026-08-21

## Context

The desktop client must remain fast and useful offline, while multiple devices need shared project state.

## Decision

SQLite stores the machine-local index and cache. PostgreSQL stores shared projects, MediaAssets, permissions, sync state, and cloud analysis results once cloud sync exists.

## Consequences

- local reads do not depend on the network;
- cloud sync must define explicit cursors, idempotency, and conflict behavior;
- the two stores may contain different projections of the same knowledge;
- code must not silently treat SQLite as a second master database;
- offline changes need a clear sync representation.
