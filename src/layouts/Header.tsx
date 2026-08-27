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

export const Header = () => {
  const { activeThemeId, setThemeId } = useTheme();
  const { fetchTabs } = useLibraryStore();

  async function handlePickFolder() {
    const dir = await pickFolder();
    if (!dir) return;
    await scanMusic(dir);
    await fetchTabs();
  }

  return (
    <header data-tauri-drag-region className={styles.header}>
      <Dropdown.Root trigger={<CgMenuLeft size={24} />}>
        <Dropdown.Item onClick={handlePickFolder}>Scan Library</Dropdown.Item>

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
        <Dropdown.Item onClick={() => console.log("settings")}>
          Settings
        </Dropdown.Item>
      </Dropdown.Root>

      <SearchBar className={styles.searchBar} />
      <WindowControls />
    </header>
  );
};
