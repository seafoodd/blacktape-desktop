import styles from "@/pages/artist-albums/artist-albums.module.css";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderAlbumImage from "@/assets/react.svg";
import clsx from "clsx";
import { HiDotsHorizontal } from "react-icons/hi";
import { formatDuration } from "@/shared/lib/time.ts";
import { Platform, Song } from "@/shared/types.ts";

export type OnlineAlbum = {
  title: string;
  album_artist: string;
  platform: Platform;
  cover_url: string | null;
  songs: Song[];
};

const OnlineAlbumPreview = (album: OnlineAlbum) => {
  return (
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
            <div
              className={styles.song}
              // onClick={() => handlePlaySong(song.id)}
              // onContextMenu={(e) => handleOpenContextMenu(e, song)}
              key={song.id}
              role="button"
              tabIndex={0}
            >
              <div className={styles.songLeft}>
                <div className={styles.songTrackNumber}>
                  <>
                    {song.track_number &&
                      String(song.track_number).padStart(2, "0")}
                  </>
                </div>

                <div className={clsx(styles.songTitle, "truncate")}>
                  {song.title}
                </div>
              </div>
              <div className={styles.songRight}>
                <div className={styles.songDuration}>
                  {formatDuration(song.duration_ms)}
                </div>
                <button
                  className={styles.threeDotButton}
                  // onClick={(e) => handleOpenContextMenu(e, song, true)}
                  aria-label="Song options"
                >
                  <HiDotsHorizontal className={styles.threeDotIcon} size={24} />
                </button>
              </div>
            </div>
          ))}
        </ul>
      </div>
    </section>
  );
};

export default OnlineAlbumPreview;
