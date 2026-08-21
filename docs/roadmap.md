# Roadmap

The roadmap is intentionally issue-driven. The issue is the execution unit; this document explains sequencing and exit criteria.

## Phase 1 — Local scanner and index

Epic: [#1](../issues/1)

- [#2](../issues/2) Bootstrap the Tauri desktop shell
- [#3](../issues/3) Implement media discovery and change detection
- [#4](../issues/4) Extract technical metadata with FFprobe
- [#5](../issues/5) Build the SQLite local index and content identity
- [#6](../issues/6) Implement local search, filters, and clip opening
- [#7](../issues/7) Establish automated tests and CI foundations

Exit criterion: a user can index a local sample folder, search it offline, and open the original clips while moved/changed/deleted files remain correctly represented.

## Phase 2 — Cloud identity and sync

Epic: [#8](../issues/8)

- [#9](../issues/9) Define shared API contracts and PostgreSQL domain model
- [#10](../issues/10) Implement incremental, idempotent local-to-cloud sync

Exit criterion: multiple devices can share project knowledge while content identity remains stable and local functionality survives network loss.

## Phase 3 — Explicit async analysis

Epic: [#15](../issues/15)

- [#11](../issues/11) Implement queue, worker, retries, and dead-letter handling

Exit criterion: one useful selected-media analysis can be processed reliably and explained when it fails.

## Phase 4/5 — Search and AWS architecture

Epic: [#12](../issues/12)

- [#13](../issues/13) Harden security, observability, and failure behavior

Exit criterion: deterministic search, optional semantic search, reproducible infrastructure, cost controls, and operational failure behavior are visible and testable.

## Cross-cutting documentation

- [#14](../issues/14) Maintain architecture decisions and engineering documentation

Documentation is not a final polish step. It changes with the behavior and is reviewed through the same PR.
