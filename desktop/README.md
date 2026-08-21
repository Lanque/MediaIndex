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
- symlink skipping so a scan cannot follow a linked folder outside the selected root;
- warnings for unreadable directories or files instead of aborting the whole scan;
- pure change classification for new, unchanged, modified, moved, and deleted files.
- local FFprobe metadata extraction through a replaceable Rust interface;
- deterministic duration, size, container, codec, resolution, frame-rate, and timestamp fields;
- actionable metadata error state when FFprobe is unavailable, fails, or returns invalid JSON.

SQLite persistence and a searchable result list remain separate work in issues [#5](https://github.com/Lanque/MediaIndex/issues/5) and [#6](https://github.com/Lanque/MediaIndex/issues/6). The scanner and metadata interfaces are intentionally kept independent of persistence so they can be tested before the local schema is introduced.
