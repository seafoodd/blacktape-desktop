/**
 * Own zustand store of the full search page (NOT libraryStore). OWNER: part C implements the TODO bodies;
 * the state shape and action names are FROZEN (SearchBar and SearchResults depend on them).
 *
 * Navigation contract (D14): SearchBar calls `useSearchPageStore.getState().open(query)` on Enter and on
 * "Browse all results". `open` sets `query`, resets sections/profile, switches the app to the page with
 * `useLibraryStore.getState().setActiveView("SEARCH_RESULTS")` and starts the search. App.tsx already renders
 * `<SearchResults />` for that view; the page reads everything from this store.
 *
 * Search flow (D13): on `open`/`retry` fire `searchFull` for Track, Album and Artist IN PARALLEL with
 * platforms ["youtube","bandcamp"]; each section has its own status/error. Stale responses are dropped with
 * `createLatestGuard` (shared/lib/searchGuard): one guard for the first-page group, one per section for
 * "Show more", one for the profile (D19). `retry` re-runs the whole search.
 * As soon as the Artist section is ready, call `pickProfileArtist(query, artists)` and, if non-null, `getArtistProfile`;
 * a failed profile only means no card (`profile.status` "error").
 * `loadMore(kind)` calls `searchFull` with the section's `nextCursor` and appends via `mergePages`.
 *
 * Error semantics: a first-page failure sets `status: "error"` (items empty, Retry re-runs the search).
 * A failed "Show more" keeps the items, returns `status` to "ready", sets `error` (inline message) and
 * leaves `hasMore` true, so "Show more" simply retries.
 */
