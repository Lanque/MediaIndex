import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./styles.css";

import {
  ALL_MODEL_PRESETS,
  type AiAnalysisPlan,
  type AiConfig,
  type AiConnectionReport,
  type AiIndexReport,
  type AiProgress,
  type AiProvider,
  type AiSearchFocus,
  type AiSearchResult,
  type GeminiOAuthStatus,
  type IndexReport,
  type ModelPreset,
  type SavedAiMoment,
  type SearchFilters,
  type SearchResult,
} from "./types";
import {
  conciseMessage,
  dateToUnixMs,
  escapeHtml,
  formatBytes,
  formatDuration,
  numberToMs,
} from "./utils/formatters";
import * as tauriApi from "./services/tauri";

const MAX_RENDERED_RESULTS = 500;

const app = document.querySelector<HTMLDivElement>("#app");

if (!app) {
  throw new Error("MediaIndex root element was not found.");
}

app.innerHTML = `
  <main class="shell">
    <header class="topbar">
      <div class="topbar-brand">
        <div class="topbar-logo">
          <span class="topbar-logo-icon">M</span>
          <span class="topbar-product">MediaIndex</span>
          <span class="desktop-product-mark">Windows desktop</span>
        </div>
        <div class="topbar-breadcrumb" id="topbar-breadcrumb">
          <span class="topbar-breadcrumb-sep">/</span>
          <span id="header-folder-name">No folder selected</span>
        </div>
      </div>
      <div class="topbar-actions">
        <button class="primary-button" id="select-folder" type="button">
          Select Footage Folder
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
          <div class="library-card-header">
            <span class="status-dot" aria-hidden="true"></span>
            <strong class="library-card-title" id="library-status">No folder indexed</strong>
          </div>
          <p class="library-card-path" id="library-path">Choose a local folder to begin</p>
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

        <details class="ai-settings" open>
          <summary>AI Vision & Embeddings</summary>
          <p class="settings-help">
            Original videos stay on your computer. Cloud providers receive sampled frame images; OpenAI can also receive a compressed speech track when timestamped transcription is enabled. Local Ollama sends nothing off-device.
          </p>

          <div class="active-model-card" id="active-model-card">
            <div class="active-model-top">
              <span class="model-card-provider">Current Model</span>
              <span class="model-badge badge-recommended" id="active-model-badge">Recommended</span>
            </div>
            <div class="active-model-name" id="active-model-title">
              <strong id="active-model-name">Gemini 3.8 Flash</strong>
            </div>
            <p class="active-model-cost" id="active-model-cost">Input $0.75 · output $3.75 / 1M tokens</p>
            <button class="open-model-hub-btn" id="open-model-hub" type="button">
              Change Model & View Pricing...
            </button>
          </div>

          <form class="settings-form" id="ai-settings-form">
            <label id="ai-api-key-label">API Key
              <input id="ai-api-key" name="api-key" type="password" autocomplete="off" placeholder="Enter Gemini (AIza...) or OpenAI (sk-...) API key" />
            </label>
            <div class="provider-auth-panel" id="provider-auth-panel">
              <p id="provider-auth-help"></p>
              <div class="provider-auth-actions">
                <button class="text-button" id="open-provider-key-page" type="button">Get API key</button>
                <button class="text-button" id="login-gemini-oauth" type="button">Login with Google</button>
                <button class="text-button" id="use-gemini-api-key" type="button" hidden>Use API key</button>
                <button class="text-button" id="logout-gemini-oauth" type="button" hidden>Disconnect Google</button>
                <button class="text-button" id="open-provider-oauth-docs" type="button">OAuth setup</button>
              </div>
            </div>

            <details class="advanced-ai-settings" style="margin-top: 4px;">
              <summary style="font-size: 11px; color: var(--text-muted); cursor: pointer;">Advanced Configuration</summary>
              <div style="display: grid; gap: 8px; margin-top: 8px;">
                <label>Provider
                  <select id="ai-provider" name="provider">
                    <option value="gemini">Google Gemini API</option>
                    <option value="openai">OpenAI API</option>
                    <option value="local">Local (Ollama)</option>
                  </select>
                </label>
                <label>Vision model
                  <input id="ai-vision-model" name="vision-model" placeholder="gemini-3.8-flash" />
                </label>
                <label>Embedding model
                  <input id="ai-embedding-model" name="embedding-model" placeholder="gemini-embedding-2" />
                </label>
                <label>Base URL
                  <input id="ai-base-url" name="base-url" placeholder="https://generativelanguage.googleapis.com/v1beta" />
                </label>
                <label>FFmpeg path <span class="optional-label">(optional)</span>
                  <input id="ai-ffmpeg-path" name="ffmpeg-path" placeholder="Uses system PATH if empty" />
                </label>
                <label>Library context <span class="optional-label">(optional)</span>
                  <input id="ai-context-hint" name="context-hint" placeholder="Project name, subjects, landmarks, location..." />
                </label>
                <div class="settings-grid">
                  <label>Sample every (s)
                    <input id="ai-sample-seconds" name="sample-seconds" min="1" type="number" />
                  </label>
                  <label>Max frames / video
                    <input id="ai-max-frames" name="max-frames" min="1" type="number" />
                  </label>
                </div>
                <div class="speech-settings" id="speech-settings">
                  <label class="checkbox-field">
                    <input id="ai-transcribe-audio" name="transcribe-audio" type="checkbox" />
                    Transcribe spoken audio with timestamps
                  </label>
                  <label>Transcription model
                    <input id="ai-transcription-model" name="transcription-model" placeholder="whisper-1" />
                  </label>
                  <p>OpenAI speech transcription uploads a compressed mono audio track for the analyzed time span. Disable this to analyze frames only.</p>
                </div>
                <label class="checkbox-field">
                  <input id="ai-reanalyze-existing" name="reanalyze-existing" type="checkbox" />
                  Reanalyze existing clips
                </label>
              </div>
            </details>

            <div class="settings-actions">
              <button class="secondary-button" id="save-ai-settings" type="submit">Save Settings</button>
              <button class="secondary-button" id="test-ai-connection" type="button">Test Connection</button>
            </div>
            <p class="settings-status" id="ai-config-status" role="status"></p>
          </form>
        </details>

        <p class="section-label">Metadata Filters</p>
        <form class="filter-form" id="filter-form">
          <label class="filter-full-width">Folder<input id="filter-folder" name="folder" placeholder="e.g. B-Roll" /></label>
          <label>Date from<input id="filter-date-from" name="date-from" type="date" /></label>
          <label>Date to<input id="filter-date-to" name="date-to" type="date" /></label>
          <label>Resolution<input id="filter-resolution" name="resolution" placeholder="1920x1080" /></label>
          <label>FPS<input id="filter-frame-rate" name="frame-rate" placeholder="29.97" /></label>
          <label>Min duration (s)<input id="filter-duration-min" name="duration-min" min="0" type="number" /></label>
          <label>Max duration (s)<input id="filter-duration-max" name="duration-max" min="0" type="number" /></label>
          <label class="filter-full-width">Codec<input id="filter-codec" name="codec" placeholder="h264, hevc, prores" /></label>
          <button class="secondary-button filter-full-width" id="filter-submit" type="submit">Apply Filters</button>
        </form>
      </aside>

      <section class="content-panel">
        <div class="panel-header">
          <div>
            <h1 class="panel-title" id="panel-title">Current folder</h1>
            <p class="panel-subtitle" id="panel-subtitle">Grouped by source folder · originals remain untouched</p>
          </div>
          <div class="panel-header-actions">
            <div class="library-view-tabs" aria-label="Library view">
              <button class="library-view-tab is-active" data-library-view="current" type="button">Current folder</button>
              <button class="library-view-tab" data-library-view="analyzed" type="button">Analyzed archive</button>
            </div>
            <span class="count-badge" id="clip-count">0 clips</span>
          </div>
        </div>
        <div class="search-container">
          <form class="search-form" id="search-form">
            <div class="search-input-wrapper">
              <input id="search-input" name="keyword" placeholder="Filter by filename or technical metadata…" />
            </div>
            <select id="sort-by" aria-label="Sort results">
              <option value="name">Name</option>
              <option value="duration">Duration</option>
              <option value="size">Size</option>
              <option value="modified">Date</option>
              <option value="resolution">Resolution</option>
            </select>
            <select id="sort-direction" aria-label="Sort direction">
              <option value="asc">Asc</option>
              <option value="desc">Desc</option>
            </select>
            <button class="secondary-button" id="search-submit" type="submit">Filter</button>
          </form>
          <form class="ai-search-form" id="ai-search-form">
            <div class="search-input-wrapper">
              <input id="ai-search-input" name="ai-query" placeholder="AI Visual Search: character, action, scenery, event, visible text…" />
            </div>
            <select id="ai-search-focus" aria-label="AI search relevance">
              <option value="focused">Focused</option>
              <option value="balanced">Balanced</option>
              <option value="broad">Broad</option>
            </select>
            <button class="primary-button" id="ai-search-submit" type="submit">AI Search</button>
          </form>
        </div>
        <p class="search-status" id="ai-search-status" role="status"></p>
        <div class="empty-state" id="empty-state">
          <div class="empty-icon" aria-hidden="true">
            <svg viewBox="0 0 32 32" role="presentation"><path d="M3.5 8.5h9l2.5 3h13.5v14H3.5z"/><path d="M3.5 11.5v-5h8l2 2"/><path d="m14 15.5 6 3.5-6 3.5z"/></svg>
          </div>
          <h3 id="empty-title">No footage indexed yet</h3>
          <p id="empty-copy">
            Select a folder containing your video files. MediaIndex indexes metadata and visual scenes locally, keeping original files safe on your computer.
          </p>
        </div>
        <div class="result-list" id="result-list" hidden></div>
        <p class="results-note" id="results-note" hidden></p>
      </section>
    </section>
  </main>

  <!-- AI Model Hub Modal -->
  <div class="preview-backdrop" id="model-picker-dialog" hidden>
    <section class="model-hub-modal" role="dialog" aria-modal="true" aria-labelledby="model-hub-title">
      <div class="preview-header">
        <div>
          <p class="eyebrow" style="margin-bottom: 4px;">AI MODEL SELECTION & PRICING</p>
          <h2 id="model-hub-title" style="font-size: 1.35rem;">Choose a Vision Model</h2>
          <p class="lede" style="font-size: 0.85rem; margin-top: 4px; color: var(--text-muted);">
            Gemini, OpenAI, and local Ollama options. API prices are informational and can change.
          </p>
        </div>
        <button class="secondary-button" id="close-model-hub" type="button">Close</button>
      </div>

      <div class="model-hub-tabs" id="model-hub-tabs">
        <button class="model-hub-tab is-active" type="button" data-filter="all">All Models (${ALL_MODEL_PRESETS.length})</button>
        <button class="model-hub-tab" type="button" data-filter="gemini">Google Gemini</button>
        <button class="model-hub-tab" type="button" data-filter="openai">OpenAI</button>
        <button class="model-hub-tab" type="button" data-filter="local">Local (Ollama)</button>
      </div>

      <div class="model-hub-grid" id="model-hub-grid"></div>
    </section>
  </div>

  <!-- Saved AI analysis Modal -->
  <div class="preview-backdrop" id="analysis-dialog" hidden>
    <section class="analysis-modal" role="dialog" aria-modal="true" aria-labelledby="analysis-title">
      <div class="preview-header">
        <div>
          <p class="section-label">Saved AI analysis</p>
          <h2 id="analysis-title">AI analysis</h2>
          <p class="analysis-path" id="analysis-path"></p>
        </div>
        <button class="secondary-button" id="close-analysis" type="button">Close</button>
      </div>
      <div class="analysis-modal-body">
        <div class="analysis-explainer">
          <strong>What is an AI moment?</strong>
          <p>MediaIndex samples timestamps from the video, then combines adjacent samples with the same action, setting, and situation into one contextual time range. Dialogue, visible text, entities, and other detected details remain searchable inside that range.</p>
        </div>
        <p class="analysis-status" id="analysis-status" role="status">Loading saved analysis…</p>
        <div class="analysis-content" id="analysis-content"></div>
      </div>
    </section>
  </div>

  <!-- Video Preview Modal -->
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
const activeModelBadge = document.querySelector<HTMLElement>("#active-model-badge");
const activeModelName = document.querySelector<HTMLElement>("#active-model-name");
const activeModelCost = document.querySelector<HTMLElement>("#active-model-cost");
const openModelHubBtn = document.querySelector<HTMLButtonElement>("#open-model-hub");
const closeModelHubBtn = document.querySelector<HTMLButtonElement>("#close-model-hub");
const modelPickerDialog = document.querySelector<HTMLElement>("#model-picker-dialog");
const modelHubGrid = document.querySelector<HTMLElement>("#model-hub-grid");
const modelHubTabs = document.querySelector<HTMLElement>("#model-hub-tabs");
const aiApiKey = document.querySelector<HTMLInputElement>("#ai-api-key");
const aiApiKeyLabel = document.querySelector<HTMLLabelElement>("#ai-api-key-label");
const providerAuthPanel = document.querySelector<HTMLElement>("#provider-auth-panel");
const providerAuthHelp = document.querySelector<HTMLElement>("#provider-auth-help");
const openProviderKeyPageButton = document.querySelector<HTMLButtonElement>("#open-provider-key-page");
const openProviderOauthDocsButton = document.querySelector<HTMLButtonElement>("#open-provider-oauth-docs");
const loginGeminiOauthButton = document.querySelector<HTMLButtonElement>("#login-gemini-oauth");
const useGeminiApiKeyButton = document.querySelector<HTMLButtonElement>("#use-gemini-api-key");
const logoutGeminiOauthButton = document.querySelector<HTMLButtonElement>("#logout-gemini-oauth");
const aiVisionModel = document.querySelector<HTMLInputElement>("#ai-vision-model");
const aiEmbeddingModel = document.querySelector<HTMLInputElement>("#ai-embedding-model");
const aiBaseUrl = document.querySelector<HTMLInputElement>("#ai-base-url");
const aiFfmpegPath = document.querySelector<HTMLInputElement>("#ai-ffmpeg-path");
const aiContextHint = document.querySelector<HTMLInputElement>("#ai-context-hint");
const speechSettings = document.querySelector<HTMLElement>("#speech-settings");
const aiTranscribeAudio = document.querySelector<HTMLInputElement>("#ai-transcribe-audio");
const aiTranscriptionModel = document.querySelector<HTMLInputElement>("#ai-transcription-model");
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
const panelTitle = document.querySelector<HTMLElement>("#panel-title");
const panelSubtitle = document.querySelector<HTMLElement>("#panel-subtitle");
const emptyState = document.querySelector<HTMLElement>("#empty-state");
const resultList = document.querySelector<HTMLElement>("#result-list");
const resultsNote = document.querySelector<HTMLElement>("#results-note");
const analysisDialog = document.querySelector<HTMLElement>("#analysis-dialog");
const analysisTitle = document.querySelector<HTMLElement>("#analysis-title");
const analysisPath = document.querySelector<HTMLElement>("#analysis-path");
const analysisStatus = document.querySelector<HTMLElement>("#analysis-status");
const analysisContent = document.querySelector<HTMLElement>("#analysis-content");
const closeAnalysisButton = document.querySelector<HTMLButtonElement>("#close-analysis");
const previewDialog = document.querySelector<HTMLElement>("#preview-dialog");
const previewTitle = document.querySelector<HTMLElement>("#preview-title");
const previewPath = document.querySelector<HTMLElement>("#preview-path");
const previewMessage = document.querySelector<HTMLElement>("#preview-message");
const previewVideo = document.querySelector<HTMLVideoElement>("#preview-video");
const closePreviewButton = document.querySelector<HTMLButtonElement>("#close-preview");

let selectedLibraryPath = "";
let pendingPreviewTimestamp = 0;
let previewGeneration = 0;
let analysisGeneration = 0;
let lastAiProgressPercent = 0;
let thumbnailGeneration = 0;
let aiAnalysisRunning = false;
let aiCancellationPending = false;
let aiStopRequested = false;
let currentHubFilter: "all" | AiProvider = "all";
let libraryView: "current" | "analyzed" = "current";
let analysisStartedAt = 0;
let geminiAuthMode: "api_key" | "oauth" = "api_key";
let geminiOAuthStatus: GeminiOAuthStatus = {
  connected: false,
  project_id: null,
  expires_at_unix_ms: null,
};
const thumbnailCache = new Map<string, string>();
const savedAnalysisCache = new Map<string, SavedAiMoment[]>();

const AI_SETTINGS_STORAGE_KEY = "mediaindex.ai.settings.v1";
const AI_API_KEY_SESSION_STORAGE_KEY = "mediaindex.ai.api-key.session.v1";
const LIBRARY_PATH_STORAGE_KEY = "mediaindex.library.path.v1";
const AI_TIMING_STORAGE_KEY = "mediaindex.ai.timing.v1";

type AiTimingHistory = Record<string, { millisecondsPerRequest: number; samples: number }>;

function normalizeVisionModel(provider: AiProvider, model: string): string {
  const trimmed = model.trim();
  return provider === "openai" && trimmed.toLowerCase() === "04-mini" ? "o4-mini" : trimmed;
}

function timingKey(config: AiConfig): string {
  const speech = config.transcribeAudio ? `:speech-${config.transcriptionModel.toLowerCase()}` : "";
  return `${config.provider}:${normalizeVisionModel(config.provider, config.visionModel).toLowerCase()}:${config.embeddingModel.toLowerCase()}${speech}`;
}

function readTimingHistory(): AiTimingHistory {
  try {
    return JSON.parse(localStorage.getItem(AI_TIMING_STORAGE_KEY) ?? "{}") as AiTimingHistory;
  } catch {
    return {};
  }
}

function recordAnalysisTiming(config: AiConfig, plan: AiAnalysisPlan, elapsedMs: number): void {
  if (plan.estimated_vision_requests <= 0 || elapsedMs <= 0) return;
  const history = readTimingHistory();
  const key = timingKey(config);
  const previous = history[key];
  const measured = elapsedMs / plan.estimated_vision_requests;
  const samples = Math.min((previous?.samples ?? 0) + 1, 10);
  history[key] = {
    millisecondsPerRequest: previous
      ? (previous.millisecondsPerRequest * (samples - 1) + measured) / samples
      : measured,
    samples,
  };
  try {
    localStorage.setItem(AI_TIMING_STORAGE_KEY, JSON.stringify(history));
  } catch {
    // A timing estimate is optional; analysis results are already stored in SQLite.
  }
}

function formatApproximateTime(milliseconds: number): string {
  const seconds = Math.max(1, Math.round(milliseconds / 1_000));
  if (seconds < 60) return `about ${seconds}s`;
  const minutes = Math.max(1, Math.round(seconds / 60));
  if (minutes < 60) return `about ${minutes} min`;
  const hours = Math.floor(minutes / 60);
  const remainingMinutes = minutes % 60;
  return remainingMinutes ? `about ${hours}h ${remainingMinutes}m` : `about ${hours}h`;
}

function analysisTimeEstimate(plan: AiAnalysisPlan, config: AiConfig): string {
  const timing = readTimingHistory()[timingKey(config)];
  if (!timing || plan.estimated_vision_requests <= 0) {
    const model = config.visionModel.toLowerCase();
    const millisecondsPerRequest = config.provider === "local"
      ? 12_000
      : model.includes("terra")
        ? 8_000
        : model.includes("o4-mini")
          ? 6_000
          : config.provider === "gemini"
            ? 2_500
            : 3_500;
    const preparationMs = plan.analyze_file_count * (config.provider === "local" ? 2_500 : 1_500);
    const speechMs = plan.estimated_audio_seconds * 180;
    const estimate = Math.max(
      1_000,
      plan.estimated_vision_requests * millisecondsPerRequest + preparationMs + speechMs,
    );
    return `Rough first-run estimate: ${formatApproximateTime(estimate)}. The completed run will calibrate future estimates on this computer.`;
  }
  return `Estimated time: ${formatApproximateTime(timing.millisecondsPerRequest * plan.estimated_vision_requests)} based on ${timing.samples} completed local run${timing.samples === 1 ? "" : "s"}.`;
}

function cleanApiKey(raw: string): string {
  let cleaned = raw.trim();
  if (cleaned.toLowerCase().startsWith("bearer ")) {
    cleaned = cleaned.slice(7).trim();
  }
  if ((cleaned.startsWith('"') && cleaned.endsWith('"')) || (cleaned.startsWith("'") && cleaned.endsWith("'"))) {
    if (cleaned.length >= 2) {
      cleaned = cleaned.slice(1, -1).trim();
    }
  }
  return cleaned;
}

function getSessionApiKey(provider: AiProvider): string {
  if (provider === "local") return "";
  return sessionStorage.getItem(`mediaindex.ai.api-key.${provider}.session.v1`) ?? "";
}

function setSessionApiKey(provider: AiProvider, key: string): void {
  if (provider === "local") return;
  const clean = cleanApiKey(key);
  if (clean) {
    sessionStorage.setItem(`mediaindex.ai.api-key.${provider}.session.v1`, clean);
  } else {
    sessionStorage.removeItem(`mediaindex.ai.api-key.${provider}.session.v1`);
  }
}

function aiDefaults(provider: AiProvider): AiConfig {
  if (provider === "openai") {
    return {
      provider,
      authMode: "api_key",
      apiKey: "",
      visionModel: "gpt-5.6-luna",
      embeddingModel: "text-embedding-3-small",
      baseUrl: "https://api.openai.com/v1",
      ffmpegPath: "",
      sampleIntervalSeconds: 5,
      maxFrames: 60,
      contextHint: "",
      transcribeAudio: true,
      transcriptionModel: "whisper-1",
      reanalyzeExisting: false,
    };
  }
  if (provider === "gemini") {
    return {
      provider,
      authMode: "api_key",
      apiKey: "",
      visionModel: "gemini-3.8-flash",
      embeddingModel: "gemini-embedding-2",
      baseUrl: "https://generativelanguage.googleapis.com/v1beta",
      ffmpegPath: "",
      sampleIntervalSeconds: 5,
      maxFrames: 120,
      contextHint: "",
      transcribeAudio: false,
      transcriptionModel: "whisper-1",
      reanalyzeExisting: false,
    };
  }
  return {
    provider: "local",
    authMode: "api_key",
    apiKey: "",
    visionModel: "gemma4:e2b",
    embeddingModel: "embeddinggemma",
    baseUrl: "http://127.0.0.1:11434",
    ffmpegPath: "",
    sampleIntervalSeconds: 5,
    maxFrames: 120,
    contextHint: "",
    transcribeAudio: false,
    transcriptionModel: "whisper-1",
    reanalyzeExisting: false,
  };
}

function readAiConfig(): AiConfig {
  const provider = (aiProvider?.value as AiProvider) || "gemini";
  const defaults = aiDefaults(provider);
  let baseUrl = aiBaseUrl?.value.trim() || defaults.baseUrl;
  if (provider === "openai" && (baseUrl.includes("googleapis.com") || baseUrl.includes("11434"))) {
    baseUrl = defaults.baseUrl;
    if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  } else if (provider === "gemini" && (baseUrl.includes("api.openai.com") || baseUrl.includes("11434"))) {
    baseUrl = defaults.baseUrl;
    if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  } else if (provider === "local" && (baseUrl.includes("googleapis.com") || baseUrl.includes("api.openai.com"))) {
    baseUrl = defaults.baseUrl;
    if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  }
  return {
    provider,
    authMode: provider === "gemini" ? geminiAuthMode : "api_key",
    apiKey:
      provider === "gemini" && geminiAuthMode === "oauth"
        ? ""
        : cleanApiKey(aiApiKey?.value ?? ""),
    visionModel: normalizeVisionModel(
      provider,
      aiVisionModel?.value.trim() || defaults.visionModel,
    ),
    embeddingModel: aiEmbeddingModel?.value.trim() || defaults.embeddingModel,
    baseUrl,
    ffmpegPath: aiFfmpegPath?.value.trim() ?? "",
    sampleIntervalSeconds: Math.max(1, Number(aiSampleSeconds?.value ?? 5) || 5),
    maxFrames: Math.max(1, Number(aiMaxFrames?.value ?? defaults.maxFrames) || defaults.maxFrames),
    contextHint: aiContextHint?.value.trim() ?? "",
    transcribeAudio: provider === "openai" && (aiTranscribeAudio?.checked ?? false),
    transcriptionModel: aiTranscriptionModel?.value.trim() || "whisper-1",
    reanalyzeExisting: aiReanalyzeExisting?.checked ?? false,
  };
}

function aiProviderLabel(provider: AiProvider): string {
  return provider === "openai" ? "OpenAI" : provider === "gemini" ? "Google Gemini" : "Local (Ollama)";
}

function updateActiveModelCard(): void {
  const config = readAiConfig();
  const matched = ALL_MODEL_PRESETS.find(
    (p) =>
      p.provider === config.provider &&
      p.visionModel.toLowerCase() === config.visionModel.toLowerCase(),
  );

  if (matched) {
    if (activeModelName) activeModelName.textContent = matched.name;
    if (activeModelCost) activeModelCost.textContent = matched.pricing;
    if (activeModelBadge) {
      activeModelBadge.textContent = matched.badge ?? "Active";
      activeModelBadge.className = `model-badge ${
        matched.badgeType === "free"
          ? "badge-free"
          : matched.badgeType === "pro"
            ? "badge-pro"
            : matched.badgeType === "recommended"
              ? "badge-recommended"
              : "badge-default"
      }`;
    }
  } else {
    if (activeModelName)
      activeModelName.textContent = `${config.visionModel} (${aiProviderLabel(config.provider)})`;
    if (activeModelCost) activeModelCost.textContent = "Custom configuration";
    if (activeModelBadge) {
      activeModelBadge.textContent = "Custom";
      activeModelBadge.className = "model-badge badge-default";
    }
  }
}

function renderModelHubGrid(filter: "all" | AiProvider = "all"): void {
  if (!modelHubGrid) return;
  const currentConfig = readAiConfig();
  const presets =
    filter === "all"
      ? ALL_MODEL_PRESETS
      : ALL_MODEL_PRESETS.filter((p) => p.provider === filter);

  modelHubGrid.innerHTML = presets
    .map((preset) => {
      const isCurrent =
        preset.provider === currentConfig.provider &&
        preset.visionModel.toLowerCase() === currentConfig.visionModel.toLowerCase();
      const badgeClass =
        preset.badgeType === "free"
          ? "badge-free"
          : preset.badgeType === "pro"
            ? "badge-pro"
            : preset.badgeType === "recommended"
              ? "badge-recommended"
              : "badge-default";

      return `<article class="model-card-item ${isCurrent ? "is-current" : ""}" data-preset-id="${escapeHtml(preset.id)}">
        <div>
          <div class="model-card-top">
            <div>
              <span class="model-card-provider">${escapeHtml(aiProviderLabel(preset.provider))}</span>
              <h3 class="model-card-title">${escapeHtml(preset.name)}</h3>
            </div>
            ${preset.badge ? `<span class="model-badge ${badgeClass}">${escapeHtml(preset.badge)}</span>` : ""}
          </div>

          <div class="model-card-pricing">
            <span>${escapeHtml(preset.pricing)}</span>
          </div>

          <div class="model-card-specs">
            <span>Speed: <strong>${escapeHtml(preset.speed)}</strong></span>
            <span>Accuracy: <strong>${escapeHtml(preset.accuracy)}</strong></span>
          </div>

          <p class="model-card-summary">${escapeHtml(preset.summary)}</p>

          <div class="model-card-reqs">
            <span>Requires: ${escapeHtml(preset.requirements)}</span>
          </div>
        </div>

        <button
          class="primary-button model-select-btn ${isCurrent ? "is-active-btn" : ""}"
          type="button"
          data-select-preset="${escapeHtml(preset.id)}"
        >
          ${isCurrent ? "Selected Model" : "Select Model"}
        </button>
      </article>`;
    })
    .join("");

  modelHubGrid.querySelectorAll<HTMLButtonElement>("[data-select-preset]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const presetId = btn.dataset.selectPreset;
      if (!presetId) return;
      selectModelPreset(presetId);
    });
  });
}

function selectModelPreset(presetId: string): void {
  const preset = ALL_MODEL_PRESETS.find((p) => p.id === presetId);
  if (!preset) return;
  const defaults = aiDefaults(preset.provider);
  if (aiProvider) aiProvider.value = preset.provider;
  if (aiVisionModel) aiVisionModel.value = preset.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = preset.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  if (preset.provider !== "local") {
    if (aiMaxFrames && Number(aiMaxFrames.value) > 60) aiMaxFrames.value = "60";
  }
  if (aiApiKey) {
    aiApiKey.value = getSessionApiKey(preset.provider);
  }
  updateAiProviderFields();
  saveAiConfig();
  updateActiveModelCard();
  closeModelHub();
  if (aiConfigStatus) {
    aiConfigStatus.textContent = `Selected model: ${preset.name} (${aiProviderLabel(preset.provider)}). ${preset.pricing}`;
  }
}

function openModelHub(): void {
  if (!modelPickerDialog) return;
  renderModelHubGrid(currentHubFilter);
  modelPickerDialog.hidden = false;
}

function closeModelHub(): void {
  if (modelPickerDialog) modelPickerDialog.hidden = true;
}

function applyAiConfig(config: AiConfig): void {
  if (aiProvider) aiProvider.value = config.provider;
  if (config.provider === "gemini") {
    geminiAuthMode = config.authMode === "oauth" ? "oauth" : "api_key";
  }
  if (aiApiKey) aiApiKey.value = config.apiKey || getSessionApiKey(config.provider);
  if (aiVisionModel) aiVisionModel.value = config.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = config.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = config.baseUrl;
  if (aiFfmpegPath) aiFfmpegPath.value = config.ffmpegPath;
  if (aiSampleSeconds) aiSampleSeconds.value = String(config.sampleIntervalSeconds);
  if (aiMaxFrames) aiMaxFrames.value = String(config.maxFrames);
  if (aiContextHint) aiContextHint.value = config.contextHint;
  if (aiTranscribeAudio) aiTranscribeAudio.checked = config.transcribeAudio;
  if (aiTranscriptionModel) aiTranscriptionModel.value = config.transcriptionModel;
  if (aiReanalyzeExisting) aiReanalyzeExisting.checked = config.reanalyzeExisting;
  updateAiProviderFields();
  updateActiveModelCard();
}

function updateAiProviderFields(): void {
  const provider = (aiProvider?.value as AiProvider) || "gemini";
  const usesGeminiOauth = provider === "gemini" && geminiAuthMode === "oauth";
  if (aiApiKeyLabel) aiApiKeyLabel.hidden = provider === "local" || usesGeminiOauth;
  if (providerAuthPanel) providerAuthPanel.hidden = provider === "local";
  if (speechSettings) speechSettings.hidden = provider !== "openai";
  if (providerAuthHelp) {
    providerAuthHelp.textContent =
      provider === "openai"
        ? "OpenAI API access uses an API key. A ChatGPT login or subscription does not authorize API requests."
        : provider === "gemini"
          ? usesGeminiOauth
            ? geminiOAuthStatus.connected
              ? `Signed in with Google for Cloud project ${geminiOAuthStatus.project_id ?? "unknown"}. The access token stays in app memory only.`
              : "Google login is selected but not active. Press Login with Google and choose your Desktop OAuth client JSON."
            : "Use a Google AI Studio API key, or sign in with a Google Cloud Desktop OAuth client JSON."
          : "Local Ollama needs no account or API key.";
  }
  if (openProviderKeyPageButton) {
    openProviderKeyPageButton.textContent =
      provider === "openai" ? "Open OpenAI API keys" : "Open Google AI Studio";
  }
  if (openProviderOauthDocsButton) {
    openProviderOauthDocsButton.hidden = provider !== "gemini";
  }
  if (loginGeminiOauthButton) {
    loginGeminiOauthButton.hidden = provider !== "gemini" || (usesGeminiOauth && geminiOAuthStatus.connected);
  }
  if (useGeminiApiKeyButton) {
    useGeminiApiKeyButton.hidden = provider !== "gemini" || !usesGeminiOauth;
  }
  if (logoutGeminiOauthButton) {
    logoutGeminiOauthButton.hidden = provider !== "gemini" || !geminiOAuthStatus.connected;
  }
  if (aiApiKey) {
    aiApiKey.placeholder =
      provider === "local"
        ? "Not needed for Local (Ollama)"
        : provider === "gemini"
          ? "Gemini auth key from Google AI Studio"
          : "OpenAI API key (starts with sk-...)";
  }
  if (aiVisionModel) {
    aiVisionModel.placeholder =
      provider === "local"
        ? "gemma4:e2b"
        : provider === "gemini"
          ? "gemini-3.8-flash"
          : "gpt-5.6-luna";
  }
  if (aiEmbeddingModel) {
    aiEmbeddingModel.placeholder =
      provider === "local"
        ? "embeddinggemma"
        : provider === "gemini"
          ? "gemini-embedding-2"
          : "text-embedding-3-small";
  }
  if (aiBaseUrl) {
    aiBaseUrl.placeholder =
      provider === "local"
        ? "http://127.0.0.1:11434"
        : provider === "gemini"
          ? "https://generativelanguage.googleapis.com/v1beta"
          : "https://api.openai.com/v1";
  }
  updateActiveModelCard();
}

function loadAiConfig(): void {
  const fallback = aiDefaults("gemini");
  try {
    const saved = JSON.parse(
      localStorage.getItem(AI_SETTINGS_STORAGE_KEY) ?? "null",
    ) as Partial<AiConfig> | null;
    const provider =
      saved?.provider === "openai" || saved?.provider === "gemini" || saved?.provider === "local"
        ? saved.provider
        : fallback.provider;
    geminiAuthMode =
      provider === "gemini" && saved?.authMode === "oauth" ? "oauth" : "api_key";
    const sessionApiKey = getSessionApiKey(provider);
    const defaults = aiDefaults(provider);
    let visionModel = normalizeVisionModel(
      provider,
      saved?.visionModel?.trim() || defaults.visionModel,
    );
    let embeddingModel = saved?.embeddingModel?.trim() || defaults.embeddingModel;
    if (provider === "gemini") {
      if (visionModel === "gemini-1.5-flash" || visionModel === "gemini-3.1-pro") {
        visionModel = defaults.visionModel;
      }
      if (["text-embedding-004", "embedding-001"].includes(embeddingModel)) {
        embeddingModel = defaults.embeddingModel;
      }
    }
    let baseUrl = saved?.baseUrl?.trim() || defaults.baseUrl;
    if (provider === "openai" && (baseUrl.includes("googleapis.com") || baseUrl.includes("11434"))) {
      baseUrl = defaults.baseUrl;
    } else if (provider === "gemini" && (baseUrl.includes("api.openai.com") || baseUrl.includes("11434"))) {
      baseUrl = defaults.baseUrl;
    } else if (provider === "local" && (baseUrl.includes("googleapis.com") || baseUrl.includes("api.openai.com"))) {
      baseUrl = defaults.baseUrl;
    }
    if (saved && "apiKey" in saved) {
      const { apiKey: _removedApiKey, ...safeSettings } = saved;
      localStorage.setItem(AI_SETTINGS_STORAGE_KEY, JSON.stringify(safeSettings));
    }
    applyAiConfig({
      ...defaults,
      ...saved,
      provider,
      visionModel,
      embeddingModel,
      baseUrl,
      apiKey: sessionApiKey,
      authMode: provider === "gemini" ? geminiAuthMode : "api_key",
      reanalyzeExisting: false,
    });
  } catch {
    applyAiConfig(fallback);
  }
}

function saveAiConfig(): AiConfig {
  const config = readAiConfig();
  if (aiVisionModel && aiVisionModel.value.trim() !== config.visionModel) {
    aiVisionModel.value = config.visionModel;
  }
  try {
    const { apiKey, reanalyzeExisting: _oneRunOverride, ...safeSettings } = config;
    localStorage.setItem(AI_SETTINGS_STORAGE_KEY, JSON.stringify(safeSettings));
    if (config.authMode === "api_key") setSessionApiKey(config.provider, apiKey);
    if (aiConfigStatus)
      aiConfigStatus.textContent = config.authMode === "oauth"
        ? "Settings saved. Google access stays in app memory only."
        : "Settings saved. API key is stored for this session only.";
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = `Could not save AI settings: ${String(error)}`;
  }
  updateActiveModelCard();
  return config;
}

loadAiConfig();

async function restoreGeminiOAuthStatus(): Promise<void> {
  if (!("__TAURI_INTERNALS__" in window)) return;
  try {
    geminiOAuthStatus = await tauriApi.getGeminiOAuthStatus();
  } catch {
    geminiOAuthStatus = { connected: false, project_id: null, expires_at_unix_ms: null };
  }
  updateAiProviderFields();
}

void restoreGeminiOAuthStatus();

async function restoreSelectedLibrary(): Promise<void> {
  try {
    if (!("__TAURI_INTERNALS__" in window)) return;
    const savedPath = localStorage.getItem(LIBRARY_PATH_STORAGE_KEY)?.trim() ?? "";
    if (!savedPath) {
      renderResults([], false);
      return;
    }
    selectedLibraryPath = savedPath;
    updateHeaderFolder(savedPath);
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
  if (analysisProgressTrack)
    analysisProgressTrack.setAttribute("aria-valuenow", String(safePercent));
  if (analysisProgressFill) analysisProgressFill.style.transform = `scaleX(${safePercent / 100})`;
}

void listen<AiProgress>("ai-progress", ({ payload }) => {
  const fileName = payload.current_file.split(/[\\/]/).pop() ?? payload.current_file;
  const phase = aiStopRequested ? "Stopping after current request…" : payload.phase;
  const elapsed = analysisStartedAt ? Date.now() - analysisStartedAt : 0;
  const remaining = payload.percent > 1
    ? (elapsed * (100 - payload.percent)) / payload.percent
    : 0;
  const phaseWithEta = !aiStopRequested && remaining > 0
    ? `${phase} · ${formatApproximateTime(remaining)} left`
    : phase;
  showAiProgress(payload.percent, phaseWithEta);
  if (libraryStatus) {
    libraryStatus.textContent = aiStopRequested
      ? `Stopping AI analysis · ${payload.completed_files}/${payload.total_files} clips saved`
      : `AI analysis ${payload.percent}% · ${payload.completed_files}/${payload.total_files} clips`;
  }
  if (libraryPath) {
    libraryPath.textContent = `${phase} · ${fileName} · ${payload.provider}`;
  }
});

function updateHeaderFolder(path: string): void {
  const headerFolder = document.querySelector<HTMLElement>("#header-folder-name");
  if (!headerFolder) return;
  if (!path) {
    headerFolder.textContent = "No folder selected";
  } else {
    const parts = path.split(/[\\/]/).filter(Boolean);
    const folderName = parts.pop() ?? path;
    headerFolder.textContent = folderName;
    headerFolder.title = path;
  }
}

function summarizeAiWarnings(warnings: AiIndexReport["warnings"]): string {
  if (warnings.length === 0) return "";
  const count = warnings.length;
  const first = warnings[0];
  const fileName = first.path.split(/[\\/]/).pop() ?? first.path;
  let reason = "temporary API error";
  const msg = first.message.toLowerCase();
  if (msg.includes("503") || msg.includes("high demand") || msg.includes("service unavailable")) {
    reason = "Google service demand spike (503)";
  } else if (msg.includes("429") || msg.includes("quota") || msg.includes("rate limit")) {
    reason = "Rate limit reached (429)";
  } else if (msg.includes("no usable frames") || msg.includes("ffmpeg")) {
    reason = "frame extraction";
  }
  return count === 1
    ? `${fileName} skipped (${reason})`
    : `${count} clips paused (${reason}) · rerun to complete remaining clips`;
}

function updateLibraryViewUi(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-library-view]").forEach((button) => {
    const active = button.dataset.libraryView === libraryView;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  if (panelTitle) {
    panelTitle.textContent = libraryView === "current" ? "Current folder" : "Analyzed archive";
  }
  if (panelSubtitle) {
    panelSubtitle.textContent = libraryView === "current"
      ? "Only the selected folder · grouped by source folder · originals remain untouched"
      : "Previously analyzed clips from every indexed folder · AI data remains saved locally";
  }
}

function setLibraryView(view: "current" | "analyzed"): void {
  libraryView = view;
  updateLibraryViewUi();
  void searchLibrary();
}

function readFilters(): SearchFilters {
  const value = (id: string) => document.querySelector<HTMLInputElement>(id)?.value.trim() ?? "";
  const sortBy = document.querySelector<HTMLSelectElement>("#sort-by")?.value ?? "name";
  const sortDirection =
    document.querySelector<HTMLSelectElement>("#sort-direction")?.value ?? "asc";
  return {
    keyword: value("#search-input") || undefined,
    folder: value("#filter-folder") || undefined,
    root: libraryView === "current" ? selectedLibraryPath || undefined : undefined,
    ai_only: libraryView === "analyzed",
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
    const allowedPath =
      "__TAURI_INTERNALS__" in window
        ? await tauriApi.prepareIndexedMediaPreview(path)
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

function formatFps(raw?: string | null): string {
  if (!raw) return "";
  if (raw.includes("/")) {
    const [num, den] = raw.split("/").map(Number);
    if (den && !isNaN(num) && !isNaN(den) && den !== 0) {
      const fps = num / den;
      return `${Math.round(fps * 100) / 100} fps`;
    }
  }
  const num = Number(raw);
  return !isNaN(num) ? `${Math.round(num * 100) / 100} fps` : `${raw} fps`;
}

function formatMomentRange(startMs = 0, endMs = startMs): string {
  return endMs > startMs
    ? `${formatDuration(startMs)}–${formatDuration(endMs)}`
    : formatDuration(startMs);
}

function renderSavedMomentLabel(label: string): string {
  const separator = label.indexOf(": ");
  if (separator < 0) return `<span class="analysis-label">${escapeHtml(label)}</span>`;
  const category = label.slice(0, separator);
  const value = label.slice(separator + 2);
  return `<span class="analysis-label"><strong>${escapeHtml(category)}</strong>${escapeHtml(value)}</span>`;
}

function closeSavedAnalysis(): void {
  analysisGeneration += 1;
  if (analysisDialog) analysisDialog.hidden = true;
}

async function openSavedAnalysis(
  path: string,
  fileName: string,
  savedSampleCount = 0,
  available = true,
): Promise<void> {
  if (!analysisDialog || !analysisContent || !analysisStatus) return;
  const generation = ++analysisGeneration;
  if (analysisTitle) analysisTitle.textContent = fileName;
  if (analysisPath) analysisPath.textContent = path;
  analysisContent.innerHTML = "";
  analysisStatus.hidden = false;
  analysisStatus.textContent = "Loading saved AI analysis…";
  analysisDialog.hidden = false;

  try {
    const cacheKey = path.toLocaleLowerCase();
    let moments = savedAnalysisCache.get(cacheKey);
    if (!moments) {
      moments = await tauriApi.getSavedAiMoments(path);
      savedAnalysisCache.set(cacheKey, moments);
    }
    if (generation !== analysisGeneration) return;
    if (moments.length === 0) {
      analysisStatus.textContent = "No saved AI analysis was found for this clip.";
      return;
    }

    const groups = new Map<string, SavedAiMoment[]>();
    for (const moment of moments) {
      const group = groups.get(moment.model) ?? [];
      group.push(moment);
      groups.set(moment.model, group);
    }
    const sampleSummary = savedSampleCount > 0
      ? `${savedSampleCount} saved timestamp samples are shown as ${moments.length} contextual ${moments.length === 1 ? "range" : "ranges"}. `
      : `${moments.length} contextual ${moments.length === 1 ? "range" : "ranges"}. `;
    analysisStatus.textContent = `${sampleSummary}${groups.size} model ${groups.size === 1 ? "analysis" : "analyses"} stored locally.`;
    analysisContent.innerHTML = Array.from(groups.entries())
      .map(([model, modelMoments]) => `<section class="analysis-model-group">
        <header>
          <div>
            <span>Analysis model</span>
            <code>${escapeHtml(model)}</code>
          </div>
          <strong>${modelMoments.length} ${modelMoments.length === 1 ? "moment" : "moments"}</strong>
        </header>
        <div class="analysis-moment-list">
          ${modelMoments
            .map((moment, index) => `<article class="analysis-moment-card">
              <div class="analysis-moment-heading">
                <div>
                  <span class="analysis-moment-number">${String(index + 1).padStart(2, "0")}</span>
                  <strong>${escapeHtml(formatMomentRange(moment.timestamp_ms, moment.end_timestamp_ms))}</strong>
                  ${moment.confidence == null ? "" : `<span class="analysis-confidence">${Math.round(moment.confidence * 100)}% confidence</span>`}
                </div>
                <button class="secondary-button analysis-moment-preview" type="button" data-path="${escapeHtml(path)}" data-name="${escapeHtml(fileName)}" data-timestamp-ms="${moment.timestamp_ms}" ${available ? "" : "disabled"}>Preview moment</button>
              </div>
              <p class="analysis-description">${escapeHtml(moment.description)}</p>
              ${moment.labels.length > 0 ? `<div class="analysis-labels">${moment.labels.map(renderSavedMomentLabel).join("")}</div>` : `<p class="analysis-no-labels">No additional context labels were stored.</p>`}
            </article>`)
            .join("")}
        </div>
      </section>`)
      .join("");
    analysisContent.querySelectorAll<HTMLButtonElement>(".analysis-moment-preview").forEach((button) => {
      button.addEventListener("click", () => {
        closeSavedAnalysis();
        void openPreview(
          button.dataset.path ?? "",
          button.dataset.name ?? "Preview",
          Number(button.dataset.timestampMs ?? "0"),
        );
      });
    });
  } catch (error) {
    if (generation !== analysisGeneration) return;
    analysisStatus.textContent = `Could not load saved AI analysis: ${conciseMessage(error)}`;
  }
}

function renderResultCard(result: SearchResult, index: number): string {
  const metadata = result.metadata;
  const fileName = result.path.split(/[\\/]/).pop() ?? result.path;
  const parentFolder = result.path.split(/[\\/]/).slice(0, -1).pop() ?? "Local library";
  const durationMs = metadata?.duration_ms ?? 0;
  const thumbnailKey = `${result.content_hash}:0`;
  const fpsFormatted = formatFps(metadata?.frame_rate);
  const resolution = metadata?.width && metadata?.height ? `${metadata.width}×${metadata.height}` : "";
  const codec = metadata?.video_codec ? metadata.video_codec.toUpperCase() : "";
  const size = formatBytes(result.size_bytes);
  const aiCount = result.ai_annotation_count ?? 0;

  const aiBadge = aiCount > 0
    ? `<span class="relevance-badge badge-ai-indexed">AI · ${aiCount} samples</span>`
    : `<span class="relevance-badge badge-ai-unindexed">Scan only</span>`;

  const metaPills = [
    resolution,
    fpsFormatted,
    codec,
    size,
  ].filter(Boolean).join(" · ");

  return `<article class="video-result-card ${result.available ? "" : "result-card-unavailable"}">
    <button class="video-thumbnail preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(result.path)}" data-timestamp-ms="0" data-thumbnail-key="${escapeHtml(thumbnailKey)}" ${result.available ? "" : "disabled"} aria-label="Preview ${escapeHtml(fileName)}">
      <span class="thumbnail-placeholder">Creating thumbnail…</span>
      <span class="thumbnail-play" aria-hidden="true"><svg viewBox="0 0 20 20"><path d="m7 4 8 6-8 6z"/></svg></span>
      ${durationMs > 0 ? `<span class="thumbnail-time">${escapeHtml(formatDuration(durationMs))}</span>` : ""}
      ${aiBadge}
    </button>
    <div class="video-card-body">
      <div class="video-card-heading">
        <div class="video-card-title-group">
          <span class="video-card-num">${String(index + 1).padStart(2, "0")}</span>
          <h3 title="${escapeHtml(result.path)}">${escapeHtml(fileName)}</h3>
        </div>
        <button class="icon-button open-result" type="button" data-path="${escapeHtml(result.path)}" ${result.available ? "" : "disabled"} title="Open original file" aria-label="Open original ${escapeHtml(fileName)}"><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M8 5h7v7M15 5 7 13"/><path d="M12 10v5H5V8h5"/></svg></button>
      </div>
      <p class="video-card-folder" title="${escapeHtml(result.path)}">${escapeHtml(parentFolder)}</p>
      <div class="video-card-meta">${escapeHtml(metaPills || "Metadata unavailable")}</div>
      ${aiCount > 0 ? `<button class="analysis-disclosure-button view-ai-analysis" type="button" data-path="${escapeHtml(result.path)}" data-name="${escapeHtml(fileName)}" data-sample-count="${aiCount}" data-available="${result.available}"><span>View AI analysis</span><small>${aiCount} saved ${aiCount === 1 ? "sample" : "samples"}</small></button>` : ""}
    </div>
  </article>`;
}

function renderLibraryGroups(results: SearchResult[]): string {
  const groups = new Map<string, { path: string; results: SearchResult[] }>();
  for (const result of results) {
    const parts = result.path.split(/[\\/]/);
    const parentPath = parts.slice(0, -1).join("\\") || "Local library";
    const key = parentPath.toLocaleLowerCase();
    const group = groups.get(key) ?? { path: parentPath, results: [] };
    group.results.push(result);
    groups.set(key, group);
  }

  let itemIndex = 0;
  return Array.from(groups.values())
    .map((group) => {
      const folderName = group.path.split(/[\\/]/).pop() ?? group.path;
      const relativePath = libraryView === "current" && selectedLibraryPath && group.path.toLocaleLowerCase().startsWith(selectedLibraryPath.toLocaleLowerCase())
        ? group.path.slice(selectedLibraryPath.length).replace(/^[\\/]+/, "") || "Selected folder"
        : group.path;
      const totalSize = group.results.reduce((sum, result) => sum + result.size_bytes, 0);
      const analyzedCount = group.results.filter((result) => (result.ai_annotation_count ?? 0) > 0).length;
      const cards = group.results.map((result) => renderResultCard(result, itemIndex++)).join("");

      return `<section class="folder-group">
        <header class="folder-group-header">
          <div class="folder-group-identity">
            <svg class="folder-icon" viewBox="0 0 24 24" aria-hidden="true"><path d="M2.75 6.25h7l2 2h9.5v10.5H2.75z"/><path d="M2.75 8.25v-4h6l2 2"/></svg>
            <div>
              <h2>${escapeHtml(folderName)}</h2>
              <p title="${escapeHtml(group.path)}">${escapeHtml(relativePath)}</p>
            </div>
          </div>
          <div class="folder-group-stats">
            <span>${group.results.length} ${group.results.length === 1 ? "clip" : "clips"}</span>
            <span>${escapeHtml(formatBytes(totalSize))}</span>
            <span>${analyzedCount}/${group.results.length} AI ready</span>
          </div>
        </header>
        <div class="folder-contact-sheet">${cards}</div>
      </section>`;
    })
    .join("");
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
  return groupedResults
    .map((group, groupIndex) => {
      const bestMatch = group[0];
      const fileName = bestMatch.path.split(/[\\/]/).pop() ?? bestMatch.path;
      const parentFolder = bestMatch.path.split(/[\\/]/).slice(0, -1).pop() ?? "Local library";
      const moments = [...group].sort(
        (left, right) => (left.timestamp_ms ?? 0) - (right.timestamp_ms ?? 0),
      );
      const bestScore = Math.max(...group.map((result) => result.match_score ?? 0));
      const relevance =
        groupIndex === 0 ? "Top match" : bestScore >= topScore - 0.04 ? "Strong" : "Related";
      const badgeClass =
        groupIndex === 0 ? "badge-match-top" : bestScore >= topScore - 0.04 ? "badge-match-strong" : "badge-match-related";
      const timestamp = bestMatch.timestamp_ms ?? 0;
      const thumbnailKey = `${bestMatch.content_hash}:${timestamp}`;
      const otherMoments = moments.filter((moment) => moment !== bestMatch);
      const extraMoments = otherMoments.length
        ? `<details class="extra-moments-toggle">
          <summary>${otherMoments.length} more ${otherMoments.length === 1 ? "moment" : "moments"}</summary>
          <div class="extra-moments-list">
            ${otherMoments
              .map(
                (moment) => `<button class="moment-row preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(moment.path)}" data-timestamp-ms="${moment.timestamp_ms ?? 0}" ${moment.available ? "" : "disabled"}>
              <strong>${escapeHtml(formatMomentRange(moment.timestamp_ms ?? 0, moment.end_timestamp_ms ?? moment.timestamp_ms ?? 0))}</strong>
              <span>${escapeHtml(moment.ai_description ?? "Matching scene")}</span>
              <span class="moment-play" aria-hidden="true"><svg viewBox="0 0 20 20"><path d="m7 4 8 6-8 6z"/></svg></span>
            </button>`,
              )
              .join("")}
          </div>
        </details>`
        : "";
      return `<article class="video-result-card ${bestMatch.available ? "" : "result-card-unavailable"}">
      <button class="video-thumbnail preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(bestMatch.path)}" data-timestamp-ms="${timestamp}" data-thumbnail-key="${escapeHtml(thumbnailKey)}" ${bestMatch.available ? "" : "disabled"} aria-label="Preview ${escapeHtml(fileName)} at ${escapeHtml(formatDuration(timestamp))}">
        <span class="thumbnail-placeholder">Creating thumbnail…</span>
        <span class="thumbnail-play" aria-hidden="true"><svg viewBox="0 0 20 20"><path d="m7 4 8 6-8 6z"/></svg></span>
        <span class="thumbnail-time">${escapeHtml(formatMomentRange(timestamp, bestMatch.end_timestamp_ms ?? timestamp))}</span>
        <span class="relevance-badge ${badgeClass}">${escapeHtml(relevance)}</span>
      </button>
      <div class="video-card-body">
        <div class="video-card-heading">
          <div class="video-card-title-group">
            <h3 title="${escapeHtml(fileName)}">${escapeHtml(fileName)}</h3>
          </div>
          <button class="icon-button open-result" type="button" data-path="${escapeHtml(bestMatch.path)}" ${bestMatch.available ? "" : "disabled"} title="Open original file" aria-label="Open original ${escapeHtml(fileName)}"><svg viewBox="0 0 20 20" aria-hidden="true"><path d="M8 5h7v7M15 5 7 13"/><path d="M12 10v5H5V8h5"/></svg></button>
        </div>
        <p class="video-card-folder">${escapeHtml(parentFolder)}</p>
        <p class="video-best-description">${escapeHtml(bestMatch.ai_description ?? "Matching scene")}</p>
        <button class="analysis-disclosure-button view-ai-analysis" type="button" data-path="${escapeHtml(bestMatch.path)}" data-name="${escapeHtml(fileName)}" data-sample-count="${bestMatch.ai_annotation_count ?? 0}" data-available="${bestMatch.available}"><span>Full description &amp; analysis</span><small>All saved context</small></button>
        ${extraMoments}
      </div>
    </article>`;
    })
    .join("");
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
      const placeholder = button.querySelector<HTMLElement>(".thumbnail-placeholder");
      if (!key) continue;
      try {
        let dataUrl = thumbnailCache.get(key);
        if (!dataUrl) {
          dataUrl = await tauriApi.getAiThumbnail(
            button.dataset.path ?? "",
            Number(button.dataset.timestampMs ?? "0"),
            ffmpegPath,
          );
          thumbnailCache.set(key, dataUrl);
        }
        if (generation !== thumbnailGeneration || !button.isConnected) return;
        let image = button.querySelector<HTMLImageElement>("img");
        if (!image) {
          image = document.createElement("img");
          image.alt = button.dataset.name ? `${button.dataset.name} thumbnail` : "Thumbnail";
          button.prepend(image);
        }
        image.src = dataUrl;
        button.classList.add("thumbnail-loaded");
        if (placeholder) placeholder.style.display = "none";
      } catch {
        if (generation !== thumbnailGeneration || !button.isConnected) return;
        button.classList.add("thumbnail-error");
        if (placeholder) placeholder.textContent = "Preview unavailable";
      }
    }
  };
  await Promise.all(Array.from({ length: Math.min(4, buttons.length) }, () => worker()));
}

function renderResults(results: SearchResult[], groupByVideo = false): void {
  if (!resultList || !emptyState) return;
  if (results.length === 0) {
    resultList.hidden = true;
    resultList.classList.remove("ai-result-grid");
    resultList.classList.remove("library-folder-list");
    if (resultsNote) resultsNote.hidden = true;
    const emptyTitle = document.querySelector<HTMLElement>("#empty-title");
    const emptyCopy = document.querySelector<HTMLElement>("#empty-copy");
    if (libraryView === "analyzed") {
      if (emptyTitle) emptyTitle.textContent = groupByVideo ? "No matching analyzed moments" : "No analyzed clips yet";
      if (emptyCopy) emptyCopy.textContent = groupByVideo
        ? "Try another description or switch models to search an index created with a different model."
        : "Analyze a selected folder to add its clips to this saved archive.";
    } else if (selectedLibraryPath) {
      if (emptyTitle) emptyTitle.textContent = groupByVideo ? "No matching visual moments" : "No clips match these filters";
      if (emptyCopy) emptyCopy.textContent = groupByVideo
        ? "Try a broader description, or analyze the folder again with a stronger vision model."
        : "Clear or change the filename and metadata filters to see this library again.";
    } else {
      if (emptyTitle) emptyTitle.textContent = "No folder selected";
      if (emptyCopy) emptyCopy.textContent = "Choose Select Footage Folder. Only that folder will appear in this view; older AI analyses stay in Analyzed archive.";
    }
    emptyState.hidden = false;
    return;
  }
  emptyState.hidden = true;
  resultList.hidden = false;
  resultList.classList.toggle("ai-result-grid", groupByVideo);
  resultList.classList.toggle("library-folder-list", !groupByVideo);
  const visibleResults = results.slice(0, MAX_RENDERED_RESULTS);
  if (resultsNote) {
    resultsNote.hidden = results.length <= MAX_RENDERED_RESULTS;
    resultsNote.textContent = `Showing first ${MAX_RENDERED_RESULTS} of ${results.length} clips.`;
  }
  resultList.innerHTML = groupByVideo
    ? renderGroupedAiResults(visibleResults)
    : renderLibraryGroups(visibleResults);
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
        await tauriApi.openIndexedMediaPath(button.dataset.path ?? "");
      } catch (error) {
        if (libraryStatus) libraryStatus.textContent = "Could not open clip";
        if (libraryPath) libraryPath.textContent = conciseMessage(error);
      }
    });
  });
  resultList.querySelectorAll<HTMLButtonElement>(".view-ai-analysis").forEach((button) => {
    button.addEventListener("click", () => {
      void openSavedAnalysis(
        button.dataset.path ?? "",
        button.dataset.name ?? "AI analysis",
        Number(button.dataset.sampleCount ?? "0"),
        button.dataset.available !== "false",
      );
    });
  });
}

async function searchLibrary(trigger?: HTMLButtonElement): Promise<void> {
  const originalLabel = trigger?.textContent ?? "Search";
  if (trigger) {
    trigger.disabled = true;
    trigger.textContent = "Filtering…";
  }
  if (libraryView === "current" && !selectedLibraryPath) {
    renderResults([], false);
    if (clipCount) clipCount.textContent = "0 clips";
    if (libraryStatus) libraryStatus.textContent = "No folder selected";
    if (libraryPath) libraryPath.textContent = "Choose a local folder to begin";
    if (trigger) {
      trigger.disabled = false;
      trigger.textContent = originalLabel;
    }
    return;
  }
  try {
    const results = await tauriApi.searchMedia(readFilters());
    renderResults(results, false);
    const config = readAiConfig();
    void loadAiThumbnails(config.ffmpegPath);
    const totalClips = results.length;
    const aiIndexedClips = results.filter((r) => (r.ai_annotation_count ?? 0) > 0).length;
    if (clipCount) {
      clipCount.textContent = aiIndexedClips > 0
        ? `${totalClips} clips (${aiIndexedClips} AI indexed)`
        : `${totalClips} clips`;
    }
    if (libraryStatus) {
      libraryStatus.textContent = libraryView === "analyzed"
        ? `${totalClips} analyzed clips saved`
        : `${totalClips} clips in selected folder`;
    }
    if (libraryPath && libraryView === "analyzed") {
      libraryPath.textContent = "Saved AI analysis archive · original folder structure preserved";
    } else if (libraryPath && selectedLibraryPath) {
      libraryPath.textContent = aiIndexedClips > 0
        ? `${selectedLibraryPath} · ${aiIndexedClips}/${totalClips} clips analyzed with AI (saved in SQLite)`
        : `${selectedLibraryPath} · Ready for AI analysis`;
    }
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Search failed";
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
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
  if (libraryView === "current" && !selectedLibraryPath) {
    if (aiSearchStatus) aiSearchStatus.textContent = "Select a footage folder first, or switch to Analyzed archive.";
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
    const matches = await tauriApi.searchAi(
      query,
      config,
      focus,
      libraryView === "current" ? selectedLibraryPath : undefined,
    );
    const displayResults: SearchResult[] = matches.map((match) => ({
      path: match.path,
      content_hash: match.content_hash,
      size_bytes: 0,
      modified_unix_ms: null,
      status: "ACTIVE",
      available: match.available,
      timestamp_ms: match.timestamp_ms,
      end_timestamp_ms: match.end_timestamp_ms,
      ai_description: match.description,
      match_score: match.score,
      metadata: null,
    }));
    renderResults(displayResults, true);
    void loadAiThumbnails(config.ffmpegPath);
    const videoCount = new Set(matches.map((match) => match.content_hash)).size;
    if (clipCount) clipCount.textContent = `${videoCount} videos · ${matches.length} moments`;
    if (libraryStatus)
      libraryStatus.textContent = `${matches.length} AI moments in ${videoCount} videos for “${query}”`;
    if (aiSearchStatus)
      aiSearchStatus.textContent = matches.length
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
  aiAnalysisRunning = true;
  aiCancellationPending = false;
  aiStopRequested = false;
  analyzeAiButton.disabled = true;
  analyzeAiButton.textContent = "Checking cost…";
  if (selectFolderButton) selectFolderButton.disabled = true;
  const config = saveAiConfig();
  let analysisStarted = false;
  let forceReanalysis = config.reanalyzeExisting;
  let plan: AiAnalysisPlan | null = null;
  try {
    plan = await tauriApi.planAiAnalysis(
      selectedLibraryPath,
      config,
      forceReanalysis,
    );

    if (!forceReanalysis && plan.analyze_file_count === 0 && plan.already_analyzed_file_count > 0) {
      const reanalyze = window.confirm(
        `All ${plan.already_analyzed_file_count} clips in this folder are already analyzed with ${plan.model}.\n\n` +
          "Reanalyze them anyway? Existing moments for this model will be replaced, and cloud providers may charge for the new requests.",
      );
      if (!reanalyze) {
        if (libraryStatus) libraryStatus.textContent = "Existing AI analysis kept";
        if (libraryPath) libraryPath.textContent = "No API requests were sent and saved moments were not changed.";
        if (aiSearchStatus) aiSearchStatus.textContent = "Open Analyzed archive to search the saved analysis.";
        return;
      }
      forceReanalysis = true;
      plan = await tauriApi.planAiAnalysis(selectedLibraryPath, config, true);
    }

    const replacesExisting = forceReanalysis && plan.already_analyzed_file_count > 0;
    if (plan.analyze_file_count > 0) {
      const action = replacesExisting ? "reanalyze" : "analyze";
      const skipped = plan.skipped_file_count
        ? `\n${plan.skipped_file_count} already indexed clips will be skipped.`
        : "";
      const replacementWarning = replacesExisting
        ? `\n\nWarning: saved ${plan.model} moments for ${plan.already_analyzed_file_count} clips will be replaced.`
        : "";
      const speechSummary = plan.estimated_audio_seconds > 0
        ? ` Spoken audio: about ${formatApproximateTime(plan.estimated_audio_seconds * 1_000).replace("about ", "")} sent to ${config.transcriptionModel} for timestamped transcription.`
        : "";
      const requestSummary = config.provider === "local"
        ? `${plan.estimated_sampled_frames} estimated sampled frames processed locally.`
        : `${plan.estimated_sampled_frames} estimated sampled frame images in about ${plan.estimated_vision_requests} vision requests. ` +
          `Configured maximum: ${plan.max_sampled_frames} frames in ${plan.max_vision_requests} requests.${speechSummary}`;
      const confirmed = window.confirm(
        `${aiProviderLabel(config.provider)} will ${action} ${plan.analyze_file_count} unique videos.\n\n` +
          `${requestSummary}${skipped}${replacementWarning}\n\n${analysisTimeEstimate(plan, config)}\n\nContinue?`,
      );
      if (!confirmed) {
        if (libraryStatus) libraryStatus.textContent = "AI analysis not started";
        if (libraryPath)
          libraryPath.textContent = "No API requests were sent and no credits were used.";
        if (aiSearchStatus) aiSearchStatus.textContent = "Analysis cancelled before upload.";
        return;
      }
    }

    analysisStarted = true;
    analyzeAiButton.disabled = false;
    analyzeAiButton.textContent = "Stop analysis";
    analyzeAiButton.classList.add("is-stop");
    analyzeAiButton.setAttribute("aria-pressed", "true");
    showAiProgress(0, "Preparing clips");
    if (libraryStatus) libraryStatus.textContent = "AI analysis in progress…";
    if (libraryPath)
      libraryPath.textContent = `Using ${aiProviderLabel(config.provider)} · ${config.visionModel} / ${config.embeddingModel}`;
    analysisStartedAt = Date.now();
    const report = await tauriApi.analyzeMediaFolder(
      selectedLibraryPath,
      config,
      forceReanalysis,
    );
    if (report.analyzed_file_count > 0) savedAnalysisCache.clear();
    const elapsedMs = Date.now() - analysisStartedAt;
    if (!report.cancelled && report.analyzed_file_count > 0) {
      recordAnalysisTiming(config, plan, elapsedMs);
    }
    if (report.cancelled) {
      showAiProgress(lastAiProgressPercent, "Analysis stopped");
      if (libraryStatus)
        libraryStatus.textContent = `AI analysis stopped · ${report.analyzed_file_count} clips saved`;
      if (libraryPath)
        libraryPath.textContent = `${report.annotation_count} new visual moments kept · unstarted clips were not charged`;
      if (aiSearchStatus)
        aiSearchStatus.textContent =
          "Completed clips remain searchable. Start analysis again later to continue with missing clips.";
    } else {
      const warningSuffix = report.warnings.length ? ` · ${report.warnings.length} warnings` : "";
      const warningDetails = summarizeAiWarnings(report.warnings);
      showAiProgress(
        100,
        report.warnings.length ? "Analysis complete with warnings" : "Analysis complete",
      );
      const skippedSuffix = report.skipped_file_count
        ? ` · ${report.skipped_file_count} already ready`
        : "";
      if (libraryStatus)
        libraryStatus.textContent = `AI indexed ${report.analyzed_file_count} clips${skippedSuffix}${warningSuffix}`;
      if (libraryPath) {
        libraryPath.textContent =
          report.analyzed_file_count === 0 && report.skipped_file_count > 0
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
    }
  } catch (error) {
    if (libraryStatus)
      libraryStatus.textContent = analysisStarted
        ? "AI analysis failed"
        : "Could not prepare AI analysis";
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
    if (analysisStarted) showAiProgress(lastAiProgressPercent, "Analysis stopped", true);
  } finally {
    if (analysisStarted && config.reanalyzeExisting && aiReanalyzeExisting) {
      aiReanalyzeExisting.checked = false;
      saveAiConfig();
    }
    analysisStartedAt = 0;
    aiAnalysisRunning = false;
    aiCancellationPending = false;
    aiStopRequested = false;
    analyzeAiButton.disabled = !selectedLibraryPath;
    analyzeAiButton.textContent = "Analyze with AI";
    analyzeAiButton.classList.remove("is-stop");
    analyzeAiButton.setAttribute("aria-pressed", "false");
    if (selectFolderButton) selectFolderButton.disabled = false;
  }
}

async function cancelAiAnalysis(): Promise<void> {
  if (!aiAnalysisRunning || aiCancellationPending || !analyzeAiButton) return;
  aiCancellationPending = true;
  analyzeAiButton.disabled = true;
  analyzeAiButton.textContent = "Stopping…";
  showAiProgress(lastAiProgressPercent, "Stopping after current request…");
  if (libraryStatus) libraryStatus.textContent = "Stopping AI analysis…";
  if (libraryPath)
    libraryPath.textContent =
      "No new files or frame batches will start. The current API request may still finish.";
  try {
    aiStopRequested = await tauriApi.cancelAiAnalysis();
    if (!aiStopRequested && libraryPath) {
      libraryPath.textContent = "Analysis is already finishing.";
    }
  } catch (error) {
    if (libraryStatus) libraryStatus.textContent = "Could not stop AI analysis";
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
    aiStopRequested = false;
    analyzeAiButton.disabled = false;
    analyzeAiButton.textContent = "Stop analysis";
  } finally {
    aiCancellationPending = false;
  }
}

async function testAiConnection(): Promise<void> {
  if (!testAiConnectionButton) return;
  testAiConnectionButton.disabled = true;
  const config = saveAiConfig();
  if (config.provider !== "local" && config.authMode !== "oauth" && !config.apiKey) {
    if (aiConfigStatus)
      aiConfigStatus.textContent = `Please enter your ${aiProviderLabel(config.provider)} API key first.`;
    testAiConnectionButton.disabled = false;
    return;
  }
  if (config.provider === "openai" && config.apiKey.startsWith("AIza")) {
    if (aiConfigStatus)
      aiConfigStatus.textContent = `The entered key starts with 'AIza', which is a Google Gemini key. For OpenAI, please enter an OpenAI key (starts with sk-...).`;
    testAiConnectionButton.disabled = false;
    return;
  }
  if (config.provider === "gemini" && config.authMode === "api_key" && config.apiKey.startsWith("sk-")) {
    if (aiConfigStatus)
      aiConfigStatus.textContent = `The entered key starts with 'sk-', which is an OpenAI key. For Google Gemini, please enter a Google AI Studio key (starts with AIza...).`;
    testAiConnectionButton.disabled = false;
    return;
  }
  if (aiConfigStatus)
    aiConfigStatus.textContent = `Checking ${aiProviderLabel(config.provider)} vision model and ${config.embeddingModel}…`;
  try {
    const report = await tauriApi.testAiConnection(config);
    if (aiConfigStatus) {
      aiConfigStatus.textContent =
        "Connected: " +
        report.provider +
        " · " +
        report.vision_model +
        " / " +
        report.embedding_model +
        " (" +
        report.embedding_dimensions +
        " dimensions)";
    }
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = String(error);
  } finally {
    testAiConnectionButton.disabled = false;
  }
}

// Event Listeners for Model Hub
openModelHubBtn?.addEventListener("click", openModelHub);
closeModelHubBtn?.addEventListener("click", closeModelHub);
modelPickerDialog?.addEventListener("click", (e) => {
  if (e.target === modelPickerDialog) closeModelHub();
});

modelHubTabs?.querySelectorAll<HTMLButtonElement>(".model-hub-tab").forEach((tab) => {
  tab.addEventListener("click", () => {
    modelHubTabs.querySelectorAll(".model-hub-tab").forEach((t) => t.classList.remove("is-active"));
    tab.classList.add("is-active");
    currentHubFilter = (tab.dataset.filter as "all" | AiProvider) || "all";
    renderModelHubGrid(currentHubFilter);
  });
});

aiProvider?.addEventListener("change", () => {
  const provider = (aiProvider.value as AiProvider) || "gemini";
  const defaults = aiDefaults(provider);
  geminiAuthMode = provider === "gemini" && geminiOAuthStatus.connected ? "oauth" : "api_key";
  if (aiApiKey) aiApiKey.value = getSessionApiKey(provider);
  if (aiVisionModel) aiVisionModel.value = defaults.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = defaults.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = defaults.baseUrl;
  updateAiProviderFields();
  saveAiConfig();
});

aiVisionModel?.addEventListener("input", updateActiveModelCard);

aiSettingsForm?.addEventListener("submit", (event) => {
  event.preventDefault();
  saveAiConfig();
});

testAiConnectionButton?.addEventListener("click", () => {
  void testAiConnection();
});

openProviderKeyPageButton?.addEventListener("click", async () => {
  const provider = (aiProvider?.value as AiProvider) || "gemini";
  const url = provider === "openai"
    ? "https://platform.openai.com/api-keys"
    : "https://aistudio.google.com/app/apikey";
  try {
    await openUrl(url);
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = `Could not open provider page: ${conciseMessage(error)}`;
  }
});

openProviderOauthDocsButton?.addEventListener("click", async () => {
  try {
    await openUrl("https://ai.google.dev/gemini-api/docs/oauth");
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = `Could not open OAuth setup guide: ${conciseMessage(error)}`;
  }
});

loginGeminiOauthButton?.addEventListener("click", async () => {
  const selected = await open({
    directory: false,
    multiple: false,
    title: "Select Google Desktop OAuth client JSON",
    filters: [{ name: "Google OAuth client", extensions: ["json"] }],
  });
  if (typeof selected !== "string") return;
  loginGeminiOauthButton.disabled = true;
  if (aiConfigStatus) {
    aiConfigStatus.textContent =
      "Waiting for Google sign-in in your system browser… Return here after approving access.";
  }
  try {
    geminiOAuthStatus = await tauriApi.loginGeminiOAuth(selected);
    geminiAuthMode = "oauth";
    if (aiApiKey) aiApiKey.value = "";
    saveAiConfig();
    updateAiProviderFields();
    if (aiConfigStatus) {
      aiConfigStatus.textContent = `Signed in with Google for Cloud project ${geminiOAuthStatus.project_id ?? "unknown"}. Press Test Connection to verify model access.`;
    }
  } catch (error) {
    geminiOAuthStatus = { connected: false, project_id: null, expires_at_unix_ms: null };
    if (aiConfigStatus) aiConfigStatus.textContent = conciseMessage(error);
    updateAiProviderFields();
  } finally {
    loginGeminiOauthButton.disabled = false;
  }
});

useGeminiApiKeyButton?.addEventListener("click", async () => {
  geminiOAuthStatus = await tauriApi.logoutGeminiOAuth();
  geminiAuthMode = "api_key";
  if (aiApiKey) aiApiKey.value = getSessionApiKey("gemini");
  saveAiConfig();
  updateAiProviderFields();
  if (aiConfigStatus) aiConfigStatus.textContent = "Gemini will use an AI Studio API key.";
});

logoutGeminiOauthButton?.addEventListener("click", async () => {
  geminiOAuthStatus = await tauriApi.logoutGeminiOAuth();
  geminiAuthMode = "api_key";
  saveAiConfig();
  updateAiProviderFields();
  if (aiConfigStatus) aiConfigStatus.textContent = "Google disconnected. The in-memory access token was removed.";
});

document.querySelectorAll<HTMLButtonElement>("[data-library-view]").forEach((button) => {
  button.addEventListener("click", () => {
    const view = button.dataset.libraryView;
    if (view === "current" || view === "analyzed") setLibraryView(view);
  });
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

  libraryView = "current";
  updateLibraryViewUi();
  selectFolderButton.disabled = true;
  if (analyzeAiButton) analyzeAiButton.disabled = true;
  if (libraryStatus) libraryStatus.textContent = "Scanning folder…";
  if (libraryPath) libraryPath.textContent = selected;

  try {
    const report = await tauriApi.indexMediaFolder(selected);
    selectedLibraryPath = selected;
    updateHeaderFolder(selected);
    if (analyzeAiButton) analyzeAiButton.disabled = false;
    if (libraryStatus) {
      libraryStatus.textContent =
        report.warnings.length === 0 ? "Folder indexed" : "Folder indexed with warnings";
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
    if (libraryPath) libraryPath.textContent = conciseMessage(error);
  } finally {
    selectFolderButton.disabled = false;
    if (analyzeAiButton) analyzeAiButton.disabled = aiAnalysisRunning || !selectedLibraryPath;
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
  if (aiAnalysisRunning) void cancelAiAnalysis();
  else void analyzeLibraryWithAi();
});

document.querySelector<HTMLButtonElement>("#learn-more")?.addEventListener("click", () => {
  window.alert("See docs/project-plan.md for the current MVP scope.");
});

updateLibraryViewUi();
renderResults([], false);
void restoreSelectedLibrary();

closePreviewButton?.addEventListener("click", closePreview);
previewDialog?.addEventListener("click", (event) => {
  if (event.target === previewDialog) closePreview();
});
closeAnalysisButton?.addEventListener("click", closeSavedAnalysis);
analysisDialog?.addEventListener("click", (event) => {
  if (event.target === analysisDialog) closeSavedAnalysis();
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
    previewMessage.textContent =
      "This clip cannot be previewed in the embedded player. Use Open to launch it in your system player.";
    previewMessage.hidden = false;
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    if (modelPickerDialog && !modelPickerDialog.hidden) {
      closeModelHub();
    } else if (analysisDialog && !analysisDialog.hidden) {
      closeSavedAnalysis();
    } else if (previewDialog && !previewDialog.hidden) {
      closePreview();
    }
  } else if (event.key === " " && previewDialog && !previewDialog.hidden && previewVideo) {
    if (event.target === document.body || event.target === previewDialog || event.target === previewVideo) {
      event.preventDefault();
      if (previewVideo.paused) void previewVideo.play();
      else previewVideo.pause();
    }
  }
});
