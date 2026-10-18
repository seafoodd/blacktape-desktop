import { convertFileSrc } from "@tauri-apps/api/core";
import placeholderPlaylistImage from "@/assets/images/album_cover_placeholder_dark.jpg";
import { Song } from "@/shared/types.ts";
import styles from "./playlist-cover.module.css";
import clsx from "clsx";

interface PlaylistCoverProps {
  coverUrl?: string | null;
  previewCovers?: string | null;
  songs?: Song[];
  className?: string;
}

export const PlaylistCover = ({
  coverUrl,
  previewCovers,
  songs = [],
  className,
}: PlaylistCoverProps) => {
  if (coverUrl) {
    return (
      <img
        src={convertFileSrc(coverUrl)}
        className={clsx(styles.playlistImage, className)}
        alt=""
      />
    );
  }

  const uniqueCovers =
    songs.length > 0
      ? Array.from(
          new Set(songs.map((s) => s.cover_url).filter(Boolean)),
        ).slice(0, 4)
      : previewCovers
        ? previewCovers.split(",").filter(Boolean)
        : [];

  if (uniqueCovers.length >= 4) {
    return (
      <div className={clsx(styles.playlistImageGrid, className)}>
        {uniqueCovers.map((url, index) => (
          <img key={index} src={convertFileSrc(url!)} alt="" />
        ))}
      </div>
    );
  }

  const singleCover = uniqueCovers[0];
  return (
    <img
      src={singleCover ? convertFileSrc(singleCover) : placeholderPlaylistImage}
      className={clsx(styles.playlistImage, className)}
      alt=""
    />
  );
};
