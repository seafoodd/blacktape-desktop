import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import styles from "./context-menu.module.css";
import clsx from "clsx";

export interface ContextMenuPosition {
  x: number;
  y: number;
}

export interface MenuItemOption {
  label: string;
  action?: () => void;
  separator?: boolean;
  danger?: boolean;
}

interface ContextMenuProps {
  position: ContextMenuPosition;
  onClose: () => void;
  items: MenuItemOption[];
}

export const ContextMenu = ({ position, onClose, items }: ContextMenuProps) => {
  const menuRef = useRef<HTMLDivElement>(null);
  const [adjustedPosition, setAdjustedPosition] =
    useState<ContextMenuPosition>(position);

  // Adjust menu coordinates if it overflows the viewport window boundaries
  useLayoutEffect(() => {
    if (!menuRef.current) return;

    const { innerWidth, innerHeight } = window;
    const { offsetWidth, offsetHeight } = menuRef.current;

    let newX = position.x;
    let newY = position.y;

    if (position.x + offsetWidth > innerWidth) {
      newX = innerWidth - offsetWidth - 8;
    }
    if (position.y + offsetHeight > innerHeight) {
      newY = innerHeight - offsetHeight - 8;
    }

    setAdjustedPosition({ x: Math.max(8, newX), y: Math.max(8, newY) });
  }, [position]);

  useEffect(() => {
    const handlePointerDown = (e: MouseEvent) => {
      if (menuRef.current && menuRef.current.contains(e.target as Node)) {
        return;
      }
      onClose();
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };

    const handleResize = () => onClose();

    window.addEventListener("pointerdown", handlePointerDown, {
      capture: true,
    });
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("pointerdown", handlePointerDown, {
        capture: true,
      });
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("resize", handleResize);
    };
  }, [onClose]);

  return createPortal(
    <div
      ref={menuRef}
      className={styles.menu}
      style={{
        top: `${adjustedPosition.y}px`,
        left: `${adjustedPosition.x}px`,
      }}
      onClick={(e) => e.stopPropagation()}
      onContextMenu={(e) => e.preventDefault()}
    >
      {items.map((item, index) => {
        if (item.separator) {
          return <div key={`sep-${index}`} className={styles.separator} />;
        }

        return (
          <button
            key={`${item.label}-${index}`}
            className={clsx(styles.item, {
              [styles.itemDanger]: item.danger,
            })}
            onClick={() => {
              item.action?.();
              onClose();
            }}
          >
            <span>{item.label}</span>
          </button>
        );
      })}
    </div>,
    document.body,
  );
};
