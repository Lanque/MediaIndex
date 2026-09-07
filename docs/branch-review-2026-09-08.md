# Performance and cost branch review

Review scope: `feat/performance-cost-development-2026-09-05`, compared with
`main` at `8e4b02c`. This review covers all changed production modules, their
regression tests, migrations, desktop integration, and release documentation.

## Changes assessed

- Full-content SHA-256 identities and explicit migration of previously
  unverified file identities; copied content is analyzed once.
- Local cost planning, known/unknown provider prices, per-request reservations,
  retry accounting, incomplete usage, and cancellation.
- Incremental SQLite persistence, partial/failed coverage, protected previous
  complete results, settings fingerprints, and resumable vision checkpoints.
- Stage diagnostics and bounded local JSON exports; synthetic performance
  measurements do not establish live-provider speed.
- Visible cost preview after scanning, authentication-free estimation, stale
  response protection, and executable build identification.

## Release review findings

1. **Checkpoint scope mismatch:** a nonempty runtime checkpoint set did not
   establish that the exact plan and reusable batches promised by preflight
   were still present. Require exact plan/reuse validation before new paid
   vision work, including multiple valid plans and pruning regressions.
2. **Gemini output reservation:** the request reserved an output allowance but
   omitted `maxOutputTokens`. The transmitted cap must match the reserved cap.
3. **Gemini thinking usage:** `candidatesTokenCount` alone excludes thinking
   tokens. Billed output accounting must include thinking and preserve the
   reservation when usage cannot be established. Sources:
   [GenerateContent usage metadata](https://ai.google.dev/api/generate-content#UsageMetadata)
   and [thinking pricing](https://ai.google.dev/gemini-api/docs/thinking#pricing).
4. **Build provenance:** Cargo must track Git HEAD and ref changes so a
   commit-only rebuild cannot retain an earlier commit marker.

These findings were sent to the implementation task before merge. Final test
and packaging evidence is maintained in [release verification](release-verification.md).

## Publication checks

A pattern scan of all 520 unique Git blobs reachable from the fetched refs
found no matches for common OpenAI, Google, GitHub, AWS access keys or private
key headers. This is a targeted check, not a guarantee that no sensitive text
exists. The tracked tree contains no media, database or executable artifacts;
the tracked environment file is an example. Local agent tooling is excluded
from the release changes.

## Remaining development priorities

1. **Measure gameplay search quality cheaply.** Use a small redistributable
   labeled set with death, kill and attack timestamps. Measure recall, false
   positives, elapsed time and cost before changing sampling or models. First
   evaluate context hints, existing on-screen text and local candidate scoring;
   request denser analysis only for user-selected short ranges.
2. **Show timeline coverage explicitly.** A complete result currently means
   complete coverage of sampled frames, not the entire video. Add a visible
   analyzed time range and a separate whole-video overview mode with its own
   estimate and opt-in execution.
3. **Make local extraction cancellable and bounded.** FFmpeg still runs as a
   blocking child process, and all selected JPEG frames enter memory. Add child
   cancellation, temporary-file cleanup on every exit, and bounded frame queues.
4. **Use stage measurements for ETA and caching decisions.** Calibrate against
   real workloads; then evaluate extracted-frame, transcription and embedding
   caching and batched Gemini embeddings. Vision checkpoint reuse alone does
   not remove local extraction or downstream provider work.
5. **Harden provider accounting.** Image-token estimates remain heuristic,
   prices are a small dated catalog, and an application budget is not a
   provider billing cap. Add model-specific accounting, price expiry and
   estimate-versus-reported dashboards before promising stronger limits.
6. **Complete distribution verification.** Add Authenticode signing and the
   full installed-app matrix before a stable release. Keep this first release
   clearly marked as an unsigned experimental prerelease.
