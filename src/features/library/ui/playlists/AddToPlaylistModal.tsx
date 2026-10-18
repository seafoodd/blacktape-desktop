import { useEffect, useState } from "react";
import { usePlaylistStore } from "@/shared/store/playlistStore";
import { useModalStore } from "@/shared/store/modalStore";
import styles from "./add-to-playlist-modal.module.css";
import { CreatePlaylistModal } from "@/features/library/ui/playlists/CreatePlaylistModal.tsx";
import { PlaylistCover } from "@/features/library/ui/playlists/PlaylistCover.tsx";
import clsx from "clsx";
import { BsFillPlusSquareFill } from "react-icons/bs";

interface AddToPlaylistModalProps {
  songIds: number[];
}

export const AddToPlaylistModal = ({ songIds }: AddToPlaylistModalProps) => {
  const { playlists, fetchPlaylists, addSongs } = usePlaylistStore();
  const closeModal = useModalStore((state) => state.closeModal);
  const [loadingId, setLoadingId] = useState<number | null>(null);
  const { openModal } = useModalStore();

  useEffect(() => {
    fetchPlaylists();
  }, []);

  const handleAddToPlaylist = async (playlistId: number) => {
    try {
      setLoadingId(playlistId);
      await addSongs(playlistId, songIds);
      closeModal();
    } catch (e) {
      console.error("Failed to add song to playlist:", e);
    } finally {
      fetchPlaylists();
      setLoadingId(null);
    }
  };

  const sortedPlaylists = [...playlists].sort(
    (a, b) =>
      new Date(b.date_modified).getTime() - new Date(a.date_modified).getTime(),
  );

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Add to Playlist</h2>
      <div className={styles.list}>
        <button
          onClick={() => {
            openModal(<CreatePlaylistModal songIdsToAdd={songIds} />);
          }}
          className={styles.playlistItem}
        >
          <BsFillPlusSquareFill className={styles.playlistImage} size={48} />
          <span className={styles.playlistName}>Create New Playlist</span>
        </button>
        {playlists.length > 0 &&
          sortedPlaylists.map((playlist) => (
            <button
              key={playlist.id}
              className={styles.playlistItem}
              disabled={loadingId === playlist.id}
              onClick={() => handleAddToPlaylist(playlist.id)}
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
                <span className={clsx(styles.playlistTrackCount, "truncate")}>
                  {playlist.track_count} tracks
                </span>
              </div>
              {loadingId === playlist.id && (
                <span className={styles.loading}>Adding...</span>
              )}
            </button>
          ))}
      </div>
      <button className={styles.closeButton} onClick={closeModal}>
        Cancel
      </button>
    </div>
  );
};
