import { invoke } from "@tauri-apps/api/core";
import type {
  AiAnalysisPlan,
  AiConfig,
  AiConnectionReport,
  AiIndexReport,
  AiSearchFocus,
  AiSearchResult,
  GeminiOAuthStatus,
  IndexReport,
  SearchFilters,
  SearchResult,
} from "../types";

export async function indexMediaFolder(path: string): Promise<IndexReport> {
  return invoke<IndexReport>("index_media_folder", { path });
}

export async function searchMedia(query: SearchFilters): Promise<SearchResult[]> {
  return invoke<SearchResult[]>("search_media", { query });
}

export async function planAiAnalysis(
  path: string,
  config: AiConfig,
  force = false,
): Promise<AiAnalysisPlan> {
  return invoke<AiAnalysisPlan>("plan_ai_analysis", { path, config, force });
}

export async function analyzeMediaFolder(
  path: string,
  config: AiConfig,
  force = false,
): Promise<AiIndexReport> {
  return invoke<AiIndexReport>("analyze_media_folder", { path, config, force });
}

export async function cancelAiAnalysis(): Promise<boolean> {
  return invoke<boolean>("cancel_ai_analysis");
}

export async function searchAi(
  query: string,
  config: AiConfig,
  focus: AiSearchFocus,
  root?: string,
): Promise<AiSearchResult[]> {
  return invoke<AiSearchResult[]>("search_ai", { query, config, focus, root: root || null });
}

export async function getAiThumbnail(
  path: string,
  timestampMs: number,
  ffmpegPath?: string,
): Promise<string> {
  return invoke<string>("get_ai_thumbnail", {
    path,
    timestampMs,
    ffmpegPath: ffmpegPath || null,
  });
}

export async function testAiConnection(config: AiConfig): Promise<AiConnectionReport> {
  return invoke<AiConnectionReport>("test_ai_connection", { config });
}

export async function loginGeminiOAuth(clientFilePath: string): Promise<GeminiOAuthStatus> {
  return invoke<GeminiOAuthStatus>("login_gemini_oauth", { clientFilePath });
}

export async function getGeminiOAuthStatus(): Promise<GeminiOAuthStatus> {
  return invoke<GeminiOAuthStatus>("get_gemini_oauth_status");
}

export async function logoutGeminiOAuth(): Promise<GeminiOAuthStatus> {
  return invoke<GeminiOAuthStatus>("logout_gemini_oauth");
}

export async function openIndexedMediaPath(path: string): Promise<void> {
  return invoke("open_indexed_media_path", { path });
}

export async function prepareIndexedMediaPreview(path: string): Promise<string> {
  return invoke<string>("prepare_indexed_media_preview", { path });
}

export async function getIndexedLibraryPath(): Promise<string | null> {
  return invoke<string | null>("get_indexed_library_path");
}
