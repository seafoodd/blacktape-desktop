import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { CancelDownloadButton } from "@/shared/ui/cancel-download-button/CancelDownloadButton.tsx";
import { torrentStatusMessage } from "@/features/library/model/downloadStatusMessage.ts";
import { coverSrc } from "@/shared/lib/remote/cover";
import { Fragment, useEffect, useMemo, useRef, useState } from "react";
import clsx from "clsx";
import { BiArrowBack, BiDownload, BiRightArrowAlt, BiUpArrowAlt } from "react-icons/bi";
import { FaCheckCircle } from "react-icons/fa";
import { FiCheck } from "react-icons/fi";
import { MdKeyboardArrowDown, MdSettings } from "react-icons/md";
import placeholderAlbumImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import artistStyles from "@/pages/artist-albums/artist-albums.module.css";
import { buildDownloadOverrides, useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { StreamActionRow } from "@/features/preview/ui/StreamActionRow.tsx";
import { toStreamRequest, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { useTrackStatuses } from "@/shared/hooks/useDownloadStatus.ts";
import { useRemoteTrackMenu } from "@/shared/hooks/useRemoteTrackMenu.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { ItemType, useLibraryStore, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import { mergeTorrentResolutionsIntoTracks } from "@/features/download-quality/lib/downloadQuality.ts";
import { useTorrentDiscovery } from "@/features/download-quality/lib/useTorrentDiscovery.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import {
  getLibraryAlbum,
  getLibraryTrackDetails,
  type LibraryTrackDetails,
} from "@/shared/lib/library/libraryIndex.ts";
import { AddToQueue } from "@/shared/lib/audio.ts";
import { startMixedPlayback, streamAddToQueue, type MixedEntry } from "@/shared/lib/remote/stream.ts";
import { sameUrl } from "@/shared/lib/remote/urlKey.ts";
import {
  albumAction,
  albumLibraryState,
  albumMetadataSourceBadge,
  albumMetaLine,
  estimatedPlatformQuality,
  formatEditionSubtext,
  hasMetadataChanges,
  partitionEditions,
  selectableIndexes,
  selectableTracks,
  selectedUrls,
  trackAsStreamItem,
  trackRefs,
  trackTaskStates,
  unverifiedTracksForDownload,
  upgradableAlbumTracks,
  type AlbumPreview as AlbumPreviewData,
  type AlbumTrackInfo,
} from "@/shared/lib/remote/albumPreview.ts";
import { albumTrackToRemoteTrack, type RemoteTrack } from "@/shared/lib/remote/remoteTrack.ts";
import { EMPTY_SELECTION } from "@/shared/lib/ui/selection.ts";
import { decodeHtmlEntities } from "@/shared/lib/ui/html.ts";
import { isDownloadBusy } from "@/pages/search-results/searchResultsFormat.ts";
import { AlbumTrackRow } from "./AlbumTrackRow.tsx";
import { AlbumQualityPicker } from "@/pages/album-preview/AlbumQualityPicker.tsx";
import { AlbumDiscoveryStatus } from "@/pages/album-preview/AlbumDiscoveryStatus.tsx";
import { albumSourceOptions, applyAlbumSource } from "@/pages/album-preview/albumSourceSelection.ts";
import styles from "./album-preview.module.css";

const AlbumPreview = () => {
  const {
    item,
    status,
    preview,
    originalMetadata,
    isEditingMetadata,
    error,
    selection,
    trackTasks,
    resolvingCandidates,
    editions,
    loadingEditions,
    switchingRelease,
  } = useAlbumPreviewStore();
  const {
    close,
    reload,
    selectRelease,
    setEditingMetadata,
    updatePreviewMetadata,
    undoMetadataChanges,
    selectTrack,
    selectAll,
    clearSelection,
    download,
    downloadUrls,
    downloadTrack,
  } = useAlbumPreviewStore.getState();
  const activeDownloads = useDownloadStore((s) => s.activeDownloads);
  const dismissDownload = useDownloadStore((s) => s.dismissDownload);
  const albums = useLibraryStore((s) => s.albums);
  const playStream = useStreamStore((s) => s.play);
  const { openMenu } = useRemoteTrackMenu();
  const [localTracks, setLocalTracks] = useState<Record<string, LibraryTrackDetails>>({});
  const [activeUpgradeUrl, setActiveUpgradeUrl] = useState<string | null>(null);
  const [editionsOpen, setEditionsOpen] = useState(false);
  const [showAllEditions, setShowAllEditions] = useState(false);
  const editionRef = useRef<HTMLDivElement>(null);
  const torrentScope = status === "ready" && !switchingRelease && !isEditingMetadata &&
    preview?.artist.trim() && preview.title.trim() && preview.tracks.length > 0
    ? {
        artist: preview.artist,
        title: preview.title,
        expected: preview.tracks.map((track) => ({
          index: track.index,
          title: track.title,
          artist: track.artists[0] ?? preview.artist,
          disc_number: track.disc_number ?? null,
          track_number: track.track_number ?? null,
          duration_ms: track.duration_ms ?? null,
        })),
      }
    : null;
  const torrentDiscovery = useTorrentDiscovery(preview?.url, torrentScope);
  const sourceOptions = useMemo(() => albumSourceOptions(preview?.tracks ?? []), [preview?.tracks]);
  useEffect(() => {
    if (!preview || !torrentDiscovery.resolutions?.length) return;
    const tracks = mergeTorrentResolutionsIntoTracks(preview.tracks, torrentDiscovery.resolutions);
    if (tracks.every((track, index) => track === preview.tracks[index])) return;
    useAlbumPreviewStore.setState((state) => {
      if (state.preview !== preview) return state;
      return { preview: { ...preview, tracks } };
    });
  }, [preview, torrentDiscovery]);

  useEffect(() => {
    setEditionsOpen(false);
    setShowAllEditions(false);
    setActiveUpgradeUrl(null);
  }, [preview?.url]);

  useEffect(() => {
    if (!editionsOpen) return;
    const onPointerDown = (e: MouseEvent | PointerEvent) => {
      if (editionRef.current && !editionRef.current.contains(e.target as Node)) {
        setEditionsOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setEditionsOpen(false);
    };
    document.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [editionsOpen]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !useModalStore.getState().isOpen) clearSelection();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [clearSelection]);

  const selectable = useMemo(() => (preview ? selectableIndexes(preview) : []), [preview]);
  const selectedCount = selection.selected.size;
  const uniqueDiscs = useMemo(
    () => (preview ? new Set(preview.tracks.map((t) => t.disc_number ?? 1)).size : 1),
    [preview],
  );
  const metadataBadge = useMemo(
    () => (preview ? albumMetadataSourceBadge(preview) : null),
    [preview],
  );
  const metadataDirty = useMemo(
    () => hasMetadataChanges(preview, originalMetadata),
    [preview, originalMetadata],
  );

  // Library state of every selectable track, and the download tasks that serve rows of this page.
  const refs = useMemo(() => (preview ? trackRefs(preview) : []), [preview]);
  const libraryStates = useTrackStatuses(refs);
  const downloadedSignature = useMemo(
    () => refs.map((r) => `${r.url}:${libraryStates[r.url] ?? "none"}`).join("|"),
    [refs, libraryStates],
  );
  useEffect(() => {
    if (refs.length === 0) {
      setLocalTracks({});
      return;
    }
    let cancelled = false;
    getLibraryTrackDetails(refs)
      .then((details) => {
        if (!cancelled) setLocalTracks(details);
      })
      .catch(() => {
        if (!cancelled) setLocalTracks({});
      });
    return () => {
      cancelled = true;
    };
  }, [refs, downloadedSignature, albums]);

  const localTrackIds = useMemo(() => {
    const out: Record<string, number> = {};
    for (const [url, details] of Object.entries(localTracks)) {
      out[url] = details.id;
    }
    return out;
  }, [localTracks]);

  const upgradableTracks = useMemo(
    () => (preview ? upgradableAlbumTracks(preview, localTracks) : []),
    [preview, localTracks],
  );
  const upgradeUrl = useMemo(() => {
    if (upgradableTracks.length === 0) return null;
    return upgradableTracks.length === 1
      ? `resolve:song:${upgradableTracks[0].local.id}`
      : `resolve:batch:${upgradableTracks.map((u) => u.local.id).join(",")}`;
  }, [upgradableTracks]);
  const runningUpgrade = useMemo(() => {
    const matchIds = new Set(upgradableTracks.map((u) => u.local.id));
    return Object.values(activeDownloads).find((d) => {
      if (!isDownloadBusy(d) || d.status === "completed") return false;
      if (activeUpgradeUrl && d.url === activeUpgradeUrl) return true;
      if (upgradeUrl && d.url === upgradeUrl) return true;
      if (d.url.startsWith("resolve:song:")) {
        return matchIds.has(Number(d.url.slice("resolve:song:".length)));
      }
      if (d.url.startsWith("resolve:batch:")) {
        return d.url
          .slice("resolve:batch:".length)
          .split(",")
          .some((id) => matchIds.has(Number(id)));
      }
      return false;
    });
  }, [activeDownloads, activeUpgradeUrl, upgradeUrl, upgradableTracks]);

  const startUpgrade = (items: typeof upgradableTracks) => {
    if (!preview || items.length === 0) return;
    const isSingle = items.length === 1;
    const targetUrl = isSingle
      ? `resolve:song:${items[0].local.id}`
      : `resolve:batch:${items.map((u) => u.local.id).join(",")}`;
    setActiveUpgradeUrl(targetUrl);
    void useDownloadStore.getState().startDownload({
      platform: preview.platform,
      downloadType: isSingle ? ItemType.Track : ItemType.Album,
      url: targetUrl,
      artistName: preview.artist,
      title: isSingle ? items[0].track.title : preview.title,
      coverUrl: preview.cover_url ?? undefined,
      trackUrls: isSingle ? undefined : items.map((u) => `resolve:song:${u.local.id}`),
      replaceSongId: isSingle ? items[0].local.id : undefined,
      placement: isSingle
        ? {
            album: preview.title,
            album_artist: preview.artist,
            title: items[0].track.title,
            track_number: items[0].track.track_number ?? null,
          }
        : undefined,
      format: "best",
      overrides: buildDownloadOverrides({
        ...preview,
        tracks: items.map(({ track }, index) => ({ ...track, index })),
      }),
    });
  };

  const taskStates = useMemo(
    () => trackTaskStates(trackTasks, activeDownloads, preview?.tracks, preview?.url),
    [trackTasks, activeDownloads, preview],
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
  const playableTracks = useMemo(
    () => (preview ? selectableTracks(preview) : []),
    [preview],
  );
  const streamItems = useMemo(
    () => (preview ? playableTracks.map((t) => trackAsStreamItem(preview, t)) : []),
    [preview, playableTracks],
  );
  const mixedEntries: MixedEntry[] = useMemo(() => {
    if (!preview) return [];
    return playableTracks.map((t) => {
      const localId = localTrackIds[t.url];
      if (localId !== undefined) {
        return { kind: "song", id: localId };
      }
      return { kind: "stream", ...toStreamRequest(trackAsStreamItem(preview, t)) };
    });
  }, [preview, playableTracks, localTrackIds]);
  const hasAnyLocal = useMemo(
    () => mixedEntries.some((entry) => entry.kind === "song"),
    [mixedEntries],
  );

  const remoteList: RemoteTrack[] = useMemo(
    () => (preview ? playableTracks.map((t) => albumTrackToRemoteTrack(preview, t)) : []),
    [preview, playableTracks],
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
  const failedDownload = useMemo(() => {
    if (!preview || running) return undefined;
    const matches = Object.values(activeDownloads).filter((d) => d.url === preview.url);
    const latest = matches[matches.length - 1];
    return latest?.status === "failed" ? latest : undefined;
  }, [activeDownloads, preview, running]);

  const title = preview?.title ?? item?.name ?? "";
  const artist = preview?.artist ?? item?.band_name ?? "";
  const cover = preview?.cover_url ?? (item?.img || null);

  const playTrack = (track: AlbumTrackInfo) => {
    if (!preview) return;
    const audio = useAudioStore.getState();
    const localId = localTrackIds[track.url];
    const currentIsTrack =
      (localId !== undefined && audio.currentSong?.id === localId) ||
      (!!audio.currentSong?.source_url && sameUrl(audio.currentSong.source_url, track.url));
    if (currentIsTrack) {
      void audio.togglePlay();
      return;
    }
    const idx = playableTracks.findIndex((t) => t.index === track.index);
    if (hasAnyLocal && idx >= 0 && mixedEntries.length > 0) {
      void startMixedPlayback(mixedEntries, idx);
      return;
    }
    void playStream(trackAsStreamItem(preview, track), streamItems);
  };

  const playAllMixed = () => {
    if (!preview || mixedEntries.length === 0) return;
    if (hasAnyLocal) {
      void startMixedPlayback(mixedEntries, 0);
    } else {
      void useStreamStore.getState().playAll(streamItems);
    }
  };

  const shuffleMixed = () => {
    if (!preview || mixedEntries.length === 0) return;
    if (hasAnyLocal) {
      const startIdx = Math.floor(Math.random() * mixedEntries.length);
      if (!useAudioStore.getState().shuffleMode) {
        void useAudioStore.getState().toggleShuffle();
      }
      void startMixedPlayback(mixedEntries, startIdx);
    } else {
      void useStreamStore.getState().playAll(streamItems, { shuffle: true });
    }
  };

  const enqueueMixed = async () => {
    if (!preview || mixedEntries.length === 0) return;
    if (!hasAnyLocal) {
      await useStreamStore.getState().enqueue(streamItems);
      return;
    }
    for (const entry of mixedEntries) {
      if (entry.kind === "song") {
        await AddToQueue(entry.id);
      } else {
        await streamAddToQueue([entry]);
      }
    }
  };

  const renderSkeletonRows = (count = 8) => (
    <div className={styles.skeletonTrackList} aria-hidden="true">
      {Array.from({ length: count }).map((_, idx) => (
        <div key={idx} className={styles.skeletonTrackRow}>
          <div className={clsx(styles.skeletonPulse, styles.skeletonTrackNum)} />
          <div
            className={clsx(styles.skeletonPulse, styles.skeletonTrackTitle)}
            style={{ maxWidth: `${180 + ((idx * 47) % 140)}px` }}
          />
          <div className={clsx(styles.skeletonPulse, styles.skeletonTrackDur)} />
        </div>
      ))}
    </div>
  );

  if (status === "loading") {
    return (
      <div className={artistStyles.container} aria-busy="true" aria-label="Loading album preview">
        <div className={styles.topBar}>
          <button type="button" className={styles.backButton} onClick={close}>
            <BiArrowBack size={18} />
            Back to results
          </button>
        </div>
        <div className={clsx(styles.skeletonPulse, styles.skeletonArtist)} />
        <section className={artistStyles.albumBlock}>
          <div className={artistStyles.albumBlockLeft}>
            <div className={clsx(styles.skeletonPulse, styles.skeletonCover)} />
          </div>
          <div className={artistStyles.albumBlockRight}>
            <div className={styles.skeletonTitleRow}>
              <div className={clsx(styles.skeletonPulse, styles.skeletonTitleBar)} />
              <div className={clsx(styles.skeletonPulse, styles.skeletonMetaBar)} />
            </div>
            <div className={styles.skeletonToolbar}>
              <div className={clsx(styles.skeletonPulse, styles.skeletonBtnPrimary)} />
              <div className={clsx(styles.skeletonPulse, styles.skeletonBtnSecondary)} />
              <div className={clsx(styles.skeletonPulse, styles.skeletonBtnSecondary)} />
            </div>
            {renderSkeletonRows(8)}
          </div>
        </section>
      </div>
    );
  }

  return (
    <div
      className={artistStyles.container}
      onClick={(e) => {
        // A click outside the track rows clears the selection.
        if (!(e.target as HTMLElement).closest("[data-song-row], button, input")) clearSelection();
      }}
    >
      <div className={styles.topBar}>
        <button type="button" className={styles.backButton} onClick={close}>
          <BiArrowBack size={18} />
          Back to results
        </button>
      </div>
      <div className={styles.artistHeader}>
        {isEditingMetadata && preview ? (
          <input
            type="text"
            className={styles.inlineArtistInput}
            value={preview.artist}
            onChange={(e) => updatePreviewMetadata({ artist: e.target.value })}
            onKeyDown={(e) => {
              if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
            }}
            aria-label="Album artist"
          />
        ) : (
          <h1 className={clsx(styles.artistName, "truncate")}>{artist}</h1>
        )}
        {status === "ready" && preview && (
          <AlbumDiscoveryStatus
            loadingEditions={loadingEditions}
            switchingRelease={switchingRelease}
            resolvingCandidates={resolvingCandidates}
            tracks={preview.tracks}
            torrent={torrentDiscovery}
          />
        )}
      </div>

      <section className={artistStyles.albumBlock}>
        <div className={artistStyles.albumBlockLeft}>
          <img
            key={cover ?? "placeholder"}
            src={cover ? coverSrc(cover, 200) : placeholderAlbumImage}
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
            {isEditingMetadata && preview ? (
              <input
                type="text"
                className={styles.inlineTitleInput}
                value={preview.title}
                onChange={(e) => updatePreviewMetadata({ title: e.target.value })}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
                }}
                aria-label="Album title"
              />
            ) : (
              <div className={styles.titleWithPicker}>
                <h2 className={clsx(artistStyles.albumTitle, "truncate")}>{decodeHtmlEntities(title)}</h2>
                {editions.length > 0 && (() => {
                  const activeEditionObj = editions.find((edition) =>
                    preview?.selected_release_id
                      ? edition.id === preview.selected_release_id
                      : edition.source_kind === preview?.metadata_source &&
                        Boolean(edition.url && item && sameUrl(edition.url, item.item_url_path)),
                  );
                  const { visibleMb, hiddenMbCount, platformEditions } = partitionEditions(
                    editions,
                    preview?.selected_release_id,
                    showAllEditions,
                  );
                  const chipCount = visibleMb.length + platformEditions.length;
                  const renderEditionItem = (edition: (typeof editions)[number]) => {
                    const activeEdition = edition.id === activeEditionObj?.id;
                    const platTag =
                      edition.source_kind === "youtube"
                        ? "YT"
                        : edition.source_kind === "bandcamp"
                          ? "BC"
                          : edition.source_kind === "soundcloud"
                            ? "SC"
                            : null;
                    return (
                      <button
                        key={edition.id}
                        type="button"
                        role="menuitemradio"
                        aria-checked={activeEdition}
                        className={clsx(
                          styles.editionMenuItem,
                          activeEdition && styles.editionMenuItemActive,
                        )}
                        onClick={() => {
                          setEditionsOpen(false);
                          void selectRelease(edition);
                        }}
                      >
                        <div className={styles.editionDetails}>
                          <span className={styles.editionLabelRow}>
                            {platTag && (
                              <span
                                className={clsx(
                                  styles.editionPlatTag,
                                  styles[`source_${edition.source_kind}`],
                                )}
                              >
                                {platTag}
                              </span>
                            )}
                            <span className={styles.editionLabel}>{edition.edition_label}</span>
                          </span>
                          <span className={styles.editionMeta}>
                            {formatEditionSubtext(edition) || edition.title}
                          </span>
                        </div>
                        {activeEdition && <FiCheck className={styles.candidateCheck} />}
                      </button>
                    );
                  };
                  return (
                    <div ref={editionRef} className={styles.editionPicker}>
                      <button
                        type="button"
                        className={clsx(
                          styles.editionToggleButton,
                          editionsOpen && styles.editionToggleButtonOpen,
                        )}
                        onClick={() => setEditionsOpen((v) => !v)}
                        disabled={switchingRelease}
                        title={`Switch album version (${editions.length} total available)`}
                        aria-label="Choose album edition or platform release"
                        aria-haspopup="menu"
                        aria-expanded={editionsOpen}
                      >
                        <span className={styles.editionChipText}>
                          {activeEditionObj?.edition_label ?? "Versions"}
                        </span>
                        <span className={styles.editionChipCount}>{chipCount}</span>
                        {switchingRelease ? (
                          <span className={styles.miniSpinner} aria-hidden="true" />
                        ) : (
                          <MdKeyboardArrowDown
                            size={16}
                            className={clsx(styles.editionArrow, editionsOpen && styles.arrowOpen)}
                          />
                        )}
                      </button>
                      {editionsOpen && (
                        <div className={styles.editionMenu} role="menu" aria-label="Album editions">
                          <div className={styles.editionMenuList}>
                            {visibleMb.length > 0 && (
                              <>
                                <div className={styles.editionMenuHeader}>
                                  MusicBrainz Releases ({visibleMb.length})
                                </div>
                                {visibleMb.map(renderEditionItem)}
                                {(hiddenMbCount > 0 || showAllEditions) && (
                                  <button
                                    type="button"
                                    className={styles.editionShowAllBtn}
                                    onClick={() => setShowAllEditions((v) => !v)}
                                  >
                                    {showAllEditions
                                      ? "Show fewer releases"
                                      : `Show all releases (+${hiddenMbCount})`}
                                  </button>
                                )}
                              </>
                            )}
                            {platformEditions.length > 0 && (
                              <>
                                <div className={styles.editionMenuHeader}>
                                  Other Platforms ({platformEditions.length})
                                </div>
                                {platformEditions.map(renderEditionItem)}
                              </>
                            )}
                          </div>
                        </div>
                      )}
                    </div>
                  );
                })()}
              </div>
            )}
            {metadataBadge && (
              <span
                className={clsx(styles.metadataBadge, styles[`source_${metadataBadge.kind}`])}
                title={`Album metadata from ${metadataBadge.label}`}
              >
                {metadataBadge.label}
              </span>
            )}
            {preview && <DownloadStatusBadge state={albumState} size={18} className={styles.titleBadge} />}
            {isEditingMetadata && preview ? (
              <input
                type="number"
                className={styles.inlineYearInput}
                value={preview.year ?? ""}
                placeholder="Year"
                onChange={(e) => {
                  const val = e.target.value.trim();
                  const parsed = val ? parseInt(val, 10) : null;
                  updatePreviewMetadata({ year: parsed !== null && !Number.isNaN(parsed) ? parsed : null });
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter" || e.key === "Escape") e.currentTarget.blur();
                }}
                aria-label="Release year"
              />
            ) : (
              preview &&
              albumMetaLine(preview) && (
                <span className={clsx(artistStyles.albumMeta, "truncate")}>{albumMetaLine(preview)}</span>
              )
            )}
          </div>

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
              {error && <p className={styles.error} role="alert">{error}</p>}
              <div className={styles.toolbar}>
                <div className={styles.primarySplit}>
                  <button
                    type="button"
                    className={styles.primaryButton}
                    disabled={selectable.length === 0 || running !== undefined}
                    onClick={() => {
                      if (action?.kind === "go-to-album") void goToAlbum();
                      else if (action?.kind === "download-missing") {
                        confirmDownloadWithWarnings(
                          preview,
                          action.urls,
                          () => void downloadUrls(action.urls),
                          (verified) => void downloadUrls(verified),
                        );
                      } else {
                        const selUrls = selectedUrls(preview, selection.selected);
                        const scopeUrls =
                          selUrls.length > 0 && selUrls.length < selectable.length ? selUrls : undefined;
                        confirmDownloadWithWarnings(
                          preview,
                          scopeUrls,
                          () => clearSelectionAfter(download),
                          (verified) =>
                            clearSelectionAfter(() => downloadUrls(verified)),
                        );
                      }
                    }}
                  >
                    {running ? (
                      torrentStatusMessage(running.message) ?? (running.overallPercent > 0
                        ? `Downloading ${running.overallPercent}%`
                        : running.message || "Downloading 0%")
                    ) : (
                      <>
                        {action?.kind === "go-to-album" ? <BiRightArrowAlt size={20} /> : <BiDownload size={18} />}
                        {action?.label}
                      </>
                    )}
                  </button>
                  {action && action.kind !== "go-to-album" && !running && (
                    <button
                      type="button"
                      className={`${styles.primaryButton} ${styles.primaryArrow}`}
                      disabled={selectable.length === 0}
                      title="Choose the download quality"
                      aria-label="Choose the download quality"
                      aria-haspopup="dialog"
                      onClick={() => {
                        const urls =
                          action.kind === "download-missing" ? action.urls : selectedUrls(preview, selection.selected);
                        openQualityMenu(previewDownloadParams(preview, urls), {
                          start: (format) => {
                            const latestPreview = useAlbumPreviewStore.getState().preview ?? preview;
                            if (action.kind === "download-missing") {
                              confirmDownloadWithWarnings(
                                latestPreview,
                                action.urls,
                                () => void downloadUrls(action.urls, format),
                                (verified) => void downloadUrls(verified, format),
                              );
                            } else {
                              const scopeUrls =
                                urls.length > 0 && urls.length < selectable.length ? urls : undefined;
                              confirmDownloadWithWarnings(
                                latestPreview,
                                scopeUrls,
                                () => clearSelectionAfter(() => download(format)),
                                (verified) =>
                                  clearSelectionAfter(() => downloadUrls(verified, format)),
                              );
                            }
                          },
                        });
                      }}
                    >
                      <MdSettings size={16} />
                    </button>
                  )}
                </div>
                {running && <CancelDownloadButton taskId={running.id} />}
                {action?.kind === "go-to-album" && (upgradableTracks.length > 0 || runningUpgrade) && (
                  <button
                    type="button"
                    className={styles.secondaryButton}
                    disabled={runningUpgrade !== undefined}
                    onClick={() => {
                      const targetUrls = upgradableTracks.map((u) => u.track.url);
                      confirmDownloadWithWarnings(
                        preview,
                        targetUrls,
                        () => startUpgrade(upgradableTracks),
                        (verified) => {
                          const verifiedSet = new Set(verified);
                          startUpgrade(upgradableTracks.filter((u) => verifiedSet.has(u.track.url)));
                        },
                      );
                    }}
                  >
                    {runningUpgrade ? (
                      `Upgrading ${runningUpgrade.overallPercent}%`
                    ) : (
                      <>
                        <BiUpArrowAlt size={18} />
                        Upgrade audio quality
                      </>
                    )}
                  </button>
                )}
                {runningUpgrade && runningUpgrade.id !== running?.id && <CancelDownloadButton taskId={runningUpgrade.id} />}
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
                {!isEditingMetadata && (
                  <AlbumQualityPicker
                    key={preview.url}
                    options={sourceOptions}
                    trackCount={preview.tracks.length}
                    onSelect={(option) => useAlbumPreviewStore.setState((state) => {
                      if (state.preview?.url !== preview.url) return state;
                      return {
                        preview: { ...state.preview, tracks: applyAlbumSource(state.preview.tracks, option) },
                      };
                    })}
                  />
                )}
                <StreamActionRow
                  items={streamItems}
                  playLabel="Play album"
                  onPlayAll={playAllMixed}
                  onEnqueue={() => void enqueueMixed()}
                  onShuffle={shuffleMixed}
                />
                <button
                  type="button"
                  className={clsx(
                    styles.secondaryButton,
                    isEditingMetadata && styles.secondaryButtonActive,
                  )}
                  onClick={() => setEditingMetadata(!isEditingMetadata)}
                  aria-pressed={isEditingMetadata}
                  title="Edit album and track titles inline before downloading"
                >
                  {isEditingMetadata ? "Done" : "Edit"}
                </button>
                {metadataDirty && (
                  <button
                    type="button"
                    className={styles.secondaryButton}
                    onClick={undoMetadataChanges}
                    title="Restore original album and track titles"
                  >
                    Undo changes
                  </button>
                )}
                {selectedCount > 0 && <span className={styles.count}>{selectedCount} selected</span>}
              </div>

              {failedDownload && (
                <div className={styles.downloadErrorBanner} role="alert">
                  <span>{failedDownload.message || "Download failed."}</span>
                  <button
                    type="button"
                    className={styles.downloadErrorDismiss}
                    onClick={() => dismissDownload(failedDownload.id)}
                    aria-label="Dismiss error"
                  >
                    Dismiss
                  </button>
                </div>
              )}

              {switchingRelease ? (
                renderSkeletonRows(8)
              ) : (
                <>
                  <ul className={artistStyles.songs}>
                    {preview.tracks.map((track, i) => {
                      const above = preview.tracks[i - 1];
                      const below = preview.tracks[i + 1];
                      const isNewDisc =
                        uniqueDiscs >= 2 &&
                        track.disc_number &&
                        (!above || above.disc_number !== track.disc_number);
                      return (
                        <Fragment key={track.index}>
                          {isNewDisc && (
                            <li className={styles.discHeader}>
                              Disc {track.disc_number}
                            </li>
                          )}
                          <AlbumTrackRow
                            key={track.index}
                            track={track}
                            position={i}
                            selected={selection.selected.has(track.index)}
                            selectedAbove={!!above && selection.selected.has(above.index)}
                            selectedBelow={!!below && selection.selected.has(below.index)}
                            libraryState={libraryStates[track.url]}
                            localSongId={localTrackIds[track.url]}
                            editing={isEditingMetadata}
                            onChangeTitle={(val) =>
                              updatePreviewMetadata({ trackIndex: track.index, trackTitle: val })
                            }
                            task={taskStates[track.url]}
                            cover={preview.cover_url}
                            albumTitle={preview.title}
                            resolving={resolvingCandidates}
                            estimate={estimatedPlatformQuality(preview, track.url)}
                            onSelect={(e) =>
                              selectTrack(track.index, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey })
                            }
                            onPlay={() => playTrack(track)}
                            onDownload={() =>
                              confirmDownloadWithWarnings(preview, [track.url], () => void downloadTrack(track))
                            }
                            onChooseQuality={() =>
                              openQualityMenu(previewDownloadParams(preview, [track.url], track.title), {
                                start: (format) => {
                                  const latestPreview = useAlbumPreviewStore.getState().preview ?? preview;
                                  const latestTrack = latestPreview.tracks.find((t) => t.index === track.index) ?? track;
                                  confirmDownloadWithWarnings(
                                    latestPreview,
                                    [latestTrack.url],
                                    () => void downloadTrack(latestTrack, format),
                                  );
                                },
                              })
                            }
                            onSelectCandidate={(candIdx) => {
                              const cand = track.candidates?.[candIdx];
                              if (!cand) return;
                              const nextTracks = [...preview.tracks];
                              nextTracks[i] = {
                                ...track,
                                selected_candidate_idx: candIdx,
                                url: cand.source.kind === "http_stream" ? cand.source.url : track.url,
                              };
                              useAlbumPreviewStore.setState({
                                preview: { ...preview, tracks: nextTracks },
                              });
                            }}
                            onContextMenu={(e) =>
                              openMenu(e, albumTrackToRemoteTrack(preview, track), {
                                list: remoteList,
                                downloadState: libraryStates[track.url] ?? "none",
                              })
                            }
                          />
                        </Fragment>
                      );
                    })}
                  </ul>
                  <p className={styles.hint}>
                    Click a track to select it, Ctrl/Shift for several. Double click plays a preview.
                  </p>
                </>
              )}
            </>
          )}
        </div>
      </section>
    </div>
  );
};

