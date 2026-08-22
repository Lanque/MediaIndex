# Project plan

## Product goal

MediaIndex helps a filmmaker find shots in large local footage libraries without uploading the entire archive. The desktop client indexes media where it already lives, while the cloud stores shared project knowledge and performs selected expensive analysis asynchronously.

## Core workflow

1. The user selects a local footage folder.
2. MediaIndex discovers files and extracts deterministic metadata locally.
3. The local index supports fast search, filters, and opening the original clip.
4. The user optionally syncs project knowledge to the cloud.
5. The user explicitly selects clips for transcription, visual analysis, embeddings, or previews.
6. The user searches the resulting knowledge from a connected machine.

## Architectural principles

- Original footage stays local by default.
- SQLite is a machine-local index, not a second master database.
- PostgreSQL is the shared project source of truth once cloud sync exists.
- A content hash identifies a logical MediaAsset; a local path identifies only a LocalFile instance.
- Expensive cloud processing is explicit, asynchronous, retry-safe, and observable.
- Deterministic filters are kept separate from semantic similarity search.
- Cloud failures must leave useful local functionality available.

## Delivery phases

### Phase 1 — Local desktop MVP

Tauri + TypeScript, folder scanning, FFprobe metadata, SQLite, content hashing, local search/filtering, clip opening, and file change detection.

### Phase 2 — Cloud sync

FastAPI, authentication, project ownership, PostgreSQL, cloud IDs, incremental cursor/version sync, retry-safe requests, and idempotency keys.

### Phase 3 — Async cloud processing

SQS, a worker service, explicit job requests, durable states, retries, dead-letter handling, idempotent processing, and human-readable job history.

### Phase 4 — Semantic search

The desktop MVP now has an explicit sampled-frame visual index: AI-generated
descriptions and embeddings are stored locally and natural-language queries can
return timestamped moments. Extend this with PostgreSQL full-text search and
pgvector after the deterministic and local AI search experience is useful.

### Phase 5 — AWS deployment and hardening

Terraform, ECS Fargate, RDS PostgreSQL, S3 for selected artifacts, Secrets Manager, CloudWatch, GitHub Actions, budgets, alarms, security, observability, and failure testing.

## Recommended build order

1. local scanner
2. local index
3. change detection
4. cloud identity
5. incremental sync
6. queue
7. worker
8. semantic search
9. AWS deployment
10. hardening

## Deliberate non-goals

The initial project does not attempt to be a full video hosting platform, permanent backup for every original, real-time collaboration suite, mobile application, cloud rendering farm, complex permission system, or chatbot-first product.

## Portfolio success criterion

A reviewer should be able to see why every major component exists, what happens when things fail, and how the system scales without the repository pretending to be a production service before it has earned that claim.
