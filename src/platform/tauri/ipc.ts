import { invoke, Channel } from "@tauri-apps/api/core";

/**
 * Common IPC transport layer.
 * This is the ONLY place where @tauri-apps/api/core should be imported.
 */
export async function transportInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    console.error(`IPC Error [${command}]:`, error);
    throw error;
  }
}

/**
 * Creates a new communication channel for live streaming.
 */
export function createChannel<T>(): Channel<T> {
  return new Channel<T>();
}

export type { Channel };
