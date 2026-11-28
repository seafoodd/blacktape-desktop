import { useCallback, useSyncExternalStore } from "react";
import { searchTorrentReleases, type TorrentReleaseHit } from "@/shared/lib/audio.ts";

const cacheLifetime = 2 * 60 * 1000;
const maxCachedQueries = 48;

interface Snapshot {
  hits: TorrentReleaseHit[];
  pending: boolean;
  failures: string[];
}

interface Entry {
  snapshot: Snapshot;
  listeners: Set<() => void>;
  expires: number;
}

const empty: Snapshot = { hits: [], pending: true, failures: [] };
const entries = new Map<string, Entry>();
const queryPart = (value: string) => value.normalize("NFKC").toLowerCase().trim().replace(/\s+/g, " ");

export const torrentQueryKey = (artist: string, title: string) =>
  JSON.stringify([queryPart(artist), queryPart(title)]);

function compareHits(a: TorrentReleaseHit, b: TorrentReleaseHit): number {
  return b.rank - a.rank || (b.seeders ?? -1) - (a.seeders ?? -1) ||
    a.title.localeCompare(b.title) || a.provider.localeCompare(b.provider) || a.id.localeCompare(b.id);
}

export function mergeTorrentHits(
  current: readonly TorrentReleaseHit[],
  incoming: readonly TorrentReleaseHit[],
): TorrentReleaseHit[] {
  const byId = new Map<string, TorrentReleaseHit>();
  for (const entry of [...current, ...incoming].sort(compareHits)) {
    if (!byId.has(entry.id)) byId.set(entry.id, entry);
  }
  return [...byId.values()];
}

function refresh(entry: Entry, artist: string, title: string) {
  entry.expires = Infinity;
  const publish = (snapshot: Snapshot) => {
    entry.snapshot = snapshot;
    entry.listeners.forEach((listener) => listener());
  };
  publish(empty);
  void searchTorrentReleases(artist, title, (part) => {
    if (!entry.snapshot.pending) return;
    const { hits, failures } = entry.snapshot;
    publish({
      pending: true,
      hits: mergeTorrentHits(hits, part.hits),
      failures: part.error ? [...new Set([...failures, part.provider])] : failures,
    });
  }).then((report) => {
    entry.expires = Date.now() + cacheLifetime;
    publish({
      pending: false,
      hits: report.hits,
      failures: report.providers.filter((part) => part.error).map((part) => part.provider),
    });
  }, () => {
    entry.expires = Date.now() + 10_000;
    publish({ ...entry.snapshot, pending: false, failures: ["Torrent search"] });
  });
}

function trimCache() {
  if (entries.size <= maxCachedQueries) return;
  for (const [key, entry] of entries) {
    if (entry.listeners.size === 0 && !entry.snapshot.pending) entries.delete(key);
    if (entries.size <= maxCachedQueries) break;
  }
}

export function useTorrentSearch(artist: string, title: string) {
  const key = torrentQueryKey(artist, title);
  const subscribe = useCallback((listener: () => void) => {
    let entry = entries.get(key);
    if (!entry) {
      entry = { snapshot: empty, listeners: new Set(), expires: 0 };
      entries.set(key, entry);
    }
    entry.listeners.add(listener);
    if (entry.expires <= Date.now()) refresh(entry, artist, title);
    trimCache();
    return () => {
      entry.listeners.delete(listener);
      trimCache();
    };
  }, [key, artist, title]);
  const snapshot = useCallback(() => entries.get(key)?.snapshot ?? empty, [key]);
  return useSyncExternalStore(subscribe, snapshot);
}
