import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "./lyrics.module.css";
import { openUrl } from "@tauri-apps/plugin-opener";
import clsx from "clsx";
import { LyricLine, parseLyrics } from "../lib/lyricsParser";
import {
  ContextMenu,
  ContextMenuPosition,
  MenuItemOption,
} from "@/shared/ui/context-menu";
import { useModalStore } from "@/shared/store/modalStore";
import { EditLyricsModal } from "./EditLyricsModal";

const invokeWithTimeout = async <T,>(
  cmd: string,
  args?: Record<string, unknown>,
  timeoutMs = 15000,
): Promise<T> => {
  let timer: ReturnType<typeof setTimeout>;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(() => {
      reject(new Error("Request timed out. Please try again."));
    }, timeoutMs);
  });

  try {
    return await Promise.race([invoke<T>(cmd, args), timeoutPromise]);
  } finally {
    clearTimeout(timer!);
  }
};

interface LyricsProps {
  className?: string;
}

export const Lyrics = ({ className }: LyricsProps) => {
  const { currentSong, progress, isPlaying, seek, updateProgress } =
    useAudioStore();
  const { openModal } = useModalStore();

  const [lyrics, setLyrics] = useState<string | null>(null);
  const [retrieving, setRetrieving] = useState<boolean>(false);
  const [source, setSource] = useState<string | null>(null);
  const [errorStatus, setErrorStatus] = useState<string | null>(null);

  const [activeMenu, setActiveMenu] = useState<{
    position: ContextMenuPosition;
  } | null>(null);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const activeLineRef = useRef<HTMLDivElement | null>(null);
  const [userScrolled, setUserScrolled] = useState<boolean>(false);
  const scrollTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (!currentSong?.id) {
      setLyrics(null);
      setSource(null);
      setErrorStatus(null);
      return;
    }

    let isCurrentRequest = true;
    setRetrieving(true);
    setLyrics(null);
    setSource(null);
    setErrorStatus(null);

    const fetchLyrics = async () => {
      try {
        const lyricsSource = await invokeWithTimeout<{
          lyrics: string;
          source: string;
        }>("get_lyrics", {
          id: currentSong.id,
        });

        if (isCurrentRequest) {
          const lyr = lyricsSource?.lyrics;
          setLyrics(lyr && lyr.trim().length > 0 ? lyr : null);
          setSource(lyricsSource?.source ?? null);
          setErrorStatus(null);
        }
      } catch (err: unknown) {
        console.log("Failed to fetch lyrics:", err);
        if (isCurrentRequest) {
          setLyrics(null);
          setSource(null);
          setErrorStatus(
            typeof err === "string"
              ? err
              : err instanceof Error
                ? err.message
                : "No lyrics found.",
          );
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

  const handleRefetch = async (provider?: string) => {
    if (!currentSong?.id) return;
    setRetrieving(true);
    setErrorStatus(null);
    try {
      const lyricsSource = await invokeWithTimeout<{
        lyrics: string;
        source: string;
      }>("refetch_lyrics", {
        id: currentSong.id,
        provider: provider ?? null,
      });

      const lyr = lyricsSource?.lyrics;
      setLyrics(lyr && lyr.trim().length > 0 ? lyr : null);
      setSource(lyricsSource?.source ?? null);
      setErrorStatus(null);
    } catch (err: unknown) {
      console.error(
        `Failed to refetch lyrics${provider ? ` from ${provider}` : ""}:`,
        err,
      );
      setErrorStatus(
        typeof err === "string"
          ? err
          : err instanceof Error
            ? err.message
            : `Failed to fetch lyrics${provider ? ` from ${provider}` : ""}.`,
      );
    } finally {
      setRetrieving(false);
    }
  };

  const handleDeleteLyrics = async () => {
    if (!currentSong?.id) return;
    try {
      await invoke("delete_lyrics", { id: currentSong.id });
      setLyrics(null);
      setSource(null);
      setErrorStatus("Lyrics deleted.");
    } catch (err) {
      console.error("Failed to delete lyrics:", err);
    }
  };

  const handleEditLyrics = () => {
    if (!currentSong?.id) return;
    openModal(
      <EditLyricsModal
        song={currentSong}
        initialLyrics={lyrics ?? ""}
        initialSource={source}
        onSuccess={(newLyrics, newSource) => {
          setLyrics(newLyrics.trim().length > 0 ? newLyrics : null);
          setSource(newSource);
          setErrorStatus(null);
        }}
      />,
    );
  };

  const handleContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    if (!currentSong?.id) return;
    setActiveMenu({
      position: { x: e.clientX, y: e.clientY },
    });
  };

  const contextMenuItems: MenuItemOption[] = useMemo(() => {
    if (!currentSong?.id) return [];

    return [
      {
        label: "Refetch Lyrics",
        action: () => handleRefetch(),
      },
      {
        separator: true,
        label: "",
      },
      {
        label: "Fetch from LRCLIB",
        action: () => handleRefetch("lrclib"),
      },
      {
        label: "Fetch from Musixmatch",
        action: () => handleRefetch("musixmatch"),
      },
      {
        label: "Fetch from lyrics.ovh",
        action: () => handleRefetch("lyrics.ovh"),
      },
      {
        label: "Fetch from Genius",
        action: () => handleRefetch("genius"),
      },
      {
        separator: true,
        label: "",
      },
      {
        label: "Edit Lyrics...",
        action: handleEditLyrics,
      },
      {
        label: "Delete Lyrics",
        danger: true,
        action: handleDeleteLyrics,
      },
    ];
  }, [currentSong?.id, lyrics, source]);

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
      onContextMenu={handleContextMenu}
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
              const isEmpty = !line.text.trim();

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
                    [styles.emptyTimedLine]: isTimed && isEmpty,
                  })}
                >
                  {isTimed
                    ? isEmpty
                      ? "♫"
                      : line.text.trim()
                    : line.text.trim() || "\u00A0"}
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
        <div className={styles.emptyState}>
          {errorStatus || "No lyrics found."}
        </div>
      )}

      {activeMenu && (
        <ContextMenu
          position={activeMenu.position}
          onClose={() => setActiveMenu(null)}
          items={contextMenuItems}
        />
      )}
    </div>
  );
};
