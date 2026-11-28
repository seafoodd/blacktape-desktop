import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  cancelTorrentInspection,
  resolveTorrentAlbum,
  type ExpectedTorrentTrackInput,
  type TorrentTierEstimate,
  type TorrentTierResolution,
} from "@/shared/lib/audio.ts";

export interface TorrentScope {
  artist: string;
  title: string;
  expected: ExpectedTorrentTrackInput[];
}

type DiscoveryProgress = { stage: "indexes" | "releases"; checked: number; total: number };

export type TorrentDiscovery =
  | { phase: "idle" | "searching"; progress?: DiscoveryProgress; resolutions?: TorrentTierResolution[] }
  | { phase: "estimated"; estimates: TorrentTierEstimate[]; progress?: DiscoveryProgress; resolutions?: TorrentTierResolution[] }
  | { phase: "done"; resolutions: TorrentTierResolution[]; unavailableProviders?: string[]; failed?: boolean };

type Listener = (value: TorrentDiscovery) => void;
type Entry = {
  id: string;
  value: TorrentDiscovery;
  listeners: Set<Listener>;
  finishedAt?: number;
};

const idle: TorrentDiscovery = { phase: "idle" };
const searching: TorrentDiscovery = { phase: "searching" };
const entries = new Map<string, Entry>();
const CACHE_MS = 5 * 60_000;
let indexerRevision = 0;
const indexerListeners = new Set<() => void>();

function subscribeIndexerChanges(listener: () => void) {
  indexerListeners.add(listener);
  return () => { indexerListeners.delete(listener); };
}

const getIndexerRevision = () => indexerRevision;

if (typeof window !== "undefined") {
  void listen("torrent-indexers-changed", () => {
    indexerRevision += 1;
    indexerListeners.forEach((listener) => listener());
  }).catch(() => {});
}

export function subscribeTorrentDiscovery(key: string, scope: TorrentScope, listener: Listener) {
  let entry = entries.get(key);
  if (entry?.finishedAt !== undefined && Date.now() - entry.finishedAt >= CACHE_MS) {
    entries.delete(key);
    entry = undefined;
  }
  if (!entry) {
    for (const [cachedKey, cached] of entries) {
      if (cached.finishedAt !== undefined && cached.listeners.size === 0 && entries.size >= 32) {
        entries.delete(cachedKey);
      }
    }
    const created: Entry = { id: crypto.randomUUID(), value: searching, listeners: new Set() };
    entries.set(key, created);
    entry = created;
    const publish = (value: TorrentDiscovery) => {
      if (entries.get(key) !== created) return;
      created.value = value;
      if (value.phase === "done") created.finishedAt = Date.now();
      created.listeners.forEach((notify) => notify(value));
    };
    void resolveTorrentAlbum(scope.artist, scope.title, scope.expected, created.id, (event) => {
      if (created.value.phase === "done") return;
      if (event.kind === "search_progress" || event.kind === "inspection_progress") {
        publish({
          ...created.value,
          progress: {
            stage: event.kind === "search_progress" ? "indexes" : "releases",
            checked: event.checked,
            total: event.total,
          },
        });
      }
      if (event.kind === "estimated") {
        publish({ ...created.value, phase: "estimated", estimates: event.estimates });
      }
      if (event.kind === "verified") {
        const keyOf = (release: TorrentTierResolution) => `${release.tier}:${release.info_hash}`;
        const resolutions = new Map(event.resolutions.map((release) => [keyOf(release), release]));
        for (const release of created.value.resolutions ?? []) {
          if (!resolutions.has(keyOf(release))) resolutions.set(keyOf(release), release);
        }
        publish({ ...created.value, resolutions: [...resolutions.values()] });
      }
      if (event.kind === "resolved") publish({ phase: "done", resolutions: event.resolutions, unavailableProviders: event.unavailable_providers });
    }).then(([resolutions, unavailableProviders]) => {
      if (created.value.phase !== "done") publish({ phase: "done", resolutions, unavailableProviders });
    }).catch(() => {
      if (created.value.phase !== "done") publish({ phase: "done", resolutions: created.value.resolutions ?? [], failed: true });
      if (entries.get(key) === created) entries.delete(key);
    });
  }
  const subscribed = entry;
  subscribed.listeners.add(listener);
  listener(subscribed.value);
  return () => {
    subscribed.listeners.delete(listener);
    // React may detach and reattach an effect in the same turn.
    queueMicrotask(() => {
      if (subscribed.listeners.size === 0 && subscribed.finishedAt === undefined && entries.get(key) === subscribed) {
        entries.delete(key);
        void cancelTorrentInspection(subscribed.id).catch(() => {});
      }
    });
  };
}

export function useTorrentDiscovery(url: string | undefined, scope: TorrentScope | null): TorrentDiscovery {
  const revision = useSyncExternalStore(subscribeIndexerChanges, getIndexerRevision, getIndexerRevision);
  const key = url && scope ? JSON.stringify([revision, url, scope.artist, scope.title, scope.expected.map((track) =>
    [track.index, track.title, track.artist, track.disc_number, track.track_number]
  )]) : "";
  const latestScope = useRef(scope);
  latestScope.current = scope;
  const [snapshot, setSnapshot] = useState<{ key: string; value: TorrentDiscovery }>();
  useEffect(() => {
    if (!key || !latestScope.current) return;
    return subscribeTorrentDiscovery(key, latestScope.current, (value) => setSnapshot({ key, value }));
  }, [key]);
  return !key ? idle : snapshot?.key === key ? snapshot.value : searching;
}
