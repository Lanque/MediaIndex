import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
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
  timestamp_ms?: number;
  ai_description?: string;
  match_score?: number;
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

type AiIndexReport = {
  analyzed_file_count: number;
  annotation_count: number;
  warnings: Array<{ path: string; message: string }>;
};

type AiSearchResult = {
  path: string;
  content_hash: string;
  timestamp_ms: number;
  score: number;
  description: string;
  labels: string[];
  available: boolean;
};

const MAX_RENDERED_RESULTS = 500;

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
      <div class="topbar-actions">
        <button class="primary-button" id="select-folder" type="button">
          Select footage folder
        </button>
        <button class="secondary-button" id="analyze-ai" type="button" disabled>
          Analyze with AI
        </button>
      </div>
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
          <button class="secondary-button" id="filter-submit" type="submit">Apply filters</button>
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
          <button class="secondary-button" id="search-submit" type="submit">Search</button>
        </form>
        <form class="ai-search-form" id="ai-search-form">
          <input id="ai-search-input" name="ai-query" placeholder="AI search: Fortnite kill, enemy elimination, victory" />
          <button class="secondary-button" id="ai-search-submit" type="submit">AI Search</button>
        </form>
        <p class="search-status" id="ai-search-status" role="status"></p>
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
        <p class="results-note" id="results-note" hidden></p>
      </section>
    </section>
  </main>

  <div class="preview-backdrop" id="preview-dialog" hidden>
    <section class="preview-modal" role="dialog" aria-modal="true" aria-labelledby="preview-title">
      <div class="preview-header">
        <div>
          <p class="section-label">Clip preview</p>
          <h2 id="preview-title">Preview</h2>
        </div>
        <button class="secondary-button" id="close-preview" type="button">Close</button>
      </div>
      <video id="preview-video" controls preload="metadata"></video>
      <p class="preview-path" id="preview-path"></p>
      <p class="preview-message" id="preview-message" role="status" hidden></p>
    </section>
  </div>
