/**
 * Invoke wrappers + types for the full search page (search v2). The types MIRROR the Rust types in
 * `src-tauri/src/search/full.rs` (serde, snake_case) and are FROZEN: change both sides together.
 * Owner: architect (contract). Part C only imports from here.
 */
import { invoke } from "@tauri-apps/api/core";
import type { ItemType, SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { Platform } from "@/shared/types.ts";

/** Kind of a `search_full` request. Same strings as the Rust `SearchKind` / `ItemType` enum values. */
export type SearchKind = ItemType.Track | ItemType.Album | ItemType.Artist;

/** Opaque paging state; send back unchanged. Bandcamp has no paging; YouTube Music and SoundCloud do. */
export interface SearchCursor {
  youtube?: string | null;
  /** SoundCloud offset of the next page. */
  soundcloud?: number | null;
}

export interface PlatformWarning {
  platform: Platform;
  message: string;
}

/** The same album / song / artist as a listed item, on another platform. */
export interface PlatformCopy {
  /** `item_url_path` of the item of `items` this is a copy of. */
  of: string;
  item: SearchSuggestion;
}

/** One page of ranked, deduplicated results of ONE kind. `has_more === (next_cursor !== null)`. */
export interface FullSearchPage {
  items: SearchSuggestion[];
  /** Copies of `items` on other platforms; shown instead of the item when the page is filtered by platform. */
  copies?: PlatformCopy[];
  next_cursor: SearchCursor | null;
  has_more: boolean;
  warnings: PlatformWarning[];
}

/** Artist profile card data. Missing data is null / empty array. */
export interface ArtistProfile {
  platform: Platform | null;
  name: string;
  url: string;
  /** Square avatar; null for YouTube (fall back to the `img` of the artist search item). */
  avatar: string | null;
  /** Wide header image (YouTube only). */
  banner: string | null;
  subscribers: number | null;
  /** SoundCloud follower count. */
  followers: number | null;
  monthly_audience: number | null;
  description: string | null;
  location: string | null;
  /** YouTube "Top songs" (<= 5 tracks). Bandcamp: first tracks; SoundCloud: most played tracks. */
  top_songs: SearchSuggestion[];
  /** Albums (item_type Album). */
  albums: SearchSuggestion[];
  /** YouTube singles & EPs (Album items) / Bandcamp standalone tracks (Track items). */
  singles: SearchSuggestion[];
}

export interface SearchFullArgs {
  query: string;
  kind: SearchKind;
  platforms: Platform[];
  /** `null`/omitted = first page; otherwise `next_cursor` of the previous page of the same kind. */
  cursor?: SearchCursor | null;
}

/** First page (no cursor) or "Show more" page (cursor) of one kind. Rejects with a message string. */
export const searchFull = ({
  query,
  kind,
  platforms,
  cursor = null,
}: SearchFullArgs): Promise<FullSearchPage> =>
  invoke<FullSearchPage>("search_full", { query, kind, platforms, cursor });

/** Profile of an artist item (`item_url_path` of an Artist `SearchSuggestion`). Rejects when unavailable. */
export const getArtistProfile = (
  platform: Platform,
  url: string,
): Promise<ArtistProfile> =>
  invoke<ArtistProfile>("get_artist_profile", { platform, url });
