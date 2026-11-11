import { useEffect } from "react";
import { useToolsStore } from "../model/toolsStore.ts";

/** The tools' setup state, kept current while the calling component is mounted. */
export function useToolsStatus() {
  const init = useToolsStore((s) => s.init);
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    init()
      .then((u) => (cancelled ? u() : (unlisten = u)))
      .catch((err) => console.error("Failed to load the downloader status:", err));
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [init]);
  return useToolsStore((s) => s.status);
}
