import { useCallback, useEffect, useMemo } from "react";
import styles from "./favorites.module.css";
import { useFavoritesStore } from "@/features/favorites/model/favoritesStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { findPlayingRow } from "@/shared/lib/ui/playingRow.ts";
import { FaCirclePause, FaCirclePlay, FaShuffle } from "react-icons/fa6";
import { MdFavorite, MdFavoriteBorder } from "react-icons/md";
import clsx from "clsx";
import SongGrid from "@/features/library/ui/song-grid/SongGrid.tsx";

const formatFavoritesStats = (trackCount: number, durationMs: number) => {
  const hours = Math.floor(durationMs / (1000 * 60 * 60));
  const minutes = Math.floor((durationMs % (1000 * 60 * 60)) / (1000 * 60));
  const timeString =
    hours > 0 ? `${hours} hr ${minutes} min` : `${minutes} min`;
  return `${trackCount} ${trackCount === 1 ? "song" : "songs"}, ${timeString}`;
};

const Favorites = () => {
  const { favoriteSongs, fetchFavoriteSongs, loading } = useFavoritesStore();

  const {
    startPlayback,
    isPlaying,
    shuffleMode,
    playbackContext,
    togglePlay,
    toggleShuffle,
    currentSong,
    sourceIndex,
  } = useAudioStore();

  useEffect(() => {
    fetchFavoriteSongs().catch((e) => {
      console.error("fetchFavoriteSongs error:", e);
    });
  }, [fetchFavoriteSongs]);

  const handlePlayIndex = useCallback(
    (currentIndex: number) => {
      if (currentIndex < 0 || currentIndex >= favoriteSongs.length) return;

      const queueIds = favoriteSongs.map((s) => s.id);

      startPlayback(queueIds, currentIndex, "favorites").catch((e) =>
        console.error("startPlayback error: ", e),
      );
    },
    [favoriteSongs, startPlayback],
  );

  const isFavoritesContext = playbackContext === "favorites";
  const playingIndex = findPlayingRow(
    favoriteSongs,
    currentSong?.id,
    isFavoritesContext ? sourceIndex : null,
  );

  const isFavoritesPlaying = isFavoritesContext && isPlaying;
  const isFavoritesShuffled = shuffleMode;

  const handlePlayButton = useCallback(() => {
    if (favoriteSongs.length === 0) return;

    if (!isFavoritesContext) {
      handlePlayIndex(0);
      return;
    }
    togglePlay().catch((e) => console.error("togglePlay error: ", e));
  }, [isFavoritesContext, favoriteSongs, togglePlay, handlePlayIndex]);

  const totalDurationMs = useMemo(
    () => favoriteSongs.reduce((sum, s) => sum + (s.duration_ms || 0), 0),
    [favoriteSongs],
  );

  return (
    <div className={styles.container}>
      <div className={styles.heroBlock}>
        <div className={styles.heroContent}>
          <div className={styles.favoritesCoverHero}>
            <MdFavorite size={96} />
          </div>
          <div className={styles.info}>
            <span className={styles.badge}>Collection</span>
            <span className={clsx(styles.title, "truncate")}>Favorites</span>
            <p className={clsx(styles.description, "truncate")}>
              Your favorite tracks in one place
            </p>
            <span className={styles.stats}>
              {formatFavoritesStats(favoriteSongs.length, totalDurationMs)}
            </span>
          </div>
        </div>

        <div className={styles.controls}>
          <button
            className={styles.controlsButton}
            onClick={handlePlayButton}
            aria-label={isFavoritesPlaying ? "Pause favorites" : "Play favorites"}
            disabled={favoriteSongs.length === 0}
          >
            {isFavoritesPlaying ? (
              <FaCirclePause
                className={styles.controlsButtonIconPlay}
                size={48}
              />
            ) : (
              <FaCirclePlay
                className={styles.controlsButtonIconPlay}
                size={48}
              />
            )}
          </button>
          <button
            className={clsx(
              styles.controlsButton,
              styles.shuffle,
              isFavoritesShuffled && styles.active,
            )}
            onClick={toggleShuffle}
            aria-label="Shuffle favorites"
          >
            <FaShuffle className={styles.controlsButtonIcon} size={32} />
          </button>
        </div>
      </div>

      <div className={styles.songList}>
        {favoriteSongs.length === 0 && !loading ? (
          <div className={styles.emptyNotice}>
            <MdFavoriteBorder size={48} />
            <span className={styles.emptyTitle}>
              Songs you favorite will appear here
            </span>
            <span className={styles.emptyDescription}>
              Save songs you love by clicking the heart icon on any track, in the
              player controls, or dragging them into Favorites.
            </span>
          </div>
        ) : (
          <SongGrid
            songs={favoriteSongs}
            onPlay={handlePlayIndex}
            playbackContext="favorites"
            playingIndex={playingIndex}
          />
        )}
      </div>
    </div>
  );
};

export default Favorites;
