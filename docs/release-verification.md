# Release verification

Verification snapshot: 2026-09-03, branch
`feat/performance-cost-and-search-optimizations`.

## Automated results

| Check | Result |
| --- | --- |
| `python -m unittest discover -s tests -p "test_*.py"` | passed: 33 tests; infrastructure and migration checks passed |
| `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check` | passed |
| `cargo test --manifest-path desktop/src-tauri/Cargo.toml` | passed: 42 tests; 1 opt-in FFmpeg/provider-pipeline smoke test ignored |
| `npm.cmd run build` | passed: TypeScript and Vite production bundle |
| `npm.cmd run tauri build` | passed: Windows executable and NSIS installer |
| `git diff --check` | passed; Git emitted line-ending notices only |
| production provenance check | passed: design seed `538ed156` is present in `desktop/dist/index.html` |

The Python run emits an upstream Starlette deprecation warning about its
TestClient transport. The Rust commands emit a non-fatal warning that they could
not canonicalize `C:\Users\username`. Neither warning failed a check.

## Windows artifacts

| Artifact | Size | SHA-256 |
| --- | ---: | --- |
| `desktop/src-tauri/target/release/mediaindex.exe` | 15,184,896 bytes | `142D9FA7E988AC073D3B85F7373F9F6CE89A62996C249F9E0573C1B7580AE17E` |
| `desktop/src-tauri/target/release/bundle/nsis/MediaIndex_0.1.0_x64-setup.exe` | 3,928,799 bytes | `5A55FE26FA4ED244B762B288B2A4DAEBC3540482920B8EAF1C8C53760FF65259` |

These local artifacts are unsigned unless a release signing certificate is
configured. The installer is suitable for local testing; a public release
should be Authenticode-signed.

## Manual release gate

This automated pass does not claim the complete packaged-app manual matrix. Run
the installer and execute the scan, preview/open, restart restoration, provider
connection, bounded analysis, cancellation/resume, and AI-search scenarios in
[next-agent-plan.md](next-agent-plan.md) before publishing a release or merging
the integration branch.
