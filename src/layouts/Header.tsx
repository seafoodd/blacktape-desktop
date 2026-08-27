import { SearchBar } from "@/features/search";
import { builtinThemes } from "@/shared/providers/theme-provider/themes.ts";
import { useTheme } from "@/shared/providers/theme-provider/ThemeProvider.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { pickFolder } from "@/shared/lib/dialog.ts";
import { scanMusic } from "@/shared/lib/audio.ts";
import { WindowControls } from "@/layouts/WindowControls.tsx";
import { Dropdown } from "@/shared/ui/dropdown/Dropdown.tsx";
import { CgMenuLeft } from "react-icons/cg";
import styles from "./header.module.css";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";

export const Header = () => {
  const { activeThemeId, setThemeId } = useTheme();
  const { fetchTabs } = useLibraryStore();

  async function handlePickFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    localStorage.setItem("library_dir", dir);
    await scanMusic(dir);
    await fetchTabs();
  }

  async function handleRescan() {
    const dir = localStorage.getItem("library_dir");
    if (!dir) {
      await handlePickFolder();
      return;
    }
    await scanMusic(dir);
    await fetchTabs();
  }

  const openNativeSettingsWindow = async () => {
    // Await the promise to get the actual WebviewWindow instance or null
    const existingWindow = await WebviewWindow.getByLabel("settings");

    if (existingWindow) {
      await existingWindow.setFocus();
      return;
    }

    const mainWindow = getCurrentWindow();
    const position = await mainWindow.outerPosition();
    const size = await mainWindow.outerSize();

    const width = 800;
    const height = 600;

    const x = position.x + Math.round((size.width - width) / 2);
    const y = position.y + Math.round((size.height - height) / 2);

    new WebviewWindow("settings", {
      url: "/settings",
      title: "Settings",
      width,
      height,
      x,
      y,
      decorations: false,
      transparent: true,
    });
  };

  return (
    <header data-tauri-drag-region className={styles.header}>
      <Dropdown.Root trigger={<CgMenuLeft size={24} />}>
        <Dropdown.Item onClick={handleRescan}>Rescan Library</Dropdown.Item>
        <Dropdown.Item onClick={handlePickFolder}>
          Change Library Folder...
        </Dropdown.Item>

        <Dropdown.Sub label="Theme">
          {Object.values(builtinThemes).map((theme) => (
            <Dropdown.Item
              key={theme.id}
              active={activeThemeId === theme.id}
              onClick={() => setThemeId(theme.id)}
              closeOnSelect={false}
            >
              {theme.name}
            </Dropdown.Item>
          ))}
        </Dropdown.Sub>

        <Dropdown.Separator />
        <Dropdown.Item onClick={openNativeSettingsWindow}>
          Settings
        </Dropdown.Item>
      </Dropdown.Root>

      <SearchBar className={styles.searchBar} />
      <WindowControls />
    </header>
  );
};
