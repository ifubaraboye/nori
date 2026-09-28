import { useCallback, useEffect, useState } from "react";
import { getNoriBridge } from "../bridge/noriBridge";
import { defaultSettings, type SettingsState } from "./settings";

const STORAGE_KEY = "nori.settings";

/** App-owned settings values. Host (Electron) when present, localStorage otherwise. */
export function useSettings(): [SettingsState, (patch: Partial<SettingsState>) => void] {
  const [state, setState] = useState<SettingsState>(() => {
    try {
      const raw = localStorage.getItem(STORAGE_KEY);
      if (raw) return { ...defaultSettings(), ...(JSON.parse(raw) as Partial<SettingsState>) };
    } catch { /* corrupt file reads as a first run */ }
    return defaultSettings();
  });

  useEffect(() => {
    const bridge = getNoriBridge();
    if (!bridge?.settingsGet) return;
    bridge
      .settingsGet()
      .then((host) => {
        if (host) setState((prev) => ({ ...prev, ...host }));
      })
      .catch(() => undefined);
  }, []);

  const update = useCallback((patch: Partial<SettingsState>) => {
    setState((prev) => {
      const next = { ...prev, ...patch };
      try {
        localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
      } catch { /* read-only storage must not stop a switch flip */ }
      try {
        void getNoriBridge()?.settingsSet?.(next);
      } catch { /* standalone */ }
      return next;
    });
  }, []);

  return [state, update];
}
