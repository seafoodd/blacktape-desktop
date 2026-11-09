import { describe, expect, it, vi } from "vitest";
import type { RemoteTrack } from "@/shared/lib/remoteTrack.ts";
import { ItemType } from "@/shared/store/libraryStore.ts";
import {
  buildRemoteTrackMenu,
  remoteTrackDownloadParams,
  type RemoteTrackMenuActions,
} from "./buildRemoteTrackMenu.ts";

const track = (over: Partial<RemoteTrack> = {}): RemoteTrack => ({
  platform: "bandcamp",
  url: "https://band.bandcamp.com/track/song",
  title: "Song",
  artist: "Band",
  album: "Album",
  album_url: null,
  cover: "https://img.test/c.jpg",
  duration_ms: 200_000,
  ...over,
});

const makeActions = (): RemoteTrackMenuActions => ({
  download: vi.fn(),
  addToQueue: vi.fn(),
  playNext: vi.fn(),
  copyLink: vi.fn(),
  openOnPlatform: vi.fn(),
});

describe("remoteTrackDownloadParams", () => {
  it("downloads a track without a known album as a single track", () => {
    expect(remoteTrackDownloadParams(track({ album_url: null }))).toEqual({
      platform: "bandcamp",
      downloadType: ItemType.Track,
      url: "https://band.bandcamp.com/track/song",
      artistName: "Band",
      title: "Song",
      coverUrl: "https://img.test/c.jpg",
    });
  });

  it("sends a track with an album url through the album pipeline with only that track", () => {
    const params = remoteTrackDownloadParams(
      track({ album_url: "https://band.bandcamp.com/album/record", url: "https://band.bandcamp.com/track/a" }),
    );
    expect(params.downloadType).toBe(ItemType.Album);
    expect(params.url).toBe("https://band.bandcamp.com/album/record");
    expect(params.trackUrls).toEqual(["https://band.bandcamp.com/track/a"]);
    expect(params.title).toBe("Album");
  });

  it("uses the track title as the album title when the album name is unknown", () => {
    const params = remoteTrackDownloadParams(track({ album_url: "https://a.test/album/x", album: null }));
    expect(params.title).toBe("Song");
  });

  it("maps a missing cover to an undefined coverUrl", () => {
    expect(remoteTrackDownloadParams(track({ cover: null })).coverUrl).toBeUndefined();
  });

  it("keeps the platform of the track", () => {
    expect(remoteTrackDownloadParams(track({ platform: "youtube" })).platform).toBe("youtube");
  });
});

describe("buildRemoteTrackMenu", () => {
  it("lists Download, Add to queue, Play next, a separator, Copy link and Open on the platform, in order", () => {
    const items = buildRemoteTrackMenu(track(), {}, makeActions());
    expect(items.map((i) => i.label)).toEqual([
      "Download",
      "Add to queue",
      "Play next",
      "",
      "Copy link",
      "Open on Bandcamp",
    ]);
    expect(items[3].separator).toBe(true);
  });

  it("names the platform in the open item", () => {
    expect(buildRemoteTrackMenu(track({ platform: "soundcloud" }), {}, makeActions())[5].label).toBe(
      "Open on SoundCloud",
    );
    expect(buildRemoteTrackMenu(track({ platform: "youtube" }), {}, makeActions())[5].label).toBe(
      "Open on YouTube",
    );
  });

  it("shows a disabled-looking Downloaded item without an action when the track is downloaded", () => {
    const actions = makeActions();
    const items = buildRemoteTrackMenu(track(), { downloadState: "downloaded" }, actions);
    expect(items[0].label).toBe("Downloaded");
    expect(items[0].disabled).toBe(true);
    expect(items[0].action).toBeUndefined();
    expect(actions.download).not.toHaveBeenCalled();
  });

  it("keeps Download for a partly downloaded or unknown track", () => {
    expect(buildRemoteTrackMenu(track(), { downloadState: "partial" }, makeActions())[0].label).toBe(
      "Download",
    );
    expect(buildRemoteTrackMenu(track(), { downloadState: "none" }, makeActions())[0].label).toBe(
      "Download",
    );
    expect(buildRemoteTrackMenu(track(), {}, makeActions())[0].label).toBe("Download");
  });

  it("Download calls the download action with the track", () => {
    const actions = makeActions();
    const t = track();
    buildRemoteTrackMenu(t, {}, actions)[0].action?.();
    expect(actions.download).toHaveBeenCalledWith(t);
  });

  it("Add to queue calls only addToQueue", () => {
    const actions = makeActions();
    const t = track();
    buildRemoteTrackMenu(t, {}, actions)[1].action?.();
    expect(actions.addToQueue).toHaveBeenCalledWith(t);
    expect(actions.playNext).not.toHaveBeenCalled();
  });

  it("Play next calls only playNext", () => {
    const actions = makeActions();
    const t = track();
    buildRemoteTrackMenu(t, {}, actions)[2].action?.();
    expect(actions.playNext).toHaveBeenCalledWith(t);
    expect(actions.addToQueue).not.toHaveBeenCalled();
  });

  it("Copy link passes the track url", () => {
    const actions = makeActions();
    buildRemoteTrackMenu(track(), {}, actions)[4].action?.();
    expect(actions.copyLink).toHaveBeenCalledWith("https://band.bandcamp.com/track/song");
  });

  it("Open on the platform passes the track url", () => {
    const actions = makeActions();
    buildRemoteTrackMenu(track(), {}, actions)[5].action?.();
    expect(actions.openOnPlatform).toHaveBeenCalledWith("https://band.bandcamp.com/track/song");
  });

  it("does not depend on the album url for the menu items (only Download uses it)", () => {
    const labels = buildRemoteTrackMenu(track({ album_url: "https://a.test/album/x" }), {}, makeActions()).map(
      (i) => i.label,
    );
    expect(labels).toHaveLength(6);
  });
});
