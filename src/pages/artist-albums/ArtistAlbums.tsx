import React, { useEffect, useMemo, useRef, useState } from "react";
import styles from "./artist-albums.module.css";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { coverSrc } from "@/shared/lib/cover";
import placeholderAlbumImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import clsx from "clsx";
import { Album, Song } from "@/shared/types.ts";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { ContextMenu, ContextMenuPosition, MenuItemOption } from "@/shared/ui/context-menu";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { AddNextToQueue, AddToQueue } from "@/shared/lib/audio.ts";
import {
  EMPTY_SELECTION,
  nextSelection,
  pruneSelection,
  selectGroup,
  selectionForContextMenu,
  type SelectionState,
} from "@/shared/lib/selection.ts";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";
import { albumMetaLine } from "@/shared/lib/albumMeta.ts";
import { SONG_DRAG_MIME, encodeSongDrag } from "@/shared/lib/dragSongs.ts";
import { setDragGhost } from "@/shared/lib/dragGhost.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { MdWarningAmber } from "react-icons/md";
import { listTrash, TRASH_CHANGED_EVENT, type TrashItem } from "@/shared/lib/trash.ts";
import { listen } from "@tauri-apps/api/event";
import { sameUrl } from "@/shared/lib/urlKey.ts";
import CancelDownloadModal from "@/features/library/ui/CancelDownloadModal.tsx";
import DeleteSongModal from "@/features/library/ui/DeleteSongModal.tsx";
import FindMissingModal from "@/features/library/ui/FindMissingModal.tsx";
import EditAlbumModal from "@/features/library/ui/EditAlbumModal.tsx";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import type { AlbumTrackInfo } from "@/shared/lib/albumPreview.ts";
import type { MissingTracks } from "@/shared/lib/libraryIndex.ts";
import { remoteTrackToStreamItem } from "@/shared/lib/remoteTrack.ts";
import { toStreamRequest, useStreamStore } from "@/features/preview/model/streamStore.ts";
import { startMixedPlayback, type MixedEntry } from "@/shared/lib/stream.ts";
import SongListItem from "@/features/library/ui/song-list-item/SongListItem.tsx";
import {
  playableMissingTracks,
  sourceOfAlbum,
  useMissingTracksStore,
  withoutUnavailable,
} from "@/features/library/model/missingTracksStore.ts";
import MissingTrackRow from "@/features/library/ui/missing-track-row/MissingTrackRow.tsx";
import {
  type Ghost,
  ghostTrack,
  GHOST_TTL_MS,
  mergeAlbumRows,
  nextGhosts,
  withoutDownloaded,
  missingAlbumMenuItems,
  missingRemoteTrack,
  missingTrackPercent,
  missingTrackDownloading,
  findTrashItem,
  albumHasTrackGaps,
} from "@/features/library/ui/missing-track-row/missingTrackRow.ts";

