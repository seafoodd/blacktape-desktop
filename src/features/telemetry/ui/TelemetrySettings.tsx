import { useEffect, useState } from "react";
import { getTelemetryEnabled, setTelemetryEnabled } from "@/shared/lib/audio.ts";
import { TELEMETRY_COPY } from "../model/telemetrySettingsHelper.ts";

interface Props {
  rowClassName: string;
  mutedClassName: string;
}

export const TelemetrySettings = ({ rowClassName, mutedClassName }: Props) => {
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
    </>
  );
};
