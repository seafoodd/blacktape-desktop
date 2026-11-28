import { emitTo } from "@tauri-apps/api/event";
import { renderToStaticMarkup } from "react-dom/server";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { respondToYouTubeDownloadSignIn, YouTubeSignInModal } from "./YouTubeDownloadSignIn.tsx";

vi.mock("@tauri-apps/api/event", () => ({ emitTo: vi.fn() }));

beforeEach(() => vi.clearAllMocks());

describe("YouTube download sign-in", () => {
  it("explains why signing in is needed and waits for a choice", () => {
    const html = renderToStaticMarkup(<YouTubeSignInModal requestId="request-id" />);

    expect(html).toContain('role="dialog"');
    expect(html).toContain("This download uses YouTube for audio");
    expect(html).toContain("Continue to sign in on Google");
    expect(html).toContain("Cancel");
    expect(html).toContain("Continue to YouTube");
    expect(emitTo).not.toHaveBeenCalled();
  });

  it.each([true, false])("sends the explicit decision %s to the pending main-window request", async (proceed) => {
    await respondToYouTubeDownloadSignIn("request-id", proceed);

    expect(emitTo).toHaveBeenCalledExactlyOnceWith(
      { kind: "WebviewWindow", label: "main" },
      "download-youtube-sign-in-response",
      { request_id: "request-id", proceed },
    );
  });
});
