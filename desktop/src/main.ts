import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./styles.css";

type IndexReport = {
  active_file_count: number;
  changes: Array<{ kind: string; path: string; content_hash?: string }>;
  warnings: Array<{ path: string; message: string }>;
};

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("MediaIndex root element was not found.");
}

app.innerHTML = `
  <main class="shell">
    <header class="topbar">
      <div>
        <p class="eyebrow">LOCAL-FIRST MEDIA INDEX</p>
        <h1>Find the right shot.</h1>
        <p class="lede">
          Start with a local folder. MediaIndex will keep the original footage
          where it already lives.
        </p>
      </div>
      <button class="primary-button" id="select-folder" type="button">
        Select footage folder
      </button>
    </header>

    <section class="workspace" aria-label="Media library">
      <aside class="sidebar">
        <p class="section-label">Library</p>
        <div class="library-card">
          <span class="status-dot" aria-hidden="true"></span>
          <div>
            <strong id="library-status">No folder indexed</strong>
            <span id="library-path">Choose a local folder to begin</span>
          </div>
        </div>
        <p class="section-label">Filters</p>
        <div class="filter-placeholder">
          <span>Metadata filters arrive with the local scanner.</span>
        </div>
      </aside>

      <section class="content-panel">
        <div class="panel-heading">
          <div>
            <p class="section-label">Local library</p>
            <h2>Ready when you are</h2>
          </div>
          <span class="count-badge" id="clip-count">0 clips</span>
        </div>
        <div class="empty-state">
          <div class="empty-icon" aria-hidden="true">⌁</div>
          <h3>Your footage stays on your machine</h3>
          <p>
            The next step is the local scanner: discovery, FFprobe metadata,
            content hashing, and a searchable SQLite index.
          </p>
          <button class="secondary-button" id="learn-more" type="button">
            View the MVP plan
          </button>
        </div>
      </section>
    </section>
  </main>
`;

const selectFolderButton = document.querySelector<HTMLButtonElement>("#select-folder");
const libraryStatus = document.querySelector<HTMLElement>("#library-status");
const libraryPath = document.querySelector<HTMLElement>("#library-path");
const clipCount = document.querySelector<HTMLElement>("#clip-count");

selectFolderButton?.addEventListener("click", async () => {
  const selected = await open({
    directory: true,
    multiple: false,
    title: "Select footage folder",
  });

  if (typeof selected !== "string") {
    return;
  }

  selectFolderButton.disabled = true;
  if (libraryStatus) libraryStatus.textContent = "Scanning folder…";
  if (libraryPath) libraryPath.textContent = selected;

  try {
    const report = await invoke<IndexReport>("index_media_folder", { path: selected });
    if (libraryStatus) {
      libraryStatus.textContent = report.warnings.length === 0 ? "Folder indexed" : "Folder indexed with warnings";
    }
    if (clipCount) clipCount.textContent = `${report.active_file_count} clips`;
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Scan failed";
    if (libraryPath) libraryPath.textContent = String(error);
  } finally {
    selectFolderButton.disabled = false;
  }
});

document.querySelector<HTMLButtonElement>("#learn-more")?.addEventListener(
  "click",
  () => {
    window.alert("See docs/project-plan.md for the current MVP scope.");
  },
);
