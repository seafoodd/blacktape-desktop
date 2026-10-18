import styles from "./edit-playlist-modal.module.css";
import { useState } from "react";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";

interface EditPlaylistModalProps {
  playlistId: number;
  name: string;
  description: string;
  onSuccess?: () => void;
}

const EditPlaylistModal = ({
  playlistId,
  name,
  description,
  onSuccess,
}: EditPlaylistModalProps) => {
  const [newName, setName] = useState(name);
  const [newDescription, setDescription] = useState(description || "");
  const [touched, setTouched] = useState(false);
  const closeModal = useModalStore((state) => state.closeModal);
  const [loading, setLoading] = useState(false);
  const { updatePlaylist } = usePlaylistStore();

  const isNameEmpty = !newName.trim();

  const handleUpdate = async (e: React.FormEvent) => {
    e.preventDefault();
    setTouched(true);

    if (isNameEmpty) return;

    try {
      setLoading(true);
      await updatePlaylist(playlistId, newName.trim(), newDescription.trim());
      onSuccess?.();
      closeModal();
    } catch (e) {
      console.error("Failed to update playlist:", e);
    } finally {
      setLoading(false);
    }
  };

  return (
    <form className={styles.container} onSubmit={handleUpdate}>
      <h2 className={styles.title}>Edit Playlist</h2>
      <div className={styles.inputWrapper}>
        <input
          className={`${styles.nameInput} ${touched && isNameEmpty ? styles.inputError : ""}`}
          type="text"
          placeholder="Playlist name..."
          value={newName}
          onChange={(e) => {
            setName(e.target.value);
            if (touched) setTouched(false);
          }}
          onBlur={() => setTouched(true)}
        />
        {touched && isNameEmpty && (
          <span className={styles.errorText}>
            Playlist name cannot be empty
          </span>
        )}
      </div>
      <input
        className={styles.descriptionInput}
        placeholder="Description..."
        value={newDescription}
        autoFocus
        onChange={(e) => setDescription(e.target.value)}
      />
      <div className={styles.actions}>
        <button
          type="button"
          className={styles.cancelButton}
          onClick={closeModal}
        >
          Cancel
        </button>
        <button
          type="submit"
          className={styles.createButton}
          disabled={loading}
        >
          {loading ? "Updating..." : "Update"}
        </button>
      </div>
    </form>
  );
};

export default EditPlaylistModal;
