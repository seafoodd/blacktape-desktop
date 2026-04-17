import styles from "./artist-albums.module.css";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderAlbumImage from "@/assets/react.svg";
import { formatDuration } from "@/shared/lib/time.ts";
import clsx from "clsx";
import { HiOutlineMusicNote } from "react-icons/hi";

const ArtistAlbums = () => {
  const { selectedTab, albums } = useLibraryStore();
  const { startPlayback, currentSong, isPlaying } = useAudioStore();

  const handlePlaySong = (clickedSongId: number) => {
    // Flatten all songs from all albums into one continuous array
    const allSongs = albums.flatMap((album) => album.songs);

    // Find where the clicked song is in this master list
    const currentIndex = allSongs.findIndex(
      (song) => song.id === clickedSongId,
    );

    if (currentIndex === -1) return; // Safety check

    // Everything before the clicked song (chronological order)
    const historyIds = allSongs.slice(0, currentIndex).map((s) => s.id);

    // Everything after the clicked song
    const queueIds = allSongs.slice(currentIndex + 1).map((s) => s.id);

    // Send it to the Tauri backend!
    startPlayback(clickedSongId, queueIds, historyIds);
  };

  return (
    <div className={styles.container}>
      <h1 className={styles.artistName}>{selectedTab}</h1>
      <div className={styles.albums}>
        {albums.map((album) => (
          <section className={styles.albumBlock} key={album.title}>
            <div className={styles.albumBlockLeft}>
              {album.cover_url ? (
                <img
                  className={styles.albumCover}
                  src={convertFileSrc(album.cover_url)}
                  alt={album.title}
                />
              ) : (
                <img
                  className={styles.albumCover}
                  src={placeholderAlbumImage}
                  alt={album.title}
                />
              )}
            </div>
            <div className={styles.albumBlockRight}>
              <h2 className={styles.albumTitle}>{album.title}</h2>
              <ul className={styles.songs}>
                {album.songs.map((song) => (
                  <button
                    className={clsx(styles.song, {
                      [styles.playing]:
                        currentSong && song.id == currentSong.id,
                    })}
                    onClick={() => handlePlaySong(song.id)}
                    key={song.id}
                  >
                    <div className={styles.songLeft}>
                      {isPlaying && currentSong && song.id == currentSong.id ? (
                        <HiOutlineMusicNote className={styles.playingIcon} />
                      ) : (
                        <div className={styles.songTrackNumber}>
                          <>
                            {song.track_number &&
                              String(song.track_number).padStart(2, "0")}
                          </>
                        </div>
                      )}
                      <div className={styles.songTitle}>{song.title}</div>
                    </div>
                    <div className={styles.songDuration}>
                      {formatDuration(song.duration_ms)}
                    </div>
                  </button>
                ))}
              </ul>
            </div>
          </section>
        ))}
      </div>
    </div>
  );
};

export default ArtistAlbums;
