import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { ItemType } from "@/shared/store/libraryStore.ts";
import type { AlbumPreview, AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";

type Handler = (event: { payload: unknown }) => void;

const {
  getAlbumPreviewMock,
  getReleaseGroupReleasesMock,
  listenMock,
  listeners,
  syncAlbumCandidatesMock,
  syncAlbumCandidatesDoneMock,
} = vi.hoisted(() => {
  const listeners = new Map<string, Set<Handler>>();
  const listenMock = vi.fn(async (event: string, handler: Handler) => {
    const handlers = listeners.get(event) ?? new Set<Handler>();
    handlers.add(handler);
    listeners.set(event, handlers);
    return () => handlers.delete(handler);
  });
  return {
    getAlbumPreviewMock: vi.fn(),
    getReleaseGroupReleasesMock: vi.fn().mockResolvedValue([]),
    listenMock,
    listeners,
    syncAlbumCandidatesMock: vi.fn(),
    syncAlbumCandidatesDoneMock: vi.fn(),
  };
});

vi.mock("@tauri-apps/api/event", () => ({ listen: listenMock }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/shared/lib/remote/albumPreview.ts", async () => ({
  ...(await vi.importActual<typeof import("@/shared/lib/remote/albumPreview.ts")>(
    "@/shared/lib/remote/albumPreview.ts",
  )),
  getAlbumPreview: getAlbumPreviewMock,
}));
vi.mock("@/shared/lib/audio.ts", () => ({ getReleaseGroupReleases: getReleaseGroupReleasesMock }));
vi.mock("@/shared/store/downloadStore.ts", () => ({
  useDownloadStore: { getState: () => ({ startDownload: vi.fn(), activeDownloads: {} }) },
}));
vi.mock("@/shared/store/libraryStore.ts", () => ({
  ItemType: { Track: "Track", Album: "Album", Artist: "Artist" },
  useLibraryStore: { getState: () => ({ setActiveView: vi.fn() }), subscribe: vi.fn(() => () => {}) },
}));
vi.mock("@/features/navigation/model/navigationStore.ts", () => ({
  useNavigationStore: {
    getState: () => ({
      syncAlbumCandidates: syncAlbumCandidatesMock,
      syncAlbumCandidatesDone: syncAlbumCandidatesDoneMock,
    }),
  },
}));

const ALBUM_URL = "https://fixture.example/incomplete-album";
const albumItem = {
  item_type: ItemType.Album,
  name: "Fixture album",
  band_name: "Fixture artist",
  item_url_path: ALBUM_URL,
  img: "",
  platform: "bandcamp" as const,
};

const track = (candidates: AudioCandidate[] = []): AlbumTrackInfo => ({
  index: 0,
  title: "Fixture track",
  artists: ["Fixture artist"],
  track_number: 1,
  url: "https://fixture.example/track",
  available: true,
  note: null,
  candidates,
  selected_candidate_idx: 0,
});

const preview = (candidates: AudioCandidate[] = []): AlbumPreview => ({
  platform: "bandcamp",
  url: ALBUM_URL,
  title: "Fixture album",
  artist: "Fixture artist",
  cover_url: null,
  year: 2020,
  genres: [],
  tracks: [track(candidates)],
  candidates_resolved: false,
});

const stream: AudioCandidate = {
  source: { kind: "http_stream", platform: "soundcloud", url: "https://soundcloud.com/fixture/track" },
  quality_tier: "standard_lossy",
  codec: "MP3",
  bitrate_kbps: 128,
  sample_rate_hz: 44_100,
  duration_ms: 180_000,
  match_confidence: 1,
  is_strict_match: true,
  display_label: "SoundCloud MP3",
};

let useAlbumPreviewStore: typeof import("./albumPreviewStore.ts").useAlbumPreviewStore;
let initialState: ReturnType<typeof useAlbumPreviewStore.getState>;

beforeAll(async () => {
  vi.stubGlobal("window", { __TAURI_INTERNALS__: {} });
  ({ useAlbumPreviewStore } = await import("./albumPreviewStore.ts"));
  initialState = useAlbumPreviewStore.getState();
});

beforeEach(() => {
  getAlbumPreviewMock.mockReset();
  getReleaseGroupReleasesMock.mockReset().mockResolvedValue([]);
  syncAlbumCandidatesMock.mockReset();
  syncAlbumCandidatesDoneMock.mockReset();
  useAlbumPreviewStore.setState(initialState, true);
});

function emit(event: string, payload: unknown) {
  for (const handler of [...(listeners.get(event) ?? [])]) handler({ payload });
}

describe("incomplete album candidate completion", () => {
  it("keeps visible partial candidates but clears their cache after incomplete completion", async () => {
    getAlbumPreviewMock.mockResolvedValueOnce(preview());
    await useAlbumPreviewStore.getState().open(albumItem);
    expect(useAlbumPreviewStore.getState().preview?.tracks[0].candidates).toEqual([]);

    emit("album-track-candidates", {
      album_url: ALBUM_URL,
      track_index: 0,
      candidates: [stream],
      selected_candidate_idx: 0,
      duration_ms: 180_000,
    });
    expect(useAlbumPreviewStore.getState().preview?.tracks[0].candidates).toEqual([stream]);

    emit("album-candidates-done", { album_url: ALBUM_URL, complete: false });
    expect(useAlbumPreviewStore.getState().preview?.tracks[0].candidates).toEqual([stream]);
    expect(useAlbumPreviewStore.getState().preview?.candidates_resolved).toBe(false);
    expect(useAlbumPreviewStore.getState().resolvingCandidates).toBe(false);
    const calls = syncAlbumCandidatesMock.mock.calls;
    const updateSnapshot = calls[calls.length - 1]?.[1];
    expect(updateSnapshot).toBeDefined();
    const synchronized = updateSnapshot?.(preview([stream]));
    expect(synchronized?.preview.candidates_resolved).toBe(false);
    expect(synchronized?.resolvingCandidates).toBe(false);

    getAlbumPreviewMock.mockResolvedValueOnce(preview());
    await useAlbumPreviewStore.getState().resumeAfterRestore();
    expect(getAlbumPreviewMock).toHaveBeenCalledTimes(2);
    expect(getAlbumPreviewMock).toHaveBeenLastCalledWith("bandcamp", ALBUM_URL, false);
    expect(useAlbumPreviewStore.getState().preview?.tracks[0].candidates).toEqual([stream]);

    getAlbumPreviewMock.mockResolvedValueOnce(preview());
    await useAlbumPreviewStore.getState().open(albumItem);
    const reopenedTrack = useAlbumPreviewStore.getState().preview?.tracks[0];
    expect(reopenedTrack?.candidates).toEqual([]);
    expect(useAlbumPreviewStore.getState().preview?.candidates_resolved).toBe(false);
  });
});