import { create } from "zustand";
import {
  ItemType,
  useLibraryStore,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import {
  getArtistProfile,
  searchFull,
  type ArtistProfile,
  type FullSearchPage,
  type SearchCursor,
  type SearchKind,
} from "@/shared/lib/searchFull.ts";
import {
  createLatestGuard,
  type LatestGuard,
} from "@/shared/lib/searchGuard.ts";
import type { Platform } from "@/shared/types.ts";
import { useStreamStore } from "@/features/preview/model/streamStore.ts";
import {
  DEFAULT_FILTERS,
  DEFAULT_SORT,
  mergePages,
  type ItemCopy,
  pickProfileArtist,
  type SearchPageFilters,
  type SearchPageSort,
} from "./searchPageFilters.ts";

export type SectionStatus =
  | "idle"
  | "loading"
  | "ready"
  | "loadingMore"
  | "error";

export interface SectionState {
  items: SearchSuggestion[];
  /** Copies of `items` on other platforms (see `itemsForPlatform`). */
  copies: ItemCopy[];
  status: SectionStatus;
  error: string | null;
  nextCursor: SearchCursor | null;
  hasMore: boolean;
  /** Platform warnings of the last response, e.g. "YouTube unavailable". */
  warnings: string[];
}

export type ProfileStatus = "idle" | "loading" | "ready" | "error";

export interface ProfileState {
  /** The artist search item the profile belongs to (avatar fallback = its `img`). */
  artist: SearchSuggestion | null;
  status: ProfileStatus;
  data: ArtistProfile | null;
}

export interface SearchPageState {
  query: string;
  sections: Record<SearchKind, SectionState>;
  profile: ProfileState;
  filters: SearchPageFilters;
  sort: SearchPageSort;

  /** Navigate to the page and search `query` (trimmed; empty query is ignored). */
  open: (query: string) => void;
  /** Re-run the whole search for the current query (e.g. after an error). */
  retry: () => void;
  /** "Show more" for one section; no-op unless status is "ready" and hasMore. */
  loadMore: (kind: SearchKind) => Promise<void>;
  setFilters: (patch: Partial<SearchPageFilters>) => void;
  resetFilters: () => void;
  setSort: (sort: SearchPageSort) => void;
}

export const emptySection = (): SectionState => ({
  items: [],
  copies: [],
  status: "idle",
  error: null,
  nextCursor: null,
  hasMore: false,
  warnings: [],
});

const SEARCH_PLATFORMS: Platform[] = ["youtube", "bandcamp", "soundcloud"];
const KINDS: SearchKind[] = [ItemType.Track, ItemType.Album, ItemType.Artist];

// Module-level guards: a newer search (or profile) invalidates every older response still in flight.
const searchGuard = createLatestGuard();
const profileGuard = createLatestGuard();
const loadMoreGuards: Record<SearchKind, LatestGuard> = {
  [ItemType.Track]: createLatestGuard(),
  [ItemType.Album]: createLatestGuard(),
  [ItemType.Artist]: createLatestGuard(),
};

/** YouTube stream urls take seconds to resolve: warm the first few so their preview starts right away. */
const WARM_YOUTUBE_TRACKS = 4;

function warmPreviewStreams(tracks: SearchSuggestion[]): void {
  const { prefetch } = useStreamStore.getState();
  tracks
    .filter((t) => t.platform === "youtube")
    .slice(0, WARM_YOUTUBE_TRACKS)
    .forEach(prefetch);
}

const loadingSection = (): SectionState => ({
  ...emptySection(),
  status: "loading",
});

const sectionsWith = (make: () => SectionState): Record<SearchKind, SectionState> => ({
  [ItemType.Track]: make(),
  [ItemType.Album]: make(),
  [ItemType.Artist]: make(),
});

const idleProfile = (): ProfileState => ({
  artist: null,
  status: "idle",
  data: null,
});

function errorMessage(err: unknown): string {
  if (typeof err === "string" && err.trim() !== "") return err;
  if (err instanceof Error && err.message !== "") return err.message;
  if (
    typeof err === "object" &&
    err !== null &&
    typeof (err as { message?: unknown }).message === "string" &&
    (err as { message: string }).message !== ""
  ) {
    return (err as { message: string }).message;
  }
  return "Search failed";
}

const readySection = (page: FullSearchPage): SectionState => {
  const nextCursor = page.next_cursor ?? null;
  return {
    items: page.items ?? [],
    copies: page.copies ?? [],
    status: "ready",
    error: null,
    nextCursor,
    hasMore: Boolean(page.has_more) && nextCursor !== null,
    warnings: (page.warnings ?? []).map((w) => w.message),
  };
};

export const useSearchPageStore = create<SearchPageState>((set, get) => {
  const setSection = (kind: SearchKind, patch: Partial<SectionState>): void =>
    set((s) => ({
      sections: { ...s.sections, [kind]: { ...s.sections[kind], ...patch } },
    }));

  /** Profile of the best artist match, fetched after the Artist section is ready. */
  const startProfile = (
    query: string,
    artists: SearchSuggestion[],
  ): void => {
    const artist = pickProfileArtist(query, artists);
    if (artist === null) return;
    const token = profileGuard.begin();
    set({ profile: { artist, status: "loading", data: null } });
    getArtistProfile(artist.platform, artist.item_url_path).then(
      (data) => {
        if (!profileGuard.isLatest(token)) return;
        set({ profile: { artist, status: "ready", data } });
        warmPreviewStreams(data.top_songs ?? []);
      },
      () => {
        if (!profileGuard.isLatest(token)) return;
        set({ profile: { artist, status: "error", data: null } });
      },
    );
  };

  const loadFirstPage = async (
    kind: SearchKind,
    query: string,
    token: number,
  ): Promise<void> => {
    try {
      const page = await searchFull({
        query,
        kind,
        platforms: SEARCH_PLATFORMS,
      });
      if (!searchGuard.isLatest(token)) return;
      setSection(kind, readySection(page));
      if (kind === ItemType.Artist) startProfile(query, page.items ?? []);
      if (kind === ItemType.Track) warmPreviewStreams(page.items ?? []);
    } catch (err) {
      if (!searchGuard.isLatest(token)) return;
      setSection(kind, {
        ...emptySection(),
        status: "error",
        error: errorMessage(err),
      });
    }
  };

  const runSearch = (query: string): void => {
    const token = searchGuard.begin();
    profileGuard.begin(); // invalidates a profile still loading for the previous search
    for (const kind of KINDS) loadMoreGuards[kind].begin();
    set({
      query,
      sections: sectionsWith(loadingSection),
      profile: idleProfile(),
    });
    for (const kind of KINDS) void loadFirstPage(kind, query, token);
  };

  return {
    query: "",
    sections: sectionsWith(emptySection),
    profile: idleProfile(),
    filters: DEFAULT_FILTERS,
    sort: DEFAULT_SORT,

    open: (query) => {
      const q = query.trim();
      if (q === "") return;
      useLibraryStore.getState().setActiveView("SEARCH_RESULTS");
      // A different query starts clean: old year/duration/text/platform filters would silently hide results.
      if (q !== get().query) {
        set((s) => ({ filters: { ...DEFAULT_FILTERS, tab: s.filters.tab } }));
      }
      runSearch(q);
    },

    retry: () => {
      const { query } = get();
      if (query.trim() === "") return;
      runSearch(query);
    },

    loadMore: async (kind) => {
      const state = get();
      const section = state.sections[kind];
      const cursor = section.nextCursor;
      if (section.status !== "ready" || !section.hasMore || cursor === null) {
        return;
      }
      const query = state.query;
      const guard = loadMoreGuards[kind];
      const token = guard.begin();
      setSection(kind, { status: "loadingMore", error: null });
      try {
        const page = await searchFull({
          query,
          kind,
          platforms: SEARCH_PLATFORMS,
          cursor,
        });
        if (!guard.isLatest(token)) return;
        const current = get().sections[kind];
        const next = readySection(page);
        setSection(kind, {
          ...next,
          items: mergePages(current.items, next.items),
          copies: [...current.copies, ...next.copies],
          // keep the first page's platform warnings (e.g. "Bandcamp unavailable")
          warnings: [...new Set([...current.warnings, ...next.warnings])],
        });
      } catch (err) {
        if (!guard.isLatest(token)) return;
        setSection(kind, { status: "ready", error: errorMessage(err) });
      }
    },

    setFilters: (patch) => set((s) => ({ filters: { ...s.filters, ...patch } })),
    resetFilters: () => set({ filters: DEFAULT_FILTERS }),
    setSort: (sort) => set({ sort }),
  };
});
