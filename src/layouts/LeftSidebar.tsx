import { useCallback, useEffect, useState } from "react";
import styles from "./left-sidebar.module.css";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { Tabs } from "@/shared/ui/tabs";
import clsx from "clsx";
import { Library, Playlists } from "@/features/library";

const TABS = [
  { id: "library", label: "Library" },
  { id: "playlists", label: "Playlists" },
] as const;
type TabId = (typeof TABS)[number]["id"];

export const LeftSidebar = () => {
  const { fetchTabs } = useLibraryStore();
  const [sidebarWidth, setSidebarWidth] = useState(260);
  const [isResizing, setIsResizing] = useState(false);
  const [activeTab, setActiveTab] = useState<TabId>("library");

  const startResizing = useCallback(() => {
    setIsResizing(true);
  }, []);

  const stopResizing = useCallback(() => {
    setIsResizing(false);
  }, []);

  const resize = useCallback(
    (mouseMoveEvent: MouseEvent) => {
      if (isResizing) {
        const newWidth = Math.min(Math.max(150, mouseMoveEvent.clientX), 600);
        setSidebarWidth(newWidth);
      }
    },
    [isResizing],
  );

  useEffect(() => {
    if (isResizing) {
      document.body.style.userSelect = "none";
      document.body.style.cursor = "col-resize";

      window.addEventListener("mousemove", resize);
      window.addEventListener("mouseup", stopResizing);
    }
    return () => {
      document.body.style.userSelect = "";
      document.body.style.cursor = "";

      window.removeEventListener("mousemove", resize);
      window.removeEventListener("mouseup", stopResizing);
    };
  }, [resize, stopResizing]);

  useEffect(() => {
    fetchTabs().catch((e) => {
      console.log("fetch tabs error: ", e);
    });
  }, []);

  return (
    <aside
      className={styles.leftSidebar}
      style={{ width: sidebarWidth }}
      onDragOver={(e) => {
        e.preventDefault();
        setActiveTab("playlists");
      }}
    >
      <Tabs tabs={TABS} activeTab={activeTab} onChange={setActiveTab} />
      <Library className={clsx({ [styles.hidden]: activeTab !== "library" })} />
      <Playlists
        className={clsx({ [styles.hidden]: activeTab !== "playlists" })}
      />
      <div className={styles.resizer} onMouseDown={startResizing} />
    </aside>
  );
};
