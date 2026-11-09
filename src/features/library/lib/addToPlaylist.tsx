import { playlistApi } from "@/shared/api/playlists.ts";
import { usePlaylistStore } from "@/shared/store/playlistStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { splitDuplicates } from "@/shared/lib/playlistDuplicates.ts";
import {
  DuplicateChoice,
  DuplicateSongsModal,
} from "@/features/library/ui/playlists/DuplicateSongsModal.tsx";

/** Asks yes/no in a modal. Closing the modal any other way counts as "cancel". */
function askAboutDuplicates(
  playlistName: string | undefined,
  total: number,
  duplicates: number,
): Promise<DuplicateChoice> {
  return new Promise((resolve) => {
    let settled = false;
    const settle = (choice: DuplicateChoice) => {
      if (settled) return;
      settled = true;
      unsubscribe();
      resolve(choice);
    };
    const unsubscribe = useModalStore.subscribe((state) => {
      if (!state.isOpen) settle("cancel");
    });
    useModalStore
      .getState()
      .openModal(
        <DuplicateSongsModal
          playlistName={playlistName}
          total={total}
          duplicates={duplicates}
          onChoose={settle}
        />,
      );
  });
}

/**
 * Adds songs to a playlist, asking first when some of them are already in it.
 * Returns how many songs were added (0 when the user said no).
 */
export async function addSongsToPlaylist(
  playlistId: number,
  songIds: number[],
): Promise<number> {
  const existing = await playlistApi.getSongs(playlistId);
  const { fresh, duplicates } = splitDuplicates(
    existing.map((s) => s.id),
    songIds,
  );

  let toAdd = songIds;
  if (duplicates.length > 0) {
    const name = usePlaylistStore
      .getState()
      .playlists.find((p) => p.id === playlistId)?.name;
    const choice = await askAboutDuplicates(
      name,
      songIds.length,
      duplicates.length,
    );
    if (choice === "cancel") return 0;
    if (choice === "skip") toAdd = fresh;
  }

  if (toAdd.length === 0) return 0;
  await usePlaylistStore.getState().addSongs(playlistId, toAdd);
  return toAdd.length;
}
