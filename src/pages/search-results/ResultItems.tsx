import clsx from "clsx";
import {
  ItemType,
  type DownloadProgress,
  type SearchSuggestion,
} from "@/shared/store/libraryStore.ts";
import type { SearchKind } from "@/shared/lib/searchFull.ts";
import { DownloadButton } from "./DownloadButton.tsx";
import { StreamButton } from "@/features/preview/ui/StreamButton.tsx";
import { useRemoteTrackMenu } from "@/shared/hooks/useRemoteTrackMenu.ts";
import { searchItemToRemoteTrack } from "@/shared/lib/remoteTrack.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import { remoteRefOf, useAlbumStatus } from "@/shared/hooks/useDownloadStatus.ts";
import { DownloadStatusBadge } from "@/shared/ui/download-status-badge/DownloadStatusBadge.tsx";
import { PlatformBadge } from "./PlatformBadge.tsx";
import {
  formatDurationCell,
  formatPlays,
  formatAudience,
  trackAlbumItem,
  trackSubline,
} from "./searchResultsFormat.ts";
import { coverSrc } from "@/shared/lib/cover";
import styles from "./search-results.module.css";

/** Cover image shown at `size` CSS px; a flat placeholder when the item has no image. */
export const Cover = ({
  src,
  alt,
  className,
  size,
}: {
  src: string;
  alt: string;
  className: string;
  size: number;
}) =>
  src ? (
    <img
      className={className}
      src={coverSrc(src, size)}
      alt={alt}
      referrerPolicy="no-referrer"
      loading="lazy"
    />
  ) : (
    <div className={clsx(className, styles.coverPlaceholder)} aria-hidden="true" />
  );

interface TrackRowProps {
  item: SearchSuggestion;
  /** The list the row belongs to: playing the row queues the tracks after it. */
  list?: readonly SearchSuggestion[];
  progress?: DownloadProgress;
  /** Compact rows (artist profile "Top songs") keep the play count out. */
  compact?: boolean;
}

export const TrackRow = ({ item, list, progress, compact = false }: TrackRowProps) => {
  const subline = trackSubline(item.band_name, item.album_name);
  const albumItem = trackAlbumItem(item);
  const { openMenu } = useRemoteTrackMenu();
  return (
    <div
      className={styles.trackRow}
      onContextMenu={(e) =>
        openMenu(e, searchItemToRemoteTrack(item), { list: list?.map(searchItemToRemoteTrack) })
      }
    >
      <div className={styles.trackCoverBox}>
        <Cover src={item.img} alt={item.name} className={styles.trackCover} size={44} />
        <StreamButton item={item} list={list} className={styles.trackPreview} />
      </div>
      <div className={styles.trackInfo}>
        <div className={styles.titleRow}>
          <p className={clsx(styles.trackTitle, styles.ellipsis)} title={item.name}>
            {item.name}
          </p>
        </div>
        <p className={clsx(styles.subline, styles.ellipsis)} title={subline}>
          {albumItem ? (
            <>
              {trackSubline(item.band_name, "")}
              {item.band_name.trim() !== "" && " - "}
              <button
                type="button"
                className={styles.albumLink}
                title={`Open album ${albumItem.name}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void useAlbumPreviewStore.getState().open(albumItem);
                }}
              >
                {albumItem.name}
              </button>
            </>
          ) : (
            subline
          )}
        </p>
      </div>
      {/* Columns keep their width when the value is missing, so the rows line up. */}
      <span className={styles.trackDuration}>{formatDurationCell(item.duration)}</span>
      {!compact && <span className={styles.trackPlays}>{formatPlays(item.view_count) ?? ""}</span>}
      {!compact && (
        <span className={styles.badgeColumn}>
          <PlatformBadge platform={item.platform} />
        </span>
      )}
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

export const AlbumCard = ({ item, progress, compact = false }: AlbumCardProps) => {
  const albumState = useAlbumStatus(remoteRefOf(item)).state;
  return (
    <article
      className={clsx(styles.card, styles.cardClickable)}
      role="button"
      tabIndex={0}
      aria-label={`Open album ${item.name}`}
      onClick={() => void useAlbumPreviewStore.getState().open(item)}
      onKeyDown={(e) => {
        if (e.key === "Enter" && e.target === e.currentTarget) {
          void useAlbumPreviewStore.getState().open(item);
        }
      }}
    >
      <div className={styles.coverWrap}>
        <Cover src={item.img} alt={item.name} className={styles.cover} size={240} />
        {/* Fully downloaded albums show the check in place of the download button. */}
        <DownloadStatusBadge
          state={albumState === "partial" ? "partial" : "none"}
          size={20}
          className={styles.coverBadge}
        />
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
};

export const ArtistCard = ({ item }: { item: SearchSuggestion }) => {
  const audience = formatAudience(item.subscriber_count, item.audience_kind);
  return (
    <article className={clsx(styles.card, styles.artistCard)}>
      <div className={clsx(styles.coverWrap, styles.coverRound)}>
        <Cover src={item.img} alt={item.name} className={styles.cover} size={240} />
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
