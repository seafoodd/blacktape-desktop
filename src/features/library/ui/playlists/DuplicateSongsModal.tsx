import { useModalStore } from "@/shared/store/modalStore.ts";
import styles from "./duplicate-songs-modal.module.css";

export type DuplicateChoice = "all" | "skip" | "cancel";

interface DuplicateSongsModalProps {
  playlistName?: string;
  /** Songs being added. */
  total: number;
  /** How many of them are already in the playlist. */
  duplicates: number;
  onChoose: (choice: DuplicateChoice) => void;
}

export const DuplicateSongsModal = ({
  playlistName,
  total,
  duplicates,
  onChoose,
}: DuplicateSongsModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const allDuplicates = duplicates === total;

  const choose = (choice: DuplicateChoice) => {
    onChoose(choice);
    closeModal();
  };

  const target = playlistName ? `"${playlistName}"` : "this playlist";
  const message =
    total === 1
      ? `This song is already in ${target}. Add it again?`
      : allDuplicates
        ? `All ${total} songs are already in ${target}. Add them again?`
        : `${duplicates} of ${total} songs are already in ${target}. Add them again?`;

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Already in playlist</h2>
      <p className={styles.text}>{message}</p>
      <div className={styles.buttons}>
        <button className={styles.button} onClick={() => choose("cancel")}>
          No
        </button>
        {!allDuplicates && (
          <button className={styles.button} onClick={() => choose("skip")}>
            Skip duplicates
          </button>
        )}
        <button
          className={styles.primaryButton}
          onClick={() => choose("all")}
          autoFocus
        >
          Yes, add
        </button>
      </div>
    </div>
  );
};
