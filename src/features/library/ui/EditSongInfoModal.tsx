import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { refreshLibrary } from "@/shared/lib/library/libraryRefresh.ts";
import { Song } from "@/shared/types.ts";
import styles from "./edit-song-info-modal.module.css";

interface EditSongInfoModalProps {
  song: Song;
}

const EditSongInfoModal = ({ song }: EditSongInfoModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const [title, setTitle] = useState(song.title);
  const [artists, setArtists] = useState(song.artists.join(", "));
  const [album, setAlbum] = useState(song.album);
  const [albumArtist, setAlbumArtist] = useState(song.album_artist);
  const [trackNumber, setTrackNumber] = useState(
    song.track_number ? String(song.track_number) : "",
  );
  const [genre, setGenre] = useState(song.genre ?? "");
  const [year, setYear] = useState(
    song.release_year ? String(song.release_year) : "",
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!title.trim()) {
      setError("Title cannot be empty");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const parsedTrack = parseInt(trackNumber, 10);
      const parsedYear = parseInt(year, 10);
      await invoke("update_song_info", {
        id: song.id,
        title,
        artists,
        album,
        albumArtist,
        trackNumber: Number.isFinite(parsedTrack) ? parsedTrack : null,
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
      <h2 className={styles.title}>Edit Info</h2>

      <label className={styles.field}>
        <span>Title</span>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          autoFocus
        />
      </label>
      <label className={styles.field}>
        <span>Artists (comma separated)</span>
        <input value={artists} onChange={(e) => setArtists(e.target.value)} />
      </label>
      <label className={styles.field}>
        <span>Album</span>
        <input value={album} onChange={(e) => setAlbum(e.target.value)} />
      </label>
      <label className={styles.field}>
        <span>Album artist</span>
        <input
          value={albumArtist}
          onChange={(e) => setAlbumArtist(e.target.value)}
        />
      </label>
      <label className={styles.field}>
        <span>Track number</span>
        <input
          type="number"
          min={1}
          value={trackNumber}
          onChange={(e) => setTrackNumber(e.target.value)}
        />
      </label>

      <label className={styles.field}>
        <span>Genre</span>
        <input value={genre} onChange={(e) => setGenre(e.target.value)} />
      </label>
      <label className={styles.field}>
        <span>Year</span>
        <input
          type="number"
          min={1}
          max={9999}
          value={year}
          onChange={(e) => setYear(e.target.value)}
        />
      </label>

      <p className={styles.tip}>
        Changes are also written to the audio file&apos;s tags. If the artist
        changes, the song moves to that artist&apos;s page.
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

export default EditSongInfoModal;
