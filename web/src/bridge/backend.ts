// Renderer-side backend adapter: uses window.nori when running inside
// Electron, otherwise stays on the in-memory mock store.
//
// Phase 1 (this port): local reducer remains the source of truth so the UI
// works standalone AND inside Electron. Host calls are fire-and-forget
// mirrors (open/star/send/sync) plus a liveness subscription that re-syncs
// the host on push events. A full host-backed store adapter can replace the
// reducer later without changing components.
import { useEffect } from "react";
import { getNoriBridge } from "./noriBridge";

export function isElectron(): boolean {
  return getNoriBridge() !== null;
}

export function useNoriBackend(): void {
  useEffect(() => {
    const bridge = getNoriBridge();
    if (!bridge) return;
    let cancelled = false;
    (async () => {
      try {
        await bridge.sync?.();
      } catch {
        // Offline first run: mock store stays visible.
      }
    })();
    const unsubscribe = bridge.subscribe(() => {
      if (cancelled) return;
      bridge
        .sync?.()
        .catch(() => undefined);
    });
    return () => {
      cancelled = true;
      unsubscribe();
    };
  }, []);
}

/** Mirror a local open to the host (marks read server-side). */
export function notifyOpen(id: number | string): void {
  try {
    void getNoriBridge()?.open(String(id));
  } catch { /* standalone */ }
}

/** Mirror a local star toggle to the host. */
export function notifyToggleStar(id: number | string): void {
  try {
    void getNoriBridge()?.toggleStar(String(id));
  } catch { /* standalone */ }
}

/** Mirror a local send to the host. */
export function notifySend(draft: { to: string; subject: string; body: string }): void {
  try {
    void getNoriBridge()?.send(draft);
  } catch { /* standalone */ }
}
