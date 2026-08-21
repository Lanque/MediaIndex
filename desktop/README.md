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
npm run tauri build
```

On Windows this creates the release executable and a current-user NSIS installer
under `src-tauri/target/release/bundle/nsis/`. The installer does not require
administrator privileges. Pull-request CI builds the same installer and keeps
it as the `mediaindex-windows-installer` workflow artifact for 14 days.

Local and pull-request installers are unsigned unless a Windows Authenticode
certificate is configured for the release environment. They are suitable for
local testing, but a public download should be code-signed before release to
give Windows a verifiable publisher identity. Follow the
[official Tauri Windows signing guide](https://v2.tauri.app/distribute/sign/windows/)
without committing certificate material or passwords.

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
- per-file preview authorization: the WebView can load only an active video that the Rust backend has verified in the local SQLite index;
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
   the budget GPT-5.6 Luna preset or the more detailed GPT-5.6 Terra preset.
   Luna is the default for cost-sensitive analysis; Terra is roughly ten times
   Luna's model token price and should be reserved for difficult footage. See
   the [official OpenAI model catalog](https://developers.openai.com/api/docs/models)
   for current prices.
   **Library context** can optionally list the project, franchise, setting, or
   possible fictional characters to help with an unfamiliar collection; it is
   treated as a hint and not as visual proof.
4. Select and index a folder, press **Analyze with AI**, and search with queries
   such as `Fortnite kill`, `enemy elimination`, or `victory`.

Non-secret settings are stored in this computer's local app storage and are not
committed to the repository. A cloud API key is kept only in the current app
session; older persisted keys are removed from local storage when the updated
app starts. **OpenAI (ChatGPT API)** means the OpenAI developer API, not the
ChatGPT website subscription.

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
Sampling defaults to one frame per five seconds. New OpenAI configurations use
a cost-conscious maximum of 60 frames per file; other provider defaults remain
120. For short, fast events such as a kill feed, set **Every (s)** to `2` or `1`.
The 0–100% progress bar shows the current phase and clip while analysis is
running.

**Analyze with AI** skips clips that already have annotations for the selected
provider and models, preventing repeated button presses from spending credits
again. Enable **Reanalyze existing clips** only for an intentional one-time
refresh after changing sampling or library context; the checkbox resets after
the run.

The vision result stores entities, actions, setting, situation, readable
on-screen text, and a general scene description. It covers ordinary footage,
films, events, tutorials, travel, sports, performances, and gameplay rather than
depending on game-specific labels. AI Search combines embedding similarity with
exact/inflected keyword matching. Adjacent hits from the same video within
three seconds are presented as one best-scoring moment, so one event does not
appear separately at seconds 4, 5, and 6. AI search results are shown in
visual video cards with a locally generated best-moment thumbnail. Clicking the
thumbnail starts Preview at that timestamp; additional moments remain in a
compact chronological list. **Focused** relevance is the default and limits
weak results and repeated moments with both an adaptive window and an absolute
floor. Exact visible text or labels can retain a result while generic semantic
similarity alone is rejected. **Balanced** and **Broad** progressively expand
discovery. If the provider, vision model, or
embedding model changes, run **Analyze with AI** again; incompatible provider
annotations are intentionally kept separate.

AI analysis runs in a background worker, so the desktop window remains
responsive while FFmpeg and network requests are in progress. While analysis
is running, **Analyze with AI** becomes **Stop analysis**. Stopping prevents new
files and frame batches from starting; a request that has already been sent may
finish before the run stops. Completed clips are kept and remain searchable,
and a later analysis continues with clips that are still missing for the active
provider/model namespace. A second analysis cannot start while one is active.

The production WebView uses a restrictive Content Security Policy. Local video
paths are not exposed through a whole-disk asset scope: clicking Preview asks
the backend to validate and authorize that one indexed file for the current app
run.

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
