<p align="center">
  <img src="desktop/src-tauri/icons/icon.svg" alt="MediaIndex icon" width="96">
</p>

<h1 align="center">MediaIndex</h1>

<p align="center">
  Find the right moment in local footage without uploading your archive.
</p>

<p align="center">
  <a href="https://github.com/Lanque/MediaIndex/actions/workflows/ci.yml"><img src="https://github.com/Lanque/MediaIndex/actions/workflows/ci.yml/badge.svg" alt="CI status"></a>
  <a href="https://github.com/Lanque/MediaIndex/releases/tag/v0.1.0"><img src="https://img.shields.io/badge/release-v0.1.0%20experimental-F5A623" alt="v0.1.0 experimental prerelease"></a>
  <img src="https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white" alt="Windows">
  <img src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white" alt="Tauri 2">
</p>

<p align="center">
  <a href="#download">Download</a> ·
  <a href="#what-it-does">What it does</a> ·
  <a href="#quick-start">Quick start</a> ·
  <a href="#development">Development</a> ·
  <a href="#documentation">Documentation</a>
</p>

MediaIndex is a Windows desktop application for finding clips and moments in
large local video folders. It discovers footage where it already lives,
extracts metadata locally, and keeps the working index in SQLite. Deterministic
search and preview work without an account or network connection. An optional
AI index adds sampled visual descriptions and semantic search through OpenAI,
Google Gemini, or a local Ollama installation.

## Release status

`v0.1.0` is an experimental prerelease for Windows. The installer is unsigned,
so Windows cannot show a verified publisher identity. Use it for evaluation and
keep a backup of the local index before upgrading. A signed stable release and
the complete installed-app verification matrix remain future release work.

## Download

