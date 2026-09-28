// Electron preload: exposes the versioned window.nori bridge (context-isolated).
// Port of the contract in web/src/bridge/noriBridge.ts, now implemented.
import { contextBridge, ipcRenderer } from "electron";
import type { DraftSeed, Email, EmailSummary, Mailbox, NoriEvent } from "./ipc.js";
import { NORI_PROTOCOL_VERSION } from "./ipc.js";

function invoke<T>(channel: string, ...args: unknown[]): Promise<T> {
  return ipcRenderer.invoke(channel, ...args) as Promise<T>;
}

const api = {
  protocolVersion: NORI_PROTOCOL_VERSION,
  list: (mailbox: Mailbox) => invoke<EmailSummary[]>("nori:list", mailbox),
  get: (id: string) => invoke<Email | null>("nori:get", id),
  open: (id: string) => invoke<void>("nori:open", id),
  toggleStar: (id: string) => invoke<boolean>("nori:toggleStar", id),
  search: (query: string) => invoke<EmailSummary[]>("nori:search", query),
  send: (draft: DraftSeed) => invoke<void>("nori:send", draft),
  signin: () => invoke<string>("nori:signin"),
  signout: () => invoke<void>("nori:signout"),
  sync: () => invoke<{ account: string | null }>("nori:sync"),
  fetchBody: (id: string) => invoke<string[]>("nori:fetchBody", id),
  counts: () => invoke<Record<Mailbox, number>>("nori:counts"),
  subscribe: (cb: (event: NoriEvent) => void) => {
    const listener = (_event: unknown, event: NoriEvent) => cb(event);
    (ipcRenderer as unknown as { on: (channel: string, listener: (event: unknown, data: NoriEvent) => void) => void }).on(
      "nori-event",
      listener,
    );
    return () => {
      (ipcRenderer as unknown as { removeListener: (channel: string, listener: unknown) => void }).removeListener(
        "nori-event",
        listener,
      );
    };
  },
};

contextBridge.exposeInMainWorld("nori", api);
export type NoriPreloadApi = typeof api;
