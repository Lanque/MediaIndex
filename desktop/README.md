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

Packaging is intentionally disabled until application icons and the first user workflow are in place.

## Current scope

The desktop shell now includes the first local scanner slice from issue [#3](https://github.com/Lanque/MediaIndex/issues/3):

- recursive discovery of configured media extensions;
- deterministic path ordering;
- SHA-256 content hashes computed locally;
- symlink skipping so a scan cannot follow a linked folder outside the selected root;
- warnings for unreadable directories or files instead of aborting the whole scan;
- pure change classification for new, unchanged, modified, moved, and deleted files.

SQLite persistence, FFprobe metadata, and a searchable result list remain separate work in issues [#4](https://github.com/Lanque/MediaIndex/issues/4) through [#6](https://github.com/Lanque/MediaIndex/issues/6). The change classifier is intentionally kept independent of persistence so it can be tested before the local schema is introduced in issue [#5](https://github.com/Lanque/MediaIndex/issues/5).
