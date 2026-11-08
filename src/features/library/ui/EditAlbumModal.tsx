import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { refreshLibrary } from "@/shared/lib/libraryRefresh.ts";
import { Album } from "@/shared/types.ts";
import styles from "./edit-song-info-modal.module.css";

interface EditAlbumModalProps {
  album: Album;
}

/** The shared value of a field across all songs, or "" when they differ. */
function commonValue<T extends string | number>(
  values: (T | null | undefined)[],
): string {
  const first = values[0];
  if (first === null || first === undefined) return "";
  return values.every((v) => v === first) ? String(first) : "";
}

const EditAlbumModal = ({ album }: EditAlbumModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const genres = album.songs.map((s) => s.genre);
  const years = album.songs.map((s) => s.release_year);

  const [title, setTitle] = useState(album.title);
  const [albumArtist, setAlbumArtist] = useState(album.album_artist);
  const [genre, setGenre] = useState(commonValue(genres));
  const [year, setYear] = useState(commonValue(years));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const parsedYear = parseInt(year, 10);
      await invoke("update_album_info", {
        ids: album.songs.map((s) => s.id),
        album: title,
        albumArtist,
        // empty = keep each song's own value
        genre: genre.trim() || null,
        releaseYear: Number.isFinite(parsedYear) ? parsedYear : null,
      });
      await refreshLibrary();
      closeModal();
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  };

  return (
    <form className={styles.container} onSubmit={handleSave}>
      <h2 className={styles.title}>Edit Album</h2>

      <label className={styles.field}>
        <span>Album</span>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          autoFocus
        />
      </label>
      <label className={styles.field}>
        <span>Album artist</span>
        <input
          value={albumArtist}
          onChange={(e) => setAlbumArtist(e.target.value)}
        />
      </label>
      <label className={styles.field}>
        <span>Genre</span>
        <input
          value={genre}
          placeholder={genres.some((g) => g) ? "Varies by track" : ""}
          onChange={(e) => setGenre(e.target.value)}
        />
      </label>
      <label className={styles.field}>
        <span>Year</span>
        <input
          type="number"
          min={1}
          max={9999}
          value={year}
          placeholder={years.some((y) => y) ? "Varies by track" : ""}
          onChange={(e) => setYear(e.target.value)}
        />
      </label>

      <p className={styles.tip}>
        Applied to all {album.songs.length} tracks and written to their tags.
        Empty genre/year keeps each track&apos;s own value.
      </p>
      {error && <span className={styles.error}>{error}</span>}

      <div className={styles.actions}>
        <button
          type="button"
          className={styles.cancelButton}
          onClick={closeModal}
          disabled={busy}
        >
          Cancel
        </button>
        <button type="submit" className={styles.saveButton} disabled={busy}>
          {busy ? "Saving..." : "Save"}
        </button>
      </div>
    </form>
  );
};

export default EditAlbumModal;
