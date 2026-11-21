import { useCallback, useEffect, useState } from "react";
import i18n from "../i18n";
import { receiptGateway, type NotificationPrefs, type Receipt } from "../platform/tauri/receiptGateway";

export type { NotificationPrefs, Receipt };

export const NOTIFICATION_SOUNDS = [
  { id: "silver" },
  { id: "satin" },
  { id: "bloom" },
  { id: "felt" },
];

/** Desktop notification preferences, stored by the Rust side. */
export function useNotificationPrefs() {
  const [prefs, setPrefs] = useState<NotificationPrefs | null>(null);

  useEffect(() => {
    receiptGateway.prefs().then(setPrefs).catch(() => setPrefs(null));
  }, []);

  const update = useCallback(
    (change: Partial<NotificationPrefs>) => {
      if (!prefs) return;
      const next = { ...prefs, ...change };
      setPrefs(next);
      receiptGateway.setPrefs(next).catch(() => {});
    },
    [prefs],
  );

  const preview = useCallback(() => {
    receiptGateway.preview().catch(() => {});
  }, []);

  return { prefs, update, preview };
}

/** Asks for a card in the corner of the screen (shown only when enabled). */
export function notifyDesktop(receipt: Receipt) {
  receiptGateway.show(receipt).catch(() => {});
}

export const receiptPage = {
  current: receiptGateway.current,
  painted: receiptGateway.painted,
  onShow: receiptGateway.onShow,
};

interface SummaryLike {
  status: string;
  completed: number;
  failed: number;
}

/**
 * When a batch finishes while ZeroConverter is in the background, show a
 * desktop card: the artist does not have to keep checking the window.
 */
export function useBatchCompletionNotice(summary: SummaryLike | null | undefined) {
  const [wasActive, setWasActive] = useState(false);
  useEffect(() => {
    if (!summary) return;
    const active = summary.status === "Active" || summary.status === "Draining";
    if (active) {
      setWasActive(true);
      return;
    }
    if (!wasActive) return;
    setWasActive(false);
    if (document.hasFocus()) return;
    const { completed, failed } = summary;
    notifyDesktop(
      failed === 0
        ? { tone: "success", title: i18n.t("notice.allDone", { count: completed }), body: i18n.t("notice.allDoneBody") }
        : {
            tone: completed === 0 ? "failure" : "warning",
            title:
              completed === 0
                ? i18n.t("notice.allFailed", { count: failed })
                : i18n.t("notice.someFailed", { done: completed, failed }),
            body: i18n.t("notice.failedBody"),
          },
    );
  }, [summary, wasActive]);
}
