import { useState, useEffect, useCallback } from "react";
import { converterClient } from "./converterClient";
import { AppCapabilitiesDto } from "./converterTypes";

let cachedCapabilities: AppCapabilitiesDto | null = null;
let isFetching = false;
let fetchPromise: Promise<AppCapabilitiesDto> | null = null;

export function useAppCapabilities() {
  const [capabilities, setCapabilities] = useState<AppCapabilitiesDto | null>(cachedCapabilities);
  const [loading, setLoading] = useState<boolean>(!cachedCapabilities);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (cachedCapabilities) return;
    if (isFetching && fetchPromise) {
      fetchPromise.then(setCapabilities).catch((e) => setError(e.toString()));
      return;
    }

    isFetching = true;
    setLoading(true);
    fetchPromise = converterClient.getAppCapabilities();
    
    fetchPromise
      .then((caps) => {
        cachedCapabilities = caps;
        setCapabilities(caps);
        setError(null);
      })
      .catch((err) => {
        console.error("Failed to load capabilities", err);
        setError(err.toString());
      })
      .finally(() => {
        setLoading(false);
        isFetching = false;
      });
  }, []);

  const getReadableExtensions = useCallback((): string[] => {
    if (!capabilities) return [];
    const exts = new Set<string>();
    for (const family of capabilities.families) {
      for (const desc of family.descriptors) {
        if (desc.can_read) {
          exts.add(desc.canonical_extension.toLowerCase());
          desc.aliases.forEach((a) => exts.add(a.toLowerCase()));
        }
      }
    }
    return Array.from(exts);
  }, [capabilities]);

  return {
    capabilities,
    loading,
    error,
    getReadableExtensions,
  };
}
