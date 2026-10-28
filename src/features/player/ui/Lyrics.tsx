import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "./lyrics.module.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import { LyricLine, parseLyrics } from "../lib/lyricsParser";

interface LyricsProps {
  className?: string;
}

export const Lyrics = ({ className }: LyricsProps) => {
  const { currentSong, progress, isPlaying, seek, updateProgress } =
    useAudioStore();
  const [lyrics, setLyrics] = useState<string | null>(null);
  const [retrieving, setRetrieving] = useState<boolean>(false);
  const [source, setSource] = useState<string | null>(null);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const activeLineRef = useRef<HTMLDivElement | null>(null);
  const [userScrolled, setUserScrolled] = useState<boolean>(false);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!currentSong?.id) {
      setLyrics(null);
      setSource(null);
      return;
    }

    let isCurrentRequest = true;
    setRetrieving(true);
    setLyrics(null);
    setSource(null);

    const fetchLyrics = async () => {
      try {
        const lyricsSource = await invoke<{
          lyrics: string;
          source: string;
        }>("get_lyrics", {
          id: currentSong.id,
        });

        if (isCurrentRequest) {
          const lyr = lyricsSource?.lyrics;
          setLyrics(lyr && lyr.trim().length > 0 ? lyr : null);
          setSource(lyricsSource?.source ?? null);
        }
      } catch (err) {
        console.log("Failed to fetch lyrics:", err);
        if (isCurrentRequest) {
          setLyrics(null);
          setSource(null);
        }
      } finally {
        if (isCurrentRequest) setRetrieving(false);
      }
    };

    fetchLyrics();

    return () => {
      isCurrentRequest = false;
    };
  }, [currentSong?.id]);

  const parsedLyrics = useMemo(() => {
    return lyrics ? parseLyrics(lyrics) : null;
  }, [lyrics]);

  // Current playback time in milliseconds
  const currentTimeMs = currentSong ? currentSong.duration_ms * progress : 0;

  // Faster progress polling when playing timed lyrics for smooth syncing
  useEffect(() => {
    if (!isPlaying || !parsedLyrics?.isTimed) return;
    const interval = setInterval(() => {
      updateProgress();
    }, 250);

    return () => clearInterval(interval);
  }, [isPlaying, parsedLyrics?.isTimed, updateProgress]);

  // Find active line index based on current playback time
  const activeIndex = useMemo(() => {
    if (!parsedLyrics?.isTimed || parsedLyrics.lines.length === 0) return -1;
    let found = -1;
    for (let i = 0; i < parsedLyrics.lines.length; i++) {
      const lineTime = parsedLyrics.lines[i].timeMs ?? 0;
      if (lineTime <= currentTimeMs) {
        found = i;
      } else {
        break;
      }
    }
    return found;
  }, [parsedLyrics, currentTimeMs]);

  // Handle user manual scrolling: pause auto-scroll temporarily
  const handleScroll = () => {
    setUserScrolled(true);
    if (scrollTimeoutRef.current) {
      clearTimeout(scrollTimeoutRef.current);
    }
    scrollTimeoutRef.current = setTimeout(() => {
      setUserScrolled(false);
    }, 3500);
  };

  // Reset scroll state on song change
  useEffect(() => {
    setUserScrolled(false);
    if (containerRef.current) {
      containerRef.current.scrollTop = 0;
    }
  }, [currentSong?.id]);

  // Auto-scroll to active line
  useEffect(() => {
    if (
      parsedLyrics?.isTimed &&
      activeIndex >= 0 &&
      activeLineRef.current &&
      !userScrolled
    ) {
      activeLineRef.current.scrollIntoView({
        behavior: "smooth",
        block: "center",
      });
    }
  }, [activeIndex, userScrolled, parsedLyrics?.isTimed]);

  const handleLineClick = async (line: LyricLine) => {
    if (line.timeMs !== undefined && currentSong?.duration_ms) {
      const fraction = Math.min(
        Math.max(line.timeMs / currentSong.duration_ms, 0),
        1,
      );
      await seek(fraction);
      setUserScrolled(false);
    }
  };

  return (
    <div
      ref={containerRef}
      onScroll={handleScroll}
      className={clsx(styles.container, className)}
    >
      {retrieving && (
        <div className={styles.statusText}>Retrieving lyrics...</div>
      )}

      {!retrieving && parsedLyrics && parsedLyrics.lines.length > 0 && (
        <>
          <div className={styles.lyricsWrapper}>
            {parsedLyrics.lines.map((line, index) => {
              const isTimed = parsedLyrics.isTimed;
              const isActive = isTimed && index === activeIndex;
              const isPassed =
                isTimed && activeIndex >= 0 && index < activeIndex;
              const isFuture =
                isTimed && (activeIndex < 0 || index > activeIndex);

              return (
                <div
                  key={index}
                  ref={isActive ? activeLineRef : undefined}
                  onClick={() => isTimed && handleLineClick(line)}
                  className={clsx(styles.lyricLine, {
                    [styles.timedLine]: isTimed,
                    [styles.activeLine]: isActive,
                    [styles.passedLine]: isPassed,
                    [styles.futureLine]: isFuture,
                    [styles.plainLine]: !isTimed,
                  })}
                >
                  {line.text.trim() || "\u00A0"}
                </div>
              );
            })}
          </div>

          <div className={styles.source}>
            Source:{" "}
            {source ? (
              <a
                href="#"
                onClick={(e) => {
                  e.preventDefault();
                  openUrl(source);
                }}
              >
                {source.replace(/^https?:\/\//, "")}
              </a>
            ) : (
              "Unknown"
            )}
          </div>
        </>
      )}

      {!retrieving && (!parsedLyrics || parsedLyrics.lines.length === 0) && (
        <div className={styles.emptyState}>No lyrics found.</div>
      )}
    </div>
  );
};
