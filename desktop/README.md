# MediaIndex desktop

This directory contains the Tauri 2 desktop shell and its TypeScript frontend.

## Prerequisites

Install the current Node.js LTS, Rust, and the platform dependencies required by Tauri. See the [official Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for the operating-system-specific list.

## Development

```bash
cd desktop
npm install
npm run tauri dev
```

The shell starts a Vite development server on `http://localhost:5173` and opens a native MediaIndex window.

## Production build

```bash
cd desktop
npm run build
```

The desktop release build is available with `npm run tauri build` after the platform-specific Tauri prerequisites are installed.

## Verification

Run the deterministic Rust suite with:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

An opt-in smoke test exercises real FFmpeg extraction plus the complete OpenAI
response, embedding, SQLite persistence, and AI search pipeline without sending
the clip to an external service. It uses a local HTTP stub and requires an
existing video:

```powershell
$env:MEDIAINDEX_SMOKE_VIDEO = "C:\path\to\clip.mp4"
cargo test --manifest-path src-tauri/Cargo.toml analyzes_real_video_through_openai_response_pipeline -- --ignored
```

## Current scope

The desktop shell now includes the first local scanner slice from issue [#3](https://github.com/Lanque/MediaIndex/issues/3):

- recursive discovery of configured media extensions;
- deterministic path ordering;
- SHA-256 content hashes computed locally;
- iterative traversal that skips symlinks and Windows junction/reparse points so a scan cannot recurse outside the selected root or overflow the call stack;
- warnings for unreadable directories or files instead of aborting the whole scan;
- pure change classification for new, unchanged, modified, moved, and deleted files.
- local FFprobe metadata extraction through a replaceable Rust interface;
- deterministic duration, size, container, codec, resolution, frame-rate, and timestamp fields;
- actionable metadata error state when FFprobe is unavailable, fails, or returns invalid JSON.
- a versioned SQLite local index where content hashes identify assets and paths identify locations;
- idempotent re-indexing with duplicate, moved, modified, deleted, and incomplete-scan handling.
- offline search across file names and technical metadata with folder, date, resolution, FPS, duration, and codec filters;
- local FFprobe metadata is collected during indexing and results can be sorted by name, duration, file size, modified date, or resolution in either direction;
- opening available original files from a result while clearly identifying unavailable paths;
- an embedded video preview for available clips, with a system-player fallback when the WebView cannot decode a codec;
- cached hashes and FFprobe metadata on repeat scans when path, size, and modification time are unchanged;
- a 500-result render cap so a large search result cannot freeze the desktop window;
- explicit AI analysis of sampled frames with timestamped descriptions and embeddings for natural-language search.
- restoration of the last successfully indexed library after an app restart, so existing clips and AI analysis remain immediately available.

The first scan hashes each discovered video locally so moved and modified files
can be detected reliably. Large footage folders can therefore take time during
the initial scan; later scans reuse unchanged hashes and cached metadata. The
original media is not uploaded.

### AI search

AI analysis is explicit because it sends sampled JPEG frames to the configured
AI provider and can consume time or API credits. Configure it in the app:

1. Open **AI connection** in the left sidebar.
2. Choose **Local (Ollama)**, **OpenAI (ChatGPT API)**, or **Google Gemini API**.
3. Enter the API key for a cloud provider, check the model names and base URL,
   then press **Test connection** and **Save settings**. OpenAI users can choose
   the fast GPT-5.6 Luna preset or the more detailed GPT-5.6 Terra preset.
   **Library context** can optionally list the project, franchise, setting, or
   possible fictional characters to help with an unfamiliar collection; it is
   treated as a hint and not as visual proof.
4. Select and index a folder, press **Analyze with AI**, and search with queries
   such as `Fortnite kill`, `enemy elimination`, or `victory`.

Settings, including a cloud API key, are stored in this computer's local app
storage and are not committed to the repository. **OpenAI (ChatGPT API)** means
the OpenAI developer API, not the ChatGPT website subscription.

For the local option, install and run [Ollama](https://ollama.com/), then make
the selected vision and embedding models available (for example `gemma4` and
`embeddinggemma`). The default endpoint is `http://127.0.0.1:11434`.

FFmpeg is resolved from the optional path in **AI connection**, then from the
`MEDIAINDEX_FFMPEG_PATH` environment variable, then from `PATH`. FFmpeg and
FFprobe child processes are created without a console window in Windows release
builds. Frame
extraction runs once per clip and embeddings are sent in batches. Cloud vision
requests contain up to eight sampled frames, sampled JPEGs are capped at 1280
pixels wide, and MediaIndex analyzes up to two clips concurrently. These bounds
avoid oversized parallel uploads while retaining high-detail HUD analysis;
local Ollama analysis stays sequential to protect local model resources.
Sampling defaults to one frame per five seconds and at most 120 frames per file;
for short, fast events such as a kill feed, set **Every (s)** to `2` or `1`.
The 0–100% progress bar shows the current phase and clip while analysis is
running.

The vision result stores entities, actions, setting, situation, readable
on-screen text, and a general scene description. It covers ordinary footage,
films, events, tutorials, travel, sports, performances, and gameplay rather than
depending on game-specific labels. AI Search combines embedding similarity with
exact/inflected keyword matching. Adjacent hits from the same video within
three seconds are presented as one best-scoring moment, so one event does not
appear separately at seconds 4, 5, and 6. AI search results are shown in
collapsible video groups: videos are ordered by their best match and each
video's moments are ordered by timestamp. If the provider, vision model, or
embedding model changes, run **Analyze with AI** again; incompatible provider
annotations are intentionally kept separate.

AI analysis runs in a background worker, so the desktop window remains
responsive while FFmpeg and network requests are in progress.

The environment variables remain available for automation and older launch
scripts (`MEDIAINDEX_AI_PROVIDER`, provider-specific API keys, model names,
base URLs, `MEDIAINDEX_FFMPEG_PATH`, `MEDIAINDEX_AI_SAMPLE_SECONDS`, and
`MEDIAINDEX_AI_MAX_FRAMES`). Optional collection context can be supplied with
`MEDIAINDEX_AI_CONTEXT`. The in-app configuration takes precedence.

Only sampled frames and their text/embedding results are sent for analysis; the
original video is not uploaded as a whole. Deterministic local search remains
available without an API key. See [docs/local-search.md](../docs/local-search.md)
and [docs/adr/0005-explicit-ai-visual-index.md](../docs/adr/0005-explicit-ai-visual-index.md)
for the detailed flow.

See [docs/local-index.md](../docs/local-index.md) for the schema and hashing policy and [docs/local-search.md](../docs/local-search.md) for the supported filters and usage example.
