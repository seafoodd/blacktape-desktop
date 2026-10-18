import styles from "./artist-albums.module.css";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderAlbumImage from "@/assets/react.svg";
import clsx from "clsx";
import { Song } from "@/shared/types.ts";
import SongListItem from "@/features/library/ui/song-list-item/SongListItem.tsx";

const ArtistAlbums = () => {
  const { selectedTab, albums } = useLibraryStore();
  const { startPlayback } = useAudioStore();

  const handlePlaySong = (clickedSongId: number) => {
    const allSongs = albums.flatMap((album) => album.songs);

    const currentIndex = allSongs.findIndex(
      (song) => song.id === clickedSongId,
    );
    if (currentIndex === -1) return;

    // const historyIds = allSongs.slice(0, currentIndex).map((s) => s.id);
    const queueIds = allSongs.map((s) => s.id);

    startPlayback(queueIds, currentIndex, "library").catch((e) =>
      console.log("startPlayback error: ", e),
    );
  };

  if (!selectedTab) {
    return;
  }

  return (
    <div className={styles.container}>
      <h1 className={clsx(styles.artistName, "truncate")}>{selectedTab}</h1>
      <div className={styles.albums}>
        {albums.map((album, index) => (
          <section className={styles.albumBlock} key={index}>
            <div className={styles.albumBlockLeft}>
              <img
                src={
                  album.cover_url
                    ? convertFileSrc(album.cover_url)
                    : placeholderAlbumImage
                }
                className={styles.playlistImage}
                alt={album.title}
              />
            </div>
            <div className={styles.albumBlockRight}>
              <h2 className={clsx(styles.albumTitle, "truncate")}>
                {album.title}
              </h2>
              <ul className={styles.songs}>
                {album.songs.map((song: Song) => (
                  <SongListItem
                    key={song.id}
                    song={song}
                    onPlay={handlePlaySong}
                    playbackContext={"library"}
                  />
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
