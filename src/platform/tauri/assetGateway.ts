import { convertFileSrc } from "@tauri-apps/api/core";

export function toAssetUrl(path: string): string {
  return convertFileSrc(path);
}
