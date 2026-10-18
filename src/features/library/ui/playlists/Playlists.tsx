import styles from "./playlists.module.css";
import { AiOutlineSearch } from "react-icons/ai";
import { useEffect, useState } from "react";
import { BiPlus } from "react-icons/bi";
import clsx from "clsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { CreatePlaylistModal } from "./CreatePlaylistModal";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { ActiveView, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { PlaylistCover } from "@/features/library/ui/playlists/PlaylistCover.tsx";

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
    activePlaylistId,
    addSongs,
  } = usePlaylistStore();
  const { setActiveView, setSelectedTab } = useLibraryStore();
  const [dragOverPlaylistId, setDragOverPlaylistId] = useState<number | null>(
    null,
  );

  useEffect(() => {
    fetchPlaylists().catch((e) => {
      console.log("fetch playlists error: ", e);
    });
  }, []);

  const handleNewPlaylist = () => {
    openModal(<CreatePlaylistModal />);
  };

  const handleSelectPlaylist = async (id: number) => {
    await selectPlaylist(id);
    setSelectedTab("");
    setActiveView("PLAYLIST" as ActiveView);
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
    const data = e.dataTransfer.getData("application/json");
    if (!data) return;

    try {
      const { songId } = JSON.parse(data);
      await addSongs(playlistId, [songId]);
      await fetchPlaylists();
    } catch (err) {
      console.error("Failed to add song to playlist via drag and drop:", err);
    }
  };

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
                `.${styles.artistItem}`,
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
