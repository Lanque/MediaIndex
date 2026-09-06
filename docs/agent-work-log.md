# Agent work log

## 2026-09-06 — MI-01 incomplete usage and budget safety

- Baseline: `8804176` (`fix: budget transcription by extracted audio duration`).
- Implementation commit: `6d9fcb9` (`fix: retain budget for incomplete AI usage`).
- Formatting commit: `71bedc0` (`style: format MI-01 usage tests`).
- Documentation commit: `09483d4` (`docs: document incomplete usage budget safety`).
- Scope: provider/operation-specific usage completeness, conservative handling of
  missing or partial usage, known lower-bound charging when it exceeds the
  request reserve, and local HTTP regression coverage for API errors, timeouts,
  unreadable bodies, parallel requests, and retries.
- No paid provider requests were made; all network behavior tests use local
  HTTP stubs.

### Checks

- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib --offline`:
  92 passed, 0 failed, 1 ignored.
- Targeted timeout regression test: passed.
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --offline`:
  92 passed, 0 failed, 1 ignored; main tests, binary tests, and doc-tests all
  completed successfully.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `npm.cmd run build`: passed.
- `git diff --check`: passed; Git reported only the existing LF/CRLF
  normalization warnings.

### Remaining risks

- Provider APIs can add new usage shapes; unsupported shapes remain conservative
  and keep the reserve rather than being inferred as zero.
- The existing real-media smoke test still requires a configured local video and
  FFmpeg, so it remains ignored in this environment.

## 2026-09-06 — MI-02 cancellation propagation

- Baseline: `65eb10f` (`docs: record MI-01 verification`).
- Implementation commit: `a67079b` (`fix: stop analysis retries after cancellation`).
- Documentation commit: `b4192ac` (`docs: document analysis cancellation semantics`).
- Scope: shared analysis cancellation checks before reservations and sends,
  cancellable retry backoff, cancellation-safe reservation cleanup, and a
  cancellation check between Gemini document embeddings. Search and Test
  connection retain their independent non-cancellable request path.
- No paid provider requests were made; all new network tests use local HTTP
  stubs.

### Checks

- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --lib --offline cancellation`:
  7 passed, 0 failed.
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --offline`:
  97 passed, 0 failed, 1 ignored; main tests, binary tests, and doc-tests all
  completed successfully.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `npm.cmd run build`: passed.
- `git diff --check`: passed; Git reported only the existing LF/CRLF
  normalization warnings.

### Remaining risks

- A blocking HTTP request already sent to a provider cannot be withdrawn; it may
  finish within the configured timeout and its usage remains recorded.
- FFmpeg process cancellation, checkpoint/resume, pricing-model expansion, and
  UI changes are outside MI-02.

## 2026-09-06 — MI-03 durable AI coverage and safe partial results

- Baseline: `f8160ab` (`docs: record MI-02 verification`).
- Implementation commit: `438cac6` (`fix: persist partial AI coverage safely`).
- Documentation commit: `aab22e1` (`docs: record MI-03 coverage safeguards`).
- Scope: structured per-file complete/partial/failed results, failed vision
  batch timestamp ranges, a settings fingerprint covering prompt version and
  sampling/context/speech settings, durable SQLite coverage history, legacy
  coverage-unknown migration handling, explicit partial/legacy retry choice,
  visible coverage warnings, and protection for prior complete same-model
  annotations during failed or partial retries.
- Partial annotations remain searchable when no protected complete result
  exists. A failed or partial retry records diagnostics without deleting the
  prior complete result; other model namespaces remain independent.
- No paid provider requests were made; coverage tests use SQLite fixtures and
  local test data.

### Checks

- Coverage-focused Rust tests: 5 passed, 0 failed.
- Full offline Rust test suite: 106 passed, 0 failed, 1 ignored.
- `npm.cmd run build`: passed.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed apart from the repository's existing LF/CRLF
  normalization warnings.

### Remaining risks

- FFmpeg process cancellation, checkpoint/resume, performance/ETA
  instrumentation, and batch continuation remain intentionally outside MI-03.
- The real-media OpenAI smoke test still requires a configured local video and
  FFmpeg, so it remains ignored in this environment.

## 2026-09-06 — MI-03R active coverage pointer and cancellation fix

- Baseline: `aab22e1` (`docs: record MI-03 coverage safeguards`).
- Implementation commit: `df125c0` (`fix: separate active AI coverage from latest attempts`).
- Documentation commit: `f6f0ef0` (`docs: record MI-03R coverage safeguards`).
- Scope: an annotation-to-coverage pointer and migration for durable active
  results, selection that trusts only the active matching settings fingerprint,
  separate latest-attempt diagnostics in library/search/inspector responses,
  and cancellation-specific handling for speech retries.
