import { useCallback, useEffect, useState } from "react";
import { updateGateway } from "../platform/tauri/updateGateway";

export type UpdateCheck = "idle" | "checking" | "latest" | "failed" | "installing";

/** Update setting, a newer version waiting for the user's yes, and "check now". */
export function useAppUpdate() {
  const [auto, setAutoState] = useState<boolean | null>(null);
  const [available, setAvailable] = useState<string | null>(null);
  const [check, setCheck] = useState<UpdateCheck>("idle");

  useEffect(() => {
    updateGateway
      .status()
      .then((s) => {
        setAutoState(s.auto_update);
        setAvailable(s.available);
      })
      .catch(() => setAutoState(null));
  }, []);

  const setAuto = useCallback((enabled: boolean) => {
    setAutoState(enabled);
    // Not saved: show the setting as it really is.
    updateGateway.setAuto(enabled).catch(() =>
      updateGateway
        .status()
        .then((s) => setAutoState(s.auto_update))
        .catch(() => setAutoState(null)),
    );
  }, []);

  const checkNow = useCallback(async () => {
    setCheck("checking");
    try {
      const version = await updateGateway.check();
      setAvailable(version);
      setCheck(version ? "idle" : "latest");
    } catch {
      setCheck("failed");
    }
  }, []);

  const install = useCallback(async () => {
    setCheck("installing");
    try {
      await updateGateway.install();
    } catch {
      setCheck("failed");
    }
  }, []);

  return { auto, available, check, setAuto, checkNow, install };
}
