import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { TorrentDiscovery } from "@/features/download-quality/lib/useTorrentDiscovery.ts";
import { focusOrOpenSettingsWindow } from "@/layouts/settingsWindow.ts";
import { isRuTrackerEnabled } from "@/shared/lib/audio.ts";
import type { AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import styles from "./album-preview.module.css";

const DISMISSED_KEY = "rutracker_discovery_dismissed";

export function AlbumDiscoveryStatus({
  loadingEditions, switchingRelease, resolvingCandidates, tracks, torrent,
}: {
  loadingEditions: boolean;
  switchingRelease: boolean;
  resolvingCandidates: boolean;
  tracks: readonly AlbumTrackInfo[];
  torrent: TorrentDiscovery;
}) {
  const [ruTrackerEnabled, setRuTrackerEnabled] = useState<boolean>();
  const [dismissed, setDismissed] = useState(() =>
    typeof window !== "undefined" && window.localStorage?.getItem(DISMISSED_KEY) === "1",
  );

  useEffect(() => {
    let disposed = false;
    const load = () => {
      isRuTrackerEnabled()
        .then((enabled) => {
          if (!disposed) setRuTrackerEnabled(enabled);
        })
        .catch(() => {});
    };
    load();
    let unlisten: (() => void) | undefined;
    void listen("torrent-indexers-changed", load)
      .then((fn) => {
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const status: { text: string; busy: boolean }[] = [];
  if (loadingEditions) {
    status.push({ text: "Looking up editions on MusicBrainz and other platforms…", busy: true });
  }
  if (switchingRelease) {
    status.push({ text: "Loading selected edition…", busy: true });
  } else if (resolvingCandidates) {
    const available = tracks.filter((track) => track.available);
    const ready = available.filter((track) =>
      track.candidates?.some((candidate) => candidate.source.kind === "http_stream"),
    ).length;
    status.push({ text: `Checking streaming sources · ${ready}/${available.length} tracks with sources`, busy: true });
  }
  if (torrent.phase === "searching" || torrent.phase === "estimated") {
    const progress = torrent.progress;
    const text = progress?.stage === "releases"
      ? `Matching torrent files · ${progress.checked}/${progress.total} releases checked`
      : progress
        ? `Searching torrents · ${progress.checked}/${progress.total} indexes checked`
        : "Searching torrent indexes…";
    status.push({ text, busy: true });
  }
  let coveredCount = 0;
  let ruTrackerUnavailable = false;
  if (torrent.phase === "done") {
    const covered = new Set(torrent.resolutions.flatMap((release) => release.tracks.map((track) => track.expected_index)));
    coveredCount = covered.size;
    ruTrackerUnavailable = Boolean(torrent.unavailableProviders?.includes("RuTracker"));
    const result = torrent.failed
      ? "Torrent search failed"
      : covered.size > 0
        ? `Torrent sources matched ${covered.size}/${tracks.length} tracks`
        : "No matching torrent sources found";
    const unavailable = torrent.unavailableProviders?.join(", ");
    status.push({ text: unavailable ? `${result} · Unavailable indexes: ${unavailable}` : result, busy: false });
  }

  const showConnectHint =
    torrent.phase === "done" &&
    ruTrackerEnabled === false &&
    !dismissed &&
    !ruTrackerUnavailable &&
    coveredCount < tracks.length;

  return (
    <div className={styles.discoveryStatus} aria-live="polite">
      {status.map(({ text, busy }) => (
        <span key={text} className={styles.discoveryStep}>
          {busy && <span className={styles.miniSpinner} aria-hidden="true" />}
          {text}
        </span>
      ))}
      {ruTrackerUnavailable && (
        <span className={styles.discoveryStep}>
          <button
            type="button"
            className={styles.discoveryLink}
            onClick={() => void focusOrOpenSettingsWindow("downloader")}
          >
            Sign in to RuTracker
          </button>
        </span>
      )}
      {showConnectHint && (
        <span className={styles.discoveryStep}>
          <span>Want more lossless sources?</span>
          <button
            type="button"
            className={styles.discoveryLink}
            onClick={() => void focusOrOpenSettingsWindow("downloader")}
          >
            Connect RuTracker
          </button>
          <button
            type="button"
            className={styles.discoveryDismiss}
            aria-label="Dismiss RuTracker hint"
            onClick={() => {
              setDismissed(true);
              window.localStorage?.setItem(DISMISSED_KEY, "1");
            }}
          >
            ×
          </button>
        </span>
      )}
    </div>
  );
}
