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

The desktop AI connection persists non-secret settings only. Cloud API keys
are held in the current WebView session and legacy keys are removed from
persistent local storage on startup. Gemini Desktop OAuth uses PKCE and an
ephemeral `127.0.0.1` callback; its access token stays in Rust process memory
and is cleared on disconnect, expiry, or app exit. The selected OAuth client
JSON is read for login and is not copied into app settings. The backend never
returns a key or OAuth token to the WebView, search, or thumbnail results.

The production desktop WebView enforces a Content Security Policy that permits
only bundled application resources, Tauri IPC, local asset-protocol video, and
in-memory thumbnail images. The asset protocol starts with an empty filesystem
scope. Before Preview receives a local URL, a Rust command confirms that the
path is an active SQLite-indexed file, confirms it still exists, and authorizes
only that file for the current process. System-player opening applies the same
index and availability checks.

AI request attempts are recorded locally with operation/model, retry number,
duration, HTTP status, optional provider request ID, pricing status, and an
explicit possible-charge flag. Request bodies, media bytes, API keys, and
response content are excluded. A request reserve is settled only when its
provider usage is complete: vision needs both input and output tokens,
embedding needs input tokens, and transcription needs a provider-reported or
locally measured audio duration. Missing or partial usage remains unknown or
partial and keeps the conservative reserve; an independently known lower bound
can increase the committed cost when it exceeds that reserve. Explicit numeric
zero usage is valid and releases the unused reserve. HTTP errors, timeouts, and
unreadable responses retain unknown cost instead of being inferred as zero.

Windows installers built locally or in pull-request CI are intentionally
unsigned until the release owner provides an Authenticode certificate through
the release environment. Certificate material and passwords must never be
committed. Public distribution is gated on signing and verifying the final
installer; unsigned artifacts remain test builds.

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

## Desktop AI cost safeguards

- GPT-5.6 Luna is the cost-sensitive OpenAI default; the substantially more
  expensive Terra preset is labeled as an explicit detailed-analysis choice.
- New OpenAI settings cap analysis at 60 sampled frames per video by default.
- Timestamped OpenAI speech indexing uploads only a temporary mono 16 kHz,
  32 kbit/s MP3 for the configured analysis span, never the original video.
  The temporary file is deleted immediately after the request, and the feature
  can be disabled for frame-only analysis.
- Every run requires confirmation after a local preflight reports
  the unique content count, duration-based estimated sampled frames/vision
  requests, configured upper bounds, estimated speech-audio duration, and a
  model-aware first-run time estimate, a low/likely/high API cost estimate, and
  a locally measured per-model estimate after calibration. The API cost estimate
  is a heuristic: unknown model pricing is reported as unknown rather than zero,
  local runtime cost excludes CPU/GPU time and electricity, and the checked
  pricing date and source are retained in the plan. Cancelling the dialog sends
  no provider requests.
- Remote pricing is maintained as a small checked catalog using the
  [OpenAI pricing](https://developers.openai.com/api/docs/pricing) and
  [Gemini pricing](https://ai.google.dev/gemini-api/docs/pricing) pages. A custom
  or newly introduced model must be priced before a USD estimate can be shown.
- Duplicate file paths that share a content hash are analyzed once, preventing
  duplicate API spend for copied footage.
- **Analyze with AI** skips content that already has annotations in the active
  provider/model namespace. Reanalysis is a one-run checkbox that is never
  persisted and resets after the run. Reanalysis requires confirmation and
  clearly states that same-model annotations will be replaced.
- AI analysis coverage is durable and keyed by content hash, provider/model
  namespace, and a settings fingerprint. The fingerprint includes the prompt
  version, sampling interval, frame cap, batch shape, context hint, and speech
  settings. Each attempt records complete, partial, or failed status, planned
  and successful frame counts, failed timestamp ranges, and a user-visible
  warning. Pre-migration annotations are marked coverage-unknown and are not
  automatically sent to a paid queue.
- Partial or failed retries are an explicit user choice. Their conservative
  full-attempt estimate is shown before confirmation. A partial result can
  remain searchable, but it is labeled as incomplete; a failed or partial
  retry never deletes a prior complete same-model result. Different model
  histories remain independent. A complete, explicitly confirmed retry may
  replace the prior same-model annotations after the new result is fully
  available.
- Only one AI analysis can run at a time. The stop control prevents additional
  files and frame batches from starting, keeps completed clip annotations, and
  lets a later run continue with missing clips. The shared cancellation check
  runs before each budget reservation and network send, between individual
  Gemini embedding requests, and during retry backoff. A stop before sending
  creates no paid-attempt event and releases any race-created reservation. A
  cloud request already sent to the provider may finish before cancellation
  takes effect; its usage event is retained and unknown usage remains reserved.
  Search and Test connection use independent request paths and are not stopped
  by an analysis run cancellation.
- Focused search limits low-ranking results, videos, and moments without making
  another vision request. Query embeddings remain the only AI call during
  search and are recorded as a separate usage operation.
- Connection tests are also recorded separately from analysis; their network
  requests can therefore be distinguished from indexed-media usage.
- Best-moment thumbnails are extracted and cached locally with FFmpeg. The
  thumbnail command accepts only active paths already present in the SQLite
  index and never uploads the source frame.
