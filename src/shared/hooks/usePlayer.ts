import { useEffect, useState } from "react";
import {
  Song,
  scanMusic,
  playSong as tauriPlay,
  getPosition,
} from "../lib/audio";

export function usePlayer() {
  const [songs, setSongs] = useState<Song[]>([]);
  const [progress, setProgress] = useState(0);
  const [currentSong, setCurrentSong] = useState<Song | null>(null);
  const [isPlaying, setIsPlaying] = useState(false);
  const [isDragging, setIsDragging] = useState(false);

  useEffect(() => {
    if (!currentSong) return;

    const interval = setInterval(async () => {
      if (isDragging) return;
      const pos = await getPosition();
      setProgress(pos);
    }, 500);

    return () => clearInterval(interval);
  }, [currentSong, isDragging]);

  const loadSongs = async (dir: string) => {
    const result = await scanMusic(dir);
    setSongs(result);
  };

  const play = async (song: Song) => {
    setCurrentSong(song);
    setIsPlaying(true);
    await tauriPlay(song);
  };

  return {
    songs,
    progress,
    currentSong,
    isPlaying,
    isDragging,
    setProgress,
    setIsDragging,
    setIsPlaying,
    loadSongs,
    play,
  };
}
