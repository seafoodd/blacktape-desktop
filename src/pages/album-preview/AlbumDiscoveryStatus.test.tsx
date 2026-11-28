import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AlbumDiscoveryStatus } from "@/pages/album-preview/AlbumDiscoveryStatus.tsx";

describe("album discovery status", () => {
  it("shows edition, streaming and torrent progress independently above the controls", () => {
    const html = renderToStaticMarkup(
      <AlbumDiscoveryStatus
        tracks={[]}
        loadingEditions
        resolvingCandidates
        switchingRelease={false}
        torrent={{ phase: "estimated", estimates: [], progress: { stage: "releases", checked: 2, total: 5 } }}
      />,
    );
    expect(html).toContain('aria-live="polite"');
    expect(html).toContain("MusicBrainz");
    expect(html).toContain("Checking streaming sources");
    expect(html).toContain("Matching torrent files · 2/5 releases checked");
  });

  it("distinguishes unavailable indexes from an empty torrent result", () => {
    const html = renderToStaticMarkup(
      <AlbumDiscoveryStatus
        tracks={[]}
        loadingEditions={false}
        resolvingCandidates={false}
        switchingRelease={false}
        torrent={{ phase: "done", resolutions: [], unavailableProviders: ["Knaben"] }}
      />,
    );
    expect(html).toContain("No matching torrent sources found · Unavailable indexes: Knaben");
    expect(html).not.toContain("MusicBrainz");
    expect(html).not.toContain("Sign in to RuTracker");
  });

  it("offers a RuTracker sign-in shortcut when RuTracker is unavailable", () => {
    const html = renderToStaticMarkup(
      <AlbumDiscoveryStatus
        tracks={[]}
        loadingEditions={false}
        resolvingCandidates={false}
        switchingRelease={false}
        torrent={{ phase: "done", resolutions: [], unavailableProviders: ["RuTracker"] }}
      />,
    );
    expect(html).toContain("Unavailable indexes: RuTracker");
    expect(html).toContain("Sign in to RuTracker");
  });
});
