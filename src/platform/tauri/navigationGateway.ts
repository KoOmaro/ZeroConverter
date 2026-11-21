import { listen } from "@tauri-apps/api/event";
import { transportInvoke } from "./ipc";

/** Requests from the backend to show a part of the window. */
export const navigationGateway = {
  /** A settings page the backend asked for before the window was listening. */
  takeSettingsRequest: () => transportInvoke<string | null>("take_settings_request"),
  onOpenSettings: (handler: (section: string) => void) =>
    listen<string>("settings://open", (event) => handler(event.payload)),
};
