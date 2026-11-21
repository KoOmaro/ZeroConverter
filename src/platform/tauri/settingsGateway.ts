import { LazyStore } from "@tauri-apps/plugin-store";

const store = new LazyStore("settings.json");

export async function getSetting<T>(key: string): Promise<T | null | undefined> {
  return await store.get<T>(key);
}

export async function setSetting<T>(key: string, value: T): Promise<void> {
  await store.set(key, value);
  await store.save();
}
