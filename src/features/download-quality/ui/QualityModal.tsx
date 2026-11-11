import { useEffect, useState } from "react";
import clsx from "clsx";
import { MdCheck, MdHelpOutline } from "react-icons/md";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { ItemType, useLibraryStore, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { platformLabel } from "@/shared/lib/remote/platforms.ts";
import {
  BANDCAMP_QUALITY_HINT,
  PROBE_WAIT_TEXT,
  formatDetails,
  probeAudioFormats,
  type AudioFormat,
} from "@/features/download-quality/lib/downloadQuality.ts";
import styles from "./quality-modal.module.css";

export interface QualityMenuOptions {
  /** Called once the download was started (e.g. the fly-to-library animation of the caller). */
  onStarted?: () => void;
  /** Starts the download in `format` instead of `startDownload({...params, format})` (album preview rows). */
  start?: (format: string | undefined) => void;
}

interface QualityModalProps extends QualityMenuOptions {
  /** What is downloaded (the probe reads its platform, url and first track); the picked format is added to it. */
  params: StartDownloadParams;
}

const isAlbumDownload = (params: StartDownloadParams) =>
  params.downloadType === ItemType.Album || String(params.downloadType).toLowerCase() === "album";

/**
 * Asks yt-dlp which audio formats the track (or the album's first track) has and downloads in the one the user
 * picks. The probe takes a few seconds; the text says so. When it fails the download still runs in the best
 * quality.
 */
const QualityModal = ({ params, onStarted, start }: QualityModalProps) => {
  const closeModal = useModalStore((s) => s.closeModal);
  const goBack = useModalStore((s) => s.goBack);
  const canGoBack = useModalStore((s) => s.stack.length > 0);
  const startDownload = useLibraryStore((s) => s.startDownload);
  const [formats, setFormats] = useState<AudioFormat[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string | null>(null);
  const [hintOpen, setHintOpen] = useState(false);
  const album = isAlbumDownload(params);
  const platform = String(params.platform).toLowerCase();

  useEffect(() => {
    let cancelled = false;
    probeAudioFormats(params.platform, params.url, album, params.trackUrls?.[0])
      .then((found) => {
        if (cancelled) return;
        setFormats(found);
        setPicked(found[0]?.id ?? null);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setFormats([]);
      });
    return () => {
      cancelled = true;
    };
  }, [params.platform, params.url, params.trackUrls, album]);

  const download = () => {
    const format = picked ?? undefined;
    if (start) start(format);
    else void startDownload({ ...params, format });
    onStarted?.();
    closeModal();
  };

  const onlyLossy = formats !== null && formats.length > 0 && !formats.some((f) => f.lossless);
  const showBandcampHint = platform === "bandcamp" && (onlyLossy || error !== null);

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Download quality</h2>
      <span className={styles.subtitle}>
        {params.title || "Download"}
        {params.artistName ? ` · ${params.artistName}` : ""} · {platformLabel(params.platform)}
      </span>

      {formats === null ? (
        <div className={styles.waiting} role="status">
          <span className={styles.spinner} aria-hidden="true" />
          <span>
            Checking the formats {album && !params.trackUrls?.length ? "of the album's first track " : ""}on {platformLabel(params.platform)}…{" "}
            <span className={styles.muted}>{PROBE_WAIT_TEXT}</span>
          </span>
        </div>
      ) : formats.length > 0 ? (
        <div className={styles.list} role="radiogroup" aria-label="Formats">
          {formats.map((format, i) => (
            <button
              key={format.id}
              type="button"
              role="radio"
              aria-checked={picked === format.id}
              className={clsx(styles.option, picked === format.id && styles.picked)}
              onClick={() => setPicked(format.id)}
            >
              <span className={styles.radio} aria-hidden="true">
                {picked === format.id && <MdCheck />}
              </span>
              <span className={styles.optionText}>
                <span className={styles.optionLabel}>
                  {format.label}
                  {i === 0 && <span className={styles.best}>Best</span>}
                </span>
                {formatDetails(format) && <span className={styles.muted}>{formatDetails(format)}</span>}
              </span>
            </button>
          ))}
        </div>
      ) : (
        <span className={styles.error}>
          Could not read the formats{error ? `: ${error}` : "."} The download still works in the best quality the
          platform gives.
        </span>
      )}

      {album && (params.trackUrls?.length ?? 2) > 1 && formats !== null && formats.length > 0 && (
        <span className={styles.note}>
          Applies to every track; a track without this format downloads in the next best one.
        </span>
      )}

      {showBandcampHint && (
        <div className={styles.hint}>
          <button type="button" className={styles.hintToggle} onClick={() => setHintOpen((v) => !v)} aria-expanded={hintOpen}>
            <MdHelpOutline /> Why only MP3 128 kbps?
          </button>
          {hintOpen && <p className={styles.hintText}>{BANDCAMP_QUALITY_HINT}</p>}
        </div>
      )}

      <div className={styles.buttons}>
        <button type="button" className={styles.secondary} onClick={canGoBack ? goBack : closeModal}>
          {canGoBack ? "Back" : "Cancel"}
        </button>
        <button type="button" className={styles.primary} onClick={download} disabled={formats === null}>
          {formats !== null && formats.length === 0 ? "Download in the best quality" : "Download"}
        </button>
      </div>
    </div>
  );
};

/** Opens the quality menu for `params` (on top of an open modal, which "Back" returns to). */
export function openQualityMenu(params: StartDownloadParams, options: QualityMenuOptions = {}) {
  useModalStore.getState().openModal(<QualityModal params={params} {...options} />);
}

export default QualityModal;
