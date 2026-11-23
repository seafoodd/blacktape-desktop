import { addSongsToPlaylist } from "@/features/library/lib/addToPlaylist.tsx";
import { SONG_DRAG_MIME, decodeSongDrag } from "@/shared/lib/ui/dragSongs.ts";
import styles from "./playlists.module.css";
import { AiOutlineSearch } from "react-icons/ai";
import { useEffect, useState } from "react";
import { BiPlus } from "react-icons/bi";
import { MdFavorite } from "react-icons/md";
import clsx from "clsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { CreatePlaylistModal } from "./CreatePlaylistModal";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { ActiveView, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { PlaylistCover } from "@/features/library/ui/playlists/PlaylistCover.tsx";
import { useFavoritesStore } from "@/features/favorites/model/favoritesStore.ts";

interface PlaylistsProps {
  className?: string;
}

export const Playlists = ({ className }: PlaylistsProps) => {
  const { openModal } = useModalStore();
  const [query, setQuery] = useState("");
  const {
    playlists,
    fetchPlaylists,
    selectPlaylist,
    unselectPlaylist,
    activePlaylistId,
  } = usePlaylistStore();
  const { activeView, setActiveView, setSelectedTab } = useLibraryStore();
  const {
    favoriteIds,
    fetchFavoriteIds,
    fetchFavoriteSongs,
    addFavorites,
  } = useFavoritesStore();
  const [dragOverPlaylistId, setDragOverPlaylistId] = useState<number | null>(
    null,
  );
  const [dragOverFavorites, setDragOverFavorites] = useState(false);

  useEffect(() => {
    fetchPlaylists().catch((e) => {
      console.log("fetch playlists error: ", e);
    });
    fetchFavoriteIds().catch((e) => {
      console.log("fetch favorite ids error: ", e);
    });
  }, [fetchPlaylists, fetchFavoriteIds]);

  const handleNewPlaylist = () => {
    openModal(<CreatePlaylistModal />);
  };

  const handleSelectPlaylist = async (id: number) => {
    await selectPlaylist(id);
    setSelectedTab("");
    setActiveView("PLAYLIST" as ActiveView);
  };

  const handleSelectFavorites = async () => {
    unselectPlaylist();
    setSelectedTab("");
    setActiveView("FAVORITES" as ActiveView);
    await fetchFavoriteSongs();
  };

  const handleDragOver = (e: React.DragEvent, playlistId: number) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    if (dragOverPlaylistId !== playlistId) {
      setDragOverPlaylistId(playlistId);
    }
  };

  const handleDragLeave = (playlistId: number) => {
    if (dragOverPlaylistId === playlistId) {
      setDragOverPlaylistId(null);
    }
  };

  const handleDrop = async (e: React.DragEvent, playlistId: number) => {
    e.preventDefault();
    setDragOverPlaylistId(null);
    const data = e.dataTransfer.getData(SONG_DRAG_MIME);
    if (!data) return;

    try {
      const songIds = decodeSongDrag(data);
      if (songIds.length === 0) return;
      await addSongsToPlaylist(playlistId, songIds);
      await fetchPlaylists();
    } catch (err) {
      console.error("Failed to add song to playlist via drag and drop:", err);
    }
  };

  const handleDragOverFavorites = (e: React.DragEvent) => {
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    if (!dragOverFavorites) {
      setDragOverFavorites(true);
    }
  };

  const handleDragLeaveFavorites = () => {
    setDragOverFavorites(false);
  };

  const handleDropFavorites = async (e: React.DragEvent) => {
    e.preventDefault();
    setDragOverFavorites(false);
    const data = e.dataTransfer.getData(SONG_DRAG_MIME);
    if (!data) return;

    try {
      const songIds = decodeSongDrag(data);
      if (songIds.length === 0) return;
      await addFavorites(songIds);
    } catch (err) {
      console.error("Failed to add songs to favorites via drag and drop:", err);
    }
  };

  const normalizedQuery = query.trim().toLowerCase();
  const showFavorites =
    !normalizedQuery ||
    "favorites".includes(normalizedQuery) ||
    "favorite songs".includes(normalizedQuery);

  return (
    <div className={clsx(styles.container, className)}>
      <div className={styles.searchWrapper}>
        <input
          className={styles.searchInput}
          type="text"
          placeholder="Search playlists..."
          value={query}
          onInput={(e) => setQuery(e.currentTarget.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter" && query.trim() !== "") {
              e.preventDefault();
              const firstItem = document.querySelector<HTMLElement>(
                `.${styles.playlist}`,
              );
              firstItem?.focus();
            }
          }}
          onFocus={(e) => e.currentTarget.select()}
        />
        <AiOutlineSearch size={18} className={styles.searchIcon} />
      </div>
      <div className={styles.playlistContainer}>
        <button
          className={styles.newPlaylistButton}
          onClick={handleNewPlaylist}
        >
          <BiPlus size={24} className={styles.plusIcon} />
          New Playlist
        </button>
        <div className={styles.playlists}>
          {showFavorites && (
            <>
              <button
                className={clsx(styles.playlist, {
                  [styles.active]: activeView === "FAVORITES",
                  [styles.dragOver]: dragOverFavorites,
                })}
                onClick={handleSelectFavorites}
                onDragOver={handleDragOverFavorites}
                onDragLeave={handleDragLeaveFavorites}
                onDrop={handleDropFavorites}
              >
                <div className={styles.favoritesCover}>
                  <MdFavorite size={28} className={styles.favoritesCoverIcon} />
                </div>
                <div className={styles.playlistInfo}>
                  <span className={clsx(styles.playlistName, "truncate")}>
                    Favorites
                  </span>
                  <span className={clsx(styles.trackCount, "truncate")}>
                    {favoriteIds.size} Track
                    {favoriteIds.size === 1 ? "" : "s"}
                  </span>
                </div>
              </button>
              <div className={styles.divider} />
            </>
          )}
          {playlists.length === 0 && (
            <div className={styles.emptyNotice}>No playlists yet</div>
          )}
          {playlists.map((playlist) => (
            <button
              key={playlist.id}
              className={clsx(styles.playlist, {
                [styles.active]: activePlaylistId == playlist.id,
                [styles.dragOver]: dragOverPlaylistId == playlist.id,
              })}
              onClick={() => {
                handleSelectPlaylist(playlist.id);
              }}
              onDragOver={(e) => handleDragOver(e, playlist.id)}
              onDragLeave={() => handleDragLeave(playlist.id)}
              onDrop={(e) => handleDrop(e, playlist.id)}
            >
              <PlaylistCover
                coverUrl={playlist.cover_url}
                previewCovers={playlist.preview_covers}
                className={styles.playlistImage}
              />
              <div className={styles.playlistInfo}>
                <span className={clsx(styles.playlistName, "truncate")}>
                  {playlist.name}
                </span>
                <span className={clsx(styles.trackCount, "truncate")}>
                  {playlist.track_count} Track
                  {playlist.track_count === 1 ? "" : "s"}
                </span>
              </div>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
};

