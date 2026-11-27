import React from "react";
import { createRoot } from "react-dom/client";
import "@/styles/reset.css";
import "@/styles/variables.css";
import "@/styles/globals.css";
import "@/styles/utilities.css";
import { AlbumTrackRow } from "@/pages/album-preview/AlbumTrackRow.tsx";
import type { AlbumTrackInfo, TrackTaskState } from "@/shared/lib/remote/albumPreview.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useDownloadStore, type DownloadProgress } from "@/shared/store/downloadStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { TrackDownloadProgress } from "@/shared/ui/track-download-progress/TrackDownloadProgress.tsx";
import artistStyles from "@/pages/artist-albums/artist-albums.module.css";

const track = (index: number): AlbumTrackInfo => ({
  index, title: `Track ${index + 1}`, artists: ["Artist"], track_number: index + 1,
  url: `https://example.com/track-${index + 1}`, available: true, note: null, duration_ms: 180_000,
});

const activeDownloads: Record<string, DownloadProgress> = {};
const taskState = (id: string, percent: number): DownloadProgress => ({
  id, url: `https://example.com/${id}`, status: "downloading", current: 0, total: 1,
  message: "", artistName: "Artist", albumTitle: `Track ${id}`, itemType: "Track",
  platform: "bandcamp", overallPercent: percent,
  tracks: [{ title: `Track ${id}`, percent, status: "downloading" }],
});
activeDownloads.zero = taskState("zero", 0);
activeDownloads.middle = taskState("middle", 75);
activeDownloads.full = taskState("full", 100);
useDownloadStore.setState({ activeDownloads });
useAudioStore.setState({ currentSong: null });

function Fixture() {
  const modal = useModalStore((state) => state.content);
  const [selected, setSelected] = React.useState(0);
  const rows = [0, 75, 100].map((percent, i) => ({ percent, id: ["zero", "middle", "full"][i] }));
  return <main style={{ padding: 24, color: "white", background: "#101010", minHeight: "100vh" }}>
    <output id="selection-count">{selected}</output>
    {rows.map(({ percent, id }, position) => <AlbumTrackRow key={id} track={track(position)} position={position}
      selected={false} selectedAbove={false} selectedBelow={false} libraryState={undefined}
      task={{ phase: "running", percent, taskId: id } as TrackTaskState}
      cover={null} albumTitle="Album" onSelect={() => setSelected((n) => n + 1)} onPlay={() => {}}
      onDownload={() => {}} onChooseQuality={() => {}} onContextMenu={() => {}} />)}
    <AlbumTrackRow track={track(3)} position={3} selected={false} selectedAbove={false} selectedBelow={false}
      task={{ phase: "done" } as TrackTaskState} libraryState="downloaded" cover={null} albumTitle="Album"
      onSelect={() => setSelected((n) => n + 1)} onPlay={() => {}} onDownload={() => {}}
      onChooseQuality={() => {}} onContextMenu={() => {}} />
    <ul id="artist-downloads" style={{ marginTop: 24 }}>
      <li className={`${artistStyles.song} ${artistStyles.downloadingSong}`} data-song-row tabIndex={0}>
        <span>ArtistAlbums · downloading track</span>
        <div className={artistStyles.songRight}><div className={artistStyles.trackPercentage}>
          <TrackDownloadProgress taskId="middle" percent={75} />
        </div></div>
      </li>
    </ul>
    <div id="modal-content">{modal}</div>
  </main>;
}

createRoot(document.getElementById("root")!).render(<Fixture />);
