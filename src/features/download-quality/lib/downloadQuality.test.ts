import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  buildCanonicalQualityPlan,
  downloadButtonLabel,
  expectedQualityOf,
  extractResolveSongIds,
  formatDetails,
  getExpectedQuality,
  mergeTorrentResolutionsIntoTracks,
  mergeScopedQualityTracks,
  probeAudioFormats,
  torrentQualityLabel,
  type AudioFormat,
} from "./downloadQuality.ts";
import type { TorrentTierResolution } from "@/shared/lib/audio.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it("keeps the missing-track scope and its YouTube option when an unrelated edition is returned", () => {
  const missing: AlbumTrackInfo = {
    index: 12, title: "Endless, Nameless", url: "https://www.youtube.com/watch?v=missing",
    artists: ["Nirvana"], disc_number: 1, track_number: 13, available: true, note: null,
  };
  const unrelated: AlbumTrackInfo = {
    ...missing, title: "Territorial Pissings",
    candidates: [{
      source: { kind: "http_stream", platform: "youtube", url: "https://www.youtube.com/watch?v=wrong" },
      quality_tier: "high_lossy", codec: "Opus", duration_ms: 143_000, match_confidence: 1,
      is_strict_match: true, display_label: "Opus",
    }],
  };
  const scoped = mergeScopedQualityTracks([missing], [unrelated]);
  expect(scoped).toHaveLength(1);
  expect(scoped[0].candidates?.[0].source).toMatchObject({ url: missing.url });
  expect(buildCanonicalQualityPlan(scoped, [missing.url]).options[0].id).toBe("source:youtube:high");
});

it("never labels an MP3 codec as lossless", () => {
  const mixed = { tier: "lossless", codec: "MP3", display_label: "MP3 (Lossless)" } as TorrentTierResolution;
  expect(torrentQualityLabel(mixed)).toBe("Lossless");
  expect(torrentQualityLabel({ ...mixed, codec: "FLAC" })).toBe("FLAC (Lossless)");
});

it("accumulates later platforms for a scoped row and retains its original selection URL", () => {
  const track: AlbumTrackInfo = {
    index: 3, title: "Sunshine (Mirage)", artists: ["Heaven Pierce Her"], available: true,
    url: "https://www.youtube.com/watch?v=linked", track_number: 4, note: null,
  };
  const [linked] = mergeScopedQualityTracks([track], []);
  const [resolved] = mergeScopedQualityTracks([linked], [{
    ...track, candidates: [{
      source: { kind: "http_stream", platform: "bandcamp", url: "https://heavenpierceher.bandcamp.com/track/sunshine" },
      quality_tier: "lossless", codec: "FLAC", is_strict_match: true, match_confidence: 1,
      duration_ms: 0, display_label: "FLAC",
    }],
  }]);
  expect(resolved.url).toBe(track.url);
  expect(buildCanonicalQualityPlan([resolved], [track.url]).options.map((option) => option.id))
    .toEqual(expect.arrayContaining(["source:bandcamp:lossless", "source:youtube:high"]));
});

const format = (over: Partial<AudioFormat> = {}): AudioFormat => ({
  id: "251",
  codec: "Opus",
  bitrate_kbps: 133,
  sample_rate_hz: 48000,
  lossless: false,
  label: "Opus 133 kbps",
  ...over,
});

beforeEach(() => {
  vi.clearAllMocks();
});

describe("expectedQualityOf", () => {
  it("gives the expected quality of YouTube and SoundCloud without asking the backend", () => {
    expect(expectedQualityOf("youtube")).toEqual({ label: "Opus ~160 kbps", lossless: false });
    expect(expectedQualityOf("soundcloud")).toEqual({ label: "MP3 128 / AAC 160 kbps", lossless: false });
  });

  it("matches the platform ignoring case", () => {
    expect(expectedQualityOf("YouTube")?.label).toBe("Opus ~160 kbps");
    expect(expectedQualityOf("SOUNDCLOUD")?.label).toBe("MP3 128 / AAC 160 kbps");
  });

  it("gives null for Bandcamp (asked from the backend) and for unknown or local platforms", () => {
    expect(expectedQualityOf("bandcamp")).toBeNull();
    expect(expectedQualityOf("local")).toBeNull();
    expect(expectedQualityOf("myspace")).toBeNull();
  });
});