const ArtistAlbums = () => {
  const { selectedTab, albums, activeDownloads, cancelDownload } =
    useLibraryStore();
  const { startPlayback } = useAudioStore();
  const { openModal } = useModalStore();
  const [menu, setMenu] = useState<{
    position: ContextMenuPosition;
    items: MenuItemOption[];
  } | null>(null);

  const missingArtist = useMissingTracksStore((s) => s.artist);
  const missingSources = useMissingTracksStore((s) => s.sources);
  const [trashed, setTrashed] = useState<TrashItem[]>([]);
  useEffect(() => {
    let alive = true;
    const load = () => void listTrash().then((items) => alive && setTrashed(items)).catch(() => {});
    load();
    const unlisten = listen(TRASH_CHANGED_EVENT, load);
    return () => {
      alive = false;
      void unlisten.then((fn) => fn());
    };
  }, []);
  const missingShown = useMissingTracksStore((s) => s.shown);
  const missingTracks = useMissingTracksStore((s) => s.tracks);
  const pendingUrls = useMissingTracksStore((s) => s.pendingUrls);
  const downloadMissingMany = useMissingTracksStore((s) => s.downloadMany);
  const loadMissingSources = useMissingTracksStore((s) => s.loadSources);
  const toggleMissing = useMissingTracksStore((s) => s.toggle);
  const unlinkMissing = useMissingTracksStore((s) => s.unlink);
  const downloadMissing = useMissingTracksStore((s) => s.downloadMissing);
  // The store describes one artist at a time: until it has switched to the selected one, show none of its data.
  const missingReady = missingArtist === selectedTab;
  const unavailable = useMissingTracksStore((s) => s.unavailable);
  const missingFailures = useMissingTracksStore((s) => s.failures);
  const sourceOf = (title: string) => {
    const source = missingReady ? sourceOfAlbum(missingSources, title) : undefined;
    return source ? withoutUnavailable(source, missingTracks[title], unavailable) : undefined;
  };
  const isMissingShown = (title: string) =>
    missingReady && missingShown.has(title);

  // Sources follow the library: a download or delete changes the albums list, which re-reads them.
  useEffect(() => {
    if (selectedTab) void loadMissingSources(selectedTab);
  }, [selectedTab, albums, loadMissingSources]);

  // Songs of all albums in display order.
  const allSongs = useMemo(() => albums.flatMap((a) => a.songs), [albums]);

  // Every row of every album in display order: the songs and the shown missing songs. Missing songs have no
  // library id, so they get negative ones (-1, -2, ...) to be selectable together with the songs.
  // Songs just removed from the library stay in place as dimmed placeholders until the missing songs list has
  // caught up, so deleting (or finishing a download) never makes the rows jump.
  const prevSongsRef = useRef(new Map<string, Song[]>());
  const ghostsRef = useRef(new Map<string, Map<number, Ghost>>());
  const [ghostTick, setGhostTick] = useState(0);

  const { albumViews, orderedIds, missingById } = useMemo(() => {
    const byId = new Map<number, { album: string; track: AlbumTrackInfo; missing: MissingTracks }>();
    const ids: number[] = [];
    let next = -1;
    const now = Date.now();
    const views = albums.map((album) => {
      const key = `${album.album_artist}::${album.title}`;
      const missing =
        missingReady && missingShown.has(album.title) ? missingTracks[album.title] ?? null : null;
      let tracks: AlbumTrackInfo[] = [];
      if (missing) {
        tracks = withoutDownloaded(playableMissingTracks(missing), album.songs);
        const ghosts = nextGhosts(
          prevSongsRef.current.get(key),
          album.songs,
          tracks,
          ghostsRef.current.get(key) ?? new Map(),
          now,
        );
        ghostsRef.current.set(key, ghosts);
        tracks = [...tracks, ...[...ghosts.values()].map((g, i) => ghostTrack(g.song, 10_000 + i))];
      } else {
        ghostsRef.current.delete(key);
      }
      prevSongsRef.current.set(key, album.songs);
      const rows = mergeAlbumRows(album.songs, tracks).map((row) => {
        if (row.kind === "song") {
          ids.push(row.song.id);
          return { row, id: row.song.id };
        }
        const id = next--;
        ids.push(id);
        if (missing && row.track.url !== "") byId.set(id, { album: album.title, track: row.track, missing });
        return { row, id };
      });
      return { missing, rows };
    });
    return { albumViews: views, orderedIds: ids, missingById: byId };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [albums, missingReady, missingShown, missingTracks, ghostTick]);

  // Placeholders expire: render again once the oldest one is due.
  useEffect(() => {
    const any = [...ghostsRef.current.values()].some((g) => g.size > 0);
    if (!any) return;
    const timer = setTimeout(() => setGhostTick((t) => t + 1), GHOST_TTL_MS + 100);
    return () => clearTimeout(timer);
  }, [albumViews]);

  const [selection, setSelection] = useState<SelectionState>(EMPTY_SELECTION);

  // Keep only songs that still exist (after delete, refresh or switching artist).
  useEffect(() => {
    setSelection((prev) => pruneSelection(prev, orderedIds));
  }, [orderedIds]);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setSelection(EMPTY_SELECTION);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const { getContextMenuItems } = useSongContextMenu({
    onPlay: (song) => handlePlaySongRow(song.id),
    playbackContext: "library",
    allowDelete: true,
  });

  const handleSelectSong = (song: Song, e: React.MouseEvent) => {
    setSelection((prev) =>
      nextSelection(prev, orderedIds, song.id, {
        ctrl: e.ctrlKey || e.metaKey,
        shift: e.shiftKey,
      }),
    );
  };

  /** Click on an album's cover or title selects all its tracks (Ctrl/Cmd adds). */
  const handleSelectAlbum = (album: Album, e: React.MouseEvent) => {
    e.stopPropagation(); // the page background would clear the selection
    setSelection((prev) =>
      selectGroup(
        prev,
        (albumViews[albums.indexOf(album)]?.rows.map((r) => r.id)) ?? album.songs.map((s) => s.id),
        e.ctrlKey || e.metaKey,
      ),
    );
  };

  /** Dragging an album's cover or title (e.g. onto a playlist) carries all its tracks. */
  const handleAlbumDragStart = (album: Album, e: React.DragEvent) => {
    const ids = album.songs.map((s) => s.id);
    e.dataTransfer.setData(SONG_DRAG_MIME, encodeSongDrag(ids));
    e.dataTransfer.effectAllowed = "copy";
    setDragGhost(
      e,
      album.title,
      `${ids.length} ${ids.length === 1 ? "track" : "tracks"} · Drop to add`,
    );
  };

  /** Double click on an album's cover or title plays the album. */
  const handlePlayAlbum = (album: Album) => {
    const ids = album.songs.map((s) => s.id);
    if (ids.length === 0 && !albumViews[albums.indexOf(album)]?.missing) return;
    // Shown missing songs are part of the album: they are streamed after / between the songs.
    if (albumViews[albums.indexOf(album)]?.missing) {
      void playPicks(albumPicks(album));
      return;
    }
    startPlayback(ids, 0, "library").catch((e) =>
      console.error("Play Album error: ", e),
    );
  };

  /** Menu for several selected songs. */
  const multiSongMenuItems = (songs: Song[]): MenuItemOption[] => {
    const ids = songs.map((s) => s.id);
    const label = `${songs.length} tracks`;
    return [
      {
        label: `Play ${label}`,
        action: () => {
          startPlayback(ids, 0, "library").catch((e) =>
            console.error("Play selection error: ", e),
          );
        },
      },
      {
        label: "Play Next",
        action: async () => {
          try {
            // Each one is inserted right after the current track, so go backwards
            // to keep the selection order.
            for (const id of [...ids].reverse()) await AddNextToQueue(id);
          } catch (e) {
            console.error("Play Next error: ", e);
          }
        },
      },
      {
        label: "Add to Queue",
        action: async () => {
          try {
            for (const id of ids) await AddToQueue(id);
          } catch (e) {
            console.error("Add to Queue error: ", e);
          }
        },
      },
      { separator: true, label: "" },
      {
        label: "Add to Playlist...",
        action: () => openModal(<AddToPlaylistModal songIds={ids} />),
      },
      { separator: true, label: "" },
      {
        label: `Delete ${label}...`,
        danger: true,
        action: () => openModal(<DeleteSongModal songs={songs} />),
      },
    ];
  };

  const handleOpenSongMenu = (song: Song, position: ContextMenuPosition) => {
    const next = selectionForContextMenu(selection, song.id);
    setSelection(next);
    const selectedSongs = allSongs.filter((s) => next.selected.has(s.id));
    const selectedMissing = selectedMissingOf(next);
    setMenu({
      position,
      items:
        selectedMissing.length > 0
          ? mixedMenuItems(next)
          : selectedSongs.length > 1
            ? multiSongMenuItems(selectedSongs)
            : getContextMenuItems(song),
    });
  };

  const selectedMissingOf = (state: SelectionState) =>
    [...state.selected]
      .filter((id) => id < 0)
      .map((id) => missingById.get(id))
      .filter((m): m is NonNullable<typeof m> => !!m);

  type Pick =
    | { kind: "song"; song: Song }
    | { kind: "missing"; album: string; track: AlbumTrackInfo; missing: MissingTracks };

  /** The rows with these ids (songs and missing songs) in list order. */
  const picksOf = (include: (id: number) => boolean): Pick[] => {
    const songById = new Map(allSongs.map((s) => [s.id, s]));
    const picks: Pick[] = [];
    for (const id of orderedIds) {
      if (!include(id)) continue;
      const song = songById.get(id);
      const m = missingById.get(id);
      if (song) picks.push({ kind: "song", song });
      else if (m) picks.push({ kind: "missing", ...m });
    }
    return picks;
  };

  const streamOf = (p: Extract<Pick, { kind: "missing" }>) =>
    remoteTrackToStreamItem(missingRemoteTrack(p.missing, p.track));

  /** Queues a downloaded song from the library or streams a missing one. */
  const queuePick = (p: Pick, playNext: boolean) =>
    p.kind === "song"
      ? playNext
        ? AddNextToQueue(p.song.id)
        : AddToQueue(p.song.id)
      : useStreamStore.getState().enqueue([streamOf(p)], { playNext });

  /** The visible rows of an album: its songs and, while shown, its missing songs, in track order. */
  const albumPicks = (album: Album): Pick[] => {
    const ids = new Set(albumViews[albums.indexOf(album)]?.rows.map((r) => r.id) ?? []);
    return picksOf((id) => ids.has(id));
  };

  /** The picks as a mixed queue: library songs by id, missing songs as streams. */
  const mixedEntries = (picks: Pick[]): MixedEntry[] =>
    picks.map((p) =>
      p.kind === "song"
        ? { kind: "song", id: p.song.id }
        : { kind: "stream", ...toStreamRequest(streamOf(p)) },
    );

  /** Plays the first of `picks`; all of them are the queue (downloaded from the library, missing streamed). */
  const playPicks = async (picks: Pick[]) => {
    try {
      if (picks.length === 0) return;
      await startMixedPlayback(mixedEntries(picks), 0);
    } catch (e) {
      console.error("Play Album error: ", e);
    }
  };

  /**
   * Plays the row `id` with every row of the page as the queue, in list order: downloaded songs from the
   * library, shown missing songs streamed. The rows before it stay in the queue (reachable with "previous").
   */
  const playFromRow = async (id: number) => {
    try {
      const at = orderedIds.indexOf(id);
      if (at < 0) return;
      // every row is a pick, so the picks line up with orderedIds
      await startMixedPlayback(mixedEntries(picksOf(() => true)), at);
    } catch (e) {
      console.error("Play error: ", e);
    }
  };

  /**
   * Menu for a selection that contains missing songs. Play, Play Next and Add to Queue take the selected songs in
   * list order, downloaded ones from the library and missing ones as streams. Download touches only the missing
   * songs; Add to Playlist and Delete only the downloaded ones (their labels say so).
   */
  const mixedMenuItems = (state: SelectionState): MenuItemOption[] => {
    const picks = picksOf((id) => state.selected.has(id));
    const songs = picks.flatMap((p) => (p.kind === "song" ? [p.song] : []));
    const missing = picks.flatMap((p) => (p.kind === "missing" ? [p] : []));
    const addPick = queuePick;

    const items: MenuItemOption[] = [
      {
        label: "Play",
        action: () => void playPicks(picks),
      },
    ];
    if (missing.length > 0) {
      items.push({
        label: `Download ${missing.length} missing song${missing.length === 1 ? "" : "s"}`,
        action: () => {
          const byAlbum = new Map<string, AlbumTrackInfo[]>();
          for (const m of missing) byAlbum.set(m.album, [...(byAlbum.get(m.album) ?? []), m.track]);
          for (const [album, tracks] of byAlbum) void downloadMissingMany(album, tracks);
        },
      });
    }
    items.push(
      {
        label: "Play Next",
        action: async () => {
          try {
            // Each one is inserted right after the current track, so go backwards to keep the order.
            for (const p of [...picks].reverse()) await addPick(p, true);
          } catch (e) {
            console.error("Play Next error: ", e);
          }
        },
      },
      {
        label: "Add to Queue",
        action: async () => {
          try {
            for (const p of picks) await addPick(p, false);
          } catch (e) {
            console.error("Add to Queue error: ", e);
          }
        },
      },
    );
    if (songs.length > 0) {
      const ids = songs.map((s) => s.id);
      const noun = `${songs.length} downloaded track${songs.length === 1 ? "" : "s"}`;
      items.push(
        { separator: true, label: "" },
        {
          label: "Add to Playlist...",
          action: () => openModal(<AddToPlaylistModal songIds={ids} />),
        },
        { separator: true, label: "" },
        {
          label: `Delete ${noun}...`,
          danger: true,
          action: () => openModal(<DeleteSongModal songs={songs} />),
        },
      );
    }
    return items;
  };

  const handleSelectMissing = (id: number, e: React.MouseEvent) => {
    setSelection((prev) =>
      nextSelection(prev, orderedIds, id, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey }),
    );
  };

  /** Right click on a missing song: a multi-selection gets the combined menu, one song its own remote menu. */
  const handleMissingMenu = (id: number, e: React.MouseEvent): boolean => {
    const next = selectionForContextMenu(selection, id);
    setSelection(next);
    if (next.selected.size < 2) return false;
    e.preventDefault();
    e.stopPropagation();
    setMenu({
      position: { x: e.clientX, y: e.clientY },
      items: mixedMenuItems(next),
    });
    return true;
  };

  const openMenu = (e: React.MouseEvent, items: MenuItemOption[]) => {
    e.preventDefault();
    e.stopPropagation();
    setMenu({ position: { x: e.clientX, y: e.clientY }, items });
  };

  const albumMenuItems = (album: Album): MenuItemOption[] => {
    const ids = album.songs.map((s) => s.id);
    return [
      {
        label: "Play Album",
        action: () => handlePlayAlbum(album),
      },
      {
        label: "Add Album to Queue",
        action: async () => {
          try {
            if (albumViews[albums.indexOf(album)]?.missing) {
              for (const p of albumPicks(album)) await queuePick(p, false);
            } else {
              for (const id of ids) await AddToQueue(id);
            }
          } catch (e) {
            console.error("Add Album to Queue error: ", e);
          }
        },
      },
      {
        label: "Add Album to Playlist...",
        action: () => openModal(<AddToPlaylistModal songIds={ids} />),
      },
      ...missingAlbumMenuItems(sourceOf(album.title), isMissingShown(album.title), {
        toggle: () => toggleMissing(album.title),
        download: () => void downloadMissing(album.title),
        downloadAs: () => {
          const source = sourceOf(album.title);
          if (!source) return;
          openQualityMenu(
            {
              platform: source.platform,
              downloadType: ItemType.Album,
              url: source.source_url,
              artistName: selectedTab,
              title: album.title,
            },
            { start: (format) => void downloadMissing(album.title, format) },
          );
        },
        find: () =>
          openModal(<FindMissingModal albumArtist={selectedTab} album={album.title} />),
        unlink: () => void unlinkMissing(selectedTab, album.title),
      }),
      { separator: true, label: "" },
      {
        label: "Edit Album...",
        action: () => openModal(<EditAlbumModal album={album} />),
      },
      {
        label: "Show Album in Explorer",
        action: () => {
          if (album.songs[0]) {
            revealItemInDir(album.songs[0].path).catch((e) =>
              console.error("revealItemInDir error: ", e),
            );
          }
        },
      },
      { separator: true, label: "" },
      {
        label: "Delete Album...",
        danger: true,
        action: () =>
          openModal(
            <DeleteSongModal songs={album.songs} albumTitle={album.title} />,
          ),
      },
    ];
  };

  /** Albums whose running download only fetches missing songs: they show up inside the album, not as a new one. */
  const missingDownloadUrls = useMemo(() => new Set(pendingUrls.values()), [pendingUrls]);

  /** Library albums linked to a remote album: a download of that remote album fills the library album. */
  const linkedAlbumOf = (d: { url: string; itemType: string; albumTitle: string }): string | undefined => {
    if (!missingReady) return undefined;
    const byUrl = Object.entries(missingSources).find(([, source]) => sameUrl(source.source_url, d.url))?.[0];
    if (byUrl !== undefined) return byUrl;
    // A single track filed into an album the library has (the download carries that album's title).
    if (d.itemType !== "Track") return undefined;
    const title = d.albumTitle.trim().toLowerCase();
    return albums.find((a) => a.title.trim().toLowerCase() === title)?.title;
  };

  const activeArtistDownloads = useMemo(() => {
    if (!selectedTab) return [];
    return Object.values(activeDownloads).filter(
      (d) =>
        (d.status === "downloading" ||
          d.status === "idle" ||
          d.status === "processing") &&
        !missingDownloadUrls.has(d.url) &&
        // a single track shows up once its album is known (otherwise it would flash as "Singles" first)
        !(d.itemType === "Track" && d.albumTitle.trim() === "") &&
        linkedAlbumOf(d) === undefined &&
        d.artistName.trim().toLowerCase() === selectedTab.trim().toLowerCase(),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDownloads, selectedTab, missingDownloadUrls, missingSources, missingReady, albums]);

  // A running download of a linked album shows its songs inside the library album: open that album's rows.
  useEffect(() => {
    if (!selectedTab || !missingReady) return;
    for (const d of Object.values(activeDownloads)) {
      if (d.status !== "downloading" && d.status !== "idle" && d.status !== "processing") continue;
      const album = linkedAlbumOf(d);
      if (album !== undefined && !missingShown.has(album)) toggleMissing(album);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDownloads, missingSources, missingReady, albums]);

  // Nothing is missing any more (everything was downloaded): the album closes its list of missing songs.
  useEffect(() => {
    if (!missingReady) return;
    for (const title of missingShown) {
      const source = sourceOfAlbum(missingSources, title);
      if (!source || source.missing > 0) continue;
      const busy = Object.values(activeDownloads).some(
        (d) => linkedAlbumOf(d) === title && ["downloading", "idle", "processing"].includes(d.status),
      );
      if (!busy) toggleMissing(title);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeDownloads, missingSources, missingShown, missingReady]);

  /** Play from a song row: with missing songs shown the queue goes through them, otherwise it is the songs. */
  function handlePlaySongRow(songId: number) {
    if (missingById.size > 0) void playFromRow(songId);
    else handlePlaySong(songId);
  }

  function handlePlaySong(clickedSongId: number) {
    const currentIndex = allSongs.findIndex(
      (song) => song.id === clickedSongId,
    );
    if (currentIndex === -1) return;

    // const historyIds = allSongs.slice(0, currentIndex).map((s) => s.id);
    const queueIds = allSongs.map((s) => s.id);

    startPlayback(queueIds, currentIndex, "library").catch((e) =>
      console.log("startPlayback error: ", e),
    );
  }

  if (!selectedTab) {
    return null;
  }

  return (
    <div
      className={styles.container}
      onClick={(e) => {
        // A click outside the song rows clears the selection.
        if (!(e.target as HTMLElement).closest("[data-song-row]")) {
          setSelection(EMPTY_SELECTION);
        }
      }}
    >
      <h1 className={clsx(styles.artistName, "truncate")}>{selectedTab}</h1>
      <div className={styles.albums}>
        {/* Downloading Albums */}
        {activeArtistDownloads.map((download) => (
          <section
            className={clsx(styles.albumBlock, styles.downloadingAlbumBlock)}
            key={`download-${download.id}`}
            onContextMenu={(e) =>
              openMenu(e, [
                {
                  label: "Cancel Download",
                  danger: true,
                  action: () =>
                    openModal(
                      <CancelDownloadModal
                        title={decodeHtmlEntities(download.albumTitle)}
                        artist={download.artistName}
                        onConfirm={() => {
                          cancelDownload(download.id).catch((e) =>
                            console.error("cancelDownload error: ", e),
                          );
                        }}
                      />,
                    ),
                },
              ])
            }
          >
            <div className={styles.albumBlockLeft}>
              <div className={styles.downloadingCoverWrapper}>
                <img loading="lazy" decoding="async"
                  src={
                    download.coverUrl
                      ? download.coverUrl.startsWith("http")
                        ? download.coverUrl
                        : coverSrc(download.coverUrl, 200)
                      : placeholderAlbumImage
                  }
                  referrerPolicy="no-referrer"
                  onError={(e) => {
                    if (e.currentTarget.src !== placeholderAlbumImage) {
                      e.currentTarget.src = placeholderAlbumImage;
                    }
                  }}
                  className={styles.playlistImage}
                  alt={download.albumTitle}
                />
                <div className={styles.downloadingCoverBadge}>
                  Downloading {download.overallPercent}%
                </div>
              </div>
            </div>
            <div className={styles.albumBlockRight}>
              <div className={styles.downloadingHeader}>
                <h2 className={clsx(styles.albumTitle, "truncate")}>
                  {decodeHtmlEntities(download.albumTitle)}
                </h2>
                <span className={styles.downloadingOverallBadge}>
                  {download.overallPercent}%
                </span>
              </div>
              <ul className={styles.songs}>
                {download.tracks && download.tracks.length > 0 ? (
                  download.tracks.map((track, trackIdx) => (
                    <li
                      className={clsx(styles.song, styles.downloadingSong)}
                      key={`track-${trackIdx}`}
                    >
                      <div className={styles.songLeft}>
                        <div className={styles.songTrackNumber}>
                          {String(track.trackNumber || trackIdx + 1).padStart(
                            2,
                            "0",
                          )}
                        </div>
                        <div className={clsx(styles.songTitle, "truncate")}>
                          {decodeHtmlEntities(track.title)}
                        </div>
                      </div>
                      <div className={styles.songRight}>
                        <div className={styles.trackPercentage}>
                          {track.status === "completed" ||
                          track.percent >= 100 ? (
                            <span className={styles.percentCompleted}>
                              100%
                            </span>
                          ) : track.percent > 0 ? (
                            <span className={styles.percentActive}>
                              {track.percent}%
                            </span>
                          ) : (
                            <span className={styles.percentPending}>0%</span>
                          )}
                        </div>
                      </div>
                    </li>
                  ))
                ) : (
                  <li className={clsx(styles.song, styles.downloadingSong)}>
                    <div className={styles.songLeft}>
                      <div className={styles.songTrackNumber}>01</div>
                      <div className={clsx(styles.songTitle, "truncate")}>
                        {download.message || "Preparing download..."}
                      </div>
                    </div>
                    <div className={styles.songRight}>
                      <div className={styles.trackPercentage}>
                        <span className={styles.percentPending}>
                          {download.overallPercent}%
                        </span>
                      </div>
                    </div>
                  </li>
                )}
              </ul>
            </div>
          </section>
        ))}

        {/* Existing Downloaded Albums */}
        {albums.map((album, index) => (
          <section
            className={styles.albumBlock}
            // Stable key: with the index, every album after an added / removed one was remounted (covers reloaded).
            key={`${album.album_artist}::${album.title}`}
            onContextMenu={(e) => openMenu(e, albumMenuItems(album))}
          >
            <div
              className={clsx(styles.albumBlockLeft, styles.albumHeader)}
              onClick={(e) => handleSelectAlbum(album, e)}
              onDoubleClick={() => handlePlayAlbum(album)}
              draggable
              onDragStart={(e) => handleAlbumDragStart(album, e)}
            >
              <img loading="lazy" decoding="async" draggable={false}
                src={
                  album.cover_url
                    ? coverSrc(album.cover_url, 200)
                    : placeholderAlbumImage
                }
                referrerPolicy="no-referrer"
                onError={(e) => {
                  if (e.currentTarget.src !== placeholderAlbumImage) {
                    e.currentTarget.src = placeholderAlbumImage;
                  }
                }}
                className={styles.playlistImage}
                alt={album.title}
              />
            </div>
            <div className={styles.albumBlockRight}>
              <div className={styles.albumTitleRow}>
                <h2
                  className={clsx(
                    styles.albumTitle,
                    styles.albumHeader,
                    "truncate",
                  )}
                  onClick={(e) => handleSelectAlbum(album, e)}
                  onDoubleClick={() => handlePlayAlbum(album)}
                  draggable
                  onDragStart={(e) => handleAlbumDragStart(album, e)}
                >
                  {album.title}
                </h2>
                {albumMetaLine(album.songs) && (
                  <span className={clsx(styles.albumMeta, "truncate")}>
                    {albumMetaLine(album.songs)}
                  </span>
                )}
                {(() => {
                  const source = sourceOf(album.title);
                  if (source) {
                    // songs with no stream are not counted, but they are still listed: the button stays for them
                    const raw = sourceOfAlbum(missingSources, album.title)?.missing ?? 0;
                    const shown = isMissingShown(album.title);
                    if (raw === 0 && !shown) return null;
                    const noun = (n: number) => `${n} ${n === 1 ? "song" : "songs"}`;
                    const label =
                      source.missing > 0
                        ? `${shown ? "Hide" : "Show"} ${source.missing} missing ${source.missing === 1 ? "song" : "songs"}`
                        : shown
                          ? "Hide missing songs"
                          : `Show ${noun(raw)} with no stream`;
                    return (
                      <button
                        type="button"
                        className={clsx(
                          styles.missingButton,
                          styles.missingToggle,
                          shown && styles.missingButtonOn,
                          source.owned * 2 < source.total && styles.missingMostly,
                        )}
                        title={label}
                        aria-pressed={shown}
                        onClick={() => toggleMissing(album.title)}
                      >
                        <span>{label}</span>
                      </button>
                    );
                  }
                  if (!missingReady || !albumHasTrackGaps(album.songs)) return null;
                  return (
                    <button
                      type="button"
                      className={styles.missingButton}
                      title="Some songs may be missing. Find them on the platforms"
                      onClick={() => openModal(<FindMissingModal albumArtist={selectedTab} album={album.title} />)}
                    >
                      <MdWarningAmber size={16} />
                    </button>
                  );
                })()}
              </div>
              <ul className={styles.songs}>
                {(() => {
                  const { missing, rows } = albumViews[index];
                  const download = missing
                    ? Object.values(activeDownloads).find((d) => linkedAlbumOf(d) === album.title)
                    : undefined;
                  const selectedAt = (i: number) => {
                    const r = rows[i];
                    return !!r && selection.selected.has(r.id);
                  };
                  return rows.map(({ row, id }, i) =>
                    row.kind === "song" ? (
                      <SongListItem
                        key={row.song.id}
                        selectedAbove={selectedAt(i - 1)}
                        selectedBelow={selectedAt(i + 1)}
                        dragIds={
                          selection.selected.has(row.song.id)
                            ? orderedIds.filter((x) => x > 0 && selection.selected.has(x))
                            : [row.song.id]
                        }
                        song={row.song}
                        onPlay={handlePlaySongRow}
                        onSelect={handleSelectSong}
                        onOpenMenu={handleOpenSongMenu}
                        selected={selection.selected.has(row.song.id)}
                      />
                    ) : (
                      missing && (
                        <MissingTrackRow
                          key={`missing-${row.track.index}-${row.track.url}`}
                          album={album.title}
                          missing={missing}
                          track={row.track}
                          percent={missingTrackPercent(download, row.track)}
                          downloading={missingTrackDownloading(download, row.track)}
                          ghost={row.track.url === ""}
                          unavailable={unavailable.has(row.track.url)}
                          failure={missingFailures[row.track.url]}
                          trashId={findTrashItem(trashed, album.title, row.track)?.id}
                          selected={selection.selected.has(id)}
                          selectedAbove={selectedAt(i - 1)}
                          selectedBelow={selectedAt(i + 1)}
                          onPlay={() => void playFromRow(id)}
                          onSelect={(e) => handleSelectMissing(id, e)}
                          onMenu={(e) => handleMissingMenu(id, e)}
                        />
                      )
                    ),
                  );
                })()}
              </ul>
            </div>
          </section>
        ))}
      </div>
      {menu && (
        <ContextMenu
          position={menu.position}
          items={menu.items}
          onClose={() => setMenu(null)}
        />
      )}
    </div>
  );
};

export default ArtistAlbums;
