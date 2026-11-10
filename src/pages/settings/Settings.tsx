import { useEffect, useState } from "react";
import { WindowControls } from "@/layouts/WindowControls";
import { useTheme } from "@/shared/providers/theme-provider/ThemeProvider.tsx";
import { builtinThemes } from "@/shared/providers/theme-provider/themes.ts";
import { pickFolder } from "@/shared/lib/dialog.ts";
import { scanMusic } from "@/shared/lib/audio.ts";
import {
  MAX_SIZE_OPTIONS,
  RETENTION_OPTIONS,
  getTrashSettings,
  gbToMb,
  mbToGb,
  setTrashSettings,
  type TrashSettings,
} from "@/shared/lib/trash.ts";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import styles from "./settings.module.css";
import { openPath } from "@tauri-apps/plugin-opener";
import { UpdateSettings } from "@/features/updater/ui/UpdateSettings";
import { AccountsSettings } from "@/features/accounts/ui/AccountsSettings";

type SettingsTab = "appearance" | "playback" | "library" | "accounts" | "about";

export const Settings = () => {
  const {
    activeThemeId,
    setThemeId,
    fontSize,
    setFontSize,
    bgOpacity,
    setBgOpacity,
  } = useTheme();
  const { fetchTabs } = useLibraryStore();
  const [activeTab, setActiveTab] = useState<SettingsTab>("appearance");

  const [libraryDir, setLibraryDir] = useState<string>(() => {
    return localStorage.getItem("library_dir") || "Not set";
  });

  const [trashConfig, setTrashConfig] = useState<TrashSettings | null>(null);

  useEffect(() => {
    getTrashSettings()
      .then(setTrashConfig)
      .catch((err) => console.error("Failed to load trash settings:", err));
  }, []);

  async function handleTrashSettingChange(next: TrashSettings) {
    const previous = trashConfig;
    setTrashConfig(next);
    try {
      await setTrashSettings(next.retention_days, next.max_size_mb);
    } catch (err) {
      console.error("Failed to save trash settings:", err);
      setTrashConfig(previous);
    }
  }

  // A stored limit that is not one of the offered whole-GB steps (for example
  // set outside this UI) gets its own option so the select never goes blank.
  const maxSizeMb = trashConfig?.max_size_mb ?? null;
  const maxSizeIsCustom =
    maxSizeMb !== null &&
    !MAX_SIZE_OPTIONS.some((o) => o.gb !== null && gbToMb(o.gb) === maxSizeMb);

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
          {/*<button*/}
          {/*  className={`${styles.navItem} ${activeTab === "playback" ? styles.active : ""}`}*/}
          {/*  onClick={() => setActiveTab("playback")}*/}
          {/*>*/}
          {/*  Playback*/}
          {/*</button>*/}
          <button
            className={`${styles.navItem} ${activeTab === "library" ? styles.active : ""}`}
            onClick={() => setActiveTab("library")}
          >
            Library
          </button>
          <button
            className={`${styles.navItem} ${activeTab === "accounts" ? styles.active : ""}`}
            onClick={() => setActiveTab("accounts")}
          >
            Accounts
          </button>
          <button
            className={`${styles.navItem} ${activeTab === "about" ? styles.active : ""}`}
            onClick={() => setActiveTab("about")}
          >
            About
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

              <div className={styles.settingRow}>
                <label htmlFor="bg-opacity-slider">
                  Background Opacity ({Math.round(bgOpacity * 100)}%)
                </label>
                <input
                  id="bg-opacity-slider"
                  type="range"
                  min="0"
                  max="1"
                  step="0.05"
                  value={bgOpacity}
                  onChange={(e) => setBgOpacity(parseFloat(e.target.value))}
                />
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
            <>
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

            <section className={styles.section}>
              <h2>Trash</h2>
              <div className={styles.settingRow}>
                <label htmlFor="trash-retention-select">Delete items after</label>
                <select
                  id="trash-retention-select"
                  className={styles.select}
                  disabled={!trashConfig}
                  value={
                    trashConfig?.retention_days === null
                      ? "never"
                      : String(trashConfig?.retention_days ?? "")
                  }
                  onChange={(e) =>
                    trashConfig &&
                    handleTrashSettingChange({
                      ...trashConfig,
                      retention_days:
                        e.target.value === "never"
                          ? null
                          : Number(e.target.value),
                    })
                  }
                >
                  {RETENTION_OPTIONS.map((o) => (
                    <option
                      key={o.label}
                      value={o.days === null ? "never" : String(o.days)}
                    >
                      {o.label}
                    </option>
                  ))}
                </select>
              </div>

              <div className={styles.settingRow}>
                <label htmlFor="trash-size-select">Maximum Trash size</label>
                <select
                  id="trash-size-select"
                  className={styles.select}
                  disabled={!trashConfig}
                  value={
                    !trashConfig
                      ? ""
                      : maxSizeMb === null
                        ? "unlimited"
                        : maxSizeIsCustom
                          ? "custom"
                          : String(mbToGb(maxSizeMb))
                  }
                  onChange={(e) => {
                    if (!trashConfig || e.target.value === "custom") return;
                    handleTrashSettingChange({
                      ...trashConfig,
                      max_size_mb:
                        e.target.value === "unlimited"
                          ? null
                          : gbToMb(Number(e.target.value)),
                    });
                  }}
                >
                  {maxSizeIsCustom && (
                    <option value="custom">Custom ({maxSizeMb} MB)</option>
                  )}
                  {MAX_SIZE_OPTIONS.map((o) => (
                    <option
                      key={o.label}
                      value={o.gb === null ? "unlimited" : String(o.gb)}
                    >
                      {o.label}
                    </option>
                  ))}
                </select>
              </div>
            </section>
            </>
          )}

          {activeTab === "accounts" && (
            <section className={styles.section}>
              <h2>Accounts</h2>
              <AccountsSettings buttonClassName={styles.actionButton} />
            </section>
          )}

          {activeTab === "about" && (
            <section className={styles.section}>
              <h2>About</h2>
              <UpdateSettings
                rowClassName={styles.settingRow}
                buttonClassName={styles.actionButton}
                mutedClassName={styles.mutedText}
              />
            </section>
          )}
        </main>
      </div>
    </div>
  );
};
