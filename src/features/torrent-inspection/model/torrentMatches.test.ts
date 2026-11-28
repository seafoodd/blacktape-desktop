import { describe, expect, it } from "vitest";
import { mapTorrentTracks } from "@/features/torrent-inspection/model/torrentMatches.ts";
import type { TorrentTrack } from "@/shared/lib/audio.ts";

const file = (file_index: number, title: string, disc_number: number): TorrentTrack => ({
  file_index, title, disc_number, track_number: 1, codec: "FLAC",
  file_path: `CD${disc_number}/01 ${title}.flac`, size_bytes: 100,
});

describe("torrent file mapping", () => {
  it("keeps original file indexes when the same track number and title appear on different discs", () => {
    const rows = mapTorrentTracks(
      [{ title: "Intro", disc: 1, number: 1 }, { title: "Intro", disc: 2, number: 1 }],
      [file(7, "Intro", 2), file(3, "Intro", 1)],
    );
    expect(rows.map((row) => row.kind === "matched" ? row.file.file_index : null)).toEqual([3, 7]);
  });

  it("keeps alternate files and contested identities ambiguous", () => {
    const rows = mapTorrentTracks(
      [{ title: "Intro", disc: 1, number: 1 }, { title: "Intro" }, { title: "Intro (Live)" }],
      [file(7, "Intro", 1), file(3, "Intro", 1)],
    );
    expect(rows.map((row) => row.kind)).toEqual(["ambiguous", "ambiguous", "missing"]);
    expect(rows[0]).toMatchObject({ candidates: [{ file_index: 7 }, { file_index: 3 }] });
    expect(mapTorrentTracks([{ title: "Intro" }, { title: "Intro" }], [file(3, "Intro", 1)])
      .map((row) => row.kind)).toEqual(["ambiguous", "ambiguous"]);
  });

  it("uses an exact artist prefix and excludes samples and version variants", () => {
    const rows = mapTorrentTracks(
      [{ artist: "Björk", title: "Undo" }],
      [file(1, "Björk - Undo", 1), { ...file(2, "Undo", 1), is_sample: true }, file(3, "Undo (Live)", 1)],
    );
    expect(rows[0]).toMatchObject({ kind: "matched", file: { file_index: 1 } });
  });
});
