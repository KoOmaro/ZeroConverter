import { getSetting, setSetting } from '../platform/tauri/settingsGateway';
import { ResizeMode } from './converterTypes';
import { DEFAULT_FORMAT_OPTIONS, type FormatOptions } from './formatOptions';

export interface AppSettings {
  format: string;
  quality: number;
  overwrite: boolean;
  outputDirectory: string | null;
  familyFormats: Record<string, string>;
  familyQualities: Record<string, number>;
  resizeMode: ResizeMode;
  resizeWidth: number | null;
  resizeHeight: number | null;
  formatOptions: FormatOptions;
}

const DEFAULT_SETTINGS: AppSettings = {
  format: "",
  quality: 80,
  overwrite: false,
  outputDirectory: null,
  // Format preferences are populated only after loading core capabilities.
  familyFormats: {},
  familyQualities: {},
  resizeMode: "none",
  resizeWidth: null,
  resizeHeight: null,
  formatOptions: DEFAULT_FORMAT_OPTIONS,
};

export class SettingsManager {
  static async load(): Promise<AppSettings> {
    try {
      const saved = await getSetting<AppSettings>('app_settings');
      if (saved) {
        return {
          ...DEFAULT_SETTINGS,
          ...saved,
          formatOptions: { ...DEFAULT_FORMAT_OPTIONS, ...saved.formatOptions },
        };
      }
    } catch (e) {
      console.warn("Failed to load settings:", e);
    }
    return DEFAULT_SETTINGS;
  }

  /** Saves run one after another, so a later save never reads, merges and
   *  writes over an earlier one still in flight. */
  private static queue: Promise<void> = Promise.resolve();

  /** Rejects when the settings could not be written; the caller tells the user. */
  static save(settings: Partial<AppSettings>): Promise<void> {
    const run = this.queue.then(async () => {
      const current = await this.load();
      await setSetting('app_settings', { ...current, ...settings });
    });
    // The next save waits for this one, failed or not.
    this.queue = run.catch(() => undefined);
    return run;
  }
}
