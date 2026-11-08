import { useCallback, useEffect, useRef, useState } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import clsx from "clsx";
import {
  MdDeleteForever,
  MdDeleteOutline,
  MdInfoOutline,
  MdMusicNote,
  MdRestore,
} from "react-icons/md";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { coverSrc } from "@/shared/lib/cover";
import { refreshLibrary } from "@/shared/lib/libraryRefresh.ts";
import {
  TRASH_CHANGED_EVENT,
  deleteTrash,
  describeTrashPolicy,
  emptyTrash,
  formatBytes,
  formatDaysLeft,
  formatTrashedDate,
  getTrashSettings,
  listTrash,
  restoreTrash,
  trashDaysLeft,
  type TrashItem,
  type TrashOutcome,
  type TrashSettings,
} from "@/shared/lib/trash.ts";
import styles from "./trash.module.css";

const nowSecs = () => Math.floor(Date.now() / 1000);

const artistLine = (item: TrashItem): string =>
  [item.artists.join(", ") || "Unknown artist", item.album]
    .filter(Boolean)
    .join(" · ");

interface ConfirmTrashModalProps {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
}

const ConfirmTrashModal = ({
  title,
  message,
  confirmLabel,
  onConfirm,
}: ConfirmTrashModalProps) => {
  const closeModal = useModalStore((state) => state.closeModal);

  return (
    <div className={styles.modal}>
      <h2 className={styles.modalTitle}>{title}</h2>
      <p className={styles.modalText}>{message}</p>
      <div className={styles.modalButtons}>
        <button className={styles.cancelButton} onClick={closeModal}>
          Cancel
        </button>
        <button
          className={styles.dangerButton}
          onClick={() => {
            onConfirm();
            closeModal();
          }}
        >
          {confirmLabel}
        </button>
      </div>
    </div>
  );
};

