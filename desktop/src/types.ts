export type IndexReport = {
  active_file_count: number;
  changes: Array<{ kind: string; path: string; content_hash?: string }>;
  warnings: Array<{ path: string; message: string }>;
};

export type SearchFilters = {
  keyword?: string;
  folder?: string;
  root?: string;
  ai_only?: boolean;
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

export type SearchResult = {
  path: string;
  content_hash: string;
  size_bytes: number;
  modified_unix_ms: number | null;
  status: "ACTIVE" | "MISSING";
  available: boolean;
  timestamp_ms?: number;
  end_timestamp_ms?: number;
  ai_description?: string;
  match_score?: number;
  ai_annotation_count?: number;
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

export type AiIndexReport = {
  analyzed_file_count: number;
  skipped_file_count: number;
  annotation_count: number;
  cancelled: boolean;
  warnings: Array<{ path: string; message: string }>;
};

export type AiAnalysisPlan = {
  total_file_count: number;
  analyze_file_count: number;
  skipped_file_count: number;
  already_analyzed_file_count: number;
  max_frames_per_file: number;
  max_sampled_frames: number;
  max_vision_requests: number;
  estimated_sampled_frames: number;
  estimated_vision_requests: number;
  estimated_audio_seconds: number;
  estimated_cost: AiCostEstimate;
  model: string;
};

export type AiTokenEstimate = {
  low: number;
  likely: number;
  high: number;
};

export type AiCostEstimate = {
  currency: string;
  pricing_status: "known" | "unknown" | "local";
  estimated_low_usd: number | null;
  estimated_likely_usd: number | null;
  estimated_high_usd: number | null;
  vision_input_tokens: AiTokenEstimate;
  vision_output_tokens: AiTokenEstimate;
  embedding_input_tokens: AiTokenEstimate;
  audio_seconds: number;
  vision_request_reserve_usd: number | null;
  embedding_request_reserve_usd: number | null;
  transcription_request_reserve_usd: number | null;
  pricing_checked_at: string;
  pricing_source: string;
  budget_limit_usd: number | null;
  budget_status: "not_configured" | "within_limit" | "exceeds_limit" | "unknown" | "local";
  assumptions: string[];
};

export type AiProgress = {
  completed_files: number;
  total_files: number;
  current_file: string;
  provider: string;
  percent: number;
  phase: string;
};

export type AiProvider = "local" | "openai" | "gemini";
export type AiAuthMode = "api_key" | "oauth";
export type AiSearchFocus = "focused" | "balanced" | "broad";

export type AiConfig = {
  provider: AiProvider;
  authMode: AiAuthMode;
  apiKey: string;
  visionModel: string;
  embeddingModel: string;
  baseUrl: string;
  ffmpegPath: string;
  sampleIntervalSeconds: number;
  maxFrames: number;
  contextHint: string;
  transcribeAudio: boolean;
  transcriptionModel: string;
  budgetUsd: number | null;
  reanalyzeExisting: boolean;
};

export type GeminiOAuthStatus = {
  connected: boolean;
  project_id: string | null;
  expires_at_unix_ms: number | null;
};

export type AiConnectionReport = {
  provider: string;
  vision_model: string;
  embedding_model: string;
  embedding_dimensions: number;
};

export type AiSearchResult = {
  path: string;
  content_hash: string;
  timestamp_ms: number;
  end_timestamp_ms: number;
  score: number;
  description: string;
  labels: string[];
  available: boolean;
};

export type SavedAiMoment = {
  timestamp_ms: number;
  end_timestamp_ms: number;
  description: string;
  labels: string[];
  confidence: number | null;
  model: string;
};

export type ModelPreset = {
  id: string;
  name: string;
  provider: AiProvider;
  visionModel: string;
  embeddingModel: string;
  badge?: string;
  badgeType: "recommended" | "free" | "pro" | "default";
  pricing: string;
  speed: string;
  accuracy: string;
  summary: string;
  requirements: string;
};

export const ALL_MODEL_PRESETS: ModelPreset[] = [
  {
    id: "gemini-3.8-flash",
    name: "Gemini 3.8 Flash",
    provider: "gemini",
    visionModel: "gemini-3.8-flash",
    embeddingModel: "gemini-embedding-2",
    badge: "Recommended",
    badgeType: "recommended",
    pricing: "Input $0.75 · output $3.75 / 1M tokens (promo through Dec 2026)",
    speed: "Fast",
    accuracy: "High",
    summary: "Google's current stable Flash model with image input for detailed scene and on-screen-text indexing.",
    requirements: "Gemini auth key created in Google AI Studio",
  },
  {
    id: "gemini-3.7-flash",
    name: "Gemini 3.7 Flash",
    provider: "gemini",
    visionModel: "gemini-3.7-flash",
    embeddingModel: "gemini-embedding-2",
    badge: "Previous Stable",
    badgeType: "default",
    pricing: "Input $0.75 · output $3.75 / 1M tokens (promo through Dec 2026)",
    speed: "Fast",
    accuracy: "High",
    summary: "Previous stable Flash release with image input and strong general-purpose visual understanding.",
    requirements: "Gemini auth key created in Google AI Studio",
  },
  {
    id: "gemini-2.5-flash-lite",
    name: "Gemini 2.5 Flash Lite",
    provider: "gemini",
    visionModel: "gemini-2.5-flash-lite",
    embeddingModel: "gemini-embedding-2",
    badge: "Lowest Cost",
    badgeType: "default",
    pricing: "Input $0.10 · output $0.40 / 1M tokens; free tier available",
    speed: "Very fast",
    accuracy: "Standard",
    summary: "Low-cost stable option for broad archives where throughput matters more than subtle visual detail.",
    requirements: "Gemini auth key created in Google AI Studio",
  },
  {
    id: "gemini-2.5-flash",
    name: "Gemini 2.5 Flash",
    provider: "gemini",
    visionModel: "gemini-2.5-flash",
    embeddingModel: "gemini-embedding-2",
    badge: "Proven Balanced",
    badgeType: "default",
    pricing: "Input $0.30 · output $2.50 / 1M tokens; free tier available",
    speed: "Fast",
    accuracy: "High",
    summary: "Mature stable multimodal model for balanced cost, latency, and visual understanding.",
    requirements: "Gemini auth key created in Google AI Studio",
  },
  {
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
    provider: "openai",
    visionModel: "gpt-5.6-luna",
    embeddingModel: "text-embedding-3-small",
    badge: "OpenAI Value",
    badgeType: "recommended",
    pricing: "Input $0.20 · output $1.20 / 1M tokens",
    speed: "Very fast",
    accuracy: "High",
    summary: "Current fast OpenAI multimodal model for economical frame-by-frame library indexing.",
    requirements: "OpenAI API key",
  },
  {
    id: "gpt-5.6-terra",
    name: "GPT-5.6 Terra",
    provider: "openai",
    visionModel: "gpt-5.6-terra",
    embeddingModel: "text-embedding-3-small",
    badge: "Higher Detail",
    badgeType: "pro",
    pricing: "Input $2.00 · output $12.00 / 1M tokens",
    speed: "Moderate",
    accuracy: "Very high",
    summary: "Stronger visual reasoning for nuanced actions, relationships, characters, and scene context.",
    requirements: "OpenAI API key",
  },
  {
    id: "gpt-5.6-sol",
    name: "GPT-5.6 Sol",
    provider: "openai",
    visionModel: "gpt-5.6-sol",
    embeddingModel: "text-embedding-3-small",
    badge: "Maximum Quality",
    badgeType: "pro",
    pricing: "Input $4.00 · output $20.00 / 1M tokens",
    speed: "Slower",
    accuracy: "Maximum",
    summary: "Highest-quality OpenAI option in the catalog for difficult footage where recognition quality matters most.",
    requirements: "OpenAI API key",
  },
  {
    id: "gpt-4o-mini",
    name: "GPT-4o Mini",
    provider: "openai",
    visionModel: "gpt-4o-mini",
    embeddingModel: "text-embedding-3-small",
    badge: "Low Cost",
    badgeType: "default",
    pricing: "Input $0.15 · output $0.60 / 1M tokens",
    speed: "Fast",
    accuracy: "Standard",
    summary: "Low-cost established OpenAI model with image input for broad visual indexing.",
    requirements: "OpenAI API key",
  },
  {
    id: "o4-mini",
    name: "o4-mini",
    provider: "openai",
    visionModel: "o4-mini",
    embeddingModel: "text-embedding-3-small",
    badge: "Legacy",
    badgeType: "default",
    pricing: "Input $1.10 · output $4.40 / 1M tokens",
    speed: "Moderate",
    accuracy: "High reasoning",
    summary: "Older reasoning model with image input. Kept for existing indexes; OpenAI recommends newer successors for new work.",
    requirements: "OpenAI API key",
  },
  {
    id: "gemini-2.5-pro",
    name: "Gemini 2.5 Pro",
    provider: "gemini",
    visionModel: "gemini-2.5-pro",
    embeddingModel: "gemini-embedding-2",
    badge: "Deep Reasoning",
    badgeType: "pro",
    pricing: "Input $1.25 · output $10.00 / 1M tokens up to 200k input",
    speed: "Moderate",
    accuracy: "Very high",
    summary: "Stable higher-reasoning Gemini model for complex visual relationships and narrative context.",
    requirements: "Gemini auth key created in Google AI Studio",
  },
  {
    id: "gemma4-e2b",
    name: "Gemma 4 E2B",
    provider: "local",
    visionModel: "gemma4:e2b",
    embeddingModel: "embeddinggemma",
    badge: "Private Local",
    badgeType: "free",
    pricing: "No API usage fee; runs on this computer",
    speed: "Hardware dependent",
    accuracy: "High",
    summary: "Current compact Gemma 4 image model for private local analysis through Ollama.",
    requirements: "Ollama; pull gemma4:e2b and embeddinggemma",
  },
  {
    id: "llama3.2-vision",
    name: "Llama 3.2 Vision 11B",
    provider: "local",
    visionModel: "llama3.2-vision:11b",
    embeddingModel: "embeddinggemma",
    badge: "Larger Local",
    badgeType: "free",
    pricing: "No API usage fee; runs on this computer",
    speed: "Hardware dependent",
    accuracy: "High",
    summary: "7.8 GB Ollama vision model for local image recognition and reasoning; image prompts are English-first.",
    requirements: "Ollama; pull llama3.2-vision:11b and embeddinggemma",
  },
];

export const MODEL_CATALOG: Record<AiProvider, ModelPreset[]> = {
  openai: ALL_MODEL_PRESETS.filter((p) => p.provider === "openai"),
  gemini: ALL_MODEL_PRESETS.filter((p) => p.provider === "gemini"),
  local: ALL_MODEL_PRESETS.filter((p) => p.provider === "local"),
};
