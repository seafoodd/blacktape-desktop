import { useAudioStore } from "@/shared/store/audioStore.ts";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import styles from "./lyrics.module.css";
import { openUrl } from "@tauri-apps/plugin-opener";

const Lyrics = () => {
  const { currentSong } = useAudioStore();
  const [lyrics, setLyrics] = useState<string | null>(null);
  const [retrieving, setRetrieving] = useState<boolean>(false);
  const [source, setSource] = useState<string | null>(null);

  useEffect(() => {
    if (!currentSong?.id) {
      setLyrics(null);
      return;
    }

    let isCurrentRequest = true;
    setRetrieving(true);
    setLyrics(null);
    setSource(null);

    const fetchLyrics = async () => {
      try {
        const lyricsSource: { lyrics: string; source: string } = await invoke<{
          lyrics: string;
          source: string;
        }>("get_lyrics", {
          id: currentSong.id,
        });

        const lyr = lyricsSource.lyrics;

        if (isCurrentRequest) {
          setLyrics(lyricsSource && lyr.trim().length > 0 ? lyr : null);
          setSource(lyricsSource.source);
        }
      } catch (err) {
        console.error("Failed to fetch lyrics:", err);
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

  return (
    <div className={styles.container}>
      {retrieving && (
        <div className={styles.statusText}>Retrieving lyrics...</div>
      )}

      {!retrieving && lyrics && (
        <>
          <div className={styles.lyricsWrapper}>
            {lyrics.split("\n").map((line, index) => (
              <div key={index} className={styles.lyricLine}>
                {line.trim() || "\u00A0"}
              </div>
            ))}
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
                {source.replace("https://", "").replace("http://", "")}
              </a>
            ) : (
              "Unknown"
            )}
          </div>
        </>
      )}

      {!retrieving && !lyrics && (
        <div className={styles.noLyrics}>No lyrics found.</div>
      )}
    </div>
  );
};
export default Lyrics;
