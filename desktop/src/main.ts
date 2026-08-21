import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import "./styles.css";

type IndexReport = {
  active_file_count: number;
  changes: Array<{ kind: string; path: string; content_hash?: string }>;
  warnings: Array<{ path: string; message: string }>;
};

type SearchFilters = {
  keyword?: string;
  folder?: string;
  date_from_unix_ms?: number;
  date_to_unix_ms?: number;
  resolution?: string;
  frame_rate?: string;
  min_duration_ms?: number;
  max_duration_ms?: number;
  codec?: string;
  sort_by: "name" | "duration" | "size" | "modified" | "resolution";
  sort_direction: "asc" | "desc";
};

type SearchResult = {
  path: string;
  content_hash: string;
  size_bytes: number;
  modified_unix_ms: number | null;
  status: "ACTIVE" | "MISSING";
  available: boolean;
  metadata: {
    duration_ms: number | null;
    container: string | null;
    video_codec: string | null;
    audio_codec: string | null;
    width: number | null;
    height: number | null;
    frame_rate: string | null;
  } | null;
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
        <form class="filter-form" id="filter-form">
          <label>Folder<input id="filter-folder" name="folder" placeholder="day-one" /></label>
          <label>Date from<input id="filter-date-from" name="date-from" type="date" /></label>
          <label>Date to<input id="filter-date-to" name="date-to" type="date" /></label>
          <label>Resolution<input id="filter-resolution" name="resolution" placeholder="1920x1080" /></label>
          <label>FPS<input id="filter-frame-rate" name="frame-rate" placeholder="29.97" /></label>
          <label>Duration min (s)<input id="filter-duration-min" name="duration-min" min="0" type="number" /></label>
          <label>Duration max (s)<input id="filter-duration-max" name="duration-max" min="0" type="number" /></label>
          <label>Codec<input id="filter-codec" name="codec" placeholder="h264" /></label>
          <button class="secondary-button" type="submit">Apply filters</button>
        </form>
      </aside>

      <section class="content-panel">
        <div class="panel-heading">
          <div>
            <p class="section-label">Local library</p>
            <h2>Ready when you are</h2>
          </div>
          <span class="count-badge" id="clip-count">0 clips</span>
        </div>
        <form class="search-form" id="search-form">
          <input id="search-input" name="keyword" placeholder="Search file names and technical metadata" />
          <select id="sort-by" aria-label="Sort results">
            <option value="name">Name</option>
            <option value="duration">Duration</option>
            <option value="size">File size</option>
            <option value="modified">Modified date</option>
            <option value="resolution">Resolution</option>
          </select>
          <select id="sort-direction" aria-label="Sort direction">
            <option value="asc">Ascending</option>
            <option value="desc">Descending</option>
          </select>
          <button class="secondary-button" type="submit">Search</button>
        </form>
        <div class="empty-state" id="empty-state">
          <div class="empty-icon" aria-hidden="true">⌁</div>
          <h3>Your footage stays on your machine</h3>
          <p>
            Select a folder to scan videos locally. MediaIndex reads technical
            metadata, stores it in SQLite, and lets you search and sort without
            uploading the original footage.
          </p>
          <button class="secondary-button" id="learn-more" type="button">
            View the MVP plan
          </button>
        </div>
        <div class="result-list" id="result-list" hidden></div>
      </section>
    </section>
  </main>
`;

const selectFolderButton = document.querySelector<HTMLButtonElement>("#select-folder");
const libraryStatus = document.querySelector<HTMLElement>("#library-status");
const libraryPath = document.querySelector<HTMLElement>("#library-path");
const clipCount = document.querySelector<HTMLElement>("#clip-count");
const emptyState = document.querySelector<HTMLElement>("#empty-state");
const resultList = document.querySelector<HTMLElement>("#result-list");

function escapeHtml(value: string): string {
  return value.replace(/[&<>'"]/g, (character) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    "'": "&#39;",
    '"': "&quot;",
  })[character] ?? character);
}

function dateToUnixMs(value: string): number | undefined {
  if (!value) return undefined;
  const timestamp = Date.parse(`${value}T00:00:00Z`);
  return Number.isNaN(timestamp) ? undefined : timestamp;
}

function formatBytes(value: number): string {
  if (value < 1024) return `${value} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let amount = value;
  let unit = "B";
  for (const nextUnit of units) {
    amount /= 1024;
    unit = nextUnit;
    if (amount < 1024) break;
  }
  return `${amount.toFixed(amount >= 10 ? 0 : 1)} ${unit}`;
}

