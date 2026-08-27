import {
  createContext,
  ReactNode,
  useContext,
  useEffect,
  useRef,
  useState,
} from "react";
import styles from "./dropdown.module.css";
import { MdArrowRight } from "react-icons/md";
import { FiCheck } from "react-icons/fi";

// --- Context ---
interface DropdownContextValue {
  closeAll: () => void;
}
const DropdownContext = createContext<DropdownContextValue | undefined>(
  undefined,
);

// --- Root Component ---
export const DropdownRoot = ({
  trigger,
  children,
}: {
  trigger: ReactNode;
  children: ReactNode;
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const closeAll = () => setIsOpen(false);

  useEffect(() => {
    const handleClickOutside = (e: PointerEvent) => {
      if (
        containerRef.current &&
        !containerRef.current.contains(e.target as Node)
      ) {
        closeAll();
      }
    };
    if (isOpen) document.addEventListener("pointerdown", handleClickOutside);
    return () =>
      document.removeEventListener("pointerdown", handleClickOutside);
  }, [isOpen]);

  return (
    <DropdownContext.Provider value={{ closeAll }}>
      <div className={styles.container} ref={containerRef}>
        <div
          className={styles.trigger}
          onClick={() => setIsOpen((prev) => !prev)}
        >
          {trigger}
        </div>
        {isOpen && <div className={styles.menu}>{children}</div>}
      </div>
    </DropdownContext.Provider>
  );
};

// --- Item Component ---
interface ItemProps {
  children: ReactNode;
  onClick?: () => void;
  active?: boolean;
  closeOnSelect?: boolean;
}

export const DropdownItem = ({
  children,
  onClick,
  active,
  closeOnSelect = true,
}: ItemProps) => {
  const ctx = useContext(DropdownContext);

  const handleClick = () => {
    onClick?.();
    if (closeOnSelect) {
      ctx?.closeAll();
    }
  };

  return (
    <button
      className={`${styles.item} ${active ? styles.activeItem : ""}`}
      onClick={handleClick}
    >
      {children}
      {active && (
        <span className={styles.checkmark}>
          <FiCheck size={16} />
        </span>
      )}
    </button>
  );
};

// --- Submenu Component ---
export const DropdownSub = ({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) => {
  const [isOpen, setIsOpen] = useState(false);
  const timeoutRef = useRef<NodeJS.Timeout | null>(null);

  // Solves the hover-tunnel issue by adding a slight delay before closing
  const handleMouseEnter = () => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
    setIsOpen(true);
  };

  const handleMouseLeave = () => {
    timeoutRef.current = setTimeout(() => {
      setIsOpen(false);
    }, 150); // 150ms safe intent delay
  };

  return (
    <div
      className={styles.subMenuWrapper}
      onMouseEnter={handleMouseEnter}
      onMouseLeave={handleMouseLeave}
    >
      <div className={styles.item}>
        <span>{label}</span>
        <MdArrowRight className={styles.arrow} size={24} />
      </div>
      {isOpen && <div className={styles.subMenu}>{children}</div>}
    </div>
  );
};

// --- Separator Component ---
export const DropdownSeparator = () => <div className={styles.separator} />;

// --- Export Object ---
export const Dropdown = {
  Root: DropdownRoot,
  Item: DropdownItem,
  Sub: DropdownSub,
  Separator: DropdownSeparator,
};
