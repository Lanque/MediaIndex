import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
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
  type IndexReport,
  type ModelPreset,
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

        <details class="ai-settings" open>
          <summary>AI Connection & Model</summary>
          <p class="settings-help">
            Choose a cloud or local vision model. Media footage always stays on your computer.
          </p>

          <div class="active-model-card" id="active-model-card">
            <div class="active-model-header">
              <span class="eyebrow" style="margin: 0; font-size: 0.68rem;">Selected Model</span>
              <span class="model-badge badge-recommended" id="active-model-badge">Recommended</span>
            </div>
            <div class="active-model-title" id="active-model-title">
              <strong id="active-model-name">Gemini 1.5 Flash</strong>
            </div>
            <p id="active-model-cost" style="margin: 0; font-size: 0.72rem; color: #a8bfeb;">Free tier / $0.075 per 1M tokens</p>
            <button class="open-model-hub-btn" id="open-model-hub" type="button">
              Change Model & View Pricing...
            </button>
          </div>

          <form class="settings-form" id="ai-settings-form">
            <label id="ai-api-key-label">API Key
              <input id="ai-api-key" name="api-key" type="password" autocomplete="off" placeholder="Enter Gemini or OpenAI API key" />
            </label>

            <details class="advanced-ai-settings" style="margin-top: 4px;">
              <summary style="font-size: 0.72rem; color: #8fa6d8; cursor: pointer;">Advanced Configuration</summary>
              <div style="display: grid; gap: 8px; margin-top: 8px;">
                <label>Provider
                  <select id="ai-provider" name="provider">
                    <option value="gemini">Google Gemini API</option>
                    <option value="openai">OpenAI API</option>
                    <option value="local">Local (Ollama)</option>
                  </select>
                </label>
                <label>Vision model
                  <input id="ai-vision-model" name="vision-model" placeholder="gemini-1.5-flash" />
                </label>
                <label>Embedding model
                  <input id="ai-embedding-model" name="embedding-model" placeholder="text-embedding-004" />
                </label>
                <label>Base URL
                  <input id="ai-base-url" name="base-url" placeholder="http://127.0.0.1:11434" />
                </label>
                <label>FFmpeg path <span class="optional-label">(optional)</span>
                  <input id="ai-ffmpeg-path" name="ffmpeg-path" placeholder="Uses PATH if empty" />
                </label>
                <label>Library context <span class="optional-label">(optional)</span>
                  <input id="ai-context-hint" name="context-hint" placeholder="Project, franchise, characters, location..." />
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
              </div>
            </details>

            <div class="settings-actions">
              <button class="secondary-button" id="save-ai-settings" type="submit">Save Settings</button>
              <button class="secondary-button" id="test-ai-connection" type="button">Test Connection</button>
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

  <!-- AI Model Hub Modal -->
  <div class="preview-backdrop" id="model-picker-dialog" hidden>
    <section class="model-hub-modal" role="dialog" aria-modal="true" aria-labelledby="model-hub-title">
      <div class="preview-header">
        <div>
          <p class="eyebrow" style="margin-bottom: 4px;">AI MODEL SELECTION & COMPARISON</p>
          <h2 id="model-hub-title" style="font-size: 1.35rem;">Choose a Vision Model</h2>
          <p class="lede" style="font-size: 0.85rem; margin-top: 4px; color: #8fa6d8;">
            Select a model based on cost, speed, and accuracy requirements.
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
const aiVisionModel = document.querySelector<HTMLInputElement>("#ai-vision-model");
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
let aiAnalysisRunning = false;
let aiCancellationPending = false;
let aiStopRequested = false;
let currentHubFilter: "all" | AiProvider = "all";
const thumbnailCache = new Map<string, string>();

