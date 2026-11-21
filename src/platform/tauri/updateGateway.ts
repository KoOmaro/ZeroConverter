import { transportInvoke } from "./ipc";

export interface UpdateStatus {
  auto_update: boolean;
  /** A newer version that was found; nothing is downloaded yet. */
  available: string | null;
}

export const updateGateway = {
  status: () => transportInvoke<UpdateStatus>("update_status"),
  setAuto: (enabled: boolean) => transportInvoke<void>("update_set_auto", { enabled }),
  /** The newer version, or null when up to date. Downloads nothing. */
  check: () => transportInvoke<string | null>("update_check"),
  /** The user's yes: download, verify, install and restart. */
  install: () => transportInvoke<void>("update_install"),
};
