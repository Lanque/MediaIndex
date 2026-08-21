import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
  skipped_file_count: number;
  annotation_count: number;
  warnings: Array<{ path: string; message: string }>;
};

type AiProgress = {
  completed_files: number;
  total_files: number;
  current_file: string;
  provider: string;
  percent: number;
  phase: string;
};

type AiProvider = "local" | "openai" | "gemini";
type AiSearchFocus = "focused" | "balanced" | "broad";

type AiConfig = {
  provider: AiProvider;
  apiKey: string;
  visionModel: string;
  embeddingModel: string;
  baseUrl: string;
  ffmpegPath: string;
  sampleIntervalSeconds: number;
  maxFrames: number;
  contextHint: string;
  reanalyzeExisting: boolean;
};

type AiConnectionReport = {
  provider: string;
  vision_model: string;
  embedding_model: string;
  embedding_dimensions: number;
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
          <div class="library-card-copy">
            <strong id="library-status">No folder indexed</strong>
            <span id="library-path">Choose a local folder to begin</span>
            <div class="analysis-progress" id="analysis-progress" hidden>
              <div class="analysis-progress-heading">
                <span id="analysis-progress-label">Preparing clips</span>
                <strong id="analysis-progress-percent">0%</strong>
              </div>
              <div
                class="analysis-progress-track"
                id="analysis-progress-track"
                role="progressbar"
                aria-label="AI analysis progress"
                aria-valuemin="0"
                aria-valuemax="100"
                aria-valuenow="0"
              >
                <span id="analysis-progress-fill"></span>
              </div>
            </div>
          </div>
        </div>
        <details class="ai-settings">
          <summary>AI connection</summary>
          <p class="settings-help">
            Choose local Ollama, OpenAI (ChatGPT API), or Gemini. Settings stay on this computer. Use Every (s) = 1–2 for short actions and fast scene changes.
          </p>
          <form class="settings-form" id="ai-settings-form">
            <label>Provider
              <select id="ai-provider" name="provider">
                <option value="local">Local (Ollama)</option>
                <option value="openai">OpenAI (ChatGPT API)</option>
                <option value="gemini">Google Gemini API</option>
              </select>
            </label>
            <label id="ai-api-key-label">API key
              <input id="ai-api-key" name="api-key" type="password" autocomplete="off" placeholder="Only for cloud providers" />
            </label>
            <label>Vision model
              <input id="ai-vision-model" name="vision-model" placeholder="gemma4" />
            </label>
            <label id="ai-openai-preset-label">OpenAI recognition preset
              <select id="ai-openai-preset" name="openai-preset">
                <option value="custom">Current / custom model</option>
                <option value="gpt-5.6-luna">Budget default · GPT-5.6 Luna</option>
                <option value="gpt-5.6-terra">Detailed (~10× token price) · GPT-5.6 Terra</option>
              </select>
              <span class="field-help">Luna is the cost-sensitive default. Use Terra only for difficult footage; changing model requires analysis again.</span>
            </label>
            <label>Embedding model
              <input id="ai-embedding-model" name="embedding-model" placeholder="embeddinggemma" />
            </label>
            <label>Base URL
              <input id="ai-base-url" name="base-url" placeholder="http://127.0.0.1:11434" />
            </label>
            <label>FFmpeg path <span class="optional-label">(optional)</span>
              <input id="ai-ffmpeg-path" name="ffmpeg-path" placeholder="Uses PATH if empty" />
            </label>
            <label>Library context <span class="optional-label">(optional)</span>
              <input id="ai-context-hint" name="context-hint" placeholder="Project, franchise, possible characters, location…" />
              <span class="field-help">Helps with project-specific characters and circumstances; candidate names are still verified against the frames.</span>
            </label>
            <div class="settings-grid">
              <label>Every (s)
                <input id="ai-sample-seconds" name="sample-seconds" min="1" type="number" />
              </label>
              <label>Max frames
                <input id="ai-max-frames" name="max-frames" min="1" type="number" />
              </label>
            </div>
            <label class="checkbox-field">
              <input id="ai-reanalyze-existing" name="reanalyze-existing" type="checkbox" />
              Reanalyze existing clips <span class="optional-label">(uses API credits)</span>
            </label>
            <div class="settings-actions">
              <button class="secondary-button" id="save-ai-settings" type="submit">Save settings</button>
              <button class="secondary-button" id="test-ai-connection" type="button">Test connection</button>
            </div>
            <p class="settings-status" id="ai-config-status" role="status"></p>
          </form>
        </details>
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
          <input id="ai-search-input" name="ai-query" placeholder="AI search: character, action, setting, event, visible text" />
          <select id="ai-search-focus" aria-label="AI search relevance">
            <option value="focused">Focused</option>
            <option value="balanced">Balanced</option>
            <option value="broad">Broad</option>
          </select>
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
const aiSettingsForm = document.querySelector<HTMLFormElement>("#ai-settings-form");
const aiProvider = document.querySelector<HTMLSelectElement>("#ai-provider");
const aiApiKey = document.querySelector<HTMLInputElement>("#ai-api-key");
const aiApiKeyLabel = document.querySelector<HTMLLabelElement>("#ai-api-key-label");
const aiVisionModel = document.querySelector<HTMLInputElement>("#ai-vision-model");
const aiOpenAiPresetLabel = document.querySelector<HTMLLabelElement>("#ai-openai-preset-label");
const aiOpenAiPreset = document.querySelector<HTMLSelectElement>("#ai-openai-preset");
const aiEmbeddingModel = document.querySelector<HTMLInputElement>("#ai-embedding-model");
const aiBaseUrl = document.querySelector<HTMLInputElement>("#ai-base-url");
const aiFfmpegPath = document.querySelector<HTMLInputElement>("#ai-ffmpeg-path");
const aiContextHint = document.querySelector<HTMLInputElement>("#ai-context-hint");
const aiSampleSeconds = document.querySelector<HTMLInputElement>("#ai-sample-seconds");
const aiMaxFrames = document.querySelector<HTMLInputElement>("#ai-max-frames");
const aiReanalyzeExisting = document.querySelector<HTMLInputElement>("#ai-reanalyze-existing");
const saveAiSettingsButton = document.querySelector<HTMLButtonElement>("#save-ai-settings");
const testAiConnectionButton = document.querySelector<HTMLButtonElement>("#test-ai-connection");
const aiConfigStatus = document.querySelector<HTMLElement>("#ai-config-status");
const searchButton = document.querySelector<HTMLButtonElement>("#search-submit");
const filterButton = document.querySelector<HTMLButtonElement>("#filter-submit");
const aiSearchButton = document.querySelector<HTMLButtonElement>("#ai-search-submit");
const aiSearchFocus = document.querySelector<HTMLSelectElement>("#ai-search-focus");
const libraryStatus = document.querySelector<HTMLElement>("#library-status");
const libraryPath = document.querySelector<HTMLElement>("#library-path");
const analysisProgress = document.querySelector<HTMLElement>("#analysis-progress");
const analysisProgressLabel = document.querySelector<HTMLElement>("#analysis-progress-label");
const analysisProgressPercent = document.querySelector<HTMLElement>("#analysis-progress-percent");
const analysisProgressTrack = document.querySelector<HTMLElement>("#analysis-progress-track");
const analysisProgressFill = document.querySelector<HTMLElement>("#analysis-progress-fill");
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
let previewGeneration = 0;
let lastAiProgressPercent = 0;
let thumbnailGeneration = 0;
const thumbnailCache = new Map<string, string>();

const AI_SETTINGS_STORAGE_KEY = "mediaindex.ai.settings.v1";
const AI_API_KEY_SESSION_STORAGE_KEY = "mediaindex.ai.api-key.session.v1";
const LIBRARY_PATH_STORAGE_KEY = "mediaindex.library.path.v1";

function aiDefaults(provider: AiProvider): AiConfig {
  if (provider === "openai") {
    return {
      provider,
      apiKey: "",
      visionModel: "gpt-5.6-luna",
      embeddingModel: "text-embedding-3-small",
      baseUrl: "https://api.openai.com/v1",
      ffmpegPath: "",
      sampleIntervalSeconds: 5,
      maxFrames: 60,
      contextHint: "",
      reanalyzeExisting: false,
    };
  }
  if (provider === "gemini") {
    return {
      provider,
      apiKey: "",
      visionModel: "gemini-3.6-flash",
      embeddingModel: "gemini-embedding-001",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      ffmpegPath: "",
      sampleIntervalSeconds: 5,
      maxFrames: 120,
      contextHint: "",
      reanalyzeExisting: false,
    };
  }
  return {
    provider: "local",
    apiKey: "",
    visionModel: "gemma4",
    embeddingModel: "embeddinggemma",
    baseUrl: "http://127.0.0.1:11434",
    ffmpegPath: "",
    sampleIntervalSeconds: 5,
    maxFrames: 120,
    contextHint: "",
    reanalyzeExisting: false,
  };
}

function readAiConfig(): AiConfig {
  const provider = (aiProvider?.value as AiProvider) || "local";
  const defaults = aiDefaults(provider);
  return {
    provider,
    apiKey: aiApiKey?.value.trim() ?? "",
    visionModel: aiVisionModel?.value.trim() || defaults.visionModel,
    embeddingModel: aiEmbeddingModel?.value.trim() || defaults.embeddingModel,
    baseUrl: aiBaseUrl?.value.trim() || defaults.baseUrl,
    ffmpegPath: aiFfmpegPath?.value.trim() ?? "",
    sampleIntervalSeconds: Math.max(1, Number(aiSampleSeconds?.value ?? 5) || 5),
    maxFrames: Math.max(1, Number(aiMaxFrames?.value ?? defaults.maxFrames) || defaults.maxFrames),
    contextHint: aiContextHint?.value.trim() ?? "",
    reanalyzeExisting: aiReanalyzeExisting?.checked ?? false,
  };
}

function aiProviderLabel(provider: AiProvider): string {
  return provider === "openai" ? "OpenAI" : provider === "gemini" ? "Gemini" : "Local (Ollama)";
}

function applyAiConfig(config: AiConfig): void {
  if (aiProvider) aiProvider.value = config.provider;
  if (aiApiKey) aiApiKey.value = config.apiKey;
  if (aiVisionModel) aiVisionModel.value = config.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = config.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = config.baseUrl;
  if (aiFfmpegPath) aiFfmpegPath.value = config.ffmpegPath;
  if (aiSampleSeconds) aiSampleSeconds.value = String(config.sampleIntervalSeconds);
  if (aiMaxFrames) aiMaxFrames.value = String(config.maxFrames);
  if (aiContextHint) aiContextHint.value = config.contextHint;
  if (aiReanalyzeExisting) aiReanalyzeExisting.checked = config.reanalyzeExisting;
  updateAiProviderFields();
}

function syncOpenAiPreset(): void {
  if (!aiOpenAiPreset) return;
  const model = aiVisionModel?.value.trim().toLowerCase() ?? "";
  aiOpenAiPreset.value = model === "gpt-5.6-luna" || model === "gpt-5.6-terra"
    ? model
    : "custom";
}

function updateAiProviderFields(): void {
  const provider = (aiProvider?.value as AiProvider) || "local";
  if (aiApiKeyLabel) aiApiKeyLabel.hidden = provider === "local";
  if (aiOpenAiPresetLabel) aiOpenAiPresetLabel.hidden = provider !== "openai";
  if (aiApiKey) {
    aiApiKey.placeholder = provider === "local" ? "Not needed for Local (Ollama)" : "Kept until this app closes";
  }
  if (aiVisionModel) {
    aiVisionModel.placeholder = provider === "local" ? "gemma4" : provider === "gemini" ? "gemini-3.6-flash" : "gpt-5.6-luna";
  }
  if (aiEmbeddingModel) {
    aiEmbeddingModel.placeholder = provider === "local" ? "embeddinggemma" : provider === "gemini" ? "gemini-embedding-001" : "text-embedding-3-small";
  }
  if (aiBaseUrl) {
    aiBaseUrl.placeholder = provider === "local"
      ? "http://127.0.0.1:11434"
      : provider === "gemini"
        ? "https://generativelanguage.googleapis.com/v1beta"
        : "https://api.openai.com/v1";
  }
  syncOpenAiPreset();
}

function loadAiConfig(): void {
  const fallback = aiDefaults("local");
  try {
    const saved = JSON.parse(localStorage.getItem(AI_SETTINGS_STORAGE_KEY) ?? "null") as Partial<AiConfig> | null;
    const provider = saved?.provider === "openai" || saved?.provider === "gemini" || saved?.provider === "local"
      ? saved.provider
      : fallback.provider;
    const legacyApiKey = typeof saved?.apiKey === "string" ? saved.apiKey : "";
    const sessionApiKey = sessionStorage.getItem(AI_API_KEY_SESSION_STORAGE_KEY) ?? legacyApiKey;
    if (legacyApiKey && !sessionStorage.getItem(AI_API_KEY_SESSION_STORAGE_KEY)) {
      sessionStorage.setItem(AI_API_KEY_SESSION_STORAGE_KEY, legacyApiKey);
    }
    if (saved && "apiKey" in saved) {
      const { apiKey: _removedApiKey, ...safeSettings } = saved;
      localStorage.setItem(AI_SETTINGS_STORAGE_KEY, JSON.stringify(safeSettings));
    }
    applyAiConfig({
      ...aiDefaults(provider),
      ...saved,
      provider,
      apiKey: sessionApiKey,
      reanalyzeExisting: false,
    });
  } catch {
    applyAiConfig(fallback);
  }
}

function saveAiConfig(): AiConfig {
  const config = readAiConfig();
  try {
    const { apiKey, reanalyzeExisting: _oneRunOverride, ...safeSettings } = config;
    localStorage.setItem(AI_SETTINGS_STORAGE_KEY, JSON.stringify(safeSettings));
    if (apiKey) sessionStorage.setItem(AI_API_KEY_SESSION_STORAGE_KEY, apiKey);
    else sessionStorage.removeItem(AI_API_KEY_SESSION_STORAGE_KEY);
    if (aiConfigStatus) aiConfigStatus.textContent = "Settings saved. API key is kept for this app session only.";
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = `Could not save AI settings: ${String(error)}`;
  }
  return config;
}

loadAiConfig();

async function restoreSelectedLibrary(): Promise<void> {
  try {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let savedPath = localStorage.getItem(LIBRARY_PATH_STORAGE_KEY)?.trim() ?? "";
    if (!savedPath) {
      savedPath = (await invoke<string | null>("get_indexed_library_path"))?.trim() ?? "";
      if (savedPath) localStorage.setItem(LIBRARY_PATH_STORAGE_KEY, savedPath);
    }
    if (!savedPath) return;
    selectedLibraryPath = savedPath;
    if (analyzeAiButton) analyzeAiButton.disabled = false;
    if (libraryStatus) libraryStatus.textContent = "Restoring saved library…";
    if (libraryPath) libraryPath.textContent = savedPath;
    void searchLibrary();
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Could not restore saved library";
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
  }
}

function showAiProgress(percent: number, label: string, isError = false): void {
  const safePercent = Math.max(0, Math.min(100, Math.round(percent)));
  lastAiProgressPercent = safePercent;
  if (analysisProgress) {
    analysisProgress.hidden = false;
    analysisProgress.classList.toggle("is-error", isError);
  }
  if (analysisProgressLabel) analysisProgressLabel.textContent = label;
  if (analysisProgressPercent) analysisProgressPercent.textContent = `${safePercent}%`;
  if (analysisProgressTrack) analysisProgressTrack.setAttribute("aria-valuenow", String(safePercent));
  if (analysisProgressFill) analysisProgressFill.style.width = `${safePercent}%`;
}

void listen<AiProgress>("ai-progress", ({ payload }) => {
  const fileName = payload.current_file.split(/[\\/]/).pop() ?? payload.current_file;
  showAiProgress(payload.percent, payload.phase);
  if (libraryStatus) {
    libraryStatus.textContent = `AI analysis ${payload.percent}% · ${payload.completed_files}/${payload.total_files} clips`;
  }
  if (libraryPath) {
    libraryPath.textContent = `${payload.phase} · ${fileName} · ${payload.provider}`;
  }
});

function conciseMessage(value: unknown, maxLength = 240): string {
  let message = String(value).replace(/\s+/g, " ").trim();
  const rawJsonStart = message.search(/[\[{]\s*"(?:id|error|object|status)"/);
  if (rawJsonStart > 0) message = message.slice(0, rawJsonStart).trimEnd();
  return message.length > maxLength ? `${message.slice(0, maxLength - 1).trimEnd()}…` : message;
}

function summarizeAiWarnings(warnings: AiIndexReport["warnings"]): string {
  if (warnings.length === 0) return "";
  const first = warnings[0];
  const fileName = first.path.split(/[\\/]/).pop() ?? first.path;
  const remaining = warnings.length > 1 ? ` · ${warnings.length - 1} more` : "";
  return `${fileName}: ${conciseMessage(first.message)}${remaining}`;
}

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
  previewGeneration += 1;
  if (previewVideo) {
    previewVideo.pause();
    previewVideo.removeAttribute("src");
    previewVideo.load();
  }
  if (previewDialog) previewDialog.hidden = true;
}

async function openPreview(path: string, name: string, timestampMs = 0): Promise<void> {
  if (!previewDialog || !previewVideo) return;
  const generation = ++previewGeneration;
  pendingPreviewTimestamp = timestampMs;
  if (previewTitle) previewTitle.textContent = name;
  if (previewPath) previewPath.textContent = path;
  if (previewMessage) {
    previewMessage.textContent = "Loading preview…";
    previewMessage.hidden = false;
  }
  previewDialog.hidden = false;
  try {
    const allowedPath = "__TAURI_INTERNALS__" in window
      ? await invoke<string>("prepare_indexed_media_preview", { path })
      : path;
    if (generation !== previewGeneration) return;
    previewVideo.src = convertFileSrc(allowedPath);
    previewVideo.load();
  } catch (error) {
    if (generation !== previewGeneration || !previewMessage) return;
    previewMessage.textContent = `Could not preview clip: ${String(error)}`;
    previewMessage.hidden = false;
  }
}

function renderResultCard(result: SearchResult, index: number): string {
  const metadata = result.metadata;
  const details = result.ai_description
    ? `AI match ${Math.round((result.match_score ?? 0) * 100)}% · ${result.ai_description}`
    : metadata
      ? `${formatDuration(metadata.duration_ms)} · ${formatBytes(result.size_bytes)} · ${metadata.width ?? "?"}×${metadata.height ?? "?"} · ${metadata.frame_rate ?? "?"} fps · ${metadata.video_codec ?? "?"}`
      : "Technical metadata unavailable";
  const status = result.available ? "Available" : "Unavailable — rescan or restore this path";
  const fileName = result.path.split(/[\\/]/).pop() ?? result.path;
  const previewLabel = result.timestamp_ms ? `Preview @ ${formatDuration(result.timestamp_ms)}` : "Preview";
  return `<article class="result-card ${result.available ? "" : "result-card-unavailable"}">
    <div>
      <p class="result-index">${String(index + 1).padStart(2, "0")}</p>
      <h3>${escapeHtml(fileName)}</h3>
      <p>${escapeHtml(result.path)}</p>
      <span>${escapeHtml(details)} · ${escapeHtml(status)}</span>
    </div>
    <div class="result-actions">
      <button class="secondary-button preview-result" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(result.path)}" data-timestamp-ms="${result.timestamp_ms ?? 0}" ${result.available ? "" : "disabled"}>${escapeHtml(previewLabel)}</button>
      <button class="secondary-button open-result" data-path="${escapeHtml(result.path)}" ${result.available ? "" : "disabled"}>Open</button>
    </div>
  </article>`;
}

function renderGroupedAiResults(results: SearchResult[]): string {
  const groups = new Map<string, SearchResult[]>();
  for (const result of results) {
    const key = result.content_hash || result.path.toLocaleLowerCase();
    const group = groups.get(key) ?? [];
    group.push(result);
    groups.set(key, group);
  }

  const groupedResults = Array.from(groups.values());
  const topScore = groupedResults[0]?.[0]?.match_score ?? 0;
  return groupedResults.map((group, groupIndex) => {
    const bestMatch = group[0];
    const fileName = bestMatch.path.split(/[\\/]/).pop() ?? bestMatch.path;
    const parentFolder = bestMatch.path.split(/[\\/]/).slice(0, -1).pop() ?? "Local library";
    const moments = [...group].sort((left, right) =>
      (left.timestamp_ms ?? 0) - (right.timestamp_ms ?? 0)
    );
    const bestScore = Math.max(...group.map((result) => result.match_score ?? 0));
    const relevance = groupIndex === 0 ? "Top match" : bestScore >= topScore - 0.04 ? "Strong" : "Related";
    const timestamp = bestMatch.timestamp_ms ?? 0;
    const thumbnailKey = `${bestMatch.content_hash}:${timestamp}`;
    const extraMoments = moments.length > 1
      ? `<details class="video-moments">
          <summary>${moments.length} matching moments</summary>
          <div class="moment-list">
            ${moments.map((moment) => `<button class="moment-row preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(moment.path)}" data-timestamp-ms="${moment.timestamp_ms ?? 0}" ${moment.available ? "" : "disabled"}>
              <strong>${escapeHtml(formatDuration(moment.timestamp_ms))}</strong>
              <span>${escapeHtml(moment.ai_description ?? "Matching scene")}</span>
              <span aria-hidden="true">▶</span>
            </button>`).join("")}
          </div>
        </details>`
      : "";
    return `<article class="video-result-card ${bestMatch.available ? "" : "result-card-unavailable"}">
      <button class="video-thumbnail preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(bestMatch.path)}" data-timestamp-ms="${timestamp}" data-thumbnail-key="${escapeHtml(thumbnailKey)}" ${bestMatch.available ? "" : "disabled"} aria-label="Preview ${escapeHtml(fileName)} at ${escapeHtml(formatDuration(timestamp))}">
        <img alt="" />
        <span class="thumbnail-placeholder">Creating local thumbnail…</span>
        <span class="thumbnail-play" aria-hidden="true">▶</span>
        <span class="thumbnail-time">${escapeHtml(formatDuration(timestamp))}</span>
        <span class="relevance-badge">${escapeHtml(relevance)}</span>
      </button>
      <div class="video-card-body">
        <div class="video-card-heading">
          <div>
            <h3 title="${escapeHtml(fileName)}">${escapeHtml(fileName)}</h3>
            <p>${escapeHtml(parentFolder)}</p>
          </div>
          <button class="icon-button open-result" type="button" data-path="${escapeHtml(bestMatch.path)}" ${bestMatch.available ? "" : "disabled"} aria-label="Open original ${escapeHtml(fileName)}">↗</button>
        </div>
        <p class="video-best-description">${escapeHtml(bestMatch.ai_description ?? "Matching scene")}</p>
        ${extraMoments}
      </div>
    </article>`;
  }).join("");
}

async function loadAiThumbnails(ffmpegPath: string): Promise<void> {
  if (!resultList) return;
  const generation = ++thumbnailGeneration;
  const buttons = Array.from(resultList.querySelectorAll<HTMLButtonElement>(".video-thumbnail"));
  let cursor = 0;
  const worker = async () => {
    while (cursor < buttons.length) {
      const button = buttons[cursor++];
      const key = button.dataset.thumbnailKey ?? "";
      const image = button.querySelector<HTMLImageElement>("img");
      const placeholder = button.querySelector<HTMLElement>(".thumbnail-placeholder");
      if (!key || !image) continue;
      try {
        let dataUrl = thumbnailCache.get(key);
        if (!dataUrl) {
          dataUrl = await invoke<string>("get_ai_thumbnail", {
            path: button.dataset.path ?? "",
            timestampMs: Number(button.dataset.timestampMs ?? "0"),
            ffmpegPath: ffmpegPath || null,
          });
          thumbnailCache.set(key, dataUrl);
        }
        if (generation !== thumbnailGeneration || !button.isConnected) return;
        image.src = dataUrl;
        button.classList.add("thumbnail-loaded");
      } catch {
        if (generation !== thumbnailGeneration || !button.isConnected) return;
        button.classList.add("thumbnail-error");
        if (placeholder) placeholder.textContent = "Preview image unavailable";
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(3, buttons.length) }, () => worker()));
}

function renderResults(results: SearchResult[], groupByVideo = false): void {
  if (!resultList || !emptyState) return;
  if (results.length === 0) {
    resultList.hidden = true;
    resultList.classList.remove("ai-result-grid");
    if (resultsNote) resultsNote.hidden = true;
    emptyState.hidden = false;
    return;
  }
  emptyState.hidden = true;
  resultList.hidden = false;
  resultList.classList.toggle("ai-result-grid", groupByVideo);
  const visibleResults = results.slice(0, MAX_RENDERED_RESULTS);
  if (resultsNote) {
    resultsNote.hidden = results.length <= MAX_RENDERED_RESULTS;
    resultsNote.textContent = `Showing the first ${MAX_RENDERED_RESULTS} of ${results.length} matches. Refine your search to see a smaller set.`;
  }
  resultList.innerHTML = groupByVideo
    ? renderGroupedAiResults(visibleResults)
    : visibleResults.map((result, index) => renderResultCard(result, index)).join("");
  resultList.querySelectorAll<HTMLButtonElement>(".preview-result").forEach((button) => {
    button.addEventListener("click", () => {
      void openPreview(
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
    if (libraryPath && selectedLibraryPath) libraryPath.textContent = selectedLibraryPath;
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
  const config = saveAiConfig();
  const focus = (aiSearchFocus?.value as AiSearchFocus) || "focused";
  if (aiSearchStatus) {
    aiSearchStatus.textContent = `Using ${aiProviderLabel(config.provider)} · ${config.embeddingModel}. Comparing indexed visual moments…`;
  }
  if (libraryStatus) libraryStatus.textContent = "AI search in progress…";
  try {
    const matches = await invoke<AiSearchResult[]>("search_ai", { query, config, focus });
    const displayResults: SearchResult[] = matches.map((match) => ({
      path: match.path,
      content_hash: match.content_hash,
      size_bytes: 0,
      modified_unix_ms: null,
      status: "ACTIVE",
      available: match.available,
      timestamp_ms: match.timestamp_ms,
      ai_description: match.description,
      match_score: match.score,
      metadata: null,
    }));
    renderResults(displayResults, true);
    void loadAiThumbnails(config.ffmpegPath);
    const videoCount = new Set(matches.map((match) => match.content_hash)).size;
    if (clipCount) clipCount.textContent = `${videoCount} videos · ${matches.length} moments`;
    if (libraryStatus) libraryStatus.textContent = `${matches.length} AI moments in ${videoCount} videos for “${query}”`;
    if (aiSearchStatus) aiSearchStatus.textContent = matches.length
      ? `${focus === "focused" ? "Focused" : focus === "balanced" ? "Balanced" : "Broad"} results · click a thumbnail to preview.`
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
  const config = saveAiConfig();
  showAiProgress(0, "Preparing clips");
  if (libraryStatus) libraryStatus.textContent = "AI analysis in progress…";
  if (libraryPath) libraryPath.textContent = `Using ${aiProviderLabel(config.provider)} · ${config.visionModel} / ${config.embeddingModel}`;
  try {
    const report = await invoke<AiIndexReport>("analyze_media_folder", {
      path: selectedLibraryPath,
      config,
      force: config.reanalyzeExisting,
    });
    const warningSuffix = report.warnings.length ? ` · ${report.warnings.length} warnings` : "";
    const warningDetails = summarizeAiWarnings(report.warnings);
    showAiProgress(100, report.warnings.length ? "Analysis complete with warnings" : "Analysis complete");
    const skippedSuffix = report.skipped_file_count ? ` · ${report.skipped_file_count} already ready` : "";
    if (libraryStatus) libraryStatus.textContent = `AI indexed ${report.analyzed_file_count} clips${skippedSuffix}${warningSuffix}`;
    if (libraryPath) {
      libraryPath.textContent = report.analyzed_file_count === 0 && report.skipped_file_count > 0
        ? "Existing AI index kept · no API credits used"
        : warningDetails
          ? `${report.annotation_count} new visual moments stored locally · ${warningDetails}`
          : `${report.annotation_count} new visual moments stored locally`;
    }
    if (aiSearchStatus) {
      aiSearchStatus.textContent = warningDetails
        ? `AI warnings: ${warningDetails}`
        : report.analyzed_file_count === 0 && report.skipped_file_count > 0
          ? "No API work needed — every clip is already analyzed with this model."
          : "AI index ready. Search for a person, action, place, event, or visible text.";
    }
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "AI analysis failed";
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
    showAiProgress(lastAiProgressPercent, "Analysis stopped", true);
  } finally {
    if (config.reanalyzeExisting && aiReanalyzeExisting) {
      aiReanalyzeExisting.checked = false;
      saveAiConfig();
    }
    analyzeAiButton.disabled = false;
    analyzeAiButton.textContent = originalLabel;
  }
}

async function testAiConnection(): Promise<void> {
  if (!testAiConnectionButton) return;
  testAiConnectionButton.disabled = true;
  const config = saveAiConfig();
  if (aiConfigStatus) aiConfigStatus.textContent = `Testing ${aiProviderLabel(config.provider)} · ${config.embeddingModel}…`;
  try {
    const report = await invoke<AiConnectionReport>("test_ai_connection", {
      config,
    });
    if (aiConfigStatus) {
      aiConfigStatus.textContent = "Connected: " + report.provider + " · " + report.embedding_model + " · " + report.embedding_dimensions + " dimensions";
    }
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = String(error);
  } finally {
    testAiConnectionButton.disabled = false;
  }
}

aiProvider?.addEventListener("change", () => {
  const provider = (aiProvider.value as AiProvider) || "local";
  const defaults = aiDefaults(provider);
  if (aiApiKey) aiApiKey.value = "";
  if (aiVisionModel) aiVisionModel.value = defaults.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = defaults.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  updateAiProviderFields();
  saveAiConfig();
});

aiVisionModel?.addEventListener("input", syncOpenAiPreset);

aiOpenAiPreset?.addEventListener("change", () => {
  if (!aiVisionModel || !aiOpenAiPreset || aiOpenAiPreset.value === "custom") return;
  aiVisionModel.value = aiOpenAiPreset.value;
  if (aiOpenAiPreset.value === "gpt-5.6-luna" && aiMaxFrames && Number(aiMaxFrames.value) > 60) {
    aiMaxFrames.value = "60";
  }
  if (aiConfigStatus) {
    aiConfigStatus.textContent = aiOpenAiPreset.value === "gpt-5.6-luna"
      ? "Budget preset selected (up to 60 frames per video). Save settings; only missing clips are analyzed by default."
      : "Detailed model selected. It costs about 10× Luna's model token price; enable reanalysis only when needed.";
  }
});

aiSettingsForm?.addEventListener("submit", (event) => {
  event.preventDefault();
  saveAiConfig();
});

testAiConnectionButton?.addEventListener("click", () => {
  void testAiConnection();
});

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
    try {
      localStorage.setItem(LIBRARY_PATH_STORAGE_KEY, selected);
    } catch (error) {
      if (libraryPath) libraryPath.textContent = `Folder indexed · ${conciseMessage(error)}`;
    }
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

void restoreSelectedLibrary();

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
