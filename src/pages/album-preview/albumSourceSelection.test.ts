import { describe, expect, it } from "vitest";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import type { AudioCandidate } from "@/shared/types/canonical.ts";
import { albumSourceOptions, applyAlbumSource } from "@/pages/album-preview/albumSourceSelection.ts";

function source(
  platform: string, codec: string, tier: AudioCandidate["quality_tier"], id: string,
): AudioCandidate {
  return {
    source: platform === "torrent"
      ? { kind: "torrent", info_hash: id, magnet_uri: `magnet:${id}`, file_index: 0, file_path: "song.flac", file_size: 3, seeders: 1 }
      : { kind: "http_stream", platform, url: `https://example.com/${id}` },
    codec,
    quality_tier: tier,
    display_label: codec,
    duration_ms: 0,
    match_confidence: 1,
    is_strict_match: true,
  };
}

function track(index: number, ...candidates: AudioCandidate[]): AlbumTrackInfo {
  return {
    index, title: `Track ${index}`, artists: [], url: `https://example.com/original-${index}`,
    track_number: index + 1, note: null, available: true, selected_candidate_idx: 0, candidates,
  };
}

describe("album quality selection", () => {
  it("lists every platform, codec and quality, counting a track only once per option", () => {
    const tracks = [
      track(0, source("torrent", "FLAC", "lossless", "a"), source("torrent", "flac", "lossless", "b"),
        source("torrent", "MP3", "high_lossy", "c"), source("youtube", "Opus", "high_lossy", "d")),
      track(1, source("torrent", "flac", "lossless", "e"), source("youtube", "OPUS", "high_lossy", "f")),
    ];
    expect(albumSourceOptions(tracks).map(({ source, codec, tier, trackCount }) => [source, codec, tier, trackCount]))
      .toEqual([
        ["torrent", "FLAC", "lossless", 2],
        ["torrent", "MP3", "high_lossy", 1],
        ["youtube", "OPUS", "high_lossy", 2],
      ]);
  });

  it("applies the exact choice first and uses the closest quality for uncovered tracks", () => {
    const tracks = [
      track(0, source("youtube", "Opus", "high_lossy", "yt"), source("torrent", "FLAC", "lossless", "p2p")),
      track(1, source("torrent", "MP3", "low", "low"), source("bandcamp", "FLAC", "lossless", "bc")),
      track(2, source("torrent", "MP3", "standard_lossy", "standard"), source("youtube", "Opus", "high_lossy", "closest")),
      track(3),
    ];
    const option = albumSourceOptions(tracks).find((item) => item.source === "torrent" && item.tier === "lossless")!;
    const changed = applyAlbumSource(tracks, option);
    expect(changed.map((item) => item.selected_candidate_idx)).toEqual([1, 1, 1, 0]);
    expect(changed.map((item) => item.url)).toEqual([
      tracks[0].url, "https://example.com/bc", "https://example.com/closest", tracks[3].url,
    ]);
    expect(changed[3]).toBe(tracks[3]);
    expect(tracks[0].selected_candidate_idx).toBe(0);
  });

  it("prefers the higher tier when both fallback tiers are equally close", () => {
    const tracks = [
      track(0, source("youtube", "Opus", "high_lossy", "requested")),
      track(1, source("youtube", "AAC", "standard_lossy", "lower"), source("torrent", "FLAC", "lossless", "higher")),
    ];
    const option = albumSourceOptions(tracks).find((item) => item.tier === "high_lossy")!;
    expect(applyAlbumSource(tracks, option)[1].selected_candidate_idx).toBe(1);
  });
});
