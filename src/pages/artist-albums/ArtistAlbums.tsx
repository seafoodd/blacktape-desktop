import React, { useEffect, useMemo, useState } from "react";
import styles from "./artist-albums.module.css";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
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
  selectionForContextMenu,
  type SelectionState,
} from "@/shared/lib/selection.ts";
import { useSongContextMenu } from "@/shared/hooks/useSongContextMenu.tsx";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import CancelDownloadModal from "@/features/library/ui/CancelDownloadModal.tsx";
import DeleteSongModal from "@/features/library/ui/DeleteSongModal.tsx";
import EditAlbumModal from "@/features/library/ui/EditAlbumModal.tsx";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import SongListItem from "@/features/library/ui/song-list-item/SongListItem.tsx";

const ArtistAlbums = () => {
  const { selectedTab, albums, activeDownloads, cancelDownload } =
    useLibraryStore();
  const { startPlayback } = useAudioStore();
  const { openModal } = useModalStore();
  const [menu, setMenu] = useState<{
    position: ContextMenuPosition;
    items: MenuItemOption[];
  } | null>(null);

  // Songs of all albums in display order (Shift ranges follow it).
  const allSongs = useMemo(() => albums.flatMap((a) => a.songs), [albums]);
  const orderedIds = useMemo(() => allSongs.map((s) => s.id), [allSongs]);
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
    onPlay: (song) => handlePlaySong(song.id),
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
    setMenu({
      position,
      items:
        selectedSongs.length > 1
          ? multiSongMenuItems(selectedSongs)
          : getContextMenuItems(song),
    });
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
        action: () => {
          startPlayback(ids, 0, "library").catch((e) =>
            console.error("Play Album error: ", e),
          );
        },
      },
      {
        label: "Add Album to Queue",
        action: async () => {
          try {
            for (const id of ids) await AddToQueue(id);
          } catch (e) {
            console.error("Add Album to Queue error: ", e);
          }
        },
      },
      {
        label: "Add Album to Playlist...",
        action: () => openModal(<AddToPlaylistModal songIds={ids} />),
      },
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

  const activeArtistDownloads = useMemo(() => {
    if (!selectedTab) return [];
    return Object.values(activeDownloads).filter(
      (d) =>
        (d.status === "downloading" ||
          d.status === "idle" ||
          d.status === "processing") &&
        d.artistName.trim().toLowerCase() === selectedTab.trim().toLowerCase(),
    );
  }, [activeDownloads, selectedTab]);

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
            key={index}
            onContextMenu={(e) => openMenu(e, albumMenuItems(album))}
          >
            <div className={styles.albumBlockLeft}>
              <img loading="lazy" decoding="async"
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
              <h2 className={clsx(styles.albumTitle, "truncate")}>
                {album.title}
              </h2>
              <ul className={styles.songs}>
                {album.songs.map((song: Song, songIndex: number) => (
                  <SongListItem
                    key={song.id}
                    selectedAbove={
                      songIndex > 0 &&
                      selection.selected.has(album.songs[songIndex - 1].id)
                    }
                    selectedBelow={
                      songIndex < album.songs.length - 1 &&
                      selection.selected.has(album.songs[songIndex + 1].id)
                    }
                    dragIds={
                      selection.selected.has(song.id)
                        ? orderedIds.filter((id) => selection.selected.has(id))
                        : [song.id]
                    }
                    song={song}
                    onPlay={handlePlaySong}
                    onSelect={handleSelectSong}
                    onOpenMenu={handleOpenSongMenu}
                    selected={selection.selected.has(song.id)}
                  />
                ))}
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
