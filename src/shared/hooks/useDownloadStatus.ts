import { useEffect, useMemo } from "react";
import {
  type AlbumStatus,
  type DownloadState,
  type RemoteRef,
  refKey,
} from "@/shared/lib/libraryIndex.ts";
import type { Platform } from "@/shared/types.ts";
import { useLibraryIndexStore } from "@/shared/store/libraryIndexStore.ts";

/**
 * Hooks over `useLibraryIndexStore`: they request the refs (batched by the store) and return the cached answer.
 * Until the answer arrives the state is "none". These are the only way UI asks for library status.
 */

const NO_REFS: readonly RemoteRef[] = [];
const NO_ALBUM: AlbumStatus = { state: "none", owned: 0, total: 0 };

/** The ref of a search item or album row: its page url on its platform. */
export const remoteRefOf = (item: {
  platform: Platform;
  item_url_path: string;
  name?: string;
  band_name?: string;
}): RemoteRef => ({
  platform: item.platform,
  url: item.item_url_path,
  artist: item.band_name,
  title: item.name,
});

/** Identity of a ref list by its urls: the same urls in the same order give the same string. */
export const refsKey = (refs: readonly RemoteRef[]): string => refs.map(refKey).join("\n");

const hasOwn = (record: object, key: string): boolean =>
  Object.prototype.hasOwnProperty.call(record, key);

/** The cached answer of one url, or undefined when it has none yet. */
export function readStatus<V>(cache: Record<string, V>, url: string): V | undefined {
  return hasOwn(cache, url) ? cache[url] : undefined;
}

/** The answers that exist for the refs, keyed by url; urls without an answer are absent. */
export function pickStatuses<V>(
  cache: Record<string, V>,
  refs: readonly RemoteRef[],
): Record<string, V> {
  const out: Record<string, V> = {};
  for (const ref of refs) {
    const value = readStatus(cache, ref.url);
    if (value !== undefined) out[ref.url] = value;
  }
  return out;
}

/** Keeps the array identity while the urls stay the same, so effects do not re-run on every render. */
function useStableRefs(refs: readonly RemoteRef[]): readonly RemoteRef[] {
  const key = refsKey(refs);
  // The key is the identity: a new array with the same urls must not trigger a new request.
  return useMemo(() => refs, [key]);
}

/** Library state of many tracks: track url -> state (urls without an answer are absent). */
export function useTrackStatuses(refs: readonly RemoteRef[]): Record<string, DownloadState> {
  const stable = useStableRefs(refs);
  const requestTracks = useLibraryIndexStore((s) => s.requestTracks);
  const tracks = useLibraryIndexStore((s) => s.tracks);
  useEffect(() => {
    requestTracks(stable);
  }, [stable, requestTracks]);
  return useMemo(() => pickStatuses(tracks, stable), [tracks, stable]);
}

/**
 * Library state of one remote track; "none" while unknown or without a ref. Selects its own url only, so an
 * answer for another row does not re-render this one.
 */
export function useTrackStatus(ref: RemoteRef | null | undefined): DownloadState {
  const url = ref ? ref.url : null;
  const stable = useStableRefs(ref ? [ref] : NO_REFS);
  const requestTracks = useLibraryIndexStore((s) => s.requestTracks);
  useEffect(() => {
    requestTracks(stable);
  }, [stable, requestTracks]);
  const state = useLibraryIndexStore((s) => (url === null ? undefined : readStatus(s.tracks, url)));
  return state ?? "none";
}

/** Library status of a remote album (from `album_sources`); `{state:"none", owned:0, total:0}` when unknown. */
export function useAlbumStatus(ref: RemoteRef | null | undefined): AlbumStatus {
  const url = ref ? ref.url : null;
  const stable = useStableRefs(ref ? [ref] : NO_REFS);
  const requestAlbums = useLibraryIndexStore((s) => s.requestAlbums);
  useEffect(() => {
    requestAlbums(stable);
  }, [stable, requestAlbums]);
  const status = useLibraryIndexStore((s) => (url === null ? undefined : readStatus(s.albums, url)));
  return status ?? NO_ALBUM;
}
