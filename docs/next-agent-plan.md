# Next-agent implementation plan

This file is the continuation handoff for the MediaIndex desktop MVP.

## Objective

Finish and integrate a documented Windows desktop application that indexes real
video folders, previews original clips, and searches sampled visual moments
without presenting MediaIndex as a browser-hosted product.

## Repository state

- Workspace: `C:\Users\grego\Documents\Playground\MediaIndex`
- Active branch: `feat/performance-cost-and-search-optimizations`
- Integration target: fetched `origin/main` at `79e2626`
- Local `main` is stale at `78cb453`. Merge commit `6ef9655` synchronized the
  feature branch with `origin/main`; the branch is 13 commits ahead and 0 behind
  before the final documentation correction (14 ahead after it is committed).
  Read
  [main-integration-review.md](main-integration-review.md) before changing Git
  history.
- Preserve user-owned `.idea/` files and unrelated working-tree changes.
- Treat `.github/agents/`, `.github/hooks/`, `.github/skills/`, and
  `.impeccable/` as local design/tooling material until their intended tracked
  scope is explicitly reviewed.
- Main-branch protection was deferred by the owner; do not retry or change it
  without a new request.

## Implemented in the current finishing pass

1. Updated OpenAI, Gemini, and Ollama model defaults and the visible catalog.
2. Made **Test connection** validate both vision and embedding capabilities.
3. Added provider-specific authentication, missing-model, and migration errors.
4. Migrated retired Gemini embedding selections to Gemini Embedding 2 without a
   silent retired-model fallback.
5. Kept provider/model embedding spaces isolated and documented reanalysis.
6. Grouped the library by each clip's real source folder.
7. Preserved per-video AI result grouping and collapsed adjacent duplicate
   moments.
8. Kept scanning and AI work off the Tauri UI thread with progress/cancellation.
9. Reworked the embedded desktop UI as an archive accession desk/contact sheet,
   with SVG controls and explicit Windows desktop labeling.
10. Added `PRODUCT.md`, `DESIGN.md`, provider compatibility, and integration
    review documentation.
11. Clarified that localhost/Vite is development infrastructure only; the
    product is the packaged Tauri app.

## First actions in the next session

1. Run `git status --short`, inspect every uncommitted diff, and avoid staging
   unrelated local tooling or IDE files.
2. Confirm no visual-QA fixture remains:
   `rg -n "visual-fixture|Autumn Campaign|fixture-a" desktop/src/main.ts`.
3. Authenticate `gh` for `Lanque/MediaIndex`, then audit live issues and pull
   requests before changing their state.
4. Fetch `origin/main` again immediately before publishing and merge any new
   commits without rewriting the existing issue-linked history.
5. Compare the final branch against `origin/main` by subsystem and update
   `docs/engineering-status.md` from live GitHub evidence.

## Automated verification

Run from the repository root unless a command says otherwise:

```powershell
.\.venv\Scripts\python.exe -m unittest discover -s tests -p "test_*.py"
cargo fmt --manifest-path desktop/src-tauri/Cargo.toml --check
cargo test --manifest-path desktop/src-tauri/Cargo.toml
Set-Location desktop
npm.cmd run build
npm.cmd run tauri build
Set-Location ..
git diff --check
```

After the frontend build, verify that the design provenance is present in the
production bundle:

```powershell
rg -n "538ed156" desktop/dist
```

## Packaged-app manual matrix

Use the executable or NSIS-installed app, not the Vite browser preview.

- select a small folder and a large nested footage folder;
- confirm the window remains responsive and folder groups populate;
- repeat the scan and confirm unchanged metadata/hashes are reused;
- search by filename and filters, then clear and repeat;
- preview clips with spaces and non-ASCII characters in their paths;
- open the original clip in the system player;
- restart and confirm the indexed library restores from SQLite;
- test connection for the selected provider/model pair;
- run bounded AI analysis, watch progress, cancel midway, and resume;
- search visible text, an action, an entity, and a general situation;
- confirm adjacent seconds from one event appear as one featured moment;
- change embedding model and confirm the UI requires reanalysis;
- verify no command-prompt window appears during FFmpeg/FFprobe work.

## GitHub reconciliation

When authenticated, list all open and closed issues/PRs and map the 14
post-`origin/main` commits to them. The foundation stack is already merged
through PR #39, so use one focused follow-up PR rather than recreating that
stack. The follow-up PR should include:

- linked issues and preserved commit traceability;
- a subsystem-oriented summary;
- exact automated test output;
- packaged installer path/artifact name;
- desktop screenshots;
- security/privacy statement for sampled cloud frames;
- explicit known limitations and deferred branch-protection state.

## Exit criteria

The task is complete when the packaged Windows app passes the manual matrix,
CI is green, documentation matches behavior, the GitHub issue/PR state is
reconciled from authenticated evidence, and the owner can merge through the
repository's normal review path.
