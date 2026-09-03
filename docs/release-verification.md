# Release verification

Verification snapshot: 2026-09-03, branch
`feat/performance-cost-and-search-optimizations`.

## Automated results

| Check | Result |
| --- | --- |
| `python -m unittest discover -s tests -p "test_*.py"` | passed: 33 tests; infrastructure and migration checks passed |
| `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check` | passed |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml` | passed: 52 tests; 1 opt-in FFmpeg/provider-pipeline smoke test ignored in the default run |
| OpenAI/Gemini connection contracts | passed: Bearer auth, `x-goog-api-key`, model checks, Embedding 2 payload, and no API key in Gemini URLs |
| opt-in real-video pipeline smoke test | passed with a generated MP4, real FFmpeg extraction, local OpenAI HTTP stub, embeddings, SQLite persistence, and search; no provider credits used |
| `npm.cmd run build` | passed: TypeScript and Vite production bundle |
| `npm.cmd run tauri build` | passed: Windows executable and NSIS installer |
| packaged executable startup | passed: the final release process stayed alive and reported responsive before clean shutdown |
| `git diff --check` | passed; Git emitted line-ending notices only |
| production provenance check | passed: design seed `538ed156` is present in `desktop/dist/index.html` |

The Python run emits an upstream Starlette deprecation warning about its
TestClient transport. The Rust commands emit a non-fatal warning that they could
not canonicalize `C:\Users\grego`. Neither warning failed a check.

## Windows artifacts

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `desktop/src-tauri/target/release/mediaindex.exe` | 15,260,160 bytes | `60988FC17540200B3CAE895AB4D1E25CAD205255B4BC4C0832A2E850B39BFE6E` |
| `desktop/src-tauri/target/release/bundle/nsis/MediaIndex_0.1.0_x64-setup.exe` | 3,937,846 bytes | `B36479841F9D11C40FEFFC3FDA1776147F8198FC801273480380E907CB25A0AB` |

These local artifacts are unsigned unless a release signing certificate is
configured. The installer is suitable for local testing; a public release
should be Authenticode-signed.

## Manual release gate

This automated pass does not claim the complete packaged-app manual matrix. Run
the installer and execute the scan, preview/open, restart restoration, provider
connection, bounded analysis, cancellation/resume, and AI-search scenarios in
[next-agent-plan.md](next-agent-plan.md) before publishing a release or merging
the integration branch.