function formatDuration(durationMs: number | null | undefined): string {
  if (durationMs == null) return "duration unavailable";
  const totalSeconds = Math.round(durationMs / 1000);
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`
    : `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function numberToMs(value: string): number | undefined {
  if (!value) return undefined;
  const seconds = Number(value);
  return Number.isFinite(seconds) && seconds >= 0 ? Math.round(seconds * 1000) : undefined;
}

function readFilters(): SearchFilters {
  const value = (id: string) => document.querySelector<HTMLInputElement>(id)?.value.trim() ?? "";
  const sortBy = document.querySelector<HTMLSelectElement>("#sort-by")?.value ?? "name";
  const sortDirection = document.querySelector<HTMLSelectElement>("#sort-direction")?.value ?? "asc";
  return {
    keyword: value("#search-input") || undefined,
    folder: value("#filter-folder") || undefined,
    date_from_unix_ms: dateToUnixMs(value("#filter-date-from")),
    date_to_unix_ms: dateToUnixMs(value("#filter-date-to")),
    resolution: value("#filter-resolution") || undefined,
    frame_rate: value("#filter-frame-rate") || undefined,
    min_duration_ms: numberToMs(value("#filter-duration-min")),
    max_duration_ms: numberToMs(value("#filter-duration-max")),
    codec: value("#filter-codec") || undefined,
    sort_by: sortBy as SearchFilters["sort_by"],
    sort_direction: sortDirection as SearchFilters["sort_direction"],
  };
}

function renderResults(results: SearchResult[]): void {
  if (!resultList || !emptyState) return;
  if (results.length === 0) {
    resultList.hidden = true;
    emptyState.hidden = false;
    return;
  }
  emptyState.hidden = true;
  resultList.hidden = false;
  resultList.innerHTML = results.map((result, index) => {
    const metadata = result.metadata;
    const details = metadata
      ? `${formatDuration(metadata.duration_ms)} · ${formatBytes(result.size_bytes)} · ${metadata.width ?? "?"}×${metadata.height ?? "?"} · ${metadata.frame_rate ?? "?"} fps · ${metadata.video_codec ?? "?"}`
      : "Technical metadata unavailable";
    const status = result.available ? "Available" : "Unavailable — rescan or restore this path";
    return `<article class="result-card ${result.available ? "" : "result-card-unavailable"}">
      <div>
        <p class="result-index">${String(index + 1).padStart(2, "0")}</p>
        <h3>${escapeHtml(result.path.split(/[\\/]/).pop() ?? result.path)}</h3>
        <p>${escapeHtml(result.path)}</p>
        <span>${escapeHtml(details)} · ${escapeHtml(status)}</span>
      </div>
      <button class="secondary-button open-result" data-path="${escapeHtml(result.path)}" ${result.available ? "" : "disabled"}>Open</button>
    </article>`;
  }).join("");
  resultList.querySelectorAll<HTMLButtonElement>(".open-result").forEach((button) => {
    button.addEventListener("click", async () => {
      await openPath(button.dataset.path ?? "");
    });
  });
}

async function searchLibrary(): Promise<void> {
  try {
    const results = await invoke<SearchResult[]>("search_media", { query: readFilters() });
    renderResults(results);
    if (libraryStatus) libraryStatus.textContent = `${results.length} matching clips`;
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Search failed";
    if (libraryPath) libraryPath.textContent = String(error);
  }
}

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
    await searchLibrary();
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Scan failed";
    if (libraryPath) libraryPath.textContent = String(error);
  } finally {
    selectFolderButton.disabled = false;
  }
});

document.querySelector<HTMLFormElement>("#search-form")?.addEventListener("submit", (event) => {
  event.preventDefault();
  void searchLibrary();
});

document.querySelector<HTMLFormElement>("#filter-form")?.addEventListener("submit", (event) => {
  event.preventDefault();
  void searchLibrary();
});

document.querySelector<HTMLButtonElement>("#learn-more")?.addEventListener(
  "click",
  () => {
    window.alert("See docs/project-plan.md for the current MVP scope.");
  },
);
