# Testing strategy

The project grows from a dependency-free repository foundation into a multi-component system. Tests should follow the boundaries that make failure behavior observable.

## Current repository foundation

Run from the repository root:

```bash
python scripts/check_repository.py
python -m unittest discover -s tests -p "test_*.py"
```

These checks validate that the documentation map, ADR sequence, issue/PR templates, and initial roadmap remain intact.

## CI contract

Every pull request should run:

1. whitespace and diff checks;
2. repository/documentation validation;
3. repository foundation tests;
4. component-specific checks once code exists.

The workflow is intentionally small while the repository is still a documentation-first scaffold. It should gain jobs rather than become a single opaque script.

The current desktop job runs on Windows and verifies `npm ci`, the
TypeScript/Vite build, Rust formatting, and the Rust test suite. This mirrors
the supported desktop toolchain without requiring cloud services or media
uploads. FFprobe parsing uses a small committed JSON fixture, while an
unavailable FFprobe executable is tested as an actionable error state.
The job also creates the production NSIS installer and uploads it as a
short-lived workflow artifact, so packaging failures block the pull request.

An opt-in real-media smoke test uses a local OpenAI HTTP stub, runs one real
video through FFmpeg frame sampling, response parsing, embeddings, SQLite AI
search, and local JPEG thumbnail extraction. Set `MEDIAINDEX_SMOKE_VIDEO` and
run the ignored `analyzes_real_video_through_openai_response_pipeline` test;
private footage and generated thumbnails must not be committed.

Deterministic cancellation tests verify that concurrent AI runs are rejected,
a stop request is observed before frame/API work begins, and the run lock is
released after cancellation.

## Planned component checks

| Component | Planned checks |
| --- | --- |
| `desktop/` | TypeScript lint/type checks, Rust formatting/checks, scanner/hash tests |
| `api/` | pytest, formatting, static typing, migration checks |
| `worker/` | pytest, job-state tests, retry/idempotency tests |
| `shared/` | contract/schema compatibility tests |
| `migrations/` | PostgreSQL integration tests |
| `infra/` | Terraform formatting, validation, plan review |
| `tests/` | cross-component integration and failure tests |

## Failure-focused testing

The important cases are not only successful indexing and processing. The test plan must cover:

- a file moved without content changing;
- a file modified during or after indexing;
- interrupted hashing;
- FFprobe failure;
- network loss during sync;
- stale sync cursor and repeated idempotency key;
- duplicate queue delivery;
- worker crash and graceful shutdown;
- authorization failure;
- invalid file type or size;
- cloud failure while local search remains available.

## Fixture policy

Prefer tiny synthetic or openly redistributable fixtures. Do not commit large private footage, production metadata, credentials, or generated databases.
