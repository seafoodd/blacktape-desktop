async (path, kind, signal) => {
    if (location.origin !== "https://rutracker.org" && location.origin !== "https://rutracker.net") return { error: "unavailable" };
    const response = await fetch(path, { redirect: "manual", cache: "no-store", signal });
    if (response.headers.get("cf-mitigated") === "challenge") return { error: "cloudflare_challenge" };
    if (response.type === "opaqueredirect" || response.status === 401) return { error: "sign_in" };
    if (response.status === 429) return { error: "rate_limited" };
    if (!response.ok) {
        if (response.status === 403) {
            const body = await response.text().catch(() => "");
            if (/just a moment|cf-browser-verification|cf_chl|challenges\.cloudflare\.com/i.test(body)) {
                return { error: "cloudflare_challenge" };
            }
            return { error: "blocked" };
        }
        return { error: "unavailable" };
    }
    const contentType = response.headers.get("content-type") || "";
    if (kind === "torrent" && contentType.includes("text/html")) return { error: "sign_in" };
    if (!response.body) return { error: "malformed" };
    const reader = response.body.getReader();
    const chunks = [];
    let size = 0;
    while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        size += value.length;
        if (size > 4 * 1024 * 1024) {
            void reader.cancel();
            return { error: "response_too_large" };
        }
        chunks.push(value);
    }
    const bytes = new Uint8Array(size);
    let offset = 0;
    for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.length;
    }
    if (kind === "torrent") {
        const blocks = [];
        for (let start = 0; start < bytes.length; start += 16384) {
            blocks.push(String.fromCharCode(...bytes.subarray(start, start + 16384)));
        }
        return { data: btoa(blocks.join("")) };
    }
    const charset = contentType.match(/charset\s*=\s*["']?([\w-]+)/i)?.[1] || "windows-1251";
    const page = new DOMParser().parseFromString(new TextDecoder(charset).decode(bytes), "text/html");
    if (page.querySelector('form[action*="login.php"]') && !page.querySelector("#logged-in-username")) {
        return { error: "sign_in" };
    }
    const table = page.querySelector("#tor-tbl");
    if (!table && !page.querySelector("#logged-in-username")) return { error: "malformed" };
    const rows = [];
    for (const row of table?.querySelectorAll("tbody tr") || []) {
        const title = row.querySelector(".t-title a.tLink")?.textContent.trim();
        const link = row.querySelector("td.tor-size a.tr-dl")?.getAttribute("href");
        if (!title || title.length > 1536 || !link || link.length > 512) continue;
        const seeds = row.querySelector("td:nth-child(7) b")?.textContent.trim();
        rows.push({ title, link, seeders: /^\d+$/.test(seeds || "") ? Number(seeds) : null });
        if (rows.length === 100) break;
    }
    return { data: rows };
}
