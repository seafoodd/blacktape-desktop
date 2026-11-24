import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

export const PID_FILE_NAME = ".dev-fresh.pid";

export function getFreshDirs(
  platform = process.platform,
  env = process.env,
  home = os.homedir(),
) {
  const p = platform === "win32" ? path.win32 : path.posix;
  const dirs = [];
  if (platform === "win32") {
    if (env.APPDATA) {
      dirs.push(p.join(env.APPDATA, "dev.seafood.blacktape.fresh"));
    }
    if (env.LOCALAPPDATA) {
      dirs.push(p.join(env.LOCALAPPDATA, "dev.seafood.blacktape.fresh"));
    }
  } else if (platform === "darwin") {
    dirs.push(
      p.join(
        home,
        "Library",
        "Application Support",
        "dev.seafood.blacktape.fresh",
      ),
    );
  } else {
    const dataHome =
      env.XDG_DATA_HOME || p.join(home, ".local", "share");
    const configHome = env.XDG_CONFIG_HOME || p.join(home, ".config");
    dirs.push(p.join(dataHome, "dev.seafood.blacktape.fresh"));
    dirs.push(p.join(configHome, "dev.seafood.blacktape.fresh"));
  }
  return dirs;
}

export function cleanFreshDirs(
  platform = process.platform,
  env = process.env,
  home = os.homedir(),
) {
  for (const dir of getFreshDirs(platform, env, home)) {
    try {
      if (fs.existsSync(dir)) {
        fs.rmSync(dir, { recursive: true, force: true });
        console.log(`[dev:fresh] Cleaned fresh profile: ${dir}`);
      }
    } catch (err) {
      console.warn(`[dev:fresh] Notice: Could not remove ${dir}: ${err.message}`);
    }
  }
}

export function seedFreshToolsAndCookies(
  platform = process.platform,
  env = process.env,
  home = os.homedir(),
) {
  const baseDirs = [];
  if (platform === "win32") {
    if (env.APPDATA) {
      baseDirs.push({
        src: path.join(env.APPDATA, "dev.seafood.blacktape"),
        dst: path.join(env.APPDATA, "dev.seafood.blacktape.fresh"),
      });
    }
    if (env.LOCALAPPDATA) {
      baseDirs.push({
        src: path.join(env.LOCALAPPDATA, "dev.seafood.blacktape"),
        dst: path.join(env.LOCALAPPDATA, "dev.seafood.blacktape.fresh"),
      });
    }
  } else if (platform === "darwin") {
    const appSupport = path.join(home, "Library", "Application Support");
    baseDirs.push({
      src: path.join(appSupport, "dev.seafood.blacktape"),
      dst: path.join(appSupport, "dev.seafood.blacktape.fresh"),
    });
  } else {
    const dataHome = env.XDG_DATA_HOME || path.join(home, ".local", "share");
    const configHome = env.XDG_CONFIG_HOME || path.join(home, ".config");
    baseDirs.push({
      src: path.join(dataHome, "dev.seafood.blacktape"),
      dst: path.join(dataHome, "dev.seafood.blacktape.fresh"),
    });
    baseDirs.push({
      src: path.join(configHome, "dev.seafood.blacktape"),
      dst: path.join(configHome, "dev.seafood.blacktape.fresh"),
    });
  }

  for (const { src, dst } of baseDirs) {
    if (!fs.existsSync(src)) continue;
    try {
      fs.mkdirSync(dst, { recursive: true });
      const cookieFile = path.join(src, "youtube_cookies.txt");
      if (fs.existsSync(cookieFile)) {
        fs.copyFileSync(cookieFile, path.join(dst, "youtube_cookies.txt"));
      }
      for (const file of [
        "yt-dlp.exe",
        "yt-dlp",
        "yt-dlp.version",
        "yt-dlp.checked",
        "deno.exe",
        "deno",
      ]) {
        const s = path.join(src, file);
        if (fs.existsSync(s)) {
          fs.copyFileSync(s, path.join(dst, file));
        }
      }
      const ffmpegSrc = path.join(src, "ffmpeg");
      const ffmpegDst = path.join(dst, "ffmpeg");
      if (fs.existsSync(ffmpegSrc)) {
        fs.cpSync(ffmpegSrc, ffmpegDst, { recursive: true });
      }
    } catch (_) {}
  }
}

