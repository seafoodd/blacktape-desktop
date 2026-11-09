import type { MouseEvent } from "react";
import clsx from "clsx";
import { BiDownload } from "react-icons/bi";
import { MdPause, MdPlayArrow } from "react-icons/md";
import rowStyles from "@/features/library/ui/song-list-item/song-list-item.module.css";
import { isStreamingUrl, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { flyToLibrary } from "@/shared/lib/flyAnimation.ts";
import {
  trackRowStatus,
  type AlbumTrackInfo,
  type TrackTaskState,
} from "@/shared/lib/albumPreview.ts";
import type { DownloadState } from "@/shared/lib/libraryIndex.ts";
import { formatTrackDuration } from "@/shared/lib/trackDuration.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import styles from "./album-preview.module.css";

export interface AlbumTrackRowProps {
  track: AlbumTrackInfo;
  position: number;
  selected: boolean;
  selectedAbove: boolean;
  selectedBelow: boolean;
  /** Library state of the track (from `useTrackStatuses`). */
  libraryState: DownloadState | undefined;
  /** Download task that serves this row, when there is one. */
  task: TrackTaskState | undefined;
  cover: string | null;
  onSelect: (e: MouseEvent) => void;
  onPlay: () => void;
  onDownload: () => void;
  onContextMenu: (e: MouseEvent) => void;
}

/** One track of the album preview: number, play button, title, then duration and the download slot. */
export const AlbumTrackRow = ({
  track,
  position,
  selected,
  selectedAbove,
  selectedBelow,
  libraryState,
  task,
  cover,
  onSelect,
  onPlay,
  onDownload,
  onContextMenu,
}: AlbumTrackRowProps) => {
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);
  const streamLoading = useAudioStore((s) => s.streamLoading);
  const loadingUrl = useStreamStore((s) => s.pendingUrl);
  const active = track.available && isStreamingUrl(currentSong, isStreaming, track.url);
  const playing = active && isPlaying && !streamLoading;
  const loading = track.available && (loadingUrl === track.url || (active && streamLoading));
  const status = trackRowStatus(track, libraryState, task);

  return (
    <div
      className={clsx(rowStyles.song, styles.trackRow, {
        [rowStyles.selected]: selected,
        [rowStyles.selectedTop]: selected && !selectedAbove,
        [rowStyles.selectedBottom]: selected && !selectedBelow,
        [styles.unavailable]: !track.available,
      })}
      data-song-row
      aria-selected={selected}
      aria-disabled={!track.available}
      title={track.available ? undefined : (track.note ?? "Not available")}
      role="button"
      tabIndex={0}
      onClick={(e) => track.available && onSelect(e)}
      onDoubleClick={() => track.available && onPlay()}
      onContextMenu={track.available ? onContextMenu : undefined}
    >
      <div className={rowStyles.songLeft}>
        <div className={rowStyles.numSlot}>
          <div className={clsx(rowStyles.numContent, (active || loading) && styles.numHidden)}>
            <div className={rowStyles.songTrackNumber}>
              {String(track.track_number ?? position + 1).padStart(2, "0")}
            </div>
          </div>
          {track.available && (
            <button
              className={clsx(rowStyles.rowPlayButton, (active || loading) && styles.alwaysShown)}
              title={playing ? "Pause" : "Play preview"}
              aria-label={playing ? "Pause" : "Play preview"}
              onClick={(e) => {
                e.stopPropagation();
                onPlay();
              }}
              onDoubleClick={(e) => e.stopPropagation()}
            >
              {loading ? (
                <span className={styles.spinner} aria-hidden="true" />
              ) : playing ? (
                <MdPause size={22} />
              ) : (
                <MdPlayArrow size={22} className={rowStyles.playIcon} />
              )}
            </button>
          )}
        </div>
        <div className={clsx(rowStyles.songTitle, "truncate")}>
          {decodeHtmlEntities(track.title)}
          {track.artists.length > 0 && (
            <span className={styles.trackArtist}> · {track.artists.join(", ")}</span>
          )}
        </div>
      </div>
      <div className={rowStyles.songRight}>
        {!track.available && track.note && (
          <div className={clsx(styles.note, "truncate")}>{track.note}</div>
        )}
        <span className={styles.duration}>{formatTrackDuration(track.duration_ms)}</span>
        <div className={styles.rowAction}>
          {status.kind === "downloaded" && <DownloadStatusBadge state="downloaded" size={16} />}
          {status.kind === "running" &&
            (status.percent > 0 ? (
              <span className={styles.percent} aria-live="polite">
                {status.percent}%
              </span>
            ) : (
              <span className={styles.spinner} role="status" aria-label="Downloading" />
            ))}
          {status.kind === "download" && (
            <button
              type="button"
              className={styles.downloadBtn}
              title={`Download ${track.title}`}
              aria-label={`Download ${track.title}`}
              onClick={(e) => {
                e.stopPropagation();
                flyToLibrary(e.currentTarget.getBoundingClientRect(), cover ?? undefined);
                onDownload();
              }}
              onDoubleClick={(e) => e.stopPropagation()}
            >
              <BiDownload size={18} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
};