describe("getExpectedQuality", () => {
  it("answers YouTube at once, without an invoke", async () => {
    await expect(getExpectedQuality("youtube", "https://www.youtube.com/watch?v=a")).resolves.toEqual({
      label: "Opus ~160 kbps",
      lossless: false,
    });
    expect(invoke).not.toHaveBeenCalled();
  });

  it("asks the backend for Bandcamp", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({ label: "FLAC (lossless)", lossless: true });
    const result = await getExpectedQuality("bandcamp", "https://a.bandcamp.com/album/x");
    expect(invoke).toHaveBeenCalledWith("get_expected_quality", {
      platform: "bandcamp",
      url: "https://a.bandcamp.com/album/x",
    });
    expect(result).toEqual({ label: "FLAC (lossless)", lossless: true });
  });
});

describe("probeAudioFormats", () => {
  it("sends the platform, url and album flag, with trackUrl null when none is given", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([format()]);
    const formats = await probeAudioFormats("bandcamp", "https://a.bandcamp.com/album/x", true);
    expect(invoke).toHaveBeenCalledWith("probe_audio_formats", {
      platform: "bandcamp",
      url: "https://a.bandcamp.com/album/x",
      isAlbum: true,
      trackUrl: null,
    });
    expect(formats).toEqual([format()]);
  });

  it("passes the track url when the caller knows the track", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);
    await probeAudioFormats("youtube", "https://www.youtube.com/watch?v=a", false, "https://www.youtube.com/watch?v=a");
    expect(invoke).toHaveBeenCalledWith("probe_audio_formats", {
      platform: "youtube",
      url: "https://www.youtube.com/watch?v=a",
      isAlbum: false,
      trackUrl: "https://www.youtube.com/watch?v=a",
    });
  });
});

describe("formatDetails", () => {
  it("gives the sample rate in kHz, without trailing zeros", () => {
    expect(formatDetails(format({ sample_rate_hz: 48000 }))).toBe("48 kHz");
    expect(formatDetails(format({ sample_rate_hz: 44100 }))).toBe("44.1 kHz");
  });

  it("adds lossless for lossless formats", () => {
    expect(formatDetails(format({ lossless: true, sample_rate_hz: null }))).toBe("lossless");
  });

  it("joins the sample rate and lossless with a middle dot", () => {
    expect(formatDetails(format({ lossless: true, sample_rate_hz: 44100 }))).toBe("44.1 kHz · lossless");
  });

  it("is empty when yt-dlp gave no sample rate and the format is lossy", () => {
    expect(formatDetails(format({ sample_rate_hz: null, lossless: false }))).toBe("");
  });
});

