import { MdClose } from "react-icons/md";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { isRunning } from "@/shared/lib/library/downloadMatch.ts";
import { CancelDownloadModal } from "@/features/library/ui/CancelDownloadModal.tsx";
import styles from "./cancel-download-button.module.css";

type Props = { taskId: string; taskIds?: never } | { taskIds: readonly string[]; taskId?: never };

export const CancelDownloadButton = ({ taskId, taskIds }: Props) => {
  const active = useDownloadStore((state) => state.activeDownloads);
  const tasks = [...new Set(taskIds ?? [taskId])].flatMap((id) => {
    const task = id ? active[id] : undefined;
    return task && isRunning(task) ? [task] : [];
  });
  if (tasks.length === 0) return null;
  const task = tasks[0];
  const title = tasks.length === 1 && task.itemType === "Track"
    ? task.tracks[0]?.title || task.albumTitle || "Download"
    : task.albumTitle || "Download";
  const pending = tasks.some((task) => task.cancelling);
  const label = pending ? "Cancelling download…" : `Cancel ${tasks.length > 1 ? "downloads for" : "download of"} ${title}`;

  return (
    <button
      type="button"
      className={styles.button}
      title={label}
      aria-label={label}
      aria-busy={pending || undefined}
      disabled={pending}
      onMouseDown={(event) => { event.preventDefault(); event.stopPropagation(); }}
      onDoubleClick={(event) => event.stopPropagation()}
      onClick={(event) => {
        event.stopPropagation();
        useModalStore.getState().openModal(
          <CancelDownloadModal
            title={title}
            artist={task.artistName}
            taskCount={tasks.length}
            onConfirm={async () => {
              await Promise.all(tasks.map((task) => useDownloadStore.getState().cancelDownload(task.id)));
            }}
          />,
        );
      }}
    >
      {pending ? <span className={styles.spinner} aria-hidden="true" /> : <MdClose size={18} />}
    </button>
  );
};
