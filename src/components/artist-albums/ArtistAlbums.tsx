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
    const allSongs = albums.flatMap((album) => album.songs);

    const currentIndex = allSongs.findIndex(
      (song) => song.id === clickedSongId,
    );
    if (currentIndex === -1) return;

    const historyIds = allSongs.slice(0, currentIndex).map((s) => s.id);
    const queueIds = allSongs.slice(currentIndex + 1).map((s) => s.id);

    startPlayback(clickedSongId, queueIds, historyIds).catch((e) =>
      console.log("startPlayback error: ", e),
    );
  };

  // useEffect(() => {
  //   setSelectedTab("")
  // }, []);

  if (!selectedTab) {
    return;
  }

  return (
    <div className={styles.container}>
      <h1 className={clsx(styles.artistName, "truncate")}>{selectedTab}</h1>
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
              <h2 className={clsx(styles.albumTitle, "truncate")}>{album.title}</h2>
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
                      <div className={clsx(styles.songTitle, "truncate")}>{song.title}</div>
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
