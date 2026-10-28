import { useState } from "react";
import styles from "./edit-lyrics-modal.module.css";
import { useModalStore } from "@/shared/store/modalStore";
import { invoke } from "@tauri-apps/api/core";
import { Song } from "@/shared/types";

interface EditLyricsModalProps {
  song: Song;
  initialLyrics: string;
  initialSource?: string | null;
  onSuccess: (lyrics: string, source: string) => void;
}

export const EditLyricsModal = ({
  song,
  initialLyrics,
  initialSource,
  onSuccess,
}: EditLyricsModalProps) => {
  const [content, setContent] = useState(initialLyrics);
  const [loading, setLoading] = useState(false);
  const closeModal = useModalStore((state) => state.closeModal);

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!song.id) return;

    try {
      setLoading(true);
      const source = initialSource || "Custom";
      const result = await invoke<{ lyrics: string; source: string }>(
        "save_lyrics",
        {
          id: song.id,
          lyrics: content,
          source,
        },
      );
      onSuccess(result.lyrics, result.source);
      closeModal();
    } catch (err) {
      console.error("Failed to save lyrics:", err);
    } finally {
      setLoading(false);
    }
  };

  const artistDisplay =
    song.album_artist || song.artists.join(", ") || "Unknown Artist";

  return (
    <form className={styles.container} onSubmit={handleSave}>
      <div className={styles.header}>
        <h2 className={styles.title}>Edit Lyrics</h2>
        <p className={styles.subtitle}>
          {song.title} — {artistDisplay}
        </p>
      </div>

      <textarea
        className={styles.textarea}
        value={content}
        onChange={(e) => setContent(e.target.value)}
        placeholder="Paste or write lyrics here...&#10;[00:12.34] Timed lyrics in LRC format are supported!"
        rows={15}
        autoFocus
      />

      <p className={styles.tip}>
        Tip: Supports timed LRC format ([mm:ss.xx] line) or plain untimed lines.
      </p>

      <div className={styles.actions}>
        <button
          type="button"
          className={styles.cancelButton}
          onClick={closeModal}
        >
          Cancel
        </button>
        <button type="submit" className={styles.saveButton} disabled={loading}>
          {loading ? "Saving..." : "Save"}
        </button>
      </div>
    </form>
  );
};
