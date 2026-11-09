import { useMemo } from "react";
import clsx from "clsx";
import { BiDownload } from "react-icons/bi";
import { HiOutlineMusicNote } from "react-icons/hi";
import { HiMiniPause } from "react-icons/hi2";
import { MdErrorOutline, MdPause, MdPlayArrow } from "react-icons/md";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { MissingTracks } from "@/shared/lib/libraryIndex.ts";
import { formatTrackDuration } from "@/shared/lib/trackDuration.ts";
import { remoteTrackToStreamItem } from "@/shared/lib/remoteTrack.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { explainFailure } from "@/features/library/model/downloadFailure.ts";
import FindMissingModal from "@/features/library/ui/FindMissingModal.tsx";
import { platformLabel } from "@/shared/lib/platforms.ts";
import { restoreTrash } from "@/shared/lib/trash.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useRemoteTrackMenu } from "@/shared/hooks/useRemoteTrackMenu.ts";
import { isStreamingUrl, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { useMissingTracksStore } from "@/features/library/model/missingTracksStore.ts";
import {
  missingRemoteTrack,
  missingRemoteTracks,
  missingStreamItems,
  missingTrackNumber,
} from "./missingTrackRow.ts";
import styles from "./missing-track-row.module.css";

interface MissingTrackRowProps {
  /** Library title of the album (the key of the missing songs store). */
  album: string;
  missing: MissingTracks;
  track: AlbumTrackInfo;
  /** Progress of the song's download (0..100), shown instead of the download button while it runs. */
  percent?: number;
  /** A running download (started elsewhere, e.g. from the search) fetches this song. */
  downloading?: boolean;
  /** A song that was just removed: shown dimmed in its place, no actions. */
  ghost?: boolean;
  /** The platform has no stream for this song: no download or preview, but "Find another source". */
  unavailable?: boolean;
  /** The error of the song's last failed download: the row shows the red error button with the way out. */
  failure?: string;
  /** Id of this song in the trash: the menu offers "Restore from trash". */
  trashId?: number;
  selected?: boolean;
  /** Whether the rows above/below are selected too (one frame per run of selected rows). */
  selectedAbove?: boolean;
  selectedBelow?: boolean;
  /** Starts playback of the song (the page queues the other visible rows after it). Default: streams the album's missing songs. */
  onPlay?: () => void;
  onSelect?: (e: React.MouseEvent) => void;
  /** Right click: the page updates the selection; true when it opened its own (multi-selection) menu. */
  onMenu?: (e: React.MouseEvent) => boolean;
}

/**
 * A song of a library album that the library does not have yet: a dimmed row after the album's songs. Play
 * streams a preview, the download button starts the album download of this song, and the right-click menu is
 * the shared remote-track menu.
 */
const MissingTrackRow = ({
  album,
  missing,
  track,
  percent = 0,
  downloading = false,
  ghost = false,
  unavailable = false,
  failure,
  trashId,
  selected = false,
  selectedAbove = false,
  selectedBelow = false,
  onPlay,
  onSelect,
  onMenu,
}: MissingTrackRowProps) => {
  const { openMenu } = useRemoteTrackMenu();
  const openModal = useModalStore((s) => s.openModal);
  const downloadOne = useMissingTracksStore((s) => s.downloadOne);
  const retryOne = useMissingTracksStore((s) => s.retry);
  // The song's album download is running or queued: a second one would download it twice.
  const pending = useMissingTracksStore((s) => s.pendingUrls.has(track.url));
  const busy = pending || downloading;
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);

  const queue = useMemo(() => missingRemoteTracks(missing), [missing]);
  const remote = useMemo(() => missingRemoteTrack(missing, track), [missing, track]);
  const isCurrent = isStreamingUrl(currentSong, isStreaming, remote.url);
  const playing = isCurrent && isPlaying;
  const title = decodeHtmlEntities(track.title);

  const play = () => {
    if (onPlay) return onPlay();
    void useStreamStore.getState().play(remoteTrackToStreamItem(remote), missingStreamItems(missing));
  };

  return (
    <li
      className={clsx(styles.row, {
        [styles.ghost]: ghost,
        [styles.unavailable]: unavailable,
        [styles.playing]: isCurrent,
        [styles.selected]: selected,
        [styles.selectedTop]: selected && !selectedAbove,
        [styles.selectedBottom]: selected && !selectedBelow,
      })}
      data-song-row
      aria-selected={selected}
      onClick={(e) => onSelect?.(e)}
      onDoubleClick={play}
      onContextMenu={(e) => {
        if (onMenu?.(e)) return;
        openMenu(e, remote, {
          list: queue,
          downloadState: "none",
          extraItems:
            trashId === undefined ? undefined : [{ label: "Restore from trash", action: () => void restoreTrash([trashId]) }],
        });
      }}
    >
      <div className={styles.left}>
        <div className={clsx(styles.numSlot, isCurrent && styles.numSlotCurrent)}>
          {/* Hidden on hover: the play button takes its place. */}
          <div className={styles.number}>
            {isCurrent ? (
              playing ? (
                <HiOutlineMusicNote className={styles.playingIcon} />
              ) : (
                <HiMiniPause className={styles.pauseIcon} />
              )
            ) : (
              missingTrackNumber(track)
            )}
          </div>
          <button
            type="button"
            className={styles.playButton}
            title={playing ? "Pause" : "Play preview"}
            aria-label={playing ? "Pause" : "Play preview"}
            onClick={(e) => {
              e.stopPropagation();
              play();
            }}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            {playing ? <MdPause size={22} /> : <MdPlayArrow size={22} />}
          </button>
        </div>
        <div className={clsx(styles.title, "truncate")}>{title}</div>
      </div>
      <div className={styles.right}>
        {!busy && !unavailable && failure === undefined && (
          <span className={styles.duration}>{formatTrackDuration(track.duration_ms)}</span>
        )}
        {(unavailable || failure !== undefined) && !busy ? (
          <button
            type="button"
            className={styles.errorButton}
            title={`${explainFailure(failure ?? "no output file", platformLabel(missing.platform)).reason} Click for options.`}
            aria-label="Song unavailable: find another source"
            onClick={(e) => {
              e.stopPropagation();
              openModal(
                <FindMissingModal
                  albumArtist={missing.album_artist}
                  album={album}
                  excludeUrl={missing.source_url}
                  failedSong={{
                    artist: missing.album_artist,
                    title,
                    platform: platformLabel(missing.platform),
                    trackNumber: track.track_number ?? null,
                    message: failure,
                    retry: () => void retryOne(album, track),
                  }}
                />,
              );
            }}
          >
            <MdErrorOutline size={18} />
          </button>
        ) : busy ? (
          <span
            className={clsx(
              styles.percent,
              percent >= 100 ? styles.percentCompleted : percent > 0 ? styles.percentActive : styles.percentPending,
            )}
            aria-live="polite"
          >
            {percent}%
          </span>
        ) : (
          <button
            type="button"
            className={styles.downloadButton}
            title="Download"
            aria-label="Download song"
            onClick={(e) => {
              e.stopPropagation();
              void downloadOne(album, track);
            }}
          >
            <BiDownload size={18} />
          </button>
        )}
      </div>
    </li>
  );
};

export default MissingTrackRow;