/** What the quality menu probes and shows for a download of `urls` (all tracks when empty) of the album. */
function previewDownloadParams(preview: AlbumPreviewData, urls: string[], trackTitle?: string): StartDownloadParams {
  return {
    platform: preview.platform,
    downloadType: ItemType.Album,
    url: preview.url,
    artistName: preview.artist,
    title: trackTitle ?? preview.title,
    coverUrl: preview.cover_url ?? undefined,
    trackUrls: urls.length > 0 ? urls : undefined,
  };
}

/** Confirms downloads when any target track has an unverified/non-strict candidate. */
function confirmDownloadWithWarnings(
  preview: AlbumPreviewData,
  targetUrls: readonly string[] | undefined,
  onProceedAll: () => void,
  onProceedVerifiedOnly?: (verifiedUrls: string[]) => void,
) {
  const warnings = unverifiedTracksForDownload(preview, targetUrls);
  if (warnings.length === 0) {
    onProceedAll();
    return;
  }

  const allScope = targetUrls && targetUrls.length > 0
    ? targetUrls
    : selectableTracks(preview).map((t) => t.url);
  const warnedUrls = new Set(warnings.map((w) => w.track.url));
  const verifiedOnly = allScope.filter((u) => !warnedUrls.has(u));
  const { openModal, closeModal } = useModalStore.getState();

  openModal(
    <div className={styles.confirmModal}>
      <h3 className={styles.confirmTitle}>
        {warnings.length === 1
          ? "1 track has an unverified source match"
          : `${warnings.length} tracks have unverified source matches`}
      </h3>
      <p className={styles.confirmSubtitle}>
        These audio sources differ slightly in title or duration from the canonical release and could be a different version:
      </p>
      <ul className={styles.confirmList}>
        {warnings.map((w) => (
          <li key={w.track.index} className={styles.confirmListItem}>
            <span className={styles.confirmTrackName}>{w.track.title}</span>
            <span className={styles.confirmTrackWarn}>{w.warning}</span>
          </li>
        ))}
      </ul>
      <div className={styles.confirmActions}>
        <button type="button" className={styles.secondaryButton} onClick={closeModal}>
          Cancel
        </button>
        {onProceedVerifiedOnly && verifiedOnly.length > 0 && (
          <button
            type="button"
            className={styles.secondaryButton}
            onClick={() => {
              closeModal();
              onProceedVerifiedOnly(verifiedOnly);
            }}
          >
            Download verified only ({verifiedOnly.length})
          </button>
        )}
        <button
          type="button"
          className={styles.primaryButton}
          onClick={() => {
            closeModal();
            onProceedAll();
          }}
        >
          Download anyway
        </button>
      </div>
    </div>,
  );
}

/** Starts the download and clears the selection: the queue shows what is downloading from here on. */
function clearSelectionAfter(download: () => Promise<void>) {
  void download().finally(() => useAlbumPreviewStore.setState({ selection: EMPTY_SELECTION }));
}

export default AlbumPreview;
