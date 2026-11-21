import { transportInvoke } from "./ipc";

/** Renderer never passes an arbitrary local path to the OS opener. */
export async function revealCompletedOutput(jobId: string): Promise<void> {
  await transportInvoke<void>("open_completed_output", { jobId, reveal: true });
}

export async function openCompletedOutput(jobId: string): Promise<void> {
  await transportInvoke<void>("open_completed_output", { jobId, reveal: false });
}
