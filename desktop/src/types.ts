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
  visionModel: string;
  embeddingModel: string;
  costTier: "free" | "budget" | "premium";
  costLabel: string;
  speed: string;
  accuracy: string;
  description: string;
};

export const MODEL_CATALOG: Record<AiProvider, ModelPreset[]> = {
  openai: [
    {
      id: "gpt-4o-mini",
      name: "⭐ GPT-4o mini — Parim hinna/kvaliteedi suhe",
      visionModel: "gpt-4o-mini",
      embeddingModel: "text-embedding-3-small",
      costTier: "budget",
      costLabel: "Ülimalt soodne (~$0.15 / 1M tokenit, alla 0.001€ / klipp)",
      speed: "Välkkiire",
      accuracy: "Väga hea",
      description: "Soovituslik parim valik igapäevaseks videoindekseerimiseks. Tuvastab inimesed, tegevused, kohad ja teksti minimaalse kuluga.",
    },
    {
      id: "gpt-4o",
      name: "🔥 GPT-4o — Lipulaev, maksimaalne detailsus",
      visionModel: "gpt-4o",
      embeddingModel: "text-embedding-3-small",
      costTier: "premium",
      costLabel: "Premium (~$2.50 / 1M tokenit, ~15× kallim kui mini)",
      speed: "Keskmine",
      accuracy: "Tipptase",
      description: "Kasuta keerukate ja peene visuaalse kontekstiga kaadrite süvaanalüüsiks.",
    },
    {
      id: "gpt-5.6-luna",
      name: "⚡ GPT-5.6 Luna — Kiire vaikevalik",
      visionModel: "gpt-5.6-luna",
      embeddingModel: "text-embedding-3-small",
      costTier: "budget",
      costLabel: "Soodne mudel",
      speed: "Kiire",
      accuracy: "Hea",
      description: "Kuluteadlik valik kiireks visuaalseks otsinguks.",
    },
    {
      id: "gpt-5.6-terra",
      name: "💎 GPT-5.6 Terra — Detailne analüüs",
      visionModel: "gpt-5.6-terra",
      embeddingModel: "text-embedding-3-small",
      costTier: "premium",
      costLabel: "Kallim (~10× Luna hind)",
      speed: "Põhjalik",
      accuracy: "Väga detailne",
      description: "Spetsiifiliste ja raskete stseenide eristamiseks.",
    },
    {
      id: "custom",
      name: "✏️ Kohandatud mudel (Custom)",
      visionModel: "",
      embeddingModel: "text-embedding-3-small",
      costTier: "budget",
      costLabel: "Sõltub valitud mudelist",
      speed: "Erinev",
      accuracy: "Erinev",
      description: "Sisesta käsitsi soovitud OpenAI mudelite nimetused.",
    },
  ],
  gemini: [
    {
      id: "gemini-1.5-flash",
      name: "⭐ Gemini 1.5 Flash — Parim hinna/kvaliteedi suhe",
      visionModel: "gemini-1.5-flash",
      embeddingModel: "text-embedding-004",
      costTier: "budget",
      costLabel: "Tasuta limiit olemas / tasuline ~$0.075 / 1M tokenit",
      speed: "Välkkiire",
      accuracy: "Väga hea",
      description: "Turu soodsaima hinnaga pilvemudel. Sisaldab Google'i tasuta päringute limiiti, ideaalne sadade videote indekseerimiseks.",
    },
    {
      id: "gemini-2.0-flash",
      name: "⚡ Gemini 2.0 Flash — Uue põlvkonna kiirus",
      visionModel: "gemini-2.0-flash",
      embeddingModel: "text-embedding-004",
      costTier: "budget",
      costLabel: "Uusim välkkiire mudel (~$0.10 / 1M)",
      speed: "Reaalajas ülikiire",
      accuracy: "Väga hea",
      description: "Google'i uusim kiire multimodaalne mudel madala latentsusega.",
    },
    {
      id: "gemini-1.5-pro",
      name: "🔥 Gemini 1.5 Pro — Süvaanalüüs ja suur kontekst",
      visionModel: "gemini-1.5-pro",
      embeddingModel: "text-embedding-004",
      costTier: "premium",
      costLabel: "Kvaliteet (~$1.25 / 1M tokenit)",
      speed: "Tavaline",
      accuracy: "Maksimaalne mõtlemisvõime",
      description: "Mõeldud pikkade ja keerukate stseenide süvitsi mõistmiseks.",
    },
    {
      id: "custom",
      name: "✏️ Kohandatud mudel (Custom)",
      visionModel: "",
      embeddingModel: "text-embedding-004",
      costTier: "budget",
      costLabel: "Sõltub valitud mudelist",
      speed: "Erinev",
      accuracy: "Erinev",
      description: "Sisesta käsitsi soovitud Google Gemini mudeli kood.",
    },
  ],
  local: [
    {
      id: "llava",
      name: "⭐ LLaVA 7B — Populaarseim kohalik vaikevalik",
      visionModel: "llava",
      embeddingModel: "nomic-embed-text",
      costTier: "free",
      costLabel: "100% Tasuta & privaatne (0€, töötab sinu arvutis)",
      speed: "Kiire (nõuab 6GB+ GPU-d)",
      accuracy: "Väga hea",
      description: "Kõige levinum avatud lähtekoodiga visuaalne mudel Ollamas. Ei saada midagi internetti.",
    },
    {
      id: "moondream",
      name: "⚡ Moondream2 — Kergekaaluline / CPU sõbralik",
      visionModel: "moondream",
      embeddingModel: "all-minilm",
      costTier: "free",
      costLabel: "100% Tasuta & ülimalt kerge (0€)",
      speed: "Välkkiire (Töötab ka tavalisel CPU-l / 2GB VRAM)",
      accuracy: "Põhiline",
      description: "Väike 1.8B mudel, mis töötab sujuvalt isegi ilma eraldiseisva videokaardita tavalises arvutis.",
    },
    {
      id: "llama3.2-vision",
      name: "🔥 Llama 3.2 Vision — Tipptasemel kohalik mudel",
      visionModel: "llama3.2-vision",
      embeddingModel: "nomic-embed-text",
      costTier: "free",
      costLabel: "100% Tasuta (nõuab 8GB+ GPU-d)",
      speed: "Keskmine",
      accuracy: "Tipptase kohalike seas",
      description: "Meta võimas visuaalne mudel, parim kirjeldustäpsus ilma pilveteenusteta.",
    },
    {
      id: "gemma4",
      name: "Gemma 4 — Kompaktne kohalik",
      visionModel: "gemma4",
      embeddingModel: "embeddinggemma",
      costTier: "free",
      costLabel: "100% Tasuta",
      speed: "Kiire",
      accuracy: "Hea",
      description: "Google Gemma kohalik mudel Ollama kaudu.",
    },
    {
      id: "custom",
      name: "✏️ Kohandatud kohalik mudel (Custom)",
      visionModel: "",
      embeddingModel: "nomic-embed-text",
      costTier: "free",
      costLabel: "100% Tasuta (Ollama)",
      speed: "Sõltub riistvarast",
      accuracy: "Sõltub mudelist",
      description: "Sisesta oma kohalikku Ollamasse tõmmatud mudeli nimi (nt qwen2-vl).",
    },
  ],
};
