import fs from "node:fs";
import path from "node:path";
import os from "node:os";
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

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

// Run if directly executed
const currentFile = fileURLToPath(import.meta.url);
const invokedFile = process.argv[1] ? path.resolve(process.argv[1]) : "";

if (invokedFile && currentFile === invokedFile) {
  cleanFreshDirs();

  const isWin = process.platform === "win32";
  const tauriCmd = isWin ? "npx.cmd" : "npx";

  console.log(
    "\n🚀 [dev:fresh] Launching Blacktape in an isolated, temporary profile...\n" +
      "   • Database: completely clean\n" +
      "   • Library: completely clean (no synced music)\n" +
      "   • Settings & LocalStorage: completely clean\n" +
      "   • Existing database/library: completely untouched\n",
  );

  const child = spawn(
    tauriCmd,
    ["tauri", "dev", "--config", "src-tauri/tauri.fresh.conf.json"],
    {
      stdio: "inherit",
      shell: isWin,
    },
  );

  const cleanup = () => {
    console.log("\n[dev:fresh] Cleaning up temporary profile...");
    cleanFreshDirs();
    process.exit(0);
  };

  process.on("SIGINT", cleanup);
  process.on("SIGTERM", cleanup);

  child.on("exit", (code) => {
    cleanFreshDirs();
    process.exit(code ?? 0);
  });
}
