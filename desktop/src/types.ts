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
  costTier: "free" | "budget" | "premium";
  highlightBadge?: string;
  costLabel: string;
  speed: string;
  speedRating: number; // 1-5
  accuracy: string;
  accuracyRating: number; // 1-5
  description: string;
  recommendedFor: string;
  requirements: string;
  pros: string[];
};

export const ALL_MODEL_PRESETS: ModelPreset[] = [
  // Gemini Models (Best value & speed)
  {
    id: "gemini-1.5-flash",
    name: "Gemini 1.5 Flash",
    provider: "gemini",
    visionModel: "gemini-1.5-flash",
    embeddingModel: "text-embedding-004",
    costTier: "budget",
    highlightBadge: "⭐ SOOVITATUD · PARIM HIND",
    costLabel: "Tasuta limiit / ~$0.075 / 1M tokenit (0.0004€ / video)",
    speed: "Välkkiire (< 1 sek kaader)",
    speedRating: 5,
    accuracy: "Väga kõrge (Multimodal 1.5)",
    accuracyRating: 5,
    description: "Turu parima hinna ja kiirusega pilvemudel. Google pakub heldet tasuta limiiti ning tasuline kasutus maksab tuhandete videote puhul vaid sente.",
    recommendedFor: "Parim valik 95% kasutajatest: kiire, üliodav ja väga täpne.",
    requirements: "Tasuta Google AI Studio API võti (saadaval paari klikiga)",
    pros: ["Google tasuta päringute limiit", "Tuvastab mängud, näod, tegevused ja teksti", "Tohutu 1M kontekstiaken"],
  },
  {
    id: "gemini-2.0-flash",
    name: "Gemini 2.0 Flash",
    provider: "gemini",
    visionModel: "gemini-2.0-flash",
    embeddingModel: "text-embedding-004",
    costTier: "budget",
    highlightBadge: "⚡ UUS PÕLVKOND · ÜLIKIIRE",
    costLabel: "Ülisoodne (~$0.10 / 1M tokenit)",
    speed: "Reaalajas välkkiire",
    speedRating: 5,
    accuracy: "Tipptase (Uusim Gemini 2.0)",
    accuracyRating: 5,
    description: "Google'i värskeim reaalajas multimodaalne mudel. Erakordselt madal latentsus ja kõrge detailituvastus.",
    recommendedFor: "Suurte videokogude kiireks läbitöötamiseks ilma ootamiseta.",
    requirements: "Tasuta Google AI Studio API võti",
    pros: ["Madalaim latentsus", "Väga terav objektide eristus", "Soodne hind"],
  },
  {
    id: "gemini-1.5-pro",
    name: "Gemini 1.5 Pro",
    provider: "gemini",
    visionModel: "gemini-1.5-pro",
    embeddingModel: "text-embedding-004",
    costTier: "premium",
    highlightBadge: "🔥 SÜVAANALÜÜS",
    costLabel: "Kvaliteet (~$1.25 / 1M tokenit, ~15× Flash)",
    speed: "Mõõdukas (Põhjalik)",
    speedRating: 3,
    accuracy: "Maksimaalne mõtlemisvõime",
    accuracyRating: 5,
    description: "Google'i tipptasemel mudel, mis suudab analüüsida keerulisi ja peenekoelisi stseene ning emotsioone ja nüansse.",
    recommendedFor: "Keerukate dokumentaalide ja detailirikaste kunstiliste kaadrite süvaanalüüsiks.",
    requirements: "Google API võti",
    pros: ["Põhjalik kontekstitaju", "Tipptasemel arutlusvõime", "Suur 2M kontekst"],
  },

  // OpenAI Models
  {
    id: "gpt-4o-mini",
    name: "GPT-4o mini",
    provider: "openai",
    visionModel: "gpt-4o-mini",
    embeddingModel: "text-embedding-3-small",
    costTier: "budget",
    highlightBadge: "⭐ PARIM OPENAI VALIK",
    costLabel: "Ülimalt soodne (~$0.15 / 1M tokenit, alla 0.001€ / klipp)",
    speed: "Välkkiire",
    speedRating: 5,
    accuracy: "Väga kõrge",
    accuracyRating: 5,
    description: "OpenAI parim ja kulutõhusaim nägemismudel. Väga kiire, odav ja suudab tuvastada inimesi, tegevusi, objekte ning ekraaniteksti.",
    recommendedFor: "Kõikidele OpenAI kasutajatele, kes soovivad hoida kulud minimaalsed.",
    requirements: "OpenAI API võti (kulu tavaliselt mõni sent kuus)",
    pros: ["15× soodsam kui täis-GPT-4o", "Usaldusväärne tekstituvastus", "Kiire vastamisaeg"],
  },
  {
    id: "gpt-4o",
    name: "GPT-4o (Omni)",
    provider: "openai",
    visionModel: "gpt-4o",
    embeddingModel: "text-embedding-3-small",
    costTier: "premium",
    highlightBadge: "💎 OPENAI LIPULAEV",
    costLabel: "Premium (~$2.50 / 1M tokenit, ~15× kallim)",
    speed: "Keskmine",
    speedRating: 4,
    accuracy: "Maksimaalne täpsus",
    accuracyRating: 5,
    description: "OpenAI võimsaim multimodaalne lipulaev. Äärmiselt täpne detailide ja peente visuaalsete vihjete leidmisel.",
    recommendedFor: "Väikeste oluliste videote süvaotsinguks, kus hind pole esmatähtis.",
    requirements: "OpenAI API võti",
    pros: ["Maksimaalne visuaalne teravus", "Tipptasemel arutlus", "Parim objektide peendetailides"],
  },
  {
    id: "gpt-5.6-luna",
    name: "GPT-5.6 Luna",
    provider: "openai",
    visionModel: "gpt-5.6-luna",
    embeddingModel: "text-embedding-3-small",
    costTier: "budget",
    highlightBadge: "⚡ KIIRE VAIKEVALIK",
    costLabel: "Soodne eelarvemudel",
    speed: "Kiire",
    speedRating: 4,
    accuracy: "Hea",
    accuracyRating: 4,
    description: "Kergekaaluline OpenAI mudel kiireks visuaalseks otsinguks.",
    recommendedFor: "Igapäevane skännimine OpenAI infrastruktuuris.",
    requirements: "OpenAI API võti",
    pros: ["Madal hind", "Kiire", "Sobib põhituvastuseks"],
  },

  // Local Models (100% Free, Offline, Private)
  {
    id: "llava",
    name: "LLaVA 7B (Ollama)",
    provider: "local",
    visionModel: "llava",
    embeddingModel: "nomic-embed-text",
    costTier: "free",
    highlightBadge: "🟢 100% TASUTA & PRIVHAATNE",
    costLabel: "Täiesti tasuta (0€, töötab lokaalselt arvutis)",
    speed: "Kiire (sõltub videokaardist)",
    speedRating: 4,
    accuracy: "Väga hea",
    accuracyRating: 4,
    description: "Kõige populaarsem avatud lähtekoodiga visuaalne mudel Ollamas. Töötab täielikult sinu arvutis, ei vaja internetti ega API krediite.",
    recommendedFor: "Privaatsust hindavale kasutajale, kellel on vähemalt 6 GB VRAM-iga videokaart.",
    requirements: "Ollama programm arvutis + käsk: ollama run llava",
    pros: ["Täielik privaatsus (0 võrguliiklust)", "Puuduvad igasugused kulud", "Töötab võrguühenduseta"],
  },
  {
    id: "moondream",
    name: "Moondream2 (Ollama)",
    provider: "local",
    visionModel: "moondream",
    embeddingModel: "all-minilm",
    costTier: "free",
    highlightBadge: "🟢 KERGEKAALULINE / CPU",
    costLabel: "Täiesti tasuta (0€, töötab ka tavalisel protsessoril)",
    speed: "Välkkiire (väike 1.8B mudel)",
    speedRating: 5,
    accuracy: "Põhiline (Objektid ja tekst)",
    accuracyRating: 3,
    description: "Ülimalt kompaktne 1.8B parameetriga mudel. Töötab sujuvalt ka ilma kalli videokaardita tavalises sülearvutis.",
    recommendedFor: "Kasutajatele ilma eraldiseisva videokaardita või nõrgema arvutiga.",
    requirements: "Ollama programm arvutis + käsk: ollama run moondream",
    pros: ["Töötab tavalisel protsessoril (CPU)", "Vajab vaid ~2 GB RAMi", "100% tasuta ja offline"],
  },
  {
    id: "llama3.2-vision",
    name: "Llama 3.2 Vision 11B (Ollama)",
    provider: "local",
    visionModel: "llama3.2-vision",
    embeddingModel: "nomic-embed-text",
    costTier: "free",
    highlightBadge: "🔥 TIPPTASE KOHALIKE SEAS",
    costLabel: "Täiesti tasuta (0€, avatud mudel)",
    speed: "Mõõdukas (nõuab 8GB+ GPU)",
    speedRating: 3,
    accuracy: "Tipptase avatud mudelite seas",
    accuracyRating: 5,
    description: "Meta võimsaim kohalik visuaalne mudel. Pakub pilvemudelitega võrreldavat detailituvastust otse sinu arvutis.",
    recommendedFor: "Võimsa videokaardiga (8GB-16GB VRAM) kasutajatele, kes soovivad parimat kohalikku kvaliteeti.",
    requirements: "Ollama programm + käsk: ollama run llama3.2-vision",
    pros: ["Parim avatud lähtekoodiga mudel", "100% tasuta ja privaatne", "Suurepärane detailitaju"],
  },
];

export const MODEL_CATALOG: Record<AiProvider, ModelPreset[]> = {
  openai: ALL_MODEL_PRESETS.filter((p) => p.provider === "openai"),
  gemini: ALL_MODEL_PRESETS.filter((p) => p.provider === "gemini"),
  local: ALL_MODEL_PRESETS.filter((p) => p.provider === "local"),
};
