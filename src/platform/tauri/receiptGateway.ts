import { listen } from "@tauri-apps/api/event";
import { transportInvoke } from "./ipc";

export type ReceiptTone = "success" | "warning" | "failure";

export interface Receipt {
  tone: ReceiptTone;
  title: string;
  body: string;
}

export interface NotificationPrefs {
  enabled: boolean;
  sound: boolean;
  sound_id: string;
  volume: number;
}

export const receiptGateway = {
  prefs: () => transportInvoke<NotificationPrefs>("notification_prefs"),
  setPrefs: (prefs: NotificationPrefs) => transportInvoke<void>("set_notification_prefs", { prefs }),
  preview: () => transportInvoke<void>("preview_notification"),
  show: (receipt: Receipt) => transportInvoke<void>("show_receipt", { receipt }),
  current: () => transportInvoke<Receipt | null>("receipt_current"),
  painted: () => transportInvoke<void>("receipt_painted"),
  onShow: (handler: (receipt: Receipt) => void) =>
    listen<Receipt>("receipt://show", (event) => handler(event.payload)),
};
