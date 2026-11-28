import { useEffect, useId, useRef, useState } from "react";
import clsx from "clsx";
import { MdKeyboardArrowDown } from "react-icons/md";
import type { AlbumSourceOption } from "@/pages/album-preview/albumSourceSelection.ts";
import styles from "./album-preview.module.css";

export function AlbumQualityPicker({
  options, trackCount, onSelect,
}: {
  options: AlbumSourceOption[];
  trackCount: number;
  onSelect: (option: AlbumSourceOption) => void;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !ref.current?.contains(event.target)) setOpen(false);
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.stopPropagation();
      setOpen(false);
      button.current?.focus();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", escape);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", escape);
    };
  }, [open]);

  return (
    <div ref={ref} className={styles.candidateSelector}>
      <button
        ref={button}
        type="button"
        className={clsx(styles.secondaryButton, styles.albumQualityButton)}
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        disabled={!open && options.length === 0}
        onClick={() => setOpen((value) => !value)}
      >
        Select quality
        <MdKeyboardArrowDown className={clsx(styles.arrowIcon, open && styles.arrowOpen)} aria-hidden="true" />
      </button>
      {open && (
        <div id={menuId} className={clsx(styles.candidateMenu, styles.albumQualityMenu)} role="group" aria-label="Album audio sources">
          <div className={styles.candidateMenuHeader}>Apply to all tracks</div>
          {options.map((option) => (
            <button
              key={option.key}
              type="button"
              className={styles.candidateMenuItem}
              onClick={() => {
                onSelect(option);
                setOpen(false);
                button.current?.focus();
              }}
            >
              <span>{option.label}</span>
              <span className={styles.albumQualityCoverage}>{option.trackCount}/{trackCount} tracks</span>
            </button>
          ))}
          {options.some((option) => option.trackCount < trackCount) && (
            <div className={styles.albumQualityNote}>Other tracks use the closest available quality.</div>
          )}
        </div>
      )}
    </div>
  );
}
