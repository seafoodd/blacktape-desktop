import { useEffect, useState } from "react";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import {
  getTelemetryEnabled,
  setTelemetryEnabled,
  setOnboardingCompleted,
} from "@/shared/lib/audio.ts";
import { TELEMETRY_COPY } from "../model/telemetrySettingsHelper.ts";

interface Props {
  rowClassName: string;
  buttonClassName?: string;
  mutedClassName: string;
}

export const TelemetrySettings = ({
  rowClassName,
  buttonClassName,
  mutedClassName,
}: Props) => {
  const [enabled, setEnabled] = useState(true);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    getTelemetryEnabled()
      .then((val) => {
        setEnabled(val);
        setLoading(false);
      })
      .catch((err) => {
        console.error("Failed to load telemetry setting:", err);
        setLoading(false);
      });
  }, []);

  const handleToggle = async (next: boolean) => {
    setEnabled(next);
    try {
      await setTelemetryEnabled(next);
    } catch (err) {
      console.error("Failed to save telemetry setting:", err);
      setEnabled(!next);
    }
  };

  const handleRerunWizard = async () => {
    try {
      await setOnboardingCompleted(false);
      const channel = new BroadcastChannel("blacktape_onboarding_channel");
      channel.postMessage({ type: "OPEN_ONBOARDING" });
      channel.close();

      const mainWindow = await WebviewWindow.getByLabel("main");
      if (mainWindow) {
        await mainWindow.unminimize();
        await mainWindow.show();
        await mainWindow.setFocus();
      }
    } catch (err) {
      console.error("Failed to trigger onboarding wizard:", err);
    }
  };

  return (
    <>
      <div className={rowClassName}>
        <span>
          Crash Reports & Telemetry{" "}
          <span className={mutedClassName}>
            ({enabled ? "Enabled" : "Disabled"})
          </span>
        </span>
        <input
          id="telemetry-setting-toggle"
          type="checkbox"
          checked={enabled}
          disabled={loading}
          onChange={(e) => handleToggle(e.target.checked)}
        />
      </div>
      <p className={mutedClassName} style={{ margin: "4px 0 12px 0", lineHeight: 1.4 }}>
        {TELEMETRY_COPY.description}
      </p>
      <div className={rowClassName}>
        <span>Setup Wizard</span>
        <button
          type="button"
          className={buttonClassName}
          onClick={handleRerunWizard}
        >
          Re-run setup wizard
        </button>
      </div>
    </>
  );
};
