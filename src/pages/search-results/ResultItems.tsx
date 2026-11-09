import clsx from "clsx";
import {
  ItemType,
  type DownloadProgress,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type { SearchKind } from "@/shared/lib/searchFull.ts";
import { DownloadButton } from "./DownloadButton.tsx";
import { PreviewButton } from "@/features/preview/ui/PreviewButton.tsx";
import { PlatformBadge } from "./PlatformBadge.tsx";
import {
  formatDurationSecs,
  formatPlays,
  formatAudience,
  trackSubline,
} from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";

/** Cover image; a flat placeholder when the item has no image. */
export const Cover = ({
  src,
  alt,
  className,
}: {
  src: string;
  alt: string;
  className: string;
}) =>
  src ? (
    <img
      className={className}
      src={src}
      alt={alt}
      referrerPolicy="no-referrer"
      loading="lazy"
    />
  ) : (
    <div className={clsx(className, styles.coverPlaceholder)} aria-hidden="true" />
  );

interface TrackRowProps {
  item: SearchSuggestion;
  progress?: DownloadProgress;
  /** Compact rows (artist profile "Top songs") keep the play count out. */
  compact?: boolean;
}

export const TrackRow = ({ item, progress, compact = false }: TrackRowProps) => {
  const plays = compact ? null : formatPlays(item.view_count);
  const subline = trackSubline(item.band_name, item.album_name);
  return (
    <div className={styles.trackRow}>
      <div className={styles.trackCoverBox}>
        <Cover src={item.img} alt={item.name} className={styles.trackCover} />
        <PreviewButton item={item} className={styles.trackPreview} />
      </div>
      <div className={styles.trackInfo}>
        <p className={styles.ellipsis} title={item.name}>
          {item.name}
        </p>
        <p className={clsx(styles.subline, styles.ellipsis)} title={subline}>
          {subline}
        </p>
      </div>
      {typeof item.duration === "number" && (
        <span className={styles.trackDuration}>{formatDurationSecs(item.duration)}</span>
      )}
      {plays && <span className={styles.trackPlays}>{plays}</span>}
      {!compact && <PlatformBadge platform={item.platform} />}
      <DownloadButton item={item} progress={progress} variant="row" />
    </div>
  );
};

interface AlbumCardProps {
  item: SearchSuggestion;
  progress?: DownloadProgress;
  /** Discography tile: title and year only (artist and platform are already known). */
  compact?: boolean;
}

export const AlbumCard = ({ item, progress, compact = false }: AlbumCardProps) => (
  <article className={styles.card}>
    <div className={styles.coverWrap}>
      <Cover src={item.img} alt={item.name} className={styles.cover} />
      <DownloadButton item={item} progress={progress} variant="overlay" />
    </div>
    <div className={styles.cardText}>
      <p className={clsx(styles.cardTitle, styles.ellipsis)} title={item.name}>
        {item.name}
      </p>
      {!compact && (
        <p className={clsx(styles.cardSub, styles.ellipsis)} title={item.band_name}>
          {item.band_name}
        </p>
      )}
      <div className={styles.cardMetaRow}>
        {typeof item.year === "number" && (
          <span className={styles.cardSub}>{item.year}</span>
        )}
        {!compact && <PlatformBadge platform={item.platform} />}
      </div>
    </div>
  </article>
);

export const ArtistCard = ({ item }: { item: SearchSuggestion }) => {
  const audience = formatAudience(item.subscriber_count, item.audience_kind);
  return (
    <article className={clsx(styles.card, styles.artistCard)}>
      <div className={clsx(styles.coverWrap, styles.coverRound)}>
        <Cover src={item.img} alt={item.name} className={styles.cover} />
      </div>
      <div className={clsx(styles.cardText, styles.centered)}>
        <p className={clsx(styles.cardTitle, styles.ellipsis)} title={item.name}>
          {item.name}
        </p>
        {audience && <p className={clsx(styles.cardSub, styles.ellipsis)}>{audience}</p>}
        <PlatformBadge platform={item.platform} />
      </div>
    </article>
  );
};

/** Placeholder rows and cards while a section loads. */
export const SectionSkeleton = ({ kind }: { kind: SearchKind }) => {
  const count = 6;
  if (kind === ItemType.Track) {
    return (
      <div className={styles.trackList} aria-hidden="true">
        {Array.from({ length: count }, (_, i) => (
          <div key={i} className={styles.skeletonRow}>
            <div className={clsx(styles.skeleton, styles.skeletonCover)} />
            <div className={styles.skeletonTextCol}>
              <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineLong)} />
              <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineShort)} />
            </div>
          </div>
        ))}
      </div>
    );
  }
  const round = kind === ItemType.Artist;
  return (
    <div className={round ? styles.artistGrid : styles.albumGrid} aria-hidden="true">
      {Array.from({ length: count }, (_, i) => (
        <div key={i} className={clsx(styles.card, round && styles.artistCard)}>
          <div className={clsx(styles.skeleton, styles.coverWrap, round && styles.coverRound)} />
          <div className={clsx(styles.cardText, round && styles.centered)}>
            <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineLong)} />
            <div className={clsx(styles.skeleton, styles.skeletonLine, styles.skeletonLineShort)} />
          </div>
        </div>
      ))}
    </div>
  );
};
