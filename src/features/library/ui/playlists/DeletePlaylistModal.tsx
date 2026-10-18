import { useModalStore } from "@/shared/store/modalStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import styles from "./delete-playlist-modal.module.css";

interface DeletePlaylistModalProps {
  playlistId: number;
}

const DeletePlaylistModal = ({ playlistId }: DeletePlaylistModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const { deletePlaylist, fetchPlaylists, unselectPlaylist } =
    usePlaylistStore();

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Delete Playlist</h2>
      <span className={styles.text}>
        Are you sure you want to delete this playlist?
      </span>
      <div className={styles.buttons}>
        <button className={styles.cancelButton} onClick={closeModal}>
          Cancel
        </button>
        <button
          className={styles.deleteButton}
          onClick={() => {
            deletePlaylist(playlistId).finally(() => {
              unselectPlaylist();
              fetchPlaylists();
              closeModal();
            });
          }}
        >
          Delete
        </button>
      </div>
    </div>
  );
};

export default DeletePlaylistModal;
