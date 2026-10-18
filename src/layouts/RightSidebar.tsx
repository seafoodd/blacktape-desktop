import { useCallback, useEffect, useState } from "react";
import { Lyrics } from "@/features/player";
import styles from "./right-sidebar.module.css";
import { Tabs } from "@/shared/ui/tabs";
import Queue from "@/features/player/ui/Queue.tsx";
import clsx from "clsx";

const MIN_WIDTH = 150;
const MAX_WIDTH = 600;

const TABS = [
  { id: "lyrics", label: "Lyrics" },
  { id: "queue", label: "Queue" },
] as const;
type TabId = (typeof TABS)[number]["id"];

export const RightSidebar = () => {
  const [activeTab, setActiveTab] = useState<TabId>("lyrics");
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
      <Tabs tabs={TABS} activeTab={activeTab} onChange={setActiveTab} />
      <div className={styles.rightSidebarContent}>
        <Lyrics className={clsx({ [styles.hidden]: activeTab !== "lyrics" })} />
        <Queue className={clsx({ [styles.hidden]: activeTab !== "queue" })} />
      </div>
    </aside>
  );
};
