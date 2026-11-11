import clsx from "clsx";
import { platformLabel } from "@/shared/lib/remote/platforms.ts";
import styles from "./platform-badge.module.css";

/** Small muted badge tinted with the platform's brand colour ("YOUTUBE", "BANDCAMP", "LOCAL"). */
export const PlatformBadge = ({
  platform,
  className,
}: {
  platform: string | null | undefined;
  className?: string;
}) => (
  <span
    className={clsx(
      styles.badge,
      platform === "youtube" && styles.youtube,
      platform === "bandcamp" && styles.bandcamp,
      platform === "soundcloud" && styles.soundcloud,
      className,
    )}
  >
    {platform ? platformLabel(platform) : ""}
  </span>
);
