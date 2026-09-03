# Engineering status

Status snapshot: 2026-09-03.

The local integration branch contains the implementation areas below. GitHub
issue and PR state is deliberately not labeled open, closed, draft, or merged in
this snapshot: the local GitHub CLI is not authenticated for the private
repository and returns HTTP 401. Verify live status before changing any issue or
pull request.

| Area | Local issue/PR references | Local implementation state |
| --- | --- | --- |
| Tauri shell and CI foundation | #2, #7 / #16–#18 | present on integration branch |
| Discovery and change detection | #3 / #20 | implemented and covered by Rust tests |
| FFprobe metadata | #4 / #21 | implemented with cached metadata and errors |
| SQLite local index | #5 / #22 | implemented with per-root reconciliation and saved cross-folder AI data |
| Offline search, grouping, preview, and opening | #6 / #23 | implemented with separate current-folder and analyzed-archive views |
| Cloud contracts and PostgreSQL model | #9 / #24 | scaffold and contract checks present |
| Cursor/idempotent sync | #10 / #25 | implementation present |
| Worker retries and dead letters | #11 / #26 | implementation present |
| Security guardrails | #13 / #27 | static safeguards and documentation present |
| AWS/Terraform scaffold | #12 / #28 | scaffold only; not applied |
| Documentation maintenance | #14 | updated in the integration branch |
| Selected-media preview job | #30 / #32 | implementation present |
| FastAPI sync runtime | #31 | implementation and tests present |
| Desktop AI visual index and search | #38 / #39 | implemented with provider compatibility fixes, contextual time ranges, timestamped OpenAI speech, Gemini Desktop OAuth, reanalysis confirmation, and first-run/calibrated ETA |

## Integration state

- Fetched `origin/main` is `79e2626` and contains merged PRs #16, #17, #18,
  #20, #21, #22, #37, and #39. The local `main` pointer remains stale at
  `78cb453`; use `origin/main` for comparisons.
- `feat/performance-cost-and-search-optimizations` is the current desktop MVP
  follow-up branch. Commit `6ef9655` merged `origin/main`, so the branch now
  contains the fetched baseline and is 18 commits ahead, 0 behind after this
  snapshot.
- The merge strategy and live GitHub reconciliation are documented in
  [main-integration-review.md](main-integration-review.md).
- Exact continuation and verification steps are in
  [next-agent-plan.md](next-agent-plan.md).

## Deliberate limitations

- Main-branch protection was previously attempted but unavailable under the
  repository settings/plan at that time. The owner chose to defer it.
- Terraform CLI and a PostgreSQL server are not required for the default static
  checks. Operators must run environment-specific validation before any apply.
- AWS infrastructure has not been applied and no credentials are stored in the
  repository.
- Cloud AI receives sampled JPEG frames only when the user explicitly starts
  analysis. OpenAI can additionally receive a temporary compressed speech track
  when timestamped transcription is enabled. Local Ollama is available for an
  off-device-free workflow.
- Public Windows releases should be Authenticode-signed; local/CI installers are
  suitable for testing but are unsigned unless release signing is configured.
