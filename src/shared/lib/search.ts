import { invoke } from "@tauri-apps/api/core";
import { SearchSuggestion } from "@/shared/store/libraryStore.ts";

export type Platform = "Youtube" | "Bandcamp";

export const searchPlatforms = (
  query: string,
  platforms: Platform[],
): Promise<SearchSuggestion[]> => {
  return invoke<SearchSuggestion[]>("get_search_suggestions", {
    query,
    platforms,
  });
};
