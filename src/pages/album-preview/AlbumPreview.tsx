import { useEffect, useMemo } from "react";
import clsx from "clsx";
import { BiArrowBack, BiDownload } from "react-icons/bi";
import { FaCheckCircle } from "react-icons/fa";
import { MdPause, MdPlayArrow } from "react-icons/md";
import placeholderAlbumImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import artistStyles from "@/pages/artist-albums/artist-albums.module.css";
import rowStyles from "@/features/library/ui/song-list-item/song-list-item.module.css";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { isStreamingUrl, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import {
  albumInfoLine,
  downloadLabel,
  selectableIndexes,
  trackAsStreamItem,
  trackCountLabel,
  type AlbumTrackInfo,
} from "@/shared/lib/albumPreview.ts";
import { EMPTY_SELECTION } from "@/shared/lib/selection.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import { isDownloadBusy } from "@/pages/search-results/searchResultsFormat.ts";
import styles from "./album-preview.module.css";

const TrackRow = ({
  track,
  position,
  selected,
  selectedAbove,
  selectedBelow,
  onSelect,
  onPlay,
}: {
  track: AlbumTrackInfo;
  position: number;
  selected: boolean;
  selectedAbove: boolean;
  selectedBelow: boolean;
  onSelect: (e: React.MouseEvent) => void;
  onPlay: () => void;
}) => {
  const currentSong = useAudioStore((s) => s.currentSong);
  const isStreaming = useAudioStore((s) => s.isStreaming);
  const isPlaying = useAudioStore((s) => s.isPlaying);
  const streamLoading = useAudioStore((s) => s.streamLoading);
  const loadingUrl = useStreamStore((s) => s.pendingUrl);
  const active = track.available && isStreamingUrl(currentSong, isStreaming, track.url);
  const playing = active && isPlaying && !streamLoading;
  const loading = track.available && (loadingUrl === track.url || (active && streamLoading));

  return (
    <div
      className={clsx(rowStyles.song, {
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
    >
      <div className={rowStyles.songLeft}>
        <div className={rowStyles.numSlot}>
          <div className={rowStyles.numContent}>
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
      </div>
    </div>
  );
};

const AlbumPreview = () => {
  const { item, status, preview, error, selection } = useAlbumPreviewStore();
  const { close, reload, selectTrack, selectAll, clearSelection, download } =
    useAlbumPreviewStore.getState();
  const activeDownloads = useLibraryStore((s) => s.activeDownloads);
  const playStream = useStreamStore((s) => s.play);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") clearSelection();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [clearSelection]);

  const selectable = useMemo(() => (preview ? selectableIndexes(preview) : []), [preview]);
  const selectedCount = selection.selected.size;

  // The album download that is running for this page (any selection of it).
  const running = useMemo(
    () =>
      preview
        ? Object.values(activeDownloads).find(
            (d) => d.url === preview.url && isDownloadBusy(d) && d.status !== "completed",
          )
        : undefined,
    [activeDownloads, preview],
  );
  const justCompleted = useMemo(
    () =>
      preview
        ? Object.values(activeDownloads).some(
            (d) => d.url === preview.url && d.status === "completed",
          )
        : false,
    [activeDownloads, preview],
  );

  const title = preview?.title ?? item?.name ?? "";
  const artist = preview?.artist ?? item?.band_name ?? "";
  const cover = preview?.cover_url ?? (item?.img || null);

  const playTrack = (track: AlbumTrackInfo) => {
    if (!preview) return;
    const list = preview.tracks.filter((t) => t.available).map((t) => trackAsStreamItem(preview, t));
    void playStream(trackAsStreamItem(preview, track), list);
  };

  return (
    <div
      className={artistStyles.container}
      onClick={(e) => {
        // A click outside the track rows clears the selection.
        if (!(e.target as HTMLElement).closest("[data-song-row], button")) clearSelection();
      }}
    >
      <div className={styles.topBar}>
        <button type="button" className={styles.backButton} onClick={close}>
          <BiArrowBack size={18} />
          Back to results
        </button>
      </div>
      <h1 className={clsx(artistStyles.artistName, "truncate")}>{artist}</h1>

      <section className={artistStyles.albumBlock}>
        <div className={artistStyles.albumBlockLeft}>
          <img
            src={cover ?? placeholderAlbumImage}
            referrerPolicy="no-referrer"
            onError={(e) => {
              if (e.currentTarget.src !== placeholderAlbumImage) {
                e.currentTarget.src = placeholderAlbumImage;
              }
            }}
            className={artistStyles.playlistImage}
            alt={title}
            draggable={false}
          />
        </div>

        <div className={artistStyles.albumBlockRight}>
          <div className={artistStyles.albumTitleRow}>
            <h2 className={clsx(artistStyles.albumTitle, "truncate")}>{decodeHtmlEntities(title)}</h2>
            {preview && albumInfoLine(preview) && (
              <span className={clsx(artistStyles.albumMeta, "truncate")}>{albumInfoLine(preview)}</span>
            )}
          </div>

          {status === "loading" && <p className={styles.state}>Loading tracks...</p>}

          {status === "error" && (
            <div className={styles.state}>
              <p className={styles.error}>{error ?? "Could not load the album."}</p>
              <button type="button" className={styles.secondaryButton} onClick={() => void reload()}>
                Retry
              </button>
            </div>
          )}

          {status === "ready" && preview && (
            <>
              <div className={styles.toolbar}>
                <button
                  type="button"
                  className={styles.primaryButton}
                  disabled={selectable.length === 0 || running !== undefined}
                  onClick={() => {
                    clearSelectionAfter(download);
                  }}
                >
                  {running ? (
                    `Downloading ${running.overallPercent}%`
                  ) : (
                    <>
                      <BiDownload size={18} />
                      {downloadLabel(selectedCount, selectable.length)}
                    </>
                  )}
                </button>
                {justCompleted && !running && (
                  <span className={styles.done}>
                    <FaCheckCircle size={14} /> Downloaded
                  </span>
                )}
                <button
                  type="button"
                  className={styles.secondaryButton}
                  disabled={selectable.length === 0}
                  onClick={selectedCount >= selectable.length ? clearSelection : selectAll}
                >
                  {selectedCount >= selectable.length && selectable.length > 0 ? "Clear selection" : "Select all"}
                </button>
                <span className={styles.count}>
                  {trackCountLabel(preview)}
                  {selectedCount > 0 && ` · ${selectedCount} selected`}
                </span>
              </div>

              <ul className={artistStyles.songs}>
                {preview.tracks.map((track, i) => {
                  const above = preview.tracks[i - 1];
                  const below = preview.tracks[i + 1];
                  return (
                    <TrackRow
                      key={track.index}
                      track={track}
                      position={i}
                      selected={selection.selected.has(track.index)}
                      selectedAbove={!!above && selection.selected.has(above.index)}
                      selectedBelow={!!below && selection.selected.has(below.index)}
                      onSelect={(e) =>
                        selectTrack(track.index, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })
                      }
                      onPlay={() => playTrack(track)}
                    />
                  );
                })}
              </ul>
              <p className={styles.hint}>
                Click a track to select it, Ctrl/Shift for several. Double click plays a preview.
              </p>
            </>
          )}
        </div>
      </section>
    </div>
  );
};

/** Starts the download and clears the selection: the queue shows what is downloading from here on. */
function clearSelectionAfter(download: () => Promise<void>) {
  void download().finally(() => useAlbumPreviewStore.setState({ selection: EMPTY_SELECTION }));
}

export default AlbumPreview;
