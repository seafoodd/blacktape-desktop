import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { installLabel, useUpdateStore, type UpdateStatus } from "../model/updateStore";
import { CHECK_INTERVAL_OPTIONS, readCheckInterval, writeCheckInterval } from "../model/checkInterval";

interface Props {
  rowClassName: string;
  buttonClassName: string;
  mutedClassName: string;
  selectClassName: string;
}

function statusText(status: UpdateStatus, version: string | null, error: string | null): string {
  switch (status) {
    case "checking":
      return "Checking…";
    case "upToDate":
      return "You have the latest version.";
    case "available":
    case "downloading":
    case "installing":
      return `Version ${version} is available.`;
    case "error":
      return `Could not check for updates: ${error ?? "unknown error"}`;
    default:
      return "";
  }
}

/** Rows for the settings window: current version, manual check, install, how often to check. */
export const UpdateSettings = ({ rowClassName, buttonClassName, mutedClassName, selectClassName }: Props) => {
  const { status, version, downloaded, total, error, checkForUpdate, installUpdate } =
    useUpdateStore();
  const [current, setCurrent] = useState<string>("");
  const [intervalHours, setIntervalHours] = useState(readCheckInterval);

  useEffect(() => {
    getVersion().then(setCurrent).catch(() => setCurrent("unknown"));
  }, []);

  const busy = status === "checking" || status === "downloading" || status === "installing";
  const canInstall = status === "available" || status === "downloading" || status === "installing";

  return (
    <>
      <div className={rowClassName}>
        <span>Version</span>
        <span className={mutedClassName}>{current}</span>
      </div>
      <div className={rowClassName}>
        <span>
          Updates <span className={mutedClassName}>{statusText(status, version, error)}</span>
        </span>
        {canInstall ? (
          <button className={buttonClassName} onClick={installUpdate} disabled={busy}>
            {installLabel(status, version, downloaded, total)}
          </button>
        ) : (
          <button
            className={buttonClassName}
            onClick={() => checkForUpdate()}
            disabled={busy}
          >
            Check for updates
          </button>
        )}
      </div>
      <div className={rowClassName}>
        <label htmlFor="update-interval-select">Check automatically</label>
        <select
          id="update-interval-select"
          className={selectClassName}
          value={intervalHours}
          onChange={(e) => {
            const hours = Number(e.target.value);
            setIntervalHours(hours);
            writeCheckInterval(hours);
          }}
        >
          {CHECK_INTERVAL_OPTIONS.map((o) => (
            <option key={o.hours} value={o.hours}>
              {o.label}
            </option>
          ))}
        </select>
      </div>
    </>
  );
};
