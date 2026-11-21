import { useCallback, useEffect, useState } from "react";
import { renameGateway, type QuickJobInfo, type QuickOptions, type WatchedFolder } from "../platform/tauri/renameGateway";
import { pickFolder } from "../platform/tauri/dialogGateway";
import i18n from "../i18n";

export type { QuickJobInfo, QuickOptions, WatchedFolder };

/** On/off state of rename-to-convert, shared with the tray menu. */
export function useRenameConvert() {
  const [enabled, setEnabledState] = useState<boolean | null>(null);
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [contextMenu, setContextMenuState] = useState<boolean | null>(null);
  const [folders, setFolders] = useState<WatchedFolder[]>([]);
  const [folderError, setFolderError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    renameGateway
      .status()
      .then((s) => {
        setEnabledState(s.enabled);
        setAutostartState(s.autostart);
        setFolders(s.folders ?? []);
        setContextMenuState(s.context_menu ?? null);
      })
      .catch(() => setEnabledState(null));
  }, []);

  const addFolder = useCallback(async () => {
    const path = await pickFolder(i18n.t("dialog.watchFolder"));
    if (!path) return;
    try {
      await renameGateway.addFolder(path);
      setFolderError(null);
    } catch (e) {
      setFolderError(String(e));
    }
    refresh();
  }, [refresh]);

  const removeFolder = useCallback(
    (path: string) => {
      setFolders((list) => list.filter((f) => f.builtin || f.path !== path));
      renameGateway.removeFolder(path).then(refresh, saveFailed);
    },
    [refresh],
  );

  useEffect(() => {
    refresh();
    // The tray can flip it while the window is hidden.
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  // A setting that could not be saved says so, instead of coming back
  // after a restart.
  const saveFailed = useCallback(
    (e: unknown) => {
      setFolderError(String(e));
      refresh();
    },
    [refresh],
  );

  const setEnabled = useCallback((value: boolean) => {
    setEnabledState(value);
    renameGateway.setEnabled(value).then(() => setFolderError(null), saveFailed);
  }, [saveFailed]);

  const setAutostart = useCallback((value: boolean) => {
    setAutostartState(value);
    renameGateway.setAutostart(value).catch(refresh);
  }, [refresh]);

  const setContextMenu = useCallback((value: boolean) => {
    setContextMenuState(value);
    renameGateway.setContextMenu(value).then(() => setFolderError(null), saveFailed);
  }, [saveFailed]);

  return {
    enabled,
    setEnabled,
    autostart,
    setAutostart,
    contextMenu,
    setContextMenu,
    folders,
    addFolder,
    removeFolder,
    folderError,
  };
}

export const quickJob = {
  info: renameGateway.quickInfo,
  confirm: renameGateway.quickConfirm,
  cancel: renameGateway.quickCancel,
};

export { QUALITY_TARGETS, BITRATES } from "./formatOptions";
