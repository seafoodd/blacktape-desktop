import { createServer } from "vite";
import { describe, expect, it } from "vitest";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { spawn, type ChildProcess } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";

const enabled = process.env.BLACKTAPE_UI_TESTS === "1";
const edgePath = "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe";

class Cdp {
  private socket: WebSocket;
  private sequence = 0;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void }>();
  private sessions = new Map<string, string>();

  constructor(endpoint: string) {
    this.socket = new WebSocket(endpoint);
    this.socket.addEventListener("message", (event) => {
      const message = JSON.parse(String(event.data));
      if (message.method === "Target.attachedToTarget") this.sessions.set(message.params.targetInfo.targetId, message.params.sessionId);
      if (message.id !== undefined) {
        const pending = this.pending.get(message.id);
        if (pending) {
          this.pending.delete(message.id);
          message.error ? pending.reject(new Error(message.error.message)) : pending.resolve(message.result);
        }
      }
    });
  }

  async ready() {
    await new Promise<void>((resolve, reject) => {
      this.socket.addEventListener("open", () => resolve(), { once: true });
      this.socket.addEventListener("error", () => reject(new Error("Could not connect to owned Edge browser")), { once: true });
    });
  }

  send(method: string, params: object = {}, sessionId?: string): Promise<any> {
    const id = ++this.sequence;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Timed out waiting for CDP ${method}`));
      }, 5000);
      this.pending.set(id, {
        resolve: (value) => { clearTimeout(timer); resolve(value); },
        reject: (error) => { clearTimeout(timer); reject(error); },
      });
      this.socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }

  close() { this.socket.close(); }
  sessionId(targetId: string) { return this.sessions.get(targetId); }
}

describe.skipIf(!enabled)("AlbumTrackRow browser interaction regression", () => {
  it("keeps cancel button hit-testable and aligned with percentage across viewport widths", async () => {
    let child: ChildProcess | undefined;
    let profile: string | undefined;
    let cdp: Cdp | undefined;
    let vite: Awaited<ReturnType<typeof createServer>> | undefined;
    try {
      profile = await mkdtemp(path.join(tmpdir(), "blacktape-album-browser-"));
      const fixture = path.resolve("src/pages/album-preview/AlbumTrackRow.browser.fixture.html");
      vite = await createServer({
        configFile: path.resolve("vite.config.ts"),
        cacheDir: path.join(profile, "vite-cache"),
        optimizeDeps: { entries: [fixture] },
        server: { host: "127.0.0.1", port: 0, strictPort: false },
      });
      const address = await vite.listen();
      const url = `http://127.0.0.1:${(address.httpServer!.address() as import("node:net").AddressInfo).port}/${path.relative(process.cwd(), fixture).replace(/\\/g, "/")}`;
      child = spawn(edgePath, ["--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--remote-allow-origins=*", "--remote-debugging-port=0", `--user-data-dir=${profile}`, "about:blank"], { stdio: "ignore", windowsHide: true });
      const portFile = path.join(profile, "DevToolsActivePort");
      let endpoint = "";
      for (let i = 0; i < 100; i++) {
        if (child.exitCode !== null) throw new Error(`Owned Edge exited with code ${child.exitCode}`);
        try {
          const [port, browserPath] = (await readFile(portFile, "utf8")).trim().split(/\r?\n/);
          endpoint = `ws://127.0.0.1:${port}${browserPath}`;
          break;
        } catch { await delay(100); }
      }
      if (!endpoint) throw new Error("Timed out waiting for owned Edge DevTools endpoint");
      cdp = new Cdp(endpoint);
      await cdp.ready();
      const { targetId } = await cdp.send("Target.createTarget", { url: "about:blank" });
      await cdp.send("Target.attachToTarget", { targetId, flatten: true });
      const deadline = Date.now() + 3000;
      while (!cdp.sessionId(targetId) && Date.now() < deadline) await delay(10);
      const session = cdp.sessionId(targetId);
      if (!session) throw new Error("Could not attach to owned Edge target");
      await cdp.send("Page.enable", {}, session);
      await cdp.send("Runtime.enable", {}, session);
      await cdp.send("Page.navigate", { url }, session);
      let rendered = false;
      const renderDeadline = Date.now() + 10_000;
      let renderError = "";
      while (!rendered && Date.now() < renderDeadline) {
        try {
          rendered = await evaluate(cdp, session, `!!document.querySelector('[aria-label="Cancel download of Track zero"]')`);
          if (!rendered) await delay(100);
        } catch (error) {
          renderError = String(error);
          await delay(100);
        }
      }
      if (!rendered) {
        const detail = await evaluate(cdp, session, `({ url: location.href, title: document.title, body: document.body?.innerText?.slice(0, 1200) || '', errors: [...document.querySelectorAll('vite-error-overlay')].map((e) => e.innerText) })`).catch((error) => ({ evaluateError: String(error) }));
        throw new Error(`Fixture did not render the cancel button within 10s. ${renderError} ${JSON.stringify(detail)}`);
      }

      for (const width of [800, 1100]) {
        await cdp.send("Emulation.setDeviceMetricsOverride", { width, height: 720, deviceScaleFactor: 1, mobile: false }, session);
        await delay(100);
        const bounds = await evaluate(cdp, session, `(() => {
          const button = document.querySelector('[aria-label="Cancel download of Track zero"]');
          const percent = button?.parentElement?.querySelector('[aria-live="polite"]');
          const row = button?.closest('[data-song-row]');
          if (!button || !percent || !row) throw new Error('Fixture did not render active track controls');
          const rect = (element) => { const r = element.getBoundingClientRect(); return { x:r.x, y:r.y, width:r.width, height:r.height }; };
          return { button: rect(button), percent: rect(percent), row: rect(row), initialWidth: row.getBoundingClientRect().width };
        })()`);
        const rowX = Math.round(bounds.row.x + 100);
        const rowY = Math.round(bounds.row.y + bounds.row.height / 2);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: rowX, y: rowY }, session);
        await delay(80);
        const revealed = await evaluate(cdp, session, `(() => { const r = document.querySelector('[aria-label="Cancel download of Track zero"]').getBoundingClientRect(); return { x:r.x, y:r.y, width:r.width, height:r.height }; })()`);
        const x = Math.round(revealed.x + revealed.width / 2);
        const y = Math.round(revealed.y + revealed.height / 2);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x, y }, session);
        await delay(80);
        const hover = await evaluate(cdp, session, `(() => {
          const button = document.querySelector('[aria-label="Cancel download of Track zero"]');
          const percent = button.parentElement.querySelector('[aria-live="polite"]');
          const row = button.closest('[data-song-row]');
          const hit = document.elementFromPoint(${x}, ${y});
          return { hitButton: hit === button || button.contains(hit), cursor: getComputedStyle(button).cursor,
            buttonOpacity: getComputedStyle(button).opacity, percentageOpacity: getComputedStyle(percent).opacity,
            percentageVisibility: getComputedStyle(percent).visibility, percentagePointerEvents: getComputedStyle(percent).pointerEvents,
            hovered: button.matches(':hover'), rowWidth: row.getBoundingClientRect().width,
            buttonCenter: button.getBoundingClientRect().x + button.getBoundingClientRect().width / 2,
            percentCenter: percent.getBoundingClientRect().x + percent.getBoundingClientRect().width / 2,
            checkCenter: (() => { const r = document.querySelector('[aria-label="Downloaded"]').getBoundingClientRect(); return r.x + r.width / 2; })(),
            edgeHits: [-9,0,9].every(offset => { const el = document.elementFromPoint(${x} + offset, ${y}); return el === button || button.contains(el); }) };
        })()`);
        expect(hover.hitButton, `cancel button receives pointer at ${width}px viewport`).toBe(true);
        expect(hover.hovered).toBe(true);
        expect(hover.cursor).toBe("pointer");
        expect(hover.buttonOpacity).toBe("1");
        expect(hover.percentageVisibility).toBe("hidden");
        expect(hover.percentagePointerEvents).toBe("none");
        expect(hover.edgeHits).toBe(true);
        expect(Math.abs(hover.buttonCenter - hover.percentCenter)).toBeLessThanOrEqual(2);
        expect(Math.abs(hover.buttonCenter - hover.checkCenter)).toBeLessThanOrEqual(0.5);
        expect(hover.rowWidth).toBeCloseTo(bounds.initialWidth, 1);

        if (process.env.BLACKTAPE_UI_SCREENSHOT) {
          const screenshot = await cdp.send("Page.captureScreenshot", { format: "png" }, session);
          const screenshotPath = path.resolve(process.env.BLACKTAPE_UI_SCREENSHOT);
          await mkdir(path.dirname(screenshotPath), { recursive: true });
          await writeFile(screenshotPath, Buffer.from(screenshot.data, "base64"));
        }

        await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x, y, button: "left", clickCount: 1 }, session);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x, y, button: "left", clickCount: 1 }, session);
        await delay(100);
        const modal = await evaluate(cdp, session, `({ text: document.querySelector('#modal-content')?.innerText || '', selected: document.querySelector('#selection-count')?.textContent })`);
        expect(modal.text).toContain("Cancel Download");
        expect(modal.text).toContain('"Track zero"');
        expect(modal.selected).toBe("0");

        const keep = await evaluate(cdp, session, `(() => { const r = document.querySelector('#modal-content button')?.getBoundingClientRect(); return r && { x:r.x, y:r.y, width:r.width, height:r.height }; })()`);
        if (!keep) throw new Error("Cancel confirmation did not render its Keep Downloading button");
        const keepX = Math.round(keep.x + keep.width / 2);
        const keepY = Math.round(keep.y + keep.height / 2);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: keepX, y: keepY }, session);
        await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: keepX, y: keepY, button: "left", clickCount: 1 }, session);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: keepX, y: keepY, button: "left", clickCount: 1 }, session);
        await delay(80);
        const closed = await evaluate(cdp, session, `document.querySelector('#modal-content')?.innerText || ''`);
        expect(closed).toBe("");

        const artistBounds = await evaluate(cdp, session, `(() => { const r = document.querySelector('#artist-downloads button').getBoundingClientRect(); return {x:r.x+r.width/2,y:r.y+r.height/2}; })()`);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseMoved", x: artistBounds.x, y: artistBounds.y }, session);
        const artistHover = await evaluate(cdp, session, `(() => {
          const button = document.querySelector('#artist-downloads button');
          const percentage = button.parentElement.querySelector('[aria-live]');
          const hit = document.elementFromPoint(${artistBounds.x}, ${artistBounds.y});
          const a = button.getBoundingClientRect(), b = percentage.getBoundingClientRect();
          return {hit:button===hit||button.contains(hit),opacity:getComputedStyle(button).opacity,
            hidden:getComputedStyle(percentage).visibility,aligned:Math.abs(a.x+a.width/2-b.x-b.width/2)<.5};
        })()`);
        expect(artistHover).toEqual({hit:true,opacity:"1",hidden:"hidden",aligned:true});
        if (process.env.BLACKTAPE_UI_SCREENSHOT) {
          const screenshot = await cdp.send("Page.captureScreenshot", { format: "png", clip: { x: 0, y: 0, width: 660, height: 290, scale: 1 } }, session);
          await writeFile(path.resolve(process.env.BLACKTAPE_UI_SCREENSHOT), Buffer.from(screenshot.data, "base64"));
        }
        await cdp.send("Input.dispatchMouseEvent", { type: "mousePressed", x: artistBounds.x, y: artistBounds.y, button: "left", clickCount: 1 }, session);
        await cdp.send("Input.dispatchMouseEvent", { type: "mouseReleased", x: artistBounds.x, y: artistBounds.y, button: "left", clickCount: 1 }, session);
        const artistModal = await evaluate(cdp, session, `document.querySelector('#modal-content').innerText`);
        expect(artistModal).toContain('"Track middle"');
        expect(artistModal).toContain("This stops the entire download task.");
        await evaluate(cdp, session, `document.querySelector('#modal-content button').click()`);
        await delay(80);
      }
    } finally {
      if (cdp) {
        try { await cdp.send("Browser.close"); } catch { /* Edge may already have exited. */ }
      }
      cdp?.close();
      if (child && child.exitCode === null) {
        const exited = new Promise<void>((resolve) => child!.once("exit", () => resolve()));
        await Promise.race([exited, delay(2000)]);
        if (child.exitCode === null) {
          child.kill();
          await Promise.race([exited, delay(2000)]);
        }
      }
      await vite?.close();
      if (profile) {
        try { await rm(profile, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }); }
        catch (error) { console.warn(`Could not clean owned Edge temp profile ${profile}: ${String(error)}`); }
      }
    }
  }, 30_000);
});

async function evaluate(cdp: Cdp, session: string, expression: string): Promise<any> {
  const result = await cdp.send("Runtime.evaluate", { awaitPromise: true, returnByValue: true, expression }, session);
  if (result.exceptionDetails) throw new Error(result.exceptionDetails.exception?.description ?? result.exceptionDetails.text);
  return result.result.value;
}
