# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

Delivery surface: Windows desktop application packaged with Tauri. A browser-hosted product is not in scope; “web” describes only the embedded WebView interface layer used by the desktop shell.

## Users

Primary users are video creators, editors, and players who have many clips on a Windows computer and need to find a useful moment without opening every file manually. The initial validated usage is a local footage folder that may include gameplay captures as well as ordinary video.

## Product Purpose

MediaIndex indexes video where it already lives, lets the user search and sort the local library, and can create an optional visual AI index from sampled frames. Success means a user can choose a folder, keep working while it is indexed, find a relevant video or moment in plain language, preview it at the matching timestamp, and open the original file without uploading the full archive.

## Positioning

MediaIndex is a local-first footage finder rather than a video host or chatbot. Original media remains the user's source of truth on disk; deterministic metadata search works offline; and only explicitly requested sampled frames are sent to a configured AI provider for visual search.

## Operating Context

The product is a Tauri 2 desktop application with a TypeScript/Vite interface and a Rust backend. A user selects a local directory, scans and filters indexed clips, configures either OpenAI, Google Gemini, or local Ollama analysis, explicitly starts analysis, searches indexed moments, and previews or opens the original video. The first supported distribution target is Windows.

## Capabilities and Constraints

- Local recursive discovery, FFprobe metadata extraction, content hashing, SQLite indexing, metadata filters, sorting, preview, and original-file opening are baseline capabilities.
- Visual search stores timestamped descriptions and embeddings locally. Provider/model namespaces stay separate so incompatible vectors are not mixed.
- API keys are session-only secrets and must never be committed, logged, or silently moved between providers.
- Cloud analysis must show its scope before spending credits, remain cancellable, keep the UI responsive, and preserve completed work after partial failure.
- Large libraries must use bounded concurrency and bounded rendering. The original videos are not uploaded as whole files.
- The cloud sync and worker architecture exists as a documented later layer; the local desktop workflow must remain useful without it.
- Current model availability, pricing, quotas, and free-tier claims are volatile and require dated links to provider documentation instead of unsupported promises.
- Open decision: macOS and Linux packaging are not yet validated release targets.

## Brand Commitments

The product name is MediaIndex. Product copy is direct, factual, and operational: it names what will happen, what leaves the machine, what can cost money, and how to recover from a failure. The user has explicitly rejected generic AI-generated dashboard styling; the visual system must avoid decorative gradients, glow, excessive pills, nested cards, ornamental AI language, and redundant labels.

## Evidence on Hand

- The runnable desktop implementation and its Rust tests are under `desktop/`.
- Local API stubs exercise OpenAI request and response contracts without exposing or spending a real key.
- Architecture decisions, security boundaries, and the implementation roadmap are under `docs/`.
- A real local video can be supplied through `MEDIAINDEX_SMOKE_VIDEO` for an opt-in end-to-end FFmpeg smoke test.
- No public customer testimonials, audited performance benchmarks, guaranteed recognition rates, or provider availability guarantees are on hand; future product copy must not invent them.

## Product Principles

1. Keep original footage local and make data movement explicit.
2. Make ordinary metadata search and clip access dependable before adding AI complexity.
3. Treat AI analysis as a visible, bounded, resumable job with honest cost and failure states.
4. Organize results around videos and useful moments, not raw model output.
5. Prefer verifiable product truth and observable behavior over marketing claims.
