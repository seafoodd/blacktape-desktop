import { useCallback, useEffect, useState } from "react";
import { Lyrics } from "@/features/player";
import styles from "./right-sidebar.module.css";

const MIN_WIDTH = 150;
const MAX_WIDTH = 600;

export const RightSidebar = () => {
  const [sidebarWidth, setSidebarWidth] = useState(300);
  const [isResizing, setIsResizing] = useState(false);

  const startResizing = useCallback(() => setIsResizing(true), []);
  const stopResizing = useCallback(() => setIsResizing(false), []);

  const resize = useCallback((e: MouseEvent) => {
    const rawWidth = window.innerWidth - e.clientX;
    const clampedWidth = Math.min(Math.max(rawWidth, MIN_WIDTH), MAX_WIDTH);
    setSidebarWidth(clampedWidth);
  }, []);

  useEffect(() => {
    if (!isResizing) return;

    document.body.style.userSelect = "none";
    document.body.style.cursor = "col-resize";

    window.addEventListener("mousemove", resize);
    window.addEventListener("mouseup", stopResizing);

    return () => {
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
      window.removeEventListener("mousemove", resize);
      window.removeEventListener("mouseup", stopResizing);
    };
  }, [isResizing, resize, stopResizing]);

  return (
    <aside className={styles.rightSidebar} style={{ width: sidebarWidth }}>
      <div className={styles.resizer} onMouseDown={startResizing} />
      <div className={styles.rightSidebarContent}>
        <Lyrics />
      </div>
    </aside>
  );
};