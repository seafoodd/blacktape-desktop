import { useEffect, useState } from "react";
import { useModalStore } from "@/shared/store/modalStore.ts";
import {
  findAlbumCandidates,
  type AlbumCandidate,
} from "@/shared/lib/libraryIndex.ts";
import { explainFailure } from "@/features/library/model/downloadFailure.ts";
import { sameUrl } from "@/shared/lib/urlKey.ts";
import FindAlternativeModal from "@/features/library/ui/FindAlternativeModal.tsx";
import { useMissingTracksStore } from "@/features/library/model/missingTracksStore.ts";
import { ExpectedQualityBadge } from "@/features/download-quality/ui/ExpectedQualityBadge.tsx";
import { candidateMeta } from "@/features/library/ui/missing-track-row/missingTrackRow.ts";
import styles from "./find-missing-modal.module.css";

interface FindMissingModalProps {
  albumArtist: string;
  /** Title of the library album. */
  album: string;
  /** Page url of the album that is linked now: left out of the list (a change of link needs another one). */
  excludeUrl?: string;
  /** A song of the album has no stream on the linked platform: the text says so and offers to find just that song. */
  failedSong?: {
    artist: string;
    title: string;
    platform: string;
    trackNumber: number | null;
    /** The error of the failed download; unknown for a song marked earlier as having no stream. */
    message?: string;
    retry?: () => void;
  };
}

/** Looks for the platform album of a library album that has no stored source and links the one the user picks. */
const FindMissingModal = ({ albumArtist, album, excludeUrl, failedSong }: FindMissingModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);
  const openModal = useModalStore((state) => state.openModal);
  const [candidates, setCandidates] = useState<AlbumCandidate[] | null>(null);
  const [linking, setLinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // the platform album the library album is linked to now (Change linked album)
  const linked = useMissingTracksStore((s) => s.sources[album]);

  const explained = failedSong
    ? explainFailure(failedSong.message ?? "no output file", failedSong.platform)
    : null;

  useEffect(() => {
    let cancelled = false;
    findAlbumCandidates(albumArtist, album)
      .then((found) => {
        if (!cancelled) setCandidates(excludeUrl ? found.filter((c) => !sameUrl(c.url, excludeUrl)) : found);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setCandidates([]);
      });
    return () => {
      cancelled = true;
    };
  }, [albumArtist, album, excludeUrl]);

  const handlePick = async (candidate: AlbumCandidate) => {
    setLinking(true);
    setError(null);
    try {
      await useMissingTracksStore.getState().link(albumArtist, album, candidate.platform, candidate.url);
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
            ? failedSong
            ? `Pick another version of "${album}":`
            : `Which one is "${album}"?`
            : "No matching album was found."}
      </span>
      {linked && (
        <span className={styles.linked}>
          <span className={styles.linkedLabel}>Linked now:</span> {candidateMeta({ platform: linked.platform, year: null })}
          <ExpectedQualityBadge platform={linked.platform} url={linked.source_url} />
        </span>
      )}
      {failedSong && (
        <span className={styles.problem}>
          <strong>"{failedSong.title}" could not be downloaded.</strong> {explained?.reason} {explained?.hint}
        </span>
      )}
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
                <span className={styles.metaRow}>
                  <span className={styles.meta}>{candidateMeta(candidate)}</span>
                  <ExpectedQualityBadge platform={candidate.platform} url={candidate.url} />
                </span>
              </div>
            </button>
          ))}
        </div>
      )}
      {error && <span className={styles.error}>{error}</span>}
      <div className={styles.buttons}>
        {failedSong?.retry && explained?.kind !== "no-stream" && (
          <button
            className={styles.cancelButton}
            disabled={linking}
            onClick={() => {
              failedSong.retry?.();
              closeModal();
            }}
          >
            Try again
          </button>
        )}
        {failedSong && (
          <button
            className={styles.cancelButton}
            disabled={linking}
            onClick={() =>
              openModal(
                <FindAlternativeModal
                  artist={failedSong.artist}
                  title={failedSong.title}
                  excludePlatform={failedSong.platform.toLowerCase()}
                  placement={{
                    album,
                    album_artist: albumArtist,
                    title: failedSong.title,
                    track_number: failedSong.trackNumber,
                  }}
                />,
              )
            }
          >
            Only this song from YouTube / SoundCloud
          </button>
        )}
        <button className={styles.cancelButton} onClick={closeModal} disabled={linking}>
          Cancel
        </button>
      </div>
    </div>
  );
};

export default FindMissingModal;
