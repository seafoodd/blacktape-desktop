import { useMemo } from "react";
import clsx from "clsx";
import { MdFileDownload, MdHourglassEmpty, MdPause, MdPlayArrow } from "react-icons/md";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { MissingTracks } from "@/shared/lib/libraryIndex.ts";
import { formatTrackDuration } from "@/shared/lib/trackDuration.ts";
import { remoteTrackToStreamItem } from "@/shared/lib/remoteTrack.ts";
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
}

/**
 * A song of a library album that the library does not have yet: a dimmed row after the album's songs. Play
 * streams a preview, the download button starts the album download of this song, and the right-click menu is
 * the shared remote-track menu.
 */
const MissingTrackRow = ({ album, missing, track }: MissingTrackRowProps) => {
  const { openMenu } = useRemoteTrackMenu();
  const downloadOne = useMissingTracksStore((s) => s.downloadOne);
  // The song's album download is running or queued: a second one would download it twice.
  const busy = useMissingTracksStore((s) => s.pendingUrls.has(track.url));
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);

  const queue = useMemo(() => missingRemoteTracks(missing), [missing]);
  const remote = useMemo(() => missingRemoteTrack(missing, track), [missing, track]);
  const isCurrent = isStreamingUrl(currentSong, isStreaming, remote.url);
  const playing = isCurrent && isPlaying;
  const title = decodeHtmlEntities(track.title);

  const play = () => {
    void useStreamStore.getState().play(remoteTrackToStreamItem(remote), missingStreamItems(missing));
  };

  return (
    <li
      className={styles.row}
      onDoubleClick={play}
      onContextMenu={(e) => openMenu(e, remote, { list: queue, downloadState: "none" })}
    >
      <div className={styles.left}>
        <div className={styles.numSlot}>
          {/* Hidden on hover: the play button takes its place. */}
          <div className={styles.number}>{missingTrackNumber(track)}</div>
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
        <span className={styles.duration}>{formatTrackDuration(track.duration_ms)}</span>
        <button
          type="button"
          className={clsx(styles.downloadButton, busy && styles.busy)}
          title={busy ? "Downloading" : "Download"}
          aria-label={busy ? "Downloading song" : "Download song"}
          disabled={busy}
          onClick={(e) => {
            e.stopPropagation();
            void downloadOne(album, track);
          }}
        >
          {busy ? <MdHourglassEmpty size={16} /> : <MdFileDownload size={18} />}
        </button>
      </div>
    </li>
  );
};

export default MissingTrackRow;
