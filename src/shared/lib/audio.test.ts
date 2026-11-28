import { describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  searchCanonicalReleases,
  getCanonicalRelease,
  resolveCanonicalCandidates,
  prewarmAlbumPreviews,
  listTorrentIndexers,
  openRuTrackerLogin,
  setTorrentIndexerEnabled,
  testTorrentIndexer,
} from "./audio";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string, args: Record<string, unknown>) => {
    switch (cmd) {
      case "search_canonical_releases":
        return Promise.resolve([{ id: "mbid-1", title: "Test", artist: "Artist", track_count: 10 }]);
      case "get_canonical_release":
        return Promise.resolve({ id: args.id, title: "Test Release" });
      case "resolve_canonical_candidates":
        return Promise.resolve(args.track);
      case "prewarm_album_previews":
        return Promise.resolve();
      default:
        return Promise.resolve();
    }
  }),
}));

describe("canonical audio IPC wrappers", () => {
  it("calls search_canonical_releases", async () => {
    const res = await searchCanonicalReleases("test");
    expect(res).toHaveLength(1);
    expect(res[0].id).toBe("mbid-1");
  });

  it("calls get_canonical_release", async () => {
    const res = await getCanonicalRelease("mbid-123");
    expect(res.id).toBe("mbid-123");
  });

  it("calls resolve_canonical_candidates", async () => {
    const track = {
      track_number: 1,
      disc_number: 1,
      title: "Track 1",
      artists: ["Artist"],
      duration_ms: 180000,
      candidates: [],
      selected_candidate_idx: 0,
    };
    const res = await resolveCanonicalCandidates(track);
    expect(res.title).toBe("Track 1");
  });

  it("calls prewarm_album_previews", async () => {
    await expect(prewarmAlbumPreviews([["youtube", "https://test.com"]])).resolves.toBeUndefined();
  });
});

describe("torrent indexer settings IPC", () => {
  it("uses the torrent plugin and sends only the selected indexer and enabled state", async () => {
    await listTorrentIndexers();
    expect(invoke).toHaveBeenLastCalledWith("plugin:torrent|list_torrent_indexers");
    await setTorrentIndexerEnabled("api_bay", false);
    expect(invoke).toHaveBeenLastCalledWith("plugin:torrent|set_torrent_indexer_enabled", { id: "api_bay", enabled: false });
    await testTorrentIndexer("archive");
    expect(invoke).toHaveBeenLastCalledWith("plugin:torrent|test_torrent_indexer", { id: "archive" });
  });

  it("opens the RuTracker sign-in window without sending credentials to the bridge", async () => {
    await openRuTrackerLogin();
    expect(invoke).toHaveBeenLastCalledWith("plugin:torrent|open_rutracker_login");
  });
});
