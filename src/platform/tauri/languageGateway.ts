import { transportInvoke } from "./ipc";

/** Tells Rust which language the window runs in (tray menu, Explorer menu, notices). */
export async function reportUiLanguage(language: string): Promise<void> {
  await transportInvoke<void>("set_ui_language", { language });
}
