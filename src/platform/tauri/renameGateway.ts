import { transportInvoke } from "./ipc";

export interface WatchedFolder {
  path: string;
  /** One of the user's own folders: always watched, cannot be removed. */
  builtin: boolean;
}

export interface RenameConvertStatus {
  enabled: boolean;
  autostart: boolean;
  folders: WatchedFolder[];
  /** "Convert with ZeroConverter" in Explorer's right-click menu. */
  context_menu: boolean;
}

export interface QuickJobInfo {
  name: string;
  source: string;
  target: string;
  family: string;
  /** Over 100 MB: converting waits for the user's go-ahead. */
  large_bytes: number | null;
}

export interface QuickOptions {
  quality: number;
  resize_mode: string;
  resize_width: number | null;
  resize_height: number | null;
  bitrate_kbps?: number | null;
}

export const renameGateway = {
  status: () => transportInvoke<RenameConvertStatus>("rename_convert_status"),
  setEnabled: (enabled: boolean) => transportInvoke<void>("rename_convert_set_enabled", { enabled }),
  setAutostart: (enabled: boolean) => transportInvoke<void>("rename_convert_set_autostart", { enabled }),
  addFolder: (path: string) => transportInvoke<void>("rename_convert_add_folder", { path }),
  removeFolder: (path: string) => transportInvoke<void>("rename_convert_remove_folder", { path }),
  setContextMenu: (enabled: boolean) => transportInvoke<void>("context_menu_set", { enabled }),
  quickInfo: (id: string) => transportInvoke<QuickJobInfo | null>("quick_job_info", { id }),
  quickConfirm: (id: string, options: QuickOptions) => transportInvoke<void>("quick_job_confirm", { id, options }),
  quickCancel: (id: string) => transportInvoke<void>("quick_job_cancel", { id }),
};
