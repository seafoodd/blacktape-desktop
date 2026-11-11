import { describe, it, expect, vi, beforeEach } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  expectedQualityOf,
  formatDetails,
  getExpectedQuality,
  probeAudioFormats,
  type AudioFormat,
} from "./downloadQuality.ts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

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
