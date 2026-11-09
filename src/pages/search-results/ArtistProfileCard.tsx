import clsx from "clsx";
import type { SearchSuggestion } from "@/shared/store/libraryStore.ts";
import type { ArtistProfile } from "@/shared/lib/searchFull.ts";
import { formatCompactNumber } from "@/shared/lib/number.ts";
import { AlbumCard, Cover, TrackRow } from "./ResultItems.tsx";
import { PlatformBadge } from "./PlatformBadge.tsx";
import {
  formatAudience,
  formatSubscribers,
  joinMeta,
  type DownloadMap,
} from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";

const TOP_SONGS_LIMIT = 5;

interface ArtistProfileCardProps {
  /** The artist search item the card belongs to (fallback avatar, name and audience). */
  artist: SearchSuggestion;
  /** Loaded profile, or null while it loads. */
  profile: ArtistProfile | null;
  downloads: DownloadMap;
}

const ProfileSkeleton = () => (
  <div className={styles.profileSkeleton} aria-hidden="true">
    <div className={clsx(styles.skeleton, styles.skeletonAvatarLarge)} />
    <div className={styles.profileInfo}>
      <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineLong)} />
      <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineShort)} />
    </div>
  </div>
);

/**
 * Meta line parts of the card: subscribers, monthly audience, location. The search row's count is
 * only used for the metric it actually measures (never relabelled).
 */
export function profileMetaParts(
  profile: Pick<ArtistProfile, "subscribers" | "monthly_audience" | "location"> &
    Partial<Pick<ArtistProfile, "followers">>,
  artist: Pick<SearchSuggestion, "subscriber_count" | "audience_kind">,
): string[] {
  const subscribers = formatSubscribers(profile.subscribers);
  const monthly =
    typeof profile.monthly_audience === "number" && profile.monthly_audience > 0
      ? `${formatCompactNumber(profile.monthly_audience)} monthly audience`
      : null;
  const followers =
    typeof profile.followers === "number" && profile.followers > 0
      ? `${formatCompactNumber(profile.followers)} followers`
      : null;
  const fromRow = formatAudience(artist.subscriber_count, artist.audience_kind);
  return [
    subscribers ?? (artist.audience_kind === "subscribers" ? fromRow : null),
    followers ?? (artist.audience_kind === "followers" ? fromRow : null),
    monthly ?? (artist.audience_kind === "monthly_audience" ? fromRow : null),
    profile.location,
  ].filter((p): p is string => typeof p === "string" && p !== "");
}

export const ArtistProfileCard = ({ artist, profile, downloads }: ArtistProfileCardProps) => {
  if (!profile) {
    return (
      <section className={styles.profile} aria-label={`Loading ${artist.name} profile`}>
        <ProfileSkeleton />
      </section>
    );
  }

  const name = profile.name || artist.name;
  const avatar = profile.avatar ?? artist.img;
  const meta = joinMeta(profileMetaParts(profile, artist));
  const discography = [...profile.albums, ...profile.singles];
  const topSongs = profile.top_songs.slice(0, TOP_SONGS_LIMIT);
  const hasBody = topSongs.length > 0 || discography.length > 0;

  return (
    <section className={styles.profile} aria-label={`${name} profile`}>
      <div className={styles.profileHead}>
        {profile.banner && (
          <img
            className={styles.profileBanner}
            src={profile.banner}
            alt=""
            referrerPolicy="no-referrer"
            loading="lazy"
          />
        )}
        <Cover src={avatar} alt={name} className={styles.profileAvatar} />
        <div className={styles.profileInfo}>
          <div className={styles.profileTitleRow}>
            <h3 className={clsx(styles.profileName, styles.ellipsis)} title={name}>
              {name}
            </h3>
            <PlatformBadge platform={profile.platform ?? artist.platform} />
          </div>
          {meta && (
            <p className={clsx(styles.profileMeta, styles.ellipsis)} title={meta}>
              {meta}
            </p>
          )}
          {profile.description && (
            <p className={styles.profileDesc} title={profile.description}>
              {profile.description}
            </p>
          )}
        </div>
      </div>

      {hasBody && (
        <div className={styles.profileBody}>
          {topSongs.length > 0 && (
            <div className={styles.profileCol}>
              <h4 className={styles.subhead}>Top songs</h4>
              <div className={styles.trackList}>
                {topSongs.map((song, i) => (
                  <TrackRow
                    key={`${i}-${song.item_url_path}`}
                    item={song}
                    progress={downloads.get(song.item_url_path)}
                    compact
                  />
                ))}
              </div>
            </div>
          )}

          {discography.length > 0 && (
            <div className={styles.profileCol}>
              <h4 className={styles.subhead}>Discography</h4>
              <div className={styles.discoRow}>
                {discography.map((item, i) => (
                  <div key={`${i}-${item.item_url_path}`} className={styles.discoItem}>
                    <AlbumCard
                      item={item}
                      progress={downloads.get(item.item_url_path)}
                      compact
                    />
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </section>
  );
};
