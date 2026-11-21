import { ask, open } from "@tauri-apps/plugin-dialog";

export interface FileFilter {
  name: string;
  extensions: string[];
}

export async function pickInputFiles(filters: FileFilter[]): Promise<string[]> {
  const selected = await open({
    multiple: true,
    filters,
  });

  if (Array.isArray(selected)) return selected;
  if (typeof selected === "string") return [selected];
  return [];
}

/** A native Yes/No question. */
export async function confirmAction(message: string, title: string, okLabel: string, cancelLabel: string): Promise<boolean> {
  return ask(message, { title, kind: "warning", okLabel, cancelLabel });
}

export async function pickOutputDirectory(title: string): Promise<string | null> {
  return pickFolder(title);
}

export async function pickFolder(title: string): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
    title,
  });

  return typeof selected === "string" ? selected : null;
}
