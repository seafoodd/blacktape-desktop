import { invoke } from "@tauri-apps/api/core";
import { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import { Platform } from "@/shared/types.ts";

import type { SearchSource } from "@/shared/lib/search/searchProgress.ts";

export const searchPlatforms = (
  query: string,
  platforms: Platform[],
  requestId: number,
  sources?: SearchSource[],
): Promise<SearchSuggestion[]> => {
  return invoke<SearchSuggestion[]>("get_search_suggestions", {
    query,
    platforms,
    requestId,
    ...(sources ? { sources } : {}),
  });
};
