import clsx from "clsx";
import { platformLabel } from "./searchResultsFormat.ts";
import styles from "./search-results.module.css";

/** Small muted badge tinted with the platform's brand colour. */
export const PlatformBadge = ({ platform }: { platform: string | null | undefined }) => (
  <span
    className={clsx(
      styles.badge,
      platform === "youtube" && styles.badgeYoutube,
      platform === "bandcamp" && styles.badgeBandcamp,
    )}
  >
    {platformLabel(platform)}
  </span>
);
