/**
 * How often the running app looks for an update (the startup check always runs). Stored per machine in
 * localStorage; the settings window tells the main window about a change over a BroadcastChannel.
 */

export const CHECK_INTERVAL_KEY = "update_check_interval_hours";
export const CHECK_INTERVAL_CHANNEL = "updater";
export const DEFAULT_CHECK_INTERVAL_HOURS = 1;

/** Hours between checks; 0 = only the check at startup. */
export const CHECK_INTERVAL_OPTIONS: { hours: number; label: string }[] = [
  { hours: 1, label: "Every hour" },
  { hours: 3, label: "Every 3 hours" },
  { hours: 6, label: "Every 6 hours" },
  { hours: 12, label: "Every 12 hours" },
  { hours: 24, label: "Once a day" },
  { hours: 0, label: "Only at startup" },
];

/** The stored value as one of the options; anything else (missing, garbage, removed option) is the default. Pure. */
export function parseCheckInterval(raw: string | null | undefined): number {
  if (raw === null || raw === undefined || raw.trim() === "") return DEFAULT_CHECK_INTERVAL_HOURS;
  const hours = Number(raw);
  return CHECK_INTERVAL_OPTIONS.some((o) => o.hours === hours) ? hours : DEFAULT_CHECK_INTERVAL_HOURS;
}

/** Milliseconds between periodic checks, or null when there are none. Pure. */
export function checkIntervalMs(hours: number): number | null {
  return hours > 0 ? hours * 60 * 60 * 1000 : null;
}

export function readCheckInterval(): number {
  try {
    return parseCheckInterval(localStorage.getItem(CHECK_INTERVAL_KEY));
  } catch {
    return DEFAULT_CHECK_INTERVAL_HOURS;
  }
}

/** Stores the choice and tells the other windows (the main window runs the timer). */
export function writeCheckInterval(hours: number): void {
  try {
    localStorage.setItem(CHECK_INTERVAL_KEY, String(hours));
  } catch {
    // storage unavailable: the choice lasts until the window closes
  }
  try {
    const channel = new BroadcastChannel(CHECK_INTERVAL_CHANNEL);
    channel.postMessage({ type: "CHECK_INTERVAL_CHANGED", hours });
    channel.close();
  } catch {
    // no BroadcastChannel: the main window picks the value up on its next start
  }
}
