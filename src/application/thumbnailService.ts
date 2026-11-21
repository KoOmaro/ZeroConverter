import { useState, useEffect } from "react";
import { thumbnailGateway } from "../platform/tauri/thumbnailGateway";

const THUMB_SIZE = 200;

/**
 * Mount-settle delay in milliseconds.
 * Components that mount during fast scrolling will unmount before this
 * timer fires, preventing the backend IPC call from ever being sent.
 */
const SETTLE_DELAY_MS = 150;

// ─── Debug Stats ─────────────────────────────────────────────────────

export const thumbnailDebugStats = {
  requestThumbnailCalls: 0,
  completedCacheHits: 0,
  inFlightJoins: 0,
  gatewayCallsStarted: 0,
  failures: 0,
};

export function getThumbnailDebugStats() {
  return { ...thumbnailDebugStats };
}

export function resetThumbnailDebugStats() {
  thumbnailDebugStats.requestThumbnailCalls = 0;
  thumbnailDebugStats.completedCacheHits = 0;
  thumbnailDebugStats.inFlightJoins = 0;
  thumbnailDebugStats.gatewayCallsStarted = 0;
  thumbnailDebugStats.failures = 0;
}

// ─── Caches ──────────────────────────────────────────────────────────

/** Completed thumbnail asset URLs, keyed by composite thumbnail key. */
const thumbnailCache = new Map<string, string>();

/**
 * In-flight backend requests, keyed by composite thumbnail key.
 * Multiple callers for the same key share a single Promise,
 * preventing duplicate IPC calls during fast mount/unmount cycles.
 */
const inFlightThumbnails = new Map<string, Promise<string>>();

// ─── Key ─────────────────────────────────────────────────────────────

/** Build a composite cache key from request parameters. */
function thumbnailKey(path: string, size: number, cacheKeyHint?: string | null): string {
  return `${size}::${cacheKeyHint ?? ""}::${path}`;
}

// ─── Service API ─────────────────────────────────────────────────────

/** Check if a thumbnail is already cached (synchronous). */
export function getCachedThumbnail(path: string): string | undefined {
  return thumbnailCache.get(thumbnailKey(path, THUMB_SIZE));
}

/**
 * Request a thumbnail from the backend cache.
 * Returns cached result if available, deduplicates in-flight requests,
 * and cleans up the in-flight entry on resolve/reject.
 */
export async function requestThumbnail(path: string): Promise<string> {
  thumbnailDebugStats.requestThumbnailCalls++;

  const key = thumbnailKey(path, THUMB_SIZE);

  // 1. Check completed cache
  const cached = thumbnailCache.get(key);
  if (cached) {
    thumbnailDebugStats.completedCacheHits++;
    return cached;
  }

  // 2. Join existing in-flight request if one exists
  const existing = inFlightThumbnails.get(key);
  if (existing) {
    thumbnailDebugStats.inFlightJoins++;
    return existing;
  }

  // 3. Start new backend request and store the shared promise
  thumbnailDebugStats.gatewayCallsStarted++;
  const promise = thumbnailGateway
    .getOrCreateThumbnailAsset(path, THUMB_SIZE)
    .then((result) => {
      thumbnailCache.set(key, result.asset_url);
      return result.asset_url;
    })
    .catch((err) => {
      thumbnailDebugStats.failures++;
      throw err;
    })
    .finally(() => {
      // Always clean up the in-flight entry so future callers
      // can retry on failure or get a fresh completed-cache hit.
      inFlightThumbnails.delete(key);
    });

  inFlightThumbnails.set(key, promise);
  return promise;
}

// ─── React Hook ──────────────────────────────────────────────────────

/**
 * Lazy thumbnail hook with mount-settle delay and stale-state guard.
 *
 * - Returns synchronous cache hit immediately (no delay).
 * - For cache misses, waits SETTLE_DELAY_MS before requesting.
 * - If the component unmounts or the path changes before the delay
 *   completes, the backend call is never initiated.
 * - Once an IPC call has started, it is NOT cancelled (Tauri IPC calls
 *   are fire-and-forget), but the stale-state guard prevents setState
 *   on an unmounted component.
 * - In-flight deduplication ensures that even if the same path is
 *   requested by multiple visible components simultaneously, only one
 *   backend call is made.
 *
 * @param src - Local filesystem path, or undefined to skip.
 */
export function useThumbnail(src: string | undefined): string | undefined {
  const [url, setUrl] = useState<string | undefined>(() => {
    if (!src) return undefined;
    return getCachedThumbnail(src);
  });

  useEffect(() => {
    if (!src) {
      setUrl(undefined);
      return;
    }

    // Synchronous cache hit — no delay needed
    const cached = getCachedThumbnail(src);
    if (cached) {
      setUrl(cached);
      return;
    }

    // Mount-settle delay: wait before sending the IPC request.
    // If the component unmounts during this window (e.g. fast scroll),
    // the timer is cleared and the backend call never fires.
    let stale = false;
    const timer = setTimeout(() => {
      if (stale) return;
      requestThumbnail(src)
        .then((thumbUrl) => {
          if (!stale) setUrl(thumbUrl);
        })
        .catch(() => {
          // Backend failure — leave url undefined so UI shows fallback
        });
    }, SETTLE_DELAY_MS);

    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [src]);

  return url;
}
