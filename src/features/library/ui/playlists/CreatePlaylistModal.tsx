import { type SyntheticEvent, useState } from "react";
import { useModalStore } from "@/shared/store/modalStore";
import styles from "./create-playlist-modal.module.css";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { ActiveView, useLibraryStore } from "@/shared/store/libraryStore.ts";

interface CreatePlaylistModalProps {
  songIdsToAdd?: number[];
}

export const CreatePlaylistModal = ({
  songIdsToAdd,
}: CreatePlaylistModalProps) => {
  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [loading, setLoading] = useState(false);
  // const closeModal = useModalStore((state) => state.closeModal);
  const { goBack, closeModal, stack } = useModalStore();
  const { createPlaylist, selectPlaylist, addSongs, fetchPlaylists } =
    usePlaylistStore();
  const { setActiveView, setSelectedTab } = useLibraryStore();

  const handleCancelButton = () => {
    if (stack.length > 0) return goBack();
    closeModal();
  };

  const handleCreate = async (e: SyntheticEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    try {
      setLoading(true);
      const createdPlaylistId = await createPlaylist(
        name.trim(),
        description.trim() || undefined,
      );

      if (songIdsToAdd && songIdsToAdd.length > 0) {
        await addSongs(createdPlaylistId, songIdsToAdd);
      }

      await selectPlaylist(createdPlaylistId);
      setSelectedTab("");
      setActiveView("PLAYLIST" as ActiveView);
      await fetchPlaylists();
      closeModal();
    } catch (e) {
      console.error("Failed to create playlist:", e);
    } finally {
      setLoading(false);
    }
  };

  return (
    <form onSubmit={handleCreate} className={styles.container}>
      <h2 className={styles.title}>New Playlist</h2>
      <input
        className={styles.nameInput}
        type="text"
        placeholder="Playlist name..."
        value={name}
        onChange={(e) => setName(e.target.value)}
        autoFocus
      />
      <input
        className={styles.descriptionInput}
        placeholder="Description (optional)..."
        value={description}
        onChange={(e) => setDescription(e.target.value)}
      />
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.cancelButton}
          onClick={handleCancelButton}
        >
          {stack.length > 0 ? "Back" : "Cancel"}
        </button>
        <button
          type="submit"
          className={styles.createButton}
          disabled={!name.trim() || loading}
        >
          {loading ? "Creating..." : "Create"}
        </button>
      </div>
    </form>
  );
};
