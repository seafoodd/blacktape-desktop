import { BrowserRouter, Route, Routes } from "react-router-dom";
import { PlayerControls } from "@/features/player";
import { fetchState } from "./shared/lib/audio";
import { useEffect } from "react";
import { Header, LeftSidebar, MainLayout, RightSidebar } from "@/layouts";
import ArtistAlbums from "@/pages/artist-albums/ArtistAlbums.tsx";
import { useLibraryStore } from "@/shared/store/libraryStore.ts";
import SearchResults from "@/pages/search-results/SearchResults.tsx";
import { Settings } from "@/pages/settings/Settings";
import Playlist from "@/pages/playlist/Playlist.tsx";
import Trash from "@/pages/trash/Trash.tsx";
import AlbumPreview from "@/pages/album-preview/AlbumPreview.tsx";
import { RemoteTrackMenuHost } from "@/features/remote-track-menu/ui/RemoteTrackMenuHost.tsx";

function PlayerView() {
  const { activeView, initDownloadListeners } = useLibraryStore();

  useEffect(() => {
    fetchState();
    initDownloadListeners().catch(console.error);
  }, [initDownloadListeners]);

  return (
    <>
    <MainLayout
      header={<Header />}
      leftSidebar={<LeftSidebar />}
      rightSidebar={<RightSidebar />}
      footer={<PlayerControls />}
    >
      {activeView === "ARTIST_ALBUMS" && <ArtistAlbums />}
      {activeView === "SEARCH_RESULTS" && <SearchResults />}
      {activeView === "ALBUM_PREVIEW" && <AlbumPreview />}
      {activeView === "PLAYLIST" && <Playlist />}
      {activeView === "TRASH" && <Trash />}
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
