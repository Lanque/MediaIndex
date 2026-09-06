# Agent work log

## 2026-09-06 — MI-01 incomplete usage and budget safety

- Baseline: `8804176` (`fix: budget transcription by extracted audio duration`).
- Implementation commit: `6d9fcb9` (`fix: retain budget for incomplete AI usage`).
- Documentation commit: pending after the final documentation checks.
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
- `cargo fmt --check`, `npm.cmd run build`, and `git diff --check`: pending
  until the documentation commit is prepared.

### Remaining risks

- Provider APIs can add new usage shapes; unsupported shapes remain conservative
  and keep the reserve rather than being inferred as zero.
- The existing real-media smoke test still requires a configured local video and
  FFmpeg, so it remains ignored in this environment.