const AI_SETTINGS_STORAGE_KEY = "mediaindex.ai.settings.v1";
const AI_API_KEY_SESSION_STORAGE_KEY = "mediaindex.ai.api-key.session.v1";
const LIBRARY_PATH_STORAGE_KEY = "mediaindex.library.path.v1";

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
      apiKey: "",
      visionModel: "gpt-4o-mini",
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
      visionModel: "gemini-3.7-flash",
      embeddingModel: "text-embedding-004",
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
    visionModel: "llava",
    embeddingModel: "nomic-embed-text",
    baseUrl: "http://127.0.0.1:11434",
    ffmpegPath: "",
    sampleIntervalSeconds: 5,
    maxFrames: 120,
    contextHint: "",
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
    apiKey: cleanApiKey(aiApiKey?.value ?? ""),
    visionModel: aiVisionModel?.value.trim() || defaults.visionModel,
    embeddingModel: aiEmbeddingModel?.value.trim() || defaults.embeddingModel,
    baseUrl,
    ffmpegPath: aiFfmpegPath?.value.trim() ?? "",
    sampleIntervalSeconds: Math.max(1, Number(aiSampleSeconds?.value ?? 5) || 5),
    maxFrames: Math.max(1, Number(aiMaxFrames?.value ?? defaults.maxFrames) || defaults.maxFrames),
    contextHint: aiContextHint?.value.trim() ?? "",
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
  if (preset.id === "gpt-4o-mini" || preset.id.includes("flash")) {
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
  if (aiApiKey) aiApiKey.value = config.apiKey || getSessionApiKey(config.provider);
  if (aiVisionModel) aiVisionModel.value = config.visionModel;
  if (aiEmbeddingModel) aiEmbeddingModel.value = config.embeddingModel;
  if (aiBaseUrl) aiBaseUrl.value = config.baseUrl;
  if (aiFfmpegPath) aiFfmpegPath.value = config.ffmpegPath;
  if (aiSampleSeconds) aiSampleSeconds.value = String(config.sampleIntervalSeconds);
  if (aiMaxFrames) aiMaxFrames.value = String(config.maxFrames);
  if (aiContextHint) aiContextHint.value = config.contextHint;
  if (aiReanalyzeExisting) aiReanalyzeExisting.checked = config.reanalyzeExisting;
  updateAiProviderFields();
  updateActiveModelCard();
}

function updateAiProviderFields(): void {
  const provider = (aiProvider?.value as AiProvider) || "gemini";
  if (aiApiKeyLabel) aiApiKeyLabel.hidden = provider === "local";
  if (aiApiKey) {
    aiApiKey.placeholder =
      provider === "local"
        ? "Not needed for Local (Ollama)"
        : provider === "gemini"
          ? "Google Gemini API key (starts with AIza...)"
          : "OpenAI API key (starts with sk-...)";
  }
  if (aiVisionModel) {
    aiVisionModel.placeholder =
      provider === "local"
        ? "llava"
        : provider === "gemini"
          ? "gemini-3.7-flash"
          : "gpt-4o-mini";
  }
  if (aiEmbeddingModel) {
    aiEmbeddingModel.placeholder =
      provider === "local"
        ? "nomic-embed-text"
        : provider === "gemini"
          ? "text-embedding-004"
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
    const sessionApiKey = getSessionApiKey(provider);
    const defaults = aiDefaults(provider);
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
      baseUrl,
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
    setSessionApiKey(config.provider, apiKey);
    if (aiConfigStatus)
      aiConfigStatus.textContent = "Settings saved. API key is stored for this session only.";
  } catch (error) {
    if (aiConfigStatus) aiConfigStatus.textContent = `Could not save AI settings: ${String(error)}`;
  }
  updateActiveModelCard();
  return config;
}

loadAiConfig();