export function isWorkspaceDevProcess(execPath, workspaceRoot = process.cwd()) {
  if (!execPath || typeof execPath !== "string") return false;
  const isWindowsPath =
    /^[a-zA-Z]:[/\\]/.test(execPath) ||
    /^[a-zA-Z]:[/\\]/.test(workspaceRoot) ||
    execPath.includes("\\") ||
    workspaceRoot.includes("\\");

  const pathModule = isWindowsPath ? path.win32 : path.posix;
  const resolvedTarget = pathModule.resolve(
    workspaceRoot,
    "src-tauri",
    "target",
  );
  const resolvedExec = pathModule.resolve(workspaceRoot, execPath);

  const normTarget = resolvedTarget.replace(/\\/g, "/").toLowerCase();
  const normExec = resolvedExec.replace(/\\/g, "/").toLowerCase();

  return (
    normExec === normTarget ||
    normExec.startsWith(normTarget.endsWith("/") ? normTarget : `${normTarget}/`)
  );
}

export function getPidFilePath(workspaceRoot = process.cwd()) {
  return path.resolve(workspaceRoot, PID_FILE_NAME);
}

export function writePidFile(pidInfo, workspaceRoot = process.cwd()) {
  const filePath = getPidFilePath(workspaceRoot);
  fs.writeFileSync(filePath, JSON.stringify(pidInfo, null, 2), "utf8");
}

export function readPidFile(workspaceRoot = process.cwd()) {
  const filePath = getPidFilePath(workspaceRoot);
  if (!fs.existsSync(filePath)) return null;
  try {
    const raw = fs.readFileSync(filePath, "utf8");
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export function removePidFile(workspaceRoot = process.cwd()) {
  const filePath = getPidFilePath(workspaceRoot);
  if (fs.existsSync(filePath)) {
    try {
      fs.unlinkSync(filePath);
    } catch (_) {}
  }
}

export function stopDevFresh(workspaceRoot = process.cwd()) {
  const pidInfo = readPidFile(workspaceRoot);
  if (pidInfo?.runnerPid) {
    try {
      process.kill(pidInfo.runnerPid, "SIGTERM");
      console.log(`[dev:fresh] Sent SIGTERM to runner process ${pidInfo.runnerPid}`);
    } catch (_) {}
  }
  if (pidInfo?.childPid) {
    try {
      process.kill(pidInfo.childPid, "SIGTERM");
      console.log(`[dev:fresh] Sent SIGTERM to child process ${pidInfo.childPid}`);
    } catch (_) {}
  }

  // On Windows, terminate child processes specifically within src-tauri/target/
  if (process.platform === "win32") {
    if (pidInfo?.childPid) {
      try {
        spawnSync("taskkill", ["/PID", String(pidInfo.childPid), "/T", "/F"], {
          stdio: "ignore",
        });
      } catch (_) {}
    }
    if (pidInfo?.runnerPid) {
      try {
        spawnSync("taskkill", ["/PID", String(pidInfo.runnerPid), "/T", "/F"], {
          stdio: "ignore",
        });
      } catch (_) {}
    }
    try {
      const psCmd = `Get-CimInstance Win32_Process -Filter "Name like '%blacktape%'" | Where-Object { $_.ExecutablePath -like "*src-tauri*target*" } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }`;
      spawnSync("powershell", ["-NoProfile", "-Command", psCmd], {
        stdio: "ignore",
      });
    } catch (_) {}
  }

  removePidFile(workspaceRoot);
  cleanFreshDirs();
  console.log("[dev:fresh] Successfully stopped dev:fresh and cleaned profile.");
}

// Run if directly executed
const currentFile = fileURLToPath(import.meta.url);
const invokedFile = process.argv[1] ? path.resolve(process.argv[1]) : "";

if (invokedFile && currentFile === invokedFile) {
  const args = process.argv.slice(2);
  if (args.includes("--stop")) {
    stopDevFresh();
    process.exit(0);
  }

  cleanFreshDirs();
  seedFreshToolsAndCookies();

  const isWin = process.platform === "win32";
  const tauriCmd = isWin ? "npx.cmd" : "npx";

  console.log(
    "\n🚀 [dev:fresh] Launching Blacktape in an isolated, temporary profile...\n" +
      "   • Database: completely clean (dev.seafood.blacktape.fresh)\n" +
      "   • Library: completely clean (no synced music)\n" +
      "   • Settings & LocalStorage: completely clean\n" +
      "   • Installed/release build: completely untouched & safe\n",
  );

  const child = spawn(
    tauriCmd,
    ["tauri", "dev", "--config", "src-tauri/tauri.fresh.conf.json"],
    {
      stdio: "inherit",
      shell: isWin,
      env: {
        ...process.env,
        BLACKTAPE_DEV_FRESH: "1",
      },
    },
  );

  writePidFile({
    runnerPid: process.pid,
    childPid: child.pid,
    startedAt: new Date().toISOString(),
  });

  const cleanup = () => {
    console.log("\n[dev:fresh] Cleaning up temporary profile...");
    removePidFile();
    cleanFreshDirs();
    process.exit(0);
  };

  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);

  child.on("exit", (code) => {
    removePidFile();
    cleanFreshDirs();
    process.exit(code ?? 0);
  });
}
