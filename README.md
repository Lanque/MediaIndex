# MediaIndex

> Find any shot in your local footage in seconds, without uploading your entire archive.

MediaIndex is a local-first media indexing and search platform for large video projects. The desktop application discovers footage where it already lives, builds a fast local index, and syncs project knowledge to the cloud only when that provides value.

## Current status

The current integration branch contains a working Windows desktop MVP packaged
with Tauri. MediaIndex is not a browser-hosted product: the TypeScript interface
runs inside the native desktop shell and works with the local filesystem through
the Rust backend.

- recursive folder scanning, stable hashing, and change detection;
- cached FFprobe metadata and a machine-local SQLite index;
- a selected-folder view plus a separate saved AI-analysis archive, both grouped
  by the footage's real source folders;
- deterministic search, filters, local preview, and opening the original clip;
- explicit sampled-frame AI analysis through OpenAI, Gemini, or local Ollama;
- per-video visual search that combines similar adjacent frames into contextual
  time ranges;
- background analysis progress, measured time estimates, cancellation, and
  bounded cloud cost preflight.

The API, worker, migration, and Terraform foundations are present, but no AWS
infrastructure is applied by this repository. GitHub's `origin/main` now
contains the merged foundation, scanner, metadata, local-index, responsive
preview, and AI-search stack through PR #39. The current branch is synchronized
with that baseline and adds the performance, provider, and interface follow-up;
review the exact delta in
[docs/main-integration-review.md](docs/main-integration-review.md) before
merging.

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
| `PRODUCT.md` | product brief and desktop delivery boundary |
| `DESIGN.md` | canonical desktop visual system |
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
The detailed continuation checklist is in
[docs/next-agent-plan.md](docs/next-agent-plan.md).

## Scope boundaries

The initial project deliberately does not include permanent cloud backup of every original, real-time team collaboration, a mobile app, a cloud rendering farm, complex permissions, or an AI chatbot as the primary UI.

## Source and status of documentation

The attached project plan is the product and architecture input for this repository. Repository documentation is the living engineering record: when implementation decisions change, update the relevant document or add an ADR in the same pull request.
