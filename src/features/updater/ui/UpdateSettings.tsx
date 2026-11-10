import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { installLabel, useUpdateStore, type UpdateStatus } from "../model/updateStore";

interface Props {
  rowClassName: string;
  buttonClassName: string;
  mutedClassName: string;
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

/** Rows for the settings window: current version, manual check, install. */
export const UpdateSettings = ({ rowClassName, buttonClassName, mutedClassName }: Props) => {
  const { status, version, downloaded, total, error, checkForUpdate, installUpdate } =
    useUpdateStore();
  const [current, setCurrent] = useState<string>("");

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
    </>
  );
};
