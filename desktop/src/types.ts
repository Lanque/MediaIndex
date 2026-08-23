export type IndexReport = {
  active_file_count: number;
  changes: Array<{ kind: string; path: string; content_hash?: string }>;
  warnings: Array<{ path: string; message: string }>;
};

export type SearchFilters = {
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

export type SearchResult = {
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

export type AiIndexReport = {
  analyzed_file_count: number;
  skipped_file_count: number;
  annotation_count: number;
  cancelled: boolean;
  warnings: Array<{ path: string; message: string }>;
};

export type AiAnalysisPlan = {
  analyze_file_count: number;
  skipped_file_count: number;
  max_frames_per_file: number;
  max_sampled_frames: number;
  max_vision_requests: number;
  model: string;
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
export type AiSearchFocus = "focused" | "balanced" | "broad";

export type AiConfig = {
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
  score: number;
  description: string;
  labels: string[];
  available: boolean;
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
    id: "gemini-3.7-flash",
    name: "Gemini 3.7 Flash",
    provider: "gemini",
    visionModel: "gemini-3.7-flash",
    embeddingModel: "text-embedding-004",
    badge: "Frontier Recommended",
    badgeType: "recommended",
    pricing: "Free quota / ~$0.075 per 1M tokens (~$0.0004 / clip)",
    speed: "Ultra-fast",
    accuracy: "Maximum",
    summary: "Google's state-of-the-art multimodal model. Best-in-class visual comprehension, frame-by-frame entity tracking, and high-speed execution.",
    requirements: "Google Gemini API key",
  },
  {
    id: "gemini-3.5-flash-lite",
    name: "Gemini 3.5 Flash Lite",
    provider: "gemini",
    visionModel: "gemini-3.5-flash-lite",
    embeddingModel: "text-embedding-004",
    badge: "Lowest Cost",
    badgeType: "recommended",
    pricing: "~$0.035 per 1M tokens (Ultra-budget)",
    speed: "Instant",
    accuracy: "High",
    summary: "Ultra-high throughput, lowest cost vision model. Built for indexing massive archives with minimal API spending.",
    requirements: "Google Gemini API key",
  },
  {
    id: "gemini-3.1-pro",
    name: "Gemini 3.1 Pro",
    provider: "gemini",
    visionModel: "gemini-3.1-pro",
    embeddingModel: "text-embedding-004",
    badge: "Deep Reasoning",
    badgeType: "pro",
    pricing: "$1.25 per 1M tokens",
    speed: "Moderate",
    accuracy: "Maximum",
    summary: "Google's flagship deep reasoning vision model with 2M+ token context for nuanced artistic and narrative video searches.",
    requirements: "Google Gemini API key",
  },
  {
    id: "gemini-2.5-flash",
    name: "Gemini 2.5 Flash",
    provider: "gemini",
    visionModel: "gemini-2.5-flash",
    embeddingModel: "text-embedding-004",
    badge: "Proven Fast",
    badgeType: "default",
    pricing: "~$0.075 per 1M tokens",
    speed: "Ultra-fast",
    accuracy: "Very High",
    summary: "Proven workhorse multimodal model with high reliability across all video formats.",
    requirements: "Google Gemini API key",
  },
  {
    id: "gpt-4o-mini",
    name: "GPT-4o mini",
    provider: "openai",
    visionModel: "gpt-4o-mini",
    embeddingModel: "text-embedding-3-small",
    badge: "Best OpenAI Value",
    badgeType: "recommended",
    pricing: "$0.15 per 1M tokens (~$0.001 / clip)",
    speed: "Fast",
    accuracy: "High",
    summary: "OpenAI's most cost-effective vision model. High-precision scene description and action detection at minimal cost.",
    requirements: "OpenAI API key",
  },
  {
    id: "llava",
    name: "LLaVA 7B",
    provider: "local",
    visionModel: "llava",
    embeddingModel: "nomic-embed-text",
    badge: "Free & Private",
    badgeType: "free",
    pricing: "100% Free (Runs locally on GPU)",
    speed: "Fast (with GPU)",
    accuracy: "Standard",
    summary: "Most popular open-source local vision model. Analyzes video entirely on your machine with zero network traffic or API fees.",
    requirements: "Ollama with 6GB+ VRAM GPU",
  },
  {
    id: "moondream",
    name: "Moondream2",
    provider: "local",
    visionModel: "moondream",
    embeddingModel: "all-minilm",
    badge: "CPU / Lightweight",
    badgeType: "free",
    pricing: "100% Free (Runs locally on CPU/GPU)",
    speed: "Fast (Low resource)",
    accuracy: "Basic",
    summary: "Ultra-compact 1.8B vision model. Runs smoothly on standard CPUs and laptops without requiring a dedicated graphics card.",
    requirements: "Ollama (Runs on standard CPU / 2GB RAM)",
  },
  {
    id: "llama3.2-vision",
    name: "Llama 3.2 Vision",
    provider: "local",
    visionModel: "llama3.2-vision",
    embeddingModel: "nomic-embed-text",
    badge: "Pro Local",
    badgeType: "free",
    pricing: "100% Free (Runs locally on GPU)",
    speed: "Moderate",
    accuracy: "Very High",
    summary: "Meta's flagship open-weights vision model. Near cloud-grade scene understanding fully offline.",
    requirements: "Ollama with 8GB+ VRAM GPU",
  },
  {
    id: "gpt-4o",
    name: "GPT-4o",
    provider: "openai",
    visionModel: "gpt-4o",
    embeddingModel: "text-embedding-3-small",
    badge: "Flagship",
    badgeType: "pro",
    pricing: "$2.50 per 1M tokens (~15× mini)",
    speed: "Moderate",
    accuracy: "Maximum",
    summary: "OpenAI's flagship multimodal model. Maximum fidelity for complex visual details, nuanced interactions, and subtle events.",
    requirements: "OpenAI API key",
  },
  {
    id: "gemini-2.5-pro",
    name: "Gemini 2.5 Pro",
    provider: "gemini",
    visionModel: "gemini-2.5-pro",
    embeddingModel: "text-embedding-004",
    badge: "Deep Reasoning",
    badgeType: "pro",
    pricing: "$1.25 per 1M tokens",
    speed: "Moderate",
    accuracy: "Maximum",
    summary: "Deep multimodal reasoning with massive context window for demanding artistic and narrative visual searches.",
    requirements: "Google Gemini API key",
  },
  {
    id: "gemini-1.5-flash",
    name: "Gemini 1.5 Flash",
    provider: "gemini",
    visionModel: "gemini-1.5-flash",
    embeddingModel: "text-embedding-004",
    badge: "Legacy Default",
    badgeType: "default",
    pricing: "Free quota / ~$0.075 per 1M tokens",
    speed: "Fast",
    accuracy: "High",
    summary: "Stable previous-generation Gemini flash model with broad compatibility across all API keys.",
    requirements: "Google Gemini API key",
  },
];

export const MODEL_CATALOG: Record<AiProvider, ModelPreset[]> = {
  openai: ALL_MODEL_PRESETS.filter((p) => p.provider === "openai"),
  gemini: ALL_MODEL_PRESETS.filter((p) => p.provider === "gemini"),
  local: ALL_MODEL_PRESETS.filter((p) => p.provider === "local"),
};
