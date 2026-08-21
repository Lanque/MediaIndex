# ADR 0005: Explicit sampled-frame AI visual index

- Status: accepted
- Date: 2026-08-21

## Context

Filename and FFprobe metadata cannot answer queries such as “find the Fortnite
kill”. The application needs timestamped visual descriptions, but analyzing an
entire local archive automatically would be expensive, slow, and surprising.

## Decision

Add an explicit **Analyze with AI** action. The desktop app uses local FFmpeg to
sample frames from already indexed clips, sends only those sampled images to a
configured vision-capable AI provider, creates embeddings for the returned
descriptions, and stores timestamped annotations in SQLite. The provider can be
local Ollama or a cloud API (OpenAI or Gemini), configured in the desktop UI;
environment variables remain an automation fallback. AI search embeds the
natural-language query and ranks stored annotations by cosine similarity while
filtering to the active provider/model namespace.

Deterministic local search remains independent and continues to work without an
API key. Sampling interval, maximum frames, provider, model, and endpoint are
configurable in-app, with environment variables retained for automation.

## Consequences

- a query can return both a clip and the moment to preview;
- users explicitly control when frames leave the machine and when API cost is incurred;
- short events can be missed when the sample interval is too large;
- switching provider or embedding model does not compare incompatible vector
  dimensions against the previous provider's annotations;
- a later worker/cloud implementation can reuse the annotation contract without
  changing the local search behavior.
