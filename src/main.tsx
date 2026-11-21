import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource-variable/inter";
import "@fontsource-variable/jetbrains-mono";
import App from "./App";
import { QuickConvert } from "./components/quick/QuickConvert";
import { ReceiptCard } from "./components/receipt/ReceiptCard";
import "./i18n";
import { installDesktopChrome } from "./application/browserChrome";
// Applies the saved theme in every window (main, `+`, notification card).
import "./application/themeManager";
import "./index.css";

async function boot() {
  installDesktopChrome();
  if (import.meta.env.DEV && !("__TAURI_INTERNALS__" in window)) {
    (await import("./dev/browserPreviewMock")).installBrowserPreviewMock();
  }
  // `+` renames open a small window on #/quick/<id>.
  const quickId = window.location.hash.match(/^#\/quick\/([\w-]+)/)?.[1];
  // The desktop notification window loads #/receipt.
  const page = window.location.hash === "#/receipt" ? <ReceiptCard /> : quickId ? <QuickConvert id={quickId} /> : <App />;
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<React.StrictMode>{page}</React.StrictMode>);
}

boot();
