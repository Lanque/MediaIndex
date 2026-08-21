# MediaIndex

> Find any shot in your local footage in seconds, without uploading your entire archive.

MediaIndex is a local-first media indexing and search platform for large video projects. The desktop application discovers footage where it already lives, builds a fast local index, and syncs project knowledge to the cloud only when that provides value.

## Current status

The repository is being bootstrapped from the project plan. The first implementation target is the local desktop MVP:

- folder scanning and media discovery
- FFprobe/FFmpeg metadata extraction
- SQLite local index
- stable content hashing
- metadata and keyword search
- opening the original local clip
- detection of new, modified, moved, and deleted files

The cloud roadmap follows after the local workflow is useful on its own.

## Product principles

- Original footage stays local by default.
- SQLite is a machine-local index, not a second source of truth.
- PostgreSQL becomes the shared project source of truth when cloud sync is introduced.
- A content hash identifies a logical media asset; a local path identifies only one local copy.
- Expensive cloud processing is explicit, asynchronous, retry-safe, and observable.
- Deterministic filters remain separate from AI similarity search.

## Planned architecture

```text
Local footage
    |
    v
Tauri desktop app ---> SQLite local index ---> local search / clip opening
    |
    | explicit, incremental sync
    v
FastAPI ---> PostgreSQL project state
    |
    | explicit processing requests
    v
SQS ---> worker service ---> transcripts / tags / embeddings / previews
```

The full architecture and the reasoning behind each component live in [docs/architecture.md](docs/architecture.md).

## Repository map

| Path | Purpose |
| --- | --- |
| `desktop/` | Tauri + TypeScript desktop client |
| `api/` | FastAPI service |
| `worker/` | asynchronous processing worker |
| `shared/` | API contracts and shared schemas |
| `migrations/` | PostgreSQL migrations |
| `infra/` | Terraform infrastructure |
| `tests/` | integration and contract tests |
| `docs/` | architecture, roadmap, ADRs, and engineering decisions |

## Working in this repository

All work should start from a GitHub issue and land through a pull request:

1. Choose or create an issue.
2. Create a branch named `<type>/<issue-number>-<short-slug>`.
3. Keep commits focused and use Conventional Commit style.
4. Open a draft PR early, linking the issue.
5. Add tests and documentation with the change.
6. Merge only after the PR checklist and CI are green.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/development-workflow.md](docs/development-workflow.md).

## Scope boundaries

The initial project deliberately does not include permanent cloud backup of every original, real-time team collaboration, a mobile app, a cloud rendering farm, complex permissions, or an AI chatbot as the primary UI.

## Source and status of documentation

The attached project plan is the product and architecture input for this repository. Repository documentation is the living engineering record: when implementation decisions change, update the relevant document or add an ADR in the same pull request.
