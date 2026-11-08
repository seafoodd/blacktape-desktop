import { useMemo } from "react";
import styles from "./artist-albums.module.css";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { useAudioStore } from "@/shared/store/audioStore.ts";
import { coverSrc } from "@/shared/lib/cover";
import placeholderAlbumImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import clsx from "clsx";
import { Song } from "@/shared/types.ts";
import { decodeHtmlEntities } from "@/shared/lib/html.ts";
import SongListItem from "@/features/library/ui/song-list-item/SongListItem.tsx";

const ArtistAlbums = () => {
  const { selectedTab, albums, activeDownloads } = useLibraryStore();
  const { startPlayback } = useAudioStore();

  const activeArtistDownloads = useMemo(() => {
    if (!selectedTab) return [];
    return Object.values(activeDownloads).filter(
      (d) =>
        (d.status === "downloading" ||
          d.status === "idle" ||
          d.status === "processing") &&
        d.artistName.trim().toLowerCase() === selectedTab.trim().toLowerCase(),
    );
  }, [activeDownloads, selectedTab]);

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
    return null;
  }

  return (
    <div className={styles.container}>
      <h1 className={clsx(styles.artistName, "truncate")}>{selectedTab}</h1>
      <div className={styles.albums}>
        {/* Downloading Albums */}
        {activeArtistDownloads.map((download) => (
          <section
            className={clsx(styles.albumBlock, styles.downloadingAlbumBlock)}
            key={`download-${download.id}`}
          >
            <div className={styles.albumBlockLeft}>
              <div className={styles.downloadingCoverWrapper}>
                <img loading="lazy" decoding="async"
                  src={
                    download.coverUrl
                      ? download.coverUrl.startsWith("http")
                        ? download.coverUrl
                        : coverSrc(download.coverUrl, 200)
                      : placeholderAlbumImage
                  }
                  referrerPolicy="no-referrer"
                  onError={(e) => {
                    if (e.currentTarget.src !== placeholderAlbumImage) {
                      e.currentTarget.src = placeholderAlbumImage;
                    }
                  }}
                  className={styles.playlistImage}
                  alt={download.albumTitle}
                />
                <div className={styles.downloadingCoverBadge}>
                  Downloading {download.overallPercent}%
                </div>
              </div>
            </div>
            <div className={styles.albumBlockRight}>
              <div className={styles.downloadingHeader}>
                <h2 className={clsx(styles.albumTitle, "truncate")}>
                  {decodeHtmlEntities(download.albumTitle)}
                </h2>
                <span className={styles.downloadingOverallBadge}>
                  {download.overallPercent}%
                </span>
              </div>
              <ul className={styles.songs}>
                {download.tracks && download.tracks.length > 0 ? (
                  download.tracks.map((track, trackIdx) => (
                    <li
                      className={clsx(styles.song, styles.downloadingSong)}
                      key={`track-${trackIdx}`}
                    >
                      <div className={styles.songLeft}>
                        <div className={styles.songTrackNumber}>
                          {String(track.trackNumber || trackIdx + 1).padStart(
                            2,
                            "0",
                          )}
                        </div>
                        <div className={clsx(styles.songTitle, "truncate")}>
                          {decodeHtmlEntities(track.title)}
                        </div>
                      </div>
                      <div className={styles.songRight}>
                        <div className={styles.trackPercentage}>
                          {track.status === "completed" ||
                          track.percent >= 100 ? (
                            <span className={styles.percentCompleted}>
                              100%
                            </span>
                          ) : track.percent > 0 ? (
                            <span className={styles.percentActive}>
                              {track.percent}%
                            </span>
                          ) : (
                            <span className={styles.percentPending}>0%</span>
                          )}
                        </div>
                      </div>
                    </li>
                  ))
                ) : (
                  <li className={clsx(styles.song, styles.downloadingSong)}>
                    <div className={styles.songLeft}>
                      <div className={styles.songTrackNumber}>01</div>
                      <div className={clsx(styles.songTitle, "truncate")}>
                        {download.message || "Preparing download..."}
                      </div>
                    </div>
                    <div className={styles.songRight}>
                      <div className={styles.trackPercentage}>
                        <span className={styles.percentPending}>
                          {download.overallPercent}%
                        </span>
                      </div>
                    </div>
                  </li>
                )}
              </ul>
            </div>
          </section>
        ))}

        {/* Existing Downloaded Albums */}
        {albums.map((album, index) => (
          <section className={styles.albumBlock} key={index}>
            <div className={styles.albumBlockLeft}>
              <img loading="lazy" decoding="async"
                src={
                  album.cover_url
                    ? coverSrc(album.cover_url, 200)
                    : placeholderAlbumImage
                }
                referrerPolicy="no-referrer"
                onError={(e) => {
                  if (e.currentTarget.src !== placeholderAlbumImage) {
                    e.currentTarget.src = placeholderAlbumImage;
                  }
                }}
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
