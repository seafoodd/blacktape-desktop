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
  const { play, currentSong, isPlaying } = useAudioStore();

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
                    onClick={() => play(song.id)}
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
