import { BrowserRouter, Route, Routes } from "react-router-dom";
import { PlayerControls } from "@/features/player";
import {
  fetchState,
  getOnboardingCompleted,
  restorePlaybackState,
  setOnboardingCompleted,
  setVolume,
} from "./shared/lib/audio";
import { useEffect } from "react";
import { Header, LeftSidebar, MainLayout, RightSidebar } from "@/layouts";
import ArtistAlbums from "@/pages/artist-albums/ArtistAlbums.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import SearchResults from "@/pages/search-results/SearchResults.tsx";
import { Settings } from "@/pages/settings/Settings";
import Playlist from "@/pages/playlist/Playlist.tsx";
import Trash from "@/pages/trash/Trash.tsx";
import Favorites from "@/pages/favorites/Favorites.tsx";
import AlbumPreview from "@/pages/album-preview/AlbumPreview.tsx";
import { SetupBanner } from "@/features/downloader-setup";
import { RemoteTrackMenuHost } from "@/features/remote-track-menu/ui/RemoteTrackMenuHost.tsx";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { WelcomeModal } from "@/features/onboarding";
import { useOnboardingStore } from "@/features/onboarding/model/onboardingStore.ts";
import {
  applyNavEntry,
  initNavigationListener,
} from "@/features/navigation/model/navigationStore.ts";
import { useGlobalShortcuts } from "@/features/keymaps/lib/useGlobalShortcuts.ts";
import { initKeymapsSyncListener } from "@/features/keymaps/model/keymapStore.ts";
import { useFavoritesStore } from "@/features/favorites/model/favoritesStore.ts";
import {
  loadSavedNavEntry,
  loadSavedPlayerState,
  loadSavedVolume,
} from "@/features/player/model/playerPersistence.ts";

function PlayerView() {
  const { activeView, initDownloadListeners } = useLibraryStore();
  useGlobalShortcuts();

  useEffect(() => {
    const unlistenNav = initNavigationListener();
    const unlistenKeymaps = initKeymapsSyncListener();

    const savedVol = loadSavedVolume();
    if (savedVol !== null) {
      void setVolume(savedVol);
    }

    const savedPlayer = loadSavedPlayerState();
    if (savedPlayer && savedPlayer.queue.length > 0) {
      void restorePlaybackState(
        savedPlayer.queue,
        savedPlayer.queueCursor ?? 0,
      );
    }

    const savedNav = loadSavedNavEntry();
    if (savedNav && (savedNav.view !== "ARTIST_ALBUMS" || savedNav.artistName)) {
      void applyNavEntry(savedNav);
    }

    useFavoritesStore
      .getState()
      .fetchFavoriteIds()
      .catch((e) => console.error("Failed to load favorite ids:", e));

    fetchState();
    initDownloadListeners().catch(console.error);

    const showOnboarding = () => {
      useOnboardingStore.getState().reset();
      useModalStore.getState().openModal(<WelcomeModal />, { dismissible: false });
    };

    getOnboardingCompleted()
      .then((completed) => {
        if (!completed) {
          showOnboarding();
        }
      })
      .catch((err) => console.error("Failed to check onboarding state:", err));

    if (typeof window !== "undefined") {
      (window as unknown as { __openWelcomeWizard?: () => void }).__openWelcomeWizard = showOnboarding;
      (window as unknown as { __resetOnboarding?: () => Promise<void> }).__resetOnboarding = async () => {
        await setOnboardingCompleted(false);
        showOnboarding();
      };
    }

    const channel = new BroadcastChannel("blacktape_onboarding_channel");
    channel.onmessage = (e) => {
      if (e.data?.type === "OPEN_ONBOARDING") {
        showOnboarding();
      }
    };

    return () => {
      unlistenNav();
      unlistenKeymaps();
      channel.close();
    };
  }, [initDownloadListeners]);

  return (
    <>
    <MainLayout
      header={<Header />}
      leftSidebar={<LeftSidebar />}
      rightSidebar={<RightSidebar />}
      footer={<PlayerControls />}
    >
      <SetupBanner />
      {activeView === "ARTIST_ALBUMS" && <ArtistAlbums />}
      {activeView === "SEARCH_RESULTS" && <SearchResults />}
      {activeView === "ALBUM_PREVIEW" && <AlbumPreview />}
      {activeView === "PLAYLIST" && <Playlist />}
      {activeView === "TRASH" && <Trash />}
      {activeView === "FAVORITES" && <Favorites />}
    </MainLayout>
    <RemoteTrackMenuHost />
    </>
  );
}

function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/" element={<PlayerView />} />
        <Route path="/settings" element={<Settings />} />
      </Routes>
    </BrowserRouter>
  );
}

export default App;
