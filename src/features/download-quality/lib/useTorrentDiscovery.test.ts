import { beforeEach, describe, expect, it, vi } from "vitest";
import type { TorrentResolveEvent, TorrentTierResolution } from "@/shared/lib/audio.ts";
import { subscribeTorrentDiscovery } from "@/features/download-quality/lib/useTorrentDiscovery.ts";

const bridge = vi.hoisted(() => ({
  resolve: vi.fn(),
  cancel: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("@/shared/lib/audio.ts", () => ({
  resolveTorrentAlbum: bridge.resolve,
  cancelTorrentInspection: bridge.cancel,
}));

const scope = { artist: "Aphex Twin", title: "Syro", expected: [{ index: 0, title: "minipops 67" }] };

describe("torrent discovery subscriptions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    bridge.resolve.mockImplementation(() => new Promise(() => {}));
  });

  it("shares live updates between the preview and quality menu and reuses the completed result", () => {
    const preview = vi.fn();
    const menu = vi.fn();
    const stopPreview = subscribeTorrentDiscovery("share-test", scope, preview);
    const stopMenu = subscribeTorrentDiscovery("share-test", scope, menu);
    expect(bridge.resolve).toHaveBeenCalledTimes(1);
    expect(menu).toHaveBeenLastCalledWith({ phase: "searching" });
    const emit = bridge.resolve.mock.calls[0][4] as (event: TorrentResolveEvent) => void;
    emit({ kind: "search_progress", checked: 1, total: 3 });
    const indexes = { stage: "indexes", checked: 1, total: 3 };
    emit({ kind: "estimated", estimates: [] });
    expect(preview).toHaveBeenLastCalledWith({ phase: "estimated", estimates: [], progress: indexes });
    emit({ kind: "inspection_progress", checked: 2, total: 4 });
    expect(menu).toHaveBeenLastCalledWith({
      phase: "estimated", estimates: [], progress: { stage: "releases", checked: 2, total: 4 },
    });
    stopPreview();
    expect(bridge.cancel).not.toHaveBeenCalled();
    emit({ kind: "resolved", resolutions: [], unavailable_providers: ["ApiBay"] });
    const done = { phase: "done", resolutions: [], unavailableProviders: ["ApiBay"] };
    expect(menu).toHaveBeenLastCalledWith(done);
    stopMenu();
    const reopens = vi.fn();
    subscribeTorrentDiscovery("share-test", scope, reopens)();
    expect(reopens).toHaveBeenLastCalledWith(done);
    expect(bridge.resolve).toHaveBeenCalledTimes(1);
  });

  it("cancels an abandoned album and ignores its late results", async () => {
    const first = vi.fn();
    subscribeTorrentDiscovery("abandoned-test", scope, first)();
    await Promise.resolve();
    const request = bridge.resolve.mock.calls[0];
    expect(bridge.cancel).toHaveBeenCalledWith(request[3]);
    const emit = request[4] as (event: TorrentResolveEvent) => void;
    const second = vi.fn();
    const stop = subscribeTorrentDiscovery("abandoned-test", scope, second);
    emit({ kind: "estimated", estimates: [] });
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenLastCalledWith({ phase: "searching" });
    stop();
    await Promise.resolve();
  });

  it("publishes verified options during inspection and retains them as more sources arrive", () => {
    const listener = vi.fn();
    const stop = subscribeTorrentDiscovery("verified-while-inspecting", scope, listener);
    const emit = bridge.resolve.mock.calls[0][4] as (event: TorrentResolveEvent) => void;
    const flac: TorrentTierResolution = {
      tier: "lossless", info_hash: "flac-hash", magnet_uri: "magnet:?xt=urn:btih:flac-hash",
      release_title: "Aphex Twin - Syro", provider: "RuTor", seeders: 3,
      codec: "FLAC", bitrate_kbps: null, display_label: "FLAC",
      matched_count: 1, total_expected: 1, total_matched_bytes: 20_000_000,
      missing_titles: [], ambiguous_titles: [], layout_warning: null, tracks: [],
    };
    const mp3: TorrentTierResolution = { ...flac, tier: "high_lossy", info_hash: "mp3-hash", codec: "MP3" };
    emit({ kind: "inspection_progress", checked: 1, total: 4 });
    emit({ kind: "verified", resolutions: [flac] });
    expect(listener).toHaveBeenLastCalledWith({
      phase: "searching", resolutions: [flac], progress: { stage: "releases", checked: 1, total: 4 },
    });
    emit({ kind: "estimated", estimates: [] });
    emit({ kind: "verified", resolutions: [mp3] });
    expect(listener).toHaveBeenLastCalledWith({
      phase: "estimated", estimates: [], resolutions: [mp3, flac],
      progress: { stage: "releases", checked: 1, total: 4 },
    });
    emit({ kind: "resolved", resolutions: [flac, mp3], unavailable_providers: [] });
    expect(listener).toHaveBeenLastCalledWith({
      phase: "done", resolutions: [flac, mp3], unavailableProviders: [],
    });
    stop();
  });

  it("keeps the request during a same-turn effect remount", async () => {
    subscribeTorrentDiscovery("remount-test", scope, vi.fn())();
    const cleanup = subscribeTorrentDiscovery("remount-test", scope, vi.fn());
    await Promise.resolve();
    expect(bridge.resolve).toHaveBeenCalledTimes(1);
    expect(bridge.cancel).not.toHaveBeenCalled();
    cleanup();
    await Promise.resolve();
  });
});
