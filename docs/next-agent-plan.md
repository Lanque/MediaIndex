# Next-agent implementation plan

This file is the continuation handoff for the MediaIndex desktop MVP.

## Objective

Finish and integrate a documented Windows desktop application that indexes real
video folders, previews original clips, and searches sampled visual moments
without presenting MediaIndex as a browser-hosted product.

## Repository state

- Workspace: `C:\Users\grego\Documents\Playground\MediaIndex`
- Active branch: `main`
- Published branch: `origin/feat/performance-cost-and-search-optimizations`
- Integration baseline: fetched `origin/main` at `79e2626`
- Merge commit `8bdcc78` integrated the 19-commit feature history into local
  `main` after a final fetch confirmed the feature branch was 0 commits behind.
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
12. Scoped the main library and AI search to the explicitly selected root while
    keeping a separate cross-folder **Analyzed archive**.
13. Made per-root reconciliation preserve other folders and retained missing
    clips as discoverable saved AI results.
14. Replaced fixed timestamp suppression with context-aware time ranges and
    action/setting/situation boundaries while retaining changing dialogue and
    on-screen text inside the searchable range.
15. Added reanalysis replacement warnings, duration-based request estimates,
    a first-run estimate, measured per-model timing, live ETA, `gpt-4o-mini`,
    and legacy `o4-mini`.
16. Added optional timestamped OpenAI speech transcription using a compressed
    temporary audio track and included speech duration in analysis preflight.
17. Added native Gemini **Login with Google** using a user-owned Desktop OAuth
    client JSON, PKCE, a loopback callback, and memory-only access tokens.
18. Added a lazy saved-analysis inspector with complete descriptions, contextual
    ranges, labels, confidence, preview controls, and separate model histories;
    fixed reanalysis so it replaces only the active model instead of deleting
    other model results.

## First actions in the next session

1. Run `git status --short`, inspect every uncommitted diff, and avoid staging
   unrelated local tooling or IDE files.
2. Confirm no visual-QA fixture remains:
   `rg -n "visual-fixture|Autumn Campaign|fixture-a" desktop/src/main.ts`.
3. Authenticate `gh` for `Lanque/MediaIndex`, then audit live issues and pull
   requests before changing their state.
4. Confirm local and remote `main` point at the completed integration history.
5. Update `docs/engineering-status.md` from live GitHub issue/PR evidence when
   CLI authentication is restored.

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
- select a second folder and confirm **Current folder** contains no clips from
  the first while **Analyzed archive** retains earlier analyzed clips;
- repeat the scan and confirm unchanged metadata/hashes are reused;
- search by filename and filters, then clear and repeat;
- preview clips with spaces and non-ASCII characters in their paths;
- open the original clip in the system player;
- restart and confirm the indexed library restores from SQLite;
- test connection for the selected provider/model pair;
- when a Google Desktop OAuth client is available, use **Login with Google**,
  verify the quota project, test Gemini, disconnect, and confirm the token is
  removed; otherwise test the AI Studio API-key path;
- run bounded AI analysis, watch progress, cancel midway, and resume;
- search visible text, spoken dialogue, an action, an entity, and a general situation;
- open **View AI analysis** from ordinary and AI-search cards, verify long text
  is complete, and preview multiple saved contextual ranges;
- confirm adjacent seconds from one event appear as one time range and a real
  action/setting/situation change starts a new range while changing dialogue
  remains searchable within it;
- confirm repeated analysis warns before replacing same-model moments and the
  preflight/live progress show a first-run or calibrated estimate;
- change embedding model and confirm the UI requires reanalysis;
- verify no command-prompt window appears during FFmpeg/FFprobe work.

## GitHub reconciliation

When authenticated, list all open and closed issues/PRs and map the 19 integrated
feature commits to them. The foundation stack is already merged through PR #39,
so do not recreate that stack. Future release notes should include:

- linked issues and preserved commit traceability;
- a subsystem-oriented summary;
- exact automated test output;
- packaged installer path/artifact name;
- desktop screenshots;
- security/privacy statement for sampled cloud frames, temporary speech audio,
  and Gemini memory-only OAuth credentials;
- explicit known limitations and deferred branch-protection state.

## Exit criteria

The release is ready to publish when the packaged Windows app passes the manual
matrix, CI is green, documentation matches behavior, and the GitHub issue/PR
state is reconciled from authenticated evidence.
