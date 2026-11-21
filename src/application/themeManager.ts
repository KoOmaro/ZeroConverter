// ==============================================================================
// OWNERSHIP:
//   Theme and display preferences: which palette is active, interface scale
//   and reduced motion. Persists in localStorage; applies to <html>.
// ==============================================================================
import { windowChrome } from "../platform/tauri/windowGateway";

export type ThemeId = "graphite-steel" | "graphite-light" | "twilight" | "desert-dusk" | "matcha-atelier";
export type ThemeMode = "system" | ThemeId;

export interface ThemeInfo {
  id: ThemeId;
  name: string;
  appearance: "dark" | "light";
  /** Value of data-theme on <html>. */
  attr: string;
  /** Preview swatches: shell, surface, accent. */
  swatch: [string, string, string];
}

export const THEMES: ThemeInfo[] = [
  { id: "graphite-steel", name: "Graphite Steel", appearance: "dark", attr: "dark", swatch: ["#0E1012", "#181B1E", "#2D5C82"] },
  { id: "twilight", name: "Twilight", appearance: "dark", attr: "twilight", swatch: ["#17131C", "#2D2735", "#B6A0DA"] },
  { id: "desert-dusk", name: "Desert Dusk", appearance: "dark", attr: "desert-dusk", swatch: ["#15110C", "#241D15", "#8A4926"] },
  { id: "matcha-atelier", name: "Matcha Atelier", appearance: "dark", attr: "matcha-atelier", swatch: ["#151B15", "#293229", "#AFC49D"] },
  { id: "graphite-light", name: "Graphite Light", appearance: "light", attr: "light", swatch: ["#DDE2E8", "#FAFBFC", "#2D6F9F"] },
];

export type UiScale = 100 | 110 | 125;
export type MotionPreference = "system" | "reduce";

export interface DisplayPreferences {
  theme: ThemeMode;
  scale: UiScale;
  motion: MotionPreference;
}

const STORAGE_KEY = "zeroconverter-display";
const LEGACY_THEME_KEY = "zeroconverter-theme-mode";
const DEFAULTS: DisplayPreferences = { theme: "graphite-steel", scale: 100, motion: "system" };

function themeById(id: ThemeId): ThemeInfo {
  return THEMES.find((t) => t.id === id) ?? THEMES[0];
}

function read(): DisplayPreferences {
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<DisplayPreferences> | null;
    if (saved) {
      const theme = saved.theme === "system" || THEMES.some((t) => t.id === saved.theme) ? saved.theme! : DEFAULTS.theme;
      return { ...DEFAULTS, ...saved, theme };
    }
    // Earlier builds stored only "dark" | "light" | "system".
    const legacy = localStorage.getItem(LEGACY_THEME_KEY);
    if (legacy === "light") return { ...DEFAULTS, theme: "graphite-light" };
    if (legacy === "system") return { ...DEFAULTS, theme: "system" };
  } catch {
    // Storage unavailable: fall through to defaults.
  }
  return DEFAULTS;
}

class ThemeManager {
  private prefs: DisplayPreferences = read();
  private listeners = new Set<(prefs: DisplayPreferences) => void>();
  private mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  private motionQuery = window.matchMedia("(prefers-reduced-motion: reduce)");

  constructor() {
    this.apply(false);
    this.mediaQuery.addEventListener("change", () => {
      if (this.prefs.theme === "system") this.apply(true);
    });
    // The notification card and `+` windows follow changes made in the main window.
    window.addEventListener("storage", (e) => {
      if (e.key !== STORAGE_KEY) return;
      this.prefs = read();
      this.apply(false);
      this.listeners.forEach((l) => l(this.prefs));
    });
  }

  get(): DisplayPreferences {
    return this.prefs;
  }

  /** The palette in use once "system" is resolved. */
  resolved(): ThemeInfo {
    if (this.prefs.theme === "system") {
      return themeById(this.mediaQuery.matches ? "graphite-steel" : "graphite-light");
    }
    return themeById(this.prefs.theme);
  }

  reducedMotion(): boolean {
    return this.prefs.motion === "reduce" || this.motionQuery.matches;
  }

  update(next: Partial<DisplayPreferences>) {
    this.prefs = { ...this.prefs, ...next };
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(this.prefs));
    } catch {
      // Not persisted; still applied for this session.
    }
    this.apply(next.theme !== undefined);
    this.listeners.forEach((l) => l(this.prefs));
  }

  subscribe(listener: (prefs: DisplayPreferences) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  private apply(animate: boolean) {
    const root = document.documentElement;
    const theme = this.resolved();
    const paint = () => {
      root.setAttribute("data-theme", theme.attr);
      root.style.colorScheme = theme.appearance;
    };
    root.toggleAttribute("data-reduce-motion", this.prefs.motion === "reduce");
    // Only the main window scales; the small windows have fixed sizes.
    if (!window.location.hash) {
      try {
        windowChrome.setZoom(this.prefs.scale / 100).catch(() => {});
      } catch {
        // No native webview (browser preview).
      }
    }
    const doc = document as Document & { startViewTransition?: (cb: () => void) => unknown };
    if (animate && !this.reducedMotion() && doc.startViewTransition) {
      doc.startViewTransition(paint);
    } else {
      paint();
    }
  }
}

export const themeManager = new ThemeManager();
