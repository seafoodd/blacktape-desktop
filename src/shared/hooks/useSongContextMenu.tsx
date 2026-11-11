import { MenuItemOption } from "@/shared/ui/context-menu";
import { Song } from "@/shared/types.ts";
import { AddNextToQueue, AddToQueue } from "@/shared/lib/audio.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { PlaybackContext, useAudioStore } from "@/shared/store/audioStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import { ItemType, useLibraryStore, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { openQualityMenu } from "@/features/download-quality/ui/QualityModal.tsx";
import { refreshLibrary } from "@/shared/lib/library/libraryRefresh.ts";
import { canRedownload as canRedownloadSong } from "@/shared/lib/remote/platforms.ts";
import { EditLyricsModal } from "@/features/player/ui/EditLyricsModal.tsx";
import EditSongInfoModal from "@/features/library/ui/EditSongInfoModal.tsx";
import FetchLyricsModal from "@/features/library/ui/FetchLyricsModal.tsx";
import DeleteSongModal from "@/features/library/ui/DeleteSongModal.tsx";

interface UseSongContextMenuOptions {
  playbackContext: PlaybackContext;
  contextId?: number;
  /** `index` is the row the menu was opened on, when the list passes one. */
  onPlay?: (song: Song, index?: number) => void;
  onRemoveFromPlaylist?: (index: number) => void;
  /** Text of the remove item (default "Remove from Playlist"). */
  removeLabel?: string;
  /** Adds a "Delete..." item (remove from library / delete file). */
  allowDelete?: boolean;
}

export const useSongContextMenu = (options: UseSongContextMenuOptions) => {
  const { currentSong, isPlaying, togglePlay, startPlayback } = useAudioStore();
  const { openModal } = useModalStore();

  /**
   * `isCurrentRow` overrides the default "same id as the playing song" check, for
   * lists where one song can appear several times and only one row is playing.
   */
  const getContextMenuItems = (
    song: Song,
    index?: number,
    isCurrentRow?: boolean,
  ): MenuItemOption[] => {
    const isCurrentSong =
      isCurrentRow ?? (currentSong && song.id === currentSong.id);

    const items: MenuItemOption[] = [
      isCurrentSong
        ? isPlaying
          ? {
              label: "Pause",
              action: () => {
                togglePlay().catch((e) => {
                  console.error("context menu play/pause button error: ", e);
                });
              },
            }
          : {
              label: "Resume",
              action: () => {
                togglePlay().catch((e) => {
                  console.error("context menu Resume button error: ", e);
                });
              },
            }
        : {
            label: "Play",
            action: () => {
              if (options?.onPlay) {
                options.onPlay(song, index);
              } else {
                startPlayback(
                  [song.id],
                  0,
                  options.playbackContext,
                  options.contextId,
                ).catch((e) => console.error("startPlayback error: ", e));
              }
            },
          },
      ...(!isCurrentSong && song.id != null
        ? [
            {
              label: "Play Next",
              action: () => {
                AddNextToQueue(song.id).catch((e) => {
                  console.error("AddNextToQueue error: ", e);
                });
              },
            },
            {
              label: "Add to Queue",
              action: () => {
                AddToQueue(song.id).catch((e) => {
                  console.error("AddToQueue error: ", e);
                });
              },
            },
          ]
        : []),
      ...(song.id != null
        ? [
            {
              separator: true,
              label: "",
            },
            {
              label: "Add to Playlist...",
              action: () => {
                openModal(<AddToPlaylistModal songIds={[song.id]} />);
              },
            },
          ]
        : []),
    ];

    const artistText =
      song.artists.join(", ") || song.album_artist || "Unknown Artist";
    const copy = (text: string) =>
      navigator.clipboard
        .writeText(text)
        .catch((e) => console.error("clipboard error: ", e));
    const canRedownload = canRedownloadSong(song.source, song.source_url);
    const redownloadParams = (): StartDownloadParams => ({
      platform: song.source,
      downloadType: ItemType.Track,
      url: song.source_url!,
      artistName: song.album_artist || artistText,
      title: song.title,
    });

    items.push(
      { separator: true, label: "" },
      {
        label: "Copy",
        submenu: [
          { label: "Title", action: () => copy(song.title) },
          {
            label: "Artist – Title",
            action: () => copy(`${artistText} – ${song.title}`),
          },
        ],
      },
      { separator: true, label: "" },
      {
        label: "Edit Info...",
        action: () => openModal(<EditSongInfoModal song={song} />),
      },
      {
        label: "Edit Lyrics...",
        action: () =>
          openModal(
            <EditLyricsModal
              song={song}
              initialLyrics={song.lyrics ?? ""}
              initialSource={song.lyrics_source}
              onSuccess={() => {
                refreshLibrary().catch((e) =>
                  console.error("refreshLibrary error: ", e),
                );
              }}
            />,
          ),
      },
      {
        label: "Fetch Lyrics...",
        action: () => openModal(<FetchLyricsModal song={song} />),
      },
      { separator: true, label: "" },
      ...(song.source_url
        ? [
            {
              label: "Open Source Page",
              action: () => {
                openUrl(song.source_url!).catch((e) =>
                  console.error("openUrl error: ", e),
                );
              },
            },
          ]
        : []),
      ...(canRedownload
        ? [
            {
              label: "Re-download",
              action: () => {
                useLibraryStore
                  .getState()
                  .startDownload(redownloadParams())
                  .catch((e) => console.error("re-download error: ", e));
              },
            },
            {
              label: "Re-download as…",
              action: () => openQualityMenu(redownloadParams()),
            },
          ]
        : []),
      {
        label: "Show in Explorer",
        action: () => {
          revealItemInDir(song.path).catch((e) =>
            console.error("revealItemInDir error: ", e),
          );
        },
      },
    );

    if (options.allowDelete) {
      items.push(
        { separator: true, label: "" },
        {
          label: "Delete...",
          danger: true,
          action: () => openModal(<DeleteSongModal songs={[song]} />),
        },
      );
    }

    if (index === undefined) return items;

    if (options.onRemoveFromPlaylist) {
      items.push({
        label: options.removeLabel ?? "Remove from Playlist",
        action: () => {
          if (options.onRemoveFromPlaylist) {
            options.onRemoveFromPlaylist(index);
          }
        },
      });
    }

    return items;
  };

  return { getContextMenuItems };
};
