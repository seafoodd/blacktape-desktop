import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import styles from "./context-menu.module.css";
import clsx from "clsx";
import { MdArrowRight } from "react-icons/md";

export interface ContextMenuPosition {
  x: number;
  y: number;
}

export interface MenuItemOption {
  label: string;
  action?: () => void;
  separator?: boolean;
  danger?: boolean;
  /** Nested items shown in a flyout on hover/click; `action` is ignored when set. */
  submenu?: MenuItemOption[];
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
  const [openSub, setOpenSub] = useState<{ index: number; flip: boolean } | null>(
    null,
  );

  const showSubmenu = (index: number, el: HTMLElement) => {
    // open to the left when the flyout would run off the right edge
    const flip = el.getBoundingClientRect().right + 200 > window.innerWidth;
    setOpenSub({ index, flip });
  };

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

        if (item.submenu) {
          const isOpen = openSub?.index === index;
          return (
            <div
              key={`${item.label}-${index}`}
              className={styles.submenuWrap}
              onMouseEnter={(e) => showSubmenu(index, e.currentTarget)}
              onMouseLeave={() => setOpenSub(null)}
            >
              <button
                className={clsx(styles.item, {
                  [styles.itemDanger]: item.danger,
                })}
                aria-haspopup="menu"
                aria-expanded={isOpen}
                onClick={(e) => showSubmenu(index, e.currentTarget.parentElement!)}
              >
                <span>{item.label}</span>
                <MdArrowRight className={styles.arrow} size={24} />
              </button>
              {isOpen && (
                <div
                  className={clsx(styles.menu, styles.submenu, {
                    [styles.submenuFlip]: openSub?.flip,
                  })}
                >
                  {item.submenu.map((sub, subIndex) => (
                    <button
                      key={`${sub.label}-${subIndex}`}
                      className={clsx(styles.item, {
                        [styles.itemDanger]: sub.danger,
                      })}
                      onClick={() => {
                        sub.action?.();
                        onClose();
                      }}
                    >
                      <span>{sub.label}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          );
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
