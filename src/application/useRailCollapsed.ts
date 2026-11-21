import { useCallback, useState } from "react";

const KEY = "zeroconverter-rail-collapsed";

/** Whether the left panel is folded to its icon strip; remembered per machine. */
export function useRailCollapsed(): [boolean, () => void] {
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(KEY) === "1";
    } catch {
      return false;
    }
  });
  const toggle = useCallback(() => {
    setCollapsed((c) => {
      try {
        localStorage.setItem(KEY, c ? "0" : "1");
      } catch {
        // Not remembered; still toggles.
      }
      return !c;
    });
  }, []);
  return [collapsed, toggle];
}
