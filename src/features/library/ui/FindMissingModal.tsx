import { useEffect, useState } from "react";
import { useModalStore } from "@/shared/store/modalStore.ts";
import {
  findAlbumCandidates,
  linkAlbumSource,
  type AlbumCandidate,
} from "@/shared/lib/libraryIndex.ts";
import { useMissingTracksStore } from "@/features/library/model/missingTracksStore.ts";
import { candidateMeta } from "@/features/library/ui/missing-track-row/missingTrackRow.ts";
import styles from "./find-missing-modal.module.css";

interface FindMissingModalProps {
  albumArtist: string;
  /** Title of the library album. */
  album: string;
}

/** Looks for the platform album of a library album that has no stored source and links the one the user picks. */
const FindMissingModal = ({ albumArtist, album }: FindMissingModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const [candidates, setCandidates] = useState<AlbumCandidate[] | null>(null);
  const [linking, setLinking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    findAlbumCandidates(albumArtist, album)
      .then((found) => {
        if (!cancelled) setCandidates(found);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setCandidates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [albumArtist, album]);

  const handlePick = async (candidate: AlbumCandidate) => {
    setLinking(true);
    setError(null);
    try {
      await linkAlbumSource(albumArtist, album, candidate.platform, candidate.url);
      await useMissingTracksStore.getState().loadSources(albumArtist);
      useMissingTracksStore.getState().show(album);
      closeModal();
    } catch (e) {
      setError(String(e));
      setLinking(false);
    }
  };

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Find missing songs</h2>
      <span className={styles.text}>
        {candidates === null
          ? `Looking for "${album}" on the platforms...`
          : candidates.length > 0
            ? `Which one is "${album}"?`
            : "No matching album was found."}
      </span>
      {candidates && candidates.length > 0 && (
        <div className={styles.list}>
          {candidates.map((candidate) => (
            <button
              key={candidate.url}
              className={styles.candidate}
              disabled={linking}
              onClick={() => handlePick(candidate)}
            >
              {candidate.cover_url ? (
                <img className={styles.cover} src={candidate.cover_url} alt="" />
              ) : (
                <div className={styles.cover} />
              )}
              <div className={styles.info}>
                <span className={styles.name}>{candidate.title}</span>
                <span className={styles.meta}>{candidate.artist}</span>
                <span className={styles.meta}>{candidateMeta(candidate)}</span>
              </div>
            </button>
          ))}
        </div>
      )}
      {error && <span className={styles.error}>{error}</span>}
      <div className={styles.buttons}>
        <button className={styles.cancelButton} onClick={closeModal} disabled={linking}>
          Cancel
        </button>
      </div>
    </div>
  );
};

export default FindMissingModal;
