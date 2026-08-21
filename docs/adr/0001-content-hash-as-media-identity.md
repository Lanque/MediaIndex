# ADR 0001: Use content hash as MediaAsset identity

- Status: accepted
- Date: 2026-08-21

## Context

File paths are local implementation details. They change when a disk is mounted differently, a folder is moved, or the same footage is indexed on another machine.

## Decision

Use a content hash as the stable identity of a logical `MediaAsset`. Store each machine’s path as a separate `LocalFile` record.

## Consequences

- moved files can remain linked to the same asset;
- duplicate copies can be recognized;
- multi-device sync does not depend on path conventions;
- hashing has a cost and must be incremental, resumable, and tested;
- byte-identical content is treated as the same logical asset unless later product requirements introduce a different identity layer.

## Rejected alternative

Using the absolute local path as the primary identity would be simpler initially but would make synchronization and deduplication unreliable.
