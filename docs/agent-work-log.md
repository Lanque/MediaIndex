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
- Documentation commit: pending after the final documentation commit.
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