async function restoreSelectedLibrary(): Promise<void> {
  try {
    if (!("__TAURI_INTERNALS__" in window)) return;
    let savedPath = localStorage.getItem(LIBRARY_PATH_STORAGE_KEY)?.trim() ?? "";
    if (!savedPath) {
      savedPath = (await tauriApi.getIndexedLibraryPath())?.trim() ?? "";
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
  if (analysisProgressTrack)
    analysisProgressTrack.setAttribute("aria-valuenow", String(safePercent));
  if (analysisProgressFill) analysisProgressFill.style.width = `${safePercent}%`;
}

void listen<AiProgress>("ai-progress", ({ payload }) => {
  const fileName = payload.current_file.split(/[\\/]/).pop() ?? payload.current_file;
  const phase = aiStopRequested ? "Stopping after current request…" : payload.phase;
  showAiProgress(payload.percent, phase);
  if (libraryStatus) {
    libraryStatus.textContent = aiStopRequested
      ? `Stopping AI analysis · ${payload.completed_files}/${payload.total_files} clips saved`
      : `AI analysis ${payload.percent}% · ${payload.completed_files}/${payload.total_files} clips`;
  }
  if (libraryPath) {
    libraryPath.textContent = `${phase} · ${fileName} · ${payload.provider}`;
  }
});

function summarizeAiWarnings(warnings: AiIndexReport["warnings"]): string {
  if (warnings.length === 0) return "";
  const first = warnings[0];
  const fileName = first.path.split(/[\\/]/).pop() ?? first.path;
  const remaining = warnings.length > 1 ? ` · ${warnings.length - 1} more` : "";
  return `${fileName}: ${conciseMessage(first.message)}${remaining}`;
}

function readFilters(): SearchFilters {
  const value = (id: string) => document.querySelector<HTMLInputElement>(id)?.value.trim() ?? "";
  const sortBy = document.querySelector<HTMLSelectElement>("#sort-by")?.value ?? "name";
  const sortDirection =
    document.querySelector<HTMLSelectElement>("#sort-direction")?.value ?? "asc";
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

function renderResultCard(result: SearchResult, index: number): string {
  const metadata = result.metadata;
  const details = result.ai_description
    ? `AI match ${Math.round((result.match_score ?? 0) * 100)}% · ${result.ai_description}`
    : metadata
      ? `${formatDuration(metadata.duration_ms)} · ${formatBytes(result.size_bytes)} · ${metadata.width ?? "?"}×${metadata.height ?? "?"} · ${metadata.frame_rate ?? "?"} fps · ${metadata.video_codec ?? "?"}`
      : "Technical metadata unavailable";
  const status = result.available ? "Available" : "Unavailable — rescan or restore this path";
  const fileName = result.path.split(/[\\/]/).pop() ?? result.path;
  const previewLabel = result.timestamp_ms
    ? `Preview @ ${formatDuration(result.timestamp_ms)}`
    : "Preview";
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
      const timestamp = bestMatch.timestamp_ms ?? 0;
      const thumbnailKey = `${bestMatch.content_hash}:${timestamp}`;
      const otherMoments = moments.filter((moment) => moment !== bestMatch);
      const extraMoments = otherMoments.length
        ? `<details class="video-moments">
          <summary>${otherMoments.length} more ${otherMoments.length === 1 ? "moment" : "moments"}</summary>
          <div class="moment-list">
            ${otherMoments
              .map(
                (moment) => `<button class="moment-row preview-result" type="button" data-name="${escapeHtml(fileName)}" data-path="${escapeHtml(moment.path)}" data-timestamp-ms="${moment.timestamp_ms ?? 0}" ${moment.available ? "" : "disabled"}>
              <strong>${escapeHtml(formatDuration(moment.timestamp_ms))}</strong>
              <span>${escapeHtml(moment.ai_description ?? "Matching scene")}</span>
              <span aria-hidden="true">▶</span>
            </button>`,
              )
              .join("")}
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
      const image = button.querySelector<HTMLImageElement>("img");
      const placeholder = button.querySelector<HTMLElement>(".thumbnail-placeholder");
      if (!key || !image) continue;
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
        await tauriApi.openIndexedMediaPath(button.dataset.path ?? "");
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
    const results = await tauriApi.searchMedia(readFilters());
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
    const matches = await tauriApi.searchAi(query, config, focus);
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
  try {
    const plan = await tauriApi.planAiAnalysis(
      selectedLibraryPath,
      config,
      config.reanalyzeExisting,
    );
    if (config.provider !== "local" && plan.analyze_file_count > 0) {
      const action = config.reanalyzeExisting ? "reanalyze" : "analyze";
      const skipped = plan.skipped_file_count
        ? `\n${plan.skipped_file_count} already indexed clips will be skipped.`
        : "";
      const confirmed = window.confirm(
        `${aiProviderLabel(config.provider)} will ${action} ${plan.analyze_file_count} unique videos.\n\n` +
          `Maximum configured upload: ${plan.max_sampled_frames} sampled frame images in up to ${plan.max_vision_requests} vision batches ` +
          `(${plan.max_frames_per_file} frames per video). Short clips may use less.${skipped}\n\nContinue?`,
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
    const report = await tauriApi.analyzeMediaFolder(
      selectedLibraryPath,
      config,
      config.reanalyzeExisting,
    );
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
  if (config.provider !== "local" && !config.apiKey) {
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
  if (config.provider === "gemini" && config.apiKey.startsWith("sk-")) {
    if (aiConfigStatus)
      aiConfigStatus.textContent = `The entered key starts with 'sk-', which is an OpenAI key. For Google Gemini, please enter a Google AI Studio key (starts with AIza...).`;
    testAiConnectionButton.disabled = false;
    return;
  }
  if (aiConfigStatus)
    aiConfigStatus.textContent = `Testing ${aiProviderLabel(config.provider)} · ${config.embeddingModel}…`;
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
  if (analyzeAiButton) analyzeAiButton.disabled = true;
  if (libraryStatus) libraryStatus.textContent = "Scanning folder…";
  if (libraryPath) libraryPath.textContent = selected;

  try {
    const report = await tauriApi.indexMediaFolder(selected);
    selectedLibraryPath = selected;
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
    previewMessage.textContent =
      "This clip cannot be previewed in the embedded player. Use Open to launch it in your system player.";
    previewMessage.hidden = false;
  }
});
document.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    if (modelPickerDialog && !modelPickerDialog.hidden) {
      closeModelHub();
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
