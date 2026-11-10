import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

// Reads the real index.html at the repo root (this file lives in src/shared/lib).
const html = readFileSync(fileURLToPath(new URL("../../../index.html", import.meta.url)), "utf8");

// Tolerates extra whitespace, attribute order, quote style and an optional self-closing slash.
const referrerMeta = /<meta\b[^>]*\bname\s*=\s*["']referrer["'][^>]*\bcontent\s*=\s*["']no-referrer["'][^>]*\/?>/i;

const headMatch = /<head\b[^>]*>([\s\S]*?)<\/head\s*>/i.exec(html);

describe("index.html referrer policy", () => {
  // Google image hosts (YouTube covers) sometimes reject requests carrying the app's referrer
  // (net::ERR_BLOCKED_BY_ORB), so the whole window must send no referrer.
  it("sends no referrer for the whole window via a <meta name=\"referrer\"> in <head>", () => {
    expect(headMatch).not.toBeNull();
    expect(headMatch![1]).toMatch(referrerMeta);
  });

  // The policy only takes effect when the tag is parsed as part of the document head.
  it("places the referrer meta tag inside <head>, not in <body>", () => {
    expect(headMatch).not.toBeNull();
    const headStart = headMatch!.index;
    const headEnd = headStart + headMatch![0].length;

    const tag = referrerMeta.exec(html);
    expect(tag).not.toBeNull();
    expect(tag!.index).toBeGreaterThan(headStart);
    expect(tag!.index + tag![0].length).toBeLessThanOrEqual(headEnd);
    expect(html.search(/<body\b/i)).toBeGreaterThan(headEnd);
  });
});
