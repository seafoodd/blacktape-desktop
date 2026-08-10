import styles from "./search-results.module.css";
import { ItemType, useLibraryStore } from "@/shared/store/libraryStore.ts";
import { formatCompactNumber } from "@/shared/lib/number.ts";
import { BiDownload, BiPlayCircle } from "react-icons/bi";
import { invoke } from "@tauri-apps/api/core";
import clsx from "clsx";

const SearchResults = () => {
  const {
    committedResults,
    submittedQuery,
    activeCategory,
    activePlatform,
    setActiveCategory,
    setActivePlatform,
  } = useLibraryStore();

  const filteredResults = committedResults.filter((item) => {
    const matchesCategory =
      activeCategory === "All" || item.item_type === activeCategory;
    const matchesPlatform =
      activePlatform === "All" ||
      item.platform?.toLowerCase() === activePlatform.toLowerCase();
    return matchesCategory && matchesPlatform;
  });

  const artists = filteredResults.filter(
    (item) => item.item_type === ItemType.Artist,
  );
  const albums = filteredResults.filter(
    (item) => item.item_type === ItemType.Album,
  );
  const tracks = filteredResults.filter(
    (item) => item.item_type === ItemType.Track,
  );

  if (committedResults.length === 0 && submittedQuery) {
    return (
      <div className={styles.emptyContainer}>
        <p>No results found for "{submittedQuery}"</p>
      </div>
    );
  }

  return (
    <div className={styles.container}>
      <button
        onClick={() => {
          invoke("launch_youtube_login", { forceVisible: true });
        }}
      >
        launch_youtube_login
      </button>
      <button
        onClick={() => {
          invoke("check_auth_status").then((r) => {
            console.log(r);
          });
        }}
      >
        get_connected_account
      </button>
      <header className={styles.header}>
        <h2>
          Results for <span>"{submittedQuery}"</span>
        </h2>

        {/* Inline Quick Filters for better UX */}
        <div className={styles.quickFilters}>
          <button
            className={clsx(
              styles.pill,
              activeCategory === "All" && styles.pillActive,
            )}
            onClick={() => setActiveCategory("All")}
          >
            All
          </button>
          <button
            className={clsx(
              styles.pill,
              activeCategory === ItemType.Artist && styles.pillActive,
            )}
            onClick={() => setActiveCategory(ItemType.Artist)}
          >
            Artists
          </button>
          <button
            className={clsx(
              styles.pill,
              activeCategory === ItemType.Album && styles.pillActive,
            )}
            onClick={() => setActiveCategory(ItemType.Album)}
          >
            Albums
          </button>
          <button
            className={clsx(
              styles.pill,
              activeCategory === ItemType.Track && styles.pillActive,
            )}
            onClick={() => setActiveCategory(ItemType.Track)}
          >
            Tracks
          </button>

          <div className={styles.divider} />

          <button
            className={clsx(
              styles.pill,
              activePlatform === "All" && styles.pillActive,
            )}
            onClick={() => setActivePlatform("All")}
          >
            All Platforms
          </button>
          <button
            className={clsx(
              styles.pill,
              activePlatform === "Youtube" && styles.pillActive,
            )}
            onClick={() => setActivePlatform("Youtube")}
          >
            YouTube
          </button>
          <button
            className={clsx(
              styles.pill,
              activePlatform === "Bandcamp" && styles.pillActive,
            )}
            onClick={() => setActivePlatform("Bandcamp")}
          >
            Bandcamp
          </button>
        </div>
      </header>

      <div className={styles.content}>
        {/* ARTISTS */}
        {artists.length > 0 && (
          <section className={styles.section}>
            <h3>Artists</h3>
            <div className={styles.grid}>
              {artists.map((artist, idx) => (
                <div key={`artist-${idx}`} className={styles.sleekCard}>
                  <div className={styles.imageWrapper}>
                    <img
                      className={clsx(styles.cover, styles.artistCover)}
                      src={artist.img}
                      referrerPolicy="no-referrer"
                      alt={artist.name}
                    />
                  </div>
                  <div className={styles.cardInfo}>
                    <p className={clsx(styles.title, "truncate")}>
                      {artist.name}
                    </p>
                    <p className={styles.subtitle}>
                      {artist.subscriber_count
                        ? `${formatCompactNumber(artist.subscriber_count)} followers`
                        : "Artist"}
                    </p>
                  </div>
                  <span className={styles.miniBadge}>{artist.platform}</span>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* ALBUMS */}
        {albums.length > 0 && (
          <section className={styles.section}>
            <h3>Albums</h3>
            <div className={styles.grid}>
              {albums.map((album, idx) => (
                <div key={`album-${idx}`} className={styles.sleekCard}>
                  <div className={styles.imageWrapper}>
                    <img
                      className={styles.cover}
                      src={album.img}
                      referrerPolicy="no-referrer"
                      alt={album.name}
                    />
                    <div className={styles.imageOverlay}>
                      <button
                        className={styles.downloadBtn}
                        onClick={(e) => {
                          e.stopPropagation();
                          invoke("download", {
                            platform: album.platform,
                            downloadType: album.item_type,
                            url: album.item_url_path,
                          }).catch(console.error);
                        }}
                      >
                        <BiDownload size={24} />
                      </button>
                    </div>
                  </div>
                  <div className={styles.cardInfo}>
                    <p className={clsx(styles.title, "truncate")}>
                      {album.name}
                    </p>
                    <p className={clsx(styles.subtitle, "truncate")}>
                      {album.band_name}
                    </p>
                  </div>
                  <span className={styles.miniBadge}>{album.platform}</span>
                </div>
              ))}
            </div>
          </section>
        )}

        {/* TRACKS */}
        {tracks.length > 0 && (
          <section className={styles.section}>
            <h3>Tracks</h3>
            <div className={styles.trackList}>
              {tracks.map((track, idx) => (
                <div key={`track-${idx}`} className={styles.trackRow}>
                  <div className={styles.trackImageWrapper}>
                    <img
                      className={styles.trackCover}
                      src={track.img}
                      referrerPolicy="no-referrer"
                      alt={track.name}
                    />
                    <div className={styles.trackOverlay}>
                      <BiPlayCircle size={24} />
                    </div>
                  </div>
                  <div className={styles.trackInfo}>
                    <p className={clsx(styles.trackTitle, "truncate")}>
                      {track.name}
                    </p>
                    <p className={clsx(styles.trackArtist, "truncate")}>
                      {track.band_name}
                    </p>
                  </div>
                  <span className={styles.miniBadge}>{track.platform}</span>
                  <button
                    className={styles.trackDownloadBtn}
                    onClick={() =>
                      invoke("download", {
                        platform: track.platform,
                        downloadType: track.item_type,
                        url: track.item_url_path,
                      }).catch(console.error)
                    }
                  >
                    <BiDownload size={20} />
                  </button>
                </div>
              ))}
            </div>
          </section>
        )}
      </div>
    </div>
  );
};

export default SearchResults;
