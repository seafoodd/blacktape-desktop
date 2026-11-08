import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { refreshLibrary } from "@/shared/lib/libraryRefresh.ts";
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

  const handleDelete = async (deleteFile: boolean) => {
    setBusy(true);
    setError(null);
    try {
      await invoke("delete_songs", { ids: songs.map((s) => s.id), deleteFile });
      await refreshLibrary();
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
          <b>Delete from disk</b> permanently deletes {isMany ? "the files" : "the file"}.
          This cannot be undone.
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
          onClick={() => handleDelete(false)}
          disabled={busy}
        >
          Remove from app
        </button>
        <button
          className={styles.deleteButton}
          onClick={() => handleDelete(true)}
          disabled={busy}
        >
          Delete from disk
        </button>
      </div>
    </div>
  );
};

export default DeleteSongModal;
