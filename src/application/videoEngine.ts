import { useCallback, useEffect, useState } from "react";
import { converterClient } from "./converterClient";
import type { FfmpegInstallProgress, FfmpegStatus } from "./converterTypes";
import { navigationGateway } from "../platform/tauri/navigationGateway";

/** FFmpeg for Settings → Video: is it there, and download it on request. */
export function useVideoEngine() {
  const [status, setStatus] = useState<FfmpegStatus | null>(null);
  const [progress, setProgress] = useState<FfmpegInstallProgress | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    converterClient
      .ffmpegStatus()
      .then(setStatus)
      .catch(() => setStatus(null));
  }, []);

  useEffect(refresh, [refresh]);

  const install = useCallback(async () => {
    setError(null);
    setProgress({ phase: "download", done: 0, total: status?.downloadBytes ?? 0 });
    try {
      setStatus(await converterClient.installVideoEngine(setProgress));
    } catch (e) {
      if (!String(e).includes("CANCELLED")) setError(String(e).replace(/^Error:\s*/, ""));
      refresh();
    } finally {
      setProgress(null);
    }
  }, [status, refresh]);

  const cancel = useCallback(() => {
    converterClient.cancelVideoEngine().catch(() => {});
  }, []);

  return { status, progress, error, install, cancel };
}

/**
 * Opens Settings at the page the backend asks for (a rename that needs
 * FFmpeg opens Video). Outside Tauri nothing ever asks.
 */
export function useSettingsRequests(open: (section: string) => void) {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    navigationGateway
      .takeSettingsRequest()
      .then((section) => section && !disposed && open(section))
      .catch(() => {});
    try {
      navigationGateway
        .onOpenSettings((section) => {
          navigationGateway.takeSettingsRequest().catch(() => {});
          open(section);
        })
        .then((fn) => (disposed ? fn() : (unlisten = fn)))
        .catch(() => {});
    } catch {
      /* not running inside Tauri */
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [open]);
}
