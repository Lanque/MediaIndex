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
- a 500-result render cap so a large search result cannot freeze the desktop window.

The first scan hashes each discovered video locally so moved and modified files
can be detected reliably. Large footage folders can therefore take time during
the initial scan; later scans reuse unchanged hashes and cached metadata. The
original media is not uploaded.

See [docs/local-index.md](../docs/local-index.md) for the schema and hashing policy and [docs/local-search.md](../docs/local-search.md) for the supported filters and usage example.
