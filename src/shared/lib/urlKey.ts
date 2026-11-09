/**
 * Comparable key of a page url (the frontend twin of `normalize_url_key` in the Rust library index, kept small):
 * scheme, `www.`/`m.`/`music.` prefixes, query, fragment and trailing slashes are ignored, the rest is lowercased.
 * YouTube watch / shorts / youtu.be urls are keyed by their video id (`yt:<id>`, case-sensitive).
 * Null for an empty or unparsable url.
 */
export function urlKey(url: string | null | undefined): string | null {
  const trimmed = url?.trim();
  if (!trimmed) return null;
  let parsed: URL;
  try {
    parsed = new URL(trimmed.includes("://") ? trimmed : `https://${trimmed.replace(/^\/\//, "")}`);
  } catch {
    return null;
  }
  if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return null;
  // Page urls carry no credentials; this also rejects "mailto:a@b.com", which parses as userinfo.
  if (parsed.username || parsed.password) return null;
  const host = parsed.hostname.toLowerCase().replace(/^(www|m|music)\./, "");
  if (!host.includes(".")) return null;

  if (host === "youtu.be") {
    const id = parsed.pathname.split("/").filter(Boolean)[0];
    if (id) return `yt:${id}`;
  }
  if (host === "youtube.com") {
    const watch = parsed.searchParams.get("v");
    if (parsed.pathname === "/watch" && watch) return `yt:${watch}`;
    const shorts = /^\/shorts\/([^/]+)/.exec(parsed.pathname);
    if (shorts) return `yt:${shorts[1]}`;
  }
  const port = parsed.port ? `:${parsed.port}` : "";
  return `${host}${port}${parsed.pathname.replace(/\/+$/, "").toLowerCase()}`;
}

/** True when both urls point at the same page. */
export function sameUrl(a: string | null | undefined, b: string | null | undefined): boolean {
  const ka = urlKey(a);
  return ka !== null && ka === urlKey(b);
}
