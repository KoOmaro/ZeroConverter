// ==============================================================================
// OWNERSHIP:
//   Makes the webview behave like a desktop app, not a browser tab: no page
//   context menu (Back, Reload, Print, Save as…) and no browser shortcuts.
//   Text fields keep their cut/copy/paste menu. Dev builds keep everything.
// ==============================================================================

function isEditable(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const field = target.closest("input, textarea, [contenteditable='true']");
  if (!field) return false;
  if (field instanceof HTMLInputElement) {
    return !["checkbox", "radio", "range", "button", "submit", "file", "color"].includes(field.type);
  }
  return true;
}

/** Browser shortcuts with no meaning in a desktop app. */
function isBrowserShortcut(e: KeyboardEvent): boolean {
  const key = e.key.toLowerCase();
  if (key === "f5" || key === "f7" || key === "f12" || key === "browserback" || key === "browserforward") return true;
  if (e.altKey && (key === "arrowleft" || key === "arrowright")) return true;
  if (!(e.ctrlKey || e.metaKey)) return false;
  if (e.shiftKey && (key === "i" || key === "j" || key === "c")) return true;
  return ["r", "p", "s", "f", "g", "u", "j", "h", "n", "t", "=", "+", "-", "0"].includes(key);
}

export function installDesktopChrome(): void {
  if (import.meta.env.DEV) return;
  window.addEventListener("contextmenu", (e) => {
    if (!isEditable(e.target)) e.preventDefault();
  });
  window.addEventListener(
    "keydown",
    (e) => {
      if (isBrowserShortcut(e)) e.preventDefault();
    },
    { capture: true },
  );
  // Ctrl + wheel zooms the page in a browser.
  window.addEventListener(
    "wheel",
    (e) => {
      if (e.ctrlKey) e.preventDefault();
    },
    { passive: false },
  );
}
