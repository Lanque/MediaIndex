# ADR: MI-04 persistent vision checkpoints

## Status

Accepted for MI-04 implementation.

## Decision

MediaIndex stores each successfully validated vision frame batch in a local
SQLite checkpoint table before the next paid vision request or the later audio
and embedding stages. A checkpoint is reusable only when all of these values
match the current work:

- content hash;
- analysis settings fingerprint, including prompt and sampling settings;
- checkpoint schema/batch version; and
- the complete, ordered frame-timestamp list plus the exact batch timestamp
  slice.

The serialized checkpoint contains only normalized frame-analysis metadata and
text. It contains no API key, provider credential, original media, or frame
bytes. A corrupt, incomplete, or incompatible row is ignored and its batch is
reanalyzed; it cannot make a run complete by itself.

The analysis worker asks the SQLite-owning thread to load and store checkpoints
through a request/acknowledgement channel. Each store is a short committed
transaction. A failed store returns an error before another paid vision batch
can start. The worker does not retry a failed database write as a provider
request.

Normal explicit “continue saved work” analysis may reuse matching checkpoints.
The existing explicit full-reanalysis path bypasses checkpoints and retains the
MI-03 rule that a new complete result replaces annotations only after all later
stages succeed. Audio and document embeddings are intentionally not cached in
MI-04, so a continuation still estimates and performs those stages again.

Checkpoints survive cancellation, restart, failed audio, and failed embedding
stages. They are deleted transactionally with a successful complete annotation
commit. A deterministic global retention bound prevents abandoned plans from
growing the table without limit.

## Consequences

The preflight plan reports reusable vision frames/requests and remaining vision
requests separately. Its API estimate charges only missing vision work while
still charging the full required audio and embedding work. A continuation is
shown with an explicit cost confirmation; full reanalysis remains a separate
choice that can intentionally bypass saved checkpoints.

MI-04 does not add parallelism, a new sampling strategy, audio caching, or
embedding caching. The real media smoke test remains environment-dependent.
