import { useEffect, useMemo } from "react";
import clsx from "clsx";
import { BiArrowBack, BiDownload, BiRightArrowAlt } from "react-icons/bi";
import { FaCheckCircle } from "react-icons/fa";
import placeholderAlbumImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import artistStyles from "@/pages/artist-albums/artist-albums.module.css";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { StreamActionRow } from "@/features/preview/ui/StreamActionRow.tsx";
import { useStreamStore } from "@/features/preview/model/streamStore.ts";
import { useTrackStatuses } from "@/shared/hooks/useDownloadStatus.ts";
import { useRemoteTrackMenu } from "@/shared/hooks/useRemoteTrackMenu.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { getLibraryAlbum } from "@/shared/lib/libraryIndex.ts";
import {
  albumAction,
  albumLibraryState,
  albumMetaLine,
  selectableIndexes,
  selectableTracks,
  trackAsStreamItem,
  trackRefs,
  trackTaskStates,
  type AlbumTrackInfo,
} from "@/shared/lib/albumPreview.ts";
import { albumTrackToRemoteTrack, type RemoteTrack } from "@/shared/lib/remoteTrack.ts";
import { EMPTY_SELECTION } from "@/shared/lib/selection.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import { isDownloadBusy } from "@/pages/search-results/searchResultsFormat.ts";
import { AlbumTrackRow } from "./AlbumTrackRow.tsx";
import styles from "./album-preview.module.css";

const AlbumPreview = () => {
  const { item, status, preview, error, selection, trackTasks } = useAlbumPreviewStore();
  const { close, reload, selectTrack, selectAll, clearSelection, download, downloadUrls, downloadTrack } =
    useAlbumPreviewStore.getState();
  const activeDownloads = useLibraryStore((s) => s.activeDownloads);
  const playStream = useStreamStore((s) => s.play);
  const { openMenu } = useRemoteTrackMenu();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") clearSelection();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [clearSelection]);

  const selectable = useMemo(() => (preview ? selectableIndexes(preview) : []), [preview]);
  const selectedCount = selection.selected.size;

  // Library state of every selectable track, and the download tasks that serve rows of this page.
  const refs = useMemo(() => (preview ? trackRefs(preview) : []), [preview]);
  const libraryStates = useTrackStatuses(refs);
  const taskStates = useMemo(
    () => trackTaskStates(trackTasks, activeDownloads),
    [trackTasks, activeDownloads],
  );
  const albumState = useMemo(
    () => (preview ? albumLibraryState(preview, libraryStates) : "none"),
    [preview, libraryStates],
  );

  const action = useMemo(
    () =>
      preview ? albumAction(preview, libraryStates, selectedCount) : null,
    [preview, libraryStates, selectedCount],
  );

  // Opens the library page of the album: the artist's albums (the album was saved under the artist it came from).
  const goToAlbum = async () => {
    if (!preview) return;
    const library = await getLibraryAlbum(preview.url).catch(() => null);
    void useLibraryStore.getState().setSelectedTab(library?.album_artist ?? preview.artist);
  };

  // Stream and menu lists: the downloadable tracks in album order.
  const streamItems = useMemo(
    () => (preview ? selectableTracks(preview).map((t) => trackAsStreamItem(preview, t)) : []),
    [preview],
  );
  const remoteList: RemoteTrack[] = useMemo(
    () => (preview ? selectableTracks(preview).map((t) => albumTrackToRemoteTrack(preview, t)) : []),
    [preview],
  );

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
    void playStream(trackAsStreamItem(preview, track), streamItems);
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
            {preview && <DownloadStatusBadge state={albumState} size={18} className={styles.titleBadge} />}
            {preview && albumMetaLine(preview) && (
              <span className={clsx(artistStyles.albumMeta, "truncate")}>{albumMetaLine(preview)}</span>
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
                    if (action?.kind === "go-to-album") void goToAlbum();
                    else if (action?.kind === "download-missing") void downloadUrls(action.urls);
                    else clearSelectionAfter(download);
                  }}
                >
                  {running ? (
                    `Downloading ${running.overallPercent}%`
                  ) : (
                    <>
                      {action?.kind === "go-to-album" ? <BiRightArrowAlt size={20} /> : <BiDownload size={18} />}
                      {action?.label}
                    </>
                  )}
                </button>
                {justCompleted && !running && action?.kind !== "go-to-album" && (
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
                <StreamActionRow items={streamItems} playLabel="Play album" />
                {selectedCount > 0 && <span className={styles.count}>{selectedCount} selected</span>}
              </div>

              <ul className={artistStyles.songs}>
                {preview.tracks.map((track, i) => {
                  const above = preview.tracks[i - 1];
                  const below = preview.tracks[i + 1];
                  return (
                    <AlbumTrackRow
                      key={track.index}
                      track={track}
                      position={i}
                      selected={selection.selected.has(track.index)}
                      selectedAbove={!!above && selection.selected.has(above.index)}
                      selectedBelow={!!below && selection.selected.has(below.index)}
                      libraryState={libraryStates[track.url]}
                      task={taskStates[track.url]}
                      cover={preview.cover_url}
                      onSelect={(e) =>
                        selectTrack(track.index, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })
                      }
                      onPlay={() => playTrack(track)}
                      onDownload={() => void downloadTrack(track)}
                      onContextMenu={(e) =>
                        openMenu(e, albumTrackToRemoteTrack(preview, track), {
                          list: remoteList,
                          downloadState: libraryStates[track.url] ?? "none",
                        })
                      }
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
