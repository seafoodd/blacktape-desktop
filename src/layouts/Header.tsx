import { SearchBar } from "@/features/search";
import { UpdateButton } from "@/features/updater";
import { builtinThemes } from "@/shared/providers/theme-provider/themes.ts";
import { useTheme } from "@/shared/providers/theme-provider/ThemeProvider.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import { pickFolder } from "@/shared/lib/ui/dialog.ts";
import { scanMusic } from "@/shared/lib/audio.ts";
import { WindowControls } from "@/layouts/WindowControls.tsx";
import { Dropdown } from "@/shared/ui/dropdown";
import { MAIN_HEADER_HEIGHT, dismissOnPress, useChromeHeight } from "@/shared/ui/modal";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { CgMenuLeft } from "react-icons/cg";
import { MdChevronLeft, MdChevronRight, MdHome } from "react-icons/md";
import { useNavigationStore } from "@/features/navigation/model/navigationStore.ts";
import styles from "./header.module.css";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";

export const Header = () => {
  const { activeThemeId, setThemeId } = useTheme();
  const { fetchTabs, setActiveView, activeView, selectedTab } =
    useLibraryStore();
  const canGoBack = useNavigationStore((s) => s.canGoBack());
  const canGoForward = useNavigationStore((s) => s.canGoForward());
  const goBack = useNavigationStore((s) => s.goBack);
  const goForward = useNavigationStore((s) => s.goForward);
  const goHome = useNavigationStore((s) => s.goHome);
  const isHome = activeView === "ARTIST_ALBUMS" && !selectedTab;
  useChromeHeight(MAIN_HEADER_HEIGHT);
  // The header stays above an open modal (to drag the window). A press on its menu or search closes the modal
  // instead of using them.
  const modalOpen = useModalStore((s) => s.isOpen);
  const closeModal = useModalStore((s) => s.closeModal);
  const dismiss = dismissOnPress(modalOpen, closeModal);

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
      <div className={styles.leftNav}>
        <Dropdown.Root trigger={<CgMenuLeft size={24} />} {...dismiss}>
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

          <Dropdown.Item onClick={() => setActiveView("TRASH")}>
            Trash
          </Dropdown.Item>

          <Dropdown.Separator />
          <Dropdown.Item onClick={openNativeSettingsWindow}>
            Settings
          </Dropdown.Item>
        </Dropdown.Root>

        <div className={styles.navButtons}>
          <button
            type="button"
            className={styles.navButton}
            onClick={() => void goBack()}
            disabled={!canGoBack}
            title="Go back (Alt + ←)"
            aria-label="Go back"
          >
            <MdChevronLeft size={22} />
          </button>
          <button
            type="button"
            className={styles.navButton}
            onClick={() => void goForward()}
            disabled={!canGoForward}
            title="Go forward (Alt + →)"
            aria-label="Go forward"
          >
            <MdChevronRight size={22} />
          </button>
          {!isHome && (
            <button
              type="button"
              className={styles.navButton}
              onClick={() => void goHome()}
              title="Home"
              aria-label="Home"
            >
              <MdHome size={19} />
            </button>
          )}
        </div>
      </div>

      <SearchBar className={styles.searchBar} {...dismiss} />
      {/* one flex item on the right: with two auto margins the update pill landed in the middle, under the search bar */}
      <div className={styles.end}>
        <UpdateButton />
        <WindowControls />
      </div>
    </header>
  );
};
