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

The shell is a visual and runtime foundation only. Folder selection, scanning, FFprobe metadata, hashing, SQLite persistence, and local search are tracked in issues [#3](https://github.com/Lanque/MediaIndex/issues/3) through [#6](https://github.com/Lanque/MediaIndex/issues/6).