const Trash = () => {
  const openModal = useModalStore((state) => state.openModal);
  const [items, setItems] = useState<TrashItem[] | null>(null);
  const [settings, setSettings] = useState<TrashSettings | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<number>>(new Set());

  // Overlapping loads (e.g. two trash-changed events) can finish out of order.
  // Only the most recent request may write state.
  const latestLoad = useRef(0);

  const load = useCallback(async () => {
    const requestId = ++latestLoad.current;
    try {
      const [list, current] = await Promise.all([
        listTrash(),
        getTrashSettings(),
      ]);
      if (requestId !== latestLoad.current) return;
      setItems(list);
      setSettings(current);
      // Drop selections of items that are gone (restored, deleted, auto-cleaned).
      setSelected((prev) => {
        const ids = new Set(list.map((i) => i.id));
        const next = new Set([...prev].filter((id) => ids.has(id)));
        return next.size === prev.size ? prev : next;
      });
    } catch (e) {
      if (requestId !== latestLoad.current) return;
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    let disposed = false;
    let unlisten: UnlistenFn | undefined;

    load();
    listen(TRASH_CHANGED_EVENT, () => {
      load();
    })
      .then((fn) => {
        // The effect may have been cleaned up before listen resolved.
        if (disposed) fn();
        else unlisten = fn;
      })
      .catch((e) => console.error("Failed to listen for trash changes:", e));

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [load]);

  // Runs a trash mutation, shows its failures and reloads the list.
  const runAction = async (action: () => Promise<TrashOutcome>) => {
    setBusy(true);
    setError(null);
    try {
      const outcome = await action();
      if (outcome.failures.length > 0) setError(outcome.failures.join("\n"));
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRestore = (ids: number[]) =>
    runAction(async () => {
      const outcome = await restoreTrash(ids);
      if (outcome.done > 0) {
        await refreshLibrary();
      }
      return outcome;
    });

  const list = items ?? [];

  const confirmDelete = (ids: number[]) => {
    const single = ids.length === 1 ? list.find((i) => i.id === ids[0]) : null;
    openModal(
      <ConfirmTrashModal
        title="Delete Permanently"
        message={
          single
            ? `"${single.title}" will be deleted from disk. This cannot be undone.`
            : `${ids.length} songs will be deleted from disk. This cannot be undone.`
        }
        confirmLabel="Delete Permanently"
        onConfirm={() => runAction(() => deleteTrash(ids))}
      />,
    );
  };

  const selectedIds = list.filter((i) => selected.has(i.id)).map((i) => i.id);
  const allSelected = list.length > 0 && selectedIds.length === list.length;

  const toggleOne = (id: number) =>
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const toggleAll = () =>
    setSelected(allSelected ? new Set() : new Set(list.map((i) => i.id)));

  const totalBytes = list.reduce((sum, item) => sum + item.size_bytes, 0);

  const confirmEmpty = () =>
    openModal(
      <ConfirmTrashModal
        title="Empty Trash"
        message={`Permanently delete ${list.length} ${list.length === 1 ? "song" : "songs"} (${formatBytes(totalBytes)}) from disk? This cannot be undone.`}
        confirmLabel="Empty Trash"
        onConfirm={() => runAction(emptyTrash)}
      />,
    );

  const now = nowSecs();

  return (
    <div className={styles.container}>
      <div className={styles.header}>
        <div className={styles.headerText}>
          <h1 className={styles.title}>
            <MdDeleteOutline size={28} />
            Trash
          </h1>
          <span className={styles.summary}>
            {items === null
              ? "Loading..."
              : `${list.length} ${list.length === 1 ? "song" : "songs"} · ${formatBytes(totalBytes)}`}
          </span>
        </div>
        <button
          className={clsx(styles.button, styles.dangerOutline)}
          onClick={confirmEmpty}
          disabled={busy || list.length === 0}
        >
          Empty Trash
        </button>
      </div>

      {settings && (
        <div className={styles.policy}>
          <MdInfoOutline size={18} />
          <span>{describeTrashPolicy(settings)}</span>
        </div>
      )}
      {error && <div className={styles.error}>{error}</div>}

      {items !== null && list.length === 0 && (
        <div className={styles.empty}>
          <MdDeleteOutline size={56} className={styles.emptyIcon} />
          <span className={styles.emptyTitle}>Trash is empty</span>
          <span className={styles.emptyText}>
            Songs you move to the Trash from the library show up here until you
            restore or delete them.
          </span>
        </div>
      )}

      {list.length > 0 && (
        <div
          className={clsx(
            styles.toolbar,
            selectedIds.length > 0 && styles.toolbarActive,
          )}
        >
          <label className={styles.selectAll}>
            <input
              type="checkbox"
              className={styles.checkbox}
              checked={allSelected}
              onChange={toggleAll}
            />
            <span>
              {selectedIds.length > 0
                ? `${selectedIds.length} selected`
                : "Select all"}
            </span>
          </label>
          {selectedIds.length > 0 && (
            <div className={styles.bulkActions}>
              <button
                className={clsx(styles.button, styles.primary)}
                onClick={() => handleRestore(selectedIds)}
                disabled={busy}
              >
                <MdRestore size={18} />
                Restore
              </button>
              <button
                className={clsx(styles.button, styles.dangerOutline)}
                onClick={() => confirmDelete(selectedIds)}
                disabled={busy}
              >
                <MdDeleteForever size={18} />
                Delete
              </button>
            </div>
          )}
        </div>
      )}

      {list.length > 0 && (
        <ul className={styles.list}>
          {list.map((item) => {
            const daysLeft = settings
              ? trashDaysLeft(item.trashed_at, settings.retention_days, now)
              : null;
            const isSelected = selected.has(item.id);
            return (
              <li
                key={item.id}
                className={clsx(styles.row, isSelected && styles.rowSelected)}
              >
                <input
                  type="checkbox"
                  className={styles.checkbox}
                  checked={isSelected}
                  onChange={() => toggleOne(item.id)}
                  aria-label={`Select ${item.title}`}
                />
                <div className={styles.thumb}>
                  {item.cover_url ? (
                    <img
                      loading="lazy"
                      decoding="async"
                      src={coverSrc(item.cover_url, 44)}
                      referrerPolicy="no-referrer"
                      alt=""
                      className={styles.thumbImg}
                      onError={(e) => {
                        e.currentTarget.style.display = "none";
                      }}
                    />
                  ) : null}
                  <MdMusicNote size={22} className={styles.thumbIcon} />
                </div>
                <div className={styles.info}>
                  <span
                    className={clsx(styles.rowTitle, "truncate")}
                    title={item.original_path}
                  >
                    {item.title}
                  </span>
                  <span className={clsx(styles.rowMeta, "truncate")}>
                    {artistLine(item)}
                  </span>
                </div>
                <span className={styles.size}>
                  {formatBytes(item.size_bytes)}
                </span>
                <div className={styles.dates}>
                  <span>{formatTrashedDate(item.trashed_at)}</span>
                  {daysLeft !== null && (
                    <span
                      className={clsx(
                        styles.daysLeft,
                        daysLeft <= 3 && styles.daysLeftSoon,
                      )}
                    >
                      {formatDaysLeft(daysLeft)}
                    </span>
                  )}
                </div>
                <div className={styles.actions}>
                  <button
                    className={styles.iconButton}
                    onClick={() => handleRestore([item.id])}
                    disabled={busy}
                    title="Restore"
                    aria-label="Restore"
                  >
                    <MdRestore size={20} />
                  </button>
                  <button
                    className={clsx(styles.iconButton, styles.iconDanger)}
                    onClick={() => confirmDelete([item.id])}
                    disabled={busy}
                    title="Delete permanently"
                    aria-label="Delete permanently"
                  >
                    <MdDeleteForever size={20} />
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
};

export default Trash;
