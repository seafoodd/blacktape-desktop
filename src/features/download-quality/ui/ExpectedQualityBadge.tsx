import { useEffect, useState } from "react";
import clsx from "clsx";
import { expectedQualityOf, getExpectedQuality, type ExpectedQuality } from "@/features/download-quality/lib/downloadQuality.ts";
import styles from "./expected-quality-badge.module.css";

interface ExpectedQualityBadgeProps {
  platform: string;
  /** Page of the album or track (Bandcamp reads it to see whether lossless files are offered). */
  url: string;
  className?: string;
}

/**
 * The quality a download from this platform is expected to have ("Opus ~160 kbps", "FLAC (lossless)"), shown
 * without a yt-dlp probe. YouTube and SoundCloud answer at once; Bandcamp reads its page (about 1 s) and shows
 * "MP3 128 kbps" until then.
 */
export const ExpectedQualityBadge = ({ platform, url, className }: ExpectedQualityBadgeProps) => {
  const known = expectedQualityOf(platform);
  const [quality, setQuality] = useState<ExpectedQuality | null>(known);

  useEffect(() => {
    if (known) return;
    let cancelled = false;
    getExpectedQuality(platform, url)
      .then((q) => !cancelled && setQuality(q))
      .catch(() => !cancelled && setQuality({ label: "MP3 128 kbps", lossless: false }));
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [platform, url]);

  if (platform.toLowerCase() === "local") return null;
  return (
    <span
      className={clsx(styles.badge, quality?.lossless && styles.lossless, !quality && styles.pending, className)}
      title="Expected download quality"
    >
      {quality?.label ?? "Checking quality…"}
    </span>
  );
};
