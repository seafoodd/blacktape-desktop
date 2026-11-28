import { describe, expect, it, vi } from "vitest";
import { mergeTorrentHits, torrentQueryKey, useTorrentSearch } from "@/features/torrent-inspection/model/torrentSearch.ts";
import type { TorrentReleaseHit } from "@/shared/lib/audio.ts";

const mocks = vi.hoisted(() => ({ mounted: vi.fn(), search: vi.fn() }));
vi.mock("react", () => ({
  useCallback: (callback: unknown) => callback,
  useSyncExternalStore: (subscribe: unknown, snapshot: unknown) => mocks.mounted(subscribe, snapshot),
}));
vi.mock("@/shared/lib/audio.ts", () => ({ searchTorrentReleases: mocks.search }));

const hit = (id: string, rank: number, seeders: number): TorrentReleaseHit => ({
  id, rank, seeders, title: id, input: id, provider: "Archive.org", size_bytes: null,
});

describe("progressive torrent results", () => {
  it("deduplicates out-of-order results and puts identity before seeder counts", () => {
    const weak = hit("same", 1, 80);
    const stronger = hit("same", 3, 0);
    const distant = hit("other", 2, 800);
    expect(mergeTorrentHits([weak], [distant, stronger])).toEqual([stronger, distant]);
    expect(mergeTorrentHits([distant, stronger], [weak])).toEqual([stronger, distant]);
  });

  it("shares a cache key across equivalent scopes and preserves artist/title boundaries", () => {
    expect(torrentQueryKey("  BJÖRK", "Debut   Live ")).toBe(torrentQueryKey("Björk", "debut live"));
    expect(torrentQueryKey("AB", "C")).not.toBe(torrentQueryKey("A", "BC"));
  });

  it("publishes an early release to both consumers before the remaining providers settle", async () => {
    mocks.search.mockImplementation(() => new Promise(() => {}));
    const viewA = vi.fn();
    const viewB = vi.fn();
    useTorrentSearch("Earlier Artist", "Release");
    useTorrentSearch("earlier artist", " Release ");
    const [subscribeA, snapshotA] = mocks.mounted.mock.calls[0];
    const [subscribeB, snapshotB] = mocks.mounted.mock.calls[1];
    const unsubscribeA = subscribeA(viewA);
    const unsubscribeB = subscribeB(viewB);
    expect(mocks.search).toHaveBeenCalledTimes(1);
    const publish = mocks.search.mock.calls[0][2];
    const result = hit("new", 3, 7);
    publish({ provider: "Knaben", hits: [result], error: null });
    expect(snapshotA()).toMatchObject({ hits: [result], pending: true });
    expect(snapshotB()).toBe(snapshotA());
    expect(viewA).toHaveBeenCalled();
    expect(viewB).toHaveBeenCalled();
    unsubscribeA();
    unsubscribeB();
    useTorrentSearch("Later Artist", "Next Release");
    const [subscribeNext, snapshotNext] = mocks.mounted.mock.lastCall!;
    const nextView = vi.fn();
    const unsubscribeNext = subscribeNext(nextView);
    publish({ provider: "ApiBay", hits: [hit("stale", 3, 2)], error: null });
    expect(snapshotNext().hits).toEqual([]);
    unsubscribeNext();
  });
});