`;

const selectFolderButton = document.querySelector<HTMLButtonElement>("#select-folder");
const analyzeAiButton = document.querySelector<HTMLButtonElement>("#analyze-ai");
const searchButton = document.querySelector<HTMLButtonElement>("#search-submit");
const filterButton = document.querySelector<HTMLButtonElement>("#filter-submit");
const aiSearchButton = document.querySelector<HTMLButtonElement>("#ai-search-submit");
const libraryStatus = document.querySelector<HTMLElement>("#library-status");
const libraryPath = document.querySelector<HTMLElement>("#library-path");
const aiSearchStatus = document.querySelector<HTMLElement>("#ai-search-status");
const clipCount = document.querySelector<HTMLElement>("#clip-count");
const emptyState = document.querySelector<HTMLElement>("#empty-state");
const resultList = document.querySelector<HTMLElement>("#result-list");
const resultsNote = document.querySelector<HTMLElement>("#results-note");
const previewDialog = document.querySelector<HTMLElement>("#preview-dialog");
const previewTitle = document.querySelector<HTMLElement>("#preview-title");
const previewPath = document.querySelector<HTMLElement>("#preview-path");
const previewMessage = document.querySelector<HTMLElement>("#preview-message");
const previewVideo = document.querySelector<HTMLVideoElement>("#preview-video");
const closePreviewButton = document.querySelector<HTMLButtonElement>("#close-preview");
let selectedLibraryPath = "";
let pendingPreviewTimestamp = 0;

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

function closePreview(): void {
  if (previewVideo) {
    previewVideo.pause();
    previewVideo.removeAttribute("src");
    previewVideo.load();
  }
  if (previewDialog) previewDialog.hidden = true;
}

function openPreview(path: string, name: string, timestampMs = 0): void {
  if (!previewDialog || !previewVideo) return;
  pendingPreviewTimestamp = timestampMs;
  if (previewTitle) previewTitle.textContent = name;
  if (previewPath) previewPath.textContent = path;
  if (previewMessage) {
    previewMessage.textContent = "Loading preview…";
    previewMessage.hidden = false;
  }
  previewVideo.src = convertFileSrc(path);
  previewVideo.load();
  previewDialog.hidden = false;
}

function renderResults(results: SearchResult[]): void {
  if (!resultList || !emptyState) return;
  if (results.length === 0) {
    resultList.hidden = true;
    if (resultsNote) resultsNote.hidden = true;
    emptyState.hidden = false;
    return;
  }
  emptyState.hidden = true;
  resultList.hidden = false;
  const visibleResults = results.slice(0, MAX_RENDERED_RESULTS);
  if (resultsNote) {
    resultsNote.hidden = results.length <= MAX_RENDERED_RESULTS;
    resultsNote.textContent = `Showing the first ${MAX_RENDERED_RESULTS} of ${results.length} matches. Refine your search to see a smaller set.`;
  }
  resultList.innerHTML = visibleResults.map((result, index) => {
    const metadata = result.metadata;
    const details = result.ai_description
      ? `AI match ${Math.round((result.match_score ?? 0) * 100)}% · ${result.ai_description}`
      : metadata
        ? `${formatDuration(metadata.duration_ms)} · ${formatBytes(result.size_bytes)} · ${metadata.width ?? "?"}×${metadata.height ?? "?"} · ${metadata.frame_rate ?? "?"} fps · ${metadata.video_codec ?? "?"}`
        : "Technical metadata unavailable";
    const status = result.available ? "Available" : "Unavailable — rescan or restore this path";
    const previewLabel = result.timestamp_ms ? `Preview @ ${formatDuration(result.timestamp_ms)}` : "Preview";
    return `<article class="result-card ${result.available ? "" : "result-card-unavailable"}">
      <div>
        <p class="result-index">${String(index + 1).padStart(2, "0")}</p>
        <h3>${escapeHtml(result.path.split(/[\\/]/).pop() ?? result.path)}</h3>
        <p>${escapeHtml(result.path)}</p>
        <span>${escapeHtml(details)} · ${escapeHtml(status)}</span>
      </div>
      <div class="result-actions">
        <button class="secondary-button preview-result" data-name="${escapeHtml(result.path.split(/[\\/]/).pop() ?? result.path)}" data-path="${escapeHtml(result.path)}" data-timestamp-ms="${result.timestamp_ms ?? 0}" ${result.available ? "" : "disabled"}>${escapeHtml(previewLabel)}</button>
        <button class="secondary-button open-result" data-path="${escapeHtml(result.path)}" ${result.available ? "" : "disabled"}>Open</button>
      </div>
    </article>`;
  }).join("");
  resultList.querySelectorAll<HTMLButtonElement>(".preview-result").forEach((button) => {
    button.addEventListener("click", () => {
      openPreview(
        button.dataset.path ?? "",
        button.dataset.name ?? "Preview",
        Number(button.dataset.timestampMs ?? "0"),
      );
    });
  });
  resultList.querySelectorAll<HTMLButtonElement>(".open-result").forEach((button) => {
    button.addEventListener("click", async () => {
      try {
        await invoke("open_indexed_media_path", { path: button.dataset.path ?? "" });
      } catch (error) {
        if (libraryStatus) libraryStatus.textContent = "Could not open clip";
        if (libraryPath) libraryPath.textContent = String(error);
      }
    });
  });
}

async function searchLibrary(trigger?: HTMLButtonElement): Promise<void> {
  const originalLabel = trigger?.textContent ?? "Search";
  if (trigger) {
    trigger.disabled = true;
    trigger.textContent = "Searching…";
  }
  if (libraryStatus) libraryStatus.textContent = "Searching local index…";
  if (libraryPath) libraryPath.textContent = "Applying filters and sorting";
  try {
    const results = await invoke<SearchResult[]>("search_media", { query: readFilters() });
    renderResults(results);
    if (clipCount) clipCount.textContent = `${results.length} matches`;
    if (libraryStatus) libraryStatus.textContent = `${results.length} matching clips`;
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Search failed";
    if (libraryPath) libraryPath.textContent = String(error);
  } finally {
    if (trigger) {
      trigger.disabled = false;
      trigger.textContent = originalLabel;
    }
  }
}

async function searchAiLibrary(): Promise<void> {
  const query = document.querySelector<HTMLInputElement>("#ai-search-input")?.value.trim() ?? "";
  if (!query) {
    if (aiSearchStatus) aiSearchStatus.textContent = "Enter a natural-language AI query first.";
    return;
  }

  const originalLabel = aiSearchButton?.textContent ?? "AI Search";
  if (aiSearchButton) {
    aiSearchButton.disabled = true;
    aiSearchButton.textContent = "AI searching…";
  }
  if (aiSearchStatus) aiSearchStatus.textContent = "Comparing your query with indexed visual moments…";
  if (libraryStatus) libraryStatus.textContent = "AI search in progress…";
  try {
    const matches = await invoke<AiSearchResult[]>("search_ai", { query });
    renderResults(matches.map((match) => ({
      path: match.path,
      content_hash: match.content_hash,
      size_bytes: 0,
      modified_unix_ms: null,
      status: "ACTIVE",
      available: match.available,
      timestamp_ms: match.timestamp_ms,
      ai_description: `${match.description} · ${match.labels.join(", ")}`,
      match_score: match.score,
      metadata: null,
    })));
    if (clipCount) clipCount.textContent = `${matches.length} AI matches`;
    if (libraryStatus) libraryStatus.textContent = `${matches.length} AI matches for “${query}”`;
    if (aiSearchStatus) aiSearchStatus.textContent = matches.length
      ? "AI matches are timestamped; Preview opens at the matching moment."
      : "No AI matches. Analyze the selected folder first or try another description.";
  } catch (error) {
    const message = String(error);
    if (aiSearchStatus) aiSearchStatus.textContent = message;
    if (libraryStatus) libraryStatus.textContent = "AI search failed";
    if (libraryPath) libraryPath.textContent = message;
  } finally {
    if (aiSearchButton) {
      aiSearchButton.disabled = false;
      aiSearchButton.textContent = originalLabel;
    }
  }
}

async function analyzeLibraryWithAi(): Promise<void> {
  if (!selectedLibraryPath || !analyzeAiButton) return;
  analyzeAiButton.disabled = true;
  const originalLabel = analyzeAiButton.textContent ?? "Analyze with AI";
  analyzeAiButton.textContent = "Analyzing…";
  if (libraryStatus) libraryStatus.textContent = "AI analysis in progress…";
  if (libraryPath) libraryPath.textContent = "Sampling frames and creating searchable descriptions";
  try {
    const report = await invoke<AiIndexReport>("analyze_media_folder", { path: selectedLibraryPath });
    const warningSuffix = report.warnings.length ? ` · ${report.warnings.length} warnings` : "";
    if (libraryStatus) libraryStatus.textContent = `AI indexed ${report.analyzed_file_count} clips${warningSuffix}`;
    if (libraryPath) libraryPath.textContent = `${report.annotation_count} timestamped visual moments stored locally`;
    if (aiSearchStatus) aiSearchStatus.textContent = "AI index ready. Try “Fortnite kill” or “enemy elimination”.";
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "AI analysis failed";
    if (libraryPath) libraryPath.textContent = String(error);
  } finally {
    analyzeAiButton.disabled = false;
    analyzeAiButton.textContent = originalLabel;
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
  selectedLibraryPath = selected;
  if (analyzeAiButton) analyzeAiButton.disabled = false;
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
  void searchLibrary(searchButton ?? undefined);
});

document.querySelector<HTMLFormElement>("#filter-form")?.addEventListener("submit", (event) => {
  event.preventDefault();
  void searchLibrary(filterButton ?? undefined);
});

document.querySelector<HTMLFormElement>("#ai-search-form")?.addEventListener("submit", (event) => {
  event.preventDefault();
  void searchAiLibrary();
});

analyzeAiButton?.addEventListener("click", () => {
  void analyzeLibraryWithAi();
});

document.querySelector<HTMLButtonElement>("#learn-more")?.addEventListener(
  "click",
  () => {
    window.alert("See docs/project-plan.md for the current MVP scope.");
  },
);

closePreviewButton?.addEventListener("click", closePreview);
previewDialog?.addEventListener("click", (event) => {
  if (event.target === previewDialog) closePreview();
});
previewVideo?.addEventListener("loadedmetadata", () => {
  if (pendingPreviewTimestamp > 0 && Number.isFinite(previewVideo.duration)) {
    previewVideo.currentTime = Math.min(
      pendingPreviewTimestamp / 1_000,
      Math.max(previewVideo.duration - 0.1, 0),
    );
  }
  if (previewMessage) previewMessage.hidden = true;
});
previewVideo?.addEventListener("error", () => {
  if (previewMessage) {
    previewMessage.textContent = "This clip cannot be previewed in the embedded player. Use Open to launch it in your system player.";
    previewMessage.hidden = false;
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape" && previewDialog && !previewDialog.hidden) closePreview();
});