describe("buildCanonicalQualityPlan", () => {
  it("preserves a manually selected stream when a better torrent candidate arrives", () => {
    const selectedUrl = "https://soundcloud.com/artist/track";
    const selectedCandidate = {
      source: { kind: "http_stream" as const, platform: "soundcloud", url: selectedUrl },
      quality_tier: "standard_lossy" as const,
      codec: "MP3",
      bitrate_kbps: 128,
      duration_ms: 180_000,
      match_confidence: 0.95,
      is_strict_match: true,
      display_label: "SoundCloud · MP3 128 kbps",
    };
    const tracks: AlbumTrackInfo[] = [{
      index: 0,
      title: "Track One",
      artists: ["Artist"],
      track_number: 1,
      url: selectedUrl,
      available: true,
      note: null,
      selected_candidate_idx: 1,
      candidates: [
        {
          source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=one" },
          quality_tier: "high_lossy",
          codec: "Opus",
          bitrate_kbps: 160,
          duration_ms: 180_000,
          match_confidence: 0.99,
          is_strict_match: true,
          display_label: "YouTube · Opus 160 kbps",
        },
        selectedCandidate,
      ],
    }];
    const resolutions: TorrentTierResolution[] = [{
      tier: "lossless",
      info_hash: "lossless-hash",
      magnet_uri: "magnet:?xt=urn:btih:lossless-hash",
      release_title: "Artist - Album [FLAC]",
      provider: "Archive.org",
      seeders: 12,
      codec: "FLAC",
      bitrate_kbps: null,
      display_label: "FLAC (Lossless)",
      matched_count: 1,
      total_expected: 1,
      total_matched_bytes: 30_000_000,
      missing_titles: [],
      ambiguous_titles: [],
      layout_warning: null,
      tracks: [{
        expected_index: 0,
        candidate: {
          source: {
            kind: "torrent",
            info_hash: "lossless-hash",
            magnet_uri: "magnet:?xt=urn:btih:lossless-hash",
            file_index: 0,
            file_path: "01 Track One.flac",
            file_size: 30_000_000,
            seeders: 12,
          },
          quality_tier: "lossless",
          codec: "FLAC",
          duration_ms: 180_000,
          match_confidence: 1,
          is_strict_match: true,
          display_label: "Torrent · FLAC",
        },
      }],
    }];

    const [merged] = mergeTorrentResolutionsIntoTracks(tracks, resolutions);

    expect(merged.selected_candidate_idx).toBe(2);
    expect(merged.candidates?.[merged.selected_candidate_idx ?? 0]).toBe(selectedCandidate);
    expect(merged.candidates?.[merged.selected_candidate_idx ?? 0]?.source).toMatchObject({ url: selectedUrl });
    expect(merged.url).toBe(selectedUrl);
  });

  it("falls back to semantic tiers before candidates finish resolving", () => {
    const plan = buildCanonicalQualityPlan([]);
    expect(plan.hasDiscoveredCandidates).toBe(false);
    expect(plan.best.id).toBe("best");
    expect(plan.best.lossless).toBe(false);
    expect(plan.options.map((o) => o.id)).toEqual(["high", "standard"]);
  });

  it("reports honest opus summary instead of claiming lossless when only opus exists", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Track 1",
        artists: ["Aphex Twin"],
        track_number: 1,
        url: "https://youtube.com/watch?v=1",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 200_000,
            match_confidence: 0.95,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
          {
            source: { kind: "http_stream", platform: "bandcamp", url: "https://a.bandcamp.com/track/1" },
            quality_tier: "standard_lossy",
            codec: "MP3",
            bitrate_kbps: 128,
            duration_ms: 200_000,
            match_confidence: 0.95,
            is_strict_match: true,
            display_label: "Bandcamp · MP3 128 kbps",
          },
        ],
      },
    ]);

    expect(plan.hasDiscoveredCandidates).toBe(true);
    expect(plan.best.lossless).toBe(false);
    expect(plan.best.details).toContain("Opus 160 kbps (YOUTUBE)");
    expect(plan.options.map((o) => o.id)).toEqual(["source:youtube:high", "source:bandcamp:standard"]);
    expect(plan.options[0].platform).toBe("youtube");
    expect(plan.options[1].platform).toBe("bandcamp");
  });

  it("marks discovered Bandcamp free lossless FLAC as best and includes source:bandcamp:lossless", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "In Absentia",
        artists: ["Heaven Pierce Her"],
        track_number: 1,
        url: "https://heavenpierceher.bandcamp.com/track/in-absentia",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "bandcamp", url: "https://heavenpierceher.bandcamp.com/track/in-absentia" },
            quality_tier: "lossless",
            codec: "FLAC",
            duration_ms: 273_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "Bandcamp · FLAC (Lossless)",
          },
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 273_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube Music (Opus 160k)",
          },
        ],
      },
    ]);

    expect(plan.best.lossless).toBe(true);
    expect(plan.best.details).toContain("FLAC (Lossless) (BANDCAMP)");
    expect(plan.options.map((o) => o.id)).toEqual(["source:bandcamp:lossless", "source:youtube:high"]);
    expect(plan.options[0].lossless).toBe(true);
  });

  it("reports upgradableCount and skips redundant upgrades via local_tier notes", () => {
    const plan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Already High",
        artists: ["Artist"],
        track_number: 1,
        url: "resolve:song:1",
        available: true,
        note: "local_tier:high",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
      {
        index: 1,
        title: "Needs Upgrade",
        artists: ["Artist"],
        track_number: 2,
        url: "resolve:song:2",
        available: true,
        note: "local_tier:low",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=2" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
    ]);

    expect(plan.best.upgradableCount).toBe(1);
    expect(plan.best.details).toContain("Upgrades 1/2:");

    const allHighPlan = buildCanonicalQualityPlan([
      {
        index: 0,
        title: "Already High",
        artists: ["Artist"],
        track_number: 1,
        url: "resolve:song:1",
        available: true,
        note: "local_tier:high",
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream", platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy",
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
    ]);

    expect(allHighPlan.best.upgradableCount).toBe(0);
    expect(allHighPlan.best.details).toBe("Already at equal or higher quality (no upgrade needed)");
    expect(allHighPlan.options[0].upgradableCount).toBe(0);
  });

  it("renders separate torrent tier cards and falls back to HTTP for missing torrent tracks in best", () => {
    const baseTracks = [
      {
        index: 0,
        title: "Track One",
        artists: ["Artist"],
        track_number: 1,
        url: "https://youtube.com/watch?v=1",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream" as const, platform: "youtube", url: "https://youtube.com/watch?v=1" },
            quality_tier: "high_lossy" as const,
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
      {
        index: 1,
        title: "Track Two",
        artists: ["Artist"],
        track_number: 2,
        url: "https://youtube.com/watch?v=2",
        available: true,
        note: null,
        selected_candidate_idx: 0,
        candidates: [
          {
            source: { kind: "http_stream" as const, platform: "youtube", url: "https://youtube.com/watch?v=2" },
            quality_tier: "high_lossy" as const,
            codec: "Opus",
            bitrate_kbps: 160,
            duration_ms: 180_000,
            match_confidence: 0.99,
            is_strict_match: true,
            display_label: "YouTube · Opus 160 kbps",
          },
        ],
      },
    ];

    const resolutions = [
      {
        tier: "lossless" as const,
        info_hash: "aaaa1111",
        magnet_uri: "magnet:?xt=urn:btih:aaaa1111",
        release_title: "Artist - Album [FLAC]",
        provider: "ApiBay",
        seeders: 12,
        codec: "FLAC",
        bitrate_kbps: null,
        display_label: "FLAC (Lossless)",
        matched_count: 1,
        total_expected: 2,
        total_matched_bytes: 32_000_000,
        missing_titles: ["Track Two"],
        ambiguous_titles: [],
        layout_warning: "Matched 1/2 tracks · 1 missing",
        tracks: [
          {
            expected_index: 0,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "aaaa1111",
                magnet_uri: "magnet:?xt=urn:btih:aaaa1111",
                file_index: 0,
                file_path: "01 Track One.flac",
                file_size: 32_000_000,
                seeders: 12,
              },
              quality_tier: "lossless" as const,
              codec: "FLAC",
              duration_ms: 180_000,
              match_confidence: 1.0,
              is_strict_match: true,
              display_label: "Torrent · FLAC (Lossless)",
            },
          },
        ],
      },
      {
        tier: "high_lossy" as const,
        info_hash: "bbbb2222",
        magnet_uri: "magnet:?xt=urn:btih:bbbb2222",
        release_title: "Artist - Album [MP3 320]",
        provider: "ApiBay",
        seeders: 8,
        codec: "MP3",
        bitrate_kbps: 320,
        display_label: "MP3 320 kbps",
        matched_count: 2,
        total_expected: 2,
        total_matched_bytes: 14_000_000,
        missing_titles: [],
        ambiguous_titles: [],
        layout_warning: null,
        tracks: [
          {
            expected_index: 0,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "bbbb2222",
                magnet_uri: "magnet:?xt=urn:btih:bbbb2222",
                file_index: 0,
                file_path: "01 Track One.mp3",
                file_size: 7_000_000,
                seeders: 8,
              },
              quality_tier: "high_lossy" as const,
              codec: "MP3",
              bitrate_kbps: 320,
              duration_ms: 180_000,
              match_confidence: 1.0,
              is_strict_match: true,
              display_label: "Torrent · MP3 320 kbps",
            },
          },
          {
            expected_index: 1,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "bbbb2222",
                magnet_uri: "magnet:?xt=urn:btih:bbbb2222",
                file_index: 1,
                file_path: "02 Track Two.mp3",
                file_size: 7_000_000,
                seeders: 8,
              },
              quality_tier: "high_lossy" as const,
              codec: "MP3",
              bitrate_kbps: 320,
              duration_ms: 180_000,
              match_confidence: 1.0,
              is_strict_match: true,
              display_label: "Torrent · MP3 320 kbps",
            },
          },
        ],
      },
    ];

    const merged = mergeTorrentResolutionsIntoTracks(baseTracks, resolutions);
    expect(merged[0]?.selected_candidate_idx).toBe(0);
    expect(merged[0]?.candidates?.[0]?.source.kind).toBe("torrent");
    const plan = buildCanonicalQualityPlan(merged, undefined, resolutions);

    expect(plan.best.lossless).toBe(true);
    expect(plan.best.details).toContain("1× FLAC (Lossless) (TORRENT)");
    expect(plan.best.details).toContain("1× MP3 320 kbps (TORRENT)");
    expect(plan.options.map((o) => o.id)).toEqual([
      "source:torrent:lossless",
      "source:torrent:high",
      "source:youtube:high",
    ]);
    expect(plan.options[0].warning).toContain("Matched 1/2 tracks");
    expect(plan.options[0].details).toContain("12 seeders");
    expect(plan.options[0].details).toContain("ApiBay");
    expect(plan.options[1].details).toContain("8 seeders");
    expect(plan.options[1].details).toContain("ApiBay");
  });

  it("shows all discovered torrent releases in priority order instead of hiding secondary providers", () => {
    const resolutions = [
      {
        tier: "lossless" as const,
        info_hash: "archive-hash",
        magnet_uri: "magnet:?xt=urn:btih:archive-hash",
        release_title: "Nevermind (Archive)",
        provider: "Archive.org",
        seeders: null,
        codec: "FLAC",
        bitrate_kbps: null,
        display_label: "FLAC",
        matched_count: 1,
        total_expected: 1,
        total_matched_bytes: 345_000_000,
        missing_titles: [],
        ambiguous_titles: [],
        layout_warning: null,
        tracks: [
          {
            expected_index: 0,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "archive-hash",
                magnet_uri: "magnet:?xt=urn:btih:archive-hash",
                file_index: 0,
                file_path: "01 Smells Like Teen Spirit.flac",
                file_size: 34_500_000,
                seeders: 0,
              },
              quality_tier: "lossless" as const,
              codec: "FLAC",
              duration_ms: 301000,
              match_confidence: 1,
              is_strict_match: true,
              display_label: "TORRENT",
            },
          },
        ],
      },
      {
        tier: "lossless" as const,
        info_hash: "rutracker-hash",
        magnet_uri: "rutracker:123",
        release_title: "Nevermind (RuTracker)",
        provider: "RuTracker",
        seeders: 44,
        codec: "FLAC",
        bitrate_kbps: null,
        display_label: "FLAC",
        matched_count: 1,
        total_expected: 1,
        total_matched_bytes: 320_000_000,
        missing_titles: [],
        ambiguous_titles: [],
        layout_warning: null,
        tracks: [
          {
            expected_index: 0,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "rutracker-hash",
                magnet_uri: "rutracker:123",
                file_index: 0,
                file_path: "01 Smells Like Teen Spirit.flac",
                file_size: 32_000_000,
                seeders: 44,
              },
              quality_tier: "lossless" as const,
              codec: "FLAC",
              duration_ms: 301000,
              match_confidence: 1,
              is_strict_match: true,
              display_label: "TORRENT",
            },
          },
        ],
      },
    ];
    const merged = mergeTorrentResolutionsIntoTracks(
      [
        {
          index: 0,
          title: "Smells Like Teen Spirit",
          artists: ["Nirvana"],
          url: "canonical:nevermind:1",
          available: true,
          track_number: 1,
          note: null,
          candidates: [],
        },
      ],
      resolutions,
    );
    const plan = buildCanonicalQualityPlan(merged, undefined, resolutions);
    expect(plan.options.map((o) => o.id)).toEqual([
      "source:torrent:lossless",
      "source:torrent:lossless:rutracker-hash",
    ]);
    expect(plan.options[0].details).toContain("Archive.org");
    expect(plan.options[1].details).toContain("44 seeders");
    expect(plan.options[1].details).toContain("RuTracker");
  });

  it("synthesizes track entries for matched torrent files when baseTracks is empty", () => {
    const merged = mergeTorrentResolutionsIntoTracks([], [
      {
        tier: "lossless" as const,
        info_hash: "abc123",
        magnet_uri: "magnet:?xt=urn:btih:abc123",
        release_title: "The Doors - The Doors [FLAC]",
        provider: "Archive.org",
        seeders: 10,
        codec: "FLAC",
        bitrate_kbps: null,
        display_label: "FLAC (Lossless)",
        matched_count: 1,
        total_expected: 1,
        total_matched_bytes: 25_000_000,
        missing_titles: [],
        ambiguous_titles: [],
        layout_warning: null,
        tracks: [
          {
            expected_index: 0,
            candidate: {
              source: {
                kind: "torrent" as const,
                info_hash: "abc123",
                magnet_uri: "magnet:?xt=urn:btih:abc123",
                file_index: 0,
                file_path: "01 Break On Through.flac",
                file_size: 25_000_000,
                seeders: 10,
              },
              quality_tier: "lossless" as const,
              codec: "FLAC",
              bitrate_kbps: undefined,
              duration_ms: 148_000,
              match_confidence: 0.95,
              is_strict_match: true,
              display_label: "Torrent · FLAC (Lossless)",
              matched_title: "Break On Through",
              matched_artist: "The Doors",
            },
          },
        ],
      },
    ]);

    expect(merged).toHaveLength(1);
    expect(merged[0].url).toBe("magnet:?xt=urn:btih:abc123");
    expect(merged[0].candidates).toHaveLength(1);
    expect(merged[0].candidates?.[0]?.source.kind).toBe("torrent");
    expect(merged[0].candidates?.[0]?.quality_tier).toBe("lossless");
  });
});

describe("extractResolveSongIds", () => {
  it("parses single, batch, and album resolve urls into deduplicated numeric ids", () => {
    expect(extractResolveSongIds("resolve:song:42")).toEqual([42]);
    expect(extractResolveSongIds("resolve:batch:10,20,10")).toEqual([10, 20]);
    expect(
      extractResolveSongIds("resolve:album:Artist::Album", ["resolve:song:5", "resolve:song:6", "resolve:song:5"]),
    ).toEqual([5, 6]);
  });
});

describe("downloadButtonLabel", () => {
  it("labels best-quality option accurately and uses plain Download for other presets", () => {
    expect(downloadButtonLabel(true, "best", [])).toBe("Download in the best quality");
    expect(downloadButtonLabel(true, "high", [])).toBe("Download");
    expect(downloadButtonLabel(true, "standard", [])).toBe("Download");
    expect(downloadButtonLabel(true, "source:bandcamp:standard", [])).toBe("Download");
    expect(downloadButtonLabel(false, null, [])).toBe("Download in the best quality");
    expect(downloadButtonLabel(false, "251", [format()])).toBe("Download");
  });
});