- Complete annotations stay active when a later failed or partial attempt only
  records diagnostics. Switching from A to B and back to A no longer treats a
  historical A row as active while B annotations are stored.
- No paid provider requests were made; the speech cancellation regression uses
  a local multipart HTTP stub and the coverage regressions use SQLite fixtures.

### Checks

- Full offline Rust test suite: 109 passed, 0 failed, 1 ignored.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `npm.cmd run build`: passed.
- `git diff --check`: passed apart from the repository's existing LF/CRLF
  normalization warnings.

### Remaining risks

- FFmpeg process cancellation, checkpoint/resume, performance/ETA
  instrumentation, and batch continuation remain intentionally outside MI-03R.
- The real-media OpenAI smoke test still requires a configured local video and
  FFmpeg, so it remains ignored in this environment.

## 2026-09-06 — MI-04 persistent vision checkpoints and continuation

- Baseline: `f2cf355` (`docs: finalize MI-03R work log reference`); ADR commit:
  `ff89ed1` (`docs: add MI-04 vision checkpoint ADR`).
- Implementation commits: `84eac50` (`feat: persist resumable vision
  checkpoints`) and `24dbec9` (`feat: expose vision checkpoint continuation`).
- Follow-up test commit: `eece660` (`test: cover downstream checkpoint
  recovery`).
- Scope: migration 11 for local vision-batch checkpoints, exact content/settings/
  version/timestamp identity, validated metadata-only reuse, SQLite-owner
  request/acknowledgement writes, failure-before-next-batch behavior, bounded
  retention, transactional cleanup after complete results, continuation-only
  vision cost estimation, and explicit desktop continuation/full-reanalysis
  choices. Audio and document embedding caching, new sampling, and parallelism
  remain outside MI-04.
- Checkpoint tests use a local HTTP stub and a file-backed SQLite database. The
  tests cover restart continuation without repeating committed vision requests,
  all-vision reuse after downstream failure, incompatible/corrupt rows,
  failed-write ACK boundaries, checkpoint cleanup, and explicit queue selection.
- No paid provider requests were made.

### Checks

- `cargo test --manifest-path desktop/src-tauri/Cargo.toml --offline`:
  115 passed, 0 failed, 1 ignored; the ignored test requires a real video and
  FFmpeg.
- `npm.cmd run build`: passed.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed apart from the repository's existing LF/CRLF
  normalization warnings.

### Remaining risks

- FFmpeg extraction still runs before checkpoint lookup, so MI-04 avoids paid
  vision repetition but does not yet avoid frame extraction or JPEG memory cost.
- The full production worker path still has existing per-file remote
  parallelism; a checkpoint write failure cancels other work, while an already
  in-flight provider request may finish and remain recorded.
- Stage-wise performance instrumentation, batch-level continuation across
  audio/embedding, and precise ETA calibration remain future work.

## 2026-09-06 — MI-04R actual frame-plan reconciliation and dispatcher safety

- Baseline: `4d5c919` (`docs: note MI-04 downstream recovery test`).
- Implementation commit: `1c2a398` (`fix: reconcile vision checkpoints with
  actual frames`).
- UI guard commit: `870490e` (`fix: keep declined checkpoint work out of
  no-op runs`).
- Scope: metadata-independent discovery of the latest valid persisted frame
  plan, actual-plan frame-count/cost reporting, explicit runtime failure when a
  saved plan no longer matches extracted frames, and a production
  request/acknowledgement dispatcher test with two workers. Declining saved
  work now ends a checkpoint-only UI run without starting a no-op analysis.
- The preflight regression covers a short metadata estimate predicting one
  frame while the committed plan contains two frames. A separate file-backed
  SQLite/local HTTP test uses two actual frames with missing metadata, reopens
  the database, and confirms the resumed run sends zero new vision requests.
- No paid provider requests were made; the dispatcher and AI tests use local
  SQLite fixtures and a loopback HTTP stub.

### Checks

- Full offline Rust test suite: 117 passed, 0 failed, 1 ignored.
- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`: passed.
- `npm.cmd run build`: passed.
- `git diff --check`: passed apart from the repository's existing LF/CRLF
  normalization warnings.

### Remaining risks

- FFmpeg extraction still runs before checkpoint lookup, and its deterministic
  output timestamps remain the runtime source of truth; MI-04R does not remove
  frame extraction or JPEG memory cost.
- A runtime plan mismatch is recorded as a visible per-file analysis failure;
  the user must review the estimate/scan and explicitly retry rather than
  silently falling back to a paid plan.
- Stage-wise performance instrumentation, batch-level continuation across
  audio/embedding, and precise ETA calibration remain future work.
