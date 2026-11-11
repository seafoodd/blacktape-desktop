import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { refreshLibrary } from "@/shared/lib/library/libraryRefresh.ts";
import { Song } from "@/shared/types.ts";
import styles from "./delete-song-modal.module.css";

interface FetchLyricsModalProps {
  song: Song;
}

type State =
  | { kind: "loading" }
  | { kind: "done"; source: string }
  | { kind: "error"; message: string };

const FetchLyricsModal = ({ song }: FetchLyricsModalProps) => {
  const closeModal = useModalStore((s) => s.closeModal);
  const [state, setState] = useState<State>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    invoke<{ lyrics: string; source: string }>("refetch_lyrics", {
      id: song.id,
      provider: null,
    })
      .then(async (res) => {
        if (cancelled) return;
        await refreshLibrary().catch(() => {});
        setState({ kind: "done", source: res.source });
      })
      .catch((e) => {
        if (!cancelled) setState({ kind: "error", message: String(e) });
      });
    return () => {
      cancelled = true;
    };
  }, [song.id]);

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Fetch Lyrics</h2>
      <span className={styles.text}>{song.title}</span>
      {state.kind === "loading" && (
        <span className={styles.options}>Searching providers...</span>
      )}
      {state.kind === "done" && (
        <span className={styles.options}>Saved lyrics from {state.source}.</span>
      )}
      {state.kind === "error" && (
        <span className={styles.error}>{state.message}</span>
      )}
      <div className={styles.buttons}>
        <button className={styles.cancelButton} onClick={closeModal}>
          {state.kind === "loading" ? "Cancel" : "Close"}
        </button>
      </div>
    </div>
  );
};

export default FetchLyricsModal;
