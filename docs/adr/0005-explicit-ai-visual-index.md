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
descriptions, and stores timestamped annotations in SQLite. AI search embeds the
natural-language query and ranks stored annotations by cosine similarity.

Deterministic local search remains independent and continues to work without an
API key. Sampling interval and maximum frames are environment-configurable.

## Consequences

- a query can return both a clip and the moment to preview;
- users explicitly control when frames leave the machine and when API cost is incurred;
- short events can be missed when the sample interval is too large;
- a later worker/cloud implementation can reuse the annotation contract without
  changing the local search behavior.
