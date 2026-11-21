import { useCallback, useEffect, useState } from "react";
import { windowChrome } from "../platform/tauri/windowGateway";

/**
 * State and actions for the custom title bar. Outside Tauri (plain browser
 * preview) every call is a no-op so the UI still renders.
 */
export function useWindowChrome() {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let disposed = false;
    const sync = () =>
      windowChrome.isMaximized().then((v) => !disposed && setIsMaximized(v)).catch(() => {});
    try {
      sync();
      windowChrome
        .onResized(sync)
        .then((fn) => (disposed ? fn() : (unlisten = fn)))
        .catch(() => {});
    } catch {
      /* not running inside Tauri */
    }
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, []);

  const run = useCallback((action: () => Promise<unknown>) => {
    try {
      action().catch(() => {});
    } catch {
      /* not running inside Tauri */
    }
  }, []);

  return {
    isMaximized,
    minimize: () => run(windowChrome.minimize),
    toggleMaximize: () => run(windowChrome.toggleMaximize),
    close: () => run(windowChrome.close),
  };
}
