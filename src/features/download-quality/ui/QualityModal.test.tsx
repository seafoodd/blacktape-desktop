import { invoke } from "@tauri-apps/api/core";
import { isValidElement, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ItemType, type StartDownloadParams } from "@/shared/store/libraryStore.ts";
import { useDownloadStore } from "@/shared/store/downloadStore.ts";
import { useModalStore } from "@/shared/store/modalStore.ts";
import { openQualityMenu, submitQualityDownload } from "@/features/download-quality/ui/QualityModal.tsx";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
  convertFileSrc: (path: string) => path,
}));

const album: StartDownloadParams = {
  platform: "youtube",
  downloadType: ItemType.Album,
  url: "canonical:album:example",
  title: "Example album",
  overrides: { tracks: [{ index: 0, title: "Example track" }] },
};

afterEach(() => {
  useModalStore.getState().closeModal();
  useDownloadStore.setState({ activeDownloads: {} });
  vi.clearAllMocks();
});

describe("quality selection submission", () => {
  it.each(["source:torrent:lossless", "best"])(
    "keeps the source-match confirmation open and starts %s after confirmation",
    (format) => {
      const start = vi.fn((selectedFormat?: string, overrides?: StartDownloadParams["overrides"]) => {
        const { openModal, closeModal } = useModalStore.getState();
        openModal(
          <button onClick={() => {
            closeModal();
            void useDownloadStore.getState().startDownload({ ...album, format: selectedFormat, overrides });
          }}>Download anyway</button>,
        );
      });
      openQualityMenu(album, { start });

      submitQualityDownload({ ...album, format }, { start });

      const { content, isOpen, stack } = useModalStore.getState();
      expect(isOpen).toBe(true);
      expect(stack).toHaveLength(0);
      expect(start).toHaveBeenCalledWith(format, album.overrides);
      expect(invoke).not.toHaveBeenCalled();
      expect(isValidElement(content)).toBe(true);
      const warning = content as ReactElement<{ children: string; onClick: () => void }>;
      expect(warning.props.children).toBe("Download anyway");

      warning.props.onClick();

      expect(useModalStore.getState().isOpen).toBe(false);
      expect(invoke).toHaveBeenCalledExactlyOnceWith("download", expect.objectContaining({
        url: album.url, format, overrides: album.overrides,
      }));
    },
  );

  it("closes the picker and starts a download directly when no custom confirmation is needed", () => {
    const onStarted = vi.fn();
    openQualityMenu(album);

    submitQualityDownload({ ...album, format: "best" }, { onStarted });

    expect(useModalStore.getState().isOpen).toBe(false);
    expect(invoke).toHaveBeenCalledExactlyOnceWith("download", expect.objectContaining({ url: album.url, format: "best" }));
    expect(onStarted).toHaveBeenCalledOnce();
  });
});
