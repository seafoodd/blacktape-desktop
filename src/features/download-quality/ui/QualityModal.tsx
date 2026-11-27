import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { useEffect, useMemo, useState } from "react";
import clsx from "clsx";
import { MdCheck, MdHelpOutline } from "react-icons/md";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { ItemType, useLibraryStore, type DownloadOverrides, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { useAlbumPreviewStore } from "@/features/album-preview/model/albumPreviewStore.ts";
import {
  probeUpgradeCandidates,
  probeDownloadStreams,
  type ExpectedTorrentTrackInput,
} from "@/shared/lib/audio.ts";
import { useTorrentDiscovery } from "@/features/download-quality/lib/useTorrentDiscovery.ts";
import { getAlbumPreview, type AlbumTrackInfo } from "@/shared/lib/remote/albumPreview.ts";
import { platformLabel } from "@/shared/lib/remote/platforms.ts";
import { PlatformBadge } from "@/shared/ui/platform-badge/PlatformBadge.tsx";
import {
  BANDCAMP_QUALITY_HINT,
  PROBE_WAIT_TEXT,
  buildCanonicalQualityPlan,
  downloadButtonLabel,
  extractResolveSongIds,
  formatByteSize,
  formatDetails,
  mergeTorrentResolutionsIntoTracks,
  mergeScopedQualityTracks,
  probeAudioFormats,
  tierSuffixFor,
  torrentQualityLabel,
  type AudioFormat,
} from "@/features/download-quality/lib/downloadQuality.ts";
import styles from "./quality-modal.module.css";

export interface QualityMenuOptions {
  displayTitle?: string;
  /** The exact rows selected in ArtistAlbums when its album preview is not open. */
  scopeTracks?: AlbumTrackInfo[];
  /** Called once the download was started (e.g. the fly-to-library animation of the caller). */
  onStarted?: () => void;
  /** Starts the download in `format` instead of `startDownload({...params, format})` (album preview rows). */
  start?: (format: string | undefined, overrides?: DownloadOverrides) => void;
}

interface QualityModalProps extends QualityMenuOptions {
  /** What is downloaded (the probe reads its platform, url and first track); the picked format is added to it. */
  params: StartDownloadParams;
}

const isAlbumDownload = (params: StartDownloadParams) =>
  params.downloadType === ItemType.Album || String(params.downloadType).toLowerCase() === "album";

export function submitQualityDownload(params: StartDownloadParams, { start, onStarted }: QualityMenuOptions) {
  // Album Preview may open a confirmation before starting the download.
  useModalStore.getState().closeModal();
  if (start) start(params.format, params.overrides);
  else void useDownloadStore.getState().startDownload(params);
  onStarted?.();
}

/**
 * Asks yt-dlp which audio formats the track (or the album's first track) has and downloads in the one the user
 * picks. The probe takes a few seconds; the text says so. When it fails the download still runs in the best
 * quality.
 */
const QualityModal = ({ params, onStarted, start, scopeTracks: selectedScope, displayTitle }: QualityModalProps) => {
  const closeModal = useModalStore((s) => s.closeModal);
  const goBack = useModalStore((s) => s.goBack);
  const canGoBack = useModalStore((s) => s.stack.length > 0);
  const [formats, setFormats] = useState<AudioFormat[] | null>(null);
  const [resolvedUpgradeTracks, setResolvedUpgradeTracks] = useState<AlbumTrackInfo[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [scopedCandidates, setScopedCandidates] = useState<AlbumTrackInfo[]>([]);
  const [searchingStreams, setSearchingStreams] = useState(false);
  const [picked, setPicked] = useState<string | null>(null);
  const [hintOpen, setHintOpen] = useState(false);
  const previewState = useAlbumPreviewStore((s) => s.preview);
  const localAlbums = useLibraryStore((s) => s.albums);
  const album = isAlbumDownload(params);
  const platform = String(params.platform).toLowerCase();

  const isResolveUpgrade = params.url.startsWith("resolve:");
  const isDirectTrack = !album && !isResolveUpgrade && !!params.title && !!params.artistName;
  const scopeTracks = useMemo(() => selectedScope ?? (isDirectTrack ? [{
    index: 0, title: params.placement?.title ?? params.title!, artists: [params.artistName!], url: params.url,
    available: true, track_number: params.placement?.track_number ?? null, note: null,
  }] : undefined), [selectedScope, isDirectTrack, params]);
  const isMultiSource = params.url.startsWith("canonical:") || isResolveUpgrade || !!scopeTracks;
  const resolveSongIds = useMemo(
    () => (isResolveUpgrade ? extractResolveSongIds(params.url, params.trackUrls) : []),
    [isResolveUpgrade, params.url, params.trackUrls],
  );
  const resolveSongIdsKey = resolveSongIds.join(",");
  const waitingUpgradeProbe =
    isResolveUpgrade && resolveSongIds.length > 0 && resolvedUpgradeTracks === null;

  useEffect(() => {
    if (!scopeTracks || !params.url.startsWith("canonical:")) return;
    if (platform !== "youtube" && platform !== "bandcamp" && platform !== "soundcloud") return;
    let current = true;
    void getAlbumPreview(platform, params.url).then((preview) => {
      if (current) setScopedCandidates((rows) =>
        mergeScopedQualityTracks(rows.length ? rows : mergeScopedQualityTracks(scopeTracks, []), preview.tracks));
    }).catch(() => {});
    return () => { current = false; };
  }, [scopeTracks, platform, params.url]);

  const scopedTracks = useMemo(() => scopeTracks && mergeScopedQualityTracks(scopeTracks, scopedCandidates),
    [scopeTracks, scopedCandidates]);

  const torrentScope = useMemo<{
    artist: string;
    title: string;
    expected: ExpectedTorrentTrackInput[];
  } | null>(() => {
    if (isResolveUpgrade) {
      const songById = new Map(localAlbums.flatMap((a) => a.songs).map((s) => [s.id, s]));
      const matchedSongs = resolveSongIds
        .map((id) => songById.get(id))
        .filter((s): s is NonNullable<typeof s> => s !== undefined);
      if (matchedSongs.length === 0) return null;
      const distinctAlbums = new Set(
        matchedSongs.map((s) => `${s.album_artist.toLowerCase()}::${s.album.toLowerCase()}`),
      );
      if (distinctAlbums.size > 1) return null;
      const first = matchedSongs[0];
      const searchTitle = first.album.trim() || first.title.trim();
      const searchArtist = (first.album_artist.trim() || first.artists[0]?.trim() || "").trim();
      if (!searchArtist || !searchTitle) return null;
      return {
        artist: searchArtist,
        title: searchTitle,
        expected: matchedSongs.map((song, idx) => ({
          index: idx,
          title: song.title,
          artist: song.artists[0] ?? song.album_artist,
          disc_number: song.disc_number ?? null,
          track_number: song.track_number ?? null,
          duration_ms: song.duration_ms > 0 ? song.duration_ms : null,
        })),
      };
    }

    if (scopeTracks && params.artistName?.trim() && params.title?.trim()) {
      return {
        artist: params.artistName,
        title: params.placement?.album || params.title,
        expected: scopeTracks.map((track) => ({
          index: track.index, title: track.title, artist: track.artists[0] ?? params.artistName,
          disc_number: track.disc_number ?? null, track_number: track.track_number ?? null,
          duration_ms: track.duration_ms ?? null,
        })),
      };
    }

    if (previewState && previewState.url === params.url) {
      const selectedSet = params.trackUrls?.length ? new Set(params.trackUrls) : null;
      const tracks = previewState.tracks.filter(
        (t) => !selectedSet || selectedSet.has(t.url),
      );
      if (!previewState.artist.trim() || !previewState.title.trim()) return null;
      return {
        artist: previewState.artist,
        title: previewState.title,
        expected: tracks.map((t) => ({
          index: t.index,
          title: t.title,
          artist: t.artists[0] ?? previewState.artist,
          disc_number: t.disc_number ?? null,
          track_number: t.track_number ?? null,
          duration_ms: t.duration_ms ?? null,
        })),
      };
    }

    const artist = (params.artistName ?? "").trim();
    const title = (params.title ?? params.placement?.title ?? "").trim();
    if (!artist || !title) return null;
    return {
      artist,
      title,
      expected:
        album && !params.placement
          ? []
          : [
              {
                index: 0,
                title: params.placement?.title ?? title,
                artist,
                track_number: params.placement?.track_number ?? null,
              },
            ],
    };
  }, [isResolveUpgrade, resolveSongIds, localAlbums, previewState, params, album, scopeTracks]);

  useEffect(() => {
    if (!scopeTracks?.length || !torrentScope) return;
    let current = true;
    setSearchingStreams(true);
    void probeDownloadStreams(torrentScope.artist, torrentScope.title, torrentScope.expected, (match) => {
      if (!current) return;
      const track = scopeTracks.find((row) => row.index === match.index);
      if (!track) return;
      setScopedCandidates((rows) => mergeScopedQualityTracks(
        rows.length ? rows : mergeScopedQualityTracks(scopeTracks, []),
        [{ ...track, candidates: match.candidates }],
      ));
    }).catch(() => {}).finally(() => {
      if (current) setSearchingStreams(false);
    });
    return () => { current = false; };
  }, [scopeTracks, torrentScope]);

  const torrentDiscovery = useTorrentDiscovery(params.url, torrentScope);
  const torrentPhase = torrentDiscovery.phase;
  const torrentResolutions = torrentDiscovery.resolutions ?? [];
  const torrentEstimates = torrentPhase === "estimated"
    ? torrentDiscovery.estimates.filter((estimate) =>
        !torrentResolutions.some((release) => release.tier === estimate.tier && release.matched_count > 0),
      )
    : [];

  const canonicalPlan = useMemo(() => {
    if (!isMultiSource) return null;
    const baseTracks = isResolveUpgrade
      ? (resolvedUpgradeTracks ?? [])
      : scopedTracks ?? (previewState && previewState.url === params.url
        ? previewState.tracks
        : []);
    const mergedTracks = mergeTorrentResolutionsIntoTracks(baseTracks, torrentResolutions);
    return buildCanonicalQualityPlan(
      mergedTracks,
      isResolveUpgrade ? undefined : params.trackUrls,
      torrentResolutions,
    );
  }, [
    isMultiSource,
    isResolveUpgrade,
    resolvedUpgradeTracks,
    previewState,
    params.url,
    params.trackUrls,
    torrentResolutions,
    scopedTracks,
  ]);

  const pickedCanonicalOption = useMemo(() => {
    if (!isMultiSource || !canonicalPlan) return null;
    if (picked === canonicalPlan.best.id) return canonicalPlan.best;
    return canonicalPlan.options.find((o) => o.id === picked) ?? null;
  }, [isMultiSource, canonicalPlan, picked]);
  const upgradeNotNeeded = pickedCanonicalOption?.upgradableCount === 0;

  useEffect(() => {
    if (isMultiSource && !isDirectTrack) {
      setPicked("best");
      setFormats([]);
      if (!isResolveUpgrade || resolveSongIds.length === 0) {
        setResolvedUpgradeTracks(null);
        return;
      }
      let cancelled = false;
      setResolvedUpgradeTracks(null);
      probeUpgradeCandidates(resolveSongIds)
        .then((tracks) => {
          if (!cancelled) setResolvedUpgradeTracks(tracks);
        })
        .catch(() => {
          if (!cancelled) setResolvedUpgradeTracks([]);
        });

      return () => {
        cancelled = true;
      };
    }

    let cancelled = false;
    if (isDirectTrack) setPicked("best");
    probeAudioFormats(params.platform, params.url, album, params.trackUrls?.[0])
      .then((found) => {
        if (cancelled) return;
        setFormats(found);
        if (!isDirectTrack) setPicked(found[0]?.id ?? null);
      })
      .catch((e) => {
        if (cancelled) return;
        setError(String(e));
        setFormats([]);
      });

    return () => {
      cancelled = true;
    };
  }, [params.platform, params.url, params.trackUrls, album, isMultiSource, isDirectTrack, isResolveUpgrade, resolveSongIdsKey]);

  const download = () => {
    const format = picked ?? undefined;
    let overrides: DownloadOverrides | undefined = params.overrides;
    if (scopedTracks && !(isDirectTrack && formats?.some((option) => option.id === format))) {
      overrides = {
        ...params.overrides,
        tracks: mergeTorrentResolutionsIntoTracks(scopedTracks, torrentResolutions).map((track) => ({
          index: track.index,
          original_url: params.url.startsWith("canonical:") ? undefined : track.url,
          title: track.title,
          candidates: track.candidates,
          selected_candidate_idx: format === "best" ? 0 : track.selected_candidate_idx,
        })),
      };
    }
    if (!scopedTracks && torrentResolutions.some((r) => r.matched_count > 0)) {
      if (isResolveUpgrade) {
        const merged = mergeTorrentResolutionsIntoTracks(resolvedUpgradeTracks ?? [], torrentResolutions);
        overrides = {
          ...params.overrides,
          tracks: merged.map((t) => ({
            index: t.index,
            title: t.title,
            candidates: t.candidates,
            selected_candidate_idx: t.selected_candidate_idx ?? 0,
          })),
        };
      } else if (previewState && previewState.url === params.url) {
        const merged = mergeTorrentResolutionsIntoTracks(previewState.tracks, torrentResolutions);
        overrides = {
          ...params.overrides,
          title: previewState.title,
          artist: previewState.artist,
          year: previewState.year ?? null,
          tracks: merged.map((t) => ({
            index: t.index,
            title: t.title,
            candidates: t.candidates,
            selected_candidate_idx: t.selected_candidate_idx ?? 0,
          })),
        };
      } else {
        const byIndex = new Map<number, NonNullable<DownloadOverrides["tracks"]>[number]>();
        for (const res of torrentResolutions) {
          if (res.matched_count === 0) continue;
          for (const entry of res.tracks) {
            const cur = byIndex.get(entry.expected_index) ?? {
              index: entry.expected_index,
              candidates: [],
              selected_candidate_idx: 0,
            };
            cur.candidates = [...(cur.candidates ?? []), entry.candidate];
            byIndex.set(entry.expected_index, cur);
          }
        }
        if (byIndex.size > 0) {
          overrides = { ...params.overrides, tracks: [...byIndex.values()] };
        }
      }
    }

    if (start && torrentResolutions.length > 0) {
      useAlbumPreviewStore.setState((state) => {
        if (!state.preview || state.preview.url !== params.url) return state;
        const tracks = mergeTorrentResolutionsIntoTracks(state.preview.tracks, torrentResolutions);
        return { preview: { ...state.preview, tracks } };
      });
    }
    submitQualityDownload({ ...params, format, overrides }, { start, onStarted });
  };

  const onlyLossy = formats !== null && formats.length > 0 && !formats.some((f) => f.lossless);
  const showBandcampHint = !isMultiSource && platform === "bandcamp" && (onlyLossy || error !== null);

  const torrentPendingBlock =
    torrentPhase === "searching" || (torrentPhase === "estimated" && torrentEstimates.length === 0) ? (
      <div className={clsx(styles.option, styles.optionDisabled)} role="status">
        <span className={styles.miniSpinner} aria-hidden="true" />
        <span className={styles.optionText}>
          <span className={styles.optionLabel}>
            <PlatformBadge platform="torrent" />
            <span>{torrentResolutions.length > 0 ? "Checking more releases…" : "Lossless / MP3 320 kbps"}</span>
          </span>
          <span className={styles.muted}>{torrentPhase === "searching" ? "Searching torrent sources…" : "Matching torrent files…"}</span>
        </span>
      </div>
    ) : torrentPhase === "estimated" && torrentEstimates.length > 0 ? (
      <>
        {torrentEstimates.map((est) => {
          const size = formatByteSize(est.size_bytes);
          const metaParts = [
            "Matching files…",
            size,
            est.seeders != null ? `${est.seeders} seeder${est.seeders === 1 ? "" : "s"}` : null,
            est.provider,
          ].filter(Boolean);
          return (
            <div key={`${est.tier}:${est.release_title}`} className={clsx(styles.option, styles.optionDisabled)} role="status">
              <span className={styles.miniSpinner} aria-hidden="true" />
              <span className={styles.optionText}>
                <span className={styles.optionLabel}>
                  <PlatformBadge platform="torrent" />
                  <span>{est.label}</span>
                </span>
                <span className={styles.muted}>{metaParts.join(" · ")}</span>
              </span>
            </div>
          );
        })}
      </>
    ) : null;

  return (
    <div className={styles.container}>
      <h2 className={styles.title}>Download quality</h2>
      <span className={styles.subtitle}>
        {displayTitle || params.title || "Download"}
        {params.artistName ? ` · ${params.artistName}` : ""}
        {!isMultiSource ? ` · ${platformLabel(params.platform)}` : ""}
      </span>

      {isMultiSource && waitingUpgradeProbe ? (
        <div className={styles.waiting} role="status">
          <span className={styles.spinner} aria-hidden="true" />
          <span>
            Discovering lossless &amp; platform sources…{" "}
            <span className={styles.muted}>Checking YouTube, Bandcamp &amp; SoundCloud.</span>
          </span>
        </div>
      ) : isMultiSource && canonicalPlan ? (
        <div className={styles.list} role="radiogroup" aria-label="Formats">
          {searchingStreams && (
            <div className={clsx(styles.option, styles.optionDisabled)} role="status">
              <span className={styles.miniSpinner} aria-hidden="true" />
              Checking YouTube, Bandcamp and SoundCloud…
            </div>
          )}
          <button
            key={canonicalPlan.best.id}
            type="button"
            role="radio"
            aria-checked={picked === canonicalPlan.best.id}
            className={clsx(
              styles.option,
              styles.bestCard,
              picked === canonicalPlan.best.id && styles.picked,
            )}
            onClick={() => setPicked(canonicalPlan.best.id)}
          >
            <span className={styles.radio} aria-hidden="true">
              {picked === canonicalPlan.best.id && <MdCheck />}
            </span>
            <span className={styles.optionText}>
              <span className={styles.optionLabel}>
                {canonicalPlan.best.label}
                <span className={styles.best}>Best</span>
              </span>
              <span className={styles.muted}>{canonicalPlan.best.details}</span>
            </span>
          </button>

          {(canonicalPlan.options.length > 0 || torrentPendingBlock !== null) && (
            <>
              <div className={styles.sectionDivider}>
                <span>
                  {canonicalPlan.hasDiscoveredCandidates
                    ? "Discovered sources by platform"
                    : "Quality presets"}
                </span>
              </div>
              {canonicalPlan.options.map((opt) => (
                <button
                  key={opt.id}
                  type="button"
                  role="radio"
                  aria-checked={picked === opt.id}
                  disabled={opt.disabled}
                  className={clsx(
                    styles.option,
                    picked === opt.id && styles.picked,
                    opt.disabled && styles.optionDisabled,
                  )}
                  onClick={() => {
                    if (!opt.disabled) setPicked(opt.id);
                  }}
                >
                  <span className={styles.radio} aria-hidden="true">
                    {picked === opt.id && <MdCheck />}
                  </span>
                  <span className={styles.optionText}>
                    <span className={styles.optionLabel}>
                      {opt.platform && <PlatformBadge platform={opt.platform} />}
                      <span>{opt.label}</span>
                    </span>
                    {opt.details && <span className={styles.muted}>{opt.details}</span>}
                    {opt.warning && <span className={styles.optionWarn}>{opt.warning}</span>}
                  </span>
                </button>
              ))}
              {torrentPendingBlock}
            </>
          )}
          {isDirectTrack && !!formats?.length && (
            <>
              <div className={styles.sectionDivider}><span>Formats on {platformLabel(params.platform)}</span></div>
              {formats.map((format) => (
                <button key={format.id} type="button" role="radio" aria-checked={picked === format.id}
                  className={clsx(styles.option, picked === format.id && styles.picked)} onClick={() => setPicked(format.id)}>
                  <span className={styles.radio} aria-hidden="true">{picked === format.id && <MdCheck />}</span>
                  <span className={styles.optionText}>
                    <span className={styles.optionLabel}>
                      <PlatformBadge platform={params.platform} />
                      <span>{format.label}</span>
                    </span>
                    <span className={styles.muted}>{formatDetails(format)}</span>
                  </span>
                </button>
              ))}
            </>
          )}
        </div>
      ) : formats === null ? (
        <div className={styles.waiting} role="status">
          <span className={styles.spinner} aria-hidden="true" />
          <span>
            Checking the formats {album && !params.trackUrls?.length ? "of the album's first track " : ""}on {platformLabel(params.platform)}…{" "}
            <span className={styles.muted}>{PROBE_WAIT_TEXT}</span>
          </span>
        </div>
      ) : formats.length > 0 || torrentResolutions.length > 0 || torrentPendingBlock !== null ? (
        <div className={styles.list} role="radiogroup" aria-label="Formats">
          {formats.map((format, i) => (
            <button
              key={format.id}
              type="button"
              role="radio"
              aria-checked={picked === format.id}
              className={clsx(
                styles.option,
                i === 0 && styles.bestCard,
                picked === format.id && styles.picked,
              )}
              onClick={() => setPicked(format.id)}
            >
              <span className={styles.radio} aria-hidden="true">
                {picked === format.id && <MdCheck />}
              </span>
              <span className={styles.optionText}>
                <span className={styles.optionLabel}>
                  <PlatformBadge platform={platform} />
                  <span>{format.label}</span>
                  {i === 0 && <span className={styles.best}>Best</span>}
                </span>
                {formatDetails(format) && <span className={styles.muted}>{formatDetails(format)}</span>}
              </span>
            </button>
          ))}
          {(torrentResolutions.length > 0 || torrentPendingBlock !== null) && (
            <>
              <div className={styles.sectionDivider}>
                <span>Torrents</span>
              </div>
              {(() => {
                const seenTiers = new Set<string>();
                return torrentResolutions.map((res) => {
                  const isCueOnly = res.matched_count === 0 && !!res.layout_warning;
                  const firstInTier = !isCueOnly && !seenTiers.has(res.tier);
                  if (!isCueOnly) seenTiers.add(res.tier);
                  const optId = isCueOnly
                    ? `torrent-notice:${res.info_hash}`
                    : firstInTier
                      ? `source:torrent${tierSuffixFor(res.tier)}`
                      : `source:torrent${tierSuffixFor(res.tier) || ":"}:${res.info_hash}`;
                const metaParts = [
                  res.total_expected > 1
                    ? `${res.matched_count}/${res.total_expected} tracks`
                    : res.matched_count > 0
                      ? "Verified match"
                      : null,
                  formatByteSize(res.total_matched_bytes),
                  res.seeders != null
                    ? `${res.seeders} seeder${res.seeders === 1 ? "" : "s"}`
                    : null,
                  res.provider,
                ].filter(Boolean);
                return (
                  <button
                    key={optId}
                    type="button"
                    role="radio"
                    aria-checked={picked === optId}
                    disabled={isCueOnly}
                    className={clsx(
                      styles.option,
                      picked === optId && styles.picked,
                      isCueOnly && styles.optionDisabled,
                    )}
                    onClick={() => {
                      if (!isCueOnly) setPicked(optId);
                    }}
                  >
                    <span className={styles.radio} aria-hidden="true">
                      {picked === optId && <MdCheck />}
                    </span>
                    <span className={styles.optionText}>
                      <span className={styles.optionLabel}>
                        <PlatformBadge platform="torrent" />
                        <span>{torrentQualityLabel(res)}</span>
                      </span>
                      {metaParts.length > 0 && <span className={styles.muted}>{metaParts.join(" · ")}</span>}
                      {res.layout_warning && <span className={styles.optionWarn}>{res.layout_warning}</span>}
                    </span>
                  </button>
                );
              });
            })()}
              {torrentPendingBlock}
            </>
          )}
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
        <button
          type="button"
          className={styles.primary}
          onClick={download}
          disabled={(!isMultiSource && formats === null) || waitingUpgradeProbe || upgradeNotNeeded}
        >
          {upgradeNotNeeded
            ? "Already up to date"
            : downloadButtonLabel(isMultiSource, picked, formats)}
        </button>
      </div>
    </div>
  );
};

/** Opens the quality menu for `params` (on top of an open modal, which "Back" returns to). */
export function openQualityMenu(params: StartDownloadParams, options: QualityMenuOptions = {}) {
  useModalStore.getState().openModal(<QualityModal key={JSON.stringify([params.url, params.trackUrls])} params={params} {...options} />);
}

export default QualityModal;
