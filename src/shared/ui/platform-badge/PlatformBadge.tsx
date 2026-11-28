import clsx from "clsx";
import { platformLabel } from "@/shared/lib/remote/platforms.ts";
import styles from "./platform-badge.module.css";

/** Small muted badge tinted with the platform's brand colour ("YOUTUBE", "BANDCAMP", "MULTI-SOURCE"). */
export const PlatformBadge = ({
  platform,
  canonical = false,
  className,
}: {
  platform: string | null | undefined;
  canonical?: boolean;
  className?: string;
}) => (
  <span
    className={clsx(
      styles.badge,
      canonical
        ? styles.canonical
        : [
            platform === "youtube" && styles.youtube,
            platform === "bandcamp" && styles.bandcamp,
            platform === "soundcloud" && styles.soundcloud,
            platform === "torrent" && styles.torrent,
          ],
      className,
    )}
  >
    {canonical ? "MULTI-SOURCE" : platform ? platformLabel(platform) : ""}
  </span>
);
