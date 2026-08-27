import { useState } from "react";
import { WindowControls } from "@/layouts/WindowControls";
import { useTheme } from "@/shared/providers/theme-provider/ThemeProvider.tsx";
import { builtinThemes } from "@/shared/providers/theme-provider/themes.ts";
import { pickFolder } from "@/shared/lib/dialog.ts";
import { scanMusic } from "@/shared/lib/audio.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import styles from "./settings.module.css";
import { openPath } from "@tauri-apps/plugin-opener";

type SettingsTab = "appearance" | "playback" | "library";

export const Settings = () => {
  const { activeThemeId, setThemeId, fontSize, setFontSize } = useTheme();
  const { fetchTabs } = useLibraryStore();
  const [activeTab, setActiveTab] = useState<SettingsTab>("appearance");

  const [libraryDir, setLibraryDir] = useState<string>(() => {
    return localStorage.getItem("library_dir") || "Not set";
  });

  async function handleChangeFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    localStorage.setItem("library_dir", dir);
    setLibraryDir(dir);
    await scanMusic(dir);
    await fetchTabs();
  }

  async function handleOpenFolder() {
    if (libraryDir === "Not set") return;
    try {
      await openPath(libraryDir);
    } catch (err) {
      console.error("Failed to open folder:", err);
    }
  }

  return (
    <div data-tauri-drag-region className={styles.container}>
      <header data-tauri-drag-region className={styles.header}>
        <h1 className={styles.title}>Settings</h1>
        <WindowControls />
      </header>

      <div className={styles.body}>
        <aside className={styles.sidebar}>
          <button
            className={`${styles.navItem} ${activeTab === "appearance" ? styles.active : ""}`}
            onClick={() => setActiveTab("appearance")}
          >
            Appearance
          </button>
          <button
            className={`${styles.navItem} ${activeTab === "playback" ? styles.active : ""}`}
            onClick={() => setActiveTab("playback")}
          >
            Playback
          </button>
          <button
            className={`${styles.navItem} ${activeTab === "library" ? styles.active : ""}`}
            onClick={() => setActiveTab("library")}
          >
            Library
          </button>
        </aside>

        <main className={styles.content}>
          {activeTab === "appearance" && (
            <section className={styles.section}>
              <h2>Appearance</h2>
              <div className={styles.settingRow}>
                <label htmlFor="theme-select">Theme</label>
                <select
                  id="theme-select"
                  className={styles.select}
                  value={activeThemeId}
                  onChange={(e) => setThemeId(e.target.value)}
                >
                  {Object.values(builtinThemes).map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
              </div>

              <div className={styles.settingRow}>
                <label htmlFor="font-size-select">Font Size</label>
                <select
                  id="font-size-select"
                  className={styles.select}
                  value={fontSize}
                  onChange={(e) => setFontSize(e.target.value)}
                >
                  <option value="12px">Small (12px)</option>
                  <option value="14px">Medium (14px) (default)</option>
                  <option value="16px">Large (16px)</option>
                </select>
              </div>
            </section>
          )}

          {activeTab === "playback" && (
            <section className={styles.section}>
              <h2>Playback</h2>
              <div className={styles.settingRow}>
                <span>
                  Gapless Playback{" "}
                  <span className={styles.mutedText}>
                    (on by default, disabling unimplemented)
                  </span>
                </span>
                <input type="checkbox" defaultChecked disabled={true} />
              </div>
            </section>
          )}

          {activeTab === "library" && (
            <section className={styles.section}>
              <h2>Library</h2>
              <div className={styles.settingRowStart}>
                <div className={styles.libraryInfo}>
                  <span>Music Library Folder</span>
                  <span
                    className={`${styles.libraryPath} ${libraryDir !== "Not set" ? styles.clickablePath : ""}`}
                    onClick={handleOpenFolder}
                    title={libraryDir !== "Not set" ? "Open in Explorer" : ""}
                  >
                    {libraryDir}
                  </span>{" "}
                </div>
                <button
                  className={styles.actionButton}
                  onClick={handleChangeFolder}
                >
                  Change...
                </button>
              </div>

              <div className={styles.settingRow}>
                <span>
                  Auto-scan on startup{" "}
                  <span className={styles.mutedText}>(unimplemented)</span>
                </span>
                <input type="checkbox" disabled={true} />
              </div>
            </section>
          )}
        </main>
      </div>
    </div>
  );
};