Download the [v0.1.0 experimental prerelease](https://github.com/Lanque/MediaIndex/releases/tag/v0.1.0)
from GitHub. The release page is the source for the Windows installer and its
checksums.

## What it does

- **Scans locally.** Select a folder and MediaIndex recursively discovers video
  files, computes complete SHA-256 content identities, and reads technical
  metadata through FFprobe. The original files stay in their existing folders.
- **Indexes in SQLite.** A machine-local SQLite database tracks content hashes,
  paths, metadata, availability, and saved AI results. A path is a location;
  copied footage can share one content identity.
- **Searches and previews offline.** Search filenames and technical metadata,
  filter by folder, date, resolution, frame rate, duration, or codec, then
  preview or open the original clip. The local index does not require cloud
  services.
- **Adds optional visual search.** An explicit **Analyze with AI** run samples
  frames and stores timestamped descriptions and embeddings locally. Choose
  OpenAI, Google Gemini, or Local (Ollama); provider and model histories remain
  separate.
- **Shows scope before cloud work.** After **Select Footage Folder** finishes
  its local scan, the app shows an **AI cost preview** with unique clips,
  sampled-frame and vision-request counts, optional speech duration, and the
  configured estimate. The preview is computed locally and does not need an
  API key or contact a provider. Cloud analysis starts only after explicit
  confirmation.
- **Keeps analysis bounded and recoverable.** Optional remote budgets,
  per-request accounting, resumable vision checkpoints, and partial/failed
  coverage states keep an interrupted run from silently discarding completed
  work.

### AI data movement and cost

| Provider | What leaves the computer | Setup |
| --- | --- | --- |
| Local (Ollama) | When the endpoint is local, frames, prompts, generated descriptions, context, embeddings, and AI Search query text stay on the computer. A remote Ollama endpoint receives the data sent to that endpoint. | Ollama and the selected vision/embedding models installed locally. |
| OpenAI API | Sampled JPEG frames and analysis prompts/context go to vision; generated descriptions, context, and AI Search query text go to the embedding API. Optional timestamped transcription sends a temporary compressed speech track for the analyzed span. The original video is not uploaded as a whole. | OpenAI developer API key. A ChatGPT subscription is not an API key. |
| Google Gemini API | Sampled JPEG frames and analysis prompts/context go to vision; generated descriptions, context, and AI Search query text go to the embedding API. The original video is not uploaded as a whole. | Google AI Studio API key or the supported Google Desktop OAuth flow. |

The estimate is a planning heuristic based on the selected models, sampling
settings, media metadata, and the checked pricing catalog. It is not a quote
and does not guarantee the amount on a provider invoice. Unknown model pricing
is shown as unknown rather than treated as zero; local CPU/GPU time and
electricity are outside the API estimate.

### Coverage limitation

AI analysis samples a prefix of each video. With the default OpenAI settings
of one frame every five seconds and a maximum of 60 frames, a long clip covers
roughly its first five minutes (`60 × 5 s`), rather than the whole video. Fast
gameplay events such as a death, kill, or attack can fall between samples or be
described too coarsely. For those moments, lower **Sample every (s)** to `2` or
`1` and intentionally re-analyze the relevant clips. Denser sampling can capture
more short-lived events, but does not guarantee that a provider will recognize
or label them. The resulting coverage is still bounded by the configured frame
limit.

## Requirements

The v0.1.0 desktop release targets Windows and requires:

- the Microsoft Edge **WebView2 Runtime** for the Tauri desktop shell;
- **FFmpeg** and **FFprobe** available on `PATH`. For AI analysis, an FFmpeg
  executable can instead be selected under **AI connection** or supplied as
  `MEDIAINDEX_FFMPEG_PATH`; the related FFprobe executable is resolved beside
  it when possible. Metadata scanning can use `MEDIAINDEX_FFPROBE_PATH` for an
  explicit FFprobe path;
- the Rust stable toolchain with the MSVC target, Visual Studio C++ build tools,
  and the Windows SDK when developing or building the desktop client;
- an OpenAI or Gemini credential only if you choose that cloud provider. Local
  scanning, deterministic search, preview, and the local cost estimate do not
  require an API key;
- Ollama and its selected models only if you choose Local (Ollama).

## Quick start

1. Install the requirements above and launch MediaIndex.
2. Choose **Select Footage Folder**. The initial scan hashes files and reads
   metadata locally; wait for indexing to finish.
3. Search the indexed folder or use the filters, then preview or open a result.
4. For visual search, open **AI connection**, select a provider, and configure
   its models. After the scan, review the local **AI cost preview** before
   pressing **Analyze with AI**.
5. Search indexed moments with a description such as `red car at night`,
   `person in a forest`, or `blue vehicle by a lake`.

The index is stored in the Tauri app-local data directory as
`mediaindex.sqlite3`. Before installing a newer build, close MediaIndex and
copy the file to a dated backup. On Windows it is typically at
`%LOCALAPPDATA%\ee.lanque.mediaindex\mediaindex.sqlite3`. Keep the original
footage backed up separately; MediaIndex does not replace the source files.

## Development

The repository uses Python for the API and repository checks, and Node.js,
Rust, and Tauri for the Windows desktop client. The CI workflow currently uses
Python 3.12 and Node.js 22.

From PowerShell at the repository root:

```powershell
git clone https://github.com/Lanque/MediaIndex.git
cd MediaIndex

python -m venv .venv
.\.venv\Scripts\Activate.ps1
python -m pip install -r api\requirements-dev.txt

python scripts\check_repository.py
python -m unittest discover -s tests -p "test_*.py"
python scripts\check_migrations.py
python scripts\check_infra.py

cd desktop
npm.cmd ci
npm.cmd run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo test --manifest-path src-tauri/Cargo.toml
npm.cmd run tauri dev
```

To create a Windows installer locally, run this from `desktop` after the
frontend build:

```powershell
npm.cmd run tauri build
```

Local and CI installers are unsigned unless the release environment provides a
Windows Authenticode certificate. Do not commit certificate material or
passwords.

## Repository map

| Path | Purpose |
| --- | --- |
| `desktop/` | Tauri 2 shell, TypeScript interface, Rust scanner, index, and AI worker |
| `api/` | FastAPI reference boundary for the versioned cloud sync contract |
| `worker/` | Asynchronous processing worker foundation |
| `shared/` | Shared API contracts and schemas |
| `migrations/` | PostgreSQL schema and row-level security migration |
| `infra/terraform/` | Guarded AWS infrastructure scaffold |
| `tests/` | Repository, API, migration, infrastructure, security, and worker checks |
| `docs/` | Architecture, local-index behavior, provider notes, release evidence, and ADRs |

## Documentation

- [Local index and content identity](docs/local-index.md) — SQLite schema,
  hashing, re-indexing, and saved AI data.
- [Local search and clip opening](docs/local-search.md) — offline filters,
  previews, AI search, sampling, and cost confirmation.
- [AI provider compatibility](docs/ai-provider-compatibility.md) — supported
  providers, models, authentication, and migration behavior.
- [Security and cost guardrails](docs/security-and-cost.md) — data movement,
  credentials, budgets, request accounting, and release posture.
- [Release verification](docs/release-verification.md) — current checks and
  the evidence still needed for a public stable release.
- [v0.1.0 release notes](docs/releases/v0.1.0.md) — experimental prerelease
  scope and known limitations.
- [Architecture](docs/architecture.md) — ownership boundaries and the planned
  local-to-cloud path.
- [Development workflow](docs/development-workflow.md) — issue, branch,
  commit, pull-request, and review conventions.
- [Security policy](SECURITY.md) — private vulnerability reporting and
  development rules.

## Cloud scope

The API, worker, PostgreSQL migration, and Terraform files describe a later
cloud-sync path. They are scaffolding only: this repository has not applied AWS
infrastructure, does not ship a hosted service, and does not require cloud
deployment for the local desktop workflow. Original footage remains local by
default.
