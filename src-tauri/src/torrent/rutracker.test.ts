import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { describe, expect, it, vi } from "vitest";

const script = readFileSync(new URL("./rutracker.js", import.meta.url), "utf8");

function searchWithResponse(response: Response, origin = "https://rutracker.org/forum/index.php") {
  const lookup = runInNewContext(`(${script})`, {
    location: new URL(origin),
    fetch: vi.fn().mockResolvedValue(response),
  });
  return lookup("/forum/tracker.php?nm=test", "search", new AbortController().signal);
}

describe("RuTracker access errors", () => {
  it("accepts rutracker.net as an official mirror", async () => {
    const response = new Response(null, { status: 401 });
    await expect(searchWithResponse(response, "https://rutracker.net/forum/index.php")).resolves.toEqual({
      error: "sign_in",
    });
  });
  it("reports the Cloudflare challenge that blocks authentication", async () => {
    const response = new Response(null, { status: 403, headers: { "Cf-Mitigated": "challenge" } });
    await expect(searchWithResponse(response)).resolves.toEqual({ error: "cloudflare_challenge" });
  });

  it("reports a 403 Cloudflare interstitial body as cloudflare_challenge", async () => {
    const response = new Response("<html><title>Just a moment...</title></html>", { status: 403 });
    await expect(searchWithResponse(response)).resolves.toEqual({ error: "cloudflare_challenge" });
  });

  it.each([
    [401, "sign_in"],
    [403, "blocked"],
    [429, "rate_limited"],
  ])("reports a plain HTTP %s as %s", async (status, error) => {
    await expect(searchWithResponse(new Response(null, { status }))).resolves.toEqual({ error });
  });
});
