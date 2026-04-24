import { useCallback, useEffect, useState } from "react";
import Lyrics from "@/components/lyrics/Lyrics.tsx";
import styles from "./right-sidebar.module.css";

const RightSidebar = () => {
  const [sidebarWidth, setSidebarWidth] = useState(300);
  const [isResizing, setIsResizing] = useState(false);

  const startResizing = useCallback(() => setIsResizing(true), []);
  const stopResizing = useCallback(() => setIsResizing(false), []);

  const resize = useCallback((e: MouseEvent) => {
    // Math for Right Sidebar: Window Width - Mouse X Position
    const newWidth = window.innerWidth - e.clientX;
    if (newWidth > 150 && newWidth < 600) {
      setSidebarWidth(newWidth);
    }
  }, []);

  useEffect(() => {
    if (!isResizing) return;

    // Apply styles to body to prevent text selection and show cursor during drag
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
    <aside
      className={styles.rightSidebar}
      style={{ width: sidebarWidth }}
    >
      <div
        className={styles.resizer}
        onMouseDown={startResizing}
        // style={{
        //   position: "absolute",
        //   left: 0,
        //   top: 0,
        //   bottom: 0,
        //   width: "5px",
        //   cursor: "col-resize",
        //   zIndex: 10,
        // }}
      />
      <Lyrics />
    </aside>
  );
};

export default RightSidebar;
