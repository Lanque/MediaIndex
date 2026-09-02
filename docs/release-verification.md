# Release verification

Verification snapshot: 2026-09-03, branch
`feat/performance-cost-and-search-optimizations`.

## Automated results

| Check | Result |
| --- | --- |
| `python -m unittest discover -s tests -p "test_*.py"` | passed: 33 tests; infrastructure and migration checks passed |
| `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check` | passed |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml` | passed: 44 tests; 1 opt-in FFmpeg/provider-pipeline smoke test ignored in the default run |
| OpenAI/Gemini connection contracts | passed: Bearer auth, `x-goog-api-key`, model checks, Embedding 2 payload, and no API key in Gemini URLs |
| opt-in real-video pipeline smoke test | passed with a generated MP4, real FFmpeg extraction, local OpenAI HTTP stub, embeddings, SQLite persistence, and search; no provider credits used |
| `npm.cmd run build` | passed: TypeScript and Vite production bundle |
| `npm.cmd run tauri build` | passed: Windows executable and NSIS installer |
| packaged executable startup | passed: process stayed alive, Windows title was `MediaIndex`, and the process reported responsive |
| `git diff --check` | passed; Git emitted line-ending notices only |
| production provenance check | passed: design seed `538ed156` is present in `desktop/dist/index.html` |

The Python run emits an upstream Starlette deprecation warning about its
TestClient transport. The Rust commands emit a non-fatal warning that they could
not canonicalize `C:\Users\username`. Neither warning failed a check.

## Windows artifacts

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `desktop/src-tauri/target/release/mediaindex.exe` | 15,184,896 bytes | `9EF336C14947487EB0B3A8AAE2449CB004A3542B1BCEA5FC9D5046EBA2DC53A6` |
| `desktop/src-tauri/target/release/bundle/nsis/MediaIndex_0.1.0_x64-setup.exe` | 3,925,774 bytes | `33F04F84E7DA5AD8EC759714B7EDE31663411B0FB9B82027EBEC010D50708FD0` |

These local artifacts are unsigned unless a release signing certificate is
configured. The installer is suitable for local testing; a public release
should be Authenticode-signed.

## Manual release gate

This automated pass does not claim the complete packaged-app manual matrix. Run
the installer and execute the scan, preview/open, restart restoration, provider
connection, bounded analysis, cancellation/resume, and AI-search scenarios in
[next-agent-plan.md](next-agent-plan.md) before publishing a release or merging
the integration branch.
