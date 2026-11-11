import clsx from "clsx";
import { songQuality } from "@/shared/ui/quality-badge/songQuality.ts";
import type { QualityTier } from "@/shared/types.ts";
import styles from "./quality-badge.module.css";

/** File format and quality tier of a library song ("MP3 · Low", "FLAC" in green). */
export const QualityBadge = ({
  song,
  className,
}: {
  song: { path: string; quality_tier: QualityTier };
  className?: string;
}) => {
  const q = songQuality(song);
  return (
    <span className={clsx(styles.badge, styles[q.tier], className)} title={q.title}>
      {q.label}
    </span>
  );
};
