import { MenuItemOption } from "@/shared/ui/context-menu";
import { Song } from "@/shared/types.ts";
import { AddNextToQueue, AddToQueue } from "@/shared/lib/audio.ts";
import { AddToPlaylistModal } from "@/features/library/ui/playlists/AddToPlaylistModal.tsx";
import { PlaybackContext, useAudioStore } from "@/shared/store/audioStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";

interface UseSongContextMenuOptions {
  playbackContext: PlaybackContext;
  contextId?: number;
  onPlay?: (song: Song) => void;
  onRemoveFromPlaylist?: (index: number) => void;
}

export const useSongContextMenu = (options: UseSongContextMenuOptions) => {
  const { currentSong, isPlaying, togglePlay, startPlayback } = useAudioStore();
  const { openModal } = useModalStore();

  const getContextMenuItems = (
    song: Song,
    index?: number,
  ): MenuItemOption[] => {
    const isCurrentSong = currentSong && song.id === currentSong.id;

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
                options.onPlay(song);
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
      ...(!isCurrentSong
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
    ];

    if (index === undefined) return items;

    if (options.onRemoveFromPlaylist) {
      items.push({
        label: "Remove from Playlist",
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
