# Security, observability, and cost guardrails

The project keeps original footage local by default. Security behavior that is
already executable lives in [`security/guardrails.py`](../security/guardrails.py)
and is covered by tests.

## Input and authorization

- Client-provided paths are resolved, confined to the selected root, checked
  for symlinks, limited to configured media extensions, and size-checked before
  processing.
- Project authorization compares the authenticated user to the project owner
  and is reinforced by PostgreSQL row-level security.
- Local paths are never used as cloud identity or authorization keys.
- Desktop code has no long-lived cloud credentials.

## Signed access and secrets

If selected cloud artifacts ever need remote access, signed URLs are limited to
15 minutes by policy. Development secrets belong in ignored `.env` files or a
local secret store. Production credentials belong in AWS Secrets Manager and
are injected at runtime through least-privilege roles; they must not be placed
in the desktop bundle, repository, logs, or fixtures.

## Observability and failure behavior

Structured events carry operation and job identifiers, while token-like fields
are redacted. The sync and worker tests cover network interruption, duplicate
delivery, stale cursors, authorization failures, and worker crashes.

## Cost safeguards before AWS workloads

- Start worker and ECS capacity at zero or one small task; scale from measured
  queue depth rather than an unbounded target.
- Set AWS Budgets alerts at 50%, 80%, and 100% of the approved monthly budget,
  with an owner notification path before enabling cloud workloads.
- Keep original video out of S3 by default; cap selected artifact size and
  lifecycle temporary previews/transcodes.
- Cap queue batch size, retry count, and per-job processing time.
- Require an explicit processing request before transcription, embeddings, or
  previews are created.
