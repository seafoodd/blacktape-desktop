import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { refreshLibrary } from "@/shared/lib/library/libraryRefresh.ts";
import { trashSongs } from "@/shared/lib/library/trash.ts";
import { Song } from "@/shared/types.ts";
import styles from "./delete-song-modal.module.css";

interface DeleteSongModalProps {
  songs: Song[];
  /** Shown instead of the song title, e.g. the album name. */
  albumTitle?: string;
}

const DeleteSongModal = ({ songs, albumTitle }: DeleteSongModalProps) => {
  const isMany = albumTitle !== undefined || songs.length > 1;
  const subject = isMany
    ? `${albumTitle ?? "these songs"} (${songs.length} tracks)`
    : songs[0]?.title;
  const closeModal = useModalStore((state) => state.closeModal);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleRemove = async () => {
    setBusy(true);
    setError(null);
    try {
      await invoke("delete_songs", {
        ids: songs.map((s) => s.id),
        deleteFile: false,
      });
      await refreshLibrary();
      closeModal();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  const handleMoveToTrash = async () => {
    setBusy(true);
    setError(null);
    try {
      const outcome = await trashSongs(songs.map((s) => s.id));
      // Songs that did move are already gone from the library, so refresh
      // even when some failed.
      await refreshLibrary();
      if (outcome.failures.length > 0) {
        // The error style is in a CSS file this worker does not own, so the
        // failures are joined on one line rather than with line breaks.
        setError(outcome.failures.join(" | "));
        setBusy(false);
        return;
      }
      closeModal();
    } catch (e) {
      setError(String(e));
      setBusy(false);
    }
  };

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>{isMany ? "Delete Album" : "Delete Song"}</h2>
      <span className={styles.text}>
        What do you want to do with &quot;{subject}&quot;?
      </span>
      <ul className={styles.options}>
        <li>
          <b>Remove from app</b> hides {isMany ? "them" : "it"} from the library; the
          {isMany ? " files stay" : " file stays"} on disk (a rescan can bring it back).
        </li>
        <li>
          <b>Move to Trash</b> moves {isMany ? "the files" : "the file"} to the
          Trash, where {isMany ? "they" : "it"} can be restored or deleted
          permanently later.
        </li>
      </ul>
      {error && <span className={styles.error}>{error}</span>}
      <div className={styles.buttons}>
        <button
          className={styles.cancelButton}
          onClick={closeModal}
          disabled={busy}
        >
          Cancel
        </button>
        <button
          className={styles.cancelButton}
          onClick={handleRemove}
          disabled={busy}
        >
          Remove from app
        </button>
        <button
          className={styles.deleteButton}
          onClick={handleMoveToTrash}
          disabled={busy}
        >
          Move to Trash
        </button>
      </div>
    </div>
  );
};

export default DeleteSongModal;
