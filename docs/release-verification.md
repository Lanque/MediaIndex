# Release verification

Verification snapshot: 2026-09-08, on
`feat/performance-cost-development-2026-09-05`. This page records checks
completed during the v0.1.0 documentation and release review. Final merge,
Rust, Windows packaging, and installed-app evidence is intentionally left for
the release owner to add after the final code changes land.

## Automated results

| Check | Result |
| --- | --- |
| `python scripts/check_repository.py` | passed: 12 required repository files |
| `python -m unittest discover -s tests -p "test_*.py"` | passed: 33 tests on local Python 3.14.6 |
| `python scripts/check_migrations.py` | passed: 1 migration file |
| `python scripts/check_infra.py` | passed: Terraform scaffold markers and credential checks |
| `npm.cmd run build` (from `desktop`) | passed: TypeScript compilation and Vite production bundle |
| `git diff --check` | passed for the reviewed documentation changes |
| frontend AI-estimate race probes | passed in a limited Node/TypeScript AST and VM probe: reversed folder-response order, model-change invalidation, stale rejection suppression, and API-key changes that do not invalidate the auth-independent estimate |

The Python suite emitted the upstream Starlette deprecation warning about its
TestClient transport; it did not fail a test. The frontend race probes used
local plan IPC and render callbacks only. They made no provider requests and do
not replace a manual packaged-app check.

## Release scope

The checks above cover repository structure, Python contracts, migration and
infrastructure scaffolding, the TypeScript/Vite build, and the local estimate
response guards. They do not establish provider billing accuracy, gameplay
event recall, support for every codec, or a complete Windows user experience.
The v0.1.0 release is therefore labeled an experimental unsigned prerelease.

## Pending before stable publication

The release owner should add the evidence below after the final Rust and
packaging changes are merged:

- `cargo fmt --manifest-path desktop/src-tauri/Cargo.toml -- --check`;
- `cargo test --manifest-path desktop/src-tauri/Cargo.toml`, including the
  final pass/ignored counts;
- `npm.cmd run tauri build` on Windows, with the installer tied to the exact
  final `main` merge commit and the GitHub Actions run that produced it;
- installer SHA-256 and release-asset provenance, plus Authenticode signature
  status;
- the installed-app matrix: install and startup, folder scan, preview/open,
  restart restoration, provider connection, bounded analysis, cancellation
  and resume, and AI search.

Do not copy hashes or merge SHAs from an earlier local build into this page.
Record the final release asset values only after the main-branch CI artifact is
selected for publication. Stable public distribution also requires the signed
installer gate described in [security and cost](security-and-cost.md).
