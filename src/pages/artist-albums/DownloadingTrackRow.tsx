import clsx from "clsx";
import { HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { IoMdPlay } from "react-icons/io";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { isStreamingUrl, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { decodeHtmlEntities } from "@/shared/lib/ui/html.ts";
import { TrackDownloadProgress } from "@/shared/ui/track-download-progress/TrackDownloadProgress.tsx";
import { DownloadSplitButton } from "@/shared/ui/download-split-button/DownloadSplitButton.tsx";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import type { Platform } from "@/shared/types.ts";
import styles from "@/features/library/ui/missing-track-row/missing-track-row.module.css";

interface DownloadingTrackRowProps {
  taskId: string;
  trackIndex: number;
  title: string;
  trackNumber: number;
  percent: number;
  status: string;
  platform: Platform;
  albumUrl: string;
  artistName: string;
  albumTitle: string;
  coverUrl?: string;
  playUrl?: string;
}

/** Shared row for downloading/cancelled tracks in ArtistAlbums with hover preview playback and per-track cancel/retry. */
export const DownloadingTrackRow = ({
  taskId,
  trackIndex,
  title,
  trackNumber,
  percent,
  status,
  platform,
  albumUrl,
  artistName,
  albumTitle,
  coverUrl,
  playUrl,
}: DownloadingTrackRowProps) => {
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);
  const isCurrent = Boolean(playUrl && isStreamingUrl(currentSong, isStreaming, playUrl));
  const playing = isCurrent && isPlaying;
  const decodedTitle = decodeHtmlEntities(title);
  const cancelled = status === "cancelled";
  const finished = status === "completed" || status === "failed" || cancelled;

  const play = () => {
    if (!playUrl) return;
    if (isCurrent) {
      void useAudioStore.getState().togglePlay();
      return;
    }
    void useStreamStore.getState().play({
      item_type: "Track" as never,
      name: title,
      band_name: artistName,
      album_name: albumTitle,
      item_url_path: playUrl,
      img: coverUrl ?? "",
      platform,
    } as never);
  };

  const redownload = () => {
    if (!playUrl) return;
    void useDownloadStore.getState().startDownload({
      platform,
      downloadType: "Album",
      url: albumUrl,
      trackUrls: [playUrl],
      artistName,
      title: albumTitle,
      coverUrl,
      placement: {
        album: albumTitle,
        album_artist: artistName,
        title,
        track_number: trackNumber,
      },
    });
  };

  return (
    <li
      className={clsx(styles.row, {
        [styles.unavailable]: !playUrl,
        [styles.playing]: isCurrent,
      })}
      data-song-row
      tabIndex={0}
      onDoubleClick={playUrl ? play : undefined}
    >
      <div className={styles.left}>
        <div className={clsx(styles.numSlot, isCurrent && styles.numSlotCurrent)}>
          <div className={styles.number}>
            {isCurrent ? (
              playing ? (
                <HiOutlineMusicNote className={styles.playingIcon} />
              ) : (
                <HiMiniPause className={styles.pauseIcon} />
              )
            ) : (
              String(trackNumber).padStart(2, "0")
            )}
          </div>
          {playUrl && (
            <button
              type="button"
              className={styles.playButton}
              title={playing ? "Pause" : "Play preview"}
              aria-label={playing ? `Pause ${decodedTitle}` : `Play ${decodedTitle}`}
              onClick={(e) => {
                e.stopPropagation();
                play();
              }}
            >
              {playing ? <HiMiniPause size={18} /> : <IoMdPlay size={16} />}
            </button>
          )}
        </div>
        <div className={clsx(styles.title, "truncate")}>{decodedTitle}</div>
      </div>
      <div className={styles.right}>
        {cancelled && playUrl ? (
          <span className={styles.downloadButton} onDoubleClick={(e) => e.stopPropagation()}>
            <DownloadSplitButton
              size={30}
              className={styles.downloadSplit}
              title={`Download ${decodedTitle}`}
              onDownload={redownload}
              onChooseQuality={() =>
                openQualityMenu({
                  platform,
                  downloadType: "Album",
                  url: albumUrl,
                  trackUrls: [playUrl],
                  artistName,
                  title: albumTitle,
                  coverUrl,
                  placement: {
                    album: albumTitle,
                    album_artist: artistName,
                    title,
                    track_number: trackNumber,
                  },
                })
              }
            />
          </span>
        ) : (
          <span className={styles.percent}>
            <TrackDownloadProgress
              taskId={taskId}
              trackIndex={trackIndex}
              percent={percent}
              finished={finished}
            />
          </span>
        )}
      </div>
    </li>
  );
};
